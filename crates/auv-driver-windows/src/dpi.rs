//! Process DPI awareness: the one coordinate space of the Windows driver.
//!
//! Every screen coordinate the driver reads or writes is a physical pixel: window frames, display
//! bounds, capture bounds, UI Automation rectangles and `SendInput`/`PostMessage` click points.
//! Windows hands those values out in physical pixels only to a per-monitor DPI aware caller. A
//! DPI-unaware process gets virtualized (logical) values from `GetWindowRect`,
//! `GetSystemMetrics`, `GetDpiForMonitor`, `ScreenToClient` and the `SendInput` absolute axis,
//! while the DWM extended frame bounds stay physical, so the spaces mix.
//!
//! NOTICE(windows-dpi-awareness): the process that hosts the driver owns its DPI awareness, and
//! the driver never changes it.
//!
//! - `auv.exe` declares per-monitor-v2 in its application manifest
//!   (`crates/auv-cli/auv.exe.manifest`, linked by `crates/auv-cli/build.rs`), so it is aware
//!   from process start. Microsoft recommends the manifest over the API
//!   (`https://learn.microsoft.com/en-us/windows/win32/hidpi/setting-the-default-dpi-awareness-for-a-process`).
//!   trycua/cua does the same for its driver binary (`libs/cua-driver/rust/crates/cua-driver/build.rs`
//!   and `cua-driver.manifest`), and its driver library leaves the host's mode alone.
//! - A library host declares awareness itself, best in its own manifest. Process awareness is
//!   global and must be set before the first `HWND`, so a library that switched it on its first
//!   call would change how the host's own windows are scaled, and it would lose anyway to
//!   awareness that is already set. `WindowsDriver::open_local` only prints one warning when the
//!   host is not per-monitor aware: on a scaled display its coordinates are then virtualized.
//! - AUV's own executables without the manifest that work on a real desktop (the driver's
//!   integration tests, `background-session-eval`) opt in with [`set_process_per_monitor_aware`]
//!   first.
//!
//! Evidence (luoling-windows-11, 3840x2160 at 150%, 2026-11-03): the unaware Runner listed
//! physical window frames but its clicks at those points missed, and issue #460 measured a blank
//! 486x446 crop of a 1858x1364 Notepad window at 200%; a per-monitor-aware Runner got both right.
//! A compatibility setting (`__COMPAT_LAYER`, or "Override high DPI scaling behavior" in the
//! file's properties) still overrides the manifest, so the warning also applies to `auv.exe`.
//! The Helper (`auv-helper`) has no such manifest: it only locks and unlocks the session and types
//! credentials, with no screen coordinates. Per-monitor-v2 needs Windows 10 1703 or later; the
//! manifest falls back to per-monitor (v1) before that.

/// Makes this process per-monitor-v2 DPI aware, once. Returns whether the process is per-monitor
/// aware afterwards (v1 or v2, which both see physical pixels).
///
/// For AUV's own test and evaluation executables, which have no manifest: call it first, before
/// the process creates any window. `auv.exe` and library hosts declare awareness in their
/// manifest instead (see NOTICE(windows-dpi-awareness)). When awareness was already set another
/// way, Windows keeps that setting and the result tells whether it is usable.
#[cfg(target_os = "windows")]
pub fn set_process_per_monitor_aware() -> bool {
  use std::sync::OnceLock;

  use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};

  static AWARE: OnceLock<bool> = OnceLock::new();
  *AWARE.get_or_init(|| {
    // SAFETY: changes only this process's default DPI awareness. An `Err` (access denied when
    // awareness is already set) leaves it as it was, and the read below reports it.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    is_process_per_monitor_aware()
  })
}

/// Whether this process sees physical pixels (per-monitor aware, v1 or v2).
#[cfg(target_os = "windows")]
fn is_process_per_monitor_aware() -> bool {
  use windows::Win32::Foundation::HANDLE;
  use windows::Win32::UI::HiDpi::{DPI_AWARENESS_PER_MONITOR_AWARE, GetAwarenessFromDpiAwarenessContext, GetDpiAwarenessContextForProcess};

  // A null process handle means the current process.
  let context = unsafe { GetDpiAwarenessContextForProcess(HANDLE::default()) };
  let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(context) };
  awareness == DPI_AWARENESS_PER_MONITOR_AWARE
}

/// Prints one warning per process when the host is not per-monitor DPI aware. It never changes
/// the awareness; see NOTICE(windows-dpi-awareness).
#[cfg(target_os = "windows")]
pub(crate) fn warn_if_not_per_monitor_aware() {
  use std::sync::Once;

  static CHECKED: Once = Once::new();
  CHECKED.call_once(|| {
    if !is_process_per_monitor_aware() {
      eprintln!(
        "warning: auv-driver-windows: this process is not per-monitor DPI aware, so on a scaled display its coordinates are DPI-virtualized and window frames, captures and clicks may disagree; declare per-monitor-v2 DPI awareness in the host's application manifest"
      );
    }
  });
}
