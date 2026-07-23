//! Direct CLI/MCP parity for TextEdit document.write (#101).

use std::collections::BTreeMap;
use std::sync::Arc;

use auv_cli::integrations::textedit::{DOCUMENT_WRITE_COMMAND_ID, fixture_document_write_cli, map_verification_result};
use auv_cli::product_registry;
use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput};
use auv_runtime::contract::VerificationResult;
use auv_runtime::mcp::McpInvokeInput;
use auv_runtime::run_read::list_input_action_results;
use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

#[derive(serde::Deserialize)]
struct RecordedVerification {
  verification: VerificationResult,
}

#[tokio::test]
async fn textedit_fixture_reaches_shared_domain_through_cli_and_mcp_mappings() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("frontend dispatch");
  let cli_run_id = RunId::new();
  let cli_root = dispatcher::with_default(&dispatch, || Context::root(cli_run_id));
  let cli_future = cli_root.in_scope(|| fixture_document_write_cli(cli_input(), Some("different".to_string())));
  let (_cli_output, cli_report) = cli_root.instrument(cli_future).await.expect("CLI fixture mapping");
  dispatch.flush().await.expect("flush CLI run");

  let mcp_run_id = RunId::new();
  let mcp_root = dispatcher::with_default(&dispatch, || Context::root(mcp_run_id));
  let mcp_future = mcp_root.in_scope(|| auv_cli::mcp::fixture_textedit_document_write(mcp_input(), Some("different".to_string())));
  let (_mcp_outcome, mcp_report) = mcp_root.instrument(mcp_future).await.expect("MCP fixture mapping");
  dispatch.flush().await.expect("flush MCP run");

  assert_ne!(cli_run_id, mcp_run_id);
  assert_eq!(cli_report, mcp_report);
  let cli_verification = map_verification_result(cli_report.verification.as_ref().expect("CLI verification"));
  let mcp_verification = map_verification_result(mcp_report.verification.as_ref().expect("MCP verification"));
  assert_eq!(cli_verification, mcp_verification);
  assert_eq!(cli_verification.semantic_matched, Some(false));

  for run_id in [cli_run_id, mcp_run_id] {
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("frontend run snapshot");
    assert_eq!(snapshot.run_id(), run_id);
    assert_eq!(list_input_action_results(store.as_ref(), &snapshot).await.expect("typed input results").len(), 2);
    let event = snapshot
      .events()
      .iter()
      .find(|event| event.schema().name().as_str() == "auv.textedit.document_write.verification")
      .expect("app-owned verification event");
    assert_eq!(event.schema().version().get(), 1);
    let recorded: RecordedVerification = serde_json::from_str(event.payload().get()).expect("typed verification payload");
    assert_eq!(recorded.verification, cli_verification);
  }
}

#[test]
fn product_help_lists_textedit_command_once() {
  let help = auv_cli_invoke::render_help_index(&product_registry());
  assert_eq!(help.matches(DOCUMENT_WRITE_COMMAND_ID).count(), 1);
  let command = product_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
  assert!(!auv_cli_invoke::render_command_help(&command).contains("--driver"));
  assert!(!auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry()).contains(DOCUMENT_WRITE_COMMAND_ID));
}

fn document_write_inputs() -> BTreeMap<String, String> {
  BTreeMap::from([
    ("content".to_string(), "AUV_TEXTEDIT_FIXTURE_MARKER".to_string()),
    ("verify".to_string(), "true".to_string()),
  ])
}

fn cli_input() -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
    target_application_id: Some("com.apple.TextEdit".to_string()),
    inputs: document_write_inputs(),
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  }
}

fn mcp_input() -> McpInvokeInput {
  McpInvokeInput {
    target_application_id: Some("com.apple.TextEdit".to_string()),
    target_label: None,
    inputs: document_write_inputs(),
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  }
}
