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
    id: None,
    listeners,
    store_root: root.join("store"),
    pairing_store: root.join("pairings.json"),
    discovery_file: None,
    register: false,
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
    #[cfg(windows)]
    enable_device_entry: false,
  }
}

/// Paired TCP listener on an ephemeral loopback port. HTTP never treats
/// loopback as the daemon owner, so HTTP route tests pair a Device first.
fn paired_loopback() -> ListenEndpoint {
  ListenEndpoint::Remote {
    host: "127.0.0.1".into(),
    port: 0,
  }
}

/// One paired loopback listener plus an owner channel for issuing tokens.
/// Unix daemons add their owner socket themselves; Windows adds its owner pipe
/// only when no listener is configured, so tests name one explicitly.
fn paired_http_listeners() -> Vec<ListenEndpoint> {
  #[allow(unused_mut)]
  let mut listeners = vec![paired_loopback()];
  #[cfg(windows)]
  listeners.push(ListenEndpoint::NamedPipe {
    name: format!("auv-test-{}", uuid::Uuid::now_v7()),
  });
  listeners
}

fn remote_address(server: &Server) -> std::net::SocketAddr {
  server
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Remote(address) => Some(*address),
      _ => None,
    })
    .expect("paired TCP listener")
}

fn owner_endpoint(server: &Server) -> auv_api_client::ConnectEndpoint {
  server.discovery_endpoint().expect("owner IPC listener").to_string().parse().unwrap()
}

/// Issues a token over the owner channel and enrolls one Device over paired
/// TCP, returning its bearer.
async fn pair_device(owner: auv_api_client::ConnectEndpoint, remote: std::net::SocketAddr, device_id: &str) -> String {
  let token = GrpcClient::connect(owner)
    .await
    .unwrap()
    .pairing()
    .create_pairing_token(proto::CreatePairingTokenRequest { ttl: None })
    .await
    .unwrap()
    .token;
  auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
    format!("http://{remote}").parse().unwrap(),
    proto::PairDeviceRequest {
      token,
      device_id: device_id.into(),
      label: device_id.into(),
    },
  )
  .await
  .unwrap()
  .device_credential
}

#[cfg(windows)]
#[tokio::test]
async fn ordinary_windows_server_does_not_open_system_device_entry() {
  // ROOT CAUSE:
  //
  // If every Windows Server::bind opens the privileged Device entry store,
  // ordinary `auv serve` fails before it can bind its existing API listener.
  // The SCM mode alone opts into LocalSystem-only Device entry state.
  let root = tempfile::tempdir().unwrap();
  let server = super::Server::bind(config(Vec::new(), root.path())).await.unwrap();

  assert!(server.device_local.is_none());
  assert!(!root.path().join("store/control/device-entry").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn disabled_device_entry_policy(root: &std::path::Path) {
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

  let control = root.join("store/control");
  let entry = control.join("device-entry");
  std::fs::create_dir_all(&entry).unwrap();
  std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o700)).unwrap();
  std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o700)).unwrap();
  let mut options = std::fs::OpenOptions::new();
  options.write(true).create_new(true).mode(0o600);
  std::io::Write::write_all(&mut options.open(entry.join("device-entry-policy.json")).unwrap(), br#"{"enabled":false,"enrollments":{}}"#)
    .unwrap();
}

fn device_entry_denied_reason() -> proto::DeviceEntryErrorReason {
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  return proto::DeviceEntryErrorReason::Disabled;
  #[cfg(not(any(target_os = "linux", target_os = "macos")))]
  return proto::DeviceEntryErrorReason::UnsupportedOsState;
}

