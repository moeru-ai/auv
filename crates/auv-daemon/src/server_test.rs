use auv_api_client::protocol::grpc::Client as GrpcClient;
use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::transport::websocket::v1 as transport_proto;
use futures_util::{SinkExt as _, StreamExt as _};
use prost::Message as _;
use tokio_util::sync::CancellationToken;

use super::*;

fn config(listeners: Vec<ListenEndpoint>, root: &std::path::Path) -> Config {
  Config {
    listeners,
    store_root: root.join("store"),
    pairing_store: None,
    discovery_file: None,
    publish_discovery: false,
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  }
}

const DISPLAY_SERVICE: &str = "auv.api.driver.v1.DisplayService";
const TEST_RUNNER_CLASS: &str = "example.runner.remote";

#[cfg(windows)]
#[tokio::test]
async fn owner_named_pipe_serves_the_typed_control_api() {
  let root = tempfile::tempdir().unwrap();
  let name = format!("auv-test-{}", uuid::Uuid::now_v7());
  let server = Server::bind(config(vec![ListenEndpoint::NamedPipe { name: name.clone() }], root.path())).await.unwrap();
  assert_eq!(server.endpoint(), &BoundEndpoint::NamedPipe(name.clone()));
  assert_eq!(server.discovery_endpoint(), Some(&BoundEndpoint::NamedPipe(name.clone())));

  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  // ROOT CAUSE:
  //
  // If two clients opened the pipe together, Windows returned ERROR_PIPE_BUSY
  // before the server created its next listening instance.
  //
  // Before the fix, one concurrent connection failed immediately. The client
  // now retries only this transient error within a bounded local window.
  let (first, second) = tokio::join!(
    GrpcClient::connect(auv_api_client::ConnectEndpoint::NamedPipe(name.clone())),
    GrpcClient::connect(auv_api_client::ConnectEndpoint::NamedPipe(name)),
  );
  for client in [first.unwrap(), second.unwrap()] {
    let devices = client.devices().list_devices().await.unwrap();
    assert_eq!(devices.len(), 1);
    assert!(devices[0].local);
  }

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[derive(Default)]
struct DisplayFixture;

#[tonic::async_trait]
impl driver_proto::display_service_server::DisplayService for DisplayFixture {
  async fn list_displays(
    &self,
    _request: tonic::Request<driver_proto::ListDisplaysRequest>,
  ) -> Result<tonic::Response<driver_proto::ListDisplaysResponse>, tonic::Status> {
    Ok(tonic::Response::new(driver_proto::ListDisplaysResponse {
      displays: vec![driver_proto::Display {
        display_id: "display-fixture".into(),
        ..Default::default()
      }],
    }))
  }
}

async fn remote_display_runner() -> (runner_provider::RunnerProviderConfig, tokio::task::JoinHandle<Result<(), tonic::transport::Error>>) {
  use driver_proto::display_service_server::DisplayServiceServer;
  use tokio_stream::wrappers::TcpListenerStream;

  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = listener.local_addr().unwrap();
  let display = DisplayServiceServer::new(DisplayFixture);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<DisplayServiceServer<DisplayFixture>>().await;
  let descriptor = auv_api_proto::descriptor_set_for_service(DISPLAY_SERVICE).unwrap();
  let reflection = tonic_reflection::server::Builder::configure().register_encoded_file_descriptor_set(&descriptor).build_v1().unwrap();
  let task = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(health)
      .add_service(reflection)
      .add_service(display)
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });
  (
    runner_provider::RunnerProviderConfig {
      runner_class: TEST_RUNNER_CLASS.into(),
      runtime: runner_provider::RunnerRuntime::RemoteGrpc(runner_provider::RemoteGrpcRunnerRuntime {
        endpoint: format!("http://{address}"),
      }),
    },
    task,
  )
}

