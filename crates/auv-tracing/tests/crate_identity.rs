use std::fs;

#[test]
fn core_crate_is_lightweight_auv_tracing() {
  let manifest = fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
  assert!(manifest.contains("name = \"auv-tracing\""));
  for forbidden in [
    "tokio",
    "reqwest",
    "opentelemetry",
    "RunSession",
    "OperationCatalog",
  ] {
    assert!(!manifest.contains(forbidden), "core manifest contains {forbidden}");
  }
}

#[test]
fn core_crate_exposes_no_operation_recording_wrapper() {
  let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let lib = fs::read_to_string(manifest_dir.join("src/lib.rs")).unwrap();

  assert!(!manifest_dir.join("src/recording.rs").exists());
  assert!(!lib.contains("mod recording"));
  assert!(!lib.contains("pub use recording"));
}
