//! DirectComposition/Direct2D overlay renderer.
//!
//! All COM interfaces and the HWND are owned by the render thread. Direct2D
//! draws straight into a DirectComposition surface, so this backend never
//! allocates a display-sized CPU pixel buffer.

use std::collections::HashMap;
use std::sync::Arc;

use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ROUNDED_RECT, D2D1CreateDevice, ID2D1DeviceContext, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionSurface, IDCompositionTarget,
    IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_HIT_TEST_METRICS,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, HCURSOR, HICON, SW_SHOWNOACTIVATE, ShowWindow, WNDCLASSEXW,
};
use windows::core::{Interface, PCWSTR, w};
use windows_numerics::Vector2;

use crate::api::geometry::{Point, Rect};
use crate::api::overlay::{
    Color, CursorMarker, Indicator, LabelStyle, LabelTextAnalysis, OverlayLabel, OverlayScene,
    OverlayShape,
};

use super::super::native::{
    self, NativeDimensions, OwnedWindow, OwnedWindowSpec, create_owned_window,
    reposition_owned_window,
};

const MAX_BRUSHES: usize = 64;
const MAX_TEXT_FORMATS: usize = 32;

/// Keep the unsafe DirectComposition creation contract in one typed boundary.
/// The returned interface is owned by COM and cannot outlive its device through
/// a borrowed raw pointer.
fn create_composition_visual(
    device: &IDCompositionDevice,
) -> windows::core::Result<IDCompositionVisual> {
    // SAFETY: the borrowed COM device is live for the synchronous call and the
    // method returns a separately reference-counted interface.
    unsafe { device.CreateVisual() }
}

fn clear_visual_content(visual: &IDCompositionVisual) -> windows::core::Result<()> {
    // SAFETY: clearing content does not retain a caller-owned pointer; the
    // visual remains owned by the active composition tree.
    unsafe { visual.SetContent(None) }
}

fn fill_rounded_rectangle(
    context: &ID2D1DeviceContext,
    rectangle: &D2D1_ROUNDED_RECT,
    brush: &ID2D1SolidColorBrush,
) {
    // SAFETY: all interfaces and geometry are borrowed for this synchronous
    // call, and callers invoke it only inside an active BeginDraw/EndDraw pair.
    unsafe { context.FillRoundedRectangle(rectangle, brush) };
}

fn draw_rounded_rectangle(
    context: &ID2D1DeviceContext,
    rectangle: &D2D1_ROUNDED_RECT,
    brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    // SAFETY: all interfaces and geometry are borrowed for this synchronous
    // call, and callers invoke it only inside an active BeginDraw/EndDraw pair.
    unsafe { context.DrawRoundedRectangle(rectangle, brush, width, None) };
}

/// A GPU-rendered overlay. COM interface wrappers release themselves; the
/// window wrapper below provides the same single-owner rule for HWND.
pub(super) struct GpuOverlay {
    paint_index: Option<Box<crate::platform::common::overlay_damage::PaintIndex>>,
    class_registered: bool,
    d2d: ID2D1DeviceContext,
    dcomp: IDCompositionDevice,
    dwrite: IDWriteFactory,
    content: Option<WindowContent>,
    brushes: HashMap<u32, ID2D1SolidColorBrush>,
    formats: Vec<FontEntry>,
    utf16: Vec<u16>,
    edit_layout: Option<EditLayout>,
    #[cfg(test)]
    begin_draws: usize,
}

struct EditLayout {
    text: crate::api::overlay::OverlayText,
    format: IDWriteTextFormat,
    width: f32,
    height: f32,
    trailing_text_len: u8,
    trailing_offsets: Option<(f64, f64)>,
    query_width: f64,
    layout: IDWriteTextLayout,
}

struct WindowContent {
    window: OwnedWindow,
    _target: IDCompositionTarget,
    root: IDCompositionVisual,
    static_visual: Option<IDCompositionVisual>,
    cursor_visual: IDCompositionVisual,
    indicator_visual: IDCompositionVisual,
    static_surface: Option<IDCompositionSurface>,
    cursor_surface: Option<LayerSurface>,
    indicator_surface: Option<LayerSurface>,
    last_scene: Option<Arc<OverlayScene>>,
    // Fast position updates leave the shared complete scene unchanged.
    positions: (Option<Point>, Option<Point>),
    area: Rect,
    width: u32,
    height: u32,
}

struct LayerSurface {
    surface: IDCompositionSurface,
    width: u32,
    height: u32,
}

struct FontEntry {
    family: String,
    size_bits: u64,
    bold: bool,
    alignment: crate::api::overlay::TextAlignment,
    wrap: bool,
    format: IDWriteTextFormat,
}

/// Owns the native draw pairing after a successful DirectComposition
/// `BeginDraw`. Cloned COM interfaces only adjust reference counts; they do not
/// allocate another device or surface. Drop covers every early return and panic.
struct SurfaceDrawGuard {
    d2d: ID2D1DeviceContext,
    surface: IDCompositionSurface,
    d2d_started: bool,
    clipped: bool,
    finished: bool,
}

impl SurfaceDrawGuard {
    fn new(d2d: &ID2D1DeviceContext, surface: &IDCompositionSurface) -> Self {
        Self {
            d2d: d2d.clone(),
            surface: surface.clone(),
            d2d_started: false,
            clipped: false,
            finished: false,
        }
    }

    fn begin_d2d(&mut self, bitmap: &windows::Win32::Graphics::Direct2D::ID2D1Bitmap1) {
        // SAFETY: the bitmap was created from this guard's active composition
        // surface and belongs to the same D2D device context.
        unsafe {
            self.d2d.SetTarget(bitmap);
            self.d2d.BeginDraw();
        }
        self.d2d_started = true;
    }

    fn finish(mut self) -> Result<(), String> {
        self.finish_inner()
    }

    fn finish_inner(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        // SAFETY: each flag is set only after its matching BeginDraw succeeds.
        // Marking finished first makes this exactly-once even when EndDraw
        // reports an error; the D2D target is always detached before return.
        let (d2d_result, surface_result) = unsafe {
            let d2d_result = if self.d2d_started {
                if self.clipped {
                    self.d2d.PopAxisAlignedClip();
                    self.clipped = false;
                }
                let result = self.d2d.EndDraw(None, None);
                self.d2d.SetTarget(None);
                result.map_err(|error| format!("Direct2D EndDraw failed: {error}"))
            } else {
                Ok(())
            };
            let surface_result = self
                .surface
                .EndDraw()
                .map_err(|error| format!("DirectComposition EndDraw failed: {error}"));
            (d2d_result, surface_result)
        };
        match (d2d_result, surface_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(d2d), Err(surface)) => Err(format!("{d2d}; {surface}")),
        }
    }
}

