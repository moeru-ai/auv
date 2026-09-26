//! Native no-raise experiment. AUV sends input; agent-browser only observes Chrome.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde_json::{Value, json};
use tokio::{process::Child, time::Instant};

use auv_base_evals::{
  macos::{ABC, DesktopOptions, build_app, compile, focus_cases, root as macos_root, wait_json},
  receiver::Receiver,
  support::{self, Process, binary, invoke, launch, new_output, now_ms, objects, output, path, pause, read_json, stop, write_json},
};

#[derive(Parser)]
#[command(about = "Compare activation, key-window focus, and explicit restoration")]
struct Options {
  #[command(flatten)]
  desktop: DesktopOptions,

  #[arg(long, default_value_os_t = binary("keyboard-receiver-sender"))]
  sender: PathBuf,

  #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
  repetitions: u32,

  #[arg(long, value_parser = ["swift", "electron", "chrome"])]
  receiver: Vec<String>,

  #[arg(long, value_parser = ["baseline", "no_raise", "no_raise_key", "no_raise_key_menu", "no_raise_click", "foreground_pid", "foreground"])]
  mode: Vec<String>,

  #[arg(long, default_value = "conditional", value_parser = ["conditional", "records"])]
  restore: String,

  /// Fail unless text, restoration, and the selected delivery posture all pass.
  #[arg(long)]
  require_success: bool,
}

#[tokio::main]
async fn main() -> Result<std::process::ExitCode> {
  ensure!(cfg!(target_os = "macos"), "desktop-macos evaluations require macOS");
  support::run(run(Options::parse())).await
}

struct Browser {
  session: String,
  port: String,
  open: bool,
}

impl Browser {
  async fn command(&mut self, args: &[String]) -> Result<String> {
    self.open = true;

    let mut command = vec![
      "--session".into(),
      self.session.clone(),
      "--cdp".into(),
      self.port.clone(),
    ];
    command.extend_from_slice(args);

    output(Path::new("agent-browser"), &command).await
  }

  async fn close(&mut self) -> Result<()> {
    if self.open {
      output(Path::new("agent-browser"), &["--session".into(), self.session.clone(), "close".into()]).await?;
      self.open = false;
    }

    Ok(())
  }
}

