use std::ffi::c_char;
use std::ptr::NonNull;

#[derive(Default)]
pub(super) struct PointSampler {
    native: *mut std::ffi::c_void,
}

impl PointSampler {
    pub(super) fn sample(
        &mut self,
        request: Option<crate::api::point_sample::Request>,
    ) -> Option<crate::api::Color> {
        let point = request.map(|r| r.point).unwrap_or_default();
        let mut rgba = [0u8; 4];
        // SAFETY: the opaque owner is created, used and released only on this
        // worker. The bridge writes four bytes synchronously and retains neither
        // stack pointer. Its late capture callbacks retain their own native owner.
        let ok = unsafe {
            NmkSamplePoint(
                &mut self.native,
                point.x,
                point.y,
                request.is_none(),
                rgba.as_mut_ptr(),
            )
        };
        ok.then(|| crate::api::Color::rgb(rgba[0], rgba[1], rgba[2]))
    }
}

impl Drop for PointSampler {
    fn drop(&mut self) {
        self.sample(None);
    }
}

use crate::api::command::{UiScanStatus, VisionOptions};
use crate::api::geometry::{Rect, SemanticRole, UiTarget};
use crate::platform::common::spatial_index::{SpatialIndex, TargetSource};

#[repr(C)]
#[derive(Clone, Copy)]
struct NativePoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeRect {
    origin: NativePoint,
    size: NativeSize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeConfig {
    detect_text: bool,
    detect_rectangles: bool,
    detect_contours: bool,
    timeout_ms: u64,
    minimum_confidence: f64,
    rectangle_max_candidates: u64,
    rectangle_min_size: f64,
    rectangle_min_aspect: f64,
    rectangle_max_aspect: f64,
    maximum_capture_pixels: u64,
    maximum_capture_dimension: u64,
}

#[repr(C)]
struct NativeRegion {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    confidence: f64,
    is_text: bool,
    label: *mut c_char,
    label_len: u64,
}

#[repr(C)]
struct NativeResult {
    abi_version: u32,
    result_size: u32,
    region_stride: u32,
    status: i32,
    regions: *mut NativeRegion,
    count: u64,
    message: *mut c_char,
    message_len: u64,
    captured_bounds: NativeRect,
    image: usize,
    gray: *mut u8,
    gray_width: u64,
    gray_height: u64,
}

const MAX_VISION_REGIONS: usize = crate::api::command::MAX_UI_SCAN_TARGETS;
const MAX_VISION_LABEL_BYTES: usize = 64 * 1024;
const MAX_VISION_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_CAPTURE_PIXELS: u64 = 8_388_608;
const MAX_CAPTURE_DIMENSION: u64 = 4_096;
const VISION_ABI_VERSION: u32 = 3;

struct OwnedVisionResult(NonNull<NativeResult>);

impl OwnedVisionResult {
    fn new(result: *mut NativeResult) -> Result<Option<Self>, String> {
        let Some(result) = NonNull::new(result) else {
            return Ok(None);
        };
        if result.as_ptr().addr() % std::mem::align_of::<NativeResult>() != 0 {
            return Err("Vision returned a misaligned result pointer".into());
        }
        Ok(Some(Self(result)))
    }

    fn as_ref(&self) -> &NativeResult {
        // SAFETY: the bridge returned an owned non-null result and this wrapper
        // retains it until Drop. No mutable access is exposed while borrowed.
        unsafe { self.0.as_ref() }
    }

    fn gray(&self) -> Result<(&[u8], usize, usize), String> {
        let raw = self.as_ref();
        let width = usize::try_from(raw.gray_width).map_err(|_| "invalid contour width")?;
        let height = usize::try_from(raw.gray_height).map_err(|_| "invalid contour height")?;
        let length = width
            .checked_mul(height)
            .filter(|&n| n > 0 && n <= crate::platform::common::contour::MAX_PIXELS)
            .ok_or("invalid contour frame dimensions")?;
        let pixels = NonNull::new(raw.gray).ok_or("missing contour frame")?;
        // SAFETY: the version-checked bridge allocates width*height bytes, capped
        // above; this owner keeps them alive through the scoped contour worker.
        let gray = unsafe { std::slice::from_raw_parts(pixels.as_ptr(), length) };
        Ok((gray, width, height))
    }

