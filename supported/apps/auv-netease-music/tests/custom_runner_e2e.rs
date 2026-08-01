#![cfg(all(unix, target_os = "macos"))]

use auv_api_proto::auv::api::core::v1 as core_proto;
use auv_api_server::runner_provider::{
  ExecutableRunnerRuntime, RunnerProviderConfig, RunnerProviderLifecycle, RunnerProviderServiceConfig, RunnerRuntime,
};
use auv_api_server::transport::{ApiServeConfig, ListenEndpoint};

const SERVICE: &str = "auv.netease_music.v1.NeteaseMusicService";

#[tokio::test]
async fn daemon_supervises_and_aggregates_the_netease_runner() {
  let directory = tempfile::tempdir().expect("create isolated daemon state");
  let descriptor_set = directory.path().join("netease.descriptor.bin");
  std::fs::write(&descriptor_set, auv_netease_music::api::FILE_DESCRIPTOR_SET).expect("write app-owned descriptor set");
  let services = vec![RunnerProviderServiceConfig {
    name: SERVICE.to_string(),
    externally_exposed: true,
  }];
  let descriptor_set_sha256 =
    RunnerProviderConfig::canonical_descriptor_sha256(&descriptor_set, &services).expect("pin canonical app schema");
  let socket = directory.path().join("auv.sock");
  let bound = auv_api_server::transport::bind(ApiServeConfig {
    listen: ListenEndpoint::Unix {
      path: socket.clone(),
    },
    additional_listeners: Vec::new(),
    store_root: directory.path().join("store"),
    runner_providers: vec![RunnerProviderConfig {
      runner_class: "auv.app.netease_music".to_string(),
      display_name: "NetEase Music".to_string(),
      runtime: RunnerRuntime::Executable(ExecutableRunnerRuntime {
        executable: std::path::PathBuf::from(env!("CARGO_BIN_EXE_auv-runner-netease-music")),
        arguments: Vec::new(),
      }),
      descriptor_set,
      descriptor_set_sha256,
      services,
      supported_lifecycles: vec![
        RunnerProviderLifecycle::Ephemeral,
        RunnerProviderLifecycle::UnlessIdle,
      ],
      operation_capacity: 1,
    }],
    daemon_idle_timeout: None,
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind daemon with trusted NetEase provider");
  let shutdown = tokio_util::sync::CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let server = tokio::spawn(async move { bound.serve(server_shutdown).await });

  let mut client =
    auv_api_client::Client::connect(auv_api_client::ConnectEndpoint::Unix(socket.clone())).await.expect("connect to local daemon");
  let device = client.list_devices().await.expect("list local Device").into_iter().next().expect("local Device");
  let device = device.r#ref.expect("Device ref");
  let run = client
    .create_run(core_proto::CreateRunRequest {
      devices: vec![device.clone()],
      labels: Default::default(),
    })
    .await
    .expect("create Run");
  let run = run.r#ref.expect("Run ref");
  let claimed = client
    .claim_runner(core_proto::RunnerClaim {
      run: Some(run.clone()),
      device: Some(device.clone()),
      runner_class: Some(core_proto::RunnerClassRef {
        runner_class: "auv.app.netease_music".to_string(),
      }),
      required_capabilities: vec![core_proto::RunnerCapability {
        service: SERVICE.to_string(),
        methods: vec!["GetNowPlaying".to_string()],
      }],
      reuse_policy: core_proto::RunnerReusePolicy::PreferExisting as i32,
      lifecycle: Some(core_proto::RunnerLifecycle::Ephemeral as i32),
      operation_capacity: 1,
      ..Default::default()
    })
    .await
    .expect("claim daemon-supervised NetEase Runner");
  let lease = claimed.lease.and_then(|lease| lease.r#ref).expect("Runner lease ref");
  let transport = client.runner_transport(lease.clone()).expect("attach lease metadata");
  let mut netease = auv_netease_music::api::v1::netease_music_service_client::NeteaseMusicServiceClient::new(transport);
  let response = netease
    .get_now_playing(auv_netease_music::api::v1::GetNowPlayingRequest {
      application_bundle_id: Some("dev.auv.nonexistent-test-player".to_string()),
    })
    .await
    .expect("call app-owned RPC through daemon aggregation")
    .into_inner();
  assert!(!response.present, "a non-matching media owner must be filtered by the child Runner");

  assert!(client.release_runner_lease(lease).await.expect("release Runner lease"));

  // Exercise the application frontend as a real client process. This proves
  // the inherited AUV_CONTEXT -> high-level Run/Runner placement -> generated
  // custom client path, instead of covering only the lower-level transport.
  let context = auv_api_client::AuvContext {
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

  client.stop_run(run.run_id, core_proto::RunOutcome::Succeeded).await.expect("stop Run");
  shutdown.cancel();
  server.await.expect("join daemon task").expect("serve daemon");
}
