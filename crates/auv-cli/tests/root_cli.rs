//! Subprocess coverage for the root AUV CLI boundary.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn run(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_auv")).args(args).output().expect("run root auv binary")
}

fn stdout(output: &std::process::Output) -> String {
  String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
  String::from_utf8_lossy(&output.stderr).into_owned()
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

fn wait_for_path(child: &mut Child, path: &std::path::Path) {
  let deadline = Instant::now() + Duration::from_secs(10);
  while Instant::now() < deadline {
    if path.exists() {
      return;
    }
    if let Some(status) = child.try_wait().expect("inspect daemon process") {
      panic!("daemon exited before publishing {}: {status}", path.display());
    }
    std::thread::sleep(Duration::from_millis(25));
  }
  panic!("daemon did not publish {} before the deadline", path.display());
}

#[cfg(unix)]
fn interrupt(child: &Child) {
  let status = Command::new("/bin/kill").args(["-INT", &child.id().to_string()]).status().expect("signal daemon");
  assert!(status.success(), "SIGINT delivery failed: {status}");
}

#[test]
fn root_version_exits_zero_and_names_the_package_version() {
  let output = run(&["--version"]);

  assert_eq!(output.status.code(), Some(0), "auv --version must exit 0; stderr={}", stderr(&output));
  assert_eq!(stdout(&output), format!("auv {}\n", env!("CARGO_PKG_VERSION")));
  assert!(stderr(&output).is_empty(), "auv --version must not write stderr: {}", stderr(&output));
}

#[test]
fn root_help_does_not_advertise_supported_app_or_game_frontends() {
  let output = run(&["--help"]);

  assert_eq!(output.status.code(), Some(0), "auv --help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  for removed_surface in [
    "auv-godot",
    "auv-osu",
    "auv-minecraft",
    "app.textedit.document.write",
    "__auv-internal-runner",
  ] {
    assert!(!help.contains(removed_surface), "root help must not advertise {removed_surface}:\n{help}");
  }
  assert!(
    !help.lines().any(|line| line.trim_start().starts_with("permissions ")),
    "root help must not advertise the removed permissions command:\n{help}"
  );
  assert!(
    !help.lines().any(|line| line.trim_start().starts_with("pairing ")),
    "pairing is a Device process and must not be a top-level command:\n{help}"
  );

  for expected in [
    "Commands:",
    "doctor",
    "invoke",
    "serve",
    "devices",
    "runner",
    "run",
    "mcp",
    "plugin",
    "Examples:",
  ] {
    assert!(help.contains(expected), "root help must contain {expected:?}:\n{help}");
  }
}

#[test]
fn malformed_internal_runner_invocation_fails_before_plugin_or_clap_dispatch() {
  let output = run(&["__auv-internal-runner", "unknown-role"]);
  assert_eq!(output.status.code(), Some(2));
  assert!(stdout(&output).is_empty());
  assert_eq!(stderr(&output), "invalid internal AUV Runner invocation\n");
}

#[test]
fn runner_namespace_contains_resources_not_driver_capabilities() {
  let runner = run(&["runner", "--help"]);
  assert_eq!(runner.status.code(), Some(0), "runner help must exit 0; stderr={}", stderr(&runner));
  let runner_help = stdout(&runner);
  assert!(
    !runner_help.lines().any(|line| line.trim_start().starts_with("displays ")),
    "unexpected runner capability command:\n{runner_help}"
  );
  assert!(
    !runner_help.lines().any(|line| line.trim_start().starts_with("windows ")),
    "unexpected runner capability command:\n{runner_help}"
  );

  let invoke = run(&["invoke", "--help"]);
  assert_eq!(invoke.status.code(), Some(0), "invoke help must exit 0; stderr={}", stderr(&invoke));
  let invoke_help = stdout(&invoke);
  assert!(invoke_help.contains("display.list"), "display capability must remain under invoke:\n{invoke_help}");
  assert!(invoke_help.contains("window.list"), "window capability must remain under invoke:\n{invoke_help}");
}

#[cfg(unix)]
#[test]
fn local_serve_and_devices_list_use_the_unix_daemon() {
  let directory = tempfile::tempdir().expect("temporary daemon directory");
  let socket = directory.path().join("auv.sock");
  let store = directory.path().join("store");
  let discovery = directory.path().join("daemon.json");
  let endpoint = format!("unix://{}", socket.display());
  let child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--listen",
      &endpoint,
      "--store-root",
      store.to_str().unwrap(),
      "--discovery-file",
      discovery.to_str().unwrap(),
    ])
    .env("HOSTNAME", "inherited-device")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("start local daemon");
  let mut daemon = ChildGuard(child);
  wait_for_path(&mut daemon.0, &socket);

  let classes = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["runner", "classes", "--endpoint", &endpoint, "--json"])
    .output()
    .expect("list trusted RunnerClasses");
  assert!(classes.status.success(), "stderr={}", stderr(&classes));
  let classes: serde_json::Value = serde_json::from_slice(&classes.stdout).expect("RunnerClass JSON");
  let classes = classes.as_array().expect("RunnerClass JSON array");
  let local = classes.iter().find(|class| class["runner_class"] == "auv.core.local").expect("local Driver RunnerClass");
  assert!(
    classes.iter().any(|class| class["runner_class"] == "auv.inference.ultralytics"),
    "first-party inference RunnerClass must be discoverable: {classes:?}"
  );
  assert!(
    local["capabilities"]
      .as_array()
      .is_some_and(|capabilities| capabilities.iter().any(|capability| capability["service"] == "auv.api.driver.v1.DisplayService"))
  );

  let classes_table =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["runner", "classes", "--endpoint", &endpoint]).output().expect("render RunnerClass table");
  assert!(classes_table.status.success(), "stderr={}", stderr(&classes_table));
  let classes_table = stdout(&classes_table);
  assert!(classes_table.lines().next().is_some_and(|header| header.contains("CLASS") && header.contains("CAPABILITIES")), "{classes_table}");
  assert!(classes_table.contains("auv.core.local"), "{classes_table}");

  let created_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "--device",
      "inherited-device",
      "run",
      "create",
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("create explicit Run");
  assert!(created_run.status.success(), "stderr={}", stderr(&created_run));
  let created_run: serde_json::Value = serde_json::from_slice(&created_run.stdout).expect("Run JSON");
  let run_id = created_run["run_id"].as_str().expect("Run ID");
  assert!(run_id.starts_with("run_"));
  assert_eq!(created_run["phase"], "RUN_PHASE_RUNNING");
  assert_eq!(created_run["device_ids"].as_array().map(Vec::len), Some(1), "explicit Run must resolve to exactly one Device: {created_run}");

  let fetched_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["run", "get", run_id, "--endpoint", &endpoint, "--json"])
    .output()
    .expect("get explicit Run");
  assert!(fetched_run.status.success(), "stderr={}", stderr(&fetched_run));
  let fetched_run: serde_json::Value = serde_json::from_slice(&fetched_run.stdout).expect("Run get JSON");
  assert_eq!(fetched_run["run_id"], run_id);
  let fetched_run_table =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["run", "get", run_id, "--endpoint", &endpoint]).output().expect("render Run table");
  assert!(fetched_run_table.status.success(), "stderr={}", stderr(&fetched_run_table));
  let fetched_run_table = stdout(&fetched_run_table);
  assert!(
    fetched_run_table.lines().next().is_some_and(|header| header.contains("RUN ID") && header.contains("DEVICE IDS")),
    "{fetched_run_table}"
  );
  assert!(fetched_run_table.contains(run_id), "{fetched_run_table}");
  wait_for_path(&mut daemon.0, &discovery);

  let listed = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", &endpoint, "--json"])
    .output()
    .expect("list daemon Devices");
  assert!(listed.status.success(), "stderr={}", stderr(&listed));
  let devices: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("Device JSON");
  assert_eq!(devices.as_array().map(Vec::len), Some(1), "daemon must expose exactly one local Device: {devices}");
  assert_eq!(devices[0]["local"], true);
  assert!(devices[0]["device_id"].as_str().is_some_and(|id| id.starts_with("device_")));
  let listed_table =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["devices", "list", "--endpoint", &endpoint]).output().expect("render Device table");
  assert!(listed_table.status.success(), "stderr={}", stderr(&listed_table));
  let listed_table = stdout(&listed_table);
  assert!(listed_table.lines().next().is_some_and(|header| header.contains("DEVICE ID") && header.contains("STATUS")), "{listed_table}");
  assert!(listed_table.contains("macos"), "{listed_table}");

  interrupt(&daemon.0);
  daemon.0.wait().expect("wait for local daemon");
  let deadline = Instant::now() + Duration::from_secs(2);
  while Instant::now() < deadline && (socket.exists() || discovery.exists()) {
    std::thread::sleep(Duration::from_millis(25));
  }
  assert!(!socket.exists(), "daemon must clean up its Unix socket");
  assert!(!discovery.exists(), "daemon must clean up its discovery descriptor");
}

