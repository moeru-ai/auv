//! Owned application workflows, not an Agent benchmark. Application replies and painted pixels
//! are the independent oracle. Foreground guards are checked immediately before dispatch; they
//! are not an atomic OS focus lease. No failed or uncertain input action is retried.

use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Output, Stdio};

use auv_driver_common::capture::CaptureOptions;
use auv_driver_common::geometry::RelativeRect;
use auv_driver_common::input::{InputPolicy, KeyPressOptions, Scroll, ScrollOptions, TypeTextOptions, WindowInput};
use auv_driver_common::window::Window;
use auv_driver_common::{Click, MouseButton};
use auv_driver_windows::WindowsDriverSession;

use super::super::watch::{Snapshot, Verdict};
use super::*;

pub struct Options {
  pub output: PathBuf,
  pub auv: PathBuf,
  pub rounds: u32,
  /// Resolution, capture, OCR and Runner lifecycle only; not full workflow acceptance.
  pub observe_only: bool,
}

fn physical_input_changed(start: &Snapshot, current: &Snapshot) -> bool {
  (start.physical_mouse, start.physical_keys) != (current.physical_mouse, current.physical_keys)
}

/// A workflow must stop issuing input once the witness sees the user return.
fn reject_user_input(start: &Snapshot, current: &Snapshot) -> Result<()> {
  if physical_input_changed(start, current) {
    bail!("physical user input interrupted the workflow; no further input will be sent");
  }
  Ok(())
}

/// Foreground actions may inject input, but neither mode may accept user activity.
fn quiet_verified(tally: &Tally, observe_only: bool, phase_verdict: &Verdict, start: &Snapshot, finish: &Snapshot) -> bool {
  let no_global_input = finish.injected_mouse == start.injected_mouse && finish.injected_keys == start.injected_keys;
  tally.violated == 0
    && tally.inconclusive == 0
    && !physical_input_changed(start, finish)
    && (!observe_only || (matches!(phase_verdict, Verdict::Held) && no_global_input))
}

fn runner_cleanup_verified(receipt: &Value) -> bool {
  receipt["run_stop_exit"] == 0
    && receipt["owned_tree_kill_exit"] == 0
    && receipt["fallback_root_kill"] == false
    && receipt["daemon_exited"] == true
}

/// CLI calls are external process/I/O boundaries. A disconnected daemon must not hang the
/// evaluation or its cleanup indefinitely. Tokio kills the owned CLI child on timeout.
fn command_output(command: Command, limit: Duration) -> Result<Output> {
  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
  runtime.block_on(async {
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    tokio::time::timeout(limit, command.output())
      .await
      .context("workflow CLI deadline exceeded; child termination requested")?
      .context("workflow CLI process failed")
  })
}

/// Only this daemon's owner pipe, discovery file, profiles and process tree are used. The user's
/// default daemon, profiles and paired Devices are neither selected nor stopped.
struct Runner {
  child: Child,
  auv: PathBuf,
  root: PathBuf,
  endpoint: String,
  run: String,
  evidence: PathBuf,
}

struct DaemonTermination {
  exit: ExitStatus,
  tree_kill_exit: Option<i32>,
  tree_kill_error: Option<String>,
  fallback_root_kill: bool,
}

impl Runner {
  /// Bounds the external tree kill and process wait. A root-only fallback never proves that
  /// descendants exited, so the taskkill outcome remains in the receipt.
  fn terminate(&mut self) -> Result<DaemonTermination> {
    if let Some(exit) = self.child.try_wait()? {
      return Ok(DaemonTermination {
        exit,
        tree_kill_exit: None,
        tree_kill_error: None,
        fallback_root_kill: false,
      });
    }
    let mut command = Command::new("taskkill.exe");
    command
      .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .creation_flags(0x0800_0000);
    let killed = command_output(command, Duration::from_secs(3));
    let tree_kill_exit = killed.as_ref().ok().and_then(|output| output.status.code());
    let tree_kill_error = killed.as_ref().err().map(ToString::to_string);
    let mut fallback_root_kill = false;
    if !killed.as_ref().is_ok_and(|output| output.status.success()) && self.child.try_wait()?.is_none() {
      self.child.kill().context("terminate the workflow daemon after taskkill failed")?;
      fallback_root_kill = true;
    }
    let started = Instant::now();
    loop {
      if let Some(exit) = self.child.try_wait()? {
        return Ok(DaemonTermination {
          exit,
          tree_kill_exit,
          tree_kill_error,
          fallback_root_kill,
        });
      }
      if started.elapsed() >= Duration::from_secs(3) {
        bail!("workflow daemon did not exit within the cleanup deadline");
      }
      if !fallback_root_kill && started.elapsed() >= Duration::from_secs(1) {
        self.child.kill().context("terminate the workflow daemon after taskkill reported success without an exit")?;
        fallback_root_kill = true;
      }
      std::thread::sleep(Duration::from_millis(20));
    }
  }

