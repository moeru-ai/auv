#[test]
fn product_cli_has_no_recording_runtime_or_shared_invoke_wrapper() {
  let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
  let roots = [
    repo.join("src"),
    repo.join("crates/auv-cli"),
    repo.join("crates/auv-cli-invoke"),
  ];
  let forbidden = [
    "auv_tracing_driver",
    "auv-tracing-driver",
    "RunRecordingBackend",
    "RecordedOperationContext",
    "OperationSummary",
    "invoke_recorded",
    "render_recorded_invoke",
  ];
  let matches = scan_rust_and_manifests(&roots, &forbidden);
  assert!(matches.is_empty(), "legacy product CLI references: {matches:?}");
}

fn scan_rust_and_manifests(roots: &[std::path::PathBuf], needles: &[&str]) -> Vec<std::path::PathBuf> {
  let self_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/legacy_recording_boundary.rs").canonicalize().unwrap();
  let mut pending = roots.to_vec();
  let mut matches = Vec::new();
  while let Some(path) = pending.pop() {
    if path.is_dir() {
      pending.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
    } else if path != self_path
      && (path.extension().and_then(|value| value.to_str()) == Some("rs")
        || path.file_name().and_then(|value| value.to_str()) == Some("Cargo.toml"))
      && std::fs::read_to_string(&path).is_ok_and(|source| needles.iter().any(|needle| source.contains(needle)))
    {
      matches.push(path);
    }
  }
  matches.sort();
  matches
}
