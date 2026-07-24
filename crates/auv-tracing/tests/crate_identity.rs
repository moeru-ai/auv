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

#[test]
fn authority_readback_uses_cursor_language_not_an_observation_model() {
  let dispatch = fs::read_to_string(format!("{}/src/dispatch.rs", env!("CARGO_MANIFEST_DIR"))).unwrap();

  for forbidden in [
    "ObservationLane",
    "ObservationTarget",
    "ObservationWork",
    "ObservationResult",
    "ObservationSpawnGuard",
    "ObservationTaskGuard",
    "ObservationFailureTarget",
    "CursorObservation",
    "CursorObservationFailure",
  ] {
    assert!(!dispatch.contains(forbidden), "dispatch still defines the ambiguous internal model `{forbidden}`");
  }
}