  fn finish(&mut self) -> Result<Value> {
    let mut command = self.command();
    // NOTICE: the Run is always stopped as `canceled` because this harness-owned Run is torn down
    // regardless of workflow success; the workflow verdict lives in workflow.jsonl's summary
    // (`functional_passed`/`observation_passed`), not in the Run outcome.
    command.args([
      "run",
      "stop",
      &self.run,
      "--outcome",
      "canceled",
      "--endpoint",
      &self.endpoint,
      "--json",
    ]);
    let stopped = command_output(command, Duration::from_secs(10));
    let terminated = self.terminate();
    self.keep_logs();
    let terminated = terminated?;
    Ok(json!({"daemon_pid": self.child.id(), "run_stop_exit": stopped.as_ref().ok().and_then(|output| output.status.code()),
      "run_stop_error": stopped.as_ref().err().map(ToString::to_string),
      "stopped_run": stopped.as_ref().ok().and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok()),
      "owned_tree_kill_exit": terminated.tree_kill_exit, "owned_tree_kill_error": terminated.tree_kill_error,
      "fallback_root_kill": terminated.fallback_root_kill, "daemon_exit": terminated.exit.code(), "daemon_exited": true}))
  }

  fn keep_logs(&self) {
    for name in ["serve.stdout.log", "serve.stderr.log"] {
      let _ = std::fs::copy(self.root.join(name), self.evidence.join(name));
    }
  }

  fn command(&self) -> Command {
    let mut command = Command::new(&self.auv);
    command
      .env("AUV_DISCOVERY_FILE", self.root.join("daemon.json"))
      .env("AUV_ENDPOINT", &self.endpoint)
      .env("AUV_CONFIG_PROFILES_FILE", self.root.join("profiles.json"))
      .stdin(Stdio::null())
      .creation_flags(0x0800_0000);
    command
  }

  /// Starts the private daemon and creates the harness's own Run on it. A failure after the
  /// daemon process was spawned returns it in `RunnerStartFailure::started`, because a
  /// half-started daemon must be stopped with a receipt, not recorded as never created.
  fn start(auv: &Path, root: &Path, evidence: &Path) -> Result<Self, RunnerStartFailure> {
    std::fs::create_dir_all(root).map_err(|error| RunnerStartFailure {
      error: error.into(),
      started: None,
    })?;
    std::fs::write(root.join("profiles.json"), "{\"profiles\":{}}").map_err(|error| RunnerStartFailure {
      error: error.into(),
      started: None,
    })?;
    // The daemon binds its local listener from a bare Windows pipe name; clients dial the same
    // pipe through the npipe:// URI form the endpoint parser accepts.
    let pipe = format!("auv-gui-workflow-{}-{}", std::process::id(), run_id());
    let endpoint = format!("npipe://./pipe/{pipe}");
    let stdout = std::fs::File::create(root.join("serve.stdout.log")).map_err(|error| RunnerStartFailure {
      error: error.into(),
      started: None,
    })?;
    let stderr = std::fs::File::create(root.join("serve.stderr.log")).map_err(|error| RunnerStartFailure {
      error: error.into(),
      started: None,
    })?;
    let child = match Command::new(auv)
      .args(["serve", "--local-socket", &pipe, "--store-root"])
      .arg(root.join("server"))
      .arg("--discovery-file")
      .arg(root.join("daemon.json"))
      .env("AUV_CONFIG_PROFILES_FILE", root.join("profiles.json"))
      .stdin(Stdio::null())
      .stdout(stdout)
      .stderr(stderr)
      .creation_flags(0x0800_0000)
      .spawn()
    {
      Ok(child) => child,
      Err(error) => {
        return Err(RunnerStartFailure {
          error: error.into(),
          started: None,
        });
      }
    };
    let mut scope = Self {
      child,
      auv: auv.into(),
      root: root.into(),
      endpoint,
      run: String::new(),
      evidence: evidence.into(),
    };
    let started = Instant::now();
    loop {
      match scope.child.try_wait() {
        Ok(Some(_)) => {
          return Err(RunnerStartFailure {
            error: anyhow::anyhow!("the private workflow daemon exited; see its serve logs"),
            started: Some(scope),
          });
        }
        Ok(None) => {}
        Err(error) => {
          return Err(RunnerStartFailure {
            error: error.into(),
            started: Some(scope),
          });
        }
      }
      if root.join("daemon.json").exists() {
        let mut command = scope.command();
        command.args(["run", "create", "--endpoint", &scope.endpoint, "--json"]);
        let remaining = Duration::from_secs(30).saturating_sub(started.elapsed());
        let output = match command_output(command, remaining.min(Duration::from_secs(5))) {
          Ok(output) => output,
          Err(error) => {
            return Err(RunnerStartFailure {
              error,
              started: Some(scope),
            });
          }
        };
        if output.status.success() {
          let created: Value = match serde_json::from_slice(&output.stdout) {
            Ok(created) => created,
            Err(error) => {
              return Err(RunnerStartFailure {
                error: error.into(),
                started: Some(scope),
              });
            }
          };
          scope.run = match created["run_id"].as_str() {
            Some(run_id) => run_id.into(),
            None => {
              return Err(RunnerStartFailure {
                error: anyhow::anyhow!("create Run returned no run_id"),
                started: Some(scope),
              });
            }
          };
          let encoded = match serde_json::to_vec_pretty(&created) {
            Ok(encoded) => encoded,
            Err(error) => {
              return Err(RunnerStartFailure {
                error: error.into(),
                started: Some(scope),
              });
            }
          };
          if let Err(error) = std::fs::write(root.join("run-created.json"), encoded) {
            return Err(RunnerStartFailure {
              error: error.into(),
              started: Some(scope),
            });
          }
          return Ok(scope);
        }
      }
      if started.elapsed() > Duration::from_secs(30) {
        return Err(RunnerStartFailure {
          error: anyhow::anyhow!("private Runner startup exceeded 30 seconds"),
          started: Some(scope),
        });
      }
      std::thread::sleep(Duration::from_millis(200));
    }
  }
}

impl Drop for Runner {
  fn drop(&mut self) {
    if self.child.try_wait().ok().flatten().is_some() {
      return;
    }
    // Never retry a potentially disconnected network RPC from Drop. Normal cleanup records a
    // bounded Run stop first; the error/unwind fallback only ends this owned process tree.
    // End only the process tree we spawned, including an idle Runner. Stopping a Run alone releases
    // its attachment but does not guarantee termination of every child process.
    let _ = self.terminate();
    self.keep_logs();
  }
}

struct Entry<'a> {
  route: &'static str,
  auv: &'a Path,
  store: &'a Path,
  runner: Option<&'a Runner>,
  driver: &'a WindowsDriverSession,
  window: &'a Window,
  layout: &'a Layout,
  title: &'a str,
  start: &'a Snapshot,
}

impl Entry<'_> {
  fn call(&self, command: &str, args: &[String]) -> Result<Invocation> {
    let started = Instant::now();
    if self.route == "rust" {
      let result = self.native(command, args)?;
      return Ok(Invocation {
        exit: Some(0),
        ms: started.elapsed().as_secs_f64() * 1000.0,
        json: Some(json!({"status": "completed", "result": result})),
      });
    }
    let mut process = if let Some(runner) = self.runner {
      let mut process = runner.command();
      process.args(["--run", &runner.run]);
      process
    } else {
      let mut process = Command::new(self.auv);
      process.stdin(Stdio::null()).creation_flags(0x0800_0000);
      process
    };
    process.args(["invoke", command]).args(args).args(["--json", "--no-overlay", "--store-root"]).arg(self.store);
    let output = command_output(process, Duration::from_secs(30))?;
    Ok(Invocation {
      exit: output.status.code(),
      ms: started.elapsed().as_secs_f64() * 1000.0,
      json: Some(serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        json!({"status": "failed",
          "failure": String::from_utf8_lossy(&output.stderr).chars().take(4096).collect::<String>()})
      })),
    })
  }

  fn native(&self, command: &str, args: &[String]) -> Result<Value> {
    match command {
      "window.capture" => {
        let capture = self.driver.window().capture(self.window).map_err(anyhow::Error::msg)?;
        Ok(json!({"decoded_strip": decode_strip(&capture, &self.layout.strip), "window": self.window,
          "pixels": [capture.image.width(), capture.image.height()], "backend": capture.backend}))
      }
      "screen.findText" | "screen.clickText" => {
        let capture = self.driver.display().capture(CaptureOptions::default()).map_err(anyhow::Error::msg)?;
        let matches = self
          .driver
          .vision()
          .find_text_in_capture(&capture.capture, &args[0], RelativeRect::new(0.0, 0.0, 1.0, 1.0))
          .map_err(anyhow::Error::msg)?;
        if command == "screen.findText" {
          return Ok(json!({"matches": matches.matches}));
        }
        let point = matches.best_match().context("no OCR match; no click sent")?.action_point();
        if command == "screen.clickText" {
          ensure_workflow_click_bounds(args, &point)?;
        }
        let action =
          self.driver.input().click_at(point, MouseButton::Left, Click::Single, Default::default()).map_err(anyhow::Error::msg)?;
        Ok(json!({"matches": matches, "point": point, "action": action}))
      }
      "input.key" => {
        let action = self
          .driver
          .input()
          .press_key(KeyPressOptions {
            key: args[0].clone(),
            ..Default::default()
          })
          .map_err(anyhow::Error::msg)?;
        Ok(json!({"action": action}))
      }
      "input.typeText" => {
        let action = self
          .driver
          .input()
          .type_text(
            &args[0],
            TypeTextOptions {
              policy: InputPolicy::ForegroundPreferred,
              ..Default::default()
            },
          )
          .map_err(anyhow::Error::msg)?;
        Ok(json!({"action": action}))
      }
      "input.scroll" => {
        let local = point_in_window(self.window, self.layout.edit_center);
        let action = self
          .driver
          .window()
          .scroll(
            self.window,
            local,
            Scroll::new(0.0, 360.0),
            ScrollOptions {
              policy: InputPolicy::BackgroundOnly,
              settle: Duration::from_millis(100),
              ..Default::default()
            },
          )
          .map_err(anyhow::Error::msg)?;
        Ok(json!({"action": action}))
      }
      _ => bail!("workflow has no direct Rust implementation for {command}"),
    }
  }
}

