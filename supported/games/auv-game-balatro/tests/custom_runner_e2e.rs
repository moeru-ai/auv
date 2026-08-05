#![cfg(unix)]

use auv_api_proto::auv::api::daemon::v1 as daemon_proto;
use auv_api_proto::auv::api::image::v1 as image_proto;
use auv_daemon::runner_provider::{ExecutableRunnerRuntime, RunnerProviderConfig, RunnerRuntime};
use auv_daemon::{Config, ListenEndpoint, Server};

#[tokio::test]
async fn daemon_routes_the_app_owned_balatro_runner() {
  let directory = tempfile::tempdir().expect("create isolated daemon state");
  let socket = directory.path().join("auv.sock");
  let bound = Server::bind(Config {
    pairing_store: None,
    listeners: vec![ListenEndpoint::Unix {
      path: socket.clone(),
    }],
    discovery_file: None,
    publish_discovery: false,
    store_root: directory.path().join("store"),
    runner_providers: vec![RunnerProviderConfig {
      runner_class: "auv.game.balatro".to_string(),
      runtime: RunnerRuntime::Executable(ExecutableRunnerRuntime {
        executable: env!("CARGO_BIN_EXE_auv-runner-balatro").into(),
        arguments: Vec::new(),
        working_directory: None,
        environment: Default::default(),
      }),
    }],
    daemon_idle_timeout: None,
    first_party_runners: Default::default(),
  })
  .await
  .expect("bind daemon with trusted Balatro provider");
  let shutdown = tokio_util::sync::CancellationToken::new();
  let server_shutdown = shutdown.clone();
  let server = tokio::spawn(async move { bound.serve(server_shutdown).await });

  let client =
    auv_api_client::protocol::grpc::Client::connect(auv_api_client::ConnectEndpoint::Unix(socket)).await.expect("connect to daemon");
  let device = client.devices().list_devices().await.expect("list Device").into_iter().next().expect("local Device");
  let device = device.r#ref.expect("Device ref");
  let run = client
    .runs()
    .create_run(daemon_proto::CreateRunRequest {
      devices: vec![device.clone()],
      labels: Default::default(),
    })
    .await
    .expect("create Run")
    .r#ref
    .expect("Run ref");
  let transport = client
    .routed_transport(auv_api_client::RunnerRoute {
      device_id: Some(device.device_id.clone()),
      run_id: Some(run.run_id.clone()),
      runner_class: "auv.game.balatro".to_string(),
    })
    .expect("attach route metadata");
  let error = auv_game_balatro::api::v1::balatro_detection_service_client::BalatroDetectionServiceClient::new(transport)
    .detect_objects(auv_game_balatro::api::v1::DetectObjectsRequest {
      detector: Some(auv_game_balatro::api::v1::ObjectDetectorSpec {
        detector_id: "test".to_string(),
        source: Some(auv_game_balatro::api::v1::object_detector_spec::Source::RunnerPath("/missing/model.onnx".to_string())),
        ..Default::default()
      }),
      frame: Some(image_proto::RgbFrame {
        width: 2,
        height: 2,
        data: vec![0; 11],
      }),
    })
    .await
    .expect_err("malformed frame must fail before model loading");
  assert_eq!(error.code(), tonic::Code::InvalidArgument);

  client.runs().stop_run(run.run_id, daemon_proto::RunOutcome::Succeeded).await.expect("stop Run");
  shutdown.cancel();
  server.await.expect("join daemon task").expect("serve daemon");
}