impl Drop for SurfaceDrawGuard {
    fn drop(&mut self) {
        if let Err(error) = self.finish_inner() {
            crate::report_error!("windows-overlay", "{error}");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SceneChanges {
    static_changed: bool,
    cursor_changed: bool,
    cursor_repaint: bool,
    indicator_changed: bool,
    indicator_repaint: bool,
}

impl GpuOverlay {
    pub(super) fn sample_aperture(&self, point: Option<Point>) -> Result<(), String> {
        self.content
            .as_ref()
            .map_or(Ok(()), |content| content.window.sample_aperture(point))
    }
    pub(super) const CLASS_NAME: PCWSTR = w!("KeySteerGpuOverlay");

    pub(super) fn new() -> Result<Self, String> {
        let mut d3d = None;
        // SAFETY: all out-pointers refer to initialized Options and the default
        // adapter is requested with a null module handle.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )
        }
        .map_err(|error| format!("D3D11CreateDevice failed: {error}"))?;
        let d3d: ID3D11Device = d3d.ok_or("D3D11 returned no device")?;
        let dxgi: IDXGIDevice = d3d
            .cast()
            .map_err(|error| format!("D3D11 device has no IDXGIDevice: {error}"))?;
        // SAFETY: `dxgi` is a live device owned for the renderer's lifetime.
        let d2d_device = unsafe { D2D1CreateDevice(&dxgi, None) }
            .map_err(|error| format!("D2D1CreateDevice failed: {error}"))?;
        // SAFETY: device creation is thread-affine and occurs on the renderer.
        let d2d = unsafe { d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }
            .map_err(|error| format!("CreateDeviceContext failed: {error}"))?;
        // SAFETY: DirectComposition accepts the same live DXGI device.
        let dcomp: IDCompositionDevice = unsafe { DCompositionCreateDevice(&dxgi) }
            .map_err(|error| format!("DCompositionCreateDevice failed: {error}"))?;
        // SAFETY: the requested interface matches the generic return type.
        let dwrite: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
            .map_err(|error| format!("DWriteCreateFactory failed: {error}"))?;
        Ok(Self {
            paint_index: None,
            class_registered: false,
            d2d,
            dcomp,
            dwrite,
            content: None,
            brushes: HashMap::with_capacity(16),
            formats: Vec::with_capacity(8),
            utf16: Vec::with_capacity(32),
            edit_layout: None,
            #[cfg(test)]
            begin_draws: 0,
        })
    }

    pub(super) fn present(&mut self, scene: Arc<OverlayScene>, area: Rect) -> Result<(), String> {
        if self.edit_layout.is_some()
            && !scene
                .labels
                .iter()
                .any(|label| label.edit.is_some() || label.scroll_to_cursor)
        {
            self.edit_layout = None;
        }
        let dimensions = NativeDimensions::from_f64(area.width, area.height)?;
        let width = dimensions.width_u32();
        let height = dimensions.height_u32();
        let area_changed = self.ensure_content(area, width, height)?;
        self.ensure_static_layer(scene_has_static_content(&scene), width, height)?;
        let (show, changes) = self
            .content
            .as_ref()
            .map(|content| {
                (
                    content.last_scene.is_none().then_some(content.window.raw()),
                    scene_changes(
                        content.last_scene.as_deref(),
                        scene.as_ref(),
                        content.positions,
                        area_changed,
                    ),
                )
            })
            .ok_or("DirectComposition content was not retained")?;
        if changes.static_changed {
            let static_surface = self
                .content
                .as_ref()
                .and_then(|content| content.static_surface.clone());
            if let Some(static_surface) = static_surface {
                let previous = self
                    .content
                    .as_ref()
                    .and_then(|content| content.last_scene.clone());
                if !area_changed
                    && let Some(previous) = previous
                        .as_deref()
                        .filter(|previous| scene.retains_static_content_from(previous))
                {
                    self.draw_damage(&static_surface, previous, &scene, area)?;
                } else {
                    self.draw_static(&static_surface, scene.as_ref(), area)?;
                }
            }
        }
        if changes.cursor_changed {
            if let Some(content) = self.content.as_mut() {
                content.positions.0 = None;
            }
            self.draw_cursor(scene.cursor_marker.as_ref(), area, changes.cursor_repaint)?;
            if let Some(content) = self.content.as_mut() {
                content.positions.0 = scene.cursor_marker.as_ref().map(|marker| marker.center);
            }
        }
        if changes.indicator_changed {
            if let Some(content) = self.content.as_mut() {
                content.positions.1 = None;
            }
            self.draw_indicator(scene.indicator.as_ref(), area, changes.indicator_repaint)?;
            if let Some(content) = self.content.as_mut() {
                content.positions.1 = scene.indicator.as_ref().map(|indicator| indicator.position);
            }
        }
        // A new HWND stays hidden until its first complete frame is committed.
        // Dismiss destroys the HWND, so later frames need no repeated show call.
        self.commit(show)?;
        if let Some(content) = self.content.as_mut() {
            content.last_scene = Some(scene);
        }
        Ok(())
    }

    /// Move existing dynamic visuals without opening a Direct2D surface.
    pub(super) fn update_positions(
        &mut self,
        cursor: Option<Point>,
        indicator: Option<Point>,
    ) -> Result<(), String> {
        let content = self
            .content
            .as_mut()
            .ok_or("DirectComposition content is unavailable")?;
        if let Some(center) = cursor {
            let marker = content
                .last_scene
                .as_deref()
                .and_then(|scene| scene.cursor_marker.as_ref())
                .ok_or("cursor visual has no complete scene")?;
            if content.cursor_surface.is_none() {
                return Err("cursor visual surface is unavailable".into());
            }
            let bounds = cursor_bounds(marker, center);
            // Retry after a partial native offset failure as well as after a
            // later complete scene returns to its original coordinates.
            content.positions.0 = None;
            configure_visual(
                &content.cursor_visual,
                None,
                bounds.x - content.area.x,
                bounds.y - content.area.y,
            )?;
            content.positions.0 = Some(center);
        }
        if let Some(position) = indicator {
            let item = content
                .last_scene
                .as_deref()
                .and_then(|scene| scene.indicator.as_ref())
                .ok_or("indicator visual has no complete scene")?;
            if content.indicator_surface.is_none() {
                return Err("indicator visual surface is unavailable".into());
            }
            let bounds = indicator_bounds(item, position);
            content.positions.1 = None;
            configure_visual(
                &content.indicator_visual,
                None,
                bounds.x - content.area.x,
                bounds.y - content.area.y,
            )?;
            content.positions.1 = Some(position);
        }
        self.commit(None)
    }

    fn commit(&self, show: Option<HWND>) -> Result<(), String> {
        // SAFETY: all visual mutations were recorded on `self.dcomp`; an HWND
        // supplied by `present` remains owned by `self.content`.
        unsafe {
            self.dcomp
                .Commit()
                .map_err(|error| format!("DirectComposition commit failed: {error}"))?;
            if let Some(hwnd) = show {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
        Ok(())
    }

    /// Release all screen-sized GPU resources immediately. Device and compact
    /// brush/font caches remain warm for the next mode entry. The commit is
    /// intentionally asynchronous: capture callers add the single DWM barrier
    /// needed before reading desktop pixels, while an ordinary mode exit must
    /// not wait for the compositor.
    pub(super) fn dismiss(&mut self) -> Result<(), String> {
        self.paint_index = None;
        self.edit_layout = None;
        // Keep the ordinary warm text buffer, but release exceptional title peaks
        // when leaving the overlay, outside the render hot path.
        if self.utf16.capacity() > 4096 {
            self.utf16 = Vec::new();
        }
        if self.content.is_none() {
            return Ok(());
        }
        // SAFETY: detaching a target is valid outside BeginDraw. Clearing the
        // owned tree before Commit releases every compositor reference.
        unsafe {
            self.d2d.SetTarget(None);
            self.content = None;
            self.dcomp
                .Commit()
                .map_err(|error| format!("DirectComposition dismiss commit failed: {error}"))
        }
    }

    /// Ensure the compositor tree exists and report whether its screen origin
    /// changed. A stable frame must not issue a full-screen SetWindowPos.
    fn ensure_content(&mut self, area: Rect, width: u32, height: u32) -> Result<bool, String> {
        if let Some(content) = self.content.as_mut()
            && content.width == width
            && content.height == height
        {
            let area_changed = content.area != area;
            if area_changed {
                reposition_owned_window(&content.window, area)?;
                content.area = area;
            }
            return Ok(area_changed);
        }

        self.ensure_class()?;
        let window = create_window(area)?;
        // SAFETY: the HWND and DirectComposition device are both live and
        // owned by this render thread.
        let target = unsafe { self.dcomp.CreateTargetForHwnd(window.raw(), true) }
            .map_err(|error| format!("CreateTargetForHwnd failed: {error}"))?;
        // SAFETY: device methods return owned COM interfaces.
        let visual = create_composition_visual(&self.dcomp)
            .map_err(|error| format!("CreateVisual failed: {error}"))?;
        // SAFETY: every interface belongs to this device and stays alive in
        // WindowContent. Use an explicit sibling when placing the indicator above
        // the cursor; with a null sibling, insertAbove=true means bottommost.
        let cursor_visual = create_composition_visual(&self.dcomp)
            .map_err(|error| format!("Create cursor visual failed: {error}"))?;
        // SAFETY: the compositor device returns a newly owned visual on this
        // renderer thread.
        let indicator_visual = create_composition_visual(&self.dcomp)
            .map_err(|error| format!("Create indicator visual failed: {error}"))?;
        // SAFETY: all visuals and the target belong to `self.dcomp`, remain
        // alive in WindowContent, and are attached outside BeginDraw.
        unsafe {
            visual
                .AddVisual(&cursor_visual, false, None)
                .and_then(|()| visual.AddVisual(&indicator_visual, true, Some(&cursor_visual)))
                .and_then(|()| target.SetRoot(&visual))
                .and_then(|()| self.dcomp.Commit())
        }
        .map_err(|error| format!("cannot attach DirectComposition visual: {error}"))?;
        self.content = Some(WindowContent {
            window,
            _target: target,
            root: visual,
            static_visual: None,
            cursor_visual,
            indicator_visual,
            static_surface: None,
            cursor_surface: None,
            indicator_surface: None,
            last_scene: None,
            positions: (None, None),
            area,
            width,
            height,
        });
        Ok(true)
    }

    fn ensure_static_layer(
        &mut self,
        required: bool,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if !required {
            self.paint_index = None;
        }
        let Some(content) = self.content.as_mut() else {
            return Err("DirectComposition content was not retained".into());
        };
        if required && content.static_surface.is_none() {
            let (surface, visual) = create_static_layer(
                &self.dcomp,
                &content.root,
                &content.cursor_visual,
                width,
                height,
            )?;
            content.static_surface = Some(surface);
            content.static_visual = Some(visual);
        } else if !required && let Some(visual) = content.static_visual.take() {
            // SAFETY: the visual is currently a child of this root and drawing
            // is not active. Removing it before dropping the surface releases
            // the screen-sized compositor allocation.
            unsafe { content.root.RemoveVisual(&visual) }
                .map_err(|error| format!("cannot detach static visual: {error}"))?;
            content.static_surface = None;
        }
        Ok(())
    }

    fn ensure_class(&mut self) -> Result<(), String> {
        if self.class_registered {
            return Ok(());
        }
        // SAFETY: a null module name asks for the current executable.
        let instance = native::current_module()
            .map_err(|error| format!("GetModuleHandleW failed: {error}"))?;
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: Self::CLASS_NAME,
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(),
            hbrBackground: HBRUSH::default(),
            ..Default::default()
        };
        native::register_window_class(&class)?;
        self.class_registered = true;
        Ok(())
    }

    fn draw_surface(
        &mut self,
        surface: &IDCompositionSurface,
        coordinate_origin: Point,
        clear: Option<Color>,
        draw: impl FnOnce(&mut Self, Point) -> Result<(), String>,
    ) -> Result<(), String> {
        self.draw_surface_region(surface, coordinate_origin, clear, None, draw)
    }

    fn draw_surface_region(
        &mut self,
        surface: &IDCompositionSurface,
        coordinate_origin: Point,
        clear: Option<Color>,
        update: Option<RECT>,
        draw: impl FnOnce(&mut Self, Point) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut offset = POINT::default();
        // SAFETY: the update offset is a valid out-parameter and the returned
        // interface is explicitly requested as IDXGISurface.
        let dxgi: IDXGISurface =
            unsafe { surface.BeginDraw(update.as_ref().map(std::ptr::from_ref), &mut offset) }
                .map_err(|error| format!("DirectComposition BeginDraw failed: {error}"))?;
        let mut draw_guard = SurfaceDrawGuard::new(&self.d2d, surface);
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            ..Default::default()
        };
        // SAFETY: `dxgi` is the surface returned by BeginDraw and properties
        // describe that surface's exact format.
        let bitmap = unsafe {
            self.d2d
                .CreateBitmapFromDxgiSurface(&dxgi, Some(&properties))
        }
        .map_err(|error| format!("CreateBitmapFromDxgiSurface failed: {error}"))?;
        #[cfg(test)]
        {
            self.begin_draws = self.begin_draws.wrapping_add(1);
        }
        draw_guard.begin_d2d(&bitmap);
        let origin = Point::new(
            coordinate_origin.x + update.map_or(0.0, |rect| f64::from(rect.left))
                - f64::from(offset.x),
            coordinate_origin.y + update.map_or(0.0, |rect| f64::from(rect.top))
                - f64::from(offset.y),
        );
        let clip = update.map(|update| D2D_RECT_F {
            left: offset.x as f32,
            top: offset.y as f32,
            right: (offset.x + update.right - update.left) as f32,
            bottom: (offset.y + update.bottom - update.top) as f32,
        });
        let clear = clear.map(d2d_color);
        // SAFETY: drawing is active and the optional local color stays live
        // for the duration of the call.
        unsafe {
            if let Some(clip) = &clip {
                self.d2d
                    .PushAxisAlignedClip(clip, D2D1_ANTIALIAS_MODE_ALIASED);
                draw_guard.clipped = true;
            }
            self.d2d.Clear(clear.as_ref().map(std::ptr::from_ref));
        };
        let draw_result = draw(self, origin);
        draw_result.and(draw_guard.finish())
    }

    fn draw_static(
        &mut self,
        surface: &IDCompositionSurface,
        scene: &OverlayScene,
        area: Rect,
    ) -> Result<(), String> {
        // Full draws do not query the spatial index. Keep a matching one warm,
        // but build it only when a later frame actually needs damage queries.
        self.paint_index = self.paint_index.take().filter(|index| {
            scene
                .retained
                .as_ref()
                .is_some_and(|retained| Arc::ptr_eq(&index.source, &retained.scene))
        });
        self.draw_surface(
            surface,
            Point::new(area.x, area.y),
            scene.backdrop,
            |renderer, origin| {
                if let Some(retained) = &scene.retained {
                    for shape in &retained.scene.shapes {
                        renderer.draw_shape(shape, origin)?;
                    }
                    for (i, label) in retained.scene.labels.iter().enumerate() {
                        if retained.omitted_label != Some(i) {
                            renderer.draw_label(label, origin)?;
                        }
                    }
                }
                for shape in &scene.shapes {
                    renderer.draw_shape(shape, origin)?;
                }
                for label in &scene.labels {
                    renderer.draw_label(label, origin)?;
                }
                Ok(())
            },
        )
    }

    fn draw_damage(
        &mut self,
        surface: &IDCompositionSurface,
        previous: &OverlayScene,
        scene: &OverlayScene,
        area: Rect,
    ) -> Result<(), String> {
        use crate::platform::common::overlay_damage::{damage, label_bounds, shape_bounds};
        let Some(retained) = scene.retained.as_ref() else {
            return self.draw_static(surface, scene, area);
        };
        let regions = damage(previous, scene, area);
        if regions.is_empty() {
            return Ok(());
        }
        let mut index = self
            .paint_index
            .take()
            .filter(|index| Arc::ptr_eq(&index.source, &retained.scene))
            .unwrap_or_else(|| {
                Box::new(crate::platform::common::overlay_damage::PaintIndex::new(
                    retained.scene.clone(),
                ))
            });
        let result = (|| {
            for region in regions {
                let update = RECT {
                    left: (region.x - area.x).floor().max(0.0) as i32,
                    top: (region.y - area.y).floor().max(0.0) as i32,
                    right: (region.right() - area.x).ceil().min(area.width.round()) as i32,
                    bottom: (region.bottom() - area.y).ceil().min(area.height.round()) as i32,
                };
                if update.left >= update.right || update.top >= update.bottom {
                    continue;
                }
                let region = Rect::new(
                    area.x + f64::from(update.left),
                    area.y + f64::from(update.top),
                    f64::from(update.right - update.left),
                    f64::from(update.bottom - update.top),
                );
                index.query(region);
                self.draw_surface_region(
                    surface,
                    Point::new(area.x, area.y),
                    scene.backdrop,
                    Some(update),
                    |renderer, origin| {
                        let base = &retained.scene;
                        for &hit in &index.hits {
                            if hit < base.shapes.len() {
                                renderer.draw_shape(&base.shapes[hit], origin)?;
                            } else {
                                let i = hit - base.shapes.len();
                                if retained.omitted_label != Some(i) {
                                    renderer.draw_label(&base.labels[i], origin)?;
                                }
                            }
                        }
                        for shape in &scene.shapes {
                            if shape_bounds(shape).intersect(&region).is_some() {
                                renderer.draw_shape(shape, origin)?;
                            }
                        }
                        for label in &scene.labels {
                            if label_bounds(label).intersect(&region).is_some() {
                                renderer.draw_label(label, origin)?;
                            }
                        }
                        Ok(())
                    },
                )?;
            }
            Ok(())
        })();
        self.paint_index = Some(index);
        result
    }

    fn draw_cursor(
        &mut self,
        marker: Option<&CursorMarker>,
        area: Rect,
        repaint: bool,
    ) -> Result<(), String> {
        let Some(marker) = marker else {
            if let Some(content) = self.content.as_mut() {
                // SAFETY: clearing content releases the visual's surface.
                clear_visual_content(&content.cursor_visual)
                    .map_err(|error| format!("cannot clear cursor visual: {error}"))?;
                content.cursor_surface = None;
            }
            return Ok(());
        };
        let bounds = cursor_bounds(marker, marker.center);
        let width = dimension(bounds.width)?;
        let height = dimension(bounds.height)?;
        let surface_to_paint = {
            let dcomp = &self.dcomp;
            let content = self
                .content
                .as_mut()
                .ok_or("DirectComposition content is unavailable")?;
            let recreated =
                ensure_layer_surface(dcomp, &mut content.cursor_surface, width, height)?;
            let layer = content
                .cursor_surface
                .as_ref()
                .ok_or("DirectComposition cursor surface was not retained")?;
            let bind = recreated.then(|| layer.surface.clone());
            configure_visual(
                &content.cursor_visual,
                bind.as_ref(),
                bounds.x - area.x,
                bounds.y - area.y,
            )?;
            (repaint || recreated).then(|| layer.surface.clone())
        };
        if let Some(surface) = surface_to_paint {
            self.draw_surface(
                &surface,
                Point::new(bounds.x, bounds.y),
                None,
                |renderer, origin| {
                    renderer.draw_shape(
                        &OverlayShape::Rect {
                            rect: Rect::new(
                                marker.center.x - marker.radius,
                                marker.center.y - marker.radius,
                                marker.radius * 2.0,
                                marker.radius * 2.0,
                            ),
                            fill: marker.fill,
                            stroke: marker.stroke,
                            stroke_width: marker.stroke_width,
                            corner_radius: marker.radius,
                            z_index: i32::MAX,
                        },
                        origin,
                    )
                },
            )?;
        }
        Ok(())
    }

    fn draw_indicator(
        &mut self,
        indicator: Option<&Indicator>,
        area: Rect,
        repaint: bool,
    ) -> Result<(), String> {
        let Some(indicator) = indicator else {
            if let Some(content) = self.content.as_mut() {
                // SAFETY: clearing content releases the visual's surface.
                clear_visual_content(&content.indicator_visual)
                    .map_err(|error| format!("cannot clear indicator visual: {error}"))?;
                content.indicator_surface = None;
            }
            return Ok(());
        };
        let first = indicator_rect(&indicator.text, indicator.position, &indicator.style);
        let held = indicator.held_text.as_deref().map(|text| {
            indicator_rect(
                text,
                Point::new(indicator.position.x, first.bottom() + 4.0),
                &indicator.style,
            )
        });
        let bounds = indicator_bounds(indicator, indicator.position);
        let width = dimension(bounds.width)?;
        let height = dimension(bounds.height)?;
        let surface_to_paint = {
            let dcomp = &self.dcomp;
            let content = self
                .content
                .as_mut()
                .ok_or("DirectComposition content is unavailable")?;
            let recreated =
                ensure_layer_surface(dcomp, &mut content.indicator_surface, width, height)?;
            let layer = content
                .indicator_surface
                .as_ref()
                .ok_or("DirectComposition indicator surface was not retained")?;
            let bind = recreated.then(|| layer.surface.clone());
            configure_visual(
                &content.indicator_visual,
                bind.as_ref(),
                bounds.x - area.x,
                bounds.y - area.y,
            )?;
            (repaint || recreated).then(|| layer.surface.clone())
        };
        if let Some(surface) = surface_to_paint {
            self.draw_surface(
                &surface,
                Point::new(bounds.x, bounds.y),
                None,
                |renderer, origin| {
                    renderer.draw_label_parts(
                        &indicator.text,
                        first,
                        &indicator.style,
                        LabelTextAnalysis::analyze(&indicator.text, 0),
                        origin,
                        (None, false, 0),
                    )?;
                    if let (Some(text), Some(rect)) = (&indicator.held_text, held) {
                        renderer.draw_label_parts(
                            text,
                            rect,
                            &indicator.style,
                            LabelTextAnalysis::analyze(text, 0),
                            origin,
                            (None, false, 0),
                        )?;
                    }
                    Ok(())
                },
            )?;
        }
        Ok(())
    }

    fn draw_shape(&mut self, shape: &OverlayShape, origin: Point) -> Result<(), String> {
        match shape {
            OverlayShape::Rect {
                rect,
                fill,
                stroke,
                stroke_width,
                corner_radius,
                ..
            } => {
                let rect = local_rect(*rect, origin);
                let rounded = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: *corner_radius as f32,
                    radiusY: *corner_radius as f32,
                };
                if !fill.is_transparent() {
                    let brush = self.brush(*fill)?;
                    // SAFETY: drawing is active and both pointers are live.
                    fill_rounded_rectangle(&self.d2d, &rounded, &brush);
                }
                if !stroke.is_transparent() && *stroke_width > 0.0 {
                    let brush = self.brush(*stroke)?;
                    // SAFETY: drawing is active and both pointers are live.
                    draw_rounded_rectangle(&self.d2d, &rounded, &brush, *stroke_width as f32);
                }
            }
            OverlayShape::Line {
                from,
                to,
                color,
                width,
                ..
            } if !color.is_transparent() && *width > 0.0 => {
                let brush = self.brush(*color)?;
                let from = local_point(*from, origin);
                let to = local_point(*to, origin);
                // SAFETY: drawing is active and the brush belongs to this
                // device context.
                unsafe { self.d2d.DrawLine(from, to, &brush, *width as f32, None) };
            }
            OverlayShape::Line { .. } => {}
        }
        Ok(())
    }

    fn draw_label(&mut self, label: &OverlayLabel, origin: Point) -> Result<(), String> {
        self.draw_label_parts(
            &label.text,
            label.rect,
            &label.style,
            label.text_analysis(),
            origin,
            (
                label.edit.map(Into::into),
                label.scroll_to_cursor,
                label.trailing_text_len,
            ),
        )
    }

    fn draw_label_parts(
        &mut self,
        text: &str,
        label_rect: Rect,
        style: &LabelStyle,
        analysis: LabelTextAnalysis,
        origin: Point,
        (edit, scroll_to_cursor, trailing_text_len): (
            Option<crate::api::text_edit::Selection>,
            bool,
            u8,
        ),
    ) -> Result<(), String> {
        let mut rect = local_rect(label_rect, origin);
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: style.border_radius as f32,
            radiusY: style.border_radius as f32,
        };
        if !style.background.is_transparent() {
            let brush = self.brush(style.background)?;
            // SAFETY: drawing is active and both objects are live.
            fill_rounded_rectangle(&self.d2d, &rounded, &brush);
        }
        if !style.border_color.is_transparent() && style.border_width > 0.0 {
            let brush = self.brush(style.border_color)?;
            // SAFETY: drawing is active and both objects are live.
            draw_rounded_rectangle(&self.d2d, &rounded, &brush, style.border_width as f32);
        }
        let text_offset_y = super::label_text_offset_y(style, analysis, scroll_to_cursor) as f32;
        rect.top += text_offset_y;
        rect.bottom += text_offset_y;
        let format = self.text_format(style)?;
        let normal = self.brush(style.text_color)?;
        let prefix_utf16_len = analysis.matched_utf16_len;
        let draw_options = if style.text_alignment == crate::api::overlay::TextAlignment::Left {
            D2D1_DRAW_TEXT_OPTIONS_CLIP
        } else {
            D2D1_DRAW_TEXT_OPTIONS_NONE
        };
        if edit.is_some()
            || scroll_to_cursor
            || (prefix_utf16_len > 0 && !style.matched_text_color.is_transparent())
        {
            let matched_brush = self.brush(style.matched_text_color)?;
            let mut layout_origin = Vector2 {
                X: rect.left,
                Y: rect.top,
            };
            let (query, trailing) =
                crate::api::overlay::split_trailing_text(text, trailing_text_len);
            let trailing_utf16_len = trailing.encode_utf16().count();
            // SAFETY: UTF-16 data, format, brushes and resulting COM layout
            // remain live through the draw. The text range was derived from
            // the same UTF-16 buffer and cannot exceed it.
            unsafe {
                // Keep layout dimensions independent of the compositor's
                // changing surface origin and the optical baseline offset.
                let width = label_rect.width.max(1.0) as f32;
                let height = label_rect.height.max(1.0) as f32;
                let measure = |layout: &IDWriteTextLayout, position: u32| -> Result<f64, String> {
                    let (mut x, mut y) = (0.0, 0.0);
                    let mut metrics = DWRITE_HIT_TEST_METRICS::default();
                    layout
                        .HitTestTextPosition(position, false, &mut x, &mut y, &mut metrics)
                        .map_err(|error| format!("Measure edit position failed: {error}"))?;
                    Ok(f64::from(x))
                };
                let cached = self.edit_layout.as_ref().filter(|cache| {
                    (edit.is_some() || scroll_to_cursor)
                        && cache.text == text
                        && cache.format == format
                        && cache.width == width
                        && cache.height == height
                        && cache.trailing_text_len == trailing_text_len
                });
                let (layout, trailing_offsets, query_width) = if let Some(cached) = cached {
                    (
                        cached.layout.clone(),
                        cached.trailing_offsets,
                        cached.query_width,
                    )
                } else {
                    self.utf16.clear();
                    self.utf16.extend(text.encode_utf16());
                    let layout = self
                        .dwrite
                        .CreateTextLayout(&self.utf16, &format, width, height)
                        .map_err(|error| format!("CreateTextLayout failed: {error}"))?;
                    let trailing_offsets = if trailing.is_empty() {
                        None
                    } else {
                        Some((
                            measure(&layout, (analysis.utf16_len - trailing_utf16_len) as u32)?,
                            measure(&layout, analysis.utf16_len as u32)?,
                        ))
                    };
                    let query_width = crate::api::text_edit::query_width(
                        f64::from(width),
                        style.font_size,
                        trailing_offsets.map_or(0.0, |(query_end, full_end)| full_end - query_end),
                    );
                    if edit.is_some() || scroll_to_cursor {
                        self.edit_layout = Some(EditLayout {
                            text: text.into(),
                            format: format.clone(),
                            width,
                            height,
                            trailing_text_len,
                            trailing_offsets,
                            query_width,
                            layout: layout.clone(),
                        });
                    }
                    (layout, trailing_offsets, query_width)
                };
                if !trailing.is_empty() {
                    layout
                        .SetDrawingEffect(
                            &matched_brush,
                            DWRITE_TEXT_RANGE {
                                startPosition: (analysis.utf16_len - trailing_utf16_len) as u32,
                                length: trailing_utf16_len as u32,
                            },
                        )
                        .map_err(|error| format!("Set counter color failed: {error}"))?;
                }
                if prefix_utf16_len > 0 {
                    layout
                        .SetDrawingEffect(
                            &matched_brush,
                            DWRITE_TEXT_RANGE {
                                startPosition: 0,
                                length: prefix_utf16_len as u32,
                            },
                        )
                        .map_err(|error| format!("SetDrawingEffect failed: {error}"))?;
                }
                let offset = |byte: usize| -> Result<f64, String> {
                    if text.is_empty() {
                        return Ok(style.text_alignment.offset(f64::from(width), 0.0));
                    }
                    if let Some((query_end, full_end)) = trailing_offsets {
                        if byte == query.len() {
                            return Ok(query_end);
                        }
                        if byte >= text.len() {
                            return Ok(full_end);
                        }
                    }
                    let position = if byte >= text.len() {
                        analysis.utf16_len as u32
                    } else if byte == query.len() {
                        (analysis.utf16_len - trailing_utf16_len) as u32
                    } else {
                        text.get(..byte).unwrap_or(text).encode_utf16().count() as u32
                    };
                    measure(&layout, position)
                };
                let cursor_x = if edit.is_some() || scroll_to_cursor {
                    offset(edit.map_or(query.len(), |edit| edit.cursor))?
                } else {
                    0.0
                };
                let full_end = trailing_offsets.map_or(0.0, |(_, full_end)| full_end);
                let trailing_width =
                    trailing_offsets.map_or(0.0, |(query_end, full_end)| full_end - query_end);
                let scroll = if scroll_to_cursor {
                    crate::api::text_edit::scroll_offset(query_width, style.font_size, cursor_x)
                } else {
                    0.0
                };
                layout_origin.X -= scroll as f32;
                let mut query_rect = rect;
                // The same layout contains the counter. Short queries must
                // clip at their own end so the suffix is painted only on the right.
                query_rect.right = query_rect.left
                    + trailing_offsets.map_or(query_width, |(query_end, _)| {
                        (query_end - scroll).max(0.0).min(query_width)
                    }) as f32;
                if let Some(edit) = edit {
                    let (caret, selection) = crate::api::text_edit::decoration_rects(
                        Rect::new(
                            rect.left as f64,
                            rect.top as f64,
                            query_width,
                            height as f64,
                        ),
                        style.font_size,
                        cursor_x - scroll,
                        offset(edit.anchor)? - scroll,
                    );
                    if let Some(selection) = selection {
                        let brush = self.brush(style.matched_text_color.with_opacity(0.25))?;
                        self.d2d
                            .FillRectangle(&local_rect(selection, Point::new(0.0, 0.0)), &brush);
                    }
                    self.d2d
                        .FillRectangle(&local_rect(caret, Point::new(0.0, 0.0)), &normal);
                }
                if scroll_to_cursor {
                    self.d2d
                        .PushAxisAlignedClip(&query_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
                self.d2d.DrawTextLayout(
                    layout_origin,
                    &layout,
                    &normal,
                    if scroll_to_cursor {
                        D2D1_DRAW_TEXT_OPTIONS_NONE
                    } else {
                        draw_options
                    },
                );
                if scroll_to_cursor {
                    self.d2d.PopAxisAlignedClip();
                }
                if !trailing.is_empty() {
                    let mut counter_rect = rect;
                    counter_rect.left = (counter_rect.right - trailing_width as f32).max(rect.left);
                    self.d2d
                        .PushAxisAlignedClip(&counter_rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    layout_origin.X = rect.right - full_end as f32;
                    self.d2d.DrawTextLayout(
                        layout_origin,
                        &layout,
                        &normal,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                    self.d2d.PopAxisAlignedClip();
                }
            }
        } else {
            self.utf16.clear();
            self.utf16.extend(text.encode_utf16());
            // SAFETY: UTF-16 data, format, brush and layout rectangle remain
            // live for the complete call.
            unsafe {
                self.d2d.DrawText(
                    &self.utf16,
                    &format,
                    &rect,
                    &normal,
                    draw_options,
                    DWRITE_MEASURING_MODE_NATURAL,
                )
            };
        }
        Ok(())
    }

    fn brush(&mut self, color: Color) -> Result<ID2D1SolidColorBrush, String> {
        let key = color_key(color);
        if let Some(brush) = self.brushes.get(&key) {
            return Ok(brush.clone());
        }
        if self.brushes.len() >= MAX_BRUSHES {
            self.brushes.clear();
        }
        let color = d2d_color(color);
        // SAFETY: the color pointer is valid and default brush properties are
        // requested. The returned COM interface is owned.
        let brush = unsafe { self.d2d.CreateSolidColorBrush(&color, None) }
            .map_err(|error| format!("CreateSolidColorBrush failed: {error}"))?;
        self.brushes.insert(key, brush.clone());
        Ok(brush)
    }

    fn text_format(&mut self, style: &LabelStyle) -> Result<IDWriteTextFormat, String> {
        let size_bits = style.font_size.to_bits();
        if let Some(entry) = self.formats.iter().find(|entry| {
            entry.family == style.font_family
                && entry.size_bits == size_bits
                && entry.bold == style.bold
                && entry.alignment == style.text_alignment
                && entry.wrap == style.wrap
        }) {
            return Ok(entry.format.clone());
        }
        if self.formats.len() >= MAX_TEXT_FORMATS {
            self.formats.clear();
        }
        let family = if style.font_family.is_empty() {
            "Segoe UI"
        } else {
            &style.font_family
        };
        let mut family_wide: Vec<u16> = family.encode_utf16().chain([0]).collect();
        let locale = w!("en-us");
        // SAFETY: both NUL-terminated strings and resolved dimensions are valid.
        // The factory owns the newly returned format; configuration uses documented
        // enums and SetTrimming retains the factory-owned ellipsis sign.
        let format = unsafe {
            let format = self
                .dwrite
                .CreateTextFormat(
                    PCWSTR(family_wide.as_mut_ptr()),
                    None,
                    if style.bold {
                        DWRITE_FONT_WEIGHT_BOLD
                    } else {
                        DWRITE_FONT_WEIGHT_NORMAL
                    },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.font_size.max(1.0) as f32,
                    locale,
                )
                .map_err(|error| format!("CreateTextFormat failed: {error}"))?;
            format
                .SetTextAlignment(match style.text_alignment {
                    crate::api::overlay::TextAlignment::Left => {
                        windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING
                    }
                    crate::api::overlay::TextAlignment::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                    crate::api::overlay::TextAlignment::Right => {
                        windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_TRAILING
                    }
                })
                .and_then(|()| {
                    format.SetParagraphAlignment(if style.wrap {
                        windows::Win32::Graphics::DirectWrite::DWRITE_PARAGRAPH_ALIGNMENT_NEAR
                    } else {
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER
                    })
                })
                .and_then(|()| {
                    format.SetWordWrapping(if style.wrap {
                        windows::Win32::Graphics::DirectWrite::DWRITE_WORD_WRAPPING_WRAP
                    } else {
                        DWRITE_WORD_WRAPPING_NO_WRAP
                    })
                })
                .map_err(|error| format!("cannot configure DirectWrite format: {error}"))?;
            if style.wrap {
                use windows::Win32::Graphics::DirectWrite::{
                    DWRITE_LINE_SPACING_METHOD_UNIFORM, DWRITE_TRIMMING,
                    DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                };
                format
                    .SetLineSpacing(
                        DWRITE_LINE_SPACING_METHOD_UNIFORM,
                        (style.font_size * 1.4) as f32,
                        style.font_size as f32,
                    )
                    .map_err(|error| format!("Set paragraph spacing failed: {error}"))?;
                let ellipsis = self
                    .dwrite
                    .CreateEllipsisTrimmingSign(&format)
                    .map_err(|error| format!("Create paragraph ellipsis failed: {error}"))?;
                format
                    .SetTrimming(
                        &DWRITE_TRIMMING {
                            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                            ..Default::default()
                        },
                        &ellipsis,
                    )
                    .map_err(|error| format!("Set paragraph trimming failed: {error}"))?;
            }
            format
        };
        self.formats.push(FontEntry {
            family: style.font_family.clone(),
            size_bits,
            bold: style.bold,
            alignment: style.text_alignment,
            wrap: style.wrap,
            format: format.clone(),
        });
        Ok(format)
    }
}

fn create_static_layer(
    device: &IDCompositionDevice,
    root: &IDCompositionVisual,
    cursor: &IDCompositionVisual,
    width: u32,
    height: u32,
) -> Result<(IDCompositionSurface, IDCompositionVisual), String> {
    // SAFETY: dimensions were validated before content creation. All returned
    // interfaces and both existing visuals belong to this compositor device
    // and render thread; attachment occurs outside an active BeginDraw.
    unsafe {
        let surface = device
            .CreateSurface(
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_ALPHA_MODE_PREMULTIPLIED,
            )
            .map_err(|error| format!("CreateSurface failed: {error}"))?;
        let visual = device
            .CreateVisual()
            .map_err(|error| format!("Create static visual failed: {error}"))?;
        visual
            .SetContent(&surface)
            .and_then(|()| root.AddVisual(&visual, false, Some(cursor)))
            .map_err(|error| format!("cannot attach static visual: {error}"))?;
        Ok((surface, visual))
    }
}

fn ensure_layer_surface(
    device: &IDCompositionDevice,
    slot: &mut Option<LayerSurface>,
    width: u32,
    height: u32,
) -> Result<bool, String> {
    if let Some(layer) = slot
        && layer.width == width
        && layer.height == height
    {
        return Ok(false);
    }
    // SAFETY: dimensions are positive and the format matches the renderer's
    // Direct2D bitmap target.
    let surface = unsafe {
        device.CreateSurface(
            width,
            height,
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )
    }
    .map_err(|error| format!("cannot create tight DirectComposition surface: {error}"))?;
    *slot = Some(LayerSurface {
        surface,
        width,
        height,
    });
    Ok(true)
}

fn static_scene_eq(left: &OverlayScene, right: &OverlayScene) -> bool {
    left.backdrop == right.backdrop
        && left.retained == right.retained
        && left.clip == right.clip
        && left.shapes == right.shapes
        && left.labels == right.labels
}

fn cursor_content_eq(left: Option<&CursorMarker>, right: Option<&CursorMarker>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.radius == right.radius
                && left.fill == right.fill
                && left.stroke == right.stroke
                && left.stroke_width == right.stroke_width
        }
        (None, None) => true,
        _ => false,
    }
}

fn indicator_content_eq(left: Option<&Indicator>, right: Option<&Indicator>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.text == right.text
                && left.held_text == right.held_text
                && left.style == right.style
        }
        (None, None) => true,
        _ => false,
    }
}