/// Verify the direct Rust route uses the same freshly selected, bounded OCR point as
/// the CLI and Runner routes. The allowed rectangle is in logical screen coordinates.
fn ensure_workflow_click_bounds(args: &[String], point: &auv_driver_common::geometry::Point) -> Result<()> {
  let index = args.iter().position(|arg| arg == "--within").context("workflow screen.clickText omitted its required --within fence")?;
  let raw = args.get(index + 1).context("workflow screen.clickText omitted the --within value")?;
  let coordinates = raw.split(',').map(|part| part.trim().parse::<f64>()).collect::<std::result::Result<Vec<_>, _>>()?;
  if coordinates.len() != 4
    || coordinates.iter().any(|value| !value.is_finite())
    || coordinates[2] <= coordinates[0]
    || coordinates[3] <= coordinates[1]
  {
    bail!("workflow click has an invalid --within rectangle");
  }
  if !point.x.is_finite()
    || !point.y.is_finite()
    || point.x < coordinates[0]
    || point.y < coordinates[1]
    || point.x >= coordinates[2]
    || point.y >= coordinates[3]
  {
    bail!("workflow screen.clickText re-resolved outside the owned control; no input delivered");
  }
  Ok(())
}

fn call(scenario: &mut Scenario<'_>, entry: &Entry<'_>, command: &str, args: &[String], quiet: bool) -> Result<Invocation> {
  reject_user_input(entry.start, &scenario.watch.snapshot())?;
  let measured = scenario.desk.measure(|| entry.call(command, args), || std::thread::sleep(Duration::from_millis(150))).0;
  let facts = invariants(&measured, scenario.hwnd);
  if quiet {
    scenario.tally.record(&measured);
  } else {
    let strayed = facts["foreground_event_log"]
      .as_array()
      .is_some_and(|events| events.iter().any(|event| event["became_foreground"] == true && event["owner"] == "other process"));
    scenario.tally.expect(!strayed, format!("{command}: input changed the foreground to another application"));
  }
  let latency = measured.latency_ms;
  let result = measured.value;
  scenario.records.write(
    "op",
    json!({"route": entry.route, "command": command, "args": args, "ms": latency,
    "exit": result.as_ref().ok().and_then(|run| run.exit), "error": result.as_ref().err().map(ToString::to_string),
    "selected_path": result.as_ref().ok().map(selected_path), "failure": result.as_ref().ok().map(Invocation::failure),
    "run_id": result.as_ref().ok().and_then(|run| run.json.as_ref().map(|value| value["run_id"].clone())),
    "quiet_required": quiet, "watch": facts}),
  )?;
  reject_user_input(entry.start, &scenario.watch.snapshot())?;
  let run = result?;
  if !run.completed() {
    bail!("{command} failed (exit {:?}): {}", run.exit, run.failure());
  }
  Ok(run)
}

fn find_text(
  scenario: &mut Scenario<'_>,
  entry: &Entry<'_>,
  query: &str,
  rect: [i32; 4],
) -> Result<(auv_driver_common::geometry::Point, [i32; 2])> {
  let mut point = None;
  for attempt in 1..=3 {
    let found = call(scenario, entry, "screen.findText", &[query.into()], true)?;
    let count = found.json.as_ref().and_then(|value| value["result"]["matches"].as_array()).map_or(0, Vec::len);
    if count > 1 {
      bail!("OCR preflight was ambiguous; no click sent");
    }
    if let Some(bounds) = found.json.as_ref().and_then(|value| rect_of(&value["result"]["matches"][0]["bounds"])) {
      let candidate = auv_driver_common::geometry::Point::new(bounds[0] + bounds[2] / 2.0, bounds[1] + bounds[3] / 2.0);
      let pixel = [candidate.x.round() as i32, candidate.y.round() as i32];
      if pixel[0] < rect[0] || pixel[0] >= rect[2] || pixel[1] < rect[1] || pixel[1] >= rect[3] {
        bail!("OCR resolved outside the owned control; no click sent");
      }
      point = Some((candidate, pixel));
    }
    scenario.records.write(
      "ocr_preflight",
      json!({"query": query, "attempt": attempt, "point": point.map(|(_, pixel)| pixel),
        "exact_point": point.map(|(candidate, _)| candidate), "owned_rect": rect}),
    )?;
    if point.is_some() {
      break;
    }
    std::thread::sleep(Duration::from_millis(200));
  }
  point.context("owned caption was not recognized; no click sent")
}

/// A half-open interval from a finite float to its next value contains that point alone.
fn exact_click_fence(point: &auv_driver_common::geometry::Point) -> Result<String> {
  let right = point.x.next_up();
  let bottom = point.y.next_up();
  if !point.x.is_finite() || !point.y.is_finite() || !right.is_finite() || !bottom.is_finite() {
    bail!("OCR preflight point is not finite; no click sent");
  }
  Ok(format!("{},{},{},{}", point.x, point.y, right, bottom))
}

fn click_text(scenario: &mut Scenario<'_>, entry: &Entry<'_>, query: &str, rect: [i32; 4]) -> Result<()> {
  let (point, pixel) = find_text(scenario, entry, query, rect)?;
  reject_user_input(entry.start, &scenario.watch.snapshot())?;
  scenario.pointer_may_go(&[pixel])?;
  let fence = exact_click_fence(&point)?;
  let clicked = call(scenario, entry, "screen.clickText", &[query.into(), "--within".into(), fence], false)?;
  let actual = clicked.json.as_ref().map(|value| &value["result"]["point"]);
  let contained = actual.is_some_and(|point| {
    point["x"].as_f64().is_some_and(|x| x >= f64::from(rect[0]) && x < f64::from(rect[2]))
      && point["y"].as_f64().is_some_and(|y| y >= f64::from(rect[1]) && y < f64::from(rect[3]))
  });
  scenario.records.write("click_destination", json!({"query": query, "actual": actual, "owned_rect": rect, "verified": contained}))?;
  if !contained {
    bail!("screen.clickText reported a point outside the owned control");
  }
  Ok(())
}

fn keys_may_go(scenario: &Scenario<'_>, entry: &Entry<'_>, target: &mut Receiver) -> Result<()> {
  reject_user_input(entry.start, &scenario.watch.snapshot())?;
  scenario.keys_may_go(target)?;
  reject_user_input(entry.start, &scenario.watch.snapshot())
}

fn retry_initial_blank_capture(initial: bool, attempt: u32, decoded: Option<(u8, u8)>, expected: (u8, u8), identified: bool) -> bool {
  initial && attempt < 3 && identified && expected.0 != 0 && expected.1 == 0 && decoded == Some((0, 0))
}

fn capture(scenario: &mut Scenario<'_>, entry: &Entry<'_>, target: &mut Receiver, initial: bool) -> Result<()> {
  let mut attempt = 0;
  loop {
    attempt += 1;
    let run = call(
      scenario,
      entry,
      "window.capture",
      &[
        "--target".into(),
        "win32-receiver.exe".into(),
        "--title".into(),
        entry.title.into(),
      ],
      true,
    )?;
    let decoded = if entry.route == "rust" {
      run.json.as_ref().and_then(|value| serde_json::from_value::<(u8, u8)>(value["result"]["decoded_strip"].clone()).ok())
    } else {
      decoded_capture(&run, &entry.layout.strip)?
    };
    let state = target.state()?;
    let expected = (state.clicks as u8, state.first_line as u8);
    let identity = run.json.as_ref().map(|value| &value["result"]["window"]);
    let identified =
      identity.is_some_and(|window| window["reference"]["id"] == target.hwnd().to_string() && window["process_id"] == entry.layout.pid);
    let verified = decoded == Some(expected) && identified;
    let retry = retry_initial_blank_capture(initial, attempt, decoded, expected, identified);
    scenario.records.write(
      "capture_verification",
      json!({"route": entry.route, "initial": initial, "attempt": attempt, "decoded": decoded, "application_state": expected,
      "window_identity_verified": identified, "verified": verified, "retry_initial_blank": retry}),
    )?;
    if verified {
      return Ok(());
    }
    if !retry {
      bail!("captured pixels do not show the application's state");
    }
    std::thread::sleep(Duration::from_millis(200));
  }
}

