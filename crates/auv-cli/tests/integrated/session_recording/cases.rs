use std::time::Duration;

use auv_api_proto::v1::session as proto;
use auv_api_proto::v1::session::session_service_client::SessionServiceClient;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

const SERVER_READY_PREFIX: &str = "session API: grpc://";

#[tokio::test]
async fn session_invoke_returns_the_direct_result_and_writes_trace_records() {
  let store = tempfile::tempdir().expect("temporary tracing store");
  let mut child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "session",
      "serve",
      "--host",
      "127.0.0.1",
      "--port",
      "0",
      "--store-root",
      store.path().to_str().expect("UTF-8 store path"),
    ])
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .expect("spawn session server");
  let stdout = child.stdout.take().expect("session server stdout");
  let endpoint = timeout(Duration::from_secs(30), wait_for_endpoint(stdout)).await.expect("session server ready timeout");
  let mut client = SessionServiceClient::connect(endpoint).await.expect("connect session client");
  let session = client
    .create_session(proto::CreateSessionRequest {
      client_label: "integrated-session-recording".to_string(),
    })
    .await
    .expect("create session")
    .into_inner()
    .session
    .expect("session ref");
  let response = client
    .invoke(proto::InvokeRequest {
      session: Some(session),
      command_id: "scan.coverage".to_string(),
      json_payload: br#"{"dry_run":true}"#.to_vec(),
    })
    .await
    .expect("invoke session command")
    .into_inner();

  assert!(matches!(response.terminal, Some(proto::invoke_response::Terminal::Completed(_))));
  assert!(response.recording_failure.is_empty());
  assert_records_belong_to_run(store.path().join("records.jsonl"), &response.run_id);

  child.kill().await.expect("stop session server");
  let _ = child.wait().await;
}

async fn wait_for_endpoint(stdout: tokio::process::ChildStdout) -> String {
  let mut lines = BufReader::new(stdout).lines();
  loop {
    let line = lines.next_line().await.expect("read session server stdout").expect("session server closed before ready");
    if let Some(address) = line.strip_prefix(SERVER_READY_PREFIX) {
      return format!("http://{address}");
    }
  }
}

fn assert_records_belong_to_run(records_path: std::path::PathBuf, run_id: &str) {
  let records = std::fs::read_to_string(records_path).expect("session trace records");
  let records =
    records.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).expect("trace record envelope")).collect::<Vec<_>>();

  assert!(!records.is_empty());
  assert!(records.iter().all(|envelope| envelope["record"]["run_id"] == run_id));
  assert!(records.iter().any(|envelope| envelope["record"]["type"] == "event"));
}