#[cfg(unix)]
#[test]
fn local_daemon_supervises_and_serves_a_real_driver_runner() {
  let directory = tempfile::tempdir().expect("temporary daemon directory");
  let socket = directory.path().join("auv.sock");
  let store = directory.path().join("store");
  let discovery = directory.path().join("daemon.json");
  let endpoint = format!("unix://{}", socket.display());
  let child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--listen",
      &endpoint,
      "--store-root",
      store.to_str().unwrap(),
      "--discovery-file",
      discovery.to_str().unwrap(),
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("start local daemon");
  let mut daemon = ChildGuard(child);
  wait_for_path(&mut daemon.0, &socket);

  let created = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "runner",
      "create",
      "--class",
      "auv.core.local",
      "--lifecycle",
      "unless-shutdown",
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("create local driver Runner");
  assert!(created.status.success(), "stderr={}", stderr(&created));
  let runner: serde_json::Value = serde_json::from_slice(&created.stdout).expect("Runner JSON");
  let runner_id = runner["runner_id"].as_str().expect("Runner ID");
  let process_id = runner["process_id"].as_u64().expect("Runner process ID");
  assert!(runner_id.starts_with("runner_"));
  assert_eq!(runner["phase"], "RUNNER_PHASE_READY");
  assert!(process_id > 0);
  let runners_table =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["runner", "list", "--endpoint", &endpoint]).output().expect("render Runner table");
  assert!(runners_table.status.success(), "stderr={}", stderr(&runners_table));
  let runners_table = stdout(&runners_table);
  assert!(
    runners_table.lines().next().is_some_and(|header| header.contains("RUNNER ID") && header.contains("OPERATIONS")),
    "{runners_table}"
  );
  assert!(runners_table.contains(runner_id), "{runners_table}");
  let capabilities = runner["capabilities"].as_array().expect("Runner capabilities");
  let display =
    capabilities.iter().find(|capability| capability["service"] == "auv.api.driver.v1.DisplayService").expect("DisplayService capability");
  assert_eq!(display["methods"], serde_json::json!(["ListDisplays"]));
  let window =
    capabilities.iter().find(|capability| capability["service"] == "auv.api.driver.v1.WindowService").expect("WindowService capability");
  assert_eq!(window["methods"], serde_json::json!(["ListWindows", "ResolveWindow"]));
  let capture =
    capabilities.iter().find(|capability| capability["service"] == "auv.api.driver.v1.CaptureService").expect("CaptureService capability");
  assert_eq!(capture["methods"], serde_json::json!(["CaptureWindow", "CaptureDisplay", "CaptureRegion"]));
  let text_recognition = capabilities
    .iter()
    .find(|capability| capability["service"] == "auv.api.driver.v1.TextRecognitionService")
    .expect("TextRecognitionService capability");
  assert_eq!(text_recognition["methods"], serde_json::json!(["RecognizeText", "FindWindowText", "FindDisplayText"]));
  let input =
    capabilities.iter().find(|capability| capability["service"] == "auv.api.driver.v1.InputService").expect("InputService capability");
  assert_eq!(
    input["methods"],
    serde_json::json!([
      "ClickWindowPoint",
      "ClickScreenPoint",
      "TypeText",
      "PasteText",
      "PressKey"
    ])
  );
  assert_eq!(runner["descriptor_set_sha256"].as_str().map(str::len), Some(64));
  assert!(Command::new("/bin/kill").args(["-0", &process_id.to_string()]).status().expect("probe Runner process").success());

  let listed =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["runner", "list", "--endpoint", &endpoint, "--json"]).output().expect("list Runners");
  assert!(listed.status.success(), "stderr={}", stderr(&listed));
  let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("Runner list JSON");
  assert_eq!(listed.as_array().map(Vec::len), Some(1));
  assert_eq!(listed[0]["runner_id"], runner_id);

  let fetched = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "runner",
      "get",
      runner_id,
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("get Runner");
  assert!(fetched.status.success(), "stderr={}", stderr(&fetched));
  let fetched: serde_json::Value = serde_json::from_slice(&fetched.stdout).expect("Runner get JSON");
  assert_eq!(fetched["process_id"], process_id);

  let run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["run", "create", "--endpoint", &endpoint, "--json"])
    .output()
    .expect("create Run for typed Display claim");
  assert!(run.status.success(), "stderr={}", stderr(&run));
  let run: serde_json::Value = serde_json::from_slice(&run.stdout).expect("Run JSON");
  let run_id = run["run_id"].as_str().expect("Run ID");

  tokio::runtime::Runtime::new().expect("test runtime").block_on(async {
    let mut client = auv_api_client::Client::connect(endpoint.parse().expect("daemon endpoint")).await.expect("connect API client");
    let claim = auv_api_proto::auv::api::core::v1::RunnerClaim {
      run: Some(auv_api_proto::auv::api::core::v1::RunRef {
        run_id: run_id.to_string(),
      }),
      device: None,
      device_match_labels: Default::default(),
      runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
        runner_class: "auv.core.local".to_string(),
      }),
      required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
        service: "auv.api.driver.v1.DisplayService".to_string(),
        methods: vec!["ListDisplays".to_string()],
      }],
      match_labels: Default::default(),
      reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::PreferExisting as i32,
      lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32),
      idle_timeout: None,
      operation_capacity: 1,
    };
    let first = client.claim_runner(claim.clone()).await.expect("first reusable claim");
    let second = client.claim_runner(claim).await.expect("second reusable claim");
    assert!(!first.runner_created);
    assert!(!second.runner_created);
    assert_eq!(first.runner.as_ref().and_then(|runner| runner.r#ref.as_ref()).map(|runner| runner.runner_id.as_str()), Some(runner_id));
    assert_eq!(second.runner.as_ref().and_then(|runner| runner.r#ref.as_ref()).map(|runner| runner.runner_id.as_str()), Some(runner_id));
    let first_lease = first.lease.as_ref().and_then(|lease| lease.r#ref.clone()).expect("first lease ref");
    let capture_error = client
      .capture_runner_display(
        first_lease,
        Some(auv_api_proto::auv::api::driver::v1::DisplaySelector {
          selector: Some(auv_api_proto::auv::api::driver::v1::display_selector::Selector::Name(String::new())),
        }),
      )
      .await
      .expect_err("typed CaptureDisplay must validate the display selector in the Runner");
    assert_eq!(capture_error.code(), tonic::Code::InvalidArgument);
    client.release_runner_lease(first.lease.and_then(|lease| lease.r#ref).expect("first lease")).await.expect("release first lease");
    client.release_runner_lease(second.lease.and_then(|lease| lease.r#ref).expect("second lease")).await.expect("release second lease");

    let ocr_claim = client
      .claim_runner(auv_api_proto::auv::api::core::v1::RunnerClaim {
        run: Some(auv_api_proto::auv::api::core::v1::RunRef {
          run_id: run_id.to_string(),
        }),
        runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
          runner_class: "auv.core.local".to_string(),
        }),
        required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
          service: "auv.api.driver.v1.TextRecognitionService".to_string(),
          methods: vec!["RecognizeText".to_string()],
        }],
        reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::PreferExisting as i32,
        lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32),
        operation_capacity: 1,
        ..Default::default()
      })
      .await
      .expect("claim TextRecognitionService");
    let ocr_lease = ocr_claim.lease.and_then(|lease| lease.r#ref).expect("OCR lease");
    let error = client
      .recognize_runner_text(
        ocr_lease.clone(),
        auv_api_proto::auv::api::driver::v1::RecognizeTextRequest {
          capture: Some(auv_api_proto::auv::api::driver::v1::CapturedFrame {
            image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
              width: 2,
              height: 1,
              data: vec![0; 7],
            }),
            bounds: Some(auv_api_proto::auv::api::driver::v1::ScreenRect {
              x: 0.0,
              y: 0.0,
              width: 2.0,
              height: 1.0,
            }),
            scale_factor: 1.0,
            ..Default::default()
          }),
          ..Default::default()
        },
      )
      .await
      .expect_err("child rejects malformed RGBA without invoking OCR");
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    client.release_runner_lease(ocr_lease).await.expect("release OCR lease");

    let input_claim = client
      .claim_runner(auv_api_proto::auv::api::core::v1::RunnerClaim {
        run: Some(auv_api_proto::auv::api::core::v1::RunRef {
          run_id: run_id.to_string(),
        }),
        runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
          runner_class: "auv.core.local".to_string(),
        }),
        required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
          service: "auv.api.driver.v1.InputService".to_string(),
          methods: vec![
            "ClickScreenPoint".to_string(),
            "PasteText".to_string(),
            "PressKey".to_string(),
          ],
        }],
        reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::PreferExisting as i32,
        lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32),
        operation_capacity: 1,
        ..Default::default()
      })
      .await
      .expect("claim InputService");
    let input_lease = input_claim.lease.and_then(|lease| lease.r#ref).expect("Input lease");
    let error = client
      .click_runner_screen_point(
        input_lease.clone(),
        auv_api_proto::auv::api::driver::v1::ClickScreenPointRequest {
          point: Some(auv_api_proto::auv::api::driver::v1::ScreenPoint {
            x: f64::NAN,
            y: 0.0,
          }),
          options: Some(Default::default()),
          ..Default::default()
        },
      )
      .await
      .expect_err("non-finite screen point must fail in the child before native input delivery");
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    let error = client
      .press_runner_key(
        input_lease.clone(),
        auv_api_proto::auv::api::driver::v1::PressKeyRequest {
          key: String::new(),
          ..Default::default()
        },
      )
      .await
      .expect_err("empty key must be rejected by the child before native input delivery");
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    let error = client
      .paste_runner_text(
        input_lease.clone(),
        auv_api_proto::auv::api::driver::v1::PasteTextRequest {
          text: String::new(),
          options: Some(Default::default()),
          ..Default::default()
        },
      )
      .await
      .expect_err("empty paste must be rejected before clipboard mutation or native input delivery");
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    client.release_runner_lease(input_lease).await.expect("release Input lease");

    #[cfg(target_os = "macos")]
    {
      let application_claim = client
        .claim_runner(auv_api_proto::auv::api::core::v1::RunnerClaim {
          run: Some(auv_api_proto::auv::api::core::v1::RunRef {
            run_id: run_id.to_string(),
          }),
          runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
            runner_class: "auv.core.local".to_string(),
          }),
          required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
            service: "auv.api.driver.macos.v1.ApplicationService".to_string(),
            methods: vec!["ActivateBundleId".to_string()],
          }],
          reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::PreferExisting as i32,
          lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32),
          operation_capacity: 1,
          ..Default::default()
        })
        .await
        .expect("claim macOS ApplicationService");
      let application_lease = application_claim.lease.and_then(|lease| lease.r#ref).expect("Application lease");
      let error = client
        .activate_runner_bundle_id(application_lease.clone(), "  ", None)
        .await
        .expect_err("blank bundle id must fail before application activation");
      assert_eq!(error.code(), tonic::Code::InvalidArgument);
      let error = client
        .activate_runner_bundle_id(
          application_lease.clone(),
          "com.example.MustNotActivate",
          Some(prost_types::Duration {
            seconds: -1,
            nanos: 0,
          }),
        )
        .await
        .expect_err("negative settle must fail before application activation");
      assert_eq!(error.code(), tonic::Code::InvalidArgument);
      client.release_runner_lease(application_lease).await.expect("release Application lease");

      let overlay_claim = client
        .claim_runner(auv_api_proto::auv::api::core::v1::RunnerClaim {
          run: Some(auv_api_proto::auv::api::core::v1::RunRef {
            run_id: run_id.to_string(),
          }),
          runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
            runner_class: "auv.core.local".to_string(),
          }),
          required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
            service: "auv.api.driver.v1.OverlayService".to_string(),
            methods: vec!["ShowOverlay".to_string()],
          }],
          reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::PreferExisting as i32,
          lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessShutdown as i32),
          operation_capacity: 1,
          ..Default::default()
        })
        .await
        .expect("claim OverlayService");
      let overlay_lease = overlay_claim.lease.and_then(|lease| lease.r#ref).expect("Overlay lease");
      let error = client
        .show_runner_overlay(
          overlay_lease.clone(),
          auv_api_proto::auv::api::driver::v1::ShowOverlayRequest {
            overlay: Some(auv_api_proto::auv::api::driver::v1::Overlay {
              layers: vec![auv_api_proto::auv::api::driver::v1::OverlayLayer {
                layer: Some(auv_api_proto::auv::api::driver::v1::overlay_layer::Layer::Cursor(
                  auv_api_proto::auv::api::driver::v1::Cursor {
                    point: Some(auv_api_proto::auv::api::driver::v1::ScreenPoint {
                      x: f64::NAN,
                      y: 0.0,
                    }),
                    ..Default::default()
                  },
                )),
              }],
            }),
            ..Default::default()
          },
        )
        .await
        .expect_err("non-finite overlay geometry must fail before native rendering");
      assert_eq!(error.code(), tonic::Code::InvalidArgument);
      client.release_runner_lease(overlay_lease).await.expect("release Overlay lease");
    }
  });

  let invoked_displays = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "--device-id",
      runner["device_id"].as_str().expect("Runner Device ID"),
      "--run",
      run_id,
      "invoke",
      "display.list",
      "--json",
    ])
    .env("AUV_ENDPOINT", &endpoint)
    .output()
    .expect("invoke display.list through selected Run and Runner");
  assert!(invoked_displays.status.success(), "stderr={}", stderr(&invoked_displays));
  let invoked_displays: serde_json::Value = serde_json::from_slice(&invoked_displays.stdout).expect("invoke Display JSON");
  assert_eq!(
    invoked_displays["run_id"],
    run_id.strip_prefix("run_").expect("control Run IDs carry the tracing UUID"),
    "selected invoke must record against the selected control Run identity"
  );
  assert!(
    !invoked_displays["result"]["displays"].as_array().expect("invoke Display list").is_empty(),
    "selected invoke must return the typed result produced by the daemon-backed Runner"
  );

  let invoked_windows = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "--device-id",
      runner["device_id"].as_str().expect("Runner Device ID"),
      "--run",
      run_id,
      "invoke",
      "window.list",
      "--json",
    ])
    .env("AUV_ENDPOINT", &endpoint)
    .output()
    .expect("invoke window.list through selected Run and Runner");
  assert!(invoked_windows.status.success(), "stderr={}", stderr(&invoked_windows));
  let invoked_windows: serde_json::Value = serde_json::from_slice(&invoked_windows.stdout).expect("invoke Window JSON");
  assert!(invoked_windows["result"].is_array(), "selected window.list must preserve its local direct-result schema");
  if let Some(window) = invoked_windows["result"].as_array().and_then(|windows| {
    windows.iter().find(|window| {
      let Some(bundle_id) = window["app_bundle_id"].as_str() else {
        return false;
      };
      let Some(title) = window["title"].as_str().filter(|title| !title.is_empty()) else {
        return false;
      };
      windows
        .iter()
        .filter(|candidate| {
          candidate["app_bundle_id"].as_str() == Some(bundle_id)
            && candidate["title"].as_str().is_some_and(|candidate_title| candidate_title.contains(title))
        })
        .count()
        == 1
    })
  }) {
    let rejected_click = Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "--device-id",
        runner["device_id"].as_str().expect("Runner Device ID"),
        "--run",
        run_id,
        "invoke",
        "input.clickWindowPoint",
        "--target",
        window["app_bundle_id"].as_str().unwrap(),
        "--title",
        window["title"].as_str().unwrap(),
        "--offset-x",
        "1000000",
        "--offset-y",
        "1000000",
        "--json",
      ])
      .env("AUV_ENDPOINT", &endpoint)
      .output()
      .expect("resolve selected Window without delivering an out-of-bounds click");
    assert!(!rejected_click.status.success(), "out-of-bounds click unexpectedly succeeded");
    let rejected_click: serde_json::Value = serde_json::from_slice(&rejected_click.stdout).expect("failed click JSON");
    assert!(
      rejected_click["failure"].as_str().is_some_and(|failure| failure.contains("outside target window bounds")),
      "selected click must reject after typed Window resolution and before native input: {rejected_click}"
    );
  }

  for (command_id, validation) in [
    ("window.findText", "query is required"),
    ("screen.findText", "query is required"),
    ("input.typeText", "text is required"),
    ("input.key", "key is required"),
  ] {
    let rejected_input = Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "--device-id",
        runner["device_id"].as_str().expect("Runner Device ID"),
        "--run",
        run_id,
        "invoke",
        command_id,
        "",
        "--json",
      ])
      .env("AUV_ENDPOINT", &endpoint)
      .output()
      .expect("send structurally invalid input through selected Runner");
    assert!(!rejected_input.status.success(), "{command_id} unexpectedly accepted empty input");
    let rejected_input_json: serde_json::Value = serde_json::from_slice(&rejected_input.stdout).expect("failed invoke JSON");
    assert!(
      rejected_input_json["failure"].as_str().is_some_and(|failure| failure.contains(validation)),
      "{command_id} must reach typed Runner validation before native delivery; output={rejected_input_json}"
    );
  }

  let running_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["run", "get", run_id, "--endpoint", &endpoint, "--json"])
    .output()
    .expect("get explicit Run after invoke");
  assert!(running_run.status.success(), "stderr={}", stderr(&running_run));
  let running_run: serde_json::Value = serde_json::from_slice(&running_run.stdout).expect("Run JSON after invoke");
  assert_eq!(running_run["phase"], "RUN_PHASE_RUNNING", "invoke must not stop a caller-owned explicit Run");

  let runners_after_invoke = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["runner", "list", "--endpoint", &endpoint, "--json"])
    .output()
    .expect("list Runners after selected invoke");
  assert!(runners_after_invoke.status.success(), "stderr={}", stderr(&runners_after_invoke));
  let runners_after_invoke: serde_json::Value = serde_json::from_slice(&runners_after_invoke.stdout).expect("Runner list JSON after invoke");
  let invoked_runner_id = runners_after_invoke
    .as_array()
    .expect("Runner list")
    .iter()
    .find_map(|candidate| (candidate["runner_id"] != runner_id).then(|| candidate["runner_id"].as_str()).flatten())
    .expect("selected invoke must create its unless-idle Runner when only an incompatible unless-shutdown Runner exists");
  let stopped_invoke_runner = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "runner",
      "stop",
      invoked_runner_id,
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("stop selected invoke Runner after observing it");
  assert!(stopped_invoke_runner.status.success(), "stderr={}", stderr(&stopped_invoke_runner));

  let (ephemeral_runner_id, ephemeral_pid) = tokio::runtime::Runtime::new().expect("test runtime").block_on(async {
    let mut client = auv_api_client::Client::connect(endpoint.parse().expect("daemon endpoint")).await.expect("connect API client");
    let response = client
      .claim_runner(auv_api_proto::auv::api::core::v1::RunnerClaim {
        run: Some(auv_api_proto::auv::api::core::v1::RunRef {
          run_id: run_id.to_string(),
        }),
        device: None,
        device_match_labels: Default::default(),
        runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
          runner_class: "auv.core.local".to_string(),
        }),
        required_capabilities: vec![auv_api_proto::auv::api::core::v1::RunnerCapability {
          service: "auv.api.driver.v1.DisplayService".to_string(),
          methods: vec!["ListDisplays".to_string()],
        }],
        match_labels: Default::default(),
        reuse_policy: auv_api_proto::auv::api::core::v1::RunnerReusePolicy::CreateNew as i32,
        lifecycle: Some(auv_api_proto::auv::api::core::v1::RunnerLifecycle::Ephemeral as i32),
        idle_timeout: None,
        operation_capacity: 1,
      })
      .await
      .expect("claim ephemeral Runner");
    let runner = response.runner.expect("claimed Runner");
    (runner.r#ref.expect("Runner ref").runner_id, runner.process_id)
  });
  assert!(Command::new("/bin/kill").args(["-0", &ephemeral_pid.to_string()]).status().expect("probe ephemeral Runner").success());

  let stopped_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["run", "stop", run_id, "--endpoint", &endpoint, "--json"])
    .output()
    .expect("stop Run and release its Runner leases");
  assert!(stopped_run.status.success(), "stderr={}", stderr(&stopped_run));
  let stopped_run: serde_json::Value = serde_json::from_slice(&stopped_run.stdout).expect("stopped Run JSON");
  assert_eq!(stopped_run["phase"], "RUN_PHASE_CANCELED");
  let ephemeral_exit_deadline = Instant::now() + Duration::from_secs(2);
  while Instant::now() < ephemeral_exit_deadline
    && Command::new("/bin/kill")
      .args(["-0", &ephemeral_pid.to_string()])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .expect("poll ephemeral Runner")
      .success()
  {
    std::thread::sleep(Duration::from_millis(25));
  }
  assert!(
    !Command::new("/bin/kill")
      .args(["-0", &ephemeral_pid.to_string()])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .expect("probe stopped ephemeral Runner")
      .success(),
    "StopRun must release the lease and stop ephemeral Runner {ephemeral_runner_id}"
  );

  let stopped = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "runner",
      "stop",
      runner_id,
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("stop local driver Runner");
  assert!(stopped.status.success(), "stderr={}", stderr(&stopped));
  let stopped: serde_json::Value = serde_json::from_slice(&stopped.stdout).expect("stopped Runner JSON");
  assert_eq!(stopped["phase"], "RUNNER_PHASE_STOPPED");
  assert!(
    !Command::new("/bin/kill")
      .args(["-0", &process_id.to_string()])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .expect("probe stopped Runner process")
      .success()
  );

  let after_stop = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["runner", "list", "--endpoint", &endpoint, "--json"])
    .output()
    .expect("list Runners after stop");
  assert!(after_stop.status.success(), "stderr={}", stderr(&after_stop));
  let after_stop: serde_json::Value = serde_json::from_slice(&after_stop.stdout).expect("empty Runner list JSON");
  assert_eq!(after_stop.as_array().map(Vec::len), Some(0));

  let resident = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "runner",
      "create",
      "--class",
      "auv.core.local",
      "--lifecycle",
      "unless-shutdown",
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("create Runner owned by daemon shutdown");
  assert!(resident.status.success(), "stderr={}", stderr(&resident));
  let resident: serde_json::Value = serde_json::from_slice(&resident.stdout).expect("resident Runner JSON");
  let resident_pid = resident["process_id"].as_u64().expect("resident Runner process ID");

  daemon.0.kill().expect("kill local daemon without graceful shutdown");
  daemon.0.wait().expect("wait for killed local daemon");
  let child_exit_deadline = Instant::now() + Duration::from_secs(2);
  while Instant::now() < child_exit_deadline
    && Command::new("/bin/kill")
      .args(["-0", &resident_pid.to_string()])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .expect("poll daemon-owned Runner after parent crash")
      .success()
  {
    std::thread::sleep(Duration::from_millis(25));
  }
  assert!(
    !Command::new("/bin/kill")
      .args(["-0", &resident_pid.to_string()])
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .expect("probe daemon-owned Runner after shutdown")
      .success(),
    "a Runner must exit when its daemon connection disappears"
  );
}

