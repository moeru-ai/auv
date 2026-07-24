//! Per-frame wire types for `scan-frame-v0`.
//!
//! NOTICE(scan-s1-slice-1): only this schema is owner-approved in slice 1.
//! Motion, tracks, and evidence fusion types remain deferred per S1 plan.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SCAN_FRAME_SCHEMA_VERSION: &str = "scan-frame-v0";

#[derive(Debug, Error)]
pub enum ScanFrameError {
  #[error("schema_version mismatch: expected {SCAN_FRAME_SCHEMA_VERSION}, found {found}")]
  SchemaMismatch { found: String },
  #[error("invalid bounds for {field}")]
  InvalidBounds { field: &'static str },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanBounds {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

impl ScanBounds {
  pub fn validate_positive(&self, field: &'static str) -> Result<(), ScanFrameError> {
    if self.width <= 0 {
      return Err(ScanFrameError::InvalidBounds { field });
    }
    if self.height <= 0 {
      return Err(ScanFrameError::InvalidBounds { field });
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanImageDimensions {
  pub width: u32,
  pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanFrame {
  pub schema_version: String,
  pub frame_id: String,
  pub sequence_index: u32,
  pub captured_at_millis: u64,
  pub window_bounds: ScanBounds,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub viewport_bounds: Option<ScanBounds>,
  pub image_dimensions: ScanImageDimensions,
}

impl ScanFrame {
  pub fn validate_wire(&self) -> Result<(), ScanFrameError> {
    if self.schema_version != SCAN_FRAME_SCHEMA_VERSION {
      return Err(ScanFrameError::SchemaMismatch {
        found: self.schema_version.clone(),
      });
    }
    self.window_bounds.validate_positive("window_bounds")?;
    if let Some(viewport) = &self.viewport_bounds {
      viewport.validate_positive("viewport_bounds")?;
    }
    Ok(())
  }
}
