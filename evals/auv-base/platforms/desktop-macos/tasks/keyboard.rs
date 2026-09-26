//! Launch isolated receivers and drive the CLI or persistent public Rust sender.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde_json::{Value, json};
use tokio::time::Instant;

use auv_base_evals::{
  macos::{ABC, DesktopOptions, compile, keyboard_cases, root as macos_root},
  receiver::Receiver,
  support::{self, Process, find_window, invoke, launch, new_output, now_ms, objects, output, path, pause, stop, write_json},
};

#[derive(Parser)]
#[command(about = "Test complete presses or persistent held-key input")]
struct Options {
  #[command(flatten)]
  desktop: DesktopOptions,

  #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
  repetitions: u32,

  /// Persistent sender; selects held-key scenarios instead of CLI scenarios.
  #[arg(long)]
  sender: Option<PathBuf>,

  #[arg(long = "case")]
  selected_cases: Vec<String>,

  #[arg(long, default_value = "background", value_parser = ["background", "foreground"])]
  mode: String,

  #[arg(long, default_value = ABC)]
  input_source: String,

  #[arg(long, value_parser = ["chrome", "electron"])]
  receiver: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<std::process::ExitCode> {
  ensure!(cfg!(target_os = "macos"), "desktop-macos evaluations require macOS");
  support::run(run(Options::parse())).await
}

async fn frontmost(focus: &Path) -> Result<u32> {
  Ok(output(focus, &[]).await?.parse()?)
}

async fn run_receiver(kind: &str, options: &Options, directory: &Path, focus: &Path, cases: &[keyboard_cases::Case]) -> Result<Vec<Value>> {
  std::fs::create_dir(directory)?;
  let profile = tempfile::Builder::new().prefix("auv-keyboard-").tempdir()?;
  let server = Receiver::start(None).await?;

  let previous_app = frontmost(focus).await?;
  let previous_source = output(focus, &["source".into()]).await?;

  let title = format!("AUV keyboard {kind} {}", now_ms());
  let url = server.url(&title, false);
  let command = if kind == "chrome" {
    vec![
      format!("--user-data-dir={}", profile.path().display()),
      "--no-first-run".into(),
      "--no-default-browser-check".into(),
      "--disable-background-networking".into(),
      "--disable-sync".into(),
      format!("--app={url}"),
    ]
  } else {
    vec![
      path(&objects().join("dist/electron/keyboard.js")),
      url,
      path(profile.path()),
      path(directory),
    ]
  };
  let executable = if kind == "chrome" {
    &options.desktop.chrome
  } else {
    &options.desktop.electron
  };

  let mut process = launch(executable, &command, &directory.join("process.log"))?;
  let pid = process.id().context("receiver has no process ID")?;
  let mut producer = None;

  let result: Result<Vec<Value>> = async {
    let end = Instant::now() + Duration::from_secs(20);
    let window = loop {
      ensure!(process.try_wait()?.is_none(), "receiver exited; see process.log");
      if server.snapshot()?.get("caseId").is_some()
        && let Some(window) = find_window(&invoke(&options.desktop.auv, directory, &["window.list".into()]).await?, &title)
      {
        break window;
      }

      ensure!(Instant::now() < end, "isolated receiver window not found");
      pause(100).await?;
    };
    ensure!(window["process_id"] == pid, "must address only the fixture process");

    let foreground = options.mode == "foreground";
    let foreground_pid = if foreground { pid } else { previous_app };
    output(focus, &[foreground_pid.to_string()]).await?;
    output(focus, &["source".into(), options.input_source.clone()]).await?;
    pause(500).await?;

    let window_id = window["reference"]["id"].as_str().context("window ID missing")?;
    if let Some(sender) = &options.sender {
      producer = Some(Process::spawn(sender, &[], &directory.join("sender.log"))?);
    }

    let mut results = Vec::new();
    for repetition in 1..=options.repetitions {
      for case in cases {
        let id = format!("{}-{repetition}", case.name);
        let mut record = json!({
          "case": case.name,
          "repetition": repetition,
          "expected": case.expected,
          "commands": case.commands,
          "started_ms": now_ms(),
        });

        let trial: Result<()> = async {
          // Re-establish fixture preconditions after human app switches; receipt
          // checks still distinguish a switch during the actual input interval.
          let current = frontmost(focus).await?;
          if (!foreground && current == pid) || (foreground && current != pid) {
            output(focus, &[foreground_pid.to_string()]).await?;
            pause(200).await?;
          }

          server.reset(&id, case.initial).await?;

          let source = output(focus, &["source".into()]).await?;
          ensure!(source == options.input_source, "keyboard input source changed during test");

          let before = frontmost(focus).await?;
          ensure!((before == pid) == foreground, "receiver has wrong foreground/background state");

          let mut responses = Vec::new();
          let mut intermediate = Vec::new();
          for command in &case.commands {
            if command[0] == "wait" {
              pause(command[1].parse()?).await?;
              intermediate.push(server.snapshot()?);
            } else if let Some(producer) = &mut producer {
              responses.push(producer.request(json!({ "command": command, "window": window_id, "mode": options.mode })).await?);
            } else {
              let mut args: Vec<_> = command.iter().map(|v| (*v).to_owned()).collect();
              args.extend([
                "--target".into(),
                format!("window:{window_id}"),
                "--input-policy".into(),
                if foreground {
                  "foreground-preferred"
                } else {
                  "background-only"
                }
                .into(),
              ]);
              responses.push(invoke(&options.desktop.auv, directory, &args).await?);
            }
          }

          // Observe asynchronous receipt and late duplicates; this does not add
          // dwell inside the driver's native key sequence.
          pause(800).await?;
          record["receipt"] = server.snapshot()?;
          record["foreground_before"] = json!(before);
          record["foreground_after"] = json!(frontmost(focus).await?);
          record["input_source"] = json!(source);
          record["driver_results"] = json!(responses);
          record["intermediate"] = json!(intermediate);

          let checks = keyboard_cases::check(case, &record, foreground)?;
          record["passed"] = json!(checks.as_object().context("checks missing")?.values().all(|v| v == true));
          record["checks"] = checks;

          Ok(())
        }
        .await;

        if let Err(error) = trial {
          record["passed"] = json!(false);
          record["error"] = json!(error.to_string());
          record["receipt"] = server.snapshot()?;
        }

        if let Some(producer) = &mut producer {
          producer.request(json!({ "command": ["drop"], "window": window_id, "mode": options.mode })).await?;
        }

        record["finished_ms"] = json!(now_ms());
        let verdict = if record.get("error").is_some() || record["checks"]["focus"] == false {
          "INVALID"
        } else if record["passed"] == true {
          "PASS"
        } else {
          "FAIL"
        };

        println!("{kind} {id} {verdict} {}", record["receipt"]["value"]);
        results.push(record);
        write_json(&directory.join("results.json"), &results)?;
        pause(0).await?;
      }
    }

    Ok(results)
  }
  .await;

  if let Some(producer) = &mut producer {
    producer.close().await;
  }

  stop(&mut process).await;

  // Always attempt both restores, including when a trial or launch phase failed.
  let restore_app = output(focus, &[previous_app.to_string()]).await;
  let restore_source = output(focus, &["source".into(), previous_source]).await;

  let results = result?;
  restore_app?;
  restore_source?;

  Ok(results)
}

async fn run(options: Options) -> Result<bool> {
  let mut cases = keyboard_cases::cases(options.sender.is_some());
  for name in &options.selected_cases {
    ensure!(cases.iter().any(|c| c.name == name), "unknown case: {name}");
  }

  if !options.selected_cases.is_empty() {
    cases.retain(|c| options.selected_cases.iter().any(|name| name == c.name));
  }

  let directory = new_output(&options.desktop.output)?;
  let focus = directory.join("focus");
  compile(&macos_root().join("tasks/native/focus.swift"), &focus).await?;

  let chrome_info = options.desktop.chrome.parent().and_then(Path::parent).context("Chrome bundle path missing")?.join("Info.plist");
  let mut summary = json!({
    "platform": output(Path::new("sw_vers"), &[]).await?,
    "chrome_version": output(
      Path::new("plutil"),
      &["-extract".into(), "CFBundleShortVersionString".into(), "raw".into(), path(&chrome_info)],
    ).await?,
    "mode": options.mode,
    "input_source": options.input_source,
    "suite": if options.sender.is_some() { "held_keys" } else { "cli_keyboard" },
    "repetitions": options.repetitions,
    "results": {},
  });

  let kinds = if options.receiver.is_empty() {
    vec!["chrome".into(), "electron".into()]
  } else {
    options.receiver.clone()
  };

  let mut passed = true;
  for kind in kinds {
    let results = run_receiver(&kind, &options, &directory.join(&kind), &focus, &cases).await?;
    let mut totals = json!({});
    for case in &cases {
      let rows: Vec<_> = results.iter().filter(|r| r["case"] == case.name).collect();
      let successes = rows.iter().filter(|r| r["passed"] == true).count();
      passed &= successes == options.repetitions as usize;

      totals[case.name] = json!({
        "passed": successes,
        "invalid": rows.iter().filter(|r| r.get("error").is_some() || r["checks"]["focus"] == false).count(),
        "total": options.repetitions,
      });
    }

    summary["results"][&kind] = totals;
  }

  write_json(&directory.join("summary.json"), &summary)?;

  Ok(passed)
}