#[test]
fn devices_pair_enrolls_a_pem_certificate_with_typed_scopes() {
  let directory = tempfile::tempdir().expect("temporary pairing directory");
  let certificate = rcgen::generate_simple_self_signed(vec!["paired-client".to_string()]).expect("generate client certificate");
  let certificate_path = directory.path().join("client.pem");
  let store_path = directory.path().join("pairings.json");
  std::fs::write(&certificate_path, certificate.cert.pem()).expect("write client certificate");

  let added = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "pair",
      "--store",
      store_path.to_str().unwrap(),
      "add",
      "--pair-id",
      "test-device",
      "--label",
      "test device",
      "--certificate",
      certificate_path.to_str().unwrap(),
      "--scope",
      "control-inspect",
      "--scope",
      "operations-execute",
    ])
    .output()
    .expect("enroll paired certificate");
  assert!(added.status.success(), "stderr={}", stderr(&added));
  assert!(stdout(&added).contains("pair id: test-device"));

  let listed = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "pair",
      "--store",
      store_path.to_str().unwrap(),
      "list",
      "--json",
    ])
    .output()
    .expect("list paired certificates");
  assert!(listed.status.success(), "stderr={}", stderr(&listed));
  let document: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("pairing list JSON");
  assert_eq!(document["revision"], 1);
  assert_eq!(document["devices"][0]["pair_id"], "test-device");
  assert_eq!(document["devices"][0]["scopes"][0], "control_inspect");
  assert_eq!(document["devices"][0]["scopes"][1], "operations_execute");
  assert_eq!(document["devices"][0]["credentials"][0]["certificate_fingerprint"].as_str().unwrap().len(), 64);

  let disabled = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "disable",
      "test device",
      "--store",
      store_path.to_str().unwrap(),
    ])
    .output()
    .expect("disable paired Device by unique name");
  assert!(disabled.status.success(), "stderr={}", stderr(&disabled));

  let enabled = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "enable",
      "test-device",
      "--store",
      store_path.to_str().unwrap(),
    ])
    .output()
    .expect("enable paired Device by stable ID");
  assert!(enabled.status.success(), "stderr={}", stderr(&enabled));

  let unpaired = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "unpair",
      "test device",
      "--store",
      store_path.to_str().unwrap(),
    ])
    .output()
    .expect("unpair paired Device by unique name");
  assert!(unpaired.status.success(), "stderr={}", stderr(&unpaired));

  let listed = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "pair",
      "--store",
      store_path.to_str().unwrap(),
      "list",
      "--json",
    ])
    .output()
    .expect("list paired Devices after unpair");
  assert!(listed.status.success(), "stderr={}", stderr(&listed));
  let document: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("pairing list JSON after unpair");
  assert_eq!(document["devices"], serde_json::json!([]));
}