fn scene_changes(
    previous: Option<&OverlayScene>,
    next: &OverlayScene,
    positions: (Option<Point>, Option<Point>),
    area_changed: bool,
) -> SceneChanges {
    let cursor_repaint = previous.is_none_or(|previous| {
        !cursor_content_eq(previous.cursor_marker.as_ref(), next.cursor_marker.as_ref())
    });
    let indicator_repaint = previous.is_none_or(|previous| {
        !indicator_content_eq(previous.indicator.as_ref(), next.indicator.as_ref())
    });
    SceneChanges {
        static_changed: area_changed
            || previous.is_none_or(|previous| !static_scene_eq(previous, next)),
        cursor_changed: area_changed
            || cursor_repaint
            || positions.0 != next.cursor_marker.as_ref().map(|marker| marker.center),
        cursor_repaint,
        indicator_changed: area_changed
            || indicator_repaint
            || positions.1 != next.indicator.as_ref().map(|indicator| indicator.position),
        indicator_repaint,
    }
}

fn scene_has_static_content(scene: &OverlayScene) -> bool {
    scene.retained.is_some()
        || scene.backdrop.is_some()
        || !scene.shapes.is_empty()
        || !scene.labels.is_empty()
}

fn create_window(area: Rect) -> Result<OwnedWindow, String> {
    create_owned_window(OwnedWindowSpec::GpuOverlay(area))
}

