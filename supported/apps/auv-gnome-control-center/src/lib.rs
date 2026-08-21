//! GNOME Control Center product workflows over the Linux desktop driver.
//!
//! This crate owns GNOME Settings-specific labels and page flow. Generic
//! Wayland/AT-SPI/portal mechanics stay in `auv-driver-linux`.

pub mod app;
pub mod cli;
pub mod commands;
pub mod output;
pub mod views;
pub mod windows;

mod tracing {
  #[cfg(target_os = "linux")]
  use auv_driver::{InputActionResult, WindowPoint};
  #[cfg(target_os = "linux")]
  use serde::Serialize;

  pub(crate) fn open<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.gnome_control_center.open", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(crate) fn system_details_copy<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.gnome_control_center.system_details.copy", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(crate) fn pointer_speed_set<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.gnome_control_center.mouse.pointer_speed.set", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(crate) fn pointer_speed_roundtrip<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.gnome_control_center.mouse.pointer_speed.roundtrip", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(crate) fn natural_scrolling_toggle<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.gnome_control_center.mouse.natural_scrolling.toggle", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  #[cfg(target_os = "linux")]
  #[derive(Serialize)]
  #[serde(tag = "event", rename_all = "snake_case")]
  pub(crate) enum WindowEvent {
    ExistingWindowResolved { found: bool },
    ProcessStarted,
    WindowAppeared,
    WaitTimedOut { timeout_ms: u64 },
  }

  #[cfg(all(feature = "tracing", target_os = "linux"))]
  impl auv_tracing::EventPayload for WindowEvent {
    const NAME: &'static str = "auv.gnome_control_center.window.lifecycle";
    const VERSION: u32 = 1;
  }

  #[cfg(target_os = "linux")]
  #[derive(Clone, Copy)]
  pub(crate) enum NodeAction {
    SelectSystem,
    SelectAbout,
    SelectSystemDetails,
    CopySystemDetails,
    SelectMouse,
    ToggleNaturalScrolling,
  }

  #[cfg(target_os = "linux")]
  #[derive(Serialize)]
  #[serde(tag = "action", rename_all = "snake_case")]
  enum InputDelivered {
    SelectSystem {
      label: String,
      delivery: InputActionResult,
    },
    SelectAbout {
      label: String,
      delivery: InputActionResult,
    },
    SelectSystemDetails {
      label: String,
      delivery: InputActionResult,
    },
    CopySystemDetails {
      label: String,
      delivery: InputActionResult,
    },
    SelectMouse {
      label: String,
      delivery: InputActionResult,
    },
    SetPointerSpeed {
      position: f64,
      point: WindowPoint,
      delivery: InputActionResult,
    },
    ToggleNaturalScrolling {
      label: String,
      delivery: InputActionResult,
    },
  }

  #[cfg(all(feature = "tracing", target_os = "linux"))]
  impl auv_tracing::EventPayload for InputDelivered {
    const NAME: &'static str = "auv.gnome_control_center.input.delivered";
    const VERSION: u32 = 1;
  }

  #[cfg(target_os = "linux")]
  #[derive(Serialize)]
  struct ClipboardRead {
    byte_length: usize,
  }

  #[cfg(all(feature = "tracing", target_os = "linux"))]
  impl auv_tracing::EventPayload for ClipboardRead {
    const NAME: &'static str = "auv.gnome_control_center.clipboard.read";
    const VERSION: u32 = 1;
  }

  #[cfg(target_os = "linux")]
  pub(crate) fn window(event: WindowEvent) {
    #[cfg(feature = "tracing")]
    auv_tracing::emit_event!(event);
    #[cfg(not(feature = "tracing"))]
    drop(event);
  }

  #[cfg(target_os = "linux")]
  pub(crate) fn node_input(action: NodeAction, label: String, delivery: InputActionResult) {
    let event = match action {
      NodeAction::SelectSystem => InputDelivered::SelectSystem { label, delivery },
      NodeAction::SelectAbout => InputDelivered::SelectAbout { label, delivery },
      NodeAction::SelectSystemDetails => InputDelivered::SelectSystemDetails { label, delivery },
      NodeAction::CopySystemDetails => InputDelivered::CopySystemDetails { label, delivery },
      NodeAction::SelectMouse => InputDelivered::SelectMouse { label, delivery },
      NodeAction::ToggleNaturalScrolling => InputDelivered::ToggleNaturalScrolling { label, delivery },
    };
    emit_input(event);
  }

  #[cfg(target_os = "linux")]
  pub(crate) fn pointer_speed(position: f64, point: WindowPoint, delivery: InputActionResult) {
    emit_input(InputDelivered::SetPointerSpeed {
      position,
      point,
      delivery,
    });
  }

  #[cfg(target_os = "linux")]
  fn emit_input(event: InputDelivered) {
    #[cfg(feature = "tracing")]
    auv_tracing::emit_event!(event);
    #[cfg(not(feature = "tracing"))]
    drop(event);
  }

  #[cfg(target_os = "linux")]
  pub(crate) fn clipboard_read(byte_length: usize) {
    let event = ClipboardRead { byte_length };
    #[cfg(feature = "tracing")]
    auv_tracing::emit_event!(event);
    #[cfg(not(feature = "tracing"))]
    drop(event);
  }
}

pub use commands::mouse::{
  NaturalScrollingToggleInputs, NaturalScrollingToggleResult, PointerSpeedRoundtripInputs, PointerSpeedRoundtripResult,
  PointerSpeedSetInputs, PointerSpeedSetResult, run_natural_scrolling_toggle, run_pointer_speed_roundtrip, run_pointer_speed_set,
};
pub use commands::system_details::{CopySystemDetailsInputs, CopySystemDetailsResult, run_copy_system_details};
pub use commands::{OpenInputs, OpenResult, run_open};
