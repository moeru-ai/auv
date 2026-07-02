//! Per-frame wire types for `scan-frame-v0`.
//!
//! NOTICE(scan-s1-slice-1): only this schema is owner-approved in slice 1.
//! Motion, tracks, and evidence fusion types remain deferred per S1 plan.

use serde::{Deserialize, Serialize};

pub const SCAN_FRAME_SCHEMA_VERSION: &str = "scan-frame-v0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanBounds {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

impl ScanBounds {
  pub fn validate_positive(
    &self,
    field: &'static str,
  ) -> Result<(), crate::artifact::ScanArtifactError> {
    if self.width <= 0 {
      return Err(crate::artifact::ScanArtifactError::InvalidBounds { field });
    }
    if self.height <= 0 {
      return Err(crate::artifact::ScanArtifactError::InvalidBounds { field });
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanImageRef {
  pub file_name: String,
  pub width: u32,
  pub height: u32,
  pub media_type: String,
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
  pub image: ScanImageRef,
}

impl ScanFrame {
  pub fn validate_wire(&self) -> Result<(), crate::artifact::ScanArtifactError> {
    if self.schema_version != SCAN_FRAME_SCHEMA_VERSION {
      return Err(crate::artifact::ScanArtifactError::SchemaMismatch {
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
