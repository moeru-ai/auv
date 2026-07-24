use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn semantic_verification_stays_in_app_owned_typed_results() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let files = [
    "src/contract.rs",
    "crates/auv-cli/src/mcp.rs",
    "crates/auv-cli/src/cli_frontend.rs",
    "crates/auv-cli/src/integrations/textedit/mod.rs",
    "crates/auv-cli/src/integrations/minecraft/mod.rs",
    "crates/auv-cli/src/integrations/minecraft/projection_workflow.rs",
    "crates/auv-cli/src/integrations/minecraft/verification.rs",
  ];
  let forbidden = [
    "VerificationResult",
    "VerificationMethod",
    "FailureLayer",
    "VERIFICATION_RESULT_API_VERSION",
    "map_verification_result",
    "map_world_diff_verdict_to_verification_result",
  ];

  let violations = files
    .into_iter()
    .flat_map(|relative| {
      let path = root.join(relative);
      let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
      forbidden
        .into_iter()
        .filter(move |token| source.contains(token))
        .map(move |token| format!("{} contains {token:?}", display_path(root, &path)))
    })
    .collect::<Vec<_>>();

  assert!(
    violations.is_empty(),
    "semantic verification must remain an app-owned typed result instead of a generic core schema:\n{}",
    violations.join("\n")
  );
}

#[test]
fn balatro_does_not_export_empty_future_operation_contracts() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let source = fs::read_to_string(root.join("crates/auv-game-balatro/src/lib.rs")).expect("read Balatro crate root");

  assert!(!source.contains("pub mod operation;"), "Balatro must not expose a placeholder operation module");
  for name in [
    "OperationRequest",
    "OperationResult",
    "VerificationMode",
    "VerificationProfile",
  ] {
    assert!(!source.contains(name), "Balatro must not export empty future contract {name}");
  }
}

fn display_path(root: &Path, path: &PathBuf) -> String {
  path.strip_prefix(root).unwrap_or(path).display().to_string()
}
