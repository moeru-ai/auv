//! Server lifecycle and end-to-end transport tests.

use auv_api_client::ConnectEndpoint;
use auv_api_client::protocol::grpc::Client as GrpcClient;
use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use prost::Message as _;

use crate::test_fixtures::api_temp_store_root;

use super::*;

fn test_http_client_builder() -> reqwest::ClientBuilder {
  let _ = rustls::crypto::ring::default_provider().install_default();
  reqwest::Client::builder()
}

#[test]
fn loopback_policy_rejects_remote_tcp() {
  assert!(assert_loopback_host("127.0.0.1").is_ok());
  assert!(assert_loopback_host("::1").is_ok());
  assert!(assert_loopback_host("0.0.0.0").is_err());
  assert!(assert_loopback_host("192.168.1.1").is_err());
}

#[tokio::test]
async fn remote_only_daemon_adds_a_local_executable_runner_parent_listener() {
  let root = api_temp_store_root("runner-parent-listener");
  let bound = Server::bind(ServerConfig {
    listen: ListenEndpoint::Remote {
      host: "127.0.0.1".to_string(),
      port: 0,
    },
    pairing_store: Some(root.join("pairings.json")),
    store_root: root,
    runner_providers: vec![crate::runner_provider::RunnerProviderConfig {
      runner_class: "example.runner".to_string(),
      runtime: crate::runner_provider::RunnerRuntime::Executable(crate::runner_provider::ExecutableRunnerRuntime {
        executable: "example-runner".into(),
        arguments: Vec::new(),
        working_directory: None,
        environment: Default::default(),
      }),
    }],
    ..Default::default()
  })
  .await
  .expect("bind paired listener plus internal Runner parent listener");
  #[cfg(unix)]
  assert!(bound.endpoints().iter().any(|endpoint| matches!(endpoint, BoundEndpoint::Unix(_))));
  #[cfg(not(unix))]
  assert!(bound.endpoints().iter().any(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_))));
}

