//! External gRPC client smoke tests (API-P13).
//!
//! Real [`SessionServiceClient`] over loopback TCP — the external client
//! perspective on CreateSession, Invoke, and GetOperation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use auv_api_proto::v1::session as proto;
use auv_api_proto::v1::session::session_service_client::SessionServiceClient;
use tonic::Code;
use tonic::transport::Channel;

use crate::api::session_service::transport::{
  DEFAULT_SESSION_API_HOST, SessionApiServeConfig, bind_session_api, serve_on_listener,
};
use crate::model::now_millis;

static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_store_root() -> PathBuf {
  let unique = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "auv-session-api-client-smoke-{}-{}",
    now_millis(),
    unique
  ))
}

async fn with_smoke_server<T, F, Fut>(store_root: PathBuf, f: F) -> T
where
  F: FnOnce(SessionServiceClient<Channel>) -> Fut,
  Fut: std::future::Future<Output = T>,
{
  let cleanup_root = store_root.clone();
  let config = SessionApiServeConfig {
    host: DEFAULT_SESSION_API_HOST.to_string(),
    port: 0,
    store_root: store_root.clone(),
  };
  let (listener, local_address) = bind_session_api(&config).await.expect("bind");
  let server = tokio::spawn(serve_on_listener(listener, local_address, store_root));

  let endpoint = format!("http://{local_address}");
  let client = SessionServiceClient::connect(endpoint)
    .await
    .expect("connect client");

  let output = f(client).await;

  server.abort();
  let _ = server.await;
  let _ = std::fs::remove_dir_all(cleanup_root);
  output
}

async fn create_session(client: &mut SessionServiceClient<Channel>) -> proto::SessionRef {
  client
    .create_session(proto::CreateSessionRequest {
      client_label: "session-api-smoke".to_string(),
    })
    .await
    .expect("create_session")
    .into_inner()
    .session
    .expect("session ref")
}

async fn invoke_fixture_observe(
  client: &mut SessionServiceClient<Channel>,
  session: proto::SessionRef,
) -> proto::InvokeResponse {
  client
    .invoke(proto::InvokeRequest {
      session: Some(session),
      command_id: "fixture.observe".to_string(),
      json_payload: Vec::new(),
    })
    .await
    .expect("invoke")
    .into_inner()
}

#[tokio::test]
async fn session_api_smoke_external_client_invoke_fixture_observe() {
  let store_root = temp_store_root();
  with_smoke_server(store_root, |mut client| async move {
    let session = create_session(&mut client).await;
    assert!(!session.session_id.is_empty());
    let response = invoke_fixture_observe(&mut client, session).await;

    assert_eq!(response.status, "completed");
    let operation = response.operation.expect("operation ref");
    assert!(!operation.run_id.is_empty());
    assert_eq!(operation.operation_id, "fixture.observe");
  })
  .await;
}

#[tokio::test]
async fn session_api_smoke_get_operation_requires_persisted_operation_result() {
  let store_root = temp_store_root();
  with_smoke_server(store_root, |mut client| async move {
    let session = create_session(&mut client).await;
    let invoke_response = invoke_fixture_observe(&mut client, session).await;
    assert_eq!(invoke_response.status, "completed");
    let operation = invoke_response.operation.expect("operation ref");

    let status = client
      .get_operation(proto::GetOperationRequest {
        operation: Some(operation),
      })
      .await
      .expect_err("get_operation should fail without persisted operation result");

    assert_eq!(status.code(), Code::FailedPrecondition);
    assert!(
      status.message().contains("no persisted operation result"),
      "unexpected message: {}",
      status.message()
    );
  })
  .await;
}

#[tokio::test]
async fn session_api_smoke_get_operation_round_trips_wire_command_id() {
  use crate::api::session_service::test_fixtures::{
    music_runtime_summary, music_search_operation, persist_operation_result_and_summary_run,
  };

  let run_id = "run-p13-wire-command-id";
  let fixture = persist_operation_result_and_summary_run(
    "client-smoke-get-operation",
    run_id,
    &music_search_operation(run_id),
    &music_runtime_summary(run_id),
  );
  let store_root = fixture.root.clone();

  with_smoke_server(store_root, |mut client| async move {
    // GetOperation is keyed by run_id; the session is only transport admission.
    let _session = create_session(&mut client).await;

    let response = client
      .get_operation(proto::GetOperationRequest {
        operation: Some(proto::OperationRef {
          run_id: run_id.to_string(),
          operation_id: "music.search".to_string(),
        }),
      })
      .await
      .expect("get_operation")
      .into_inner();

    assert_eq!(response.status, "completed");
    assert_eq!(response.output_summary, "did the thing");
    let operation = response.operation.expect("operation ref");
    assert_eq!(operation.run_id, run_id);
    assert_eq!(operation.operation_id, "music.search");
  })
  .await;

  fixture.cleanup();
}
