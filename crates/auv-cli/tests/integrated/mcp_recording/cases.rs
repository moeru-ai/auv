use std::path::PathBuf;

use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo, ErrorCode},
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
  let server = auv_cli::commands::mcp::McpServer::new(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).map_err(std::io::Error::other)?;
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
          "inputs": { "fixture-dir": "unused" },
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

#[cfg(unix)]
#[tokio::test]
async fn mcp_device_entry_uses_selected_daemon_and_does_not_create_a_run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let directory = tempfile::tempdir()?;
  let socket = directory.path().join("auv.sock");
  let discovery = directory.path().join("daemon.json");
  let store = directory.path().join("store");
  let profiles = directory.path().join("profiles.json");
  let endpoint = format!("unix://{}", socket.display());
  let mut daemon = tokio::process::Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--listen",
      &endpoint,
      "--store-root",
      store.to_str().unwrap(),
      "--discovery-file",
      discovery.to_str().unwrap(),
    ])
    .env("HOSTNAME", "mcp-entry-device")
    .kill_on_drop(true)
    .spawn()?;

  let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);

  while !discovery.exists() {
    assert!(tokio::time::Instant::now() < deadline, "daemon did not publish its discovery descriptor");

    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
  }

  let mut mcp = tokio::process::Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["mcp", "serve"])
    .env("AUV_DISCOVERY_FILE", &discovery)
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .kill_on_drop(true)
    .spawn()?;

  let transport = tokio::io::join(mcp.stdout.take().expect("MCP stdout"), mcp.stdin.take().expect("MCP stdin"));
  let client = TestClient.serve(transport).await?;

  for invalid in [
    serde_json::json!({ "device_name": "mcp-entry-device" }),
    serde_json::json!({ "device_name": "mcp-entry-device", "user": "neko", "session_selector": "seat0:42" }),
  ] {
    let error = client
      .call_tool(CallToolRequestParam {
        name: "device_ensure_user_session_unlocked".into(),
        arguments: Some(invalid.as_object().unwrap().clone()),
      })
      .await
      .expect_err("MCP must reject missing or conflicting OS targets");

    assert!(error.to_string().contains("set exactly one nonempty user or session_selector"), "unexpected MCP validation error: {error}");
  }

  let list = client
    .call_tool(CallToolRequestParam {
      name: "device_list_user_sessions".into(),
      arguments: Some(serde_json::json!({ "device_name": "mcp-entry-device" }).as_object().unwrap().clone()),
    })
    .await?;
  // ROOT CAUSE:
  // A headless Linux runner can have no logind session, so its supported
  // inventory succeeds with an empty list. The old test treated every
  // non-macOS result as unsupported. Check the routed response without
  // requiring a particular console login on the runner.
  let inventory_error = if list.is_error == Some(false) {
    assert!(list.structured_content.as_ref().unwrap()["sessions"].is_array(), "invalid Device session inventory: {list:?}");

    None
  } else {
    let reason = list.structured_content.as_ref().and_then(|value| value["reason"].as_str()).expect("typed inventory error");

    if cfg!(target_os = "macos") {
      panic!("macOS read-only inventory failed: {reason}");
    }

    assert!(
      matches!(reason, "UNSUPPORTED_OS_STATE" | "SERVICE_UNAVAILABLE" | "AMBIGUOUS_USER"),
      "unexpected Linux inventory error: {reason}"
    );

    Some(reason.to_owned())
  };

  for (name, arguments, expected_reason) in [
    (
      "device_get_user_session",
      serde_json::json!({ "device_name": "mcp-entry-device", "session_selector": "seat0:42" }),
      inventory_error.as_deref().unwrap_or("STALE_SESSION"),
    ),
    (
      "device_ensure_user_session_unlocked",
      serde_json::json!({ "device_name": "mcp-entry-device", "user": "__auv_no_such_user__" }),
      inventory_error.as_deref().unwrap_or("UNSUPPORTED_OS_STATE"),
    ),
  ] {
    let result = client
      .call_tool(CallToolRequestParam {
        name: name.into(),
        arguments: Some(arguments.as_object().unwrap().clone()),
      })
      .await?;

    assert_eq!(result.is_error, Some(true), "{name} unexpectedly claimed success: {result:?}");
    assert_eq!(result.structured_content.as_ref().unwrap()["reason"], expected_reason);
  }

  for (name, arguments) in [
    ("device_list_user_sessions", serde_json::json!({ "device_name": "different-device" })),
    ("device_get_user_session", serde_json::json!({ "device_name": "different-device", "session_selector": "seat0:42" })),
    ("device_ensure_user_session_unlocked", serde_json::json!({ "device_name": "different-device", "user": "neko" })),
  ] {
    let error = client
      .call_tool(CallToolRequestParam {
        name: name.into(),
        arguments: Some(arguments.as_object().unwrap().clone()),
      })
      .await
      .expect_err("mismatching Device selector must fail before entry RPC");

    match error {
      rmcp::service::ServiceError::McpError(error) => {
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{name} mapped caller selection as an infrastructure failure");
        assert!(error.message.contains("Device selection does not match"), "unexpected {name} selection error: {error:?}");
      }
      other => panic!("unexpected {name} selection failure: {other}"),
    }
  }

  let runs =
    tokio::process::Command::new(env!("CARGO_BIN_EXE_auv")).args(["run", "list", "--endpoint", &endpoint, "--json"]).output().await?;

  assert!(runs.status.success(), "run list failed: {}", String::from_utf8_lossy(&runs.stderr));
  assert_eq!(serde_json::from_slice::<serde_json::Value>(&runs.stdout)?, serde_json::json!([]));

  daemon.kill().await?;
  let unavailable = client
    .call_tool(CallToolRequestParam {
      name: "device_list_user_sessions".into(),
      arguments: Some(serde_json::json!({ "device_name": "mcp-entry-device" }).as_object().unwrap().clone()),
    })
    .await
    .expect_err("an unavailable selected daemon must fail");

  match unavailable {
    rmcp::service::ServiceError::McpError(error) => assert_eq!(error.code, ErrorCode::INTERNAL_ERROR),
    other => panic!("unexpected unavailable daemon failure: {other}"),
  }

  client.cancel().await?;
  mcp.kill().await?;
  Ok(())
}
