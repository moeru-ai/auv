use crate::driver::LinuxDriverSessionState;
use auv_driver_capture_common::{DisplayCapture, RegionCapture};
#[cfg(not(target_os = "linux"))]
use auv_driver_common::DriverError;
use auv_driver_common::display::ObservedDisplays;
use auv_driver_common::{DriverResult, Rect};
use std::sync::{Arc, Mutex};

pub fn list_displays() -> DriverResult<ObservedDisplays> {
  auv_driver_desktop::linux::display::list()
}

#[cfg(target_os = "linux")]
fn capture_monitor_frame(
  state: &Arc<Mutex<LinuxDriverSessionState>>,
  bounds: Option<Rect>,
) -> DriverResult<auv_driver_capture::linux::ScreenCastFrame> {
  let mut state = state.lock().expect("linux driver session state poisoned");
  if state.screencast_session.is_none() {
    state.screencast_session =
      Some(crate::native::portal::ScreenCastSession::open_monitor(state.restore_tokens.as_ref(), state.portal_app_id.as_ref())?);
  }
  state.screencast_session.as_mut().expect("screencast session was just ensured").capture_monitor_frame(bounds)
}

#[cfg(target_os = "linux")]
pub fn capture_display(state: &Arc<Mutex<LinuxDriverSessionState>>, selector: Option<&str>) -> DriverResult<DisplayCapture> {
  auv_driver_capture::linux::capture_display(
    selector,
    |bounds| capture_monitor_frame(state, bounds),
    auv_driver_capture::linux::Fallback::Screenshot,
  )
}
#[cfg(not(target_os = "linux"))]
pub fn capture_display(_state: &Arc<Mutex<LinuxDriverSessionState>>, _selector: Option<&str>) -> DriverResult<DisplayCapture> {
  Err(DriverError::unsupported("display.capture"))
}
#[cfg(target_os = "linux")]
pub fn capture_region(state: &Arc<Mutex<LinuxDriverSessionState>>, selector: Option<&str>, region: Rect) -> DriverResult<RegionCapture> {
  auv_driver_capture::linux::capture_region(
    selector,
    region,
    |bounds| capture_monitor_frame(state, bounds),
    auv_driver_capture::linux::Fallback::Screenshot,
  )
}
#[cfg(not(target_os = "linux"))]
pub fn capture_region(_state: &Arc<Mutex<LinuxDriverSessionState>>, _selector: Option<&str>, _region: Rect) -> DriverResult<RegionCapture> {
  Err(DriverError::unsupported("display.capture_region"))
}