const DISPLAY_SERVICE: &str = "auv.api.driver.v1.DisplayService";
const TEST_RUNNER_CLASS: &str = "example.runner.remote";

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn production_device_local_socket_serves_management_without_shared_routes() {
  use auv_api_proto::auv::api::daemon::v1::device_local_service_client::DeviceLocalServiceClient;
  use auv_api_proto::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
  use tonic::transport::Endpoint;

  let root = tempfile::tempdir().unwrap();
  let server = Server::bind(config(Vec::new(), root.path())).await.unwrap();
  let BoundEndpoint::Unix(owner_socket) = server.endpoint() else {
    panic!("owner Unix endpoint")
  };

  let owner_socket = owner_socket.clone();
  let socket = server.device_local.socket_path().to_path_buf();
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));

  for _ in 0..100 {
    if socket.exists() {
      break;
    }

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
  }

  if !socket.exists() {
    if task.is_finished() {
      panic!("daemon server stopped before DeviceLocalService socket bound: {:?}", task.await.unwrap());
    }

    panic!("dedicated DeviceLocalService socket did not bind at {}", socket.display());
  }

  use std::os::unix::fs::PermissionsExt;
  let control_mode = std::fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode() & 0o777;

  assert_eq!(control_mode, 0o700, "the socket parent is traversable by this daemon UID and root only");

  let unix_channel = |socket: std::path::PathBuf| async move {
    Endpoint::try_from("http://[::]:50051")
      .unwrap()
      .connect_with_connector(tower::service_fn(move |_: tonic::codegen::http::Uri| {
        let socket = socket.clone();
        async move { tokio::net::UnixStream::connect(socket).await.map(hyper_util::rt::TokioIo::new) }
      }))
      .await
  };
  let channel = unix_channel(socket.clone()).await.unwrap();

  let mut local = DeviceLocalServiceClient::new(channel.clone());

  assert!(local.get_policy(proto::GetPolicyRequest {}).await.unwrap().into_inner().enabled);
  assert!(local.list_enrollments(proto::ListEnrollmentsRequest {}).await.unwrap().into_inner().enrollments.is_empty());

  let missing_device_route = DeviceServiceClient::new(channel).list_user_sessions(proto::ListUserSessionsRequest {}).await.unwrap_err();

  assert_eq!(missing_device_route.code(), tonic::Code::Unimplemented);

  let mut shared = DeviceLocalServiceClient::new(unix_channel(owner_socket).await.unwrap());
  let missing_local_route = shared.get_policy(proto::GetPolicyRequest {}).await.unwrap_err();

  assert_eq!(missing_local_route.code(), tonic::Code::Unimplemented);

  shutdown.cancel();
  task.await.unwrap().unwrap();

  assert!(!socket.exists());
  assert!(!socket.parent().unwrap().exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn deep_store_root_still_binds_private_device_local_socket() {
  use auv_api_client::device_local::{DeviceLocalClient, verify_unix_socket_directory};

  // ROOT CAUSE:
  //
  // If the store root is deep, appending control/device-local.sock exceeds the
  // Unix sockaddr length and the daemon exits before serving any listener.
  // The dedicated socket now uses a short, store-specific private directory.
  let root = tempfile::tempdir().unwrap();
  let deep = root.path().join("nested".repeat(15)).join("project".repeat(15));
  std::fs::create_dir_all(&deep).unwrap();
  let server = Server::bind(config(Vec::new(), &deep)).await.unwrap();
  let socket = server.device_local.socket_path().to_path_buf();

  assert!(socket.as_os_str().len() < 104);

  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));

  for _ in 0..100 {
    if socket.exists() {
      break;
    }

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
  }

  if !socket.exists() {
    if task.is_finished() {
      panic!("daemon stopped before binding short Device-local socket: {:?}", task.await.unwrap());
    }

    panic!("daemon did not bind short Device-local socket at {}", socket.display());
  }

  verify_unix_socket_directory(&socket).unwrap();
  let mut client = DeviceLocalClient::connect_unix(&socket).await.unwrap();
  let policy = client.service().get_policy(proto::GetPolicyRequest {}).await.unwrap().into_inner();

  assert!(policy.enabled);

  shutdown.cancel();
  task.await.unwrap().unwrap();

  assert!(!socket.exists());
  assert!(!socket.parent().unwrap().exists());
}

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
  let server = Server::bind(config(paired_http_listeners(), root.path())).await.unwrap();
  let address = remote_address(&server);
  let owner = owner_endpoint(&server);
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let client = GrpcClient::connect(owner.clone()).await.unwrap();
  let devices = client.devices().list_devices().await.unwrap();
  assert_eq!(devices.len(), 1);
  assert!(devices[0].local);
  let credential = pair_device(owner, address, "rest-client").await;
  let http = authorized_http(&credential);

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
async fn device_entry_loopback_requires_authorization_before_selection() {
  // ROOT CAUSE:
  //
  // If a root daemon exposed its loopback TCP listener, another local account
  // could list sessions or request an unlock because loopback was treated as
  // the daemon owner's identity.
  //
  // Before the fix, these requests reached the Device entry policy. The fix
  // requires paired authentication or a verified local transport first.
  let root = tempfile::tempdir().unwrap();
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  disabled_device_entry_policy(root.path());
  let server = Server::bind(config(vec![paired_loopback()], root.path())).await.unwrap();
  let address = remote_address(&server);
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let mut devices = GrpcClient::connect(format!("http://{address}").parse().unwrap()).await.unwrap().devices();

  let missing = devices.ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest { target: None }).await.unwrap_err();

  assert_eq!(missing.code(), tonic::Code::Unauthenticated);

  let missing_lock = devices.ensure_user_session_locked(proto::EnsureUserSessionLockedRequest { target: None }).await.unwrap_err();

  assert_eq!(missing_lock.code(), tonic::Code::Unauthenticated);

  let blank = devices
    .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
      target: Some(proto::ensure_user_session_unlocked_request::Target::SessionSelector("  ".into())),
    })
    .await
    .unwrap_err();

  assert_eq!(blank.code(), tonic::Code::Unauthenticated);

  assert_eq!(devices.list_user_sessions().await.unwrap_err().code(), tonic::Code::Unauthenticated);

  assert_eq!(devices.get_user_session(" ").await.unwrap_err().code(), tonic::Code::Unauthenticated);
  assert_eq!(devices.get_user_session("seat0:42").await.unwrap_err().code(), tonic::Code::Unauthenticated);

  let error = devices
    .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
      target: Some(proto::ensure_user_session_unlocked_request::Target::User("neko".into())),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code(), tonic::Code::Unauthenticated);

  let lock_error = devices
    .ensure_user_session_locked(proto::EnsureUserSessionLockedRequest {
      target: Some(proto::ensure_user_session_locked_request::Target::User("neko".into())),
    })
    .await
    .unwrap_err();

  assert_eq!(lock_error.code(), tonic::Code::Unauthenticated);

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