    fn regions(&self) -> Result<&[NativeRegion], String> {
        let raw = self.as_ref();
        if raw.abi_version != VISION_ABI_VERSION
            || raw.result_size as usize != std::mem::size_of::<NativeResult>()
            || raw.region_stride as usize != std::mem::size_of::<NativeRegion>()
        {
            return Err(format!(
                "Vision ABI mismatch: version={}, result_size={}, region_stride={}",
                raw.abi_version, raw.result_size, raw.region_stride
            ));
        }
        let count = usize::try_from(raw.count)
            .map_err(|_| "Vision returned an unrepresentable region count".to_string())?;
        if count > MAX_VISION_REGIONS {
            return Err(format!(
                "Vision returned {count} regions; maximum is {MAX_VISION_REGIONS}"
            ));
        }
        if count == 0 {
            return Ok(&[]);
        }
        let regions = NonNull::new(raw.regions)
            .ok_or_else(|| "Vision returned regions with a null pointer".to_string())?;
        if regions.as_ptr().addr() % std::mem::align_of::<NativeRegion>() != 0 {
            return Err("Vision returned a misaligned region pointer".into());
        }
        let _byte_len = count
            .checked_mul(std::mem::size_of::<NativeRegion>())
            .filter(|&length| length <= isize::MAX as usize)
            .ok_or_else(|| "Vision returned an unrepresentable region buffer".to_string())?;
        // SAFETY: the native bridge allocates `count` contiguous NativeRegion
        // values, the count is capped by the shared ABI maximum, and the owner
        // keeps the allocation alive for the returned borrow.
        Ok(unsafe { std::slice::from_raw_parts(regions.as_ptr(), count) })
    }
}

impl Drop for OwnedVisionResult {
    fn drop(&mut self) {
        // SAFETY: this wrapper is constructed only from an owned bridge result
        // and Drop is the sole release path.
        unsafe { NmkFreeVisionResult(self.0.as_ptr()) };
    }
}

unsafe extern "C" {
    fn NmkSamplePoint(
        owner: *mut *mut std::ffi::c_void,
        x: f64,
        y: f64,
        reset: bool,
        rgba: *mut u8,
    ) -> bool;
    safe fn NmkSetLatestVisionScan(scan_id: u64);
    safe fn NmkDetectVisionElements(
        bounds: NativeRect,
        config: NativeConfig,
        scan_id: u64,
    ) -> *mut NativeResult;
    safe fn NmkRecognizeVisionImage(
        image: usize,
        bounds: NativeRect,
        config: NativeConfig,
        scan_id: u64,
    ) -> *mut NativeResult;
    fn NmkFreeVisionResult(result: *mut NativeResult);
}

#[derive(Debug, Clone)]
struct Candidate {
    target: UiTarget,
    confidence: f64,
    is_text: bool,
}

pub(super) fn mark_latest(scan_id: u64) {
    NmkSetLatestVisionScan(scan_id);
}

pub fn detect(
    scan_id: u64,
    bounds: Rect,
    options: &VisionOptions,
    strategy: crate::api::UiScanStrategy,
    cancelled: impl Fn() -> bool + Sync,
    publish: impl Fn(TargetSource, Vec<UiTarget>) + Sync,
) -> UiScanStatus {
    detect_inner(scan_id, bounds, options, strategy, &cancelled, &publish)
        .unwrap_or_else(UiScanStatus::Failed)
}

fn detect_inner(
    scan_id: u64,
    bounds: Rect,
    options: &VisionOptions,
    strategy: crate::api::UiScanStrategy,
    cancelled: &(impl Fn() -> bool + Sync),
    publish: &(impl Fn(TargetSource, Vec<UiTarget>) + Sync),
) -> Result<UiScanStatus, String> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(options.request_timeout_ms.clamp(1, 30_000));
    let contour_only = strategy == crate::api::UiScanStrategy::Contour;
    let (detect_text, detect_contours) =
        crate::platform::common::contour::sources(strategy, options);
    let candidate_limit = crate::platform::common::contour::candidate_limit(strategy, options);
    let config = NativeConfig {
        detect_text,
        detect_rectangles: !contour_only && detect_contours,
        detect_contours,
        timeout_ms: options.request_timeout_ms,
        minimum_confidence: options.minimum_confidence,
        rectangle_max_candidates: candidate_limit as u64,
        rectangle_min_size: options.rectangle_min_size,
        rectangle_min_aspect: options.rectangle_min_aspect,
        rectangle_max_aspect: options.rectangle_max_aspect,
        maximum_capture_pixels: MAX_CAPTURE_PIXELS,
        maximum_capture_dimension: MAX_CAPTURE_DIMENSION,
    };
    let native_bounds = NativeRect {
        origin: NativePoint {
            x: bounds.x,
            y: bounds.y,
        },
        size: NativeSize {
            width: bounds.width,
            height: bounds.height,
        },
    };
    let capture = OwnedVisionResult::new(NmkDetectVisionElements(native_bounds, config, scan_id))?
        .ok_or("Vision could not allocate a capture result")?;
    // Validate the complete shared ABI even for an empty region list.
    capture.regions()?;
    let raw = capture.as_ref();
    if raw.status != 0 {
        return Ok(decode_result(&capture, bounds, options).1);
    }
    if cancelled() {
        return Ok(UiScanStatus::ContextChanged);
    }
    if std::time::Instant::now() >= deadline {
        return Ok(UiScanStatus::TimedOut);
    }
    if raw.image == 0 {
        return Err("Vision returned no captured image".into());
    }
    let captured_bounds = raw.captured_bounds;
    let coordinate_bounds = capture_bounds_or_window(native_rect_to_rect(captured_bounds), bounds);
    let image = raw.image;
    let gray = if config.detect_contours {
        Some(capture.gray()?)
    } else {
        None
    };
    // Only a borrowed byte slice enters the scoped worker. Native capture and
    // image ownership stay here, and outlive both recognition and the join.
    let status = std::thread::scope(|scope| -> Result<UiScanStatus, String> {
        let contour = gray
            .map(|(pixels, width, height)| {
                std::thread::Builder::new()
                    .name("keysteer-contour".into())
                    .spawn_scoped(scope, move || {
                        let targets = crate::platform::common::contour::detect(
                            pixels,
                            width,
                            height,
                            coordinate_bounds,
                            candidate_limit,
                            || cancelled() || std::time::Instant::now() >= deadline,
                        );
                        if !cancelled() && std::time::Instant::now() < deadline {
                            publish(TargetSource::Contour, targets);
                        }
                    })
                    .map_err(|error| format!("cannot start contour worker: {error}"))
            })
            .transpose()?;
        let native_status = if config.detect_text || config.detect_rectangles {
            match OwnedVisionResult::new(NmkRecognizeVisionImage(
                image,
                captured_bounds,
                config,
                scan_id,
            )) {
                Ok(Some(result)) => {
                    let (targets, status) = decode_result(&result, coordinate_bounds, options);
                    if !cancelled() && std::time::Instant::now() < deadline {
                        publish(TargetSource::NativeVision, targets);
                    }
                    status
                }
                Ok(None) => {
                    UiScanStatus::Failed("Vision could not allocate recognition result".into())
                }
                Err(error) => UiScanStatus::Failed(error),
            }
        } else {
            UiScanStatus::Success
        };
        if let Some(worker) = contour {
            worker.join().map_err(|_| "contour worker panicked")?;
            if let UiScanStatus::Failed(ref error) = native_status {
                crate::support::logging::report_error("macos-vision", error);
                return Ok(UiScanStatus::Success);
            }
        }
        Ok(native_status)
    })?;
    if cancelled() {
        Ok(UiScanStatus::ContextChanged)
    } else if std::time::Instant::now() >= deadline {
        Ok(UiScanStatus::TimedOut)
    } else {
        Ok(status)
    }
}

