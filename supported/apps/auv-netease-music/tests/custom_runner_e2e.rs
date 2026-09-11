#![cfg(all(unix, target_os = "macos"))]

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_daemon::runner_provider::{ExecutableRunnerRuntime, RunnerProviderConfig, RunnerRuntime};
use auv_daemon::{Config, ListenEndpoint, Server};

#[tokio::test]
async fn daemon_supervises_and_aggregates_the_netease_runner() {
  let directory = tempfile::tempdir().expect("create isolated daemon state");
  let socket = directory.path().join("auv.sock");
  let child_context_path = directory.path().join("runner-context.json");
  let mut runner_environment = std::collections::BTreeMap::new();
  runner_environment.insert("AUV_RUNNER_TEST_CONTEXT".to_string(), child_context_path.display().to_string());
  runner_environment.insert("AUV_RUNNER_TEST_BINARY".to_string(), env!("CARGO_BIN_EXE_auv-runner-netease-music").to_string());
  let bound = Server::bind(Config {
    pairing_store: None,
    listeners: vec![ListenEndpoint::Unix {
      path: socket.clone(),
    }],
    discovery_file: None,
    publish_discovery: false,
    store_root: directory.path().join("store"),
    runner_providers: vec![RunnerProviderConfig {
      runner_class: "auv.app.netease_music".to_string(),
      runtime: RunnerRuntime::Executable(ExecutableRunnerRuntime {
        executable: std::path::PathBuf::from("/bin/sh"),
        arguments: vec![
          "-c".to_string(),
          "printf %s \"$AUV_CONTEXT\" > \"$AUV_RUNNER_TEST_CONTEXT\"; exec \"$AUV_RUNNER_TEST_BINARY\"".to_string(),
        ],
        working_directory: None,
        environment: runner_environment,
      }),
    }],
    daemon_idle_timeout: None,
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind daemon with trusted NetEase provider");
  let shutdown = tokio_util::sync::CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let server = tokio::spawn(async move { bound.serve(server_shutdown).await });

  let client = auv_api_client::protocol::grpc::Client::connect(auv_api_client::ConnectEndpoint::Unix(socket.clone()))
    .await
    .expect("connect to local daemon");
  let device = client.devices().list_devices().await.expect("list local Device").into_iter().next().expect("local Device");
  let device = device.r#ref.expect("Device ref");
  let run = client
    .runs()
    .create_run(daemon_proto::CreateRunRequest {
      devices: vec![device.clone()],
      labels: Default::default(),
    })
    .await
    .expect("create Run");
  let run = run.r#ref.expect("Run ref");
  let transport = client
    .routed_transport(auv_api_client::RunnerRoute {
      device_id: Some(device.device_id.clone()),
      run_id: Some(run.run_id.clone()),
      runner_class: "auv.app.netease_music".to_string(),
    })
    .expect("attach route metadata");
  let mut netease = auv_netease_music::api::v1::player_service_client::PlayerServiceClient::new(transport);
  let response = netease
    .get_now_playing(auv_netease_music::api::v1::GetNowPlayingRequest {
      application_bundle_id: Some("dev.auv.nonexistent-test-player".to_string()),
    })
    .await
    .expect("call app-owned RPC through daemon aggregation")
    .into_inner();
  assert!(!response.present, "a non-matching media owner must be filtered by the child Runner");
  let child_context: auv::AuvContext =
    serde_json::from_slice(&std::fs::read(&child_context_path).expect("Runner wrapper captured AUV_CONTEXT"))
      .expect("decode Runner context");
  assert_eq!(child_context.device_id.as_deref(), Some(device.device_id.as_str()));
  assert_eq!(child_context.daemon_endpoint.as_deref(), Some(format!("unix://{}", socket.display()).as_str()));

  // Exercise the application frontend as a real client process. This proves
  // the inherited AUV_CONTEXT -> high-level Run/Runner placement -> generated
  // custom client path, instead of covering only the lower-level transport.
  let context = auv::AuvContext {
    device_id: Some(device.device_id.clone()),
    run_id: Some(run.run_id.clone()),
    daemon_endpoint: Some(format!("unix://{}", socket.display())),
    ..Default::default()
  };
  let plugin = tokio::task::spawn_blocking(move || {
    std::process::Command::new(env!("CARGO_BIN_EXE_auv-netease-music"))
      .args([
        "now-playing",
        "--format",
        "json",
        "--app-id",
        "dev.auv.nonexistent-test-player",
      ])
      .env("AUV_CONTEXT", serde_json::to_string(&context).expect("encode inherited AUV context"))
      .output()
      .expect("execute NetEase client frontend")
  })
  .await
  .expect("join NetEase client process");
  assert!(plugin.status.success(), "plugin stderr: {}", String::from_utf8_lossy(&plugin.stderr));
  let plugin_response: serde_json::Value = serde_json::from_slice(&plugin.stdout).expect("plugin JSON response");
  assert_eq!(plugin_response["present"], false);

  client.runs().stop_run(run.run_id, daemon_proto::RunOutcome::Succeeded).await.expect("stop Run");
  shutdown.cancel();
  server.await.expect("join daemon task").expect("serve daemon");
}
