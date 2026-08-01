#![cfg(unix)]

use auv_api_proto::auv::api::core::v1 as core_proto;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::image::v1 as image_proto;
use auv_api_proto::auv::api::inference::v1 as inference_proto;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LOCAL_RUNNER_CLASS: &str = "auv.core.local";
const INFERENCE_RUNNER_CLASS: &str = "auv.inference.ultralytics";
const CAPTURE_SERVICE: &str = "auv.api.driver.v1.CaptureService";
const INFERENCE_SERVICE: &str = "auv.api.inference.v1.ObjectDetectionService";

struct Daemon(std::process::Child);

impl Drop for Daemon {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[tokio::test]
async fn one_run_routes_to_local_driver_and_inference_runners() {
  let directory = tempfile::tempdir().expect("create isolated daemon state");
  assert!(std::path::Path::new(env!("CARGO_BIN_EXE_auv")).is_file());
  let socket = directory.path().join("auv.sock");
  let store = directory.path().join("store");
  let endpoint = format!("unix://{}", socket.display());
  let mut daemon = Daemon(
    Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "serve",
        "--listen",
        &endpoint,
        "--store-root",
        store.to_str().expect("UTF-8 store path"),
        "--no-discovery",
      ])
      .stdout(Stdio::null())
      .stderr(Stdio::inherit())
      .spawn()
      .expect("start daemon with trusted first-party Runners"),
  );
  let deadline = Instant::now() + Duration::from_secs(10);
  while !socket.exists() && Instant::now() < deadline {
    assert!(daemon.0.try_wait().expect("poll daemon").is_none(), "daemon exited before binding");
    tokio::time::sleep(Duration::from_millis(25)).await;
  }
  assert!(socket.exists(), "daemon did not bind {}", socket.display());

  let mut client = auv_api_client::Client::connect(auv_api_client::ConnectEndpoint::Unix(socket)).await.expect("connect to local daemon");
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

  let local_claim = client
    .claim_runner(core_proto::RunnerClaim {
      run: Some(run.clone()),
      device: Some(device.clone()),
      runner_class: Some(core_proto::RunnerClassRef {
        runner_class: LOCAL_RUNNER_CLASS.to_string(),
      }),
      required_capabilities: vec![core_proto::RunnerCapability {
        service: CAPTURE_SERVICE.to_string(),
        methods: vec!["CaptureDisplay".to_string()],
      }],
      reuse_policy: core_proto::RunnerReusePolicy::PreferExisting as i32,
      lifecycle: Some(core_proto::RunnerLifecycle::Ephemeral as i32),
      operation_capacity: 1,
      ..Default::default()
    })
    .await
    .expect("claim daemon-supervised local Driver Runner");
  let local_runner = local_claim.runner.expect("local Runner resource");
  let local_runner_ref = local_runner.r#ref.clone().expect("local Runner ref");
  let local_lease = local_claim.lease.and_then(|lease| lease.r#ref).expect("local Runner lease ref");
  assert_eq!(local_runner.runner_class.as_ref().map(|class| class.runner_class.as_str()), Some(LOCAL_RUNNER_CLASS));
  assert_eq!(local_runner.device.as_ref(), Some(&device));
  assert_eq!(local_lease.run.as_ref(), Some(&run));
  assert_eq!(local_lease.runner.as_ref(), Some(&local_runner_ref));

  let local_client = client.runner(local_lease.clone()).expect("create typed local Runner client");
  let local_error = local_client
    .displays()
    .capture(Some(driver_proto::DisplaySelector {
      selector: Some(driver_proto::display_selector::Selector::Name(String::new())),
    }))
    .await
    .expect_err("local child must reject an empty display selector before capture");
  assert_eq!(local_error.code(), tonic::Code::InvalidArgument);

  let inference_claim = client
    .claim_runner(core_proto::RunnerClaim {
      run: Some(run.clone()),
      device: Some(device.clone()),
      runner_class: Some(core_proto::RunnerClassRef {
        runner_class: INFERENCE_RUNNER_CLASS.to_string(),
      }),
      required_capabilities: vec![core_proto::RunnerCapability {
        service: INFERENCE_SERVICE.to_string(),
        methods: vec!["DetectObjects".to_string()],
      }],
      reuse_policy: core_proto::RunnerReusePolicy::PreferExisting as i32,
      lifecycle: Some(core_proto::RunnerLifecycle::Ephemeral as i32),
      operation_capacity: 1,
      ..Default::default()
    })
    .await
    .expect("claim daemon-supervised inference Runner");
  let inference_runner = inference_claim.runner.expect("inference Runner resource");
  let inference_runner_ref = inference_runner.r#ref.clone().expect("inference Runner ref");
  let inference_lease = inference_claim.lease.and_then(|lease| lease.r#ref).expect("inference Runner lease ref");
  assert_eq!(inference_runner.runner_class.as_ref().map(|class| class.runner_class.as_str()), Some(INFERENCE_RUNNER_CLASS));
  assert_eq!(inference_runner.device.as_ref(), Some(&device));
  assert_eq!(inference_lease.run.as_ref(), Some(&run));
  assert_eq!(inference_lease.runner.as_ref(), Some(&inference_runner_ref));
  assert_ne!(local_runner_ref, inference_runner_ref);
  assert_ne!(local_lease.lease_id, inference_lease.lease_id);

  let inference_client = client.runner(inference_lease.clone()).expect("create typed inference Runner client");
  let inference_error = inference_client
    .inference()
    .detect_objects(
      inference_proto::ObjectDetectorSpec {
        detector_id: "malformed-frame-fixture".to_string(),
        model_path: "/missing/model.onnx".to_string(),
        ..Default::default()
      },
      image_proto::RgbFrame {
        width: 2,
        height: 2,
        data: vec![0; 11],
      },
    )
    .await
    .expect_err("child must reject malformed RGB before loading the model");
  assert_eq!(inference_error.code(), tonic::Code::InvalidArgument);
  assert!(inference_error.message().contains("expected 12"));

  let running = client.list_runners().await.expect("list both running Runners");
  assert!(running.iter().any(|runner| runner.r#ref.as_ref() == Some(&local_runner_ref)));
  assert!(running.iter().any(|runner| runner.r#ref.as_ref() == Some(&inference_runner_ref)));

  assert!(client.release_runner_lease(local_lease).await.expect("release local Runner lease"));
  assert!(client.release_runner_lease(inference_lease).await.expect("release inference Runner lease"));
  let stopped = client.stop_run(run.run_id, core_proto::RunOutcome::Succeeded).await.expect("stop shared Run");
  assert_eq!(stopped.phase, core_proto::RunPhase::Succeeded as i32);
  daemon.0.kill().expect("stop daemon");
  daemon.0.wait().expect("reap daemon");
}