#[tokio::test]
async fn typed_control_and_rest_share_the_daemon_backend() {
  let root = tempfile::tempdir().unwrap();
  let server = Server::bind(config(
    vec![ListenEndpoint::Tcp {
      host: "127.0.0.1".into(),
      port: 0,
    }],
    root.path(),
  ))
  .await
  .unwrap();
  let BoundEndpoint::Tcp(address) = server.endpoint() else {
    panic!("TCP endpoint")
  };
  let address = *address;
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let client = GrpcClient::connect(format!("http://{address}").parse().unwrap()).await.unwrap();
  let devices = client.devices().list_devices().await.unwrap();
  assert_eq!(devices.len(), 1);
  assert!(devices[0].local);
  let http = reqwest::Client::new();

  let discovery = http.get(format!("http://{address}/apis/auv/daemon/v1")).send().await.unwrap();
  assert_eq!(discovery.status(), reqwest::StatusCode::OK);
  let discovery: serde_json::Value = serde_json::from_slice(&discovery.bytes().await.unwrap()).unwrap();
  assert_eq!(discovery["resources"].as_array().unwrap().len(), 1);

  let response = http.get(format!("http://{address}/apis/auv/daemon/v1/devices")).send().await.unwrap();
  assert_eq!(response.status(), reqwest::StatusCode::OK);
  let listed: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
  assert_eq!(listed["devices"][0]["local"], true);
  let device_id = &devices[0].r#ref.as_ref().unwrap().device_id;
  assert_eq!(listed["devices"][0]["ref"]["deviceId"], device_id.as_str());

  let device = http
    .post(format!("http://{address}/apis/auv/daemon/v1/devices:get"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(serde_json::json!({"device": {"deviceId": device_id}}).to_string())
    .send()
    .await
    .unwrap();
  assert_eq!(device.status(), reqwest::StatusCode::OK);

  let created = http
    .post(format!("http://{address}/apis/auv/runtime/v1/runs"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body("{}")
    .send()
    .await
    .unwrap();
  assert_eq!(created.status(), reqwest::StatusCode::OK);
  let created: serde_json::Value = serde_json::from_slice(&created.bytes().await.unwrap()).unwrap();
  let run_id = created["run"]["ref"]["runId"].as_str().unwrap();

  let stopped = http
    .post(format!("http://{address}/apis/auv/runtime/v1/runs:stop"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(
      serde_json::json!({
        "run": {"runId": run_id},
        "outcome": "RUN_OUTCOME_CANCELED",
      })
      .to_string(),
    )
    .send()
    .await
    .unwrap();
  assert_eq!(stopped.status(), reqwest::StatusCode::OK);
  let stopped: serde_json::Value = serde_json::from_slice(&stopped.bytes().await.unwrap()).unwrap();
  assert_eq!(stopped["run"]["phase"], "RUN_PHASE_CANCELED");

  let runners = http.get(format!("http://{address}/apis/auv/runtime/v1/runners")).send().await.unwrap();
  assert_eq!(runners.status(), reqwest::StatusCode::OK);
  let runners: serde_json::Value = serde_json::from_slice(&runners.bytes().await.unwrap()).unwrap();
  assert!(runners["runners"].is_array());

  let runner_classes = http
    .post(format!("http://{address}/apis/auv/runtime/v1/runnerclasses:list"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body("{}")
    .send()
    .await
    .unwrap();
  assert_eq!(runner_classes.status(), reqwest::StatusCode::OK);
  let runner_classes: serde_json::Value = serde_json::from_slice(&runner_classes.bytes().await.unwrap()).unwrap();
  assert!(runner_classes["runnerClasses"].is_array());
  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[tokio::test]
async fn http_and_websocket_invoke_share_the_runner_route() {
  let root = tempfile::tempdir().unwrap();
  let (provider, runner_task) = remote_display_runner().await;
  let mut daemon_config = config(
    vec![ListenEndpoint::Tcp {
      host: "127.0.0.1".into(),
      port: 0,
    }],
    root.path(),
  );
  daemon_config.runner_providers.push(provider);
  let server = Server::bind(daemon_config).await.unwrap();
  let BoundEndpoint::Tcp(address) = server.endpoint() else {
    panic!("TCP endpoint")
  };
  let address = *address;
  let shutdown = CancellationToken::new();
  let server_task = tokio::spawn(server.serve(shutdown.clone()));

  let response = reqwest::Client::new()
    .post(format!("http://{address}/apis/auv/runtime/v1/invoke/{DISPLAY_SERVICE}/ListDisplays"))
    .header(reqwest::header::CONTENT_TYPE, "application/protobuf")
    .header("auv-runner-class", TEST_RUNNER_CLASS)
    .body(driver_proto::ListDisplaysRequest {}.encode_to_vec())
    .send()
    .await
    .unwrap();
  assert_eq!(response.status(), reqwest::StatusCode::OK);
  let output = driver_proto::ListDisplaysResponse::decode(response.bytes().await.unwrap()).unwrap();
  assert_eq!(output.displays[0].display_id, "display-fixture");

  let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/apis/auv/runtime/v1/invoke")).await.unwrap();
  socket
    .send(tokio_tungstenite::tungstenite::Message::Binary(
      transport_proto::ClientMessage {
        message: Some(transport_proto::client_message::Message::Open(transport_proto::Open {
          credential: String::new(),
          service: DISPLAY_SERVICE.into(),
          method: "ListDisplays".into(),
          runner_class: TEST_RUNNER_CLASS.into(),
          device_id: None,
          run_id: None,
        })),
      }
      .encode_to_vec()
      .into(),
    ))
    .await
    .unwrap();
  let ready = websocket_server_message(socket.next().await.unwrap().unwrap());
  assert!(matches!(ready.message, Some(transport_proto::server_message::Message::Ready(_))));
  for message in [
    transport_proto::client_message::Message::Input(transport_proto::Input {
      payload: driver_proto::ListDisplaysRequest {}.encode_to_vec(),
    }),
    transport_proto::client_message::Message::HalfClose(transport_proto::HalfClose {}),
  ] {
    socket
      .send(tokio_tungstenite::tungstenite::Message::Binary(
        transport_proto::ClientMessage {
          message: Some(message),
        }
        .encode_to_vec()
        .into(),
      ))
      .await
      .unwrap();
  }
  let output = websocket_server_message(socket.next().await.unwrap().unwrap());
  let Some(transport_proto::server_message::Message::Output(output)) = output.message else {
    panic!("output message")
  };
  assert_eq!(driver_proto::ListDisplaysResponse::decode(output.payload.as_slice()).unwrap().displays[0].display_id, "display-fixture");
  let end = websocket_server_message(socket.next().await.unwrap().unwrap());
  let Some(transport_proto::server_message::Message::End(end)) = end.message else {
    panic!("end message")
  };
  assert_eq!(end.grpc_status, 0);

  shutdown.cancel();
  server_task.await.unwrap().unwrap();
  runner_task.abort();
}

fn websocket_server_message(message: tokio_tungstenite::tungstenite::Message) -> transport_proto::ServerMessage {
  transport_proto::ServerMessage::decode(message.into_data()).unwrap()
}

#[tokio::test]
async fn rest_pairing_bootstraps_and_authenticates_a_remote_device() {
  // ROOT CAUSE:
  //
  // Pairing REST requests required protobuf bytes because the HTTP layer was
  // maintained by hand instead of following the protobuf HTTP contract.
  //
  // Before the fix, JSON clients received 415 Unsupported Media Type.
  // The fix keeps the protobuf service as the source of the JSON route shape.
  let root = tempfile::tempdir().unwrap();
  let mut server_config = config(
    vec![
      ListenEndpoint::Tcp {
        host: "127.0.0.1".into(),
        port: 0,
      },
      ListenEndpoint::Remote {
        host: "127.0.0.1".into(),
        port: 0,
      },
    ],
    root.path(),
  );
  server_config.pairing_store = Some(root.path().join("pairings.json"));
  let server = Server::bind(server_config).await.unwrap();
  let local = server
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Tcp(value) => Some(*value),
      _ => None,
    })
    .unwrap();
  let remote = server
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Remote(value) => Some(*value),
      _ => None,
    })
    .unwrap();
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let http = reqwest::Client::new();

  let unauthenticated_rest = http.get(format!("http://{remote}/apis/auv/daemon/v1/devices")).send().await.unwrap();
  assert_eq!(unauthenticated_rest.status(), reqwest::StatusCode::UNAUTHORIZED);
  assert_eq!(
    unauthenticated_rest.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
    Some("application/problem+json")
  );
  let similar_to_public = http
    .post(format!("http://{remote}/apis/auv/daemon/v1/pairing/devices/extra"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body("{}")
    .send()
    .await
    .unwrap();
  assert_eq!(similar_to_public.status(), reqwest::StatusCode::UNAUTHORIZED);

  let unauthenticated_grpc = GrpcClient::connect(format!("http://{remote}").parse().unwrap()).await.unwrap();
  assert_eq!(unauthenticated_grpc.devices().list_devices().await.unwrap_err().code(), tonic::Code::Unauthenticated);

  let (mut unauthenticated_socket, _) = tokio_tungstenite::connect_async(format!("ws://{remote}/apis/auv/runtime/v1/invoke")).await.unwrap();
  unauthenticated_socket
    .send(tokio_tungstenite::tungstenite::Message::Binary(
      transport_proto::ClientMessage {
        message: Some(transport_proto::client_message::Message::Open(transport_proto::Open {
          credential: String::new(),
          service: DISPLAY_SERVICE.into(),
          method: "ListDisplays".into(),
          runner_class: TEST_RUNNER_CLASS.into(),
          device_id: None,
          run_id: None,
        })),
      }
      .encode_to_vec()
      .into(),
    ))
    .await
    .unwrap();
  let end = websocket_server_message(unauthenticated_socket.next().await.unwrap().unwrap());
  let Some(transport_proto::server_message::Message::End(end)) = end.message else {
    panic!("unauthenticated WebSocket must end")
  };
  assert_eq!(end.grpc_status, tonic::Code::Unauthenticated as i32);

  let token = http
    .post(format!("http://{local}/apis/auv/daemon/v1/pairing/tokens"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(r#"{"ttl":"60s"}"#)
    .send()
    .await
    .unwrap();
  assert_eq!(token.status(), reqwest::StatusCode::OK);
  let token: serde_json::Value = serde_json::from_slice(&token.bytes().await.unwrap()).unwrap();
  let token = token["token"].as_str().unwrap();

  let enrollment = http
    .post(format!("http://{remote}/apis/auv/daemon/v1/pairing/devices"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(
      serde_json::json!({
        "token": token,
        "deviceId": "browser-device",
        "label": "Browser",
      })
      .to_string(),
    )
    .send()
    .await
    .unwrap();
  assert_eq!(enrollment.status(), reqwest::StatusCode::OK);
  let enrollment: serde_json::Value = serde_json::from_slice(&enrollment.bytes().await.unwrap()).unwrap();
  let credential = enrollment["deviceCredential"].as_str().unwrap().to_string();

  let enabled = http
    .post(format!("http://{remote}/apis/auv/daemon/v1/pairing/devices/enabled"))
    .bearer_auth(&credential)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(r#"{"deviceSelector":"browser-device","enabled":true}"#)
    .send()
    .await
    .unwrap();
  assert_eq!(enabled.status(), reqwest::StatusCode::OK);
  let enabled: serde_json::Value = serde_json::from_slice(&enabled.bytes().await.unwrap()).unwrap();
  assert!(enabled.get("changed").is_none(), "ProtoJSON omits default-valued scalar fields");

  let devices = http.get(format!("http://{remote}/apis/auv/daemon/v1/devices")).bearer_auth(&credential).send().await.unwrap();
  assert_eq!(devices.status(), reqwest::StatusCode::OK);
  let devices: serde_json::Value = serde_json::from_slice(&devices.bytes().await.unwrap()).unwrap();
  assert_eq!(devices["devices"].as_array().unwrap().len(), 1);

  let created = http
    .post(format!("http://{remote}/apis/auv/runtime/v1/runs"))
    .bearer_auth(&credential)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body("{}")
    .send()
    .await
    .unwrap();
  assert_eq!(created.status(), reqwest::StatusCode::OK);
  let created: serde_json::Value = serde_json::from_slice(&created.bytes().await.unwrap()).unwrap();
  let run_id = created["run"]["ref"]["runId"].as_str().unwrap();
  let paired = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint: format!("http://{remote}").parse().unwrap(),
    device_credential: credential,
  })
  .await
  .unwrap();
  let runs = paired.runs().list_runs().await.unwrap();
  assert!(runs.iter().any(|run| run.r#ref.as_ref().is_some_and(|value| value.run_id == run_id)));

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_owner_and_paired_bearer_share_live_pairing_administration() {
  let root = tempfile::tempdir().unwrap();
  let mut server_config = config(
    vec![
      ListenEndpoint::Tcp {
        host: "127.0.0.1".into(),
        port: 0,
      },
      ListenEndpoint::Remote {
        host: "127.0.0.1".into(),
        port: 0,
      },
    ],
    root.path(),
  );
  server_config.pairing_store = Some(root.path().join("pairings.json"));
  let server = Server::bind(server_config).await.unwrap();
  let local = server
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Tcp(value) => Some(*value),
      _ => None,
    })
    .unwrap();
  let remote = server
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Remote(value) => Some(*value),
      _ => None,
    })
    .unwrap();
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));

  let local_client = GrpcClient::connect(format!("http://{local}").parse().unwrap()).await.unwrap();
  let token_a = local_client.pairing().create_pairing_token(proto::CreatePairingTokenRequest { ttl: None }).await.unwrap().token;
  let enrollment_a = auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
    format!("http://{remote}").parse().unwrap(),
    proto::PairDeviceRequest {
      token: token_a,
      device_id: "paired-a".into(),
      label: "Paired A".into(),
    },
  )
  .await
  .unwrap();
  let paired_a = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint: format!("http://{remote}").parse().unwrap(),
    device_credential: enrollment_a.device_credential,
  })
  .await
  .unwrap();
  let token_b = paired_a.pairing().create_pairing_token(proto::CreatePairingTokenRequest { ttl: None }).await.unwrap().token;
  let enrollment_b = auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
    format!("http://{remote}").parse().unwrap(),
    proto::PairDeviceRequest {
      token: token_b,
      device_id: "paired-b".into(),
      label: "Paired B".into(),
    },
  )
  .await
  .unwrap();
  let paired_b = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint: format!("http://{remote}").parse().unwrap(),
    device_credential: enrollment_b.device_credential,
  })
  .await
  .unwrap();
  paired_a.pairing().set_enabled("Paired B", false).await.unwrap();
  assert_eq!(paired_b.devices().list_devices().await.unwrap_err().code(), tonic::Code::Unauthenticated);
  paired_a.pairing().set_enabled("paired-b", true).await.unwrap();
  paired_b.devices().list_devices().await.unwrap();
  paired_b.pairing().revoke_device_credential("paired-a").await.unwrap();
  assert_eq!(paired_a.devices().list_devices().await.unwrap_err().code(), tonic::Code::Unauthenticated);
  shutdown.cancel();
  task.await.unwrap().unwrap();
}
