//! Process DPI awareness: the one coordinate space of the Windows driver.
//!
//! Every screen coordinate the driver reads or writes is a physical pixel: window frames, display
//! bounds, capture bounds, UI Automation rectangles and `SendInput`/`PostMessage` click points.
//! Windows hands those values out in physical pixels only to a per-monitor DPI aware caller. A
//! DPI-unaware process gets virtualized (logical) values from `GetWindowRect`,
//! `GetSystemMetrics`, `GetDpiForMonitor`, `ScreenToClient` and the `SendInput` absolute axis,
//! while the DWM extended frame bounds stay physical, so the spaces mix.
//!
//! NOTICE(windows-dpi-awareness): the process is made per-monitor-v2 aware with
//! `SetProcessDpiAwarenessContext`, as the first statement of the `auv` binary's `main`, instead
//! of an application manifest. Evidence (luoling-windows-11, 3840x2160 at 150%, 2026-11-03): the
//! default unaware Runner listed physical window frames but its clicks at those points missed,
//! and issue #460 measured a blank 486x446 crop of a 1858x1364 Notepad window at 200%; the same
//! Runner started with `__COMPAT_LAYER=HIGHDPIAWARE` (per-monitor aware) got both right.
//! Trade-off: Microsoft prefers a manifest
//! (`https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process`),
//! because the API must run before the process creates any `HWND` and loses to awareness that is
//! already set (manifest, compatibility settings). A manifest, though, reaches only the `.exe`
//! it is linked into; test binaries, eval tasks and library hosts link this crate without it.
//! One function serves all of them, and `auv` calls it before any window exists. The Helper
//! (`auv-helper`) does not take it: it only locks and unlocks the session and types credentials,
//! with no screen coordinates. Per-monitor-v2 needs Windows 10 1703 or later; on an older system
//! the call fails and the process keeps its awareness.

/// Makes this process per-monitor-v2 DPI aware, once. Returns whether the process is per-monitor
/// aware afterwards (v1 or v2, which both see physical pixels).
///
/// Call it before the process creates any window: `auv` does it first in `main`. Every host of
/// the Windows driver that reads or writes screen coordinates needs it, including test binaries.
/// When awareness was already set another way, for example by a manifest or compatibility
/// settings, Windows keeps that setting and the result tells whether it is usable.
#[cfg(target_os = "windows")]
pub fn set_process_per_monitor_aware() -> bool {
  use std::sync::OnceLock;

  use windows::Win32::Foundation::HANDLE;
  use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_PER_MONITOR_AWARE, GetAwarenessFromDpiAwarenessContext,
    GetDpiAwarenessContextForProcess, SetProcessDpiAwarenessContext,
  };

  static AWARE: OnceLock<bool> = OnceLock::new();
  *AWARE.get_or_init(|| {
    // SAFETY: changes only this process's default DPI awareness. An `Err` (access denied when
    // awareness is already set) leaves it as it was, and the read below reports it.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    // A null process handle means the current process.
    let context = unsafe { GetDpiAwarenessContextForProcess(HANDLE::default()) };
    let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(context) };
    awareness == DPI_AWARENESS_PER_MONITOR_AWARE
  })
}
