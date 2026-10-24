//! Generation-scoped system OCR and contour execution with reusable native owners.

use super::*;
use crate::platform::common::contour::input::Input as ContourInput;
use crate::platform::common::spatial_index::TargetSource;

pub(super) struct CapturedFrame {
    pub(super) geometry: CaptureGeometry,
    pub(super) capture: crate::platform::windows::native::PreparedCapture,
    pub(super) layout: SystemOcrLayout,
    _ledger: crate::support::perf_probe::ResourceGuard,
}

#[derive(Default)]
pub(super) struct CaptureResources {
    frames: SmallVec<[CapturedFrame; 2]>,
    bitmap: Option<BitmapResources>,
}
struct BitmapResources {
    factory: crate::platform::windows::native::SoftwareBitmapFactory,
    _apartment: crate::platform::windows::native::ComApartment,
}
impl BitmapResources {
    fn new() -> Result<Self, String> {
        let apartment = crate::platform::windows::native::ComApartment::initialise()?;
        let factory = crate::platform::windows::native::SoftwareBitmapFactory::load()?;
        Ok(Self {
            factory,
            _apartment: apartment,
        })
    }
}

pub(super) struct SystemOcrTile {
    pub(super) geometry: CaptureGeometry,
    pub(super) core_bounds: Rect,
    pub(super) bitmap: SharedSoftwareBitmap,
}

pub(super) enum SystemOcrInput {
    Begin { tile_count: usize },
    Tile(SystemOcrTile),
    CompletionWake,
    CancelWake,
    Failed(String),
    Done,
}

pub(super) struct SystemOcrSubmission {
    pub(super) sender: mpsc::SyncSender<SystemOcrInput>,
    pub(super) credits: mpsc::Receiver<()>,
    pub(super) max_in_flight: usize,
    pub(super) deadline: Instant,
}

pub(super) struct SystemOcrCompletion(AtomicU8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SystemOcrCompletionStatus {
    Completed,
    Canceled,
    Error,
    NonTerminal,
}

impl SystemOcrCompletion {
    pub(super) fn pending() -> Self {
        Self(AtomicU8::new(0))
    }
    pub(super) fn publish(&self, status: AsyncStatus) {
        let value = match status {
            AsyncStatus::Completed => 1,
            AsyncStatus::Canceled => 2,
            AsyncStatus::Error => 3,
            _ => 4,
        };
        self.0.store(value, Ordering::Release);
    }
    pub(super) fn take(&self) -> Option<SystemOcrCompletionStatus> {
        // The provider is the only consumer and each operation completes once.
        // Load first so scanning pending tiles does not issue a locked RMW for
        // the overwhelmingly common not-ready state.
        let value = self.0.load(Ordering::Acquire);
        if value == 0 {
            return None;
        }
        self.0.store(0, Ordering::Relaxed);
        match value {
            0 => None,
            1 => Some(SystemOcrCompletionStatus::Completed),
            2 => Some(SystemOcrCompletionStatus::Canceled),
            3 => Some(SystemOcrCompletionStatus::Error),
            _ => Some(SystemOcrCompletionStatus::NonTerminal),
        }
    }
}

pub(super) struct SharedSoftwareBitmap {
    pub(super) bitmap: Option<windows::Graphics::Imaging::SoftwareBitmap>,
    pub(super) _ledger: crate::support::perf_probe::ResourceGuard,
}

impl SharedSoftwareBitmap {
    fn bitmap(&self) -> Result<&windows::Graphics::Imaging::SoftwareBitmap, VisionError> {
        self.bitmap
            .as_ref()
            .ok_or_else(|| VisionError::Cleanup("OCR SoftwareBitmap was already closed".into()))
    }

    fn close(&mut self) -> Result<(), String> {
        let Some(bitmap) = self.bitmap.take() else {
            return Ok(());
        };
        bitmap
            .Close()
            .map_err(|error| format!("cannot close OCR SoftwareBitmap: {error}"))
    }
}

impl Drop for SharedSoftwareBitmap {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            crate::support::logging::report_error("windows-vision", error);
        }
    }
}

pub(super) fn run_scan(
    mut job: ScanJob,
    shared: &Arc<SharedQueue>,
    discovery: &DiscoveryHandle,
    resources: &mut CaptureResources,
) -> bool {
    let status = run_scan_inner(&mut job, shared, discovery, resources);
    crate::support::perf_probe::mark("vision_terminal_cleanup");
    let completed = status == UiScanStatus::Success;
    job.source.finish(status);
    completed
}

