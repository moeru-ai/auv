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
