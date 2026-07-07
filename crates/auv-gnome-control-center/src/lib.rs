//! GNOME Control Center product workflows over the Linux desktop driver.
//!
//! This crate owns GNOME Settings-specific labels and page flow. Generic
//! Wayland/AT-SPI/portal mechanics stay in `auv-driver-linux`.

pub mod app;
pub mod cli;
pub mod commands;
pub mod interaction;
pub mod output;
pub mod views;
pub mod windows;

pub use commands::mouse::{
  NaturalScrollingToggleInputs, NaturalScrollingToggleResult, PointerSpeedRoundtripInputs, PointerSpeedRoundtripResult,
  PointerSpeedSetInputs, PointerSpeedSetResult, run_natural_scrolling_toggle, run_pointer_speed_roundtrip, run_pointer_speed_set,
};
pub use commands::system_details::{CopySystemDetailsInputs, CopySystemDetailsResult, run_copy_system_details};
pub use commands::{OpenInputs, OpenResult, run_open};
pub use interaction::{InteractionStep, StepOutcome};
