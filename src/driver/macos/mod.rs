// File: src/driver/macos/mod.rs
use super::Driver;
use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact, now_millis,
};

// Legacy command adapter for the shared runtime/catalog surface.
//
// Platform-owned data and typed macOS driver/session APIs live in
// `auv-driver-macos`. This module keeps the old string-command dispatch layer
// wired into the root `DriverCall`/`DriverResponse` model until command
// adapters migrate behind typed session methods.
mod ax_tree;
pub(crate) mod capture;
mod constants;
mod control;
mod descriptor;
mod dispatch;
mod native;
mod observe;
mod overlay;
mod support;
#[cfg(test)]
mod tests;

mod types;

pub(crate) use self::constants::*;
pub(crate) use self::support::*;
pub(crate) use self::types::*;

pub(crate) struct MacOsDesktopDriver;