// ROOT CAUSE:
//
// If only an http:// listener was configured with a pairing store, that
// listener required a bearer and no owner channel was guaranteed, so the first
// pairing token could not be issued without a hand-configured Unix listener.
//
// Before the fix, first pairing worked only through a hidden Runner parent
// socket. The fix always binds an owner socket and registers it for discovery.
#[cfg(unix)]
#[tokio::test]
async fn registered_daemon_with_only_a_paired_listener_publishes_its_owner_socket() {
  let root = tempfile::tempdir().unwrap();
  let descriptor = root.path().join("daemon.json");
  let mut options = config(vec![paired_loopback()], root.path());
  options.discovery_file = Some(descriptor.clone());
  options.register = true;
  let server = Server::bind(options).await.unwrap();
  let remote = remote_address(&server);
  let owner_socket = root.path().join("auv.sock");

  assert_eq!(server.discovery_endpoint(), Some(&BoundEndpoint::Unix(owner_socket.clone())));

  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  for _ in 0..100 {
    if descriptor.exists() {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
  }
  let published = auv::discovery::read_descriptor(&descriptor).unwrap().expect("registered daemon descriptor");

  assert_eq!(published.endpoint(), format!("unix://{}", owner_socket.display()));

  let anonymous = GrpcClient::connect(format!("http://{remote}").parse().unwrap()).await.unwrap();

  assert_eq!(
    anonymous.pairing().create_pairing_token(proto::CreatePairingTokenRequest { ttl: None }).await.unwrap_err().code(),
    tonic::Code::Unauthenticated
  );

  let credential = pair_device(auv_api_client::ConnectEndpoint::Unix(owner_socket), remote, "first-device").await;
  let paired = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint: format!("http://{remote}").parse().unwrap(),
    device_credential: credential,
  })
  .await
  .unwrap();

  assert_eq!(paired.devices().list_devices().await.unwrap().len(), 1);

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn unregistered_daemon_uses_a_private_owner_socket_and_publishes_nothing() {
  let root = tempfile::tempdir().unwrap();
  let descriptor = root.path().join("daemon.json");
  let mut options = config(vec![paired_loopback()], root.path());
  options.discovery_file = Some(descriptor.clone());
  let server = Server::bind(options).await.unwrap();
  let Some(BoundEndpoint::Unix(owner_socket)) = server.discovery_endpoint().cloned() else {
    panic!("owner Unix endpoint")
  };

  assert_ne!(owner_socket, root.path().join("auv.sock"), "the default socket stays free for the registered daemon");

  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let token = GrpcClient::connect(auv_api_client::ConnectEndpoint::Unix(owner_socket.clone()))
    .await
    .unwrap()
    .pairing()
    .create_pairing_token(proto::CreatePairingTokenRequest { ttl: None })
    .await
    .unwrap();

  assert!(!token.token.is_empty());
  assert!(!descriptor.exists());

  shutdown.cancel();
  task.await.unwrap().unwrap();

  assert!(!owner_socket.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn owner_verified_unix_can_reach_device_entry_policy() {
  let root = tempfile::tempdir().unwrap();
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  disabled_device_entry_policy(root.path());
  let socket = root.path().join("api.sock");
  let server = Server::bind(config(
    vec![ListenEndpoint::Unix {
      path: socket.clone(),
    }],
    root.path(),
  ))
  .await
  .unwrap();
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));
  let mut devices = GrpcClient::connect(auv_api_client::ConnectEndpoint::Unix(socket)).await.unwrap().devices();

  let sessions = devices.list_user_sessions().await.unwrap();
  let Some(proto::list_user_sessions_response::Result::Error(error)) = sessions.result else {
    panic!("verified Unix caller must reach the Device entry policy")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  let blank_get = devices.get_user_session(" ").await.unwrap_err();

  assert_eq!(blank_get.code(), tonic::Code::InvalidArgument);

  let session = devices.get_user_session("seat0:42").await.unwrap();
  let Some(proto::get_user_session_response::Result::Error(error)) = session.result else {
    panic!("verified Unix caller must reach the Device entry policy")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  let missing = devices.ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest { target: None }).await.unwrap_err();

  assert_eq!(missing.code(), tonic::Code::InvalidArgument);

  let missing_lock = devices.ensure_user_session_locked(proto::EnsureUserSessionLockedRequest { target: None }).await.unwrap_err();

  assert_eq!(missing_lock.code(), tonic::Code::InvalidArgument);

  let outcome = devices
    .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
      target: Some(proto::ensure_user_session_unlocked_request::Target::User("neko".into())),
    })
    .await
    .unwrap();

  let Some(proto::ensure_user_session_unlocked_response::Result::Error(error)) = outcome.result else {
    panic!("verified Unix caller must reach the Device entry policy")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  let lock_outcome = devices
    .ensure_user_session_locked(proto::EnsureUserSessionLockedRequest {
      target: Some(proto::ensure_user_session_locked_request::Target::User("neko".into())),
    })
    .await
    .unwrap();

  let Some(proto::ensure_user_session_locked_response::Result::Error(lock_error)) = lock_outcome.result else {
    panic!("verified Unix caller must reach the Device lock policy")
  };

  assert_eq!(lock_error.reason, device_entry_denied_reason() as i32);

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[tokio::test]
async fn http_and_websocket_invoke_share_the_runner_route() {
  let root = tempfile::tempdir().unwrap();
  let (provider, runner_task) = remote_display_runner().await;
  let mut daemon_config = config(paired_http_listeners(), root.path());
  daemon_config.runner_providers.push(provider);
  let server = Server::bind(daemon_config).await.unwrap();
  let address = remote_address(&server);
  let owner = owner_endpoint(&server);
  let shutdown = CancellationToken::new();
  let server_task = tokio::spawn(server.serve(shutdown.clone()));
  let credential = pair_device(owner, address, "invoke-client").await;

  let response = authorized_http(&credential)
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
          credential: credential.clone(),
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

/// HTTP client that sends one paired Device bearer on every request.
fn authorized_http(credential: &str) -> reqwest::Client {
  let mut headers = reqwest::header::HeaderMap::new();
  headers.insert(reqwest::header::AUTHORIZATION, format!("Bearer {credential}").parse().unwrap());
  reqwest::Client::builder().default_headers(headers).build().unwrap()
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
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  disabled_device_entry_policy(root.path());
  let server = Server::bind(config(paired_http_listeners(), root.path())).await.unwrap();
  let owner = owner_endpoint(&server);
  let remote = remote_address(&server);
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
  assert_eq!(unauthenticated_grpc.devices().list_user_sessions().await.unwrap_err().code(), tonic::Code::Unauthenticated);
  assert_eq!(unauthenticated_grpc.devices().get_user_session("seat0:42").await.unwrap_err().code(), tonic::Code::Unauthenticated);
  assert_eq!(
    unauthenticated_grpc
      .devices()
      .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
        target: Some(proto::ensure_user_session_unlocked_request::Target::User("neko".into())),
      })
      .await
      .unwrap_err()
      .code(),
    tonic::Code::Unauthenticated
  );

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

  let anonymous_token = http
    .post(format!("http://{remote}/apis/auv/daemon/v1/pairing/tokens"))
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(r#"{"ttl":"60s"}"#)
    .send()
    .await
    .unwrap();
  assert_eq!(anonymous_token.status(), reqwest::StatusCode::UNAUTHORIZED);

  let token = GrpcClient::connect(owner)
    .await
    .unwrap()
    .pairing()
    .create_pairing_token(proto::CreatePairingTokenRequest { ttl: None })
    .await
    .unwrap()
    .token;
  let token = token.as_str();

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

  let paired_token = http
    .post(format!("http://{remote}/apis/auv/daemon/v1/pairing/tokens"))
    .bearer_auth(&credential)
    .header(reqwest::header::CONTENT_TYPE, "application/json")
    .body(r#"{"ttl":"60s"}"#)
    .send()
    .await
    .unwrap();
  assert_eq!(paired_token.status(), reqwest::StatusCode::FORBIDDEN);

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
    device_credential: credential.clone(),
  })
  .await
  .unwrap();
  let mut local_on_paired = proto::device_local_service_client::DeviceLocalServiceClient::connect(format!("http://{remote}")).await.unwrap();
  let mut local_request = tonic::Request::new(proto::GetPolicyRequest {});
  local_request.metadata_mut().insert("authorization", format!("Bearer {credential}").parse().unwrap());

  assert_eq!(local_on_paired.get_policy(local_request).await.unwrap_err().code(), tonic::Code::Unimplemented);

  let runs = paired.runs().list_runs().await.unwrap();
  assert!(runs.iter().any(|run| run.r#ref.as_ref().is_some_and(|value| value.run_id == run_id)));

  let sessions = paired.devices().list_user_sessions().await.unwrap();
  let Some(proto::list_user_sessions_response::Result::Error(error)) = sessions.result else {
    panic!("paired request must reach the native support gate")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  let session = paired.devices().get_user_session("seat0:42").await.unwrap();
  let Some(proto::get_user_session_response::Result::Error(error)) = session.result else {
    panic!("paired request must reach the native support gate")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  let outcome = paired
    .devices()
    .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
      target: Some(proto::ensure_user_session_unlocked_request::Target::User("neko".into())),
    })
    .await
    .unwrap();

  let Some(proto::ensure_user_session_unlocked_response::Result::Error(error)) = outcome.result else {
    panic!("paired request must reach the native support gate")
  };

  assert_eq!(error.reason, device_entry_denied_reason() as i32);

  #[cfg(target_os = "linux")]
  {
    // The paired route must reach the same policy and durable audit as the
    // target-local service, without observing or unlocking a real session.
    let audit = std::fs::read_to_string(root.path().join("store/control/device-entry/device-entry-audit.jsonl")).unwrap();
    let records = audit.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()).collect::<Vec<_>>();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["event"], "attempt");
    assert_eq!(records[1]["event"], "outcome");
    assert_eq!(records[0]["attempt_id"], records[1]["attempt_id"]);
    assert_eq!(records[0]["caller"], "paired-device:browser-device");
    assert_eq!(records[1]["caller"], "paired-device:browser-device");
    assert_eq!(records[1]["result"], "DISABLED");
    assert!(audit.find("credential").is_none());
  }

  shutdown.cancel();
  task.await.unwrap().unwrap();
}

