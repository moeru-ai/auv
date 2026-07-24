use std::path::Path;
use std::process::Command;

#[test]
fn view_memory_carries_one_typed_source_artifact_uri() {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let output = Command::new(env!("CARGO"))
    .args([
      "tree", "--locked", "-p", "auv-view", "--edges", "normal", "--prefix", "none",
    ])
    .current_dir(workspace_root)
    .output()
    .expect("run cargo tree for auv-view");
  assert!(
    output.status.success(),
    "cargo tree failed:\nstdout:\n{}\nstderr:\n{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  let dependency_tree = String::from_utf8(output.stdout).expect("cargo tree output should be UTF-8");
  assert!(dependency_tree.lines().any(|line| line.starts_with("auv-tracing ")), "auv-view dependency tree:\n{dependency_tree}");

  let memory_model = include_str!("../src/memory/mod.rs");
  let memory_write = include_str!("../src/memory/write.rs");
  assert!(memory_model.contains("pub source_scan_uri: ArtifactUri"), "ViewMemory must carry typed source lineage");
  assert!(memory_write.contains("pub source_scan_uri: ArtifactUri"), "MemoryWriteInput must require typed source lineage");
  assert!(!memory_model.contains("source_run_id"), "source run must be derived from the artifact URI");
}

#[test]
fn view_evidence_source_only_models_the_typed_ocr_producer() {
  let source = include_str!("../src/lib.rs");

  assert!(source.contains("OcrText"), "typed OCR evidence semantics must remain available");
  for placeholder in ["AxNode", "IconMatch", "Visual"] {
    assert!(!source.contains(placeholder), "unproduced evidence source {placeholder} must not remain in the domain IR");
  }
  assert_eq!(serde_json::to_string(&auv_view::ViewEvidenceSource::default()).expect("serialize default evidence source"), "\"ocr_text\"");
}