fn run_scan_inner(
    job: &mut ScanJob,
    shared: &Arc<SharedQueue>,
    discovery: &DiscoveryHandle,
    resources: &mut CaptureResources,
) -> UiScanStatus {
    if !generation_is_current(shared, job.generation) {
        return UiScanStatus::ContextChanged;
    }
    // A pending scan may have been submitted before the active scan failed to
    // stop its providers. Apply the same guard here before starting native work.
    if shared.provider_quarantine_nonempty.load(Ordering::Acquire) {
        return quarantined_scan_status(job.capture.take());
    }
    let _coordinator_ledger = crate::support::perf_probe::ResourceGuard::new(
        crate::support::perf_probe::ResourceKind::Coordinator,
    );
    let deadline = Instant::now()
        + Duration::from_millis(
            job.request
                .vision
                .request_timeout_ms
                .min(job.request.timeout_ms.max(250))
                .clamp(250, 30_000),
        );
    let bounds = job.request.scan_bounds();
    let geometries = match capture_geometries(&job.request, bounds) {
        Ok(geometries) => geometries,
        Err(error) => {
            let mut errors = crate::support::errors::ErrorBundle::default();
            errors.push("capture geometry", error);
            if let Some(capture) = job.capture.take() {
                errors.record("capture gate release", capture.release());
            }
            return UiScanStatus::Failed(
                errors
                    .into_result()
                    .err()
                    .unwrap_or_else(|| "capture geometry failed without details".into()),
            );
        }
    };
    let parallelism =
        std::thread::available_parallelism().map_or(MAX_SYSTEM_OCR_IN_FLIGHT, usize::from);
    let layouts: SmallVec<[_; 2]> = geometries
        .iter()
        .map(|geometry| {
            SystemOcrLayout::new(
                geometry.width,
                geometry.height,
                parallelism,
                MIN_SYSTEM_OCR_TILE_SIDE,
                MAX_SYSTEM_OCR_IN_FLIGHT,
            )
        })
        .collect();
    let max_in_flight = layouts
        .iter()
        .map(|layout| layout.tile_count)
        .sum::<usize>()
        .min(parallelism)
        .clamp(1, MAX_SYSTEM_OCR_IN_FLIGHT);
    let provider_mailbox = Arc::new(ProviderMailbox::with_output(job.source.sink()));
    let cancellation = ScanCancellation::new(
        shared,
        job.generation,
        &provider_mailbox,
        Arc::downgrade(&discovery.0),
    );
    {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.active_request_id == Some(job.request.id) {
            state.active_cancellation = Some(cancellation.signal());
        }
    }
    if !generation_is_current(shared, job.generation) {
        cancellation.cancel();
        return UiScanStatus::ContextChanged;
    }
    let (detect_text, detect_contours) =
        crate::platform::common::contour::sources(job.request.strategy, &job.request.vision);
    let discovery_snapshot = if detect_text {
        match discovery.wait(deadline, || cancellation.is_cancelled()) {
            Some(snapshot) => snapshot,
            None if cancellation.is_cancelled() => return UiScanStatus::ContextChanged,
            None => return UiScanStatus::TimedOut,
        }
    } else {
        Arc::new(OcrDiscoverySnapshot::default())
    };

    let system_descriptor = discovery_snapshot.system.clone().filter(|_| detect_text);
    use crate::platform::common::scan_progress::Progress;
    for (source, enabled) in [
        (TargetSource::SystemOcr, system_descriptor.is_some()),
        (TargetSource::Contour, detect_contours),
    ] {
        // All source plans precede the first native result. Unavailable OCR
        // sources explicitly declare no work; failed work remains conservative.
        if source != TargetSource::Contour && detect_text || enabled {
            provider_mailbox.progress(
                source,
                Progress::Plan(if enabled {
                    geometries
                        .iter()
                        .map(|geometry| geometry.desktop_bounds)
                        .collect()
                } else {
                    Vec::new()
                }),
            );
        }
    }
    if system_descriptor.is_some() {
        for (geometry, layout) in geometries.iter().zip(&layouts) {
            provider_mailbox.progress(
                TargetSource::SystemOcr,
                Progress::Refine {
                    region: geometry.desktop_bounds,
                    children: layout
                        .tiles(
                            geometry.width,
                            geometry.height,
                            SYSTEM_OCR_TILE_OVERLAP as usize,
                        )
                        .map(|tile| {
                            capture_pixel_rect(
                                *geometry,
                                tile.left_halo as u32,
                                tile.top_halo as u32,
                                (tile.right_halo - tile.left_halo) as u32,
                                (tile.bottom_halo - tile.top_halo) as u32,
                            )
                        })
                        .collect(),
                },
            );
        }
    }
    let mut providers = ProviderThreads::new(cancellation.clone(), shared);
    let mut system_input = None;
    let mut pending_ocr = 0usize;
    if let Some(descriptor) = system_descriptor {
        let (image_tx, image_rx) = mpsc::sync_channel(max_in_flight.saturating_add(2));
        let completion_tx = image_tx.clone();
        cancellation.register_system_wake(image_tx.clone());
        let (credit_tx, credit_rx) = mpsc::channel();
        let credit_tx = Arc::new(credit_tx);
        cancellation.register_credit_wake(&credit_tx);
        let result_mailbox = Arc::clone(&provider_mailbox);
        let provider_cancellation = cancellation.clone();
        if providers.spawn_reused(&shared.system_ocr, "keysteer-system-ocr", move |idle| {
            let _provider_ledger = crate::support::perf_probe::ResourceGuard::new(
                crate::support::perf_probe::ResourceKind::Provider,
            );
            crate::support::perf_probe::mark("system_ocr_started");
            let started = Instant::now();
            let result = recognize_system_provider(
                descriptor,
                image_rx,
                completion_tx,
                credit_tx,
                deadline,
                &provider_cancellation,
                &result_mailbox,
                started,
                idle,
                max_in_flight,
            );
            let _ = result_mailbox.publish(ProviderEvent::OcrDone {
                elapsed: started.elapsed(),
                result,
            });
            crate::support::perf_probe::mark("system_ocr_finished");
        }) {
            system_input = Some(SystemOcrSubmission {
                sender: image_tx,
                credits: credit_rx,
                max_in_flight,
                deadline,
            });
            pending_ocr += 1;
        }
    }
    let mut contour_pending = false;
    let mut early_events = ProviderEvents::new();

    let Some(mut capture_lease) = job.capture.take() else {
        return UiScanStatus::Failed("visual capture lease was not created".into());
    };
    // Geometry, the thread-bound DIB and the WinRT apartment are prepared
    // while the renderer is independently hiding its tree. No desktop pixels
    // are read before the hidden ACK.
    let mut frames = SmallVec::<[CapturedFrame; 2]>::new();
    resources.frames.retain(|old| {
        geometries.iter().any(|geometry| {
            old.geometry.width == geometry.width && old.geometry.height == geometry.height
        })
    });
    for (geometry, layout) in geometries.into_iter().zip(layouts) {
        if let Some(index) = resources.frames.iter().position(|old| {
            old.geometry.width == geometry.width && old.geometry.height == geometry.height
        }) {
            let mut frame = resources.frames.swap_remove(index);
            frame.geometry = geometry;
            frame.layout = layout;
            frames.push(frame);
            continue;
        }
        let capture =
            match crate::platform::windows::native::PreparedCapture::new(
                geometry.width,
                geometry.height,
            ) {
                Ok(capture) => capture,
                Err(error) => {
                    let mut errors = crate::support::errors::ErrorBundle::default();
                    errors.push("capture DIB preparation", error);
                    errors.record("capture gate release", capture_lease.release());
                    return UiScanStatus::Failed(errors.into_result().err().unwrap_or_else(|| {
                        "capture DIB preparation failed without details".into()
                    }));
                }
            };
        frames.push(CapturedFrame {
            geometry,
            capture,
            layout,
            _ledger: crate::support::perf_probe::ResourceGuard::new(
                crate::support::perf_probe::ResourceKind::GdiSurface,
            ),
        });
    }
    resources.frames.clear();
    crate::support::perf_probe::mark("capture_dib_prepared");
    let mut bitmap_factory_error = None;
    if system_input.is_some() && resources.bitmap.is_none() {
        match BitmapResources::new() {
            Ok(bitmap) => resources.bitmap = Some(bitmap),
            Err(error) => bitmap_factory_error = Some(error),
        }
    }
    let bitmap_factory = resources.bitmap.as_ref().map(|bitmap| &bitmap.factory);
    if let Err(error) =
        capture_lease.wait_hidden(deadline, || !generation_is_current(shared, job.generation))
    {
        let current = context_is_current(shared, job.generation, &job.request);
        if let Err(release_error) = capture_lease.release() {
            crate::support::logging::report_error("windows-overlay", release_error);
        }
        if !current {
            return UiScanStatus::ContextChanged;
        }
        if Instant::now() >= deadline {
            return UiScanStatus::TimedOut;
        }
        return UiScanStatus::Failed(error);
    }
    if !context_is_current(shared, job.generation, &job.request) {
        if let Err(error) = capture_lease.release() {
            crate::support::logging::report_error("windows-overlay", error);
        }
        return UiScanStatus::ContextChanged;
    }
    crate::support::perf_probe::mark("capture_hidden_ack");
    let mut capture_lease = Some(capture_lease);
    let mut context_changed_during_capture = false;
    let captured = (|| -> Result<(), String> {
        // Capture every real display before releasing the overlay gate. Native
        // DIB owners keep these pixels without copying or allocating the gaps
        // in a giant virtual desktop image. No OCR/contour wait holds this gate.
        for frame in &mut frames {
            if cancellation.is_cancelled() || Instant::now() >= deadline {
                return Err("display capture was interrupted".into());
            }
            let geometry = frame.geometry;
            let bounds = geometry.desktop_bounds;
            frame.capture.capture_with(
                bounds.x.floor() as i32,
                bounds.y.floor() as i32,
                bounds.width.ceil() as i32,
                bounds.height.ceil() as i32,
                |_, width, height| {
                    crate::support::perf_probe::mark("capture_gdi_ready");
                    if width != geometry.width || height != geometry.height {
                        return Err("native capture dimensions changed unexpectedly".into());
                    }
                    if !context_is_current(shared, job.generation, &job.request) {
                        context_changed_during_capture = true;
                        return Err("visual capture context changed".into());
                    }
                    Ok(())
                },
            )?;
        }
        if let Some(lease) = capture_lease.take() {
            lease.release()?;
        }
        // Fill the first OCR concurrency window before preparing contour
        // pixels. Grayscale overlaps in-flight recognition instead of
        // delaying those tiles; contour starts before the first credit wait.
        let contour_start_after = system_input.as_ref().map_or(0, |input| input.max_in_flight);
        let mut contour_attempted = false;
        let mut launch_contour = || -> Result<(), String> {
            if contour_attempted {
                return Ok(());
            }
            contour_attempted = true;
            if detect_contours {
                let mut images = SmallVec::<[ContourInput; 2]>::new();
                for frame in &frames {
                    if let Some(image) = contour_input_from_bgra_with_progress(
                        frame.capture.pixels(),
                        frame.geometry,
                        || Ok(cancellation.is_cancelled() || Instant::now() >= deadline),
                    )? {
                        images.push(image);
                    }
                }
                if images.is_empty() {
                    return Ok(());
                }
                let result_mailbox = Arc::clone(&provider_mailbox);
                let provider_cancellation = cancellation.clone();
                let limit = crate::platform::common::contour::candidate_limit(
                    job.request.strategy,
                    &job.request.vision,
                );
                contour_pending =
                    providers.spawn_reused(&shared.contour, "keysteer-contour", move |idle| {
                        let _ledger = crate::support::perf_probe::ResourceGuard::new(
                            crate::support::perf_probe::ResourceKind::Provider,
                        );
                        let (cached, expired) = idle.take(Instant::now());
                        drop(expired);
                        let mut workspace = cached.unwrap_or_default();
                        let mut completed = true;
                        for image in images {
                            if provider_cancellation.is_cancelled() || Instant::now() >= deadline {
                                completed = false;
                                break;
                            }
                            let targets = workspace.detect_prepared(&image, limit, || {
                                provider_cancellation.is_cancelled() || Instant::now() >= deadline
                            });
                            if send_contour_batch(&result_mailbox, targets).is_err() {
                                completed = false;
                                break;
                            }
                            result_mailbox.progress(
                                TargetSource::Contour,
                                Progress::Complete(image.desktop_bounds),
                            );
                        }
                        if completed
                            && !provider_cancellation.is_cancelled()
                            && Instant::now() < deadline
                        {
                            drop(idle.retain(workspace, Instant::now()));
                        }
                        let _ = result_mailbox.publish(ProviderEvent::ContourDone);
                    });
                if !contour_pending {
                    return Err("cannot start contour provider".into());
                }
            }
            Ok(())
        };
        if let Some(input) = system_input.take() {
            if let Some(factory) = bitmap_factory.as_ref() {
                if let Err(error) = stream_system_ocr_tiles(&frames, factory, &input, |index| {
                    if index.is_some_and(|index| index + 1 == contour_start_after) {
                        launch_contour()?;
                    }
                    if !drain_early_ocr_events(
                        &provider_mailbox,
                        &job.source,
                        &mut early_events,
                        || context_is_current(shared, job.generation, &job.request),
                    ) {
                        context_changed_during_capture = true;
                        return Err("visual capture context changed".into());
                    }
                    Ok(())
                }) {
                    if context_changed_during_capture
                        || cancellation.is_cancelled()
                        || Instant::now() >= deadline
                    {
                        return Err(error);
                    }
                    let _ = input.sender.send(SystemOcrInput::Failed(error));
                    let _ = input.sender.send(SystemOcrInput::Done);
                }
            } else {
                let _ = input.sender.send(SystemOcrInput::Failed(
                    bitmap_factory_error.clone().unwrap_or_else(|| {
                        "cannot create the COM apartment for system OCR tiles".into()
                    }),
                ));
                let _ = input.sender.send(SystemOcrInput::Done);
            }
        }
        launch_contour()?;
        if !drain_early_ocr_events(&provider_mailbox, &job.source, &mut early_events, || {
            context_is_current(shared, job.generation, &job.request)
        }) {
            context_changed_during_capture = true;
            return Err("visual capture context changed".into());
        }
        Ok(())
    })();
    resources.frames = frames;
    if let Some(lease) = capture_lease.take()
        && let Err(error) = lease.release()
    {
        crate::support::logging::report_error("windows-overlay", error);
    }
    match captured {
        Ok(()) => {}
        Err(_) if context_changed_during_capture || cancellation.is_cancelled() => {
            return UiScanStatus::ContextChanged;
        }
        Err(_) if Instant::now() >= deadline => return UiScanStatus::TimedOut,
        Err(error) => return UiScanStatus::Failed(error),
    }
    if !context_is_current(shared, job.generation, &job.request) {
        return UiScanStatus::ContextChanged;
    }
    drop(system_input);
    let mut contour_done = !contour_pending;
    let mut timed_out = false;
    let mut context_changed = false;
    let mut cleanup_errors = crate::support::errors::ErrorBundle::default();
    while pending_ocr != 0 || !contour_done {
        if !generation_is_current(shared, job.generation) {
            cancellation.cancel();
            context_changed = true;
            break;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            cancellation.cancel();
            break;
        }
        let mut events = std::mem::take(&mut early_events);
        if events.is_empty() {
            match provider_mailbox.wait_until_ready(deadline) {
                Ok(()) => {}
                Err(VisionError::TimedOut) => {
                    timed_out = true;
                    cancellation.cancel();
                    break;
                }
                Err(VisionError::Cancelled) if cancellation.is_cancelled() => break,
                Err(error) => {
                    cleanup_errors.push("provider mailbox", error.to_string());
                    break;
                }
            }
        }
        provider_mailbox.drain_into(&mut events);
        let mut ocr_ready: SmallVec<[ReadyOcrBatch; 2]> = SmallVec::new();
        let mut ocr_done: SmallVec<[CompletedOcr; 2]> = SmallVec::new();
        for event in events {
            match event {
                ProviderEvent::ContourBatch(targets) => {
                    if context_is_current(shared, job.generation, &job.request) {
                        job.source.push_from(TargetSource::Contour, targets);
                    } else {
                        cancellation.cancel();
                        context_changed = true;
                    }
                }
                ProviderEvent::ContourDone => contour_done = true,
                ProviderEvent::OcrBatch { elapsed, targets } => ocr_ready.push((elapsed, targets)),
                ProviderEvent::OcrDone { elapsed, result } => ocr_done.push((elapsed, result)),
            }
        }
        ocr_ready.sort_by_key(|batch| batch.0);
        for (_elapsed, targets) in ocr_ready {
            if !context_is_current(shared, job.generation, &job.request) {
                cancellation.cancel();
                context_changed = true;
                break;
            }
            let count = targets.len();
            let source = TargetSource::SystemOcr;
            let accepted = job.source.push_from(source, targets);
            if accepted != 0 {
                crate::support::perf_probe::mark("vision_targets_accepted");
            }
            crate::log_info!(
                "windows-vision",
                "system OCR streamed {count} valid targets ({accepted} new)"
            );
        }
        for (elapsed, result) in ocr_done {
            pending_ocr = pending_ocr.saturating_sub(1);
            match result {
                Ok(count) => {
                    crate::log_info!(
                        "windows-vision",
                        "system OCR completed in {elapsed:?} with {count} valid targets"
                    );
                }
                Err(error) if error.is_control_flow() => {}
                Err(error @ VisionError::Unavailable(_)) => crate::report_warning!(
                    "windows-vision",
                    "system OCR failed after {elapsed:?}: {error}"
                ),
                Err(error) => {
                    if matches!(error, VisionError::Cleanup(_)) {
                        cleanup_errors.push(
                            format!("system OCR cleanup after {elapsed:?}"),
                            error.to_string(),
                        );
                    } else {
                        // Operational provider failures can be masked by the
                        // Rust contour, so this coordinator
                        // is their final reporting boundary.
                        crate::report_error!(
                            "windows-vision",
                            "system OCR failed after {elapsed:?}: {error}"
                        );
                    }
                }
            }
        }
    }

    cleanup_errors.record(
        "provider join",
        providers.join_all(Instant::now() + PROVIDER_STOP_TIMEOUT),
    );
    let cleanup_result = cleanup_errors.into_result();
    if context_changed || !context_is_current(shared, job.generation, &job.request) {
        if let Err(error) = cleanup_result {
            crate::support::logging::report_error("windows-vision", error);
        }
        UiScanStatus::ContextChanged
    } else if timed_out || Instant::now() >= deadline {
        if let Err(error) = cleanup_result {
            crate::support::logging::report_error("windows-vision", error);
        }
        UiScanStatus::TimedOut
    } else if let Err(error) = cleanup_result {
        UiScanStatus::Failed(error)
    } else {
        UiScanStatus::Success
    }
}