#[test]
fn device_trust_name_requires_a_unique_paired_device() {
  use auv_api_server::authority::{ApiScope, CertificateFingerprint, CredentialState, PairingCredential, PairingRecord, PairingStore};

  let directory = tempfile::tempdir().expect("temporary pairing directory");
  let store_path = directory.path().join("pairings.json");
  let store = PairingStore::open(store_path.clone()).expect("open pairing store");
  for (pair_id, certificate) in [
    ("device_a", b"certificate-a".as_slice()),
    ("device_b", b"certificate-b".as_slice()),
  ] {
    store
      .insert(PairingRecord {
        pair_id: pair_id.to_string(),
        label: "shared name".to_string(),
        enabled: true,
        scopes: vec![ApiScope::ControlInspect],
        credentials: vec![PairingCredential {
          certificate_fingerprint: CertificateFingerprint::from_der(certificate),
          state: CredentialState::Active,
        }],
      })
      .expect("seed paired Device");
  }
  drop(store);

  let ambiguous = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "disable",
      "shared name",
      "--store",
      store_path.to_str().unwrap(),
    ])
    .output()
    .expect("resolve ambiguous paired Device name");
  assert!(!ambiguous.status.success());
  let error = stderr(&ambiguous);
  assert!(error.contains("ambiguous"), "stderr={error}");
  assert!(error.contains("device_a"), "stderr={error}");
  assert!(error.contains("device_b"), "stderr={error}");
}

