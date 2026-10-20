//! Capture geometry, image partitioning, and provider input construction.

use super::*;

use crate::platform::common::contour::input::Input as ContourInput;

pub(super) fn capture_geometries(
    request: &crate::api::command::UiScanRequest,
    bounds: Rect,
) -> Result<SmallVec<[CaptureGeometry; 2]>, String> {
    let regions: SmallVec<[Rect; 2]> =
        if request.scope == crate::api::UiScanScope::All && !request.screen_bounds.is_empty() {
            request.capture_regions().collect()
        } else {
            smallvec::smallvec![bounds]
        };
    if regions.is_empty() {
        return Err("visual capture has no display intersection".into());
    }
    regions.into_iter().map(capture_geometry).collect()
}

pub(super) fn capture_geometry(bounds: Rect) -> Result<CaptureGeometry, String> {
    if bounds.width < 2.0 || bounds.height < 2.0 {
        return Err("visual capture bounds are empty".into());
    }
    let edge_scale = (MAX_CAPTURE_EDGE / bounds.width.max(bounds.height)).min(1.0);
    let pixel_scale = (MAX_CAPTURE_PIXELS / (bounds.width * bounds.height))
        .sqrt()
        .min(1.0);
    let scale = edge_scale.min(pixel_scale);
    Ok(CaptureGeometry {
        width: (bounds.width * scale).round().max(2.0) as u32,
        height: (bounds.height * scale).round().max(2.0) as u32,
        desktop_bounds: bounds,
        scale,
    })
}

pub(super) use crate::platform::common::image_tiles::OcrLayout as SystemOcrLayout;

#[cfg(test)]
pub(super) use crate::platform::common::image_tiles::partition as scaled_partition;

