//! Direct CLI/MCP parity for TextEdit document.write (#101).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use auv_cli::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID;
use auv_cli::product_registry;
use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput};
use auv_tracing::{Context, EventPayload, FileRunStore, RunId, RunStore, configure, dispatcher};
use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo},
};

#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[derive(serde::Serialize)]
struct CliFrontendLifecycle {
  frontend: &'static str,
}

impl EventPayload for CliFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

#[tokio::test]
async fn textedit_rejected_fixture_input_preserves_direct_cli_mcp_parity() {
  let store_root = tempfile_dir("textedit-direct-parity");
  let store = Arc::new(FileRunStore::open(&store_root).expect("file store"));
  let dispatch = configure().run_store(store.clone()).build().expect("CLI dispatch");
  let cli_run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(cli_run_id));
  let command = product_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
  let input = InvokeCommandInput {
    command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
    target_application_id: Some("com.apple.TextEdit".to_string()),
    inputs: rejected_fixture_inputs(),
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  };
  let future = root.in_scope(|| {
    auv_tracing::emit_event!(CliFrontendLifecycle { frontend: "cli" });
    command.invoke(input)
  });
  let cli_value = root.instrument(future).await;
  dispatch.flush().await.expect("flush CLI run");

  let server = auv_cli::mcp::server(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).expect("TextEdit MCP server");
  let (server_transport, client_transport) = tokio::io::duplex(16384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await.expect("MCP server start");
    service.waiting().await.expect("MCP server exit");
  });
  let client = DummyClientHandler.serve(client_transport).await.expect("MCP client");
  let response = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": DOCUMENT_WRITE_COMMAND_ID,
          "target": { "application_id": "com.apple.TextEdit" },
          "inputs": rejected_fixture_inputs(),
          "inspect": { "store_root": store_root.display().to_string() }
        })
        .as_object()
        .expect("MCP arguments")
        .clone(),
      ),
    })
    .await
    .expect("MCP invoke");
  let mcp_value: serde_json::Value =
    serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("MCP text response").text)
      .expect("MCP JSON response");

  let cli_error = cli_value.expect_err("CLI rejects fixture-only input");
  assert_eq!(mcp_value["status"], "failed");
  assert_eq!(mcp_value["failure_message"], cli_error);
  let mcp_run_id = mcp_value["run_id"].as_str().expect("MCP run id").parse::<RunId>().expect("valid MCP run id");
  assert_ne!(cli_run_id, mcp_run_id);

  for run_id in [cli_run_id, mcp_run_id] {
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("frontend run snapshot");
    assert_eq!(snapshot.run_id(), run_id);
    assert!(snapshot.artifacts().is_empty(), "rejected input must not fabricate result artifacts");
    assert!(snapshot.events().iter().any(|event| event.schema().name().as_str() == "auv.frontend.lifecycle"));
  }

  client.cancel().await.expect("cancel MCP client");
  server_handle.await.expect("join MCP server");
  let _ = std::fs::remove_dir_all(store_root);
}

#[test]
fn product_help_lists_textedit_command_once() {
  let help = auv_cli_invoke::render_help_index(&product_registry());
  assert_eq!(help.matches(DOCUMENT_WRITE_COMMAND_ID).count(), 1);
  let command = product_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
  assert!(!auv_cli_invoke::render_command_help(&command).contains("--driver"));
  assert!(!auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry()).contains(DOCUMENT_WRITE_COMMAND_ID));
}

fn rejected_fixture_inputs() -> BTreeMap<String, String> {
  BTreeMap::from([
    ("content".to_string(), "AUV_TEXTEDIT_FIXTURE_MARKER".to_string()),
    ("driver".to_string(), "fixture".to_string()),
    ("verify".to_string(), "true".to_string()),
  ])
}

fn tempfile_dir(label: &str) -> PathBuf {
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("system clock").as_nanos();
  let path = std::env::temp_dir().join(format!("auv-{label}-{}-{nonce}", std::process::id()));
  std::fs::create_dir_all(&path).expect("temp dir");
  path
}
