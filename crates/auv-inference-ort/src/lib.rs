use auv_inference_common::{InferenceError, InferenceResult};
use ndarray::{ArrayD, IxDyn};
#[cfg(feature = "runtime")]
use ort::{session::Session, value::TensorRef};
use std::{
  path::{Path, PathBuf},
  sync::Mutex,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrtModelConfig {
  pub model_path: PathBuf,
  pub execution_provider: ExecutionProvider,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExecutionProvider {
  #[default]
  Cpu,
  CoreMl,
  Cuda,
  DirectMl,
  OpenVino,
  TensorRt,
  WebGpu,
  Xnnpack,
}

#[derive(Clone, Debug, PartialEq)]
pub struct F32Tensor {
  pub name: String,
  pub shape: Vec<usize>,
  pub data: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopPrediction {
  pub index: usize,
  pub confidence: f32,
}

#[cfg(feature = "runtime")]
pub struct OrtSession {
  model: Mutex<Session>,
}

#[cfg(feature = "runtime")]
impl std::fmt::Debug for OrtSession {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("OrtSession").finish_non_exhaustive()
  }
}

#[cfg(not(feature = "runtime"))]
pub struct OrtSession;

#[cfg(feature = "runtime")]
impl OrtSession {
  pub fn load(config: OrtModelConfig) -> InferenceResult<Self> {
    require_model_path(&config.model_path)?;

    let mut builder = Session::builder().map_err(backend_error)?;
    let providers = execution_providers(config.execution_provider);
    if !providers.is_empty() {
      builder = builder.with_execution_providers(providers).map_err(backend_error)?;
    }
    let model = builder.commit_from_file(&config.model_path).map_err(backend_error)?;

    Ok(Self {
      model: Mutex::new(model),
    })
  }

  pub fn run_f32(&self, input: F32Tensor) -> InferenceResult<Vec<F32Tensor>> {
    let array = ArrayD::from_shape_vec(IxDyn(&input.shape), input.data).map_err(|error| InferenceError::Backend {
      message: error.to_string(),
    })?;
    let tensor = TensorRef::from_array_view(array.view()).map_err(backend_error)?;
    let mut model = self.model.lock().map_err(|error| InferenceError::SessionUnavailable {
      reason: error.to_string(),
    })?;
    let outputs = model.run(vec![(input.name, tensor)]).map_err(backend_error)?;

    outputs
      .keys()
      .map(|name| {
        let value = outputs
          .get(name)
          .ok_or_else(|| InferenceError::Backend {
            message: format!("missing ORT output value for {name}"),
          })?
          .try_extract_tensor::<f32>()
          .map_err(backend_error)?;
        Ok(F32Tensor {
          name: name.to_owned(),
          shape: value.0.iter().map(|dim| *dim as usize).collect(),
          data: value.1.to_vec(),
        })
      })
      .collect()
  }
}

#[cfg(not(feature = "runtime"))]
impl OrtSession {
  pub fn load(config: OrtModelConfig) -> InferenceResult<Self> {
    require_model_path(&config.model_path)?;
    Err(InferenceError::Backend {
      message: "auv-inference-ort built without runtime feature".to_string(),
    })
  }
}

pub fn provider_name(provider: ExecutionProvider) -> &'static str {
  match provider {
    ExecutionProvider::Cpu => "CPUExecutionProvider",
    ExecutionProvider::CoreMl => "CoreMLExecutionProvider",
    ExecutionProvider::Cuda => "CUDAExecutionProvider",
    ExecutionProvider::DirectMl => "DmlExecutionProvider",
    ExecutionProvider::OpenVino => "OpenVINOExecutionProvider",
    ExecutionProvider::TensorRt => "TensorrtExecutionProvider",
    ExecutionProvider::WebGpu => "WebGPUExecutionProvider",
    ExecutionProvider::Xnnpack => "XnnpackExecutionProvider",
  }
}

pub fn softmax(logits: &[f32]) -> Vec<f32> {
  let Some(max) = logits.iter().copied().reduce(f32::max) else {
    return Vec::new();
  };
  let exp = logits.iter().map(|value| (*value - max).exp()).collect::<Vec<_>>();
  let sum = exp.iter().sum::<f32>();
  if sum == 0.0 || !sum.is_finite() {
    return vec![0.0; logits.len()];
  }
  exp.into_iter().map(|value| value / sum).collect()
}

pub fn top1(values: &[f32]) -> Option<TopPrediction> {
  values
    .iter()
    .copied()
    .enumerate()
    .max_by(|(_, left), (_, right)| left.total_cmp(right))
    .map(|(index, confidence)| TopPrediction { index, confidence })
}

fn require_model_path(path: &Path) -> InferenceResult<()> {
  if path.exists() {
    Ok(())
  } else {
    Err(InferenceError::MissingModel {
      path: path.to_path_buf(),
    })
  }
}

#[cfg(feature = "runtime")]
fn backend_error<R>(error: ort::Error<R>) -> InferenceError {
  InferenceError::Backend {
    message: error.to_string(),
  }
}

#[cfg(feature = "runtime")]
fn execution_providers(provider: ExecutionProvider) -> Vec<ort::ep::ExecutionProviderDispatch> {
  #[allow(unreachable_patterns)]
  match provider {
    ExecutionProvider::Cpu => vec![ort::ep::CPU::default().build()],
    #[cfg(feature = "coreml")]
    ExecutionProvider::CoreMl => vec![ort::ep::CoreML::default().build()],
    #[cfg(feature = "cuda")]
    ExecutionProvider::Cuda => vec![ort::ep::CUDA::default().build()],
    #[cfg(feature = "directml")]
    ExecutionProvider::DirectMl => vec![ort::ep::DirectML::default().build()],
    #[cfg(feature = "openvino")]
    ExecutionProvider::OpenVino => vec![ort::ep::OpenVINO::default().build()],
    #[cfg(feature = "tensorrt")]
    ExecutionProvider::TensorRt => vec![ort::ep::TensorRT::default().build()],
    #[cfg(feature = "webgpu")]
    ExecutionProvider::WebGpu => vec![ort::ep::WebGPU::default().build()],
    #[cfg(feature = "xnnpack")]
    ExecutionProvider::Xnnpack => vec![ort::ep::XNNPACK::default().build()],
    _ => Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use auv_inference_common::InferenceError;
  use std::path::PathBuf;

  use crate::{ExecutionProvider, OrtModelConfig, OrtSession, TopPrediction, provider_name, softmax, top1};

  #[test]
  fn missing_model_is_rejected_before_backend_load() {
    let path = PathBuf::from("missing-card-corner-classifier.onnx");
    let err = OrtSession::load(OrtModelConfig {
      model_path: path.clone(),
      execution_provider: ExecutionProvider::Cpu,
    })
    .unwrap_err();

    assert!(matches!(err, InferenceError::MissingModel { path: actual } if actual == path));
  }

  #[test]
  fn provider_names_match_onnx_runtime_identifiers() {
    assert_eq!(provider_name(ExecutionProvider::Cpu), "CPUExecutionProvider");
    assert_eq!(provider_name(ExecutionProvider::CoreMl), "CoreMLExecutionProvider");
    assert_eq!(provider_name(ExecutionProvider::Cuda), "CUDAExecutionProvider");
  }

  #[test]
  fn softmax_returns_normalized_probabilities() {
    let probabilities = softmax(&[1.0, 2.0, 3.0]);
    let total = probabilities.iter().sum::<f32>();

    assert!((total - 1.0).abs() < 1e-6);
    assert!(probabilities[2] > probabilities[1]);
    assert!(probabilities[1] > probabilities[0]);
  }

  #[test]
  fn top1_returns_index_and_confidence() {
    assert_eq!(
      top1(&[0.1, 0.8, 0.3]),
      Some(TopPrediction {
        index: 1,
        confidence: 0.8
      })
    );
    assert_eq!(top1(&[]), None);
  }
}
