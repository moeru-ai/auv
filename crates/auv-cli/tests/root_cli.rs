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
fn device_help_explains_the_two_machine_pairing_workflow() {
  let devices = run(&["devices", "--help"]);
  assert_eq!(devices.status.code(), Some(0), "devices help must exit 0; stderr={}", stderr(&devices));
  let devices = stdout(&devices);
  assert!(devices.contains("Devices are AUV execution targets"), "unexpected devices help:\n{devices}");
  assert!(devices.contains("Examples:"), "unexpected devices help:\n{devices}");
  assert!(devices.contains("auv devices pair --help"), "unexpected devices help:\n{devices}");

  let pair = run(&["devices", "pair", "--help"]);
  assert_eq!(pair.status.code(), Some(0), "pair help must exit 0; stderr={}", stderr(&pair));
  let pair = stdout(&pair);
  assert!(pair.contains("Pairing is a two-machine enrollment flow"), "unexpected pair help:\n{pair}");
  assert!(pair.contains("On the daemon host:"), "unexpected pair help:\n{pair}");
  assert!(pair.contains("auv devices pair create-token"), "unexpected pair help:\n{pair}");
  assert!(pair.contains("On the client machine:"), "unexpected pair help:\n{pair}");
  assert!(pair.contains("connect --token <TOKEN>"), "unexpected pair help:\n{pair}");
  assert!(pair.contains("auv --device <NAME> invoke display.list"), "unexpected pair help:\n{pair}");
}

