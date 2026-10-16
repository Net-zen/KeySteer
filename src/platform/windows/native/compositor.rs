//! Compositor timing, interruption and display output ownership.
use super::handles::OwnedHandle;
use windows::Win32::Foundation::HANDLE;

unsafe extern "C" {
    fn keysteer_compositor_clock_create() -> isize;
    fn keysteer_compositor_clock_wait(stop_event: isize) -> isize;
    fn keysteer_compositor_clock_signal(stop_event: isize) -> isize;
    fn keysteer_compositor_clock_boost(enable: isize) -> isize;
}

enum CompositorCall {
    Create,
    Wait(isize),
    Signal(isize),
    Boost(bool),
}

/// Keep the C ABI and its dynamically resolved compositor functions inside one
/// reviewed native boundary. The bridge normalizes every result to `isize`.
fn compositor_call(call: CompositorCall) -> isize {
    // SAFETY: the C bridge is compiled into this crate with matching signatures.
    // Handle tokens originate from `CreateEventW`, remain owned by
    // `CompositorClockSignal`, and outlive every synchronous call using them.
    unsafe {
        match call {
            CompositorCall::Create => keysteer_compositor_clock_create(),
            CompositorCall::Wait(stop_event) => keysteer_compositor_clock_wait(stop_event),
            CompositorCall::Signal(stop_event) => keysteer_compositor_clock_signal(stop_event),
            CompositorCall::Boost(enable) => keysteer_compositor_clock_boost(enable as isize),
        }
    }
}

/// Whether Desktop Window Manager composition is available.
pub(crate) fn dwm_composition_enabled() -> windows::core::Result<bool> {
    use windows::Win32::Graphics::Dwm::DwmIsCompositionEnabled;

    // SAFETY: the function has no pointer arguments and returns a BOOL value.
    unsafe { DwmIsCompositionEnabled() }.map(|enabled| enabled.as_bool())
}

/// Compositor synchronization for the dedicated frame/capture workers only.
pub(crate) fn wait_for_dwm_frame() -> windows::core::Result<()> {
    // SAFETY: DwmFlush has no arguments or caller-owned resources.
    unsafe { windows::Win32::Graphics::Dwm::DwmFlush() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompositorWait {
    Frame,
    Interrupted,
    Failed,
}

/// Owner of the event used to interrupt an available compositor clock.
pub(crate) struct CompositorClockSignal(OwnedHandle);

impl CompositorClockSignal {
    /// Probe runtime exports, independently of OS version labels. Return `None`
    /// when the API or its stop event is unavailable; callers then use DXGI/DWM.
    pub(crate) fn try_new() -> Option<Self> {
        let token = compositor_call(CompositorCall::Create);
        (token != 0).then(|| Self(OwnedHandle::new(HANDLE(token as *mut _))))
    }

    pub(crate) fn token(&self) -> isize {
        self.0.raw().0 as isize
    }
}

/// Wake a compositor-clock wait without borrowing the worker-owned handle.
/// The token is published only while that worker retains the corresponding
/// event, so callers never own or close it.
pub(crate) fn interrupt_compositor_clock(token: isize) -> bool {
    token != 0 && compositor_call(CompositorCall::Signal(token)) != 0
}

pub(crate) fn wait_for_compositor_frame(stop_event: isize) -> CompositorWait {
    match compositor_call(CompositorCall::Wait(stop_event)) {
        1 => CompositorWait::Frame,
        0 => CompositorWait::Interrupted,
        _ => CompositorWait::Failed,
    }
}

/// Request high dynamic-refresh cadence while movement is active, when the
/// optional boost export exists. Missing boost support does not disable waiting.
pub(crate) fn boost_compositor_clock(enable: bool) -> bool {
    compositor_call(CompositorCall::Boost(enable)) != 0
}

/// Retained DXGI output selected for display-synchronised movement.
pub(crate) struct DisplayOutput(windows::Win32::Graphics::Dxgi::IDXGIOutput);

impl DisplayOutput {
    /// Block until this output reaches its next vertical blank.
    pub(crate) fn wait_for_vblank(&self) -> windows::core::Result<()> {
        // SAFETY: the retained COM interface stays alive for the synchronous
        // wait and is used only by the frame-clock worker that owns it.
        unsafe { self.0.WaitForVBlank() }
    }
}

/// Prefer actual dynamic-refresh VBlank cadence when the optional export exists.
pub(crate) fn prefer_dynamic_vblank() {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{s, w};

    // SAFETY: dxgi.dll is a system component. The optional export has the same
    // no-argument system ABI as FARPROC, and its ignored HRESULT only reports
    // whether the preference was accepted. FreeLibrary balances our load.
    unsafe {
        let Ok(module) = LoadLibraryW(w!("dxgi.dll")) else {
            return;
        };
        if let Some(disable) = GetProcAddress(module, s!("DXGIDisableVBlankVirtualization")) {
            let _ = disable();
        }
        if let Err(error) = FreeLibrary(module) {
            crate::report_error!("windows-native", "cannot unload dxgi.dll: {error}");
        }
    }
}

/// Find the DXGI output whose native monitor handle matches `monitor`.
pub(crate) fn display_output_for_monitor(monitor: isize) -> Result<Option<DisplayOutput>, String> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    use windows::Win32::Graphics::Gdi::HMONITOR;

    // SAFETY: DXGI creates retained COM wrappers. Enumeration is read-only,
    // and every adapter/output/factory interface is released by RAII.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()
            .map_err(|error| format!("cannot create DXGI factory for frame clock: {error}"))?;
        let monitor = HMONITOR(monitor as *mut _);
        let mut adapter_index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            let mut output_index = 0;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                if let Ok(description) = output.GetDesc()
                    && description.Monitor == monitor
                {
                    return Ok(Some(DisplayOutput(output)));
                }
                output_index += 1;
            }
            adapter_index += 1;
        }
        Ok(None)
    }
}
