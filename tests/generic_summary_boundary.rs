#[test]
fn descriptions_and_artifact_records_do_not_use_generic_summary_fields() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let paths = [
    "crates/auv-cli-invoke/src/command.rs",
    "crates/auv-driver-common/src/traits.rs",
    "crates/auv-driver-macos/src/descriptor.rs",
    "crates/auv-driver-windows/src/descriptor.rs",
    "crates/auv-driver-linux/src/descriptor.rs",
    "crates/auv-game-minecraft/src/dataset.rs",
  ];

  for relative in paths {
    let source = std::fs::read_to_string(root.join(relative)).unwrap_or_else(|error| panic!("read {relative}: {error}"));
    assert!(!source.contains("pub summary:"), "{relative} still exposes an ambiguous summary field");
  }

  let mcp = std::fs::read_to_string(root.join("src/mcp.rs")).expect("read MCP frontend");
  assert!(!mcp.contains("\"summary\": command."), "MCP command metadata must call static help text a description");

  let driver = std::fs::read_to_string(root.join("crates/auv-driver-common/src/lib.rs")).expect("read driver exports");
  assert!(!driver.contains("pub mod operation;"), "unused speculative OperationSpec metadata must not remain public");
  assert!(!driver.contains("OperationSpec"), "unused speculative OperationSpec must not remain exported");
}

#[test]
fn live_click_wiring_returns_typed_delivery_instead_of_a_summary_string() {
  let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  for relative in [
    "crates/auv-game-osu/src/visual_truth_spatial_query_action_wiring.rs",
    "crates/auv-cli/src/integrations/minecraft/query_live_action.rs",
    "crates/auv-cli/src/integrations/osu/query_live_action.rs",
  ] {
    let source = std::fs::read_to_string(root.join(relative)).expect("source should be readable");
    assert!(!source.contains("click_summary"), "{relative} still uses an optional summary as delivery evidence");
    assert!(!source.contains("Ok(format!(\"clicked window point"), "{relative} still returns prose as a successful click result");
  }
}
