use std::path::PathBuf;

pub type InferenceResult<T> = Result<T, InferenceError>;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
  #[error("model file does not exist: {}", path.display())]
  MissingModel { path: PathBuf },
  #[error("class list must not be empty")]
  EmptyClassList,
  #[error("{name} threshold must be finite and between 0 and 1, got {value}")]
  InvalidThreshold { name: &'static str, value: f32 },
  #[error("input size must be greater than zero, got {input_size}")]
  InvalidInputSize { input_size: u32 },
  #[error("max detections must be greater than zero, got {max_detections}")]
  InvalidMaxDetections { max_detections: usize },
  #[error("image dimensions must be greater than zero, got {width}x{height}")]
  InvalidImageSize { width: u32, height: u32 },
  #[error("detector session is unavailable: {reason}")]
  SessionUnavailable { reason: String },
  #[error("backend returned no detection result")]
  MissingResult,
  #[error("backend result does not contain detection boxes")]
  MissingBoxes,
  #[error("backend class id {class_id} has no label")]
  MissingClassLabel { class_id: usize },
  #[error("failed to decode image: {source}")]
  ImageDecode {
    #[from]
    source: image::ImageError,
  },
  #[error("backend error: {message}")]
  Backend { message: String },
  #[error("I/O error: {source}")]
  Io {
    #[from]
    source: std::io::Error,
  },
  #[error("JSON error: {source}")]
  Json {
    #[from]
    source: serde_json::Error,
  },
}