fn decode_result(
    result: &OwnedVisionResult,
    bounds: Rect,
    options: &VisionOptions,
) -> (Vec<UiTarget>, UiScanStatus) {
    let raw = result.as_ref();
    let coordinate_bounds =
        capture_bounds_or_window(native_rect_to_rect(raw.captured_bounds), bounds);
    if raw.abi_version != VISION_ABI_VERSION
        || raw.result_size as usize != std::mem::size_of::<NativeResult>()
    {
        return (
            Vec::new(),
            UiScanStatus::Failed("Vision returned incompatible result metadata".into()),
        );
    }
    let message = match native_string(raw.message, raw.message_len, MAX_VISION_MESSAGE_BYTES) {
        Ok(message) if !message.is_empty() => message,
        Ok(_) => "Vision scan failed".into(),
        Err(error) => return (Vec::new(), UiScanStatus::Failed(error)),
    };
    let status = match raw.status {
        0 => UiScanStatus::Success,
        1 => UiScanStatus::PermissionDenied(message),
        2 => UiScanStatus::TimedOut,
        4 => UiScanStatus::ContextChanged,
        5 => UiScanStatus::Unsupported(message),
        _ => UiScanStatus::Failed(message),
    };
    let regions = match result.regions() {
        Ok(regions) => regions,
        Err(error) => return (Vec::new(), UiScanStatus::Failed(error)),
    };
    let mut candidates = Vec::with_capacity(regions.len());
    for region in regions {
        match classify(region, coordinate_bounds, options) {
            Ok(Some(candidate)) => candidates.push(candidate),
            Ok(None) => {}
            Err(error) => return (Vec::new(), UiScanStatus::Failed(error)),
        }
    }
    let targets = merge_candidates(candidates, options.merge_iou_threshold);
    (targets, status)
}

