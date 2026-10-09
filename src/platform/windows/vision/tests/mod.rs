#![cfg(test)]

use super::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;

#[test]
fn ocr_execution_kind_uses_only_discovered_providers() {
    assert_eq!(
        ocr_execution_kind(false, false, true),
        OcrExecutionKind::None
    );
    assert_eq!(
        ocr_execution_kind(true, false, true),
        OcrExecutionKind::SystemOnly
    );
    assert_eq!(
        ocr_execution_kind(false, true, true),
        OcrExecutionKind::WechatOnly
    );
    assert_eq!(ocr_execution_kind(true, true, true), OcrExecutionKind::Dual);
    assert_eq!(
        ocr_execution_kind(true, true, false),
        OcrExecutionKind::None
    );
}

#[test]
fn maps_scaled_negative_desktop_coordinates() {
    let image = CaptureGeometry {
        width: 500,
        height: 300,
        desktop_bounds: Rect::new(-1_000.0, -200.0, 2_000.0, 1_200.0),
        scale: 0.5,
    };
    assert_eq!(
        image_to_desktop(image, Rect::new(25.0, 10.0, 50.0, 20.0)),
        Rect::new(-950.0, -180.0, 100.0, 40.0)
    );
}

#[test]
fn standard_uhd_capture_keeps_native_bitblt_dimensions() {
    let geometry = capture_geometry(Rect::new(0.0, 0.0, 3_840.0, 2_160.0)).unwrap();
    assert_eq!(geometry.width, 3_840);
    assert_eq!(geometry.height, 2_160);
    assert_eq!(geometry.scale, 1.0);
}

#[test]
fn system_ocr_grid_uses_the_next_cpu_square_and_a_64px_size_floor() {
    assert_eq!(system_ocr_grid_for_parallelism(3_840, 2_160, 16), 5);
    assert_eq!(system_ocr_grid_for_parallelism(7_680, 4_320, 64), 9);
    assert_eq!(system_ocr_grid_for_parallelism(320, 200, 16), 3);
    assert_eq!(system_ocr_grid_for_parallelism(200, 100, 16), 1);
}

#[test]
fn system_ocr_credit_limit_covers_production_tile_counts() {
    for count in [1, 2, 4, 9, 25, 81] {
        let limit = system_ocr_concurrency_for(count, 64);
        assert_eq!(limit, count.min(MAX_SYSTEM_OCR_IN_FLIGHT));
    }
    assert_eq!(system_ocr_concurrency_for(25, 2), 2);
    assert_eq!(system_ocr_concurrency_for(25, 4), 4);
}

#[test]
fn completion_state_survives_a_full_coalesced_wake_lane() {
    let (sender, _receiver) = mpsc::sync_channel(1);
    sender
        .send(SystemOcrInput::Begin { tile_count: 1 })
        .unwrap();
    let completion = SystemOcrCompletion::pending();
    completion.publish(AsyncStatus::Completed);
    assert!(sender.try_send(SystemOcrInput::CompletionWake).is_err());
    assert_eq!(
        completion.take(),
        Some(SystemOcrCompletionStatus::Completed)
    );
    assert_eq!(completion.take(), None);
}

#[test]
fn completion_callback_preserves_nonterminal_status_for_quarantine() {
    let completion = SystemOcrCompletion::pending();
    completion.publish(AsyncStatus::Started);
    assert_eq!(
        completion.take(),
        Some(SystemOcrCompletionStatus::NonTerminal)
    );
    assert_eq!(completion.take(), None);
}

#[test]
fn system_ocr_core_tiles_cover_pixels_once_and_never_drop_below_64px() {
    let width = 3_841;
    let height = 2_161;
    let grid = system_ocr_grid_for_parallelism(width, height, 16);
    let mut previous_bottom = 0;
    for row in 0..grid {
        let mut previous_right = 0;
        for column in 0..grid {
            let left = scaled_partition(width, column, grid);
            let right = scaled_partition(width, column + 1, grid);
            let top = scaled_partition(height, row, grid);
            let bottom = scaled_partition(height, row + 1, grid);
            assert_eq!(left, previous_right);
            assert_eq!(top, previous_bottom);
            assert!(right - left >= MIN_SYSTEM_OCR_TILE_SIDE);
            assert!(bottom - top >= MIN_SYSTEM_OCR_TILE_SIDE);
            previous_right = right;
        }
        assert_eq!(previous_right, width);
        previous_bottom = scaled_partition(height, row + 1, grid);
    }
    assert_eq!(previous_bottom, height);
}

#[test]
fn overlapping_system_ocr_tiles_map_a_seam_target_to_one_desktop_owner() {
    let parent = CaptureGeometry {
        width: 1_000,
        height: 500,
        desktop_bounds: Rect::new(-1_000.0, -200.0, 2_000.0, 1_000.0),
        scale: 0.5,
    };
    let seam = 500;
    let left_bitmap_right = seam + SYSTEM_OCR_TILE_OVERLAP;
    let right_bitmap_x = seam - SYSTEM_OCR_TILE_OVERLAP;
    let left_geometry = CaptureGeometry {
        width: left_bitmap_right,
        height: parent.height,
        desktop_bounds: capture_pixel_rect(parent, 0, 0, left_bitmap_right, parent.height),
        scale: parent.scale,
    };
    let right_geometry = CaptureGeometry {
        width: parent.width - right_bitmap_x,
        height: parent.height,
        desktop_bounds: capture_pixel_rect(
            parent,
            right_bitmap_x,
            0,
            parent.width - right_bitmap_x,
            parent.height,
        ),
        scale: parent.scale,
    };
    let global_pixel_rect = Rect::new(490.0, 120.0, 20.0, 12.0);
    let from_left = image_to_desktop(left_geometry, global_pixel_rect);
    let from_right = image_to_desktop(
        right_geometry,
        Rect::new(
            global_pixel_rect.x - f64::from(right_bitmap_x),
            global_pixel_rect.y,
            global_pixel_rect.width,
            global_pixel_rect.height,
        ),
    );
    assert_eq!(from_left, from_right);

    let left_core = capture_pixel_rect(parent, 0, 0, seam, parent.height);
    let right_core = capture_pixel_rect(parent, seam, 0, parent.width - seam, parent.height);
    let center = from_left.center();
    assert!(!left_core.contains(&center));
    assert!(right_core.contains(&center));
}

