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