impl Drop for Browser {
  fn drop(&mut self) {
    if self.open {
      // Best effort only on exceptional early exits; ordinary cleanup awaits close.
      let _ = std::process::Command::new("agent-browser")
        .args(["--session", &self.session, "close"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    }
  }
}

struct Target {
  child: Child,
  directory: PathBuf,
  server: Option<Receiver>,
  browser: Option<Browser>,
  window: Value,
  pid: u32,
}

impl Target {
  async fn launch(kind: &str, options: &Options, directory: &Path, root: &Path, control: &mut Process) -> Result<Self> {
    std::fs::create_dir(directory)?;

    let server = if kind == "swift" {
      None
    } else {
      Some(Receiver::start(Some(&directory.join("dom-state.jsonl"))).await?)
    };

    let profile = directory.join("profile");
    let (executable, args) = if let Some(server) = &server {
      let url = server.url(&format!("AUV No Raise {kind}"), true);

      if kind == "electron" {
        (
          options.desktop.electron.clone(),
          vec![
            path(&objects().join("dist/electron/focus.js")),
            url,
            path(&profile),
            path(directory),
          ],
        )
      } else {
        (
          options.desktop.chrome.clone(),
          vec![
            format!("--user-data-dir={}", profile.display()),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-sync".into(),
            "--remote-debugging-port=0".into(),
            format!("--app={url}"),
          ],
        )
      }
    } else {
      (build_app(&root.join("receiver"), root, "AUVSwiftReceiver")?, vec![path(directory), "AUV Swift Target".into()])
    };

    let child = launch(&executable, &args, &directory.join("process.log"))?;
    let pid = child.id().context("receiver PID missing")?;
    let mut target = Self {
      child,
      directory: directory.to_owned(),
      server,
      browser: None,
      window: Value::Null,
      pid,
    };

    if kind == "chrome" {
      let port_path = profile.join("DevToolsActivePort");
      let end = Instant::now() + Duration::from_secs(15);
      while !port_path.exists() {
        ensure!(Instant::now() < end, "Chrome CDP startup timed out");
        pause(30).await?;
      }

      let port = std::fs::read_to_string(port_path)?.lines().next().context("CDP port missing")?.to_owned();
      let mut browser = Browser {
        session: format!("auv-no-raise-{}-{pid}", now_ms()),
        port,
        open: false,
      };

      let snapshot = browser.command(&["snapshot".into(), "-i".into()]).await?;
      std::fs::write(directory.join("agent-browser-initial.txt"), snapshot)?;
      target.browser = Some(browser);
    }

    let end = Instant::now() + Duration::from_secs(15);
    loop {
      let receipt = target.read()?;
      if kind == "swift" && receipt.get("window").is_some() {
        target.window = receipt["window"].clone();
        break;
      }

      if kind != "swift" && receipt.get("caseId").is_some() {
        let windows = control.request(json!({ "command": "windows", "pid": pid })).await?;
        if let Some(window) = windows["windows"].as_array().and_then(|values| values.iter().find(|v| v["layer"] == 0)) {
          target.window = window["id"].clone();
          break;
        }
      }

      ensure!(target.child.try_wait()?.is_none(), "receiver exited during startup");
      ensure!(Instant::now() < end, "receiver startup timed out");
      pause(30).await?;
    }

    Ok(target)
  }

  fn read(&self) -> Result<Value> {
    match &self.server {
      Some(server) => server.snapshot(),
      None => read_json(&self.directory.join("receipt.json")),
    }
  }

  async fn reset(&self, case: &str, initial: &str) -> Result<()> {
    match &self.server {
      Some(server) => server.reset(case, initial).await,
      None => {
        write_json(&self.directory.join("command.json"), &json!({ "caseId": case, "initial": initial }))?;
        wait_json(&self.directory.join("receipt.json"), |r| r["caseId"] == case && r["value"] == initial).await?;

        Ok(())
      }
    }
  }
}

async fn observe(control: &mut Process, target: &Target, anchor: &Path) -> Result<Value> {
  Ok(json!({
    "system": control.request(json!({ "command": "state" })).await?,
    "target": target.read()?,
    "anchor": read_json(&anchor.join("receipt.json"))?,
  }))
}

async fn restore(control: &mut Process, target: &Target, anchor_pid: u32, anchor_window: &Value) -> Result<Value> {
  control
    .request(json!({
      "command": "restore_records",
      "fromPid": target.pid,
      "fromWindow": target.window,
      "pid": anchor_pid,
      "window": anchor_window,
    }))
    .await
}

async fn run_target(
  options: &Options,
  kind: &str,
  root: &Path,
  control: &mut Process,
  producer: &mut Process,
  anchor_pid: u32,
  anchor_window: &Value,
) -> Result<Vec<Value>> {
  let anchor = root.join("anchor");
  let mut target = Target::launch(kind, options, &root.join(kind), root, control).await?;

  let result: Result<Vec<Value>> = async {
    control.request(json!({ "command": "track", "windows": [anchor_window, target.window] })).await?;

    let mut results = Vec::new();
    for repetition in 1..=options.repetitions {
      for mode in &options.mode {
        for case in focus_cases::CASES {
          let id = format!("{kind}-{mode}-{}-{repetition}", case.name);
          control.request(json!({ "command": "case", "id": id })).await?;

          // Remember the target's key window, then put our own anchor in front.
          control.request(json!({ "command": "activate", "pid": target.pid })).await?;
          pause(180).await?;
          control.request(json!({ "command": "activate", "pid": anchor_pid })).await?;
          pause(180).await?;

          target.reset(&id, case.initial).await?;
          write_json(&anchor.join("command.json"), &json!({ "caseId": id, "initial": "ANCHOR" }))?;
          wait_json(&anchor.join("receipt.json"), |r| r["caseId"] == id).await?;
          control.request(json!({ "command": "source", "id": ABC })).await?;

          let before = observe(control, &target, &anchor).await?;
          let mut record = json!({
            "caseId": id,
            "kind": kind,
            "mode": mode,
            "case": case.name,
            "expected": case.expected,
            "target_pid": target.pid,
            "anchor_pid": anchor_pid,
            "before": before,
            "startMs": now_ms(),
          });

          let trial: Result<()> = async {
            ensure!(before["system"]["workspaceFrontPid"] == anchor_pid, "anchor must be foreground");

            if mode.starts_with("no_raise") {
              record["preparation"] = control
                .request(json!({ "command": "no_raise", "pid": target.pid, "window": target.window }))
                .await?;
              pause(50).await?;

              if ["no_raise_key", "no_raise_key_menu"].contains(&mode.as_str()) {
                record["key_preparation"] = control
                  .request(json!({ "command": "make_key", "pid": target.pid, "window": target.window }))
                  .await?;
              }

              if mode == "no_raise_click" {
                record["click_preparation"] = invoke(
                  &options.desktop.auv,
                  &target.directory,
                  &[
                    "input.clickPoint".into(),
                    "200".into(),
                    "160".into(),
                    "--target".into(),
                    format!("window:{}", target.window),
                    "--relative-to".into(),
                    "window".into(),
                    "--input-policy".into(),
                    "background-only".into(),
                  ],
                )
                .await?;
              }
            } else if mode.starts_with("foreground") {
              record["preparation"] = control.request(json!({ "command": "activate", "pid": target.pid })).await?;
            }

            pause(if mode.starts_with("no_raise") { 50 } else { 180 }).await?;
            record["prepared"] = observe(control, &target, &anchor).await?;

            let mut responses = Vec::new();
            for command in case.commands {
              if mode == "no_raise_key_menu" && command[0] == "hold" {
                responses.push(control.request(json!({ "command": "menu_select_all", "pid": target.pid })).await?);
                pause(200).await?;
              } else {
                responses.push(
                  producer
                    .request(json!({
                      "command": command,
                      "window": target.window.to_string(),
                      "mode": if mode == "foreground" { "foreground" } else { "background" },
                    }))
                    .await?,
                );
              }
            }

            record["driver_results"] = json!(responses);
            pause(500).await?;
            record["delivered"] = observe(control, &target, &anchor).await?;

            if let Some(browser) = &mut target.browser {
              let value = browser
                .command(&[
                  "eval".into(),
                  "JSON.stringify({value:document.querySelector('textarea').value,selection:[document.querySelector('textarea').selectionStart,document.querySelector('textarea').selectionEnd],documentFocused:document.hasFocus()})".into(),
                ])
                .await?;
              let decoded: String = serde_json::from_str(&value)?;
              record["agent_browser_receipt"] = serde_json::from_str(&decoded)?;
              record["agent_browser"] = json!(value);

              if repetition == 1 {
                browser
                  .command(&["screenshot".into(), path(&target.directory.join(format!("{mode}-{}.png", case.name)))])
                  .await?;
              }
            }

            // Conditional restore intentionally models the original CUA wrapper.
            // Explicit records address the actual receiver even if front PID never changed.
            let current = control.request(json!({ "command": "state" })).await?["workspaceFrontPid"].clone();
            record["restore_attempted"] = json!(current == target.pid);
            record["restore_mode"] = json!(options.restore);

            if options.restore == "records" && mode.starts_with("no_raise") {
              record["restore_attempted"] = json!(true);
              record["restore_result"] = restore(control, &target, anchor_pid, anchor_window).await?;
            } else if current == target.pid {
              control.request(json!({ "command": "activate", "pid": anchor_pid })).await?;
            }

            pause(200).await?;
            record["restored"] = observe(control, &target, &anchor).await?;
            record["input_source_after"] = control.request(json!({ "command": "source" })).await?["id"].clone();
            record["endMs"] = json!(now_ms());

            let log = std::fs::read_to_string(root.join("system-state.jsonl"))?;
            let mut during: Vec<Value> = log.lines().map(serde_json::from_str).collect::<std::result::Result<Vec<_>, _>>()?;
            during.retain(|v| {
              v["timeMs"].as_f64().is_some_and(|time| {
                time >= record["startMs"].as_f64().unwrap() && time <= record["endMs"].as_f64().unwrap()
              })
            });

            for stage in ["prepared", "delivered", "restored"] {
              during.push(record[stage]["system"].clone());
            }

            record["checks"] = focus_cases::check(&record, &during);

            Ok(())
          }
          .await;

          if let Err(error) = trial {
            record["error"] = json!(error.to_string());
          }

          println!("{id} {} {}", record["checks"], record["error"]);
          results.push(record);
          write_json(&target.directory.join("results.json"), &results)?;

          // Rescue even a partially applied focus pair outside the measured verdict.
          producer.request(json!({ "command": ["drop"] })).await?;
          restore(control, &target, anchor_pid, anchor_window).await?;
          control.request(json!({ "command": "activate", "pid": anchor_pid })).await?;
          pause(80).await?;
        }
      }
    }

    Ok(results)
  }
  .await;

  let cleanup = restore(control, &target, anchor_pid, anchor_window).await;
  let browser_cleanup = if let Some(browser) = &mut target.browser {
    browser.close().await
  } else {
    Ok(())
  };

  stop(&mut target.child).await;

  let results = result?;
  cleanup?;
  browser_cleanup?;

  Ok(results)
}

async fn run(mut options: Options) -> Result<bool> {
  if options.mode.is_empty() {
    options.mode = vec!["baseline".into(), "no_raise".into(), "foreground".into()];
  }

  if options.receiver.is_empty() {
    options.receiver = vec!["swift".into(), "electron".into(), "chrome".into()];
  }

  let root = new_output(&options.desktop.output)?;
  compile(&macos_root().join("tasks/native/focus_controller.swift"), &root.join("controller")).await?;
  compile(&macos_root().join("test-objects/appkit/focus.swift"), &root.join("receiver")).await?;

  let mut control = Process::spawn(&root.join("controller"), &[path(&root.join("system-state.jsonl"))], &root.join("controller.log"))?;
  let previous_app = control.request(json!({ "command": "state" })).await?["workspaceFrontPid"].clone();
  let previous_source = control.request(json!({ "command": "source" })).await?["id"].clone();
  let mut anchor_process = None;
  let mut producer = None;

  let result: Result<bool> = async {
    let anchor = root.join("anchor");
    std::fs::create_dir(&anchor)?;

    let app = build_app(&root.join("receiver"), &root, "AUVFocusAnchor")?;
    let child = launch(&app, &[path(&anchor), "AUV Focus Anchor".into()], &anchor.join("process.log"))?;
    let pid = child.id().context("anchor PID missing")?;
    anchor_process = Some(child);
    let window = wait_json(&anchor.join("receipt.json"), |r| r.get("window").is_some()).await?["window"].clone();

    producer = Some(Process::spawn(&options.sender, &[], &root.join("sender.log"))?);
    control.request(json!({ "command": "source", "id": ABC })).await?;

    let mut results = Vec::new();
    for kind in &options.receiver {
      results.extend(run_target(&options, kind, &root, &mut control, producer.as_mut().unwrap(), pid, &window).await?);
      write_json(&root.join("results.json"), &results)?;
    }

    let mut groups = json!({});
    for kind in &options.receiver {
      for mode in &options.mode {
        for case in focus_cases::CASES {
          groups[kind][mode][case.name] = json!(
            results
              .iter()
              .filter(|r| r["kind"] == *kind && r["mode"] == *mode && r["case"] == case.name)
              .map(|r| r.get("checks").cloned().unwrap_or_else(|| json!({ "error": r["error"] })))
              .collect::<Vec<_>>()
          );
        }
      }
    }

    write_json(
      &root.join("summary.json"),
      &json!({
        "repetitions": options.repetitions,
        "restore_mode": options.restore,
        "results": groups,
      }),
    )?;

    Ok(!options.require_success || results.iter().all(focus_cases::passed))
  }
  .await;

  if let Some(producer) = &mut producer {
    producer.close().await;
  }

  if let Some(anchor) = &mut anchor_process {
    stop(anchor).await;
  }

  let restore_app = control.request(json!({ "command": "activate", "pid": previous_app })).await;
  let restore_source = control.request(json!({ "command": "source", "id": previous_source })).await;
  control.close().await;

  let passed = result?;
  restore_app?;
  restore_source?;

  Ok(passed)
}
