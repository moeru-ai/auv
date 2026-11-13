//! Portable contracts for the desktop capability (window and display APIs).
//!
//! Canonical target, geometry, and error types remain in `auv-driver-common`:
//! input evidence also uses them, so moving them here would introduce a cycle.

pub use auv_driver_common::{
  Display, DriverError, DriverResult, ObservedDisplays, ObservedWindows, Window, WindowDesktop, WindowListFilter, WindowMutationAttempt,
  WindowMutationCandidate, WindowMutationKind, WindowMutationOptions, WindowMutationPath, WindowMutationPolicy, WindowMutationResult,
  WindowMutationStrategy, WindowMutationVerification, WindowRef, WindowSelector, WindowState, WindowStateTarget,
};

pub mod display;
pub mod windows;

/// Display discovery without opening a capture or input session.
pub trait DisplayDriver {
  fn list_displays(&mut self) -> DriverResult<ObservedDisplays>;
}

// TODO(desktop-window-extraction): window APIs still use the existing sessions.
// Move their contracts and native operations together in an owner-approved slice;
// this topology extraction preserves session ownership and mutation behavior.