#[test]
fn invoke_index_presents_registered_operations_as_commands() {
  let output = run(&["invoke", "--help"]);
  assert_eq!(output.status.code(), Some(0), "invoke help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  assert!(help.contains("Invoke typed computer-use operations"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Examples:"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Usage:\n  auv invoke <COMMAND> [OPTIONS]"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Commands:"), "unexpected invoke help:\n{help}");
  assert!(help.lines().any(|line| line.trim_start().starts_with("display.list ")), "unexpected invoke help:\n{help}");
  assert!(help.lines().any(|line| line.trim_start().starts_with("help ")), "unexpected invoke help:\n{help}");
  assert!(help.contains("Options:"), "unexpected invoke help:\n{help}");
  assert!(!help.contains("\nDISPLAY\n"), "invoke operations should not look like internal capability sections:\n{help}");
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
  let profiles = directory.path().join("profiles.json");
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
  assert_eq!(classes.len(), 1, "auv serve must only bundle the local Driver RunnerClass: {classes:?}");
  let local = classes.iter().find(|class| class["runner_class"] == "auv.core.local").expect("local Driver RunnerClass");
  assert!(local.get("capabilities").is_none(), "RunnerClass must not publish a daemon-owned capability manifest");

  let classes_table =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["runner", "classes", "--endpoint", &endpoint]).output().expect("render RunnerClass table");
  assert!(classes_table.status.success(), "stderr={}", stderr(&classes_table));
  let classes_table = stdout(&classes_table);
  assert!(classes_table.lines().next().is_some_and(|header| header.contains("CLASS") && header.contains("LIFECYCLES")), "{classes_table}");
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
  assert_eq!(run_id.len(), 32);
  assert!(run_id.bytes().all(|byte| byte.is_ascii_hexdigit()));
  assert_eq!(created_run["phase"], "RUN_PHASE_RUNNING");
  assert_eq!(created_run["device_ids"].as_array().map(Vec::len), Some(1), "explicit Run must resolve to exactly one Device: {created_run}");

  let fetched_run = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "run",
      "get",
      &run_id[..12],
      "--endpoint",
      &endpoint,
      "--json",
    ])
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
  assert!(fetched_run_table.contains(&run_id[..12]), "{fetched_run_table}");
  wait_for_path(&mut daemon.0, &discovery);

  let listed = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", &endpoint, "--json"])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .output()
    .expect("list daemon Devices");
  assert!(listed.status.success(), "stderr={}", stderr(&listed));
  let devices: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("Device JSON");
  assert_eq!(devices.as_array().map(Vec::len), Some(1), "daemon must expose exactly one local Device: {devices}");
  assert_eq!(devices[0]["local"], true);
  assert!(devices[0]["device_id"].as_str().is_some_and(|id| id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit())));
  let listed_table = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", &endpoint])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .output()
    .expect("render Device table");
  assert!(listed_table.status.success(), "stderr={}", stderr(&listed_table));
  let listed_table = stdout(&listed_table);
  assert!(listed_table.lines().next().is_some_and(|header| header.contains("DEVICE ID") && header.contains("STATUS")), "{listed_table}");
  // ROOT CAUSE:
  //
  // On Linux, this cross-platform test failed because it expected the macOS
  // platform label even though the daemon correctly reported `linux`.
  //
  // Before the fix, only the macOS CI runner could satisfy the assertion. The
  // fix checks the platform of the binary that started the local daemon.
  assert!(listed_table.contains(std::env::consts::OS), "{listed_table}");

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
fn local_daemon_routes_runner_grpc_without_claims_or_leases() {
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

  tokio::runtime::Runtime::new().expect("test runtime").block_on(async {
    let client =
      auv_api_client::protocol::grpc::Client::connect(endpoint.parse().expect("daemon endpoint")).await.expect("connect API client");
    let transport = client
      .routed_transport(auv_api_client::RunnerRoute {
        device_id: None,
        run_id: None,
        runner_class: "auv.core.local".to_string(),
      })
      .expect("route transport");
    // ROOT CAUSE:
    //
    // In a headless Linux runner, a routed DisplayService request failed after
    // routing succeeded because no Wayland compositor was available.
    //
    // Before the fix, this routing test also required a graphical session. The
    // fix verifies the same opaque runner transport through its health service.
    let health = tonic_health::pb::health_client::HealthClient::new(transport)
      .check(tonic_health::pb::HealthCheckRequest {
        service: String::new(),
      })
      .await
      .expect("opaque routed health call")
      .into_inner();
    assert_eq!(health.status, tonic_health::pb::health_check_response::ServingStatus::Serving as i32);
  });

  let deadline = Instant::now() + Duration::from_secs(2);
  loop {
    let listed =
      Command::new(env!("CARGO_BIN_EXE_auv")).args(["runner", "list", "--endpoint", &endpoint, "--json"]).output().expect("list Runners");
    assert!(listed.status.success(), "stderr={}", stderr(&listed));
    let runners: serde_json::Value = serde_json::from_slice(&listed.stdout).expect("Runner list JSON");
    if runners.as_array().is_some_and(Vec::is_empty) {
      break;
    }
    assert!(Instant::now() < deadline, "ephemeral route-created Runner did not stop after the RPC body completed");
    std::thread::sleep(Duration::from_millis(25));
  }

  interrupt(&daemon.0);
  daemon.0.wait().expect("wait for local daemon");
}