fn dimension(value: f64) -> Result<u32, String> {
    NativeDimensions::from_f64(value, 1.0).map(NativeDimensions::width_u32)
}

fn d2d_color(color: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
}

const fn color_key(color: Color) -> u32 {
    u32::from_be_bytes([color.r, color.g, color.b, color.a])
}

fn local_rect(rect: Rect, origin: Point) -> D2D_RECT_F {
    D2D_RECT_F {
        left: (rect.x - origin.x) as f32,
        top: (rect.y - origin.y) as f32,
        right: (rect.right() - origin.x) as f32,
        bottom: (rect.bottom() - origin.y) as f32,
    }
}

fn local_point(point: Point, origin: Point) -> Vector2 {
    Vector2 {
        X: (point.x - origin.x) as f32,
        Y: (point.y - origin.y) as f32,
    }
}

fn configure_visual(
    visual: &IDCompositionVisual,
    content: Option<&IDCompositionSurface>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    // SAFETY: the visual and optional surface belong to the same live
    // DirectComposition device; offsets are local to the owned overlay HWND.
    unsafe {
        if let Some(content) = content {
            visual
                .SetContent(content)
                .map_err(|error| format!("cannot bind dynamic surface: {error}"))?;
        }
        visual
            .SetOffsetX2(x as f32)
            .and_then(|()| visual.SetOffsetY2(y as f32))
    }
    .map_err(|error| format!("cannot position dynamic visual: {error}"))
}