#[cfg(unix)]
#[tokio::test]
async fn multi_listener_bind_is_atomic_and_cleans_up_bound_unix_sockets() {
  let directory = tempfile::tempdir().expect("temporary multi-listener directory");
  let socket_path = directory.path().join("auv.sock");
  let result = Server::bind(ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Unix {
      path: socket_path.clone(),
    },
    additional_listeners: vec![ListenEndpoint::Unix {
      path: socket_path.clone(),
    }],
    store_root: directory.path().join("runs"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await;
  let error = match result {
    Ok(_) => panic!("all listeners must bind before readiness"),
    Err(error) => error,
  };

  assert!(error.contains("already exists"));
  assert!(!socket_path.exists(), "failed multi-listener bind removes sockets bound earlier in the same attempt");
}

#[tokio::test]
async fn tcp_client_reaches_typed_control_services() {
  let config = ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("tcp"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  };
  let bound = Server::bind(config).await.expect("bind TCP server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let endpoint = format!("http://{address}").parse().expect("valid loopback endpoint");
  let client = GrpcClient::connect(endpoint).await.expect("connect TCP client");
  let devices = client.devices().list_devices().await.expect("list Devices through gRPC");
  assert_eq!(devices.len(), 1);
  assert!(devices[0].local);
  shutdown.cancel();
  server.await.expect("join server").expect("serve TCP");
}

#[tokio::test]
async fn rest_discovery_lists_the_auv_api_namespace() {
  let config = ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("rest-discovery"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  };
  let bound = Server::bind(config).await.expect("bind TCP server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = test_http_client_builder().http2_prior_knowledge().build().expect("build REST test client");

  let response = client.get(format!("http://{address}/apis")).send().await.expect("request API discovery");
  assert_eq!(response.status(), reqwest::StatusCode::OK);
  assert_eq!(response.headers().get("content-type").and_then(|value| value.to_str().ok()), Some("application/protobuf"));
  let discovery =
    daemon_proto::ListApiNamespacesResponse::decode(response.bytes().await.expect("read API discovery")).expect("decode API discovery");
  assert_eq!(
    discovery.namespaces,
    vec![daemon_proto::ApiNamespace {
      name: "auv".to_string(),
    }]
  );

  shutdown.cancel();
  server.await.expect("join server").expect("serve TCP");
}

#[tokio::test]
async fn rest_discovery_lists_auv_groups_versions_and_resources() {
  let config = ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("rest-resource-discovery"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  };
  let bound = Server::bind(config).await.expect("bind TCP server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = test_http_client_builder().http2_prior_knowledge().build().expect("build REST test client");

  let namespace = client.get(format!("http://{address}/apis/auv")).send().await.expect("request AUV namespace discovery");
  assert_eq!(namespace.status(), reqwest::StatusCode::OK);
  let namespace = daemon_proto::GetApiNamespaceResponse::decode(namespace.bytes().await.expect("read namespace discovery"))
    .expect("decode namespace discovery");
  assert_eq!(namespace.namespace, "auv");
  assert_eq!(
    namespace.groups,
    vec![
      daemon_proto::ApiGroup {
        name: "daemon".to_string(),
        versions: vec!["v1".to_string()],
      },
      daemon_proto::ApiGroup {
        name: "runtime".to_string(),
        versions: vec!["v1".to_string()],
      },
    ]
  );

  let daemon = client.get(format!("http://{address}/apis/auv/daemon/v1")).send().await.expect("request daemon discovery");
  assert_eq!(daemon.status(), reqwest::StatusCode::OK);
  let daemon =
    daemon_proto::GetApiGroupVersionResponse::decode(daemon.bytes().await.expect("read daemon discovery")).expect("decode daemon discovery");
  assert_eq!(daemon.group, "daemon");
  assert_eq!(daemon.version, "v1");
  assert_eq!(daemon.resources.len(), 1);
  assert_eq!(daemon.resources[0].name, "devices");
  assert_eq!(
    daemon.resources[0].operations,
    vec![
      daemon_proto::ApiResourceOperation::List as i32,
      daemon_proto::ApiResourceOperation::Get as i32
    ]
  );
  let runtime = client.get(format!("http://{address}/apis/auv/runtime/v1")).send().await.expect("request runtime discovery");
  assert_eq!(runtime.status(), reqwest::StatusCode::OK);
  let runtime = daemon_proto::GetApiGroupVersionResponse::decode(runtime.bytes().await.expect("read runtime discovery"))
    .expect("decode runtime discovery");
  assert_eq!(runtime.resources.iter().map(|resource| resource.name.as_str()).collect::<Vec<_>>(), vec!["runners", "runnerclasses", "runs"]);
  assert_eq!(
    runtime.resources[0].operations,
    vec![
      daemon_proto::ApiResourceOperation::List as i32,
      daemon_proto::ApiResourceOperation::Get as i32,
    ],
    "Runner mutations are not discoverable until a provider exists"
  );
  assert_eq!(
    runtime.resources[1].operations,
    vec![
      daemon_proto::ApiResourceOperation::List as i32,
      daemon_proto::ApiResourceOperation::Get as i32,
    ]
  );
  assert_eq!(
    runtime.resources[2].operations,
    vec![
      daemon_proto::ApiResourceOperation::List as i32,
      daemon_proto::ApiResourceOperation::Get as i32,
      daemon_proto::ApiResourceOperation::Create as i32,
      daemon_proto::ApiResourceOperation::Delete as i32,
    ],
    "Run lifecycle operations are advertised without scope-based hiding"
  );

  shutdown.cancel();
  server.await.expect("join server").expect("serve TCP");
}

#[tokio::test]
async fn rest_devices_list_and_get_the_persistent_local_device() {
  let store_root = api_temp_store_root("rest-devices");
  let (first_id, first_server) = {
    let bound = Server::bind(ServerConfig {
      pairing_store: None,
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: store_root.clone(),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    })
    .await
    .expect("bind first server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    let client = test_http_client_builder().http2_prior_knowledge().build().expect("build REST test client");
    let response = client.get(format!("http://{address}/apis/auv/daemon/v1/devices")).send().await.expect("list devices");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response = daemon_proto::ListDevicesResponse::decode(response.bytes().await.expect("read devices")).expect("decode devices");
    assert_eq!(response.devices.len(), 1);
    let device = response.devices.into_iter().next().expect("local device");
    assert!(device.local);
    let device_id = device.r#ref.as_ref().expect("device ref").device_id.clone();
    assert_eq!(device_id.len(), 64);
    assert!(device_id.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let get = client.get(format!("http://{address}/apis/auv/daemon/v1/devices/{device_id}")).send().await.expect("get device");
    assert_eq!(get.status(), reqwest::StatusCode::OK);
    let get = daemon_proto::GetDeviceResponse::decode(get.bytes().await.expect("read device")).expect("decode device");
    assert_eq!(get.device, Some(device));
    shutdown.cancel();
    (device_id, server)
  };
  first_server.await.expect("join first server").expect("serve first server");

  let bound = Server::bind(ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root,
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind restarted server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = test_http_client_builder().http2_prior_knowledge().build().expect("build restarted REST test client");
  let response =
    client.get(format!("http://{address}/apis/auv/daemon/v1/devices/{first_id}")).send().await.expect("get persistent local device");
  assert_eq!(response.status(), reqwest::StatusCode::OK);

  shutdown.cancel();
  server.await.expect("join restarted server").expect("serve restarted server");
}

#[tokio::test]
async fn rest_runs_create_list_and_get_under_the_local_device() {
  let bound = Server::bind(ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("rest-runs"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = test_http_client_builder().http2_prior_knowledge().build().expect("build REST test client");

  let create = client
    .post(format!("http://{address}/apis/auv/runtime/v1/runs"))
    .header("content-type", "application/protobuf")
    .body(
      daemon_proto::CreateRunRequest {
        labels: std::collections::HashMap::from([("purpose".to_string(), "test".to_string())]),
        devices: Vec::new(),
      }
      .encode_to_vec(),
    )
    .send()
    .await
    .expect("create Run");
  assert_eq!(create.status(), reqwest::StatusCode::OK);
  let created = daemon_proto::CreateRunResponse::decode(create.bytes().await.expect("read created Run")).expect("decode created Run");
  let run = created.run.expect("created Run");
  let run_id = run.r#ref.as_ref().expect("Run ref").run_id.clone();
  assert_eq!(run_id.len(), 32);
  assert!(run_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
  assert_eq!(run.phase, daemon_proto::RunPhase::Running as i32);
  assert_eq!(run.devices.len(), 1);
  assert_eq!(run.labels.get("purpose").map(String::as_str), Some("test"));

  let list = client.get(format!("http://{address}/apis/auv/runtime/v1/runs")).send().await.expect("list Runs");
  assert_eq!(list.status(), reqwest::StatusCode::OK);
  let list = daemon_proto::ListRunsResponse::decode(list.bytes().await.expect("read Runs")).expect("decode Runs");
  assert_eq!(list.runs, vec![run.clone()]);

  let get = client.get(format!("http://{address}/apis/auv/runtime/v1/runs/{run_id}")).send().await.expect("get Run");
  assert_eq!(get.status(), reqwest::StatusCode::OK);
  let get = daemon_proto::GetRunResponse::decode(get.bytes().await.expect("read Run")).expect("decode Run");
  assert_eq!(get.run, Some(run));

  shutdown.cancel();
  server.await.expect("join server").expect("serve server");
}

#[tokio::test]
async fn rest_runners_are_empty_and_creation_fails_without_a_provider() {
  let bound = Server::bind(ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("rest-runners"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind server");
  let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
    panic!("TCP endpoint");
  };
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = test_http_client_builder().http2_prior_knowledge().build().expect("build REST test client");

  let list = client.get(format!("http://{address}/apis/auv/runtime/v1/runners")).send().await.expect("list Runners");
  assert_eq!(list.status(), reqwest::StatusCode::OK);
  let list = daemon_proto::ListRunnersResponse::decode(list.bytes().await.expect("read Runners")).expect("decode Runners");
  assert!(list.runners.is_empty());

  let create = client
    .post(format!("http://{address}/apis/auv/runtime/v1/runners"))
    .header("content-type", "application/protobuf")
    .body(
      daemon_proto::CreateRunnerRequest {
        device: None,
        runner_class: Some(daemon_proto::RunnerClassRef {
          runner_class: "auv.core.local".to_string(),
        }),
        labels: std::collections::HashMap::new(),
        lifecycle: daemon_proto::RunnerLifecycle::UnlessShutdown as i32,
        idle_timeout: None,
      }
      .encode_to_vec(),
    )
    .send()
    .await
    .expect("create Runner without provider");
  assert_eq!(create.status(), reqwest::StatusCode::NOT_IMPLEMENTED);
  assert_eq!(create.headers().get("content-type").and_then(|value| value.to_str().ok()), Some("application/problem+json"));

  shutdown.cancel();
  server.await.expect("join server").expect("serve server");
}

#[cfg(unix)]
#[tokio::test]
async fn short_token_pairing_issues_live_revocable_bearer() {
  let directory = tempfile::tempdir().expect("pairing test directory");
  let socket_path = directory.path().join("auv.sock");
  let pairing_path = directory.path().join("pairings.json");
  let bound = Server::bind(ServerConfig {
    pairing_store: Some(pairing_path),
    listen: ListenEndpoint::Unix {
      path: socket_path.clone(),
    },
    additional_listeners: vec![ListenEndpoint::Remote {
      host: "127.0.0.1".to_string(),
      port: 0,
    }],
    store_root: api_temp_store_root("short-token-pairing"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind local administration and remote bearer listeners");
  let remote = bound
    .endpoints()
    .iter()
    .find_map(|endpoint| match endpoint {
      BoundEndpoint::Remote(address) => Some(*address),
      _ => None,
    })
    .expect("remote endpoint");
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));

  let local = GrpcClient::connect(ConnectEndpoint::Unix(socket_path)).await.expect("connect owner Unix client");
  let token =
    local.pairing().create_pairing_token(daemon_proto::CreatePairingTokenRequest { ttl: None }).await.expect("create one-time token").token;
  let endpoint = format!("http://{remote}").parse::<http::Uri>().expect("remote URI");
  let enrollment = auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
    endpoint.clone(),
    daemon_proto::PairDeviceRequest {
      token: token.clone(),
      device_id: "device_remote_test".to_string(),
      label: "Remote test".to_string(),
    },
  )
  .await
  .expect("consume token");
  let reused = auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
    endpoint.clone(),
    daemon_proto::PairDeviceRequest {
      token,
      device_id: "device_reuse".to_string(),
      label: String::new(),
    },
  )
  .await
  .expect_err("pairing token is one-time");
  assert_eq!(reused.code(), tonic::Code::Unauthenticated);

  let remote_client = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint,
    device_credential: enrollment.device_credential,
  })
  .await
  .expect("connect paired Device");
  assert_eq!(remote_client.devices().list_devices().await.expect("inspect through bearer").len(), 1);
  assert!(local.pairing().revoke_device_credential("device_remote_test").await.expect("revoke bearer"));
  let revoked = remote_client.devices().list_devices().await.expect_err("revocation is checked on every request");
  assert_eq!(revoked.code(), tonic::Code::Unauthenticated);

  shutdown.cancel();
  server.await.expect("join pairing server").expect("serve pairing test");
}

#[tokio::test]
async fn remote_bearer_listener_is_not_published_as_local_discovery() {
  let directory = tempfile::tempdir().expect("pairing test directory");
  let bound = Server::bind(ServerConfig {
    pairing_store: Some(directory.path().join("pairings.json")),
    listen: ListenEndpoint::Remote {
      host: "127.0.0.1".to_string(),
      port: 0,
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("remote-not-local-discovery"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind remote bearer listener");

  assert!(bound.discovery_endpoint().is_none());
}

#[tokio::test]
async fn unix_client_uses_typed_services_and_cleans_up_socket() {
  let directory = tempfile::tempdir().expect("temporary Unix socket directory");
  let socket_path = directory.path().join("auv.sock");
  let config = ServerConfig {
    pairing_store: None,
    listen: ListenEndpoint::Unix {
      path: socket_path.clone(),
    },
    additional_listeners: Vec::new(),
    store_root: api_temp_store_root("unix"),
    daemon_idle_timeout: None,
    runner_providers: Vec::new(),
    first_party_runners: Default::default(),
  };
  let bound = Server::bind(config).await.expect("bind Unix server");
  assert!(socket_path.exists());
  use std::os::unix::fs::PermissionsExt;
  assert_eq!(std::fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777, 0o600);
  let shutdown = CancellationToken::new();
  let server = tokio::spawn(bound.serve(shutdown.clone()));
  let client = GrpcClient::connect(ConnectEndpoint::Unix(socket_path.clone())).await.expect("connect Unix client");
  let implicit = client.runs().create_run(daemon_proto::CreateRunRequest::default()).await.expect("create implicit local Run");
  let implicit_id = implicit.r#ref.as_ref().expect("Run ref").run_id.clone();
  let implicit_device_id = implicit.devices.first().expect("implicit local Device ref").device_id.clone();
  assert!(client.devices().get_device(implicit_device_id).await.expect("implicit local Device").local);
  let completed = client.runs().stop_run(implicit_id.clone(), daemon_proto::RunOutcome::Succeeded).await.expect("finish implicit local Run");
  assert_eq!(completed.phase, daemon_proto::RunPhase::Succeeded as i32);
  assert_eq!(
    client.runs().get_run(implicit_id).await.expect("completed Run remains queryable").phase,
    daemon_proto::RunPhase::Succeeded as i32
  );

  shutdown.cancel();
  server.await.expect("join server").expect("serve Unix");
  assert!(!socket_path.exists());
}