// https://github.com/moeru-ai/auv/actions/runs/31052479884/job/92462591348
// ROOT CAUSE:
//
// On Windows, this pairing test cannot construct its required caller-local
// listener because that trust boundary is currently a Unix-domain socket.
//
// Before the fix, Windows attempted to parse the Unix endpoint. The fix limits
// this topology test to platforms that implement its local transport.
// TODO(windows-local-pairing-test): Add Windows coverage when an owner-approved
// local authenticated transport can create pairing tokens beside remote TCP.
#[cfg(unix)]
#[test]
fn device_trust_name_requires_a_unique_paired_device() {
  let directory = tempfile::tempdir().expect("temporary pairing directory");
  let store_path = directory.path().join("pairings.json");
  let socket = directory.path().join("auv.sock");
  let discovery = directory.path().join("daemon.json");
  let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
  let mut daemon = ChildGuard(
    Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "serve",
        "--listen",
        &format!("unix://{}", socket.display()),
        "--listen",
        &format!("http://127.0.0.1:{port}"),
        "--pairing-store",
        store_path.to_str().unwrap(),
        "--store-root",
        directory.path().join("store").to_str().unwrap(),
        "--discovery-file",
        discovery.to_str().unwrap(),
      ])
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::inherit())
      .spawn()
      .expect("start paired daemon"),
  );
  wait_for_path(&mut daemon.0, &discovery);

  for (pair_id, profile) in [("device_a", "a"), ("device_b", "b")] {
    let created = Command::new(env!("CARGO_BIN_EXE_auv"))
      .args(["devices", "pair", "create-token"])
      .env("AUV_DISCOVERY_FILE", &discovery)
      .output()
      .expect("create pairing token");
    assert!(created.status.success(), "stderr={}", stderr(&created));
    let token = stdout(&created).trim().to_string();
    let connected = Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "devices",
        "pair",
        "--endpoint",
        &format!("http://127.0.0.1:{port}"),
        "connect",
        "--token",
        &token,
        "--device-id",
        pair_id,
        "--label",
        "shared name",
        "--profile",
        profile,
      ])
      .env("AUV_CONFIG_PROFILES_FILE", directory.path().join(format!("profiles-{profile}.json")))
      .output()
      .expect("seed paired Device");
    assert!(connected.status.success(), "stderr={}", stderr(&connected));
  }

  let ambiguous = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "disable", "shared name"])
    .env("AUV_DISCOVERY_FILE", &discovery)
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
  let directory = tempfile::tempdir().expect("temporary discovery directory");
  let discovery = directory.path().join("missing.json");
  let profiles = directory.path().join("missing-profiles.json");
  let plain = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list"])
    .env_remove("AUV_ENDPOINT")
    .env("AUV_DISCOVERY_FILE", &discovery)
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
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
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
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
  assert!(help.contains("--pairing-store <PATH>"), "unexpected api-server help:\n{help}");
  #[cfg(unix)]
  assert!(help.contains("--unix-socket <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--store-root <PATH>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--daemon-idle-timeout <SECONDS>"), "unexpected api-server help:\n{help}");
  assert!(help.contains("--runner-provider <PATH>"), "unexpected api-server help:\n{help}");
}

#[test]
fn remote_server_requires_a_pairing_store() {
  let output = run(&[
    "api-server",
    "serve",
    "--remote-listen",
    "127.0.0.1",
    "--no-discovery",
  ]);

  assert_eq!(output.status.code(), Some(1));
  assert!(
    stderr(&output).contains("--remote-listen requires --pairing-store"),
    "unexpected remote configuration error:\n{}",
    stderr(&output)
  );
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
    "#!/bin/sh\nprintf 'args=%s|%s\\n' \"$1\" \"$2\"\nprintf 'auv_path=%s\\n' \"$AUV_PATH\"\nprintf 'auv_context=%s\\n' \"$AUV_CONTEXT\"\nprintf 'trace_store=%s\\n' \"$AUV_TRACING_STORE_ROOT\"\nprintf 'plugin stderr\\n' >&2\nexit 23\n",
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
  assert_eq!(
    lines.next(),
    Some(format!("trace_store={}", std::env::current_dir().expect("current directory").join(".auv/store").display()).as_str())
  );
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
  let profiles = directory.path().join("device-profiles.json");
  std::fs::write(&profiles, br#"{"profiles":{}}"#).expect("isolated Device profiles");
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

  let devices = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", &endpoint, "--json"])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .output()
    .expect("list Devices");
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
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
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
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
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
fn screen_point_click_dry_run_reports_the_validated_coordinate() {
  let output = run(&[
    "invoke",
    "input.clickScreenPoint",
    "1032.5",
    "1212",
    "--dry-run",
    "--json",
  ]);

  assert!(output.status.success(), "unexpected diagnostic:\n{}", stderr(&output));
  let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON output");
  assert_eq!(value["result"]["point"]["x"], 1032.5);
  assert_eq!(value["result"]["point"]["y"], 1212.0);
  assert!(value["result"]["action"].is_null());
}

#[test]
fn mouse_move_dry_run_reports_the_validated_coordinate() {
  let output = run(&[
    "invoke",
    "input.moveMouse",
    "1032.5",
    "1212",
    "--dry-run",
    "--json",
  ]);

  assert!(output.status.success(), "unexpected diagnostic:\n{}", stderr(&output));
  let value: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON output");
  assert_eq!(value["result"]["point"]["x"], 1032.5);
  assert_eq!(value["result"]["point"]["y"], 1212.0);
  assert!(value["result"]["action"].is_null());
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