fn native_rect_to_rect(rect: NativeRect) -> Rect {
    Rect::new(
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    )
}

fn capture_bounds_or_window(captured_bounds: Rect, window_bounds: Rect) -> Rect {
    if captured_bounds.is_empty()
        || !captured_bounds.x.is_finite()
        || !captured_bounds.y.is_finite()
        || !captured_bounds.width.is_finite()
        || !captured_bounds.height.is_finite()
    {
        window_bounds
    } else {
        captured_bounds
    }
}

fn native_string(value: *const c_char, length: u64, maximum: usize) -> Result<String, String> {
    let length = usize::try_from(length)
        .map_err(|_| "Vision returned an unrepresentable string length".to_string())?;
    if length > maximum {
        return Err(format!(
            "Vision returned a {length}-byte string; maximum is {maximum}"
        ));
    }
    if length == 0 {
        return Ok(String::new());
    }
    let value = NonNull::new(value.cast_mut())
        .ok_or_else(|| "Vision returned a non-empty string with a null pointer".to_string())?;
    // SAFETY: the ABI supplies an explicit bounded byte length, and the owning
    // result keeps every strdup allocation alive for this borrow.
    let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), length) };
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn classify(
    region: &NativeRegion,
    bounds: Rect,
    options: &VisionOptions,
) -> Result<Option<Candidate>, String> {
    let rect = normalized_to_global(
        bounds,
        Rect::new(region.x, region.y, region.width, region.height),
    );
    if rect.is_empty() || !rect.width.is_finite() || !rect.height.is_finite() {
        return Ok(None);
    }
    let aspect = rect.width / rect.height.max(f64::EPSILON);
    let role = if region.is_text {
        // Match the shared OCR contract used by Windows. Text provenance
        // drives metadata fusion; guessing a clickable role loses OCR details.
        SemanticRole::StaticText
    } else if rect.width <= options.checkbox_max_size
        && rect.height <= options.checkbox_max_size
        && (0.75..=1.35).contains(&aspect)
    {
        SemanticRole::Checkbox
    } else if (region.confidence >= options.button_min_confidence
        && (options.button_min_aspect..=options.button_max_aspect).contains(&aspect))
        || (region.confidence >= options.generic_clickable_min_confidence
            && rect.width <= options.button_icon_max_size
            && rect.height <= options.button_icon_max_size)
    {
        // Aspect-ratio fits and icon-sized candidates are both buttons. These
        // were separate arms only because they carried different diagnostic
        // native roles.
        SemanticRole::Button
    } else if rect.width >= options.image_min_size && rect.height >= options.image_min_size {
        SemanticRole::Image
    } else if region.confidence >= options.generic_clickable_min_confidence {
        SemanticRole::Control
    } else {
        return Ok(None);
    };
    let name = native_string(region.label, region.label_len, MAX_VISION_LABEL_BYTES)?;
    let target = if region.is_text {
        UiTarget::recognized_text(rect, name)
    } else {
        UiTarget {
            details: None,
            rect,
            name,
            role,
        }
    };
    Ok(Some(Candidate {
        target,
        confidence: region.confidence,
        is_text: region.is_text,
    }))
}