#[test]
fn devices_list_without_a_discovered_daemon_renders_an_empty_table() {
  let discovery = tempfile::tempdir().expect("temporary discovery directory").path().join("missing.json");
  let plain = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list"])
    .env_remove("AUV_ENDPOINT")
    .env("AUV_DISCOVERY_FILE", &discovery)
    .output()
    .expect("list Devices without daemon discovery");
  assert!(plain.status.success(), "stderr={}", stderr(&plain));
  assert!(plain.stderr.is_empty());
  let plain = stdout(&plain);
  assert!(plain.lines().next().is_some_and(|header| header.contains("DEVICE ID") && header.contains("STATUS")), "{plain}");
  assert!(plain.contains("(no devices)"), "{plain}");

  let json = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "ls", "--json"])
    .env_remove("AUV_ENDPOINT")
    .env("AUV_DISCOVERY_FILE", &discovery)
    .output()
    .expect("list JSON Devices without daemon discovery");
  assert!(json.status.success(), "stderr={}", stderr(&json));
  assert_eq!(stdout(&json), "[]\n");
  assert!(json.stderr.is_empty());
}

#[test]
fn selected_unreachable_endpoint_is_not_reported_as_an_empty_list() {
  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", "http://127.0.0.1:0"])
    .env_remove("AUV_ENDPOINT")
    .output()
    .expect("list against selected unreachable endpoint");
  assert_eq!(output.status.code(), Some(1));
  assert!(stdout(&output).is_empty());
  assert!(stderr(&output).contains("failed to connect to AUV API server"));
}