fn cursor_bounds(marker: &CursorMarker, center: Point) -> Rect {
    let padding = (marker.stroke_width.max(0.0) / 2.0).ceil() + 1.0;
    Rect::new(
        center.x - marker.radius - padding,
        center.y - marker.radius - padding,
        marker.radius * 2.0 + padding * 2.0,
        marker.radius * 2.0 + padding * 2.0,
    )
}

fn indicator_bounds(indicator: &Indicator, position: Point) -> Rect {
    let first = indicator_rect(&indicator.text, position, &indicator.style);
    indicator
        .held_text
        .as_deref()
        .map(|text| {
            indicator_rect(
                text,
                Point::new(position.x, first.bottom() + 4.0),
                &indicator.style,
            )
        })
        .map_or(first, |held| first.union(&held))
        .inset(-1.0, -1.0)
}

fn indicator_rect(text: &str, position: Point, style: &LabelStyle) -> Rect {
    let (width, height) = Indicator::label_size(text.chars().count(), style);
    Rect::new(position.x - width, position.y, width, height)
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if let Some(result) = native::click_through_hit_test(message) {
        return result;
    }
    native::default_window_proc(hwnd, message, wparam, lparam)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    #[test]
    #[ignore = "native DirectWrite layout; initializes a GPU device without displaying windows"]
    fn native_text_select_paragraph_uses_full_width_and_reuses_fonts() -> Result<(), String> {
        use windows::Win32::Graphics::DirectWrite::DWRITE_LINE_METRICS;
        let mut renderer = GpuOverlay::new()?;
        for scale in [1.0, 1.5, 2.0] {
            let mut style = LabelStyle {
                wrap: true,
                font_size: 14.0 * scale,
                bold: false,
                text_alignment: crate::api::overlay::TextAlignment::Left,
                ..Default::default()
            };
            let format = renderer.text_format(&style)?;
            assert_eq!(format, renderer.text_format(&style)?);
            for phrase in [
                "alpha beta gamma delta epsilon ",
                "中文测试自然换行 ",
                "iii WWW iii ",
            ] {
                let text = phrase.repeat(40);
                let text: Vec<_> = text.encode_utf16().collect();
                let mut first_lengths = Vec::new();
                for width in [400.0, 1000.0] {
                    let width = (width * scale) as f32;
                    let mut lines = [DWRITE_LINE_METRICS::default(); 128];
                    let mut count = 0;
                    // SAFETY: UTF-16, format and output metrics are owned through
                    // all synchronous calls; hit-test indices come from this layout.
                    unsafe {
                        let layout = renderer
                            .dwrite
                            .CreateTextLayout(
                                &text,
                                &format,
                                width,
                                (style.font_size * 1.4 * 4.0) as f32,
                            )
                            .map_err(|e| e.to_string())?;
                        layout
                            .GetLineMetrics(Some(&mut lines), &mut count)
                            .map_err(|e| e.to_string())?;
                        assert!(count > 1);
                        let first = lines[0];
                        first_lengths.push(first.length);
                        let (mut x, mut y) = (0.0, 0.0);
                        let mut metrics = DWRITE_HIT_TEST_METRICS::default();
                        layout
                            .HitTestTextPosition(
                                first.length - first.trailingWhitespaceLength - 1,
                                true,
                                &mut x,
                                &mut y,
                                &mut metrics,
                            )
                            .map_err(|e| e.to_string())?;
                        assert!(
                            x > width * 0.85 && x <= width + 0.1,
                            "{phrase}: used {x}/{width}"
                        );
                        assert!((first.height - (style.font_size * 1.4) as f32).abs() < 0.01);
                    }
                }
                assert!(first_lengths[1] > first_lengths[0] * 2);
            }
            style.wrap = false;
            assert_ne!(format, renderer.text_format(&style)?);
            style.wrap = true;
            assert_eq!(format, renderer.text_format(&style)?);
        }
        assert!(
            renderer.content.is_none(),
            "font measurement must not create a window"
        );
        Ok(())
    }

    #[test]
    #[ignore = "requires an interactive Windows compositor; disposable overlay pixel comparison"]
    fn native_retained_damage_matches_full_redraw() -> Result<(), String> {
        use crate::api::overlay::RetainedOverlay;
        crate::platform::windows::display::enable_dpi_awareness()?;
        let area = Rect::new(40.0, 40.0, 480.0, 240.0);
        let mut renderer = GpuOverlay::new()?;
        let mut capture = crate::platform::windows::native::PreparedCapture::new(480, 240)?;
        let mut base = OverlayScene::new();
        base.clip = Some(area);
        base.backdrop = Some(Color::rgb(245, 245, 245));
        for (text, x) in [("aa", 80.0), ("ab", 95.0), ("ac", 380.0)] {
            base.labels.push(
                OverlayLabel::new(text, Rect::new(x, 75.0, 48.0, 32.0), LabelStyle::default())
                    .with_z_index(2),
            );
        }
        let base = Arc::new(base);
        renderer.present(Arc::clone(&base), area)?;
        let started = renderer.begin_draws;
        let mut promoted = OverlayScene::new();
        promoted.clip = base.clip;
        promoted.backdrop = base.backdrop;
        promoted.retained = Some(RetainedOverlay {
            scene: Arc::clone(&base),
            omitted_label: None,
        });
        renderer.present(Arc::new(promoted), area)?;
        assert_eq!(renderer.begin_draws, started);
        assert!(
            renderer.paint_index.is_none(),
            "promoting an unchanged background needs no paint index"
        );
        for (scale, transparent) in [(1.0, false), (1.0, true), (1.5, true), (2.0, true)] {
            let mut dpi = super::super::overlay::DpiSceneCache::default();
            renderer.present(Arc::new(dpi.scene_for_dpi(&base, scale).into_owned()), area)?;
            crate::platform::windows::native::wait_for_dwm_frame().map_err(|e| e.to_string())?;
            for selected in [Some(0), Some(1), Some(2), Some(0), None] {
                let mut frame = OverlayScene::new();
                frame.clip = Some(area);
                frame.backdrop = Some(Color::rgb(245, 245, 245));
                frame.retained = Some(RetainedOverlay {
                    scene: base.clone(),
                    omitted_label: selected,
                });
                if let Some(index) = selected {
                    let mut label = base.labels[index].clone();
                    let style = label.style.make_mut();
                    style.background = if transparent {
                        Color::rgba(0, 180, 80, 60)
                    } else {
                        Color::rgb(0, 180, 80)
                    };
                    style.border_width = 4.0;
                    style.text_color = Color::rgb(180, 0, 120);
                    label.z_index = 3;
                    frame.labels.push(label);
                }
                frame.shapes.push(OverlayShape::fill(
                    Rect::new(65.0, 180.0, 350.0, 65.0),
                    Color::rgb(20, 30, 40),
                ));
                frame.labels.push(
                    OverlayLabel::new(
                        format!("Selected: {selected:?}"),
                        Rect::new(70.0, 185.0, 320.0, 40.0),
                        LabelStyle::default(),
                    )
                    .with_z_index(10_001),
                );
                let frame = Arc::new(dpi.scene_for_dpi(&frame, scale).into_owned());
                let started = renderer.begin_draws;
                renderer.present(frame.clone(), area)?;
                assert!(
                    renderer.begin_draws - started >= 2,
                    "use separate dirty regions, including the first retained frame"
                );
                crate::platform::windows::native::wait_for_dwm_frame()
                    .map_err(|e| e.to_string())?;
                let partial =
                    capture.capture_with(40, 40, 480, 240, |bytes, _, _| Ok(bytes.to_vec()))?;
                renderer.content.as_mut().unwrap().last_scene = None;
                renderer.present(frame.clone(), area)?;
                crate::platform::windows::native::wait_for_dwm_frame()
                    .map_err(|e| e.to_string())?;
                let full =
                    capture.capture_with(40, 40, 480, 240, |bytes, _, _| Ok(bytes.to_vec()))?;
                let difference = partial.iter().zip(&full).filter(|(a, b)| a != b).count();
                assert_eq!(
                    difference, 0,
                    "selection={selected:?} transparent={transparent} scale={scale}"
                );
            }
        }
        renderer.present(Arc::new(OverlayScene::new()), area)?;
        assert!(
            renderer.paint_index.is_none(),
            "empty frames release the old spatial index"
        );
        renderer.dismiss()?;
        Ok(())
    }

    fn percentiles(mut samples: Vec<u128>) -> (u128, u128, u128) {
        samples.sort_unstable();
        let last = samples.len() - 1;
        (
            samples[last * 50 / 100],
            samples[last * 95 / 100],
            samples[last * 99 / 100],
        )
    }

    fn report_process_metrics(label: &str) {
        let script = format!(
            "$p=Get-Process -Id {}; Write-Output ('native_metrics label={label} working_set=' + $p.WorkingSet64 + ' private_bytes=' + $p.PrivateMemorySize64 + ' handles=' + $p.HandleCount + ' threads=' + $p.Threads.Count)",
            std::process::id()
        );
        match Command::new(r"C:\Program Files\PowerShell\7\pwsh.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .output()
        {
            Ok(output) if output.status.success() => {
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            Ok(output) => println!(
                "native_metrics label={label} unavailable: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            Err(error) => println!("native_metrics label={label} unavailable: {error}"),
        }
    }

    #[test]
    fn static_cache_ignores_cursor_only_changes() {
        let mut first = OverlayScene::new();
        first.shapes.push(OverlayShape::fill(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Color::rgb(1, 2, 3),
        ));
        first.cursor_marker = Some(crate::api::overlay::CursorMarker {
            center: Point::new(10.0, 20.0),
            radius: 8.0,
            fill: Color::rgb(4, 5, 6),
            stroke: Color::rgb(7, 8, 9),
            stroke_width: 1.0,
        });
        let mut second = first.clone();
        if let Some(marker) = second.cursor_marker.as_mut() {
            marker.center = Point::new(900.0, 700.0);
        }
        assert!(static_scene_eq(&first, &second));
        second.shapes.push(OverlayShape::fill(
            Rect::new(5.0, 5.0, 10.0, 10.0),
            Color::rgb(10, 11, 12),
        ));
        assert!(!static_scene_eq(&first, &second));
    }

    #[test]
    fn dynamic_layers_move_without_repainting_unchanged_content() {
        let mut first = OverlayScene::new();
        first.cursor_marker = Some(CursorMarker {
            center: Point::new(10.0, 20.0),
            radius: 8.0,
            fill: Color::rgb(4, 5, 6),
            stroke: Color::rgb(7, 8, 9),
            stroke_width: 1.0,
        });
        first.indicator = Some(Indicator {
            text: "Normal".into(),
            held_text: Some("Shift".into()),
            position: Point::new(30.0, 40.0),
            style: LabelStyle::default(),
        });

        let mut moved = first.clone();
        moved.cursor_marker.as_mut().unwrap().center = Point::new(100.0, 200.0);
        moved.indicator.as_mut().unwrap().position = Point::new(120.0, 240.0);
        assert_eq!(
            scene_changes(
                Some(&first),
                &moved,
                (Some(Point::new(10.0, 20.0)), Some(Point::new(30.0, 40.0))),
                false,
            ),
            SceneChanges {
                static_changed: false,
                cursor_changed: true,
                cursor_repaint: false,
                indicator_changed: true,
                indicator_repaint: false,
            }
        );

        let positions = (
            Some(Point::new(100.0, 200.0)),
            Some(Point::new(120.0, 240.0)),
        );
        let restored = scene_changes(Some(&first), &first, positions, false);
        assert!(restored.cursor_changed);
        assert!(restored.indicator_changed);
        assert!(!restored.cursor_repaint && !restored.indicator_repaint);
        let already_moved = scene_changes(Some(&first), &moved, positions, false);
        assert!(!already_moved.cursor_changed && !already_moved.indicator_changed);
        let partial_failure = scene_changes(Some(&first), &moved, (None, positions.1), false);
        assert!(partial_failure.cursor_changed);
        assert!(!partial_failure.indicator_changed);

        let area_moved = scene_changes(Some(&moved), &moved, positions, true);
        assert!(area_moved.static_changed);
        assert!(area_moved.cursor_changed);
        assert!(!area_moved.cursor_repaint);
        assert!(area_moved.indicator_changed);
        assert!(!area_moved.indicator_repaint);
    }

    #[test]
    fn dynamic_layers_repaint_when_their_content_changes() {
        let mut first = OverlayScene::new();
        first.cursor_marker = Some(CursorMarker {
            center: Point::new(10.0, 20.0),
            radius: 8.0,
            fill: Color::rgb(4, 5, 6),
            stroke: Color::rgb(7, 8, 9),
            stroke_width: 1.0,
        });
        first.indicator = Some(Indicator {
            text: "Normal".into(),
            held_text: None,
            position: Point::new(30.0, 40.0),
            style: LabelStyle::default(),
        });

        let mut changed = first.clone();
        changed.cursor_marker.as_mut().unwrap().radius = 12.0;
        changed.indicator.as_mut().unwrap().held_text = Some("Ctrl".into());
        let changes = scene_changes(
            Some(&first),
            &changed,
            (Some(Point::new(10.0, 20.0)), Some(Point::new(30.0, 40.0))),
            false,
        );
        assert!(changes.cursor_changed);
        assert!(changes.cursor_repaint);
        assert!(changes.indicator_changed);
        assert!(changes.indicator_repaint);
    }

    #[test]
    fn native_surface_dimensions_are_validated() {
        assert_eq!(dimension(1.4), Ok(1));
        assert!(dimension(0.0).is_err());
        assert!(dimension(f64::NAN).is_err());
        assert!(dimension(f64::INFINITY).is_err());
    }

    #[test]
    #[ignore = "requires an interactive Windows compositor"]
    fn directcomposition_smoke() -> Result<(), String> {
        super::super::enable_dpi_awareness()?;
        let mut renderer = GpuOverlay::new()?;
        let mut scene = OverlayScene::new();
        let area = Rect::new(0.0, 0.0, 256.0, 128.0);
        scene.clip = Some(area);
        scene.backdrop = Some(Color::rgb(255, 255, 255));
        scene.shapes.push(OverlayShape::fill(
            Rect::new(8.0, 8.0, 64.0, 48.0),
            Color::rgba(20, 40, 80, 128),
        ));
        scene.shapes.push(OverlayShape::line(
            Point::new(8.0, 64.0),
            Point::new(240.0, 64.0),
            Color::rgb(255, 255, 255),
            1.0,
        ));
        scene.labels.push(OverlayLabel::new(
            "GPU",
            Rect::new(80.0, 8.0, 64.0, 32.0),
            LabelStyle::default(),
        ));
        scene.cursor_marker = Some(crate::api::overlay::CursorMarker {
            center: Point::new(180.0, 32.0),
            radius: 8.0,
            fill: Color::rgb(255, 0, 0),
            stroke: Color::rgb(255, 0, 0),
            stroke_width: 1.0,
        });
        scene.indicator = Some(crate::api::overlay::Indicator {
            text: "DComp".into(),
            held_text: Some("GPU".into()),
            position: Point::new(248.0, 72.0),
            style: LabelStyle::default(),
        });
        let scene = Arc::new(scene);
        renderer.present(Arc::clone(&scene), area)?;
        assert!(native::is_window_visible(
            renderer.content.as_ref().unwrap().window.raw()
        ));
        let mut capture = native::PreparedCapture::new(256, 128)?;
        let mut read = || -> Result<Vec<u8>, String> {
            assert!(native::pump_window_messages());
            native::wait_for_dwm_frame().map_err(|error| error.to_string())?;
            capture.capture_with(0, 0, 256, 128, |pixels, _, _| Ok(pixels.to_vec()))
        };
        let original = read()?;
        let rgb = |pixels: &[u8], x: usize, y: usize| {
            let index = (y * 256 + x) * 4;
            [pixels[index + 2], pixels[index + 1], pixels[index]]
        };
        assert_eq!(rgb(&original, 180, 32), [255, 0, 0]);
        assert_ne!(
            rgb(&original, 235, 80),
            [255, 255, 255],
            "indicator must be above the static backdrop"
        );
        let draws = renderer.begin_draws;
        renderer.update_positions(Some(Point::new(140.0, 52.0)), Some(Point::new(208.0, 92.0)))?;
        let moved = read()?;
        assert_eq!(rgb(&moved, 180, 32), [255, 255, 255]);
        assert_eq!(rgb(&moved, 140, 52), [255, 0, 0]);
        assert_eq!(rgb(&moved, 235, 80), [255, 255, 255]);
        assert_ne!(
            rgb(&moved, 195, 100),
            [255, 255, 255],
            "indicator must move independently of the cursor"
        );
        renderer.present(Arc::clone(&scene), area)?;
        assert!(
            read()? == original,
            "a complete scene must restore its positions"
        );
        assert_eq!(
            renderer.begin_draws, draws,
            "restoring positions must not repaint"
        );
        renderer.utf16.reserve(8192);
        renderer.dismiss()?;
        assert_eq!(renderer.utf16.capacity(), 0);
        assert!(renderer.content.is_none());
        renderer.present(Arc::clone(&scene), area)?;
        assert!(
            read()? == original,
            "a recreated window must show its first frame"
        );
        renderer.present(scene, Rect::new(0.0, 0.0, 288.0, 160.0))?;
        assert!(
            read()? == original,
            "a resized window must show its first frame"
        );
        renderer.dismiss()?;
        Ok(())
    }

    #[test]
    #[ignore = "requires an interactive Windows compositor"]
    fn native_search_counter_is_drawn_once_on_the_right() -> Result<(), String> {
        crate::platform::windows::display::enable_dpi_awareness()?;
        let mut renderer = GpuOverlay::new()?;
        let area = Rect::new(40.0, 40.0, 480.0, 80.0);
        let bounds = Rect::new(52.0, 52.0, 420.0, 56.0);
        let mut capture = crate::platform::windows::native::PreparedCapture::new(480, 80)?;
        for family in ["", "Consolas"] {
            for query in ["sas sas aka", "sda sd ".repeat(80).as_str()] {
                let mut scene = OverlayScene::new();
                scene.clip = Some(area);
                scene.shapes.push(OverlayShape::Rect {
                    rect: area,
                    fill: Color::rgb(255, 255, 255),
                    stroke: Color::TRANSPARENT,
                    stroke_width: 0.0,
                    corner_radius: 0.0,
                    z_index: 0,
                });
                let mut label = OverlayLabel::new(
                    format!("{query}0/155"),
                    bounds,
                    LabelStyle {
                        font_family: family.into(),
                        font_size: 21.0,
                        text_alignment: crate::api::overlay::TextAlignment::Left,
                        text_color: Color::rgb(0, 0, 200),
                        matched_text_color: Color::rgb(107, 114, 128),
                        ..Default::default()
                    },
                )
                .with_fixed_bounds();
                label.scroll_to_cursor = true;
                label.trailing_text_len = 5;
                scene.labels.push(label);
                renderer.present(Arc::new(scene), area)?;
                crate::platform::windows::native::wait_for_dwm_frame()
                    .map_err(|error| error.to_string())?;
                let offsets = renderer
                    .edit_layout
                    .as_ref()
                    .unwrap()
                    .trailing_offsets
                    .unwrap();
                let counter_left = bounds.right() - area.x - (offsets.1 - offsets.0);
                let leftmost = capture
                    .capture_with(40, 40, 480, 80, |pixels, width, _| {
                        Ok(pixels
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .enumerate()
                            .filter_map(|(index, pixel)| {
                                let channels = &pixel[..3];
                                let low = *channels.iter().min().unwrap();
                                let high = *channels.iter().max().unwrap();
                                (high < 220 && high - low <= 30).then_some(index % width as usize)
                            })
                            .min())
                    })?
                    .ok_or("counter pixels were not presented")?;
                assert!(
                    leftmost as f64 >= counter_left - 1.0,
                    "duplicate counter before x={counter_left}: x={leftmost}, {family}, query length={}",
                    query.len()
                );
            }
        }
        renderer.dismiss()?;
        Ok(())
    }

    #[test]
    #[ignore = "requires an interactive Windows compositor"]
    fn native_search_viewport_reuses_layout_for_cursor_and_point() -> Result<(), String> {
        let mut renderer = GpuOverlay::new()?;
        let area = Rect::new(0.0, 0.0, 480.0, 80.0);
        let mut scene = OverlayScene::new();
        scene.clip = Some(area);
        let query = "ahd 汉字🙂 dha had dhad h ".repeat(80);
        let mut input = OverlayLabel::new(
            query.as_str(),
            Rect::new(12.0, 12.0, 420.0, 56.0),
            LabelStyle {
                font_size: 21.0,
                text_alignment: crate::api::overlay::TextAlignment::Left,
                ..Default::default()
            },
        )
        .with_fixed_bounds();
        input.scroll_to_cursor = true;
        input.edit = Some(crate::api::overlay::LabelEdit::try_from(
            crate::api::text_edit::Selection {
                cursor: query.len(),
                anchor: query.len(),
            },
        )?);
        scene.labels.push(input);
        renderer.present(Arc::new(scene.clone()), area)?;
        let layout = renderer.edit_layout.as_ref().unwrap().layout.clone();
        renderer.utf16.clear();
        scene.labels[0].edit = Some(crate::api::overlay::LabelEdit::try_from(
            crate::api::text_edit::Selection::default(),
        )?);
        renderer.present(Arc::new(scene.clone()), area)?;
        assert_eq!(renderer.edit_layout.as_ref().unwrap().layout, layout);
        assert!(
            renderer.utf16.is_empty(),
            "cursor updates reuse encoded text"
        );
        scene.labels[0].edit = None;
        assert!(scene.labels[0].scroll_to_cursor);
        renderer.present(Arc::new(scene.clone()), area)?;
        assert_eq!(renderer.edit_layout.as_ref().unwrap().layout, layout);
        assert!(renderer.utf16.is_empty(), "Point reuses the editor's text");
        scene.labels[0].text.push_str("0/163");
        scene.labels[0].trailing_text_len = 5;
        renderer.present(Arc::new(scene.clone()), area)?;
        let layout = renderer.edit_layout.as_ref().unwrap().layout.clone();
        renderer.present(Arc::new(scene.clone()), area)?;
        assert_eq!(renderer.edit_layout.as_ref().unwrap().layout, layout);
        scene.labels[0].trailing_text_len = 0;
        renderer.present(Arc::new(scene.clone()), area)?;
        assert_ne!(renderer.edit_layout.as_ref().unwrap().layout, layout);
        scene.labels[0].scroll_to_cursor = false;
        renderer.present(Arc::new(scene), area)?;
        assert!(renderer.edit_layout.is_none());
        renderer.dismiss()?;
        assert!(renderer.edit_layout.is_none());
        Ok(())
    }

    #[test]
    #[ignore = "native performance probe; requires an interactive Windows compositor"]
    fn native_performance_probe_directcomposition_motion() -> Result<(), String> {
        const WARMUP_FRAMES: usize = 100;
        const SAMPLES: usize = 2_000;

        let gpu_started = Instant::now();
        let mut renderer = GpuOverlay::new()?;
        println!("native_gpu_ready_ns={}", gpu_started.elapsed().as_nanos());
        report_process_metrics("gpu_ready");

        let area = Rect::new(0.0, 0.0, 3840.0, 2160.0);
        let mut scene = OverlayScene::new();
        scene.clip = Some(area);
        scene.backdrop = Some(Color::rgba(0, 0, 0, 8));
        for index in 0..1_000 {
            let column = index % 40;
            let row = index / 40;
            scene.labels.push(OverlayLabel::new(
                format!("{index:03}"),
                Rect::new(column as f64 * 96.0, row as f64 * 72.0, 88.0, 32.0),
                LabelStyle::default(),
            ));
        }
        scene.cursor_marker = Some(CursorMarker {
            center: Point::new(200.0, 200.0),
            radius: 8.0,
            fill: Color::rgba(255, 255, 255, 32),
            stroke: Color::rgb(255, 255, 255),
            stroke_width: 1.0,
        });
        scene.indicator = Some(Indicator {
            text: "Normal".into(),
            held_text: Some("Shift".into()),
            position: Point::new(280.0, 220.0),
            style: LabelStyle::default(),
        });

        let first_scene = Arc::new(scene);
        let first_started = Instant::now();
        renderer.present(Arc::clone(&first_scene), area)?;
        println!(
            "native_first_present_ns={}",
            first_started.elapsed().as_nanos()
        );
        report_process_metrics("first_present");

        let mut current = first_scene;
        let begin_draws = renderer.begin_draws;
        let mut samples = Vec::with_capacity(SAMPLES);
        for frame in 0..WARMUP_FRAMES + SAMPLES {
            let x = 200.0 + (frame % 1_000) as f64;
            let y = 200.0 + (frame % 500) as f64;
            let started = Instant::now();
            renderer
                .update_positions(Some(Point::new(x, y)), Some(Point::new(x + 80.0, y + 20.0)))?;
            let elapsed = started.elapsed().as_nanos();
            if frame >= WARMUP_FRAMES {
                samples.push(elapsed);
            }
        }
        assert_eq!(
            renderer.begin_draws, begin_draws,
            "position-only frames must not enter Direct2D BeginDraw"
        );
        let (p50, p95, p99) = percentiles(samples);
        println!("native_motion_present samples={SAMPLES} p50={p50}ns p95={p95}ns p99={p99}ns");

        // Exact shape of the previous Engine/backend route: construct a new
        // shared scene and let `present` rediscover that only positions moved.
        let mut complete_samples = Vec::with_capacity(SAMPLES);
        for frame in 0..WARMUP_FRAMES + SAMPLES {
            let mut next = current.as_ref().clone();
            let x = 300.0 + (frame % 1_000) as f64;
            let y = 300.0 + (frame % 500) as f64;
            next.cursor_marker.as_mut().unwrap().center = Point::new(x, y);
            next.indicator.as_mut().unwrap().position = Point::new(x + 80.0, y + 20.0);
            let next = Arc::new(next);
            let started = Instant::now();
            renderer.present(Arc::clone(&next), area)?;
            let elapsed = started.elapsed().as_nanos();
            if frame >= WARMUP_FRAMES {
                complete_samples.push(elapsed);
            }
            current = next;
        }
        let (complete_p50, complete_p95, complete_p99) = percentiles(complete_samples);
        println!(
            "native_complete_scene_motion samples={SAMPLES} p50={complete_p50}ns p95={complete_p95}ns p99={complete_p99}ns"
        );

        // Conservative in-process representation of the previous renderer:
        // force both tight dynamic surfaces to repaint on every position
        // update. It intentionally does not add the old per-frame full-screen
        // SetWindowPos or static-surface COM clone, so the comparison cannot
        // overstate the benefit of position-only visual updates.
        let mut repaint_samples = Vec::with_capacity(SAMPLES);
        for frame in 0..WARMUP_FRAMES + SAMPLES {
            let mut next = current.as_ref().clone();
            let x = 400.0 + (frame % 1_000) as f64;
            let y = 400.0 + (frame % 500) as f64;
            let alternate = frame.is_multiple_of(2);
            let marker = next.cursor_marker.as_mut().unwrap();
            marker.center = Point::new(x, y);
            marker.fill = if alternate {
                Color::rgba(255, 255, 255, 32)
            } else {
                Color::rgba(240, 240, 240, 32)
            };
            let indicator = next.indicator.as_mut().unwrap();
            indicator.position = Point::new(x + 80.0, y + 20.0);
            indicator.style.text_color = if alternate {
                Color::rgb(255, 255, 255)
            } else {
                Color::rgb(240, 240, 240)
            };
            let next = Arc::new(next);
            let started = Instant::now();
            renderer.present(Arc::clone(&next), area)?;
            let elapsed = started.elapsed().as_nanos();
            if frame >= WARMUP_FRAMES {
                repaint_samples.push(elapsed);
            }
            current = next;
        }
        let (repaint_p50, repaint_p95, repaint_p99) = percentiles(repaint_samples);
        println!(
            "native_forced_repaint_present samples={SAMPLES} p50={repaint_p50}ns p95={repaint_p95}ns p99={repaint_p99}ns"
        );
        report_process_metrics("steady_motion");

        let dismiss_started = Instant::now();
        renderer.dismiss()?;
        println!("native_dismiss_ns={}", dismiss_started.elapsed().as_nanos());
        drop(current);
        std::thread::sleep(std::time::Duration::from_millis(50));
        report_process_metrics("dismissed");
        Ok(())
    }
}