fn normalized_to_global(bounds: Rect, normalized: Rect) -> Rect {
    Rect::new(
        bounds.x + normalized.x * bounds.width,
        bounds.y + (1.0 - normalized.y - normalized.height) * bounds.height,
        normalized.width * bounds.width,
        normalized.height * bounds.height,
    )
}

#[cfg(test)]
fn merge_targets(
    mut primary: Vec<UiTarget>,
    supplementary: Vec<UiTarget>,
    threshold: f64,
) -> Vec<UiTarget> {
    for target in supplementary {
        if primary
            .iter()
            .all(|existing| intersection_over_union(existing.rect, target.rect) < threshold)
        {
            primary.push(target);
        }
    }
    primary
}

fn merge_candidates(mut candidates: Vec<Candidate>, threshold: f64) -> Vec<UiTarget> {
    candidates.sort_by(|a, b| {
        b.is_text
            .cmp(&a.is_text)
            .then_with(|| b.confidence.total_cmp(&a.confidence))
    });
    let mut targets: Vec<UiTarget> = Vec::new();
    let mut index = SpatialIndex::new(64.0, 0.0, f64::EPSILON);
    for candidate in candidates {
        if index.insert_if_unique(candidate.target.rect, |existing, candidate| {
            intersection_over_union(existing, candidate) >= threshold
        }) {
            targets.push(candidate.target);
        }
    }
    targets
}

