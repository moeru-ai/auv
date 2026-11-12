//! Shared XDG Desktop Portal connection identity, bounded requests, and restore-token rotation.
//!
//! Capture, input, and clipboard own their native sessions. This crate owns only
//! the external Portal request and persistence boundaries they share.

#![cfg(target_os = "linux")]

mod persistence;
mod request;

pub use persistence::{RestoreTokenKind, RestoreTokenStore};
pub use request::{run, run_with_consent, session_connection};