/// A failed daemon startup. `started` carries the Runner when the daemon process was actually
/// spawned — a half-started daemon must be stopped with a receipt, not recorded as never
/// having existed.
struct RunnerStartFailure {
  error: anyhow::Error,
  started: Option<Runner>,
}

/// One case's fixture. Every field owns its cleanup — `Runner::drop` ends the daemon process
/// tree, `Receiver::drop` closes the receiver after a bounded wait and then kills it, and the
/// capture-store `TempDir` removes itself — as a backstop behind the explicit, receipt-bearing
/// cleanup the failure paths perform.
struct Fixture {
  runner: Option<Runner>,
  driver: WindowsDriverSession,
  watch: DesktopWatch,
  start: Snapshot,
  target: Receiver,
  window: Window,
  layout: Layout,
  restore: Option<Restore>,
  hwnd: isize,
}

enum ReceiverCleanup {
  NotCreated,
  // TODO(receiver-spawn-receipt): distinguish a failed spawn from an unverified termination
  // after owner-approved work adds a detached-spawn cleanup receipt.
  SpawnFailed,
  Closed(Result<()>),
}

impl ReceiverCleanup {
  fn record(self) -> Value {
    match self {
      Self::NotCreated => json!({"state": "not_created"}),
      Self::SpawnFailed => json!({"state": "unknown", "note": "spawn may have created a process; termination has no receipt"}),
      Self::Closed(Ok(())) => json!({"state": "verified"}),
      Self::Closed(Err(error)) => json!({"state": "unknown", "error": error.to_string()}),
    }
  }
}

/// A fixture build that stopped partway. `runner_stop` is the bounded stop receipt when the
/// daemon had been created (None when never reached); `receiver_cleanup` records whether the
/// receiver spawn was reached or the process was confirmed closed. The flags say which later
/// steps were reached: their resources have no cleanup receipt API and were left to Drop, so the
/// caller records them as `unknown`, never as verified.
struct SetupFailure {
  error: anyhow::Error,
  runner_stop: Option<Result<Value>>,
  receiver_cleanup: ReceiverCleanup,
  /// The receiver's answer when the foreground was explicitly handed back while it could still
  /// act on the request; None when there was no restore or the receiver was never created.
  foreground_restore: Option<Result<bool>>,
  driver_created: bool,
  witness_created: bool,
  restore_created: bool,
  /// None means that the desktop witness did not start, so activity is unknown.
  user_interrupted: Option<bool>,
}

/// Builds one case's fixture: the private daemon (runner route), the driver session, the desktop
/// witness and the case's own receiver, bound and laid out. A failure returns a `SetupFailure`
/// carrying what the build can speak for, so the caller records receipts instead of claiming a
/// cleanup it cannot see.
fn build_fixture(
  options: &Options,
  route: &str,
  auv: &Path,
  store: &Path,
  output: &Path,
  spec: &ReceiverSpec,
  extra: &[&str],
) -> Result<Fixture, SetupFailure> {
  let mut runner = if route == "runner" {
    match Runner::start(auv, &store.join("private-daemon"), output) {
      Ok(runner) => Some(runner),
      Err(mut failure) => {
        // The daemon may have been spawned before the failure; stop it with its receipt. Only
        // a spawn that never happened is recorded as not_created.
        let runner_stop = failure.started.as_mut().map(Runner::finish);
        return Err(SetupFailure {
          error: failure.error,
          runner_stop,
          receiver_cleanup: ReceiverCleanup::NotCreated,
          foreground_restore: None,
          driver_created: false,
          witness_created: false,
          restore_created: false,
          user_interrupted: None,
        });
      }
    }
  } else {
    None
  };
  let driver = match WindowsDriver::new().open_local().map_err(anyhow::Error::msg) {
    Ok(driver) => driver,
    Err(error) => {
      let runner_stop = runner.as_mut().map(Runner::finish);
      return Err(SetupFailure {
        error,
        runner_stop,
        receiver_cleanup: ReceiverCleanup::NotCreated,
        foreground_restore: None,
        driver_created: false,
        witness_created: false,
        restore_created: false,
        user_interrupted: None,
      });
    }
  };
  let watch = match DesktopWatch::start() {
    Ok(watch) => watch,
    Err(error) => {
      let runner_stop = runner.as_mut().map(Runner::finish);
      return Err(SetupFailure {
        error,
        runner_stop,
        receiver_cleanup: ReceiverCleanup::NotCreated,
        foreground_restore: None,
        driver_created: true,
        witness_created: false,
        restore_created: false,
        user_interrupted: None,
      });
    }
  };
  let start = watch.snapshot();
  let mut target = match spawn_topmost_receiver(spec, extra) {
    Ok(target) => target,
    Err(error) => {
      let runner_stop = runner.as_mut().map(Runner::finish);
      return Err(SetupFailure {
        error,
        runner_stop,
        receiver_cleanup: ReceiverCleanup::SpawnFailed,
        foreground_restore: None,
        driver_created: true,
        witness_created: true,
        restore_created: false,
        user_interrupted: Some(physical_input_changed(&start, &watch.snapshot())),
      });
    }
  };
  let hwnd = target.hwnd();
  let restore = (!options.observe_only).then(|| Restore {
    cursor: start.cursor,
    foreground: start.foreground,
    receiver: hwnd,
    // TempDir owns and verifies capture-store removal after the receiver and Runner are closed.
    store: None,
    physical_input_at_start: Some((start.physical_mouse, start.physical_keys)),
    clipboard: None,
  });
  let bound = (|| -> Result<(Window, Layout)> {
    let (window, _) = discover_by_hwnd(&driver, hwnd)?;
    let layout = target.refresh_layout()?;
    if !options.observe_only {
      target.request(json!({"cmd": "keep_foreground"}))?;
    }
    Ok((window, layout))
  })();
  let (window, layout) = match bound {
    Ok(pair) => pair,
    Err(error) => {
      // The receiver and the daemon both exist here, so their cleanups have real receipts.
      // Hand the foreground back while the receiver can still act on the request: the Restore
      // backstop only posts a message to the receiver's window, and a window that has exited
      // cannot yield a foreground it no longer holds. The receiver is closed after that
      // answer, and the daemon gets its bounded stop.
      let user_interrupted = physical_input_changed(&start, &watch.snapshot());
      let foreground_restore = if restore.is_some() && !user_interrupted {
        Some(target.yield_foreground(start.foreground))
      } else {
        None
      };
      let receiver_close = target.close();
      let runner_stop = runner.as_mut().map(Runner::finish);
      let user_interrupted = physical_input_changed(&start, &watch.snapshot());
      return Err(SetupFailure {
        error,
        runner_stop,
        receiver_cleanup: ReceiverCleanup::Closed(receiver_close),
        foreground_restore,
        driver_created: true,
        witness_created: true,
        restore_created: restore.is_some(),
        user_interrupted: Some(user_interrupted),
      });
    }
  };
  Ok(Fixture {
    runner,
    driver,
    watch,
    start,
    target,
    window,
    layout,
    restore,
    hwnd,
  })
}

