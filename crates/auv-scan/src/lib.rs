//! Temporal scan contracts — S line slice 1 (`scan-frame-v0` only).
//!
//! Stable public surface: frame wire types + per-frame artifact read/write.
//! Hermetic fixture builders are test-only and are **not** re-exported.

#[cfg(test)]
mod fixture;

pub mod artifact;
pub mod frame;

pub use artifact::{
  ScanArtifactError, frame_artifact_file_name, read_frame_artifact, write_frame_artifact,
};
pub use frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};
