use thiserror::Error;

#[cfg(test)]
use crate::artifact::ScanArtifactError;
use crate::frame::ScanFrameError;

#[derive(Debug, Error)]
pub enum ScanProducerError {
  #[cfg(test)]
  #[error(transparent)]
  Artifact(#[from] ScanArtifactError),
  #[error(transparent)]
  Frame(#[from] ScanFrameError),
  #[error("fixture image missing: {path}")]
  MissingImage { path: String },
  #[error("image has zero width or height")]
  ZeroImageDimension,
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
  #[error("fixture contains no frames")]
  NoFramesInFixture,
  #[error("duplicate frame_id in fixture: {frame_id}")]
  DuplicateFrameId { frame_id: String },
  #[error("duplicate sequence_index in fixture: {sequence_index}")]
  DuplicateSequenceIndex { sequence_index: u32 },
}