#[allow(clippy::too_many_arguments)]
fn recognize_system_provider(
    descriptor: Arc<SystemOcrDescriptor>,
    receiver: mpsc::Receiver<SystemOcrInput>,
    completion_sender: mpsc::SyncSender<SystemOcrInput>,
    credit_sender: Arc<mpsc::Sender<()>>,
    deadline: Instant,
    cancellation: &ScanCancellation,
    result_mailbox: &ProviderMailbox,
    started: Instant,
    idle: &mut crate::platform::common::scan_reuse::IdleCache<SystemOcrResources>,
    max_in_flight: usize,
) -> Result<usize, VisionError> {
    if cancellation.is_cancelled() {
        return Err(VisionError::Cancelled);
    }
    let (cached, expired) = idle.take(Instant::now());
    drop(expired);
    let mut resources = match cached {
        Some(resources) => resources,
        None => SystemOcrResources::new()?,
    };
    resources
        .engines
        .truncate(max_in_flight.clamp(1, MAX_SYSTEM_OCR_IN_FLIGHT));
    let result = recognize_system_provider_inner(
        descriptor,
        receiver,
        completion_sender,
        credit_sender,
        deadline,
        cancellation,
        result_mailbox,
        started,
        &mut resources,
    );
    if result.is_ok() && !cancellation.is_cancelled() && Instant::now() < deadline {
        drop(idle.retain(resources, Instant::now()));
    }
    result
}

