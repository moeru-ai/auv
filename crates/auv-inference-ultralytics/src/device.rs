use std::str::FromStr;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum InferenceDevice {
  #[default]
  Cpu,
  Cuda(usize),
  CoreMl,
  DirectMl(usize),
  OpenVino,
  Xnnpack,
  TensorRt(usize),
  Rocm(usize),
}

impl FromStr for InferenceDevice {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let value = value.to_lowercase();
    if let Some(index) = parse_indexed_device(&value, "cuda") {
      return index.map(Self::Cuda);
    }
    if let Some(index) = parse_indexed_device(&value, "directml") {
      return index.map(Self::DirectMl);
    }
    if let Some(index) = parse_indexed_device(&value, "tensorrt") {
      return index.map(Self::TensorRt);
    }
    if let Some(index) = parse_indexed_device(&value, "rocm") {
      return index.map(Self::Rocm);
    }
    match value.as_str() {
      "cpu" => Ok(Self::Cpu),
      "coreml" => Ok(Self::CoreMl),
      "openvino" => Ok(Self::OpenVino),
      "xnnpack" => Ok(Self::Xnnpack),
      _ => Err(format!("unknown inference device: {value}")),
    }
  }
}

impl From<InferenceDevice> for ultralytics_inference::Device {
  fn from(value: InferenceDevice) -> Self {
    match value {
      InferenceDevice::Cpu => Self::Cpu,
      InferenceDevice::Cuda(index) => Self::Cuda(index),
      InferenceDevice::CoreMl => Self::CoreMl,
      InferenceDevice::DirectMl(index) => Self::DirectMl(index),
      InferenceDevice::OpenVino => Self::OpenVino,
      InferenceDevice::Xnnpack => Self::Xnnpack,
      InferenceDevice::TensorRt(index) => Self::TensorRt(index),
      InferenceDevice::Rocm(index) => Self::Rocm(index),
    }
  }
}

fn parse_indexed_device(value: &str, provider: &str) -> Option<Result<usize, String>> {
  if value == provider {
    return Some(Ok(0));
  }
  let rest = value.strip_prefix(provider)?;
  let Some(index) = rest.strip_prefix(':') else {
    return Some(Err(format!("malformed {provider} device spec: {value}")));
  };
  if index.is_empty() {
    return Some(Err(format!("{provider} device index must not be empty")));
  }
  Some(index.parse::<usize>().map_err(|_| format!("invalid {provider} device index: {index}")))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_cpu_and_indexed_gpu_devices() {
    assert_eq!("cpu".parse::<InferenceDevice>().unwrap(), InferenceDevice::Cpu);
    assert_eq!("cuda:1".parse::<InferenceDevice>().unwrap(), InferenceDevice::Cuda(1));
    assert_eq!("tensorrt".parse::<InferenceDevice>().unwrap(), InferenceDevice::TensorRt(0));
  }

  #[test]
  fn rejects_unknown_devices() {
    assert!("mps".parse::<InferenceDevice>().is_err());
  }

  #[test]
  fn rejects_malformed_indexed_provider_prefixes() {
    for value in ["cudafoo", "directmlfoo", "tensorrtxyz", "rocmbar"] {
      assert!(value.parse::<InferenceDevice>().is_err(), "{value} should reject malformed indexed provider prefix");
    }
  }

  #[test]
  fn rejects_malformed_indexed_provider_indices() {
    for value in [
      "cuda:",
      "cuda:not-a-number",
      "cuda:-1",
      "directml:",
      "tensorrt:not-a-number",
      "rocm:-1",
    ] {
      assert!(value.parse::<InferenceDevice>().is_err(), "{value} should reject malformed indexed provider index");
    }
  }
}
