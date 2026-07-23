//! Subprocess loopback smoke (API-S1).
//!
//! Spawns the built `auv` binary via `CARGO_BIN_EXE_auv` and exercises
//! CreateSession and Invoke through the real `session serve` entry, then reads
//! the independently recorded canonical run from the configured authority.

use std::path::PathBuf;
use std::time::Duration;

use auv_api_proto::v1::session as proto;
use auv_api_proto::v1::session::session_service_client::SessionServiceClient;
use auv_tracing::{FileRunStore, RunId, RunStore};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use tonic::transport::Channel;

const SERVER_READY_PREFIX: &str = "session API: grpc://";
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(30);

fn temp_store_root(label: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("auv-session-api-{label}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&dir);
  std::fs::create_dir_all(&dir).expect("create temp store_root");
  dir
}

fn parse_grpc_endpoint(line: &str) -> Option<String> {
  line.strip_prefix(SERVER_READY_PREFIX).map(|rest| format!("http://{rest}"))
}

async fn wait_for_server_endpoint(stdout: tokio::process::ChildStdout) -> String {
  let mut lines = BufReader::new(stdout).lines();
  loop {
    let line = lines.next_line().await.expect("read server stdout").expect("server stdout closed before ready line");
    if let Some(endpoint) = parse_grpc_endpoint(&line) {
      return endpoint;
    }
  }
}

async fn create_session(client: &mut SessionServiceClient<Channel>) -> proto::SessionRef {
  client
    .create_session(proto::CreateSessionRequest {
      client_label: "session-api-subprocess-smoke".to_string(),
    })
    .await
    .expect("create_session")
    .into_inner()
    .session
    .expect("session ref")
}

async fn invoke_sample_command(client: &mut SessionServiceClient<Channel>, session: proto::SessionRef) -> proto::InvokeResponse {
  client
    .invoke(proto::InvokeRequest {
      session: Some(session),
      command_id: "scan.coverage".to_string(),
      json_payload: br#"{"dry_run":true}"#.to_vec(),
    })
    .await
    .expect("invoke")
    .into_inner()
}

#[tokio::test]
async fn session_api_subprocess_smoke_returns_direct_value_and_records_canonical_run() {
  let store_root = temp_store_root("subprocess-smoke");
  struct Cleanup(PathBuf);
  impl Drop for Cleanup {
    fn drop(&mut self) {
      let _ = std::fs::remove_dir_all(&self.0);
    }
  }
  let _cleanup = Cleanup(store_root.clone());
  let store_root_arg = store_root.to_str().expect("store_root path must be valid UTF-8").to_string();
  let auv_bin = env!("CARGO_BIN_EXE_auv");

  let mut child = Command::new(auv_bin)
    .args([
      "session",
      "serve",
      "--host",
      "127.0.0.1",
      "--port",
      "0",
      "--store-root",
      &store_root_arg,
    ])
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .expect("spawn auv session serve");

  let stdout = child.stdout.take().expect("child stdout");
  let endpoint =
    timeout(SERVER_READY_TIMEOUT, wait_for_server_endpoint(stdout)).await.expect("timed out waiting for session API ready line");

  let mut client = SessionServiceClient::connect(endpoint).await.expect("connect gRPC client");

  let session = create_session(&mut client).await;
  assert!(!session.session_id.is_empty());

  let invoke_response = invoke_sample_command(&mut client, session).await;
  assert_eq!(invoke_response.status, "completed");
  assert!(invoke_response.failure_message.is_empty());
  assert!(invoke_response.recording_failure.is_empty());
  assert!(invoke_response.known_limits.is_empty());
  assert!(invoke_response.artifacts.is_empty(), "dry-run coverage must not claim artifacts");
  let run_id = invoke_response.run_id.parse::<RunId>().expect("canonical run id");

  let store = FileRunStore::open(&store_root).expect("open session run authority");
  let snapshot = store.load_snapshot(run_id).await.expect("load session run").expect("recorded session run");
  assert_eq!(snapshot.run_id(), run_id);
  assert!(snapshot.artifacts().is_empty());
  assert_eq!(snapshot.events().len(), 1);
  assert_eq!(snapshot.events()[0].schema().name().as_str(), "auv.frontend.lifecycle");
  assert_eq!(
    serde_json::from_str::<serde_json::Value>(snapshot.events()[0].payload().get()).expect("frontend lifecycle payload"),
    serde_json::json!({ "frontend": "session-api" })
  );

  child.kill().await.expect("kill session serve child");
  let _ = child.wait().await;
}
