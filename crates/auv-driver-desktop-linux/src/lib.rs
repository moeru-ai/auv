//! Linux desktop topology shared by capture and target-facing APIs.
//!
//! TODO(desktop-window-extraction): window resolution and mutation still live
//! in `auv-driver-linux`; move them with the AT-SPI boundary in the desktop
//! extraction slice. Window and display remain APIs of one capability.

pub mod display;