fn case(options: &Options, route: &'static str, round: u32) -> Result<bool> {
  let output = options.output.join(format!("{route}-{round:02}"));
  std::fs::create_dir_all(&output)?;
  let auv = options.auv.canonicalize()?;
  let store_dir = tempfile::Builder::new().prefix("store-").tempdir_in(&output)?;
  let store = store_dir.path().to_owned();
  let mut records = Records::create(&output.join("workflow.jsonl"), &run_id(), "gui_workflow")?;
  // workflow.jsonl exists from here on, so every per-route failure is recorded instead of
  // leaving a truncated file — whenever the record medium itself is usable and normal error
  // handling can run; a forced process kill or an unwritable volume can still truncate it. The
  // steps above have no record medium yet; run() surfaces their process error and stops the
  // whole matrix, because they are global misconfigurations (output directory, auv binary, temp
  // space), not per-route outcomes.
  let (left, top) = placement(0, 420, 440);
  let spec = ReceiverSpec {
    index: 0,
    title: format!("AUV workflow {route} {round}"),
    left,
    top,
    width: 420,
    height: 440,
    color: "2E6F9E".into(),
    sink: false,
  };
  let word = match route {
    "rust" => "Cedar",
    "invoke" => "Willow",
    _ => "Maple",
  };
  let suffix = if round % 2 == 0 { "Garden" } else { "Meadow" };
  let input_caption = format!("Write {word} {suffix}");
  let button_caption = format!("Submit {word} {suffix}");
  // A black/wrong frame decodes to zero in the existing strip decoder. Start with a nonzero
  // calibration so observation-only checks cannot pass on an all-black frame. This is fixture
  // setup, not evidence that any AUV click was delivered.
  let calibration = (13 + round % 17).to_string();
  let extra = [
    "--foreground-input",
    "--input-caption",
    input_caption.as_str(),
    "--button-caption",
    button_caption.as_str(),
    "--initial-clicks",
    calibration.as_str(),
  ];
  let fixture = build_fixture(options, route, &auv, &store, &output, &spec, &extra);
  let Fixture {
    mut runner,
    driver,
    watch,
    start,
    mut target,
    window,
    layout,
    restore,
    hwnd,
  } = match fixture {
    Ok(fixture) => fixture,
    Err(failure) => {
      // Cleanup facts, not claims. The store is still open here, so close it and check removal
      // before the record says anything about it. Resources whose cleanup has no receipt API
      // are `unknown`, and a resource whose step was never reached is `not_created`.
      let error = format!("{:#}", failure.error);
      let store_path = store_dir.path().to_owned();
      let store_close = store_dir.close();
      let store_removed = store_close.is_ok() && !store_path.exists();
      let runner_cleanup = match failure.runner_stop {
        None => json!({"state": "not_created"}),
        Some(Ok(receipt)) if receipt["daemon_exited"] == true => json!({"state": "verified", "receipt": receipt}),
        Some(Ok(receipt)) => json!({"state": "unknown", "receipt": receipt}),
        Some(Err(stop_error)) => json!({"state": "unknown", "error": stop_error.to_string()}),
      };
      let receiver_cleanup = failure.receiver_cleanup.record();
      let foreground_cleanup = match failure.foreground_restore {
        None => json!({"state": "not_attempted"}),
        Some(Ok(handed)) => json!({"state": "verified", "handed": handed}),
        Some(Err(restore_error)) => json!({"state": "unknown", "error": restore_error.to_string()}),
      };
      let drop_only = |created: bool| {
        if created {
          json!({"state": "unknown", "note": "cleaned up by drop; no receipt exists"})
        } else {
          json!({"state": "not_created"})
        }
      };
      records.write("setup_failed", json!({"route": route, "round": round, "error": &error}))?;
      records.write(
        "summary",
        json!({"route": route, "round": round, "passed": false, "setup_failed": true,
        "observe_only": options.observe_only, "observation_passed": Value::Null, "functional_passed": Value::Null,
        "user_interrupted": failure.user_interrupted,
        "error": error, "failures": [], "quiet_operations": 0, "quiet_held": 0, "quiet_inconclusive": 0, "quiet_violated": 0,
        "cleanup": {"runner_daemon": runner_cleanup, "receiver": receiver_cleanup,
          "foreground_restore": foreground_cleanup,
          "driver_session": drop_only(failure.driver_created), "desktop_witness": drop_only(failure.witness_created),
          "restore": drop_only(failure.restore_created),
          "capture_store": {"state": if store_removed { "verified" } else { "unknown" }, "removed": store_removed,
            "close_error": store_close.err().map(|close_error| close_error.to_string())}}}),
      )?;
      println!("workflow {route} round {round}: FAIL (setup failed; see workflow.jsonl)");
      if failure.user_interrupted == Some(true) {
        bail!("physical user input interrupted workflow setup; no later rounds were started");
      }
      return Ok(false);
    }
  };
  records.write(
    "setup",
    json!({"route": route, "round": round, "system": system_facts(), "hwnd": hwnd.to_string(),
    "pid": layout.pid, "window_rect": layout.window_rect, "typing_rect": layout.typing_rect, "button_rect": layout.button_rect,
    "runner_run": runner.as_ref().map(|runner| &runner.run), "foreground_guard_atomic": false, "observe_only": options.observe_only,
    "calibration_click_count": calibration, "calibration_is_input_delivery": false}),
  )?;
  let mut scenario = Scenario {
    auv: &auv,
    store: &store,
    hwnd,
    watch: &watch,
    desk: Desk::new(&watch, hwnd),
    tally: Tally::default(),
    records,
  };
  let entry = Entry {
    route,
    auv: &auv,
    store: &store,
    runner: runner.as_ref(),
    driver: &driver,
    window: &window,
    layout: &layout,
    title: &spec.title,
    start: &start,
  };
  let started = Instant::now();
  let result = (|| -> Result<()> {
    capture(&mut scenario, &entry, &mut target, true)?;
    if options.observe_only {
      find_text(&mut scenario, &entry, &input_caption, layout.typing_rect)?;
      find_text(&mut scenario, &entry, &button_caption, layout.button_rect)?;
      return Ok(());
    }
    click_text(&mut scenario, &entry, &input_caption, layout.typing_rect)?;
    // This native single-line EDIT does not select all on Ctrl+A. Select the line through
    // keyboard input, then verify the actual control selection before replacing its text.
    keys_may_go(&scenario, &entry, &mut target)?;
    call(&mut scenario, &entry, "input.key", &["home".into()], false)?;
    keys_may_go(&scenario, &entry, &mut target)?;
    call(&mut scenario, &entry, "input.key", &["shift+end".into()], false)?;
    let selection = target.request(json!({"cmd": "state"}))?;
    let expected_selection = json!([0, input_caption.encode_utf16().count()]);
    let selected = selection["typing"]["focused"] == true
      && selection["typing"]["text"].as_str() == Some(input_caption.as_str())
      && selection["typing"]["selection"] == expected_selection;
    scenario.records.write(
      "selection_verification",
      json!({"expected": expected_selection, "actual": selection["typing"]["selection"], "text": selection["typing"]["text"],
      "focused": selection["typing"]["focused"], "keys": selection["typing"]["keys"], "verified": selected}),
    )?;
    if !selected {
      bail!("the application did not select the complete existing text");
    }
    keys_may_go(&scenario, &entry, &mut target)?;
    let text = format!("Workflow {route} {round} {}", std::process::id());
    call(&mut scenario, &entry, "input.typeText", &[text.clone()], false)?;
    let typed = target.request(json!({"cmd": "state"}))?;
    scenario
      .records
      .write("text_verification", json!({"expected": text, "actual": typed["typing"]["text"], "focused": typed["typing"]["focused"]}))?;
    if typed["typing"]["text"] != text {
      bail!("the application did not receive the exact typed text");
    }
    let before = target.state()?;
    let local = point_in_window(&window, layout.edit_center).point();
    call(
      &mut scenario,
      &entry,
      "input.scroll",
      &[
        local.x.to_string(),
        local.y.to_string(),
        "--target".into(),
        format!("window:{hwnd}"),
        "--dx".into(),
        "0".into(),
        "--dy".into(),
        "360".into(),
        "--input-policy".into(),
        "background-only".into(),
        // Match the direct Rust route's ScrollOptions settle so both routes wait the same time
        // before the scroll_verification observation.
        "--settle-ms".into(),
        "100".into(),
      ],
      true,
    )?;
    let after = target.state()?;
    scenario.records.write(
      "scroll_verification",
      json!({"first_line": [before.first_line, after.first_line], "verified": after.first_line > before.first_line}),
    )?;
    if after.first_line <= before.first_line {
      bail!("the application's scroll position did not change");
    }
    click_text(&mut scenario, &entry, &button_caption, layout.button_rect)?;
    let submitted = target.request(json!({"cmd": "state"}))?;
    let verified = submitted["submitted_text"] == text && submitted["clicks"] == before.clicks + 1;
    scenario.records.write(
      "submit_verification",
      json!({"clicks": [before.clicks, submitted["clicks"].as_u64().unwrap_or(0)],
      "expected": text, "submitted": submitted["submitted_text"], "verified": verified}),
    )?;
    if !verified {
      bail!("the application did not submit the typed record exactly once");
    }
    capture(&mut scenario, &entry, &mut target, false)?;

    // The submit button now has the focus. Test the guard before calling any key dispatch path.
    let before_guard = target.request(json!({"cmd": "state"}))?;
    let snapshot = watch.snapshot();
    reject_user_input(&start, &snapshot)?;
    let rejected = scenario.keys_may_go(&mut target).is_err();
    let after_guard = target.request(json!({"cmd": "state"}))?;
    let finish_guard = watch.snapshot();
    reject_user_input(&start, &finish_guard)?;
    let no_keys = snapshot.injected_keys == finish_guard.injected_keys;
    let unchanged = before_guard["typing"] == after_guard["typing"];
    scenario.records.write(
      "wrong_focus_control",
      json!({"rejected_before_dispatch": rejected, "injected_keys_unchanged": no_keys,
      "application_text_and_keys_unchanged": unchanged}),
    )?;
    if !(rejected && no_keys && unchanged) {
      bail!("wrong-focus guard did not prevent key dispatch");
    }

    // A second window owned by this harness becomes the foreground through AUV pointer input.
    // No user's window is activated or typed into for this negative control.
    let decoy_spec = ReceiverSpec {
      index: 1,
      title: "AUV workflow decoy".into(),
      left: (left - 460).max(0),
      ..spec.clone()
    };
    let mut decoy = spawn_topmost_receiver(&decoy_spec, &["--foreground-input"])?;
    decoy.request(json!({"cmd": "keep_foreground"}))?;
    let point = decoy.layout.button_center;
    reject_user_input(&start, &watch.snapshot())?;
    scenario.wait_until_idle()?;
    if !owns_point(decoy.hwnd(), point) {
      bail!("decoy is not uncovered; no click sent");
    }
    reject_user_input(&start, &watch.snapshot())?;
    driver
      .input()
      .click_at(
        auv_driver_common::geometry::Point::new(f64::from(point[0]), f64::from(point[1])),
        MouseButton::Left,
        Click::Single,
        Default::default(),
      )
      .map_err(anyhow::Error::msg)?;
    std::thread::sleep(Duration::from_millis(150));
    let decoy_before = decoy.request(json!({"cmd": "state"}))?;
    let before = watch.snapshot();
    reject_user_input(&start, &before)?;
    let rejected = scenario.keys_may_go(&mut target).is_err();
    let after = watch.snapshot();
    reject_user_input(&start, &after)?;
    let decoy_after = decoy.request(json!({"cmd": "state"}))?;
    let target_after = target.request(json!({"cmd": "state"}))?;
    let verified = before.foreground == decoy.hwnd()
      && rejected
      && before.injected_keys == after.injected_keys
      && decoy_before["typing"] == decoy_after["typing"]
      && target_after["typing"] == after_guard["typing"];
    scenario.records.write(
      "wrong_window_control",
      json!({"decoy_foreground": before.foreground == decoy.hwnd(),
      "rejected_before_dispatch": rejected, "injected_keys_unchanged": before.injected_keys == after.injected_keys, "verified": verified}),
    )?;
    if !physical_input_changed(&start, &watch.snapshot()) {
      let _ = decoy.yield_foreground(hwnd);
    }
    if !verified {
      bail!("wrong-window guard failed or the control did not become foreground");
    }
    Ok(())
  })();
  let elapsed = started.elapsed().as_secs_f64() * 1000.0;
  // Preserve lifecycle evidence before removing the temporary store of desktop captures.
  if let Some(runner) = runner.as_ref() {
    let mut command = runner.command();
    command.args([
      "run",
      "get",
      &runner.run,
      "--endpoint",
      &runner.endpoint,
      "--json",
    ]);
    let output = command_output(command, Duration::from_secs(10));
    scenario.records.write(
      "runner_scope",
      json!({"exit": output.as_ref().ok().and_then(|output| output.status.code()),
      "error": output.as_ref().err().map(ToString::to_string),
      "run": output.as_ref().ok().and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())}),
    )?;
  }
  let handed = options.observe_only
    || (!physical_input_changed(&start, &watch.snapshot()) && target.yield_foreground(start.foreground).unwrap_or(false));
  let mut runner_closed = true;
  if let Some(runner) = runner.as_mut() {
    let closed = runner.finish();
    runner_closed = closed.as_ref().is_ok_and(runner_cleanup_verified);
    scenario.records.write(
      "runner_cleanup",
      json!({"receipt": closed.as_ref().ok(), "error": closed.as_ref().err().map(ToString::to_string), "verified": runner_closed}),
    )?;
  }
  drop(runner);
  drop(restore);
  let receiver_close = target.close();
  // SAFETY: read-only query for the window this harness created.
  let window_closed = !unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(HWND(hwnd as *mut c_void)) }.as_bool();
  let store_path = store_dir.path().to_owned();
  let store_close = store_dir.close();
  let store_removed = store_close.is_ok() && !store_path.exists();
  let cleanup_verified = receiver_close.is_ok() && window_closed && store_removed;
  scenario.records.write(
    "fixture_cleanup",
    json!({"window_closed": window_closed,
    "receiver_close_error": receiver_close.err().map(|error| error.to_string()), "store_removed": store_removed,
    "store_close_error": store_close.err().map(|error| error.to_string()), "verified": cleanup_verified}),
  )?;
  let finish = watch.snapshot();
  // The per-operation witness does not cover fixture launch, gaps and cleanup. Observation-only
  // acceptance includes the complete watched interval, without moving the user's pointer back.
  let (phase_verdict, phase_timeline) = watch.judge(&start, &finish);
  let no_global_input = finish.injected_mouse == start.injected_mouse && finish.injected_keys == start.injected_keys;
  let user_interrupted = physical_input_changed(&start, &finish);
  let quiet_verified = quiet_verified(&scenario.tally, options.observe_only, &phase_verdict, &start, &finish);
  let passed = result.is_ok()
    && scenario.tally.failures.is_empty()
    && quiet_verified
    && runner_closed
    && cleanup_verified
    && (options.observe_only || finish.cursor == start.cursor)
    && handed
    && (options.observe_only || finish.foreground == start.foreground);
  // Evidence contract: `observation_passed` and `functional_passed` report only the scenario
  // body (`result`); the overall `passed` below additionally requires the quiet tally, runner
  // cleanup, fixture cleanup, and cursor/foreground restoration. Consumers must gate on
  // `passed`: a true `functional_passed` next to a false `passed` records a workflow whose
  // steps ran but whose desktop or cleanup contract failed.
  scenario.records.write(
    "summary",
    json!({"route": route, "round": round, "passed": passed, "ms": elapsed,
    "observe_only": options.observe_only, "observation_passed": options.observe_only.then_some(result.is_ok()),
    "functional_passed": (!options.observe_only).then_some(result.is_ok()), "quiet_verified": quiet_verified,
    "user_interrupted": user_interrupted,
    "error": result.err().map(|error| format!("{error:#}")), "failures": scenario.tally.failures,
    "quiet_operations": scenario.tally.ops, "quiet_held": scenario.tally.held, "quiet_inconclusive": scenario.tally.inconclusive,
    "quiet_violated": scenario.tally.violated, "gap_findings": scenario.desk.gaps,
    "phase_verdict": phase_verdict, "phase_foreground_events": super::super::background::events(&phase_timeline, start.taken_ms, &[hwnd]),
    "injected_mouse": finish.injected_mouse - start.injected_mouse, "injected_keys": finish.injected_keys - start.injected_keys,
    "no_global_input": no_global_input,
    "cursor_restored": (!options.observe_only).then_some(finish.cursor == start.cursor),
    "foreground_restored": (!options.observe_only).then_some(finish.foreground == start.foreground),
    "cursor_unchanged": finish.cursor == start.cursor, "foreground_unchanged": finish.foreground == start.foreground,
    "physical_mouse": finish.physical_mouse - start.physical_mouse, "physical_keys": finish.physical_keys - start.physical_keys,
    "input_action_retries": 0, "foreground_input_expected": !options.observe_only}),
  )?;
  println!(
    "{} {route} round {round}: {}",
    if options.observe_only {
      "observation"
    } else {
      "workflow"
    },
    if passed {
      "PASS"
    } else {
      "FAIL (see workflow.jsonl)"
    }
  );
  if user_interrupted {
    bail!("physical user input interrupted the workflow; no later rounds were started");
  }
  Ok(passed)
}