#[tokio::test]
async fn paired_devices_administer_only_themselves_and_cannot_issue_tokens() {
  let root = tempfile::tempdir().unwrap();
  let server = Server::bind(config(paired_http_listeners(), root.path())).await.unwrap();
  let owner = owner_endpoint(&server);
  let remote = remote_address(&server);
  let shutdown = CancellationToken::new();
  let task = tokio::spawn(server.serve(shutdown.clone()));

  let local_client = GrpcClient::connect(owner).await.unwrap();
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
  // ROOT CAUSE:
  //
  // If a paired bearer could issue tokens, one leaked credential could enroll
  // replacement Devices that survive its own revocation.
  //
  // Before the fix, any paired Device could create pairing tokens.
  // The fix keeps token issuance on the owner-checked local channel only.
  let denied = paired_a.pairing().create_pairing_token(proto::CreatePairingTokenRequest { ttl: None }).await.unwrap_err();
  assert_eq!(denied.code(), tonic::Code::PermissionDenied);
  let token_b = local_client.pairing().create_pairing_token(proto::CreatePairingTokenRequest { ttl: None }).await.unwrap().token;
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

  // ROOT CAUSE:
  //
  // If any paired bearer could administer every Device, one leaked credential
  // could disable, revoke, or unpair the owner's other Devices.
  //
  // Before the fix, Device A could disable B and B could revoke A.
  // The fix limits a paired Device to itself and leaves the rest to the owner.
  for denied in [
    paired_a.pairing().set_enabled("Paired B", false).await.unwrap_err(),
    paired_a.pairing().set_enabled("paired-b", false).await.unwrap_err(),
    paired_b.pairing().revoke_device_credential("paired-a").await.unwrap_err(),
    paired_b.pairing().unpair("paired-a").await.unwrap_err(),
  ] {
    assert_eq!(denied.code(), tonic::Code::PermissionDenied);
  }
  paired_b.devices().list_devices().await.unwrap();

  local_client.pairing().set_enabled("Paired B", false).await.unwrap();
  assert_eq!(paired_b.devices().list_devices().await.unwrap_err().code(), tonic::Code::Unauthenticated);
  local_client.pairing().set_enabled("paired-b", true).await.unwrap();
  paired_b.devices().list_devices().await.unwrap();

  paired_a.pairing().revoke_device_credential("paired-a").await.unwrap();
  assert_eq!(paired_a.devices().list_devices().await.unwrap_err().code(), tonic::Code::Unauthenticated);
  shutdown.cancel();
  task.await.unwrap().unwrap();
}