#[test]
fn nested_builtin_help_is_rendered_by_clap() {
  let output = run(&["api-server", "serve", "--help"]);

  assert_eq!(output.status.code(), Some(0), "api-server help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  assert!(help.contains("Serve the AUV API until interrupted"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--host <HOST>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--port <PORT>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--remote-listen <IP>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--tls-certificate <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--client-ca-certificate <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--pairing-store <PATH>"), "unexpected api-server help:\n{help}");
  #[cfg(unix)]
  assert!(help.contains("--unix-socket <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--store-root <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--daemon-idle-timeout <SECONDS>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--runner-provider <PATH>"), "unexpected api-server help:\n{help}");
}

#[test]
fn remote_server_requires_complete_tls_authority_configuration() {
  let output = run(&[
    "api-server",
    "serve",
    "--remote-listen",
    "127.0.0.1",
    "--no-discovery",
  ]);

  assert_eq!(output.status.code(), Some(1));
  assert!(
    stderr(&output).contains("--remote-listen requires --tls-certificate"),
    "unexpected remote configuration error:\n{}",
    stderr(&output)
  );
}

#[test]
fn remote_server_rejects_credential_free_discovery() {
  let output = run(&[
    "api-server",
    "serve",
    "--remote-listen",
    "127.0.0.1",
    "--tls-certificate",
    "server.pem",
    "--tls-private-key",
    "server-key.pem",
    "--client-ca-certificate",
    "client-ca.pem",
    "--pairing-store",
    "pairings.json",
  ]);

  assert_eq!(output.status.code(), Some(1));
  assert!(stderr(&output).contains("requires --no-discovery"), "unexpected remote discovery error:\n{}", stderr(&output));
}