pub fn run(options: &Options) -> Result<bool> {
  if options.rounds == 0 {
    bail!("rounds must be positive");
  }
  if options.output.exists() {
    bail!("workflow output already exists; choose a fresh directory to preserve evidence");
  }
  std::fs::create_dir_all(&options.output)?;
  let mut passed = true;
  for route in ["rust", "invoke", "runner"] {
    for round in 1..=options.rounds {
      passed &= case(options, route, round).with_context(|| format!("workflow {route} round {round}"))?;
    }
  }
  Ok(passed)
}

#[cfg(test)]
mod tests {
  use super::*;

  // https://github.com/moeru-ai/auv/actions/runs/40453112779
  #[test]
  fn initial_blank_capture_retry_is_bounded_and_identity_checked() {
    // ROOT CAUSE:
    //
    // The first capture in rust round 2 decoded to (0, 0), while the owned
    // receiver reported (15, 0). The old workflow stopped at that first frame.
    // The cause of the blank capture is not established by the recorded facts.
    assert!(retry_initial_blank_capture(true, 1, Some((0, 0)), (15, 0), true));
    assert!(retry_initial_blank_capture(true, 2, Some((0, 0)), (15, 0), true));
    assert!(!retry_initial_blank_capture(true, 3, Some((0, 0)), (15, 0), true));
    assert!(!retry_initial_blank_capture(true, 1, Some((0, 0)), (15, 0), false));
    assert!(!retry_initial_blank_capture(false, 1, Some((0, 0)), (15, 0), true));
    assert!(!retry_initial_blank_capture(true, 1, Some((4, 0)), (15, 0), true));
    assert!(!retry_initial_blank_capture(true, 1, None, (15, 0), true));
  }

