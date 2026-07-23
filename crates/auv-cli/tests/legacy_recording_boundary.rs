#[test]
fn product_cli_has_no_recording_runtime_or_shared_invoke_wrapper() {
  let repo_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let repo =
    repo_path.canonicalize().unwrap_or_else(|error| panic!("failed to canonicalize repository root {}: {error}", repo_path.display()));
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
    "execute_invoke_frontend",
    "execute_product_cli_call",
    "execute_mcp_frontend",
    "fixture_document_write_cli",
    "fixture_textedit_document_write",
    "OPERATION_RESULT_API_VERSION",
    "pub struct OperationResult",
  ];
  let matches = scan_rust_and_manifests(&roots, &forbidden).unwrap_or_else(|error| panic!("{error}"));
  assert!(matches.is_empty(), "legacy product CLI references: {matches:?}");
}

fn scan_rust_and_manifests(roots: &[std::path::PathBuf], needles: &[&str]) -> Result<Vec<std::path::PathBuf>, String> {
  let self_source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/legacy_recording_boundary.rs");
  let self_path =
    self_source.canonicalize().map_err(|error| format!("failed to canonicalize boundary test {}: {error}", self_source.display()))?;
  let mut pending = roots.to_vec();
  let mut matches = Vec::new();
  while let Some(path) = pending.pop() {
    if path.is_dir() {
      let entries = std::fs::read_dir(&path).map_err(|error| format!("failed to read directory {}: {error}", path.display()))?;
      for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read directory entry under {}: {error}", path.display()))?;
        pending.push(entry.path());
      }
      continue;
    }
    if path == self_path {
      continue;
    }
    if path.extension().and_then(|value| value.to_str()) == Some("rs")
      || path.file_name().and_then(|value| value.to_str()) == Some("Cargo.toml")
    {
      let source = std::fs::read_to_string(&path).map_err(|error| format!("failed to read candidate {}: {error}", path.display()))?;
      if needles.iter().any(|needle| source.contains(needle)) {
        matches.push(path);
      }
    }
  }
  matches.sort();
  Ok(matches)
}
