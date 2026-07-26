use std::path::PathBuf;

use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo},
};

#[derive(Debug, Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[tokio::test]
async fn mcp_invoke_returns_the_direct_result_and_writes_trace_records() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let store = tempfile::tempdir()?;
  let server = auv_cli::mcp::server(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).map_err(std::io::Error::other)?;
  let (server_transport, client_transport) = tokio::io::duplex(16_384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await?;

  let response = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": "scan.coverage",
          "dry_run": true,
          "store_root": store.path().display().to_string()
        })
        .as_object()
        .expect("invoke arguments")
        .clone(),
      ),
    })
    .await?;
  let direct = response.structured_content.expect("structured invoke result");
  let run_id = direct["run_id"].as_str().expect("run id");

  assert_eq!(direct["status"], "completed");
  assert_records_belong_to_run(store.path().join("records.jsonl"), run_id);

  client.cancel().await?;
  server_handle.await??;
  Ok(())
}

fn assert_records_belong_to_run(records_path: PathBuf, run_id: &str) {
  let records = std::fs::read_to_string(records_path).expect("MCP trace records");
  let records =
    records.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).expect("trace record envelope")).collect::<Vec<_>>();

  assert!(!records.is_empty());
  assert!(records.iter().all(|envelope| envelope["record"]["run_id"] == run_id));
  assert!(records.iter().any(|envelope| envelope["record"]["type"] == "event"));
}