pub(super) struct SystemOcrResources {
    engines: Vec<OcrEngine>,
    factory: crate::platform::windows::native::SystemOcrFactory,
    // Drop engines/factory before releasing this thread's apartment.
    _apartment: crate::platform::windows::native::ComApartment,
}

impl SystemOcrResources {
    fn new() -> Result<Self, VisionError> {
        let apartment = crate::platform::windows::native::ComApartment::initialise()
            .map_err(VisionError::Operational)?;
        let factory = crate::platform::windows::native::SystemOcrFactory::load()
            .map_err(VisionError::Unavailable)?;
        let engine = factory.create_engine().map_err(VisionError::Unavailable)?;
        Ok(Self {
            engines: vec![engine],
            factory,
            _apartment: apartment,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn recognize_system_provider_inner(
    descriptor: Arc<SystemOcrDescriptor>,
    receiver: mpsc::Receiver<SystemOcrInput>,
    completion_sender: mpsc::SyncSender<SystemOcrInput>,
    credit_sender: Arc<mpsc::Sender<()>>,
    deadline: Instant,
    cancellation: &ScanCancellation,
    result_mailbox: &ProviderMailbox,
    started: Instant,
    resources: &mut SystemOcrResources,
) -> Result<usize, VisionError> {
    // Preserve the existing overlap between OCR cold start and the overlay
    // hide/capture barrier. The remaining engines are created as tiles arrive.
    let mut expected_tiles = None;
    let mut active: Vec<Option<ActiveSystemOcrTile>> = Vec::new();
    let mut pending = 0usize;
    let mut accepted = 0usize;
    let mut operational_failures = Vec::new();
    let mut cleanup_failures = Vec::new();
    loop {
        if cancellation.is_cancelled() {
            cancel_active_system_tiles(&mut active, &receiver, deadline, &mut cleanup_failures);
            return if cleanup_failures.is_empty() {
                Err(VisionError::Cancelled)
            } else {
                Err(VisionError::Cleanup(cleanup_failures.join("; ")))
            };
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            cancel_active_system_tiles(&mut active, &receiver, deadline, &mut cleanup_failures);
            return if cleanup_failures.is_empty() {
                Err(VisionError::TimedOut)
            } else {
                Err(VisionError::Cleanup(cleanup_failures.join("; ")))
            };
        }
        let input = match receiver.recv_timeout(remaining) {
            Ok(input) => input,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancel_active_system_tiles(&mut active, &receiver, deadline, &mut cleanup_failures);
                return if cleanup_failures.is_empty() {
                    Err(VisionError::TimedOut)
                } else {
                    Err(VisionError::Cleanup(cleanup_failures.join("; ")))
                };
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                operational_failures.push("system OCR tile stream disconnected".into());
                break;
            }
        };
        match input {
            SystemOcrInput::Begin { tile_count } => {
                if expected_tiles.replace(tile_count).is_some() {
                    operational_failures.push("system OCR received a duplicate tile header".into());
                    continue;
                }
                active.reserve(tile_count);
            }
            SystemOcrInput::Tile(mut tile) => {
                if expected_tiles.is_none() {
                    operational_failures
                        .push("system OCR received a tile before its header".into());
                    if let Err(error) = tile.bitmap.close() {
                        cleanup_failures.push(error);
                    }
                    let _ = credit_sender.send(());
                    continue;
                }
                let index = active.len();
                if tile.geometry.width > descriptor.maximum_dimension
                    || tile.geometry.height > descriptor.maximum_dimension
                {
                    operational_failures.push(format!(
                        "system OCR tile {}x{} exceeds the OCR limit {}",
                        tile.geometry.width, tile.geometry.height, descriptor.maximum_dimension
                    ));
                    if let Err(error) = tile.bitmap.close() {
                        cleanup_failures.push(error);
                    }
                    active.push(None);
                    let _ = credit_sender.send(());
                    continue;
                }
                let engine = match resources
                    .engines
                    .pop()
                    .map_or_else(|| resources.factory.create_engine(), Ok)
                {
                    Ok(engine) => engine,
                    Err(error) => {
                        operational_failures.push(error);
                        if let Err(error) = tile.bitmap.close() {
                            cleanup_failures.push(error);
                        }
                        active.push(None);
                        let _ = credit_sender.send(());
                        continue;
                    }
                };
                let (operation, completion) = match tile.bitmap.bitmap().and_then(|bitmap| {
                    OcrOperationGuard::start_notified(
                        &engine,
                        bitmap,
                        index,
                        completion_sender.clone(),
                    )
                    .map_err(VisionError::Operational)
                }) {
                    Ok(operation) => operation,
                    Err(error) => {
                        operational_failures.push(error.to_string());
                        if let Err(error) = tile.bitmap.close() {
                            cleanup_failures.push(error);
                        }
                        active.push(None);
                        let _ = credit_sender.send(());
                        continue;
                    }
                };
                active.push(Some(ActiveSystemOcrTile {
                    engine,
                    operation,
                    completion,
                    tile,
                }));
                pending += 1;
            }
            SystemOcrInput::CompletionWake | SystemOcrInput::CancelWake => {}
            SystemOcrInput::Failed(error) => operational_failures.push(error),
            SystemOcrInput::Done => break,
        }
        drain_completed_system_tiles(
            &mut active,
            &mut pending,
            &mut accepted,
            result_mailbox,
            started,
            &credit_sender,
            &mut operational_failures,
            &mut cleanup_failures,
            resources,
        )?;
    }
    if let Some(expected) = expected_tiles
        && active.len() != expected
    {
        operational_failures.push(format!(
            "system OCR expected {expected} tiles but received {}",
            active.len()
        ));
    }

    while pending != 0 {
        drain_completed_system_tiles(
            &mut active,
            &mut pending,
            &mut accepted,
            result_mailbox,
            started,
            &credit_sender,
            &mut operational_failures,
            &mut cleanup_failures,
            resources,
        )?;
        if pending == 0 {
            break;
        }
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let completion = receiver.recv_timeout(remaining);
        match completion {
            Ok(SystemOcrInput::CompletionWake | SystemOcrInput::CancelWake) => {}
            Ok(SystemOcrInput::Failed(error)) => {
                operational_failures.push(error);
            }
            Ok(SystemOcrInput::Begin { .. } | SystemOcrInput::Tile(_) | SystemOcrInput::Done) => {
                operational_failures.push("system OCR received input after tile stream end".into());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                operational_failures.push("system OCR completion channel disconnected".into());
                break;
            }
        };
    }
    cancel_active_system_tiles(&mut active, &receiver, deadline, &mut cleanup_failures);
    if !cleanup_failures.is_empty() {
        Err(VisionError::Cleanup(cleanup_failures.join("; ")))
    } else if cancellation.is_cancelled() {
        Err(VisionError::Cancelled)
    } else if Instant::now() >= deadline {
        Err(VisionError::TimedOut)
    } else if !operational_failures.is_empty() {
        Err(VisionError::Operational(operational_failures.join("; ")))
    } else {
        Ok(accepted)
    }
}

struct ActiveSystemOcrTile {
    engine: OcrEngine,
    operation: OcrOperationGuard,
    completion: Arc<SystemOcrCompletion>,
    tile: SystemOcrTile,
}

#[allow(clippy::too_many_arguments)]
fn drain_completed_system_tiles(
    active: &mut [Option<ActiveSystemOcrTile>],
    pending: &mut usize,
    accepted: &mut usize,
    result_mailbox: &ProviderMailbox,
    started: Instant,
    credit_sender: &mpsc::Sender<()>,
    operational_failures: &mut Vec<String>,
    cleanup_failures: &mut Vec<String>,
    resources: &mut SystemOcrResources,
) -> Result<(), VisionError> {
    for index in 0..active.len() {
        let Some(status) = active[index]
            .as_ref()
            .and_then(|tile| tile.completion.take())
        else {
            continue;
        };
        complete_system_tile(
            (index, status),
            active,
            pending,
            accepted,
            result_mailbox,
            started,
            credit_sender,
            operational_failures,
            cleanup_failures,
            resources,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn complete_system_tile(
    (index, status): (usize, SystemOcrCompletionStatus),
    active: &mut [Option<ActiveSystemOcrTile>],
    pending: &mut usize,
    accepted: &mut usize,
    result_mailbox: &ProviderMailbox,
    started: Instant,
    credit_sender: &mpsc::Sender<()>,
    operational_failures: &mut Vec<String>,
    cleanup_failures: &mut Vec<String>,
    resources: &mut SystemOcrResources,
) -> Result<(), VisionError> {
    let Some(slot) = active.get_mut(index) else {
        operational_failures.push(format!(
            "system OCR returned an invalid tile completion index {index}"
        ));
        return Ok(());
    };
    let Some(mut completed) = slot.take() else {
        operational_failures.push(format!(
            "system OCR returned a duplicate tile completion index {index}"
        ));
        return Ok(());
    };
    *pending = pending.saturating_sub(1);
    let failures_before = (operational_failures.len(), cleanup_failures.len());
    let mut fatal = None;
    match completed.operation.complete(status) {
        Ok(result) => match stream_system_targets_from_result(
            &result,
            completed.tile.geometry,
            completed.tile.core_bounds,
            usize::MAX.saturating_sub(*accepted),
            result_mailbox,
            started,
        ) {
            Ok(count) => *accepted += count,
            Err(VisionError::Operational(error)) => operational_failures.push(error),
            Err(error) => fatal = Some(error),
        },
        Err(error) => operational_failures.push(format!("system OCR tile {index}: {error}")),
    }
    if let Err(error) = completed.tile.bitmap.close() {
        cleanup_failures.push(error);
    }
    if fatal.is_none()
        && failures_before == (operational_failures.len(), cleanup_failures.len())
        && resources.engines.len() < MAX_SYSTEM_OCR_IN_FLIGHT
    {
        resources.engines.push(completed.engine);
    }
    result_mailbox.progress(
        crate::platform::common::spatial_index::TargetSource::SystemOcr,
        crate::platform::common::scan_progress::Progress::Complete(
            completed.tile.geometry.desktop_bounds,
        ),
    );
    let _ = credit_sender.send(());
    fatal.map_or(Ok(()), Err)
}

fn cancel_active_system_tiles(
    active: &mut [Option<ActiveSystemOcrTile>],
    completions: &mpsc::Receiver<SystemOcrInput>,
    deadline: Instant,
    cleanup_failures: &mut Vec<String>,
) {
    let mut remaining = 0usize;
    for tile in active.iter().flatten() {
        remaining += 1;
        if let Err(error) = tile.operation.request_cancel() {
            cleanup_failures.push(error);
        }
    }
    while remaining != 0 {
        for slot in active.iter_mut() {
            let Some(status) = slot.as_ref().and_then(|tile| tile.completion.take()) else {
                continue;
            };
            let Some(mut tile) = slot.take() else {
                continue;
            };
            remaining = remaining.saturating_sub(1);
            let close = match status {
                SystemOcrCompletionStatus::Completed => tile.operation.complete(status).map(drop),
                SystemOcrCompletionStatus::Canceled | SystemOcrCompletionStatus::Error => {
                    tile.operation.close_terminal()
                }
                SystemOcrCompletionStatus::NonTerminal => retain_nonterminal_system_ocr_owner(
                    "system OCR completion callback reported a non-terminal status during cancellation",
                ),
            };
            if let Err(error) = close {
                cleanup_failures.push(error);
            }
            if let Err(error) = tile.tile.bitmap.close() {
                cleanup_failures.push(error);
            }
        }
        if remaining == 0 {
            break;
        }
        let completion = wait_system_ocr_cancellation_event(completions, deadline);
        match completion {
            Ok(SystemOcrInput::CompletionWake | SystemOcrInput::CancelWake) => {}
            Ok(_) => continue,
            Err(_) => retain_nonterminal_system_ocr_owner(
                "system OCR completion channel closed while an operation was still active",
            ),
        }
    }
}

pub(super) fn wait_system_ocr_cancellation_event(
    completions: &mpsc::Receiver<SystemOcrInput>,
    deadline: Instant,
) -> Result<SystemOcrInput, mpsc::RecvError> {
    let remaining_time = deadline.saturating_duration_since(Instant::now());
    if remaining_time.is_zero() {
        // Keep the complete tile owner (operation, engine, bitmap and this COM
        // apartment) on the provider thread until WinRT reports a terminal
        // state. The coordinator has its own bounded join; if a native
        // operation never calls back, that WorkerJoin moves to the explicit
        // provider quarantine instead of dropping live objects.
        return completions.recv();
    }
    match completions.recv_timeout(remaining_time) {
        Ok(input) => Ok(input),
        Err(mpsc::RecvTimeoutError::Timeout) => completions.recv(),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(mpsc::RecvError),
    }
}

#[cold]
#[inline(never)]
pub(super) fn retain_nonterminal_system_ocr_owner(reason: &str) -> ! {
    crate::support::logging::report_error(
        "windows-vision",
        format!("{reason}; retaining its complete provider owner in quarantine"),
    );
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::*;
    use crate::platform::common::scan_reuse::IdleCache;
    use windows::core::Interface;

    #[test]
    #[ignore = "requires an installed Windows OCR language pack"]
    fn native_ocr_reuses_engine_without_old_text_and_discards_failed_resources()
    -> Result<(), String> {
        let _apartment = crate::platform::windows::native::ComApartment::initialise()?;
        let factory = crate::platform::windows::native::SoftwareBitmapFactory::load()?;
        let (maximum_dimension, languages) =
            crate::platform::windows::native::probe_system_ocr_factory()?;
        let descriptor = Arc::new(SystemOcrDescriptor {
            maximum_dimension,
            languages,
        });
        let mut idle = IdleCache::default();
        idle.set_limit(1, Instant::now());
        let geometry = capture_geometry(Rect::new(0.0, 0.0, 300.0, 100.0))?;
        let blank = vec![255u8; 300 * 100 * 4];
        let mut text = blank.clone();
        // Block lettering in memory avoids desktop capture and fixture files.
        let hello = [
            [17, 17, 17, 31, 17, 17, 17],
            [31, 16, 16, 30, 16, 16, 31],
            [16, 16, 16, 16, 16, 16, 31],
            [16, 16, 16, 16, 16, 16, 31],
            [14, 17, 17, 17, 17, 17, 14],
        ];
        for (letter, rows) in hello.iter().enumerate() {
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) == 0 {
                        continue;
                    }
                    for y in 0..6 {
                        for x in 0..6 {
                            let offset =
                                ((24 + row * 6 + y) * 300 + 24 + letter * 42 + col * 6 + x) * 4;
                            text[offset..offset + 3].fill(0);
                        }
                    }
                }
            }
        }
        let shared = Arc::new(SharedQueue::default());
        let mut identity = None;
        for (generation, pixels) in [text.as_slice(), text.as_slice(), blank.as_slice()]
            .into_iter()
            .enumerate()
        {
            let generation = generation as u64 + 1;
            shared
                .latest_generation
                .store(generation, Ordering::Release);
            let mailbox = Arc::new(ProviderMailbox::new());
            let cancellation = ScanCancellation::new(&shared, generation, &mailbox, Weak::new());
            let (sender, receiver) = mpsc::sync_channel(4);
            let (credits, _) = mpsc::channel();
            sender
                .send(SystemOcrInput::Begin { tile_count: 1 })
                .unwrap();
            sender
                .send(SystemOcrInput::Tile(SystemOcrTile {
                    geometry,
                    core_bounds: geometry.desktop_bounds,
                    bitmap: SharedSoftwareBitmap {
                        bitmap: Some(factory.bgra_region(pixels, 300, 100, 0, 0, 300, 100)?),
                        _ledger: crate::support::perf_probe::ResourceGuard::new(
                            crate::support::perf_probe::ResourceKind::Bitmap,
                        ),
                    },
                }))
                .unwrap();
            sender.send(SystemOcrInput::Done).unwrap();
            let started = Instant::now();
            let count = recognize_system_provider(
                Arc::clone(&descriptor),
                receiver,
                sender,
                Arc::new(credits),
                started + Duration::from_secs(20),
                &cancellation,
                &mailbox,
                started,
                &mut idle,
                1,
            )
            .map_err(|e| e.to_string())?;
            let mut events = ProviderEvents::new();
            mailbox.drain_into(&mut events);
            let names: Vec<_> = events
                .into_iter()
                .filter_map(|event| match event {
                    ProviderEvent::OcrBatch { targets, .. } => Some(targets),
                    _ => None,
                })
                .flatten()
                .map(|target| target.name)
                .collect();
            println!(
                "native_ocr_reuse generation={generation} elapsed_ms={:.3} targets={count} text={names:?}",
                started.elapsed().as_secs_f64() * 1000.0
            );
            if generation < 3 {
                assert!(
                    count > 0
                        && names
                            .iter()
                            .any(|name| name.to_uppercase().contains("HELLO"))
                );
            } else {
                assert_eq!(count, 0);
                assert!(names.is_empty(), "blank scan retained old text");
            }
            let (owner, expired) = idle.take(Instant::now());
            assert!(expired.is_empty());
            let owner = owner.expect("successful native OCR should retain its engine");
            assert_eq!(owner.engines.len(), 1);
            let current = owner.engines[0].as_raw();
            assert_eq!(*identity.get_or_insert(current), current);
            assert!(idle.retain(owner, Instant::now()).is_none());
        }
        let mailbox = Arc::new(ProviderMailbox::new());
        let cancellation = ScanCancellation::new(&shared, 3, &mailbox, Weak::new());
        let (sender, receiver) = mpsc::sync_channel(4);
        let (credits, _) = mpsc::channel();
        sender
            .send(SystemOcrInput::Failed("test capture failure".into()))
            .unwrap();
        sender.send(SystemOcrInput::Done).unwrap();
        let started = Instant::now();
        assert!(
            recognize_system_provider(
                descriptor,
                receiver,
                sender,
                Arc::new(credits),
                started + Duration::from_secs(5),
                &cancellation,
                &mailbox,
                started,
                &mut idle,
                1,
            )
            .is_err()
        );
        assert!(idle.next_expiration().is_none());
        Ok(())
    }
}