fn intersection_over_union(a: Rect, b: Rect) -> f64 {
    let Some(intersection) = a.intersect(&b) else {
        return 0.0;
    };
    let intersection_area = intersection.width * intersection.height;
    let union = a.width * a.height + b.width * b.height - intersection_area;
    if union <= 0.0 {
        0.0
    } else {
        intersection_area / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_vision_bottom_left_coordinates_to_global_top_left() {
        let bounds = Rect::new(100.0, -200.0, 800.0, 600.0);
        let converted = normalized_to_global(bounds, Rect::new(0.25, 0.6, 0.5, 0.2));
        assert_eq!(converted, Rect::new(300.0, -80.0, 400.0, 120.0));
    }

    #[test]
    fn maps_vision_regions_to_the_actual_clipped_capture_bounds() {
        let window_bounds = Rect::new(-1600.0, 80.0, 1000.0, 700.0);
        let captured_bounds = Rect::new(-1600.0, 80.0, 600.0, 700.0);
        let bounds = capture_bounds_or_window(captured_bounds, window_bounds);
        assert_eq!(
            normalized_to_global(bounds, Rect::new(0.5, 0.25, 0.25, 0.5)),
            Rect::new(-1300.0, 255.0, 150.0, 350.0)
        );
    }

    #[test]
    fn falls_back_to_window_bounds_when_the_bridge_returns_no_capture_bounds() {
        let window_bounds = Rect::new(-1600.0, 80.0, 1000.0, 700.0);
        assert_eq!(
            capture_bounds_or_window(Rect::default(), window_bounds),
            window_bounds
        );
    }

    #[test]
    fn classifier_preserves_ocr_provenance_and_checkbox_thresholds() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let options = VisionOptions::default();
        let empty = std::ffi::CString::new("").unwrap();
        let link = NativeRegion {
            x: 0.1,
            y: 0.8,
            width: 0.3,
            height: 0.03,
            confidence: 0.9,
            is_text: true,
            label: empty.as_ptr().cast_mut(),
            label_len: 0,
        };
        assert_eq!(
            classify(&link, bounds, &options)
                .unwrap()
                .unwrap()
                .target
                .role,
            SemanticRole::StaticText
        );
        let checkbox = NativeRegion {
            x: 0.1,
            y: 0.7,
            width: 0.025,
            height: 0.03,
            confidence: 0.9,
            is_text: false,
            label: empty.as_ptr().cast_mut(),
            label_len: 0,
        };
        assert_eq!(
            classify(&checkbox, bounds, &options)
                .unwrap()
                .unwrap()
                .target
                .role,
            SemanticRole::Checkbox
        );
    }

    #[test]
    fn recognized_text_reaches_shared_ocr_metadata() {
        let text = std::ffi::CString::new("复制 Copy").unwrap();
        let region = NativeRegion {
            x: 0.1,
            y: 0.5,
            width: 0.3,
            height: 0.03,
            confidence: 0.9,
            is_text: true,
            label: text.as_ptr().cast_mut(),
            label_len: text.as_bytes().len() as u64,
        };
        let target = classify(
            &region,
            Rect::new(0.0, 0.0, 1000.0, 800.0),
            &VisionOptions::default(),
        )
        .unwrap()
        .unwrap()
        .target;
        let mut scan = crate::platform::common::scan_accumulator::ScanAccumulator::new();
        let update = scan.push(TargetSource::NativeVision, vec![target], 0.5);
        let targets: Vec<_> = update
            .batches
            .into_iter()
            .flatten()
            .chain(scan.finish().into_iter().flatten())
            .collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].ocr_text(), "复制 Copy");
        assert_eq!(targets[0].accessibility_text(), "");
    }

    #[test]
    fn classifier_rejects_low_confidence_generic_rectangles() {
        let bounds = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let options = VisionOptions::default();
        let empty = std::ffi::CString::new("").unwrap();
        let region = NativeRegion {
            x: 0.1,
            y: 0.5,
            width: 0.04,
            height: 0.1,
            confidence: 0.1,
            is_text: false,
            label: empty.as_ptr().cast_mut(),
            label_len: 0,
        };
        assert!(classify(&region, bounds, &options).unwrap().is_none());
    }

    #[test]
    fn native_strings_reject_malformed_metadata_without_scanning_for_nul() {
        assert_eq!(native_string(std::ptr::null(), 0, 8).unwrap(), "");
        assert!(native_string(std::ptr::null(), 1, 8).is_err());
        assert!(native_string(std::ptr::null(), 9, 8).is_err());
        let bytes = b"hello";
        assert_eq!(
            native_string(bytes.as_ptr().cast(), bytes.len() as u64, 8).unwrap(),
            "hello"
        );
    }

    #[test]
    fn iou_merge_prefers_primary_targets() {
        let target = |x| UiTarget {
            details: None,
            rect: Rect::new(x, 0.0, 100.0, 100.0),
            name: String::new(),
            role: SemanticRole::Button,
        };
        assert_eq!(
            merge_targets(vec![target(0.0)], vec![target(10.0)], 0.5).len(),
            1
        );
        assert_eq!(
            merge_targets(vec![target(0.0)], vec![target(80.0)], 0.5).len(),
            2
        );
    }
}
