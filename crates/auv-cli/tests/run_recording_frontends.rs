#[test]
fn cli_and_mcp_do_not_call_a_shared_recording_wrapper() {
  let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let cli = std::fs::read_to_string(manifest.join("src/cli_frontend.rs")).unwrap();
  let mcp = std::fs::read_to_string(manifest.join("../../src/mcp.rs")).unwrap();
  for forbidden in [
    "RunRecordingBackend",
    "recorded::",
    "execute_with_tracing",
    "run_operation",
  ] {
    assert!(!cli.contains(forbidden), "CLI uses {forbidden}");
    assert!(!mcp.contains(forbidden), "MCP uses {forbidden}");
  }

  for forbidden in [
    "InvokeCommandInput",
    "InvokeCommandOutput",
    "command.invoke(",
  ] {
    assert!(!mcp.contains(forbidden), "MCP executes through CLI presentation boundary {forbidden}");
  }

  assert!(cli.contains("root.in_scope"), "CLI must construct the domain future inside its root context");
  assert!(cli.contains("root.instrument"), "CLI must poll the domain future through its root context");
  assert!(mcp.contains("root.in_scope"), "MCP must construct the domain future inside its root context");
  assert!(mcp.contains("root.instrument"), "MCP must poll the domain future through its root context");
}
