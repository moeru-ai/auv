use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn invoke_frontends_do_not_exchange_results_through_generic_metadata_bags() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let files = [
    "src/mcp.rs",
    "crates/auv-cli/src/mcp.rs",
    "crates/auv-cli/src/integrations/textedit/mod.rs",
    "crates/auv-cli-invoke/src/command.rs",
    "crates/auv-cli-invoke/src/models/invoke_result.rs",
    "crates/auv-cli-invoke/src/commands/app.rs",
    "crates/auv-cli-invoke/src/commands/display.rs",
    "crates/auv-cli-invoke/src/commands/input.rs",
    "crates/auv-cli-invoke/src/commands/screen.rs",
    "crates/auv-cli-invoke/src/commands/window.rs",
    "crates/auv-netease-music/src/invoke/mod.rs",
    "crates/auv-netease-music/src/invoke/select_proof.rs",
    "crates/auv-netease-music/src/invoke/sidebar_scan_proof.rs",
  ];
  let forbidden = [
    "pub signals:",
    "signals: BTreeMap",
    ".signals",
    "insert_signal(",
    "add_window_signals",
    "add_click_window_signals",
    "insert_display_signals",
    "InvokeSignalValue",
    "InvokeCommandOutput::new",
    "pub backend:",
    "pub notes:",
    "pub known_limits:",
    "pub verification:",
    "output_summary",
    "McpInvokeOutcome",
    "pub status: RunStatus",
    "pub failure_message:",
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
    "invoke result data must use typed command values, domain facts, or frontend-owned presentation fields:\n{}",
    violations.join("\n")
  );
}

#[test]
fn recognition_and_candidate_contracts_do_not_expose_generic_limit_bags() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  for relative in [
    "src/contract.rs",
    "src/candidate_promotion.rs",
    "src/run_read/mod.rs",
  ] {
    let path = root.join(relative);
    let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert!(
      !source.contains("known_limits"),
      "{} must express dynamic facts with typed fields and keep static limitations in the contract documentation",
      display_path(root, &path)
    );
  }
}

fn display_path(root: &Path, path: &PathBuf) -> String {
  path.strip_prefix(root).unwrap_or(path).display().to_string()
}