  #[test]
  fn receiver_spawn_failure_has_unknown_cleanup_until_a_receipt_exists() {
    // ROOT CAUSE:
    //
    // A failed receiver handshake can happen after process creation. The old
    // setup summary recorded it as not_created even though termination had no receipt.
    assert_eq!(ReceiverCleanup::SpawnFailed.record()["state"], "unknown");
    assert_eq!(ReceiverCleanup::NotCreated.record()["state"], "not_created");
    assert_eq!(ReceiverCleanup::Closed(Ok(())).record()["state"], "verified");
    assert_eq!(ReceiverCleanup::Closed(Err(anyhow::anyhow!("termination denied"))).record()["state"], "unknown");
  }

  #[test]
  fn runner_cleanup_is_bounded_when_taskkill_fails() {
    // ROOT CAUSE:
    //
    // If taskkill returned without ending the daemon, Runner::finish
    // waited for the daemon with no deadline. Drop had the same unbounded wait.
    const TEST_CHILD: &str = "AUV_TEST_RUNNER_TASKKILL_FAILURE";
    if std::env::var_os(TEST_CHILD).is_some() {
      let root = std::env::current_dir().expect("test directory");
      let child = Command::new("powershell.exe")
        .args([
          "-NoProfile",
          "-NonInteractive",
          "-Command",
          "Start-Sleep -Seconds 8",
        ])
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .expect("spawn a hidden owned process");
      let mut runner = Runner {
        child,
        auv: PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec")),
        root: root.clone(),
        endpoint: "unused".into(),
        run: "unused".into(),
        evidence: root,
      };
      let started = Instant::now();
      let result = runner.finish();
      assert!(started.elapsed() < Duration::from_secs(6), "cleanup waited for the sleeping process");
      let mut receipt = result.expect("the root fallback should end the owned process");
      assert_eq!(receipt["fallback_root_kill"], true);
      assert_eq!(receipt["daemon_exited"], true);
      assert_eq!(receipt["owned_tree_kill_exit"], 0, "the substitute command should report success without ending the daemon");
      receipt["run_stop_exit"] = json!(0);
      receipt["owned_tree_kill_exit"] = json!(0);
      assert!(!runner_cleanup_verified(&receipt), "a root-only fallback does not verify descendant cleanup");
      return;
    }

    let root = tempfile::tempdir().expect("test directory");
    // Windows searches the test executable's directory before System32.
    // This copy exits on taskkill's arguments without terminating the process.
    std::fs::copy(std::env::var_os("ComSpec").expect("ComSpec"), root.path().join("taskkill.exe")).expect("copy the failing command");
    let test_exe = root.path().join("cleanup-test.exe");
    std::fs::copy(std::env::current_exe().expect("test executable"), &test_exe).expect("copy the test executable");
    let mut command = Command::new(test_exe);
    command
      .args([
        "--exact",
        "desktop_windows::foreground_commands::workflow::tests::runner_cleanup_is_bounded_when_taskkill_fails",
        "--nocapture",
      ])
      .current_dir(root.path())
      .env(TEST_CHILD, "1")
      .stdin(Stdio::null())
      .creation_flags(0x0800_0000);
    let output = command_output(command, Duration::from_secs(20)).expect("cleanup test process must end");
    assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
  }

  #[test]
  fn physical_input_must_make_the_workflow_inconclusive() {
    // ROOT CAUSE:
    //
    // If a user pressed a key without changing the foreground or pointer, the
    // phase witness reported Held and the workflow's old gate accepted it.
    // Foreground operations also did not count toward the quiet tally. The
    // workflow now rejects physical input across the whole witnessed interval.
    let start = Snapshot {
      taken_ms: 100,
      foreground: 1,
      cursor: (10, 20),
      injected_mouse: 0,
      injected_keys: 0,
      physical_mouse: 0,
      physical_keys: 0,
      foreground_changes: 0,
      last_physical_ms: 0,
    };
    let tally = Tally::default();
    for finish in [
      Snapshot {
        physical_keys: 1,
        last_physical_ms: 101,
        taken_ms: 102,
        ..start
      },
      Snapshot {
        physical_mouse: 1,
        last_physical_ms: 101,
        taken_ms: 102,
        ..start
      },
    ] {
      let phase_verdict = Snapshot::judge(&start, &finish, &super::super::super::watch::Timeline::default());
      assert!(matches!(phase_verdict, Verdict::Held), "the ordinary quiet witness alone does not detect this input");
      assert!(reject_user_input(&start, &finish).is_err(), "do not dispatch more input after the user returns");
      for observe_only in [true, false] {
        assert!(
          !quiet_verified(&tally, observe_only, &phase_verdict, &start, &finish),
          "physical input must prevent acceptance (observe_only={observe_only})"
        );
      }
    }
    assert!(reject_user_input(&start, &start).is_ok());
    assert!(quiet_verified(&tally, true, &Verdict::Held, &start, &start));
    let injected = Snapshot {
      injected_keys: 1,
      ..start
    };
    assert!(quiet_verified(
      &tally,
      false,
      &Verdict::Violated {
        reasons: vec!["injected_key_input"]
      },
      &start,
      &injected
    ));
    assert!(!quiet_verified(
      &tally,
      true,
      &Verdict::Violated {
        reasons: vec!["injected_key_input"]
      },
      &start,
      &injected
    ));
  }