#[test]
fn invoke_command_help_uses_typed_arguments_and_inline_examples() {
  let output = run(&["invoke", "screen.findText", "--help"]);

  assert_eq!(output.status.code(), Some(0), "invoke help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  assert!(help.contains("Usage: auv invoke screen.findText [OPTIONS] <TEXT>"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Arguments:"), "unexpected invoke help:\n{help}");
  assert!(help.contains("<TEXT>"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Examples:"), "unexpected invoke help:\n{help}");
  assert!(help.contains("auv invoke screen.findText \"Settings\""), "unexpected invoke help:\n{help}");
  assert!(!help.contains("--target com.apple.TextEdit"), "help must not claim unsupported target activation:\n{help}");
}

#[cfg(unix)]
#[test]
fn unknown_top_level_command_executes_matching_auv_plugin() {
  let temp = tempfile::tempdir().expect("create plugin directory");
  let plugin = temp.path().join("auv-fixture");
  std::fs::write(
    &plugin,
    "#!/bin/sh\nprintf 'args=%s|%s\\n' \"$1\" \"$2\"\nprintf 'auv_path=%s\\n' \"$AUV_PATH\"\nprintf 'auv_context=%s\\n' \"$AUV_CONTEXT\"\nprintf 'plugin stderr\\n' >&2\nexit 23\n",
  )
  .expect("write fixture plugin");
  let mut permissions = std::fs::metadata(&plugin).expect("read plugin metadata").permissions();
  permissions.set_mode(0o755);
  std::fs::set_permissions(&plugin, permissions).expect("make plugin executable");

  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["fixture", "child", "--value"])
    .env("PATH", temp.path())
    .env_remove("AUV_ENDPOINT")
    .env("AUV_DISCOVERY_FILE", temp.path().join("missing-daemon.json"))
    .output()
    .expect("run root auv binary");

  assert_eq!(output.status.code(), Some(23));
  let output_text = stdout(&output);
  let mut lines = output_text.lines();
  assert_eq!(lines.next(), Some("args=child|--value"));
  assert_eq!(lines.next(), Some(format!("auv_path={}", env!("CARGO_BIN_EXE_auv")).as_str()));
  let context = lines.next().expect("injected AUV_CONTEXT").strip_prefix("auv_context=").expect("context prefix");
  let context: serde_json::Value = serde_json::from_str(context).expect("inline context JSON");
  assert!(context["invocation_id"].as_str().is_some_and(|value| value.starts_with("invocation_")));
  assert!(context.get("version").is_none());
  assert!(context.get("credential").is_none());
  assert_eq!(lines.next(), None);
  assert_eq!(stderr(&output), "plugin stderr\n");
}

