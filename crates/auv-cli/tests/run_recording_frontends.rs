#[path = "support/instrumented_call.rs"]
mod instrumented_call;

use instrumented_call::{
  CountingCall, call_as_cli, call_as_cli_with_commit_unknown, call_as_library, call_as_mcp, call_as_mcp_with_telemetry_error,
};

#[tokio::test]
async fn library_cli_and_mcp_share_work_but_not_a_runner() {
  let call = CountingCall::new();
  let library = call_as_library(&call).await.unwrap();
  let cli = call_as_cli(&call).await.unwrap();
  let mcp = call_as_mcp(&call).await.unwrap();
  assert_eq!((library, cli.value, mcp.value), (7, 7, 7));
  assert_eq!(call.call_count(), 3);
  assert_ne!(cli.run_id, mcp.run_id);
  assert_eq!(cli.stored_event_run_ids, [cli.run_id]);
  assert_eq!(mcp.stored_event_run_ids, [mcp.run_id]);
  assert_eq!(cli.stored_event_count, 2);
  assert_eq!(mcp.stored_event_count, 2);
  assert_eq!(cli.tracing_error, None);
  assert_eq!(mcp.tracing_error, None);
}

#[tokio::test]
async fn tracing_failures_preserve_frontend_values_without_retry_or_advice() {
  let cli_call = CountingCall::new();
  let cli = call_as_cli_with_commit_unknown(&cli_call).await.unwrap();
  assert_eq!(cli.value, 7);
  assert_eq!(cli_call.call_count(), 1);
  assert!(cli.tracing_error.is_some());
  assert_no_canonical_advice(&cli.canonical_facts);

  let mcp_call = CountingCall::new();
  let mcp = call_as_mcp_with_telemetry_error(&mcp_call).await.unwrap();
  assert_eq!(mcp.value, 7);
  assert_eq!(mcp_call.call_count(), 1);
  assert!(mcp.tracing_error.is_some());
  assert_no_canonical_advice(&mcp.canonical_facts);
}

fn assert_no_canonical_advice(facts: &str) {
  for forbidden in [
    "operation-success",
    "verification",
    "retry",
    "recommended action",
  ] {
    assert!(!facts.contains(forbidden), "canonical facts contain {forbidden}: {facts}");
  }
}

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
}
