use std::path::PathBuf;

pub type YoloResult<T> = std::result::Result<T, YoloError>;

#[derive(Debug, thiserror::Error)]
pub enum YoloError {
  #[error("YOLO model class list must not be empty")]
  EmptyClassList,

  #[error("{name} threshold must be between 0.0 and 1.0, got {value}")]
  InvalidThreshold { name: &'static str, value: f32 },

  #[error("YOLO model file does not exist: {}", path.display())]
  MissingModel { path: PathBuf },

  #[error("failed to decode image: {source}")]
  ImageDecode {
    #[from]
    source: image::ImageError,
  },

  #[error("unsupported YOLO output tensor shape: {shape:?}")]
  UnsupportedOutputShape { shape: Vec<usize> },

  #[error("YOLO class count mismatch: expected {expected}, got {actual}")]
  ClassCountMismatch { expected: usize, actual: usize },

  #[error("ONNX Runtime error: {source}")]
  Ort {
    #[from]
    source: ort::Error,
  },
}