// ROOT CAUSE:
// A healthy pre-existing listener could satisfy a new launch's readiness probe.
// Each bound daemon now has its own identity, independent of process-global
// state, and every HTTP/gRPC listener reports that same identity.
#[tokio::test]
async fn health_identifies_each_daemon_instance_across_protocols_and_listeners() {
  let explicit = uuid::Uuid::now_v7();
  let mut ids = Vec::new();
  for supplied in [None, None, Some(explicit)] {
    let root = tempfile::tempdir().unwrap();
    let mut options = config(vec![paired_loopback(), paired_loopback()], root.path());
    options.id = supplied;
    let server = Server::bind(options).await.unwrap();
    let endpoints = server.endpoints().to_vec();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(server.serve(shutdown.clone()));
    let mut id = None;
    for endpoint in endpoints {
      // Health is public, so every paired TCP listener answers without a bearer.
      let BoundEndpoint::Remote(address) = endpoint else {
        continue;
      };
      let http: serde_json::Value =
        serde_json::from_slice(&reqwest::get(format!("http://{address}/health")).await.unwrap().bytes().await.unwrap()).unwrap();
      let mut grpc = proto::health_service_client::HealthServiceClient::connect(format!("http://{address}")).await.unwrap();
      let response = grpc.check(proto::CheckRequest {}).await.unwrap().into_inner();
      assert_eq!(response.status, proto::HealthStatus::Serving as i32);
      assert_eq!(http["id"], response.id);
      assert_eq!(http["status"], response.status);
      let actual = uuid::Uuid::parse_str(&response.id).unwrap();
      if let Some(expected) = supplied.or(id) {
        assert_eq!(actual, expected);
      }
      id = Some(actual);
    }
    ids.push(id.unwrap());
    shutdown.cancel();
    task.await.unwrap().unwrap();
  }
  assert_ne!(ids[0], ids[1]);
  assert_eq!(ids[2], explicit);
}
