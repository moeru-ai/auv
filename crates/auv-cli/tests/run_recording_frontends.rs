use std::path::{Path, PathBuf};
use std::process::Command;

use auv_tracing::{FileRunStore, RunId, RunStore};
use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo},
};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

struct TempStore(PathBuf);

impl TempStore {
  fn new(label: &str) -> Self {
    let path = std::env::temp_dir().join(format!(
      "auv-task16-{label}-{}-{}",
      std::process::id(),
      std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos()
    ));
    std::fs::create_dir_all(&path).expect("create test store");
    Self(path)
  }

  fn path(&self) -> &Path {
    &self.0
  }
}

impl Drop for TempStore {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn run_cli(arguments: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_auv")).args(arguments).output().expect("run auv CLI")
}

fn parse_cli_json(output: &std::process::Output) -> Value {
  assert!(output.status.success(), "CLI failed: {}", String::from_utf8_lossy(&output.stderr));
  serde_json::from_slice(&output.stdout).expect("CLI JSON output")
}

async fn load_snapshot(store_root: &Path, run_id: &str) -> auv_tracing::RunSnapshot {
  let store = FileRunStore::open(store_root).expect("open V1 store");
  store
    .load_snapshot(run_id.parse::<RunId>().expect("valid run id"))
    .await
    .expect("load V1 snapshot")
    .expect("returned run id must be persisted")
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

#[tokio::test]
async fn cli_dry_run_returns_a_v1_run_that_cli_inspect_reads() {
  let store = TempStore::new("cli-dry-run");
  let store_arg = store.path().to_str().expect("UTF-8 store path");
  let invoke = parse_cli_json(&run_cli(&[
    "invoke",
    "scan.coverage",
    "--dry-run",
    "--json",
    "--store-root",
    store_arg,
  ]));
  let run_id = invoke["run_id"].as_str().expect("run id");

  let snapshot = load_snapshot(store.path(), run_id).await;
  assert_eq!(snapshot.run_id().to_string(), run_id);
  assert_eq!(snapshot.events().len(), 1);
  assert!(snapshot.artifacts().is_empty());

  let inspect = run_cli(&["inspect", run_id, "--store-root", store_arg]);
  assert!(inspect.status.success(), "inspect failed: {}", String::from_utf8_lossy(&inspect.stderr));
  let document: Value = serde_json::from_slice(&inspect.stdout).expect("V1 inspect JSON");
  assert_eq!(document["run_id"], run_id);
  assert_eq!(document["events"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn cli_artifact_command_returns_canonical_uri_that_cli_inspect_reads() {
  let store = TempStore::new("cli-artifact");
  let store_arg = store.path().to_str().expect("UTF-8 store path");
  let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0");
  let fixture_arg = fixture.to_str().expect("UTF-8 fixture path");
  let invoke = parse_cli_json(&run_cli(&[
    "invoke",
    "scan.frame",
    "--fixture-dir",
    fixture_arg,
    "--json",
    "--store-root",
    store_arg,
  ]));
  let run_id = invoke["run_id"].as_str().expect("run id");
  let returned_uris = invoke["artifacts"]
    .as_array()
    .expect("artifact array")
    .iter()
    .map(|artifact| artifact["uri"].as_str().expect("canonical artifact URI").to_string())
    .collect::<Vec<_>>();

  let snapshot = load_snapshot(store.path(), run_id).await;
  let snapshot_uris = snapshot.artifacts().values().map(|artifact| artifact.metadata().uri().to_string()).collect::<Vec<_>>();
  assert_eq!(returned_uris, snapshot_uris);
  assert!(returned_uris.iter().all(|uri| uri.starts_with(&format!("auv://runs/{run_id}/artifacts/"))));

  let inspect = run_cli(&["inspect", run_id, "--store-root", store_arg]);
  assert!(inspect.status.success(), "inspect failed: {}", String::from_utf8_lossy(&inspect.stderr));
  let document: Value = serde_json::from_slice(&inspect.stdout).expect("V1 inspect JSON");
  let inspected_uris = document["artifacts"]
    .as_array()
    .expect("inspect artifacts")
    .iter()
    .map(|artifact| artifact["uri"].as_str().expect("inspect URI"))
    .collect::<Vec<_>>();
  assert_eq!(inspected_uris, returned_uris);
}

#[tokio::test]
async fn mcp_dry_run_returns_a_v1_run_that_mcp_inspect_reads() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let store = TempStore::new("mcp-dry-run");
  let server = auv_cli::mcp::server(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).map_err(std::io::Error::other)?;
  let (server_transport, client_transport) = tokio::io::duplex(16384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = DummyClientHandler.serve(client_transport).await?;
  let store_root = store.path().display().to_string();

  let invoke = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": "scan.coverage",
          "dry_run": true,
          "inspect": { "store_root": store_root }
        })
        .as_object()
        .expect("arguments")
        .clone(),
      ),
    })
    .await?;
  let invoke: Value = serde_json::from_str(&invoke.content[0].raw.as_text().expect("invoke text").text)?;
  let run_id = invoke["run_id"].as_str().expect("run id");
  let snapshot = load_snapshot(store.path(), run_id).await;
  assert_eq!(snapshot.events().len(), 1);

