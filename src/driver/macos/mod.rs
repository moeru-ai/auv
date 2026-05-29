// File: src/driver/macos/mod.rs
use super::Driver;
use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact, now_millis,
};

// Legacy command adapter for the shared runtime/catalog surface.
//
// Platform-owned data and typed macOS driver/session APIs live in
// `auv-driver-macos`. This module is intentionally the old compatibility edge:
// it accepts `DriverCall`, dispatches legacy string operations, and adapts
// them into the root `DriverResponse` model until command-facing callers move
// behind typed session methods.
//
// Do not treat this module as the primary macOS implementation surface.
// New platform capability should land in `auv-driver-macos` first, then be
// selectively re-exposed here through narrow command adapters when needed.
mod ax_tree;
pub(crate) mod capture;
mod control;
mod descriptor;
mod dispatch;
mod observe;
mod overlay;
pub(crate) mod support;
#[cfg(test)]
mod tests;
mod typed;
pub(crate) use self::support::*;
pub(crate) use auv_driver_macos::constants::*;
pub(crate) use auv_driver_macos::types::{
  DetectedScreenRows, ObservedAxNode, ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow,
  ObservedPointResolution, ObservedRect, ObservedWindowSnapshot, OcrTextMatch, OcrTextSnapshot,
  ScreenshotDimensions, WindowCandidate, WindowRef, WindowSelection,
};

pub(crate) struct LegacyMacosCommandDriver;
