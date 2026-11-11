//! Existing Windows capture backends and their shared native resources.
pub mod capture;
#[cfg(feature = "windows-wgc")]
pub mod wgc;
// NOTICE(desktop-path-migration): preserve the existing paths while capture and
// input callers migrate to desktop. These aliases share one native/TLS state.
#[doc(hidden)]
pub mod desktop;
// NOTICE(native-owner-migration): diagnostic ownership remains deferred until
// an approved diagnostics/input slice; do not duplicate this telemetry buffer.
#[doc(hidden)]
pub mod latency;
#[doc(hidden)]
pub mod target;

/// The existing xcap display and region backend.
#[cfg(feature = "windows-xcap")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Xcap;
#[cfg(feature = "windows-xcap")]
impl auv_driver_capture_common::DisplayCaptureDriver for Xcap {
  fn capture_display(
    &mut self,
    options: auv_driver_capture_common::CaptureOptions,
  ) -> auv_driver_common::DriverResult<auv_driver_capture_common::DisplayCapture> {
    options.validate_display()?;
    let captured = capture::capture_display(options.display.as_deref())?;
    Ok(auv_driver_capture_common::DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
#[cfg(feature = "windows-xcap")]
impl auv_driver_capture_common::RegionCaptureDriver for Xcap {
  fn capture_region(
    &mut self,
    options: auv_driver_capture_common::CaptureOptions,
  ) -> auv_driver_common::DriverResult<auv_driver_capture_common::RegionCapture> {
    let region = options.validate_region()?;
    let captured = capture::capture_region(options.display.as_deref(), region)?;
    Ok(auv_driver_capture_common::RegionCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
/// The existing PrintWindow window backend.
#[cfg(feature = "windows-printwindow")]
#[derive(Clone, Copy, Debug, Default)]
pub struct PrintWindow;
#[cfg(feature = "windows-printwindow")]
impl auv_driver_capture_common::WindowCaptureDriver for PrintWindow {
  fn capture_window(
    &mut self,
    window: &auv_driver_common::Window,
    resolution: auv_driver_capture_common::CaptureResolution,
  ) -> auv_driver_common::DriverResult<auv_driver_capture_common::Capture> {
    capture::capture_window(window).map(|capture| capture.at_resolution(resolution))
  }
}
/// The existing WGC backend; its process-wide caches remain shared with legacy callers.
#[cfg(feature = "windows-wgc")]
#[derive(Clone, Copy, Debug, Default)]
pub struct Wgc;
#[cfg(feature = "windows-wgc")]
impl auv_driver_capture_common::WindowCaptureDriver for Wgc {
  fn capture_window(
    &mut self,
    window: &auv_driver_common::Window,
    resolution: auv_driver_capture_common::CaptureResolution,
  ) -> auv_driver_common::DriverResult<auv_driver_capture_common::Capture> {
    wgc::capture_window_wgc(window).map(|capture| capture.at_resolution(resolution))
  }
}
// NOTICE(wgc-display-discovery): the existing display path resolves targets via
// xcap. Retain that dependency and behavior; a native WGC display resolver needs
// its own approved slice. Window/stream WGC remains selectable without xcap.
#[cfg(all(feature = "windows-wgc", feature = "windows-xcap"))]
impl auv_driver_capture_common::DisplayCaptureDriver for Wgc {
  fn capture_display(
    &mut self,
    options: auv_driver_capture_common::CaptureOptions,
  ) -> auv_driver_common::DriverResult<auv_driver_capture_common::DisplayCapture> {
    options.validate_display()?;
    let captured = wgc::capture_display_wgc(options.display.as_deref())?;
    Ok(auv_driver_capture_common::DisplayCapture {
      capture: captured.capture.at_resolution(options.resolution),
      ..captured
    })
  }
}
