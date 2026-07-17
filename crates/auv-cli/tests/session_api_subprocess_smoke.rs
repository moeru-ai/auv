//! Subprocess loopback smoke (API-S1).
//!
//! Spawns the built `auv` binary via `CARGO_BIN_EXE_auv` and exercises
//! CreateSession → Invoke → GetOperation through the real `session serve` entry.

use std::path::PathBuf;
use std::time::Duration;

use auv_api_proto::v1::session as proto;
use auv_api_proto::v1::session::session_service_client::SessionServiceClient;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;
use tonic::transport::Channel;

const SERVER_READY_PREFIX: &str = "session API: grpc://";
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(30);
const INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT: &str = "auv.api.session.invoke_synthetic_operation_result";

fn temp_store_root(label: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("auv-session-api-{label}-{}", std::process::id()));
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
async fn session_api_subprocess_smoke_invoke_then_get_operation_round_trips() {
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
  let operation = invoke_response.operation.expect("operation ref");

  let response = client
    .get_operation(proto::GetOperationRequest {
      operation: Some(operation),
    })
    .await
    .expect("get_operation should succeed after invoke")
    .into_inner();

  assert_eq!(response.status, "completed");
  assert_eq!(response.output_summary, "scan.coverage dry-run");
  assert_eq!(response.operation.expect("operation ref").operation_id, "scan.coverage");
  assert!(response.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

  child.kill().await.expect("kill session serve child");
  let _ = child.wait().await;
}