pub(super) fn capture_pixel_rect(
    geometry: CaptureGeometry,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Rect {
    image_to_desktop(
        geometry,
        Rect::new(
            f64::from(x),
            f64::from(y),
            f64::from(width),
            f64::from(height),
        ),
    )
}

pub(super) fn wechat_full_frame_from_bgra(
    pixels: &[u8],
    geometry: CaptureGeometry,
    factory: &crate::platform::windows::native::SoftwareBitmapFactory,
) -> Result<WechatFullFrame, String> {
    let bitmap = factory.bgra(pixels, geometry.width, geometry.height)?;
    crate::support::perf_probe::mark("ocr_bitmap_ready");
    Ok(WechatFullFrame {
        geometry,
        bitmap: Some(bitmap),
        _ledger: crate::support::perf_probe::ResourceGuard::new(
            crate::support::perf_probe::ResourceKind::Bitmap,
        ),
    })
}

pub(super) fn submit_wechat_full_frame(
    input: &mut Option<WechatSubmission>,
    pixels: &[u8],
    geometry: CaptureGeometry,
    factory: Option<&crate::platform::windows::native::SoftwareBitmapFactory>,
    factory_error: Option<&str>,
    cancellation: &ScanCancellation,
) -> Result<(), String> {
    let Some(sender) = input.as_ref() else {
        return Ok(());
    };
    let sent = sender.send(
        || match factory.map(|factory| wechat_full_frame_from_bgra(pixels, geometry, factory)) {
            Some(Ok(frame)) => WechatInput::Frame(frame),
            Some(Err(error)) => WechatInput::Failed(error),
            None => WechatInput::Failed(
                factory_error
                    .unwrap_or("cannot create the COM apartment for the WeChat OCR bitmap")
                    .into(),
            ),
        },
        cancellation,
    )?;
    if !sent {
        input.take();
    }
    Ok(())
}

pub(super) fn system_ocr_tile_from_bgra(
    pixels: &[u8],
    geometry: CaptureGeometry,
    factory: &crate::platform::windows::native::SoftwareBitmapFactory,
    tile: crate::platform::common::image_tiles::Tile,
) -> Result<SystemOcrTile, String> {
    let bitmap_x = tile.left_halo as u32;
    let bitmap_y = tile.top_halo as u32;
    let bitmap_right = tile.right_halo as u32;
    let bitmap_bottom = tile.bottom_halo as u32;
    let tile_width = bitmap_right - bitmap_x;
    let tile_height = bitmap_bottom - bitmap_y;
    let bitmap = factory.bgra_region(
        pixels,
        geometry.width,
        geometry.height,
        bitmap_x,
        bitmap_y,
        tile_width,
        tile_height,
    )?;
    let desktop_bounds = capture_pixel_rect(geometry, bitmap_x, bitmap_y, tile_width, tile_height);
    Ok(SystemOcrTile {
        geometry: CaptureGeometry {
            width: tile_width,
            height: tile_height,
            desktop_bounds,
            scale: geometry.scale,
        },
        core_bounds: capture_pixel_rect(
            geometry,
            tile.x as u32,
            tile.y as u32,
            (tile.right - tile.x) as u32,
            (tile.bottom - tile.y) as u32,
        ),
        bitmap: SharedSoftwareBitmap {
            bitmap: Some(bitmap),
            _ledger: crate::support::perf_probe::ResourceGuard::new(
                crate::support::perf_probe::ResourceKind::Bitmap,
            ),
        },
    })
}

pub(super) fn stream_system_ocr_tiles(
    frames: &[CapturedFrame],
    factory: &crate::platform::windows::native::SoftwareBitmapFactory,
    submission: &SystemOcrSubmission,
    mut progress: impl FnMut(Option<usize>) -> Result<(), String>,
) -> Result<(), String> {
    for frame in frames {
        let expected = frame.geometry.width as usize * frame.geometry.height as usize * 4;
        if frame.capture.pixels().len() != expected {
            return Err("captured BGRA byte length does not match system OCR geometry".into());
        }
    }
    let pending = crate::platform::common::image_tiles::round_robin(frames.iter().map(|frame| {
        frame.layout.tiles(
            frame.geometry.width,
            frame.geometry.height,
            SYSTEM_OCR_TILE_OVERLAP as usize,
        )
    }));
    let tile_count = frames.iter().map(|frame| frame.layout.tile_count).sum();
    if submission
        .sender
        .send(SystemOcrInput::Begin { tile_count })
        .is_err()
    {
        return Ok(());
    }
    let mut in_flight = 0usize;
    for (submitted, (frame_index, tile)) in pending.enumerate() {
        let frame = &frames[frame_index];
        while in_flight == submission.max_in_flight {
            progress(None)?;
            let remaining = submission
                .deadline
                .saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("system OCR tile submission timed out".into());
            }
            match submission.credits.recv_timeout(remaining) {
                Ok(()) => {
                    in_flight -= 1;
                    progress(None)?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err("system OCR tile submission timed out".into());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        let tile =
            system_ocr_tile_from_bgra(frame.capture.pixels(), frame.geometry, factory, tile)?;
        if submission.sender.send(SystemOcrInput::Tile(tile)).is_err() {
            return Ok(());
        }
        in_flight += 1;
        progress(Some(submitted))?;
    }
    let _ = submission.sender.send(SystemOcrInput::Done);
    Ok(())
}

#[cfg(test)]
pub(super) fn contour_input_from_bgra(
    pixels: &[u8],
    geometry: CaptureGeometry,
) -> Result<ContourInput, String> {
    contour_input_from_bgra_with_progress(pixels, geometry, || Ok(false))?
        .ok_or_else(|| "contour grayscale conversion was cancelled".into())
}

pub(super) fn contour_input_from_bgra_with_progress(
    pixels: &[u8],
    geometry: CaptureGeometry,
    cancelled: impl FnMut() -> Result<bool, String>,
) -> Result<Option<ContourInput>, String> {
    crate::platform::common::contour::input::from_bgra(
        pixels,
        (
            geometry.width as usize,
            geometry.height as usize,
            geometry.width as usize * 4,
        ),
        geometry.desktop_bounds,
        cancelled,
    )
}