  #[test]
  fn workflow_click_refuses_re_resolved_points_outside_the_owned_control() {
    let args = vec!["Submit".into(), "--within".into(), "100,200,240,290".into()];
    assert!(ensure_workflow_click_bounds(&args, &auv_driver_common::geometry::Point::new(120.0, 220.0)).is_ok());
    assert!(ensure_workflow_click_bounds(&args, &auv_driver_common::geometry::Point::new(241.0, 220.0)).is_err());
    assert!(ensure_workflow_click_bounds(&args[..1], &auv_driver_common::geometry::Point::new(120.0, 220.0)).is_err());
  }

  #[test]
  fn workflow_click_must_not_switch_to_another_point_inside_the_control() {
    // ROOT CAUSE:
    //
    // The workflow checked window ownership at the preflight point, but the
    // old fence also accepted a different OCR point inside the same control.
    let point = auv_driver_common::geometry::Point::new(120.5, 220.0);
    let args = vec![
      "Submit".into(),
      "--within".into(),
      exact_click_fence(&point).expect("finite preflight point"),
    ];
    assert!(ensure_workflow_click_bounds(&args, &point).is_ok(), "the checked point remains allowed");
    assert!(
      ensure_workflow_click_bounds(&args, &auv_driver_common::geometry::Point::new(125.0, 220.0)).is_err(),
      "a different point in the control has not passed the ownership check"
    );
    assert!(
      ensure_workflow_click_bounds(&args, &auv_driver_common::geometry::Point::new(point.x.next_up(), point.y)).is_err(),
      "even the next representable coordinate has not passed the ownership check"
    );
    for point in [
      auv_driver_common::geometry::Point::new(-120.5, -220.0),
      auv_driver_common::geometry::Point::new(0.0, 0.0),
    ] {
      let args = vec![
        "Submit".into(),
        "--within".into(),
        exact_click_fence(&point).expect("finite preflight point"),
      ];
      assert!(ensure_workflow_click_bounds(&args, &point).is_ok());
      assert!(ensure_workflow_click_bounds(&args, &auv_driver_common::geometry::Point::new(point.x.next_up(), point.y)).is_err());
    }
  }

  #[test]
  fn an_unresponsive_cli_cannot_wait_forever() {
    // A real subprocess, with no desktop or network access, exercises timeout/cancellation rather
    // than mocking the helper. The long sleep must not become the caller's wait time.
    let mut command = Command::new("powershell.exe");
    command
      .args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Start-Sleep -Seconds 10",
      ])
      .creation_flags(0x0800_0000)
      .stdin(Stdio::null());
    let started = Instant::now();
    let error = command_output(command, Duration::from_millis(100)).expect_err("the sleeping CLI must time out");
    assert!(error.to_string().contains("deadline exceeded"), "{error:#}");
    assert!(started.elapsed() < Duration::from_secs(3), "the CLI exceeded its bounded wait");
  }

  #[test]
  fn a_setup_failure_is_recorded_with_truthful_cleanup_and_not_passed() {
    // The runner route fails at the daemon spawn (an existing but non-executable auv binary),
    // before the driver session, the desktop witness or the receiver are created, so this
    // exercises the recorded failure path with no desktop contact and no input.
    let output = tempfile::tempdir().expect("temp output directory");
    let auv = output.path().join("not-an-executable");
    std::fs::write(&auv, b"not a PE image").expect("write the invalid binary");
    let options = Options {
      output: output.path().to_owned(),
      auv,
      rounds: 1,
      observe_only: true,
    };
    let passed = case(&options, "runner", 1).expect("a setup failure is a recorded outcome, not a process error");
    assert!(!passed);
    let records: Vec<Value> = std::fs::read_to_string(output.path().join("runner-01").join("workflow.jsonl"))
      .expect("the case's workflow.jsonl exists")
      .lines()
      .map(|line| serde_json::from_str(line).expect("every record is one JSON object"))
      .collect();
    let setup_failed = records.iter().find(|record| record["kind"] == "setup_failed").expect("the failure is recorded");
    assert!(setup_failed["error"].as_str().is_some_and(|error| !error.is_empty()), "{setup_failed}");
    let summary = records.iter().find(|record| record["kind"] == "summary").expect("the file ends with a summary");
    assert_eq!(summary["passed"], json!(false));
    assert_eq!(summary["setup_failed"], json!(true));
    assert_eq!(summary["functional_passed"], Value::Null);
    assert_eq!(summary["observation_passed"], Value::Null);
    assert_eq!(summary["user_interrupted"], Value::Null, "no witness existed to judge physical input");
    // Never-created resources are not claimed as cleaned, and the store — the one resource the
    // failure path can close and re-check before recording — is the only verified one.
    assert_eq!(summary["cleanup"]["runner_daemon"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["receiver"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["driver_session"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["desktop_witness"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["restore"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["capture_store"]["state"], json!("verified"));
    assert_eq!(summary["cleanup"]["capture_store"]["removed"], json!(true));
  }

  #[test]
  fn a_half_started_daemon_is_stopped_with_a_receipt_not_recorded_as_never_created() {
    // The runner route spawns a real process (cmd.exe, which exits on the unknown `serve`
    // command) that never becomes a daemon, so Runner::start fails after the spawn and must
    // hand the started process back for a receipt-bearing stop. No desktop contact, no input.
    let output = tempfile::tempdir().expect("temp output directory");
    let auv = PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec is defined on Windows"));
    let options = Options {
      output: output.path().to_owned(),
      auv,
      rounds: 1,
      observe_only: true,
    };
    let passed = case(&options, "runner", 1).expect("a setup failure is a recorded outcome, not a process error");
    assert!(!passed);
    let records: Vec<Value> = std::fs::read_to_string(output.path().join("runner-01").join("workflow.jsonl"))
      .expect("the case's workflow.jsonl exists")
      .lines()
      .map(|line| serde_json::from_str(line).expect("every record is one JSON object"))
      .collect();
    let summary = records.iter().find(|record| record["kind"] == "summary").expect("the file ends with a summary");
    assert_eq!(summary["passed"], json!(false));
    assert_eq!(summary["setup_failed"], json!(true));
    // The daemon process was spawned, so its cleanup is a verified receipt, not not_created,
    // and the receipt shows the process tree actually ended.
    assert_eq!(summary["cleanup"]["runner_daemon"]["state"], json!("verified"));
    assert_eq!(summary["cleanup"]["runner_daemon"]["receipt"]["daemon_exited"], json!(true));
    assert_eq!(summary["cleanup"]["receiver"]["state"], json!("not_created"));
    assert_eq!(summary["cleanup"]["foreground_restore"]["state"], json!("not_attempted"));
    assert_eq!(summary["cleanup"]["capture_store"]["state"], json!("verified"));
  }
}