  let inspect = client
    .call_tool(CallToolRequestParam {
      name: "run_inspect".into(),
      arguments: Some(
        serde_json::json!({ "run_id": run_id, "store_root": store.path().display().to_string() }).as_object().expect("arguments").clone(),
      ),
    })
    .await?;
  let inspect: Value = serde_json::from_str(&inspect.content[0].raw.as_text().expect("inspect text").text)?;
  assert_eq!(inspect["run_id"], run_id);
  assert_eq!(inspect["events"].as_array().map(Vec::len), Some(1));

  client.cancel().await?;
  server_handle.await??;
  Ok(())
}

#[tokio::test]
async fn mcp_artifact_command_returns_canonical_uri_that_mcp_inspect_reads() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let store = TempStore::new("mcp-artifact");
  let server = auv_cli::mcp::server(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).map_err(std::io::Error::other)?;
  let (server_transport, client_transport) = tokio::io::duplex(16384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = DummyClientHandler.serve(client_transport).await?;
  let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0");

  let invoke = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": "scan.frame",
          "inputs": { "fixture-dir": fixture.display().to_string() },
          "inspect": { "store_root": store.path().display().to_string() }
        })
        .as_object()
        .expect("arguments")
        .clone(),
      ),
    })
    .await?;
  let invoke: Value = serde_json::from_str(&invoke.content[0].raw.as_text().expect("invoke text").text)?;
  let run_id = invoke["run_id"].as_str().expect("run id");
  let returned_uris = invoke["artifacts"]
    .as_array()
    .expect("artifact array")
    .iter()
    .map(|artifact| artifact["uri"].as_str().expect("canonical URI").to_string())
    .collect::<Vec<_>>();

  let snapshot = load_snapshot(store.path(), run_id).await;
  let snapshot_uris = snapshot.artifacts().values().map(|artifact| artifact.metadata().uri().to_string()).collect::<Vec<_>>();
  assert_eq!(returned_uris, snapshot_uris);
  assert_eq!(returned_uris.len(), 2);

  let inspect = client
    .call_tool(CallToolRequestParam {
      name: "run_inspect".into(),
      arguments: Some(
        serde_json::json!({ "run_id": run_id, "store_root": store.path().display().to_string() }).as_object().expect("arguments").clone(),
      ),
    })
    .await?;
  let inspect: Value = serde_json::from_str(&inspect.content[0].raw.as_text().expect("inspect text").text)?;
  let inspected_uris = inspect["artifacts"]
    .as_array()
    .expect("inspect artifacts")
    .iter()
    .map(|artifact| artifact["uri"].as_str().expect("inspect URI").to_string())
    .collect::<Vec<_>>();
  assert_eq!(inspected_uris, returned_uris);

  client.cancel().await?;
  server_handle.await??;
  Ok(())
}