#[test]
fn division_free_nearest_offsets_match_reference_mapping() {
    for (source, destination) in [
        (3usize, 2usize),
        (17, 7),
        (100, 100),
        (3_840, 2_560),
        (2_160, 1_440),
    ] {
        assert_eq!(
            nearest_offsets(source, destination),
            (0..destination)
                .map(|output| output * source / destination)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn provider_batches_coalesce_in_the_fixed_mailbox_slot() {
    let mailbox = ProviderMailbox::new();
    let targets = (0..49)
        .map(|index| UiTarget {
            details: None,
            rect: Rect::new(index as f64, 0.0, 4.0, 4.0),
            name: index.to_string(),
            role: SemanticRole::StaticText,
        })
        .collect();
    assert_eq!(
        send_ocr_batches(&mailbox, "system", Instant::now(), targets)
            .expect("test mailbox is live"),
        49
    );
    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    let sizes = events
        .into_iter()
        .map(|event| match event {
            ProviderEvent::OcrBatch { targets, .. } => targets.len(),
            _ => 0,
        })
        .collect::<Vec<_>>();
    assert_eq!(sizes, [49]);
}

#[test]
fn provider_mailbox_preserves_a_full_generation_without_blocking() {
    let mailbox = ProviderMailbox::new();
    let targets = |offset: usize| {
        (0..MAX_OCR_TARGETS)
            .map(|index| UiTarget {
                details: None,
                rect: Rect::new((offset + index) as f64, 0.0, 4.0, 4.0),
                name: index.to_string(),
                role: SemanticRole::StaticText,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        send_ocr_batches(&mailbox, "system", Instant::now(), targets(0)).unwrap(),
        MAX_OCR_TARGETS
    );
    mailbox
        .publish(ProviderEvent::OcrDone {
            provider: "system",
            elapsed: Duration::ZERO,
            result: Ok(MAX_OCR_TARGETS),
        })
        .unwrap();
    assert_eq!(
        send_ocr_batches(&mailbox, "wechat", Instant::now(), targets(MAX_OCR_TARGETS),).unwrap(),
        MAX_OCR_TARGETS
    );
    mailbox
        .publish(ProviderEvent::OcrDone {
            provider: "wechat",
            elapsed: Duration::ZERO,
            result: Ok(MAX_OCR_TARGETS),
        })
        .unwrap();
    send_contour_batches(&mailbox, targets(MAX_OCR_TARGETS * 2)).unwrap();

    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    assert_eq!(events.len(), 6);
    let mut target_count = 0;
    let mut terminal_count = 0;
    for event in events {
        match event {
            ProviderEvent::OcrBatch { targets, .. } | ProviderEvent::ContourBatch(targets) => {
                target_count += targets.len()
            }
            ProviderEvent::OcrDone { .. } | ProviderEvent::ContourDone => {
                terminal_count += 1;
            }
        }
    }
    assert_eq!(target_count, MAX_OCR_TARGETS * 3);
    assert_eq!(terminal_count, 3);
}

#[test]
fn provider_mailbox_close_wakes_and_rejects_late_results() {
    let mailbox = ProviderMailbox::new();
    mailbox.close();
    assert!(matches!(
        mailbox.wait_until_ready(Instant::now() + Duration::from_secs(1)),
        Err(VisionError::Cancelled)
    ));
    assert!(matches!(
        mailbox.publish(ProviderEvent::ContourDone),
        Err(VisionError::Cancelled)
    ));
}

#[test]
fn quarantined_provider_mailbox_discards_published_target_owners() {
    let mailbox = ProviderMailbox::new();
    mailbox
        .publish(ProviderEvent::ContourBatch(vec![UiTarget {
            details: None,
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            name: "discard me".into(),
            role: SemanticRole::StaticText,
        }]))
        .unwrap();
    mailbox.close();
    mailbox.discard();

    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    assert!(events.is_empty());
    assert_eq!(mailbox.ready_flags.load(Ordering::Acquire), 0);
}

#[test]
fn pure_rust_detector_finds_a_closed_button_border() {
    let mut pixels = vec![255u8; 120 * 80 * 4];
    for y in 20..50 {
        for x in 25..95 {
            if y == 20 || y == 49 || x == 25 || x == 94 {
                let index = (y * 120 + x) * 4;
                pixels[index..index + 3].fill(0);
            }
        }
    }
    let geometry = CaptureGeometry {
        width: 120,
        height: 80,
        desktop_bounds: Rect::new(0.0, 0.0, 120.0, 80.0),
        scale: 1.0,
    };
    let image = contour_input_from_bgra(&pixels, geometry).unwrap();
    let targets = crate::platform::common::contour::detect(
        &image.gray,
        image.width,
        image.height,
        image.desktop_bounds,
        100,
        || false,
    );
    assert!(
        targets
            .iter()
            .any(|target| target.role == SemanticRole::Control)
    );
}

#[test]
fn worker_start_does_not_wait_for_ocr_discovery_or_start_a_coordinator() {
    let started = Instant::now();
    let mut worker = VisionWorker::start();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(worker.workers.is_empty());
    assert!(worker.discovery.worker.is_none());
    worker.stop().unwrap();
    worker.stop().unwrap();
}

#[test]
fn cancellation_does_not_start_an_idle_coordinator() {
    let mut worker = VisionWorker::start();
    worker.shared.latest_generation.store(17, Ordering::Release);
    worker
        .shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .active_request_id = Some(91);
    worker.cancel(91);
    assert_eq!(worker.shared.latest_generation.load(Ordering::Acquire), 0);
    assert!(worker.workers.is_empty());
    worker.stop().unwrap();
}

#[test]
fn completed_discovery_snapshot_is_reused() {
    let shared = Arc::new(DiscoveryShared::default());
    let snapshot = Arc::new(OcrDiscoverySnapshot::default());
    *shared
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = DiscoveryState::Ready(Arc::clone(&snapshot));
    let discovery = DiscoveryHandle(shared);
    let deadline = Instant::now() + Duration::from_secs(1);
    let first = discovery.wait(deadline, || false).unwrap();
    let second = discovery.wait(deadline, || false).unwrap();
    assert!(Arc::ptr_eq(&snapshot, &first));
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn cancelling_an_old_active_scan_does_not_invalidate_a_new_pending_scan() {
    assert!(!cancellation_clears_generation(Some(7), Some(8), 7));
    assert!(cancellation_clears_generation(Some(7), Some(8), 8));
    assert!(cancellation_clears_generation(Some(7), None, 7));
    assert!(!cancellation_clears_generation(Some(7), None, 9));
}

#[test]
fn provider_group_cancels_and_joins_every_thread() {
    let shared = Arc::new(SharedQueue::default());
    shared.latest_generation.store(23, Ordering::Release);
    let mailbox = Arc::new(ProviderMailbox::new());
    let cancellation = ScanCancellation::new(&shared, 23, &mailbox, Weak::new());
    let provider_cancellation = cancellation.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let provider_stopped = Arc::clone(&stopped);
    let mut providers = ProviderThreads::new(cancellation, &shared);
    providers.spawn("keysteer-cancellation-test", move || {
        while !provider_cancellation.is_cancelled() {
            std::thread::yield_now();
        }
        provider_stopped.store(true, Ordering::Release);
    });
    drop(providers);
    assert!(stopped.load(Ordering::Acquire));
}

fn quarantine_blocked_provider(worker: &VisionWorker) -> mpsc::Sender<()> {
    let mailbox = Arc::new(ProviderMailbox::new());
    let cancellation = ScanCancellation::new(&worker.shared, 1, &mailbox, Weak::new());
    let (release, wait) = mpsc::channel();
    let mut providers = ProviderThreads::new(cancellation, &worker.shared);
    assert!(providers.spawn("keysteer-quarantine-test", move || {
        let _ = wait.recv_timeout(Duration::from_secs(5));
    }));
    assert!(providers.join_all(Instant::now()).is_err());
    assert!(
        worker
            .shared
            .provider_quarantine_nonempty
            .load(Ordering::Acquire)
    );
    release
}

fn quarantine_scan_fixture(
    strategy: crate::api::UiScanStrategy,
    provider_count: usize,
    bounds: Rect,
) -> (
    Arc<WindowsScanPlan>,
    u64,
    Arc<super::super::ui_scan::ScanSession>,
    Arc<crate::platform::common::scan_mailbox::ScanMailbox>,
    crate::platform::common::scan_fusion::Worker,
) {
    use crate::platform::common::scan_mailbox::ScanMailbox;
    use crate::platform::windows::{EventSender, accessibility, ui_scan::ScanSession};
    let request = crate::api::UiScanRequest {
        id: 993,
        scope: crate::api::UiScanScope::Screen,
        timeout_ms: 1000,
        bounds: Some(bounds),
        roles: Vec::new(),
        max_depth: 8,
        visible_only: true,
        clickable_only: true,
        strategy,
        vision: crate::api::VisionOptions::default(),
        app: None,
    };
    let output = Arc::new(ScanMailbox::default());
    let generation = output.begin(request.id);
    let (events, _) = crate::platform::common::event_queue::channel();
    let plan = accessibility::test_scan_plan(request);
    let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
    let fusion = fusion_worker.prepare(generation).unwrap();
    let session = ScanSession::test(
        Arc::clone(&plan),
        generation,
        provider_count,
        Arc::clone(&output),
        EventSender::without_wake(events),
        &fusion,
    );
    (plan, generation, session, output, fusion_worker)
}

#[test]
fn quarantined_vision_submit_preserves_hybrid_uia_results_and_deferred_frame() {
    let mut worker = VisionWorker::start();
    let release = quarantine_blocked_provider(&worker);
    for _ in 0..3 {
        let (plan, generation, session, output, _fusion_worker) = quarantine_scan_fixture(
            crate::api::UiScanStrategy::Hybrid,
            2,
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
        );
        let uia = session.source("UI Automation");
        uia.push(vec![UiTarget {
            details: None,
            rect: Rect::new(10.0, 20.0, 80.0, 30.0),
            name: "Available button".into(),
            role: SemanticRole::Button,
        }]);
        uia.finish(UiScanStatus::Success);
        let (capture, frame_restored) =
            super::super::overlay_worker::deferred_capture_fixture(generation);
        worker
            .submit(plan, generation, session.source("visual scan"), capture)
            .expect("quarantine must not cancel the hybrid session");
        session.flush();
        let result = output.take().expect("both sources must publish a terminal");
        assert_eq!(result.status, UiScanStatus::Success);
        assert_eq!(result.targets.len(), 1);
        assert_eq!(result.targets[0].name, "Available button");
        assert!(output.take().is_none());
        assert!(frame_restored());
        assert!(worker.workers.is_empty());
        assert!(worker.discovery.worker.is_none());
    }
    release.send(()).unwrap();
    worker.stop().unwrap();
}

#[test]
fn scan_queued_before_quarantine_finishes_without_starting_native_work() {
    let mut worker = VisionWorker::start();
    let release = quarantine_blocked_provider(&worker);
    let (plan, generation, session, output, _fusion_worker) = quarantine_scan_fixture(
        crate::api::UiScanStrategy::Vision,
        1,
        Rect::new(0.0, 0.0, 1920.0, 1080.0),
    );
    worker
        .shared
        .latest_generation
        .store(generation, Ordering::Release);
    let (capture, frame_restored) =
        super::super::overlay_worker::deferred_capture_fixture(generation);
    run_scan(
        ScanJob {
            request: plan,
            generation,
            source: session.source("visual scan"),
            capture: Some(capture),
        },
        &worker.shared,
        &worker.discovery.handle(),
    );
    session.flush();
    let result = output.take().expect("queued scan must publish a terminal");
    assert!(matches!(result.status, UiScanStatus::Unsupported(_)));
    assert!(frame_restored());
    assert!(worker.discovery.worker.is_none());
    release.send(()).unwrap();
    worker.stop().unwrap();
}

#[test]
fn vision_resumes_after_quarantined_provider_exits() {
    let mut worker = VisionWorker::start();
    let release = quarantine_blocked_provider(&worker);
    worker.reap_finished();
    assert!(
        worker
            .shared
            .provider_quarantine_nonempty
            .load(Ordering::Acquire)
    );
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while worker
        .shared
        .provider_quarantine_nonempty
        .load(Ordering::Acquire)
    {
        assert!(
            Instant::now() < deadline,
            "completed provider must be reaped"
        );
        worker.reap_finished();
        std::thread::yield_now();
    }
    // Invalid bounds let us verify submission reaches the normal capture path
    // after recovery without depending on native OCR or screen contents.
    let (plan, generation, session, output, _fusion_worker) = quarantine_scan_fixture(
        crate::api::UiScanStrategy::Contour,
        1,
        Rect::new(0.0, 0.0, 0.0, 0.0),
    );
    let (capture, frame_restored) =
        super::super::overlay_worker::deferred_capture_fixture(generation);
    worker
        .submit(plan, generation, session.source("visual scan"), capture)
        .unwrap();
    let result = loop {
        if let Some(result) = output.take() {
            break result;
        }
        assert!(Instant::now() < deadline, "recovered scan must finish");
        std::thread::yield_now();
    };
    assert!(
        matches!(result.status, UiScanStatus::Failed(error) if error.contains("visual capture bounds are empty"))
    );
    assert!(frame_restored());
    worker.stop().unwrap();
}

#[test]
fn scan_cancellation_wakes_system_ocr_without_polling() {
    let shared = Arc::new(SharedQueue::default());
    shared.latest_generation.store(29, Ordering::Release);
    let mailbox = Arc::new(ProviderMailbox::new());
    let cancellation = ScanCancellation::new(&shared, 29, &mailbox, Weak::new());
    let (system_sender, system_receiver) = mpsc::sync_channel(1);
    let (credit_sender, credit_receiver) = mpsc::channel();
    let (wechat_sender, wechat_receiver) = mpsc::sync_channel(1);
    cancellation.register_system_wake(system_sender);
    cancellation.register_system_credit_wake(credit_sender);
    cancellation.register_wechat_wake(wechat_sender);
    cancellation.cancel();
    assert!(matches!(
        system_receiver.try_recv(),
        Ok(SystemOcrInput::CancelWake)
    ));
    assert!(matches!(
        wechat_receiver.try_recv(),
        Ok(WechatInput::CancelWake)
    ));
    assert_eq!(credit_receiver.try_recv(), Ok(()));
    assert!(matches!(
        mailbox.wait_until_ready(Instant::now()),
        Err(VisionError::Cancelled)
    ));
}

#[test]
fn expired_generation_deadline_still_retains_ocr_until_terminal_wake() {
    let (sender, receiver) = mpsc::channel();
    sender.send(SystemOcrInput::CompletionWake).unwrap();
    let event = wait_system_ocr_cancellation_event(&receiver, Instant::now())
        .expect("the terminal wake remains authoritative after the scan deadline");
    assert!(matches!(event, SystemOcrInput::CompletionWake));
}

#[test]
fn pure_rust_detector_stops_at_cancellation_checkpoints() {
    let geometry = CaptureGeometry {
        width: 256,
        height: 256,
        desktop_bounds: Rect::new(0.0, 0.0, 256.0, 256.0),
        scale: 1.0,
    };
    let image = contour_input_from_bgra(&vec![255; 256 * 256 * 4], geometry).unwrap();
    let checks = AtomicUsize::new(0);
    let targets = crate::platform::common::contour::detect(
        &image.gray,
        image.width,
        image.height,
        image.desktop_bounds,
        100,
        || checks.fetch_add(1, Ordering::Relaxed) >= 2,
    );
    assert!(targets.is_empty());
    assert!(checks.load(Ordering::Relaxed) >= 3);
}

#[test]
#[ignore = "requires an installed Windows OCR language pack"]
fn live_system_ocr_runtime_probe_creates_and_drops_an_engine() {
    assert!(probe_system_ocr(|| false).is_ok());
}

#[derive(Clone, Copy)]
struct OcrTileProbe {
    bitmap_x: u32,
    bitmap_y: u32,
    core_x: u32,
    core_y: u32,
    core_right: u32,
    core_bottom: u32,
}

struct OcrTileProbeResult {
    elapsed: Duration,
    raw_words: usize,
    owned_words: usize,
}

fn prepare_tile_probe(
    pixels: &[u8],
    width: u32,
    height: u32,
    grid: u32,
    overlap: u32,
) -> Result<Vec<(windows::Graphics::Imaging::SoftwareBitmap, OcrTileProbe)>, String> {
    let mut tiles = Vec::with_capacity((grid * grid) as usize);
    let factory = super::super::native::SoftwareBitmapFactory::load()?;
    for row in 0..grid {
        for column in 0..grid {
            let core_x = column * width / grid;
            let core_right = (column + 1) * width / grid;
            let core_y = row * height / grid;
            let core_bottom = (row + 1) * height / grid;
            let bitmap_x = core_x.saturating_sub(overlap);
            let bitmap_y = core_y.saturating_sub(overlap);
            let bitmap_right = core_right.saturating_add(overlap).min(width);
            let bitmap_bottom = core_bottom.saturating_add(overlap).min(height);
            let tile_width = bitmap_right - bitmap_x;
            let tile_height = bitmap_bottom - bitmap_y;
            let bitmap = factory.bgra_region(
                pixels,
                width,
                height,
                bitmap_x,
                bitmap_y,
                tile_width,
                tile_height,
            )?;
            tiles.push((
                bitmap,
                OcrTileProbe {
                    bitmap_x,
                    bitmap_y,
                    core_x,
                    core_y,
                    core_right,
                    core_bottom,
                },
            ));
        }
    }
    Ok(tiles)
}

fn run_tile_probe(
    tiles: &[(windows::Graphics::Imaging::SoftwareBitmap, OcrTileProbe)],
) -> Result<Vec<OcrTileProbeResult>, String> {
    let started = Instant::now();
    std::thread::scope(|scope| {
        let workers = tiles
            .iter()
            .map(|(bitmap, tile)| {
                let bitmap = bitmap.clone();
                scope.spawn(move || {
                    let _apartment = super::super::native::ComApartment::initialise()?;
                    let engine = super::super::native::create_system_ocr_engine()?;
                    let result = engine
                        .RecognizeAsync(&bitmap)
                        .and_then(|operation| operation.join())
                        .map_err(|error| format!("tiled OcrEngine failed: {error}"))?;
                    let mut raw_words = 0usize;
                    let mut owned_words = 0usize;
                    let lines = result
                        .Lines()
                        .map_err(|error| format!("cannot enumerate OCR lines: {error}"))?;
                    for line in &lines {
                        let words = line
                            .Words()
                            .map_err(|error| format!("cannot enumerate OCR words: {error}"))?;
                        for word in &words {
                            raw_words += 1;
                            let rect = word
                                .BoundingRect()
                                .map_err(|error| format!("cannot read OCR word bounds: {error}"))?;
                            let center_x = tile.bitmap_x as f32 + rect.X + rect.Width * 0.5;
                            let center_y = tile.bitmap_y as f32 + rect.Y + rect.Height * 0.5;
                            if center_x >= tile.core_x as f32
                                && center_x < tile.core_right as f32
                                && center_y >= tile.core_y as f32
                                && center_y < tile.core_bottom as f32
                            {
                                owned_words += 1;
                            }
                        }
                    }
                    Ok(OcrTileProbeResult {
                        elapsed: started.elapsed(),
                        raw_words,
                        owned_words,
                    })
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .map_err(|_| "tiled OCR worker panicked".to_string())?
            })
            .collect::<Result<Vec<_>, String>>()
    })
}

fn tile_probe_percentile(values: &mut [Duration], percentile: usize) -> Duration {
    values.sort_unstable();
    values[(values.len() - 1) * percentile / 100]
}

#[test]
#[ignore = "manual Windows OCR tiling benchmark; set KEYSTEER_OCR_TILE_IMAGE if needed"]
fn live_system_ocr_tiling_probe() -> Result<(), String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat};
    use windows::Storage::Streams::{Buffer, DataReader, DataWriter, InMemoryRandomAccessStream};

    const OVERLAP: u32 = 64;
    const WARMUPS: usize = 2;
    const SAMPLES: usize = 10;

    let _apartment = super::super::native::ComApartment::initialise()?;
    let image_path = std::env::var_os("KEYSTEER_OCR_TILE_IMAGE").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("browser.jpg"),
        PathBuf::from,
    );
    let encoded = std::fs::read(&image_path)
        .map_err(|error| format!("cannot read {}: {error}", image_path.display()))?;
    let stream = InMemoryRandomAccessStream::new()
        .map_err(|error| format!("cannot create image stream: {error}"))?;
    let writer = DataWriter::CreateDataWriter(&stream)
        .map_err(|error| format!("cannot create image writer: {error}"))?;
    writer
        .WriteBytes(&encoded)
        .map_err(|error| format!("cannot stage image bytes: {error}"))?;
    writer
        .StoreAsync()
        .and_then(|operation| operation.join())
        .map_err(|error| format!("cannot store image bytes: {error}"))?;
    writer
        .DetachStream()
        .map_err(|error| format!("cannot detach image stream: {error}"))?;
    writer
        .Close()
        .map_err(|error| format!("cannot close image writer: {error}"))?;
    stream
        .Seek(0)
        .map_err(|error| format!("cannot rewind image stream: {error}"))?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .and_then(|operation| operation.join())
        .map_err(|error| format!("cannot decode {}: {error}", image_path.display()))?;
    let width = decoder
        .PixelWidth()
        .map_err(|error| format!("cannot read image width: {error}"))?;
    let height = decoder
        .PixelHeight()
        .map_err(|error| format!("cannot read image height: {error}"))?;
    let full_bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Ignore)
        .and_then(|operation| operation.join())
        .map_err(|error| format!("cannot convert probe image to BGRA: {error}"))?;
    let byte_length = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "probe image byte length overflowed".to_string())?;
    let buffer = Buffer::Create(byte_length)
        .map_err(|error| format!("cannot create probe pixel buffer: {error}"))?;
    full_bitmap
        .CopyToBuffer(&buffer)
        .map_err(|error| format!("cannot copy probe bitmap pixels: {error}"))?;
    let reader = DataReader::FromBuffer(&buffer)
        .map_err(|error| format!("cannot create probe pixel reader: {error}"))?;
    let mut pixels = vec![0; byte_length as usize];
    reader
        .ReadBytes(&mut pixels)
        .map_err(|error| format!("cannot read probe bitmap pixels: {error}"))?;
    reader
        .Close()
        .map_err(|error| format!("cannot close probe pixel reader: {error}"))?;
    full_bitmap
        .Close()
        .map_err(|error| format!("cannot close decoded probe bitmap: {error}"))?;
    println!(
        "ocr_tile_probe image={} dimensions={}x{} overlap={}px samples={}",
        image_path.display(),
        width,
        height,
        OVERLAP,
        SAMPLES
    );

    for grid in 1..=7 {
        let preparation_started = Instant::now();
        let tiles = prepare_tile_probe(&pixels, width, height, grid, OVERLAP)?;
        let preparation = preparation_started.elapsed();
        for _ in 0..WARMUPS {
            run_tile_probe(&tiles)?;
        }
        let mut first_samples = Vec::with_capacity(SAMPLES);
        let mut total_samples = Vec::with_capacity(SAMPLES);
        let mut raw_words = 0usize;
        let mut owned_words = 0usize;
        for _ in 0..SAMPLES {
            let results = run_tile_probe(&tiles)?;
            first_samples.push(
                results
                    .iter()
                    .map(|result| result.elapsed)
                    .min()
                    .ok_or_else(|| "tile probe produced no result".to_string())?,
            );
            total_samples.push(
                results
                    .iter()
                    .map(|result| result.elapsed)
                    .max()
                    .ok_or_else(|| "tile probe produced no result".to_string())?,
            );
            raw_words += results.iter().map(|result| result.raw_words).sum::<usize>();
            owned_words += results
                .iter()
                .map(|result| result.owned_words)
                .sum::<usize>();
        }
        let first_p50 = tile_probe_percentile(&mut first_samples, 50);
        let first_p95 = tile_probe_percentile(&mut first_samples, 95);
        let total_p50 = tile_probe_percentile(&mut total_samples, 50);
        let total_p95 = tile_probe_percentile(&mut total_samples, 95);
        println!(
            "ocr_tile_probe grid={}x{} tiles={} prepare_ms={:.3} first_p50_ms={:.3} first_p95_ms={:.3} total_p50_ms={:.3} total_p95_ms={:.3} raw_words_avg={} owned_words_avg={}",
            grid,
            grid,
            tiles.len(),
            preparation.as_secs_f64() * 1_000.0,
            first_p50.as_secs_f64() * 1_000.0,
            first_p95.as_secs_f64() * 1_000.0,
            total_p50.as_secs_f64() * 1_000.0,
            total_p95.as_secs_f64() * 1_000.0,
            raw_words / SAMPLES,
            owned_words / SAMPLES,
        );
        for (bitmap, _) in tiles {
            bitmap
                .Close()
                .map_err(|error| format!("cannot close OCR tile bitmap: {error}"))?;
        }
    }
    stream
        .Close()
        .map_err(|error| format!("cannot close image stream: {error}"))?;
    Ok(())
}

#[test]
fn early_ocr_and_contour_are_both_published_without_waiting_for_ocr_terminal() {
    use crate::platform::common::scan_mailbox::ScanMailbox;
    use crate::platform::windows::{EventSender, accessibility, ui_scan::ScanSession};
    let input = crate::api::UiScanRequest {
        id: 991,
        scope: crate::api::UiScanScope::Screen,
        timeout_ms: 1000,
        bounds: Some(Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        roles: Vec::new(),
        max_depth: 8,
        visible_only: true,
        clickable_only: true,
        strategy: crate::api::UiScanStrategy::Hybrid,
        vision: crate::api::VisionOptions::default(),
        app: None,
    };
    let output = Arc::new(ScanMailbox::default());
    let generation = output.begin(input.id);
    let (events, _) = crate::platform::common::event_queue::channel();
    let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
    let fusion = fusion_worker.prepare(generation).unwrap();
    let session = ScanSession::test(
        accessibility::test_scan_plan(input),
        generation,
        1,
        Arc::clone(&output),
        EventSender::without_wake(events),
        &fusion,
    );
    let source = session.source("visual");
    let mailbox = ProviderMailbox::new();
    let target = |x, name: &str| UiTarget {
        details: None,
        rect: Rect::new(x, 20.0, 40.0, 20.0),
        name: name.into(),
        role: SemanticRole::Control,
    };
    mailbox
        .publish(ProviderEvent::OcrBatch {
            provider: "system",
            elapsed: Duration::ZERO,
            targets: vec![target(10.0, "OCR text")],
        })
        .unwrap();
    mailbox
        .publish(ProviderEvent::ContourBatch(vec![target(110.0, "")]))
        .unwrap();
    let mut deferred = ProviderEvents::new();
    assert!(drain_early_ocr_events(
        &mailbox,
        &source,
        &mut deferred,
        || true
    ));
    assert!(deferred.is_empty());
    source.finish(UiScanStatus::Success);
    session.flush();
    let result = output.take().unwrap();
    assert_eq!(result.targets.len(), 2);
    assert!(result.targets.iter().any(|t| t.name == "OCR text"));
    assert!(result.targets.iter().any(|t| t.rect.x == 110.0));
}

#[test]
fn early_and_late_visual_sources_use_identical_ownership() {
    use crate::platform::common::scan_mailbox::ScanMailbox;
    use crate::platform::common::spatial_index::TargetSource;
    use crate::platform::windows::{EventSender, accessibility, ui_scan::ScanSession};
    for provider in ["system", "wechat"] {
        for early_mask in 0..8 {
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                let request = crate::api::UiScanRequest {
                    id: 992,
                    scope: crate::api::UiScanScope::Screen,
                    timeout_ms: 1000,
                    bounds: Some(Rect::new(0., 0., 1920., 1080.)),
                    roles: Vec::new(),
                    max_depth: 8,
                    visible_only: true,
                    clickable_only: true,
                    strategy: crate::api::UiScanStrategy::Hybrid,
                    vision: crate::api::VisionOptions::default(),
                    app: None,
                };
                let output = Arc::new(ScanMailbox::default());
                let generation = output.begin(request.id);
                let (events, _) = crate::platform::common::event_queue::channel();
                let mut fusion_worker = crate::platform::common::scan_fusion::Worker::default();
                let fusion = fusion_worker.prepare(generation).unwrap();
                let session = ScanSession::test(
                    accessibility::test_scan_plan(request),
                    generation,
                    1,
                    Arc::clone(&output),
                    EventSender::without_wake(events),
                    &fusion,
                );
                let source = session.source("visual");
                let mailbox = ProviderMailbox::new();
                let targets = [
                    UiTarget {
                        details: None,
                        rect: Rect::new(100., 100., 240., 160.),
                        name: String::new(),
                        role: SemanticRole::Image,
                    },
                    UiTarget {
                        details: None,
                        rect: Rect::new(130., 130., 80., 20.),
                        name: "caption".into(),
                        role: SemanticRole::StaticText,
                    },
                    UiTarget {
                        details: None,
                        rect: Rect::new(140., 135., 12., 12.),
                        name: String::new(),
                        role: SemanticRole::Control,
                    },
                ];
                let button = UiTarget {
                    details: None,
                    rect: Rect::new(250., 190., 30., 25.),
                    name: "action".into(),
                    role: SemanticRole::Button,
                };
                source.push(vec![button.clone()]);
                session.flush();
                let mut visible = output.take().unwrap().targets;
                for i in order {
                    let batch = vec![targets[i].clone()];
                    if early_mask & (1 << i) != 0 {
                        let event = if i == 1 {
                            ProviderEvent::OcrBatch {
                                provider,
                                elapsed: Duration::ZERO,
                                targets: batch,
                            }
                        } else {
                            ProviderEvent::ContourBatch(batch)
                        };
                        mailbox.publish(event).unwrap();
                        assert!(drain_early_ocr_events(
                            &mailbox,
                            &source,
                            &mut ProviderEvents::new(),
                            || true
                        ));
                    } else {
                        source.push_from(
                            if i == 1 {
                                ocr_source(provider)
                            } else {
                                TargetSource::Contour
                            },
                            batch,
                        );
                    }
                    session.flush();
                    if let Some(update) = output.take() {
                        crate::api::command::remove_retired_targets(&mut visible, &update.retired);
                        visible.extend(update.targets);
                    }
                }
                source.finish(UiScanStatus::Success);
                session.flush();
                if let Some(update) = output.take() {
                    crate::api::command::remove_retired_targets(&mut visible, &update.retired);
                    visible.extend(update.targets);
                }
                assert_eq!(
                    visible.len(),
                    2,
                    "{provider} mask={early_mask} order={order:?}: {visible:?}"
                );
                assert!(visible.contains(&button));
                assert!(
                    visible
                        .iter()
                        .any(|t| t.role == SemanticRole::Image && t.name == "caption")
                );
            }
        }
    }
}

#[test]
fn contour_4k_and_8k_capture_analysis_stays_bounded_and_maps_back() {
    for (width, height) in [(3840.0, 2160.0), (7680.0, 4320.0)] {
        let bounds = Rect::new(-width, -100.0, width, height);
        let geometry = capture_geometry(bounds).unwrap();
        let mut pixels = vec![255u8; geometry.width as usize * geometry.height as usize * 4];
        for y in 100..130 {
            for x in 100..180 {
                let i = (y * geometry.width as usize + x) * 4;
                pixels[i..i + 3].fill(0);
            }
        }
        let image = contour_input_from_bgra(&pixels, geometry).unwrap();
        assert!(image.gray.len() <= crate::platform::common::contour::MAX_PIXELS);
        assert!(image.width.max(image.height) <= 2560);
        let targets = crate::platform::common::contour::detect(
            &image.gray,
            image.width,
            image.height,
            image.desktop_bounds,
            100,
            || false,
        );
        let center = crate::api::Point::new(
            bounds.x + 140.0 * width / f64::from(geometry.width),
            bounds.y + 115.0 * height / f64::from(geometry.height),
        );
        assert!(
            targets.iter().any(|t| t.rect.contains(&center)),
            "{targets:?}"
        );
    }
}

#[test]
fn contour_mailbox_transfers_full_candidate_buffer_without_rebuilding_it() {
    let mailbox = ProviderMailbox::new();
    let targets: Vec<_> = (0..MAX_OCR_TARGETS)
        .map(|i| UiTarget {
            details: None,
            rect: Rect::new(i as f64 * 10.0, 0.0, 8.0, 8.0),
            name: String::new(),
            role: SemanticRole::Control,
        })
        .collect();
    let original = targets.as_ptr();
    send_contour_batches(&mailbox, targets).unwrap();
    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    assert_eq!(events.len(), 2);
    let ProviderEvent::ContourBatch(targets) = &events[0] else {
        panic!("missing contours")
    };
    assert_eq!(targets.len(), 10_000);
    assert_eq!(targets.as_ptr(), original);
    assert!(matches!(events[1], ProviderEvent::ContourDone));
}

#[test]
fn ocr_word_and_phrase_converge_across_engines_and_batch_order() {
    use crate::platform::common::scan_accumulator::ScanAccumulator;
    use crate::platform::common::spatial_index::TargetSource;
    for reverse in [false, true] {
        for swap_engines in [false, true] {
            let phrase = UiTarget {
                details: None,
                rect: Rect::new(10., 10., 140., 20.),
                name: "full phrase".into(),
                role: SemanticRole::StaticText,
            };
            let word = UiTarget {
                details: None,
                rect: Rect::new(55., 11., 18., 18.),
                name: "word".into(),
                role: SemanticRole::StaticText,
            };
            let sources = if swap_engines {
                [TargetSource::WechatOcr, TargetSource::SystemOcr]
            } else {
                [TargetSource::SystemOcr, TargetSource::WechatOcr]
            };
            let mut batches = vec![(sources[0], phrase.clone()), (sources[1], word)];
            if reverse {
                batches.reverse();
            }
            let mut scan = ScanAccumulator::new();
            let mut visible = Vec::new();
            for (source, target) in batches {
                let update = scan.push(source, vec![target], 0.5);
                crate::api::command::remove_retired_targets(&mut visible, &update.retired);
                visible.extend(update.batches.into_iter().flatten());
            }
            let mut next_row = phrase.clone();
            next_row.rect.y = 36.;
            let update = scan.push(sources[1], vec![next_row.clone()], 0.5);
            visible.extend(update.batches.into_iter().flatten());
            visible.extend(scan.finish().into_iter().flatten());
            assert_eq!(visible.len(), 2, "reverse={reverse} swap={swap_engines}");
            assert!(visible.iter().any(|t| t.rect == phrase.rect));
            assert!(visible.iter().any(|t| t.rect == next_row.rect));
        }
    }
}

#[test]
fn ready_visual_data_bypasses_busy_native_coordinator_and_count_gates() {
    let (_plan, _generation, session, output, _fusion_worker) = quarantine_scan_fixture(
        crate::api::UiScanStrategy::Vision,
        1,
        Rect::new(0., 0., 1920., 1080.),
    );
    let source = session.source("visual");
    let mailbox = ProviderMailbox::with_output(source.sink());
    mailbox
        .publish(ProviderEvent::OcrBatch {
            provider: "system",
            elapsed: Duration::ZERO,
            targets: Vec::new(),
        })
        .unwrap();
    session.flush();
    assert!(output.take().is_none());
    let targets: Vec<_> = (0..512)
        .map(|i| {
            UiTarget::recognized_text(
                Rect::new((i % 32 * 50) as f64, (i / 32 * 50) as f64, 20., 20.),
                i.to_string(),
            )
        })
        .collect();
    send_ocr_batches(&mailbox, "system", Instant::now(), targets).unwrap();
    // Deliberately never drain the coordinator mailbox: data must already flow.
    session.flush();
    let ready = output.take().unwrap();
    assert_eq!(ready.targets.len(), 512);
    assert_eq!(ready.status, UiScanStatus::Partial);
    assert_eq!(mailbox.ready_flags.load(Ordering::Acquire), 0);
    source.finish(UiScanStatus::Success);
    session.flush();
    assert_eq!(output.take().unwrap().status, UiScanStatus::Success);
}

#[test]
fn provider_data_slots_ignore_empty_and_keep_valid_prefix_of_oversized_buffers() {
    let mailbox = ProviderMailbox::new();
    send_ocr_batches(&mailbox, "system", Instant::now(), Vec::new()).unwrap();
    mailbox
        .publish(ProviderEvent::ContourBatch(Vec::new()))
        .unwrap();
    assert_eq!(mailbox.ready_flags.load(Ordering::Acquire), 0);
    let targets = || {
        (0..MAX_OCR_TARGETS + 2)
            .map(|i| {
                UiTarget::recognized_text(Rect::new(i as f64 * 20., 0., 10., 10.), i.to_string())
            })
            .collect()
    };
    assert_eq!(
        send_ocr_batches(&mailbox, "system", Instant::now(), targets()).unwrap(),
        MAX_OCR_TARGETS
    );
    send_contour_batches(&mailbox, targets()).unwrap();
    let mut events = ProviderEvents::new();
    mailbox.drain_into(&mut events);
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                ProviderEvent::OcrBatch { targets, .. } | ProviderEvent::ContourBatch(targets) =>
                    Some(targets.len()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![MAX_OCR_TARGETS, MAX_OCR_TARGETS]
    );
}