#[cfg(unix)]
#[test]
fn root_device_and_run_flags_resolve_into_plugin_context() {
  let directory = tempfile::tempdir().expect("temporary context directory");
  let socket = directory.path().join("auv.sock");
  let store = directory.path().join("store");
  let discovery = directory.path().join("daemon.json");
  let endpoint = format!("unix://{}", socket.display());
  let child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--listen",
      &endpoint,
      "--store-root",
      store.to_str().unwrap(),
      "--discovery-file",
      discovery.to_str().unwrap(),
    ])
    .env("HOSTNAME", "fixture-device")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("start local daemon");
  let mut daemon = ChildGuard(child);
  wait_for_path(&mut daemon.0, &socket);

  let devices =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["devices", "list", "--endpoint", &endpoint, "--json"]).output().expect("list Devices");
  assert!(devices.status.success(), "stderr={}", stderr(&devices));
  let devices: serde_json::Value = serde_json::from_slice(&devices.stdout).expect("Device JSON");
  let device_id = devices[0]["device_id"].as_str().expect("Device ID");

  let created_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "run",
      "create",
      "--endpoint",
      &endpoint,
      "--device-id",
      device_id,
      "--json",
    ])
    .output()
    .expect("create Run");
  assert!(created_run.status.success(), "stderr={}", stderr(&created_run));
  let created_run: serde_json::Value = serde_json::from_slice(&created_run.stdout).expect("Run JSON");
  let run_id = created_run["run_id"].as_str().expect("Run ID");

  let plugin = directory.path().join("auv-fixture-context");
  std::fs::write(&plugin, "#!/bin/sh\nprintf '%s\\n' \"$AUV_CONTEXT\"\n").expect("write fixture plugin");
  let mut permissions = std::fs::metadata(&plugin).expect("read plugin metadata").permissions();
  permissions.set_mode(0o755);
  std::fs::set_permissions(&plugin, permissions).expect("make plugin executable");

  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "--device",
      "fixture-device",
      "--device-id",
      device_id,
      "--run",
      run_id,
      "fixture-context",
      "plugin-owned-argument",
    ])
    .env("PATH", directory.path())
    .env("AUV_ENDPOINT", &endpoint)
    .output()
    .expect("run contextual plugin");
  assert!(output.status.success(), "stderr={}", stderr(&output));
  let context: serde_json::Value = serde_json::from_slice(&output.stdout).expect("plugin context JSON");
  assert_eq!(context["device_id"], device_id);
  assert_eq!(context["device_name"], "fixture-device");
  assert_eq!(context["run_id"], run_id);
  assert_eq!(context["daemon_endpoint"], endpoint);
  assert!(context.get("version").is_none());

  let implicit = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "--device-id",
      device_id,
      "fixture-context",
      "plugin-owned-argument",
    ])
    .env("PATH", directory.path())
    .env("AUV_ENDPOINT", &endpoint)
    .output()
    .expect("run plugin with an implicit Run");
  assert!(implicit.status.success(), "stderr={}", stderr(&implicit));
  let implicit_context: serde_json::Value = serde_json::from_slice(&implicit.stdout).expect("implicit plugin context JSON");
  let implicit_run_id = implicit_context["run_id"].as_str().expect("implicit Run ID");
  let implicit_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "run",
      "get",
      implicit_run_id,
      "--endpoint",
      &endpoint,
      "--json",
    ])
    .output()
    .expect("read terminal implicit Run");
  assert!(implicit_run.status.success(), "stderr={}", stderr(&implicit_run));
  let implicit_run: serde_json::Value = serde_json::from_slice(&implicit_run.stdout).expect("implicit Run JSON");
  assert_eq!(implicit_run["phase"], "RUN_PHASE_SUCCEEDED");

  interrupt(&daemon.0);
  daemon.0.wait().expect("wait for local daemon");
}

#[cfg(unix)]
#[test]
fn plugin_list_reports_path_order_shadowing_and_builtin_collisions() {
  let first = tempfile::tempdir().expect("create first plugin directory");
  let second = tempfile::tempdir().expect("create second plugin directory");
  for path in [
    first.path().join("auv-demo"),
    second.path().join("auv-demo"),
    second.path().join("auv-invoke"),
  ] {
    std::fs::write(&path, "#!/bin/sh\n").expect("write fixture plugin");
    let mut permissions = std::fs::metadata(&path).expect("read plugin metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("make plugin executable");
  }
  let path = std::env::join_paths([first.path(), second.path()]).expect("join fixture PATH");

  let output = Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", path).output().expect("list plugins");

  assert_eq!(output.status.code(), Some(1), "plugin warnings must produce a failing status");
  assert!(stdout(&output).contains(&first.path().join("auv-demo").display().to_string()));
  let diagnostics = stderr(&output);
  assert!(diagnostics.contains("shadowed"), "missing shadow warning:\n{diagnostics}");
  assert!(diagnostics.contains("collides with built-in command `invoke`"), "missing collision warning:\n{diagnostics}");
}

#[cfg(unix)]
#[test]
fn builtins_take_precedence_and_compound_plugin_names_are_not_probed() {
  let temp = tempfile::tempdir().expect("create plugin directory");
  for name in ["auv-doctor", "auv-demo-child"] {
    let path = temp.path().join(name);
    std::fs::write(&path, "#!/bin/sh\nprintf 'plugin-ran\\n'\n").expect("write fixture plugin");
    let mut permissions = std::fs::metadata(&path).expect("read plugin metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("make plugin executable");
  }

  let builtin = Command::new(env!("CARGO_BIN_EXE_auv")).args(["doctor", "--help"]).env("PATH", temp.path()).output().expect("run builtin");
  assert!(builtin.status.success());
  assert!(!stdout(&builtin).contains("plugin-ran"));

  let compound = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["demo", "child"])
    .env("PATH", temp.path())
    .output()
    .expect("run missing single-name plugin");
  assert!(!compound.status.success());
  assert!(stderr(&compound).contains("auv-demo"));
  assert!(!stdout(&compound).contains("plugin-ran"));
}

#[cfg(unix)]
#[test]
fn plugin_list_reports_non_executable_candidates_and_empty_paths() {
  let empty = tempfile::tempdir().expect("create empty PATH directory");
  let empty_output =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", empty.path()).output().expect("list empty plugin path");
  assert!(empty_output.status.success());
  assert!(stdout(&empty_output).contains("No AUV plugins were found"));

  let candidate = empty.path().join("auv-disabled");
  std::fs::write(&candidate, "#!/bin/sh\n").expect("write non-executable candidate");
  let warning_output =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", empty.path()).output().expect("list non-executable plugin");
  assert!(!warning_output.status.success());
  assert!(stderr(&warning_output).contains("not executable"));
}

#[test]
fn typed_invoke_values_are_rejected_before_execution() {
  let output = run(&[
    "invoke",
    "screen.captureRegion",
    "--x",
    "not-a-number",
    "--y",
    "0",
    "--width",
    "10",
    "--height",
    "10",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stderr(&output).contains("invalid value 'not-a-number'"), "unexpected diagnostic:\n{}", stderr(&output));
}

#[test]
fn typed_invoke_ranges_are_rejected_by_the_handler() {
  let output = run(&[
    "invoke",
    "input.clickWindowPoint",
    "--relative-x",
    "2",
    "--relative-y",
    "0.5",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stdout(&output).contains("within 0..=1"), "unexpected diagnostic:\n{}", stdout(&output));
}

#[test]
fn invoke_store_root_cannot_consume_the_next_flag() {
  let output = run(&[
    "invoke",
    "scan.coverage",
    "--fixture-dir",
    "unused",
    "--store-root",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stderr(&output).contains("--store-root <PATH>"), "unexpected diagnostic:\n{}", stderr(&output));
}
