// File: src/cli.rs
use auv_cli_invoke::InvokeCliParse;
use auv_runtime::model::{AuvResult, ExecutionTarget, InvokeRequest};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TracingOptions {
  pub store_root: Option<String>,
}

#[derive(Debug)]
pub enum CliCommand {
  Help,
  Version,
  PermissionCheck {
    json: bool,
  },
  ListCommandsTombstone,
  InvokeHelp {
    command_id: Option<String>,
  },
  MinecraftHelp,
  OsuHelp,
  GodotHelp,
  GodotCapabilityQuery {
    json: bool,
  },
  GodotRenderObserve {
    output_dir: String,
    stages: Vec<String>,
    json: bool,
  },
  OsuBenchmark {
    beatmap_path: String,
    output_dir: Option<String>,
  },
  OsuBenchmarkDispatch {
    beatmap_path: String,
    target_app: String,
    output_dir: Option<String>,
    dispatch_limit: Option<usize>,
    capture_verify: bool,
  },
  OsuExportDataset {
    run_artifact_dir: String,
    output_dir: String,
  },
  OsuEvalDetections {
    run_artifact_dir: String,
    detections_path: String,
    output_dir: Option<String>,
  },
  OsuVisionDemo {
    beatmap_path: String,
    target_app: String,
    output_dir: Option<String>,
    dispatch_limit: Option<usize>,
    capture_verify: bool,
  },
  MinecraftProjectionBridge {
    telemetry_sample: String,
    screenshot: Option<String>,
    capture_target_app: Option<String>,
    capture_target_title: Option<String>,
    target_block: String,
    capture_skew_ms: Option<i64>,
    screenshot_is_minecraft_window: bool,
    tracing: TracingOptions,
  },
  MinecraftCalibrateProjection {
    frame_path: String,
    screenshot: String,
    target_block: String,
    target_semantics: String,
    screenshot_is_minecraft_window: bool,
    tracing: TracingOptions,
  },
  MinecraftLiveClick {
    telemetry_sample: String,
    screenshot: String,
    target_block: String,
    target_app: String,
    target_title: String,
    post_telemetry_sample: Option<String>,
    capture_skew_ms: Option<i64>,
    screenshot_is_minecraft_window: bool,
    tracing: TracingOptions,
  },
  MinecraftExport3dgsScenePacket {
    bundle_manifest_paths: Vec<String>,
    output_dir: String,
    tracing: TracingOptions,
  },
  MinecraftPrepareTextureSweep {
    sidecar_run_dir: String,
    output_dir: String,
    tracing: TracingOptions,
  },
  MinecraftBuildTextureSweepSamples {
    bundle_manifest_paths: Vec<String>,
    output_path: String,
    tracing: TracingOptions,
  },
  MinecraftEvalTextureSweep {
    samples_path: String,
    output_dir: String,
    require_real_source: bool,
    tracing: TracingOptions,
  },
  Invoke {
    request: InvokeRequest,
    tracing: TracingOptions,
    output: auv_cli_invoke::InvokeOutputOptions,
  },
  SessionServe {
    host: String,
    port: u16,
    store_root: Option<String>,
  },
  McpServe,
  XtaskGenerateSwiftBridge,
}

pub fn parse_cli(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.is_empty() {
    return Ok(CliCommand::Help);
  }

  if root_version_requested(arguments) {
    return Ok(CliCommand::Version);
  }

  match arguments[0].as_str() {
    "help" | "--help" | "-h" => Ok(CliCommand::Help),
    "--version" | "-V" => Err("usage: auv --version".to_string()),
    "doctor" => parse_permission_check(arguments),
    "permissions" => parse_permissions(arguments),
    "--xtask" => parse_xtask(arguments),
    "list-commands" => Ok(CliCommand::ListCommandsTombstone),
    "godot" => parse_godot(arguments),
    "osu" => parse_osu(arguments),
    "inspect" => Err("`auv inspect` has been retired; the replacement inspector read-side is intentionally deferred".to_string()),
    "session" => parse_session(arguments),
    "mcp" => parse_mcp(arguments),
    "invoke" => parse_invoke(arguments),
    "minecraft" => parse_minecraft(arguments),
    "skill" => Err("skill commands have been removed; use app-local Rust commands instead".to_string()),
    other => Err(format!("unknown subcommand {other}; use `help` to see supported commands")),
  }
}

/// Returns whether root `auv` can print its version before creating an async runtime.
pub fn root_version_requested(arguments: &[String]) -> bool {
  matches!(arguments, [flag] if matches!(flag.as_str(), "--version" | "-V"))
}

/// Parse donor bin argv (`capability-query …`), used by `auv-godot` / `auv-osu` / `auv-minecraft`.
pub fn parse_donor_cli(donor: &str, arguments: &[String]) -> AuvResult<CliCommand> {
  let mut full = Vec::with_capacity(arguments.len() + 1);
  full.push(donor.to_string());
  full.extend(arguments.iter().cloned());
  match donor {
    "godot" => parse_godot(&full),
    "osu" => parse_osu(&full),
    "minecraft" => parse_minecraft(&full),
    other => Err(format!("unknown donor bin {other}")),
  }
}

/// Rejects app-specific subcommands at the root binary while standalone app
/// binaries continue to reuse the same parsers.
pub fn root_donor_tombstone(arguments: &[String]) -> Option<String> {
  match arguments.first().map(String::as_str) {
    Some("godot") => Some("`auv godot` has been removed; use `auv-godot` instead".to_string()),
    Some("osu") => Some("`auv osu` has been removed; use `auv-osu` instead".to_string()),
    Some("minecraft") => Some("`auv minecraft` has been removed; use `auv-minecraft` instead".to_string()),
    _ => None,
  }
}

fn parse_xtask(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() != 2 {
    return Err("usage: auv --xtask generate-swift-bridge".to_string());
  }

  match arguments[1].as_str() {
    "generate-swift-bridge" => Ok(CliCommand::XtaskGenerateSwiftBridge),
    other => Err(format!("unknown xtask {other}; supported xtasks: generate-swift-bridge")),
  }
}

pub fn help_text() -> String {
  String::from(
    "\
  auv prototype

USAGE
  auv --version
  auv doctor [--json]
  auv permissions check [--json]
  auv-godot … (see `auv-godot --help`)
  auv-osu … (see `auv-osu --help`)
  auv-minecraft … (see `auv-minecraft --help`)
  auv invoke <command-id> [--dry-run] [--target <application-id>] [--label <text>] [--store-root <path>]
  auv session serve [--host <host>] [--port <port>] [--store-root <path>]
  auv mcp serve

NOTES
  - Names are provisional and reflect the current phase-0/1 runtime skeleton.
  - The CLI is a thin frontend over the library runtime in src/lib.rs.
  - Donor game CLIs live in `auv-minecraft` / `auv-osu` / `auv-godot` (root `auv minecraft|osu|godot` is a tombstone).
  - `invoke --help` is the discovery surface for canonical invoke commands in the current C1 scaffold.
  - `list-commands` has been retired; use `auv invoke --help` instead.
  - `overlay.showCursor`, `overlay.hideCursor`, and `overlay.shutdown` are visual-only macOS overlay probes; standalone `invoke` calls run in separate Rust processes, so use `--hold_ms` on show when manually observing the overlay.
  - `window.captureAxTree`, `input.focusText`, and `input.pressButton` accept `--reveal_shortcut cmd+f`-style hints when an app hides the target UI until a keyboard shortcut reveals it.
  - `--reveal_settle_ms <millis>` can be used to make the reveal step explicit instead of depending on hard-coded timing assumptions.
  - `input.typeText` supports `--replace_existing true`, `--submit_key return`, and `--submit_settle_ms 800` for repeatable text-entry flows.
  - `input.key` supports both special keys like `Return` and shortcuts like `cmd+f`, with optional `--settle_ms`.
  - `input.clickWindowPoint` accepts either `--offset_x/--offset_y` or `--relative_x/--relative_y` against the target window bounds.
  - `input.teachClick` captures a target window before a human-taught click, opens a small Ready prompt, records the next click as global/window-local coordinates, then captures follow-up frames at `--first_after_ms` and `--second_after_ms` (defaults 150/250).
  - `screen.findText` and `screen.clickText` use macOS Vision OCR over a captured screenshot and operate in screenshot-pixel anchors projected back to logical points.
  - `screen.waitForText` polls that same OCR path until a filtered anchor appears or the timeout expires; use it when result-page readiness is the real problem instead of guessing longer sleeps.
  - `screen.findRows`, `screen.waitForRows`, and `screen.clickRow` treat OCR observations as grouped visible rows, which is the current fallback direction when exact text anchors are visually present but not OCR-reliable.
  - `screen.findImageText` runs the same OCR matching over an existing image artifact, which is useful for verifying captured evidence without recapturing the live desktop.
  - `mediaControl.nowPlaying` prefers AX tree matching for player-title verification, which is the current direction for native playback disambiguation.
  - `window.verifyText` is the generic AX-tree text verification contract for native apps with reliable text-bearing nodes.
  - `screen.clickText` supports `--match_index` and `--click_count` when the query resolves to multiple OCR anchors.
",
  )
}

pub fn version_text() -> String {
  format!("auv {}\n", env!("CARGO_PKG_VERSION"))
}

fn parse_permission_check(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut json = false;
  for argument in arguments.iter().skip(1) {
    match argument.as_str() {
      "--json" => json = true,
      other => {
        return Err(format!("unknown doctor option {other}; usage: auv doctor [--json]"));
      }
    }
  }

  Ok(CliCommand::PermissionCheck { json })
}

fn parse_permissions(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv permissions check [--json]".to_string());
  }

  match arguments[1].as_str() {
    "check" => {
      let mut normalized = vec!["doctor".to_string()];
      normalized.extend(arguments.iter().skip(2).cloned());
      parse_permission_check(&normalized)
    }
    other => Err(format!("unknown permissions subcommand {other}; usage: auv permissions check [--json]")),
  }
}

fn parse_godot(arguments: &[String]) -> AuvResult<CliCommand> {
  if parse_help_only_invocation(arguments, "godot")? {
    return Ok(CliCommand::GodotHelp);
  }

  match arguments.get(1).map(String::as_str) {
    Some("capability-query") | Some("capabilities") => {
      let mut json = false;
      for argument in &arguments[2..] {
        match argument.as_str() {
          "--json" => json = true,
          other => {
            return Err(format!("unknown godot capability-query option {other}; expected --json"));
          }
        }
      }

      Ok(CliCommand::GodotCapabilityQuery { json })
    }
    Some("render-observe") => parse_godot_render_observe(arguments),
    Some(other) => {
      Err(format!("unknown godot subcommand {other}; supported subcommands: capability-query, render-observe; use `auv-godot --help`"))
    }
    None => unreachable!("help-only godot invocations return before subcommand match"),
  }
}

fn parse_godot_render_observe(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut output_dir = None;
  let mut stages = Vec::new();
  let mut json = false;
  let mut index = 2;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--output-dir" => {
        index += 1;
        if index >= arguments.len() {
          return Err("missing value for --output-dir".to_string());
        }
        output_dir = Some(arguments[index].clone());
      }
      "--stage" => {
        index += 1;
        if index >= arguments.len() {
          return Err("missing value for --stage".to_string());
        }
        stages.push(arguments[index].clone());
      }
      "--json" => json = true,
      other => {
        return Err(format!("unknown godot render-observe option {other}; expected --output-dir, --stage, or --json"));
      }
    }
    index += 1;
  }

  Ok(CliCommand::GodotRenderObserve {
    output_dir: output_dir.ok_or_else(|| format!("usage: {}", crate::integrations::godot::help::render_observe_usage_line()))?,
    stages,
    json,
  })
}

fn parse_help_only_invocation(arguments: &[String], command: &str) -> AuvResult<bool> {
  let help_hint = match command {
    "minecraft" | "osu" | "godot" => format!("auv-{command} --help"),
    other => format!("auv {other} --help"),
  };
  match arguments.get(1).map(String::as_str) {
    None => Ok(true),
    Some("help") | Some("--help") | Some("-h") => {
      if arguments.len() == 2 {
        Ok(true)
      } else {
        let extra = arguments[2..].join(" ");
        Err(format!("unexpected {command} help argument(s) {extra:?}; use `{help_hint}`"))
      }
    }
    _ => Ok(false),
  }
}

fn parse_osu(arguments: &[String]) -> AuvResult<CliCommand> {
  if parse_help_only_invocation(arguments, "osu")? {
    return Ok(CliCommand::OsuHelp);
  }

  match arguments.get(1).map(String::as_str) {
    Some("benchmark") => parse_osu_benchmark(arguments),
    Some("dispatch") => parse_osu_dispatch(arguments),
    Some("export-dataset") => parse_osu_export_dataset(arguments),
    Some("eval-detections") => parse_osu_eval_detections(arguments),
    Some("vision-demo") => parse_osu_vision_demo(arguments),
    Some(other) => Err(format!("unknown osu subcommand {other}; use `auv-osu --help` for full usage")),
    None => unreachable!("help-only osu invocations return before subcommand match"),
  }
}

fn parse_osu_benchmark(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv-osu benchmark <beatmap.osu> [--output-dir <dir>]".to_string());
  }

  let beatmap_path = arguments[2].clone();
  let mut output_dir = None;
  let mut index = 3;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--output-dir" => {
        if index + 1 >= arguments.len() {
          return Err("--output-dir requires a value".to_string());
        }
        output_dir = Some(arguments[index + 1].clone());
        index += 2;
      }
      other => {
        return Err(format!("unexpected osu-benchmark argument {other}"));
      }
    }
  }

  Ok(CliCommand::OsuBenchmark {
    beatmap_path,
    output_dir,
  })
}

fn parse_osu_dispatch(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err(
      "usage: auv-osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]".to_string(),
    );
  }

  let beatmap_path = arguments[2].clone();
  let mut target_app = None;
  let mut output_dir = None;
  let mut dispatch_limit = None;
  let mut capture_verify = false;
  let mut index = 3;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--target-app" => {
        if index + 1 >= arguments.len() {
          return Err("--target-app requires a value".to_string());
        }
        target_app = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--output-dir" => {
        if index + 1 >= arguments.len() {
          return Err("--output-dir requires a value".to_string());
        }
        output_dir = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--dispatch-limit" => {
        if index + 1 >= arguments.len() {
          return Err("--dispatch-limit requires a value".to_string());
        }
        dispatch_limit = Some(arguments[index + 1].parse::<usize>().map_err(|error| format!("invalid --dispatch-limit: {error}"))?);
        index += 2;
      }
      "--capture-verify" => {
        capture_verify = true;
        index += 1;
      }
      other => return Err(format!("unexpected osu-dispatch argument {other}")),
    }
  }

  let target_app = target_app.ok_or_else(|| "--target-app is required".to_string())?;

  Ok(CliCommand::OsuBenchmarkDispatch {
    beatmap_path,
    target_app,
    output_dir,
    dispatch_limit,
    capture_verify,
  })
}

fn parse_osu_export_dataset(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err("usage: auv-osu export-dataset <run-artifact-dir> --output-dir <dir>".to_string());
  }

  let run_artifact_dir = arguments[2].clone();
  let mut output_dir = None;
  let mut index = 3;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--output-dir" => {
        if index + 1 >= arguments.len() {
          return Err("--output-dir requires a value".to_string());
        }
        output_dir = Some(arguments[index + 1].clone());
        index += 2;
      }
      other => return Err(format!("unexpected osu-export-dataset argument {other}")),
    }
  }

  Ok(CliCommand::OsuExportDataset {
    run_artifact_dir,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
  })
}

fn parse_osu_eval_detections(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err("usage: auv-osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]".to_string());
  }

  let run_artifact_dir = arguments[2].clone();
  let mut detections_path = None;
  let mut output_dir = None;
  let mut index = 3;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--detections" => {
        if index + 1 >= arguments.len() {
          return Err("--detections requires a value".to_string());
        }
        detections_path = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--output-dir" => {
        if index + 1 >= arguments.len() {
          return Err("--output-dir requires a value".to_string());
        }
        output_dir = Some(arguments[index + 1].clone());
        index += 2;
      }
      other => return Err(format!("unexpected osu-eval-detections argument {other}")),
    }
  }

  Ok(CliCommand::OsuEvalDetections {
    run_artifact_dir,
    detections_path: detections_path.ok_or_else(|| "--detections is required".to_string())?,
    output_dir,
  })
}

fn parse_osu_vision_demo(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err(
      "usage: auv-osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]"
        .to_string(),
    );
  }

  let beatmap_path = arguments[2].clone();
  let mut target_app = None;
  let mut output_dir = None;
  let mut dispatch_limit = None;
  let mut capture_verify = false;
  let mut index = 3;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--target-app" => {
        if index + 1 >= arguments.len() {
          return Err("--target-app requires a value".to_string());
        }
        target_app = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--output-dir" => {
        if index + 1 >= arguments.len() {
          return Err("--output-dir requires a value".to_string());
        }
        output_dir = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--dispatch-limit" => {
        if index + 1 >= arguments.len() {
          return Err("--dispatch-limit requires a value".to_string());
        }
        dispatch_limit = Some(arguments[index + 1].parse::<usize>().map_err(|error| format!("invalid --dispatch-limit: {error}"))?);
        index += 2;
      }
      "--capture-verify" => {
        capture_verify = true;
        index += 1;
      }
      other => return Err(format!("unexpected osu-vision-demo argument {other}")),
    }
  }

  Ok(CliCommand::OsuVisionDemo {
    beatmap_path,
    target_app: target_app.ok_or_else(|| "--target-app is required".to_string())?,
    output_dir,
    dispatch_limit,
    capture_verify,
  })
}

fn parse_tracing_option(argument: &str, value: Option<&String>, tracing: &mut TracingOptions) -> AuvResult<Option<usize>> {
  match argument {
    "--store-root" => {
      let value = value.ok_or_else(|| "--store-root requires a value".to_string())?;
      tracing.store_root = Some(value.clone());
      Ok(Some(2))
    }
    _ => Ok(None),
  }
}

fn parse_invoke(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut tracing = TracingOptions::default();
  let mut invoke_arguments = Vec::with_capacity(arguments.len());
  let mut index = 0;

  if let Some(subcommand) = arguments.first() {
    invoke_arguments.push(subcommand.clone());
    index = 1;
  }

  if let Some(command_or_help) = arguments.get(index) {
    invoke_arguments.push(command_or_help.clone());
    index += 1;
  }

  while index < arguments.len() {
    let argument = arguments[index].as_str();
    if let Some(consumed) = parse_tracing_option(argument, arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    invoke_arguments.push(arguments[index].clone());
    if !auv_cli_invoke::invoke_argument_consumes_value(argument) {
      index += 1;
      continue;
    }

    if let Some(value) = arguments.get(index + 1) {
      invoke_arguments.push(value.clone());
      index += 2;
      continue;
    }

    index += 1;
  }

  match auv_cli_invoke::parse_invoke_args(&invoke_arguments)? {
    InvokeCliParse::Help { command_id } => Ok(CliCommand::InvokeHelp { command_id }),
    InvokeCliParse::Invoke {
      command_id,
      target_application_id,
      inputs,
      dry_run,
      output,
    } => Ok(CliCommand::Invoke {
      request: InvokeRequest {
        command_id,
        target: ExecutionTarget {
          application_id: target_application_id,
        },
        inputs,
        dry_run,
      },
      tracing,
      output,
    }),
  }
}

fn parse_minecraft(arguments: &[String]) -> AuvResult<CliCommand> {
  if parse_help_only_invocation(arguments, "minecraft")? {
    return Ok(CliCommand::MinecraftHelp);
  }

  match arguments.get(1).map(String::as_str) {
    Some("bridge") => parse_minecraft_bridge(arguments),
    Some("calibrate-projection") => parse_minecraft_calibrate_projection(arguments),
    Some("live-click") => parse_minecraft_live_click(arguments),
    Some("export-spatial-bundle") => Err(
      "`export-spatial-bundle` has been retired with the legacy run read-side; a replacement requires an approved inspector contract"
        .to_string(),
    ),
    Some("export-3dgs-scene-packet") => parse_minecraft_export_3dgs_scene_packet(arguments),
    Some("prepare-texture-sweep") => parse_minecraft_prepare_texture_sweep(arguments),
    Some("build-texture-sweep-samples") => parse_minecraft_build_texture_sweep_samples(arguments),
    Some("eval-texture-sweep") => parse_minecraft_eval_texture_sweep(arguments),
    Some(other) => Err(format!("unknown minecraft subcommand {other}; use `auv-minecraft --help` for full usage")),
    None => unreachable!("help-only minecraft invocations return before subcommand match"),
  }
}

fn parse_minecraft_export_3dgs_scene_packet(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut bundle_manifest_paths = Vec::new();
  let mut output_dir = None;
  let mut tracing = TracingOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--bundle-manifest" => {
        bundle_manifest_paths.push(required_flag_value(arguments, index, "--bundle-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft export-3dgs-scene-packet argument {other}"));
      }
    }
  }
  if bundle_manifest_paths.is_empty() {
    return Err("--bundle-manifest is required".to_string());
  }

  Ok(CliCommand::MinecraftExport3dgsScenePacket {
    bundle_manifest_paths,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    tracing,
  })
}

fn parse_minecraft_prepare_texture_sweep(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut sidecar_run_dir = None;
  let mut output_dir = None;
  let mut tracing = TracingOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--sidecar-run-dir" => {
        sidecar_run_dir = Some(required_flag_value(arguments, index, "--sidecar-run-dir")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft prepare-texture-sweep argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftPrepareTextureSweep {
    sidecar_run_dir: sidecar_run_dir.ok_or_else(|| "--sidecar-run-dir is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    tracing,
  })
}

fn parse_minecraft_build_texture_sweep_samples(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut bundle_manifest_paths = Vec::new();
  let mut output_path = None;
  let mut tracing = TracingOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--bundle-manifest" => {
        bundle_manifest_paths.push(required_flag_value(arguments, index, "--bundle-manifest")?);
        index += 2;
      }
      "--output" => {
        output_path = Some(required_flag_value(arguments, index, "--output")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft build-texture-sweep-samples argument {other}"));
      }
    }
  }
  if bundle_manifest_paths.is_empty() {
    return Err("--bundle-manifest is required".to_string());
  }

  Ok(CliCommand::MinecraftBuildTextureSweepSamples {
    bundle_manifest_paths,
    output_path: output_path.ok_or_else(|| "--output is required".to_string())?,
    tracing,
  })
}

fn parse_minecraft_eval_texture_sweep(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut samples_path = None;
  let mut output_dir = None;
  let mut require_real_source = false;
  let mut tracing = TracingOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--samples" => {
        samples_path = Some(required_flag_value(arguments, index, "--samples")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--require-real-source" => {
        require_real_source = true;
        index += 1;
      }
      other => {
        return Err(format!("unexpected minecraft eval-texture-sweep argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftEvalTextureSweep {
    samples_path: samples_path.ok_or_else(|| "--samples is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    require_real_source,
    tracing,
  })
}

fn parse_minecraft_bridge(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut telemetry_sample = None;
  let mut screenshot = None;
  let mut capture_target_app = None;
  let mut capture_target_title = None;
  let mut target_block = None;
  let mut capture_skew_ms = None;
  let mut screenshot_is_minecraft_window = true;
  let mut tracing = TracingOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--sample" => {
        telemetry_sample = Some(required_flag_value(arguments, index, "--sample")?);
        index += 2;
      }
      "--screenshot" => {
        screenshot = Some(required_flag_value(arguments, index, "--screenshot")?);
        index += 2;
      }
      "--capture-target-app" => {
        capture_target_app = Some(required_flag_value(arguments, index, "--capture-target-app")?);
        index += 2;
      }
      "--capture-target-title" => {
        capture_target_title = Some(required_flag_value(arguments, index, "--capture-target-title")?);
        index += 2;
      }
      "--target-block" => {
        target_block = Some(required_flag_value(arguments, index, "--target-block")?);
        index += 2;
      }
      "--capture-skew-ms" => {
        capture_skew_ms = Some(
          required_flag_value(arguments, index, "--capture-skew-ms")?
            .parse::<i64>()
            .map_err(|error| format!("invalid --capture-skew-ms: {error}"))?,
        );
        index += 2;
      }
      "--screenshot-is-minecraft-window" => {
        screenshot_is_minecraft_window = required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
          .parse::<bool>()
          .map_err(|error| format!("invalid --screenshot-is-minecraft-window: {error}"))?;
        index += 2;
      }
      other => return Err(format!("unexpected minecraft bridge argument {other}")),
    }
  }

  if screenshot.is_some() && capture_target_app.is_some() {
    return Err("--screenshot cannot be combined with --capture-target-app/--capture-target-title".to_string());
  }
  if screenshot.is_none() && capture_target_app.is_none() {
    return Err("minecraft bridge requires either --screenshot or --capture-target-app".to_string());
  }
  if capture_target_title.is_some() && capture_target_app.is_none() {
    return Err("--capture-target-title requires --capture-target-app".to_string());
  }

  Ok(CliCommand::MinecraftProjectionBridge {
    telemetry_sample: telemetry_sample.ok_or_else(|| "--sample is required".to_string())?,
    screenshot,
    capture_target_app,
    capture_target_title,
    target_block: target_block.ok_or_else(|| "--target-block is required".to_string())?,
    capture_skew_ms,
    screenshot_is_minecraft_window,
    tracing,
  })
}

fn parse_minecraft_calibrate_projection(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut frame_path = None;
  let mut screenshot = None;
  let mut target_block = None;
  let mut target_semantics = "hit_face_center".to_string();
  let mut screenshot_is_minecraft_window = true;
  let mut tracing = TracingOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--frame" => {
        frame_path = Some(required_flag_value(arguments, index, "--frame")?);
        index += 2;
      }
      "--screenshot" => {
        screenshot = Some(required_flag_value(arguments, index, "--screenshot")?);
        index += 2;
      }
      "--target-block" => {
        target_block = Some(required_flag_value(arguments, index, "--target-block")?);
        index += 2;
      }
      "--target-semantics" => {
        let value = required_flag_value(arguments, index, "--target-semantics")?;
        match value.as_str() {
          "hit_face_center" | "block_center" => target_semantics = value,
          other => {
            return Err(format!("invalid --target-semantics {other:?}; expected hit_face_center or block_center"));
          }
        }
        index += 2;
      }
      "--screenshot-is-minecraft-window" => {
        screenshot_is_minecraft_window = required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
          .parse::<bool>()
          .map_err(|error| format!("invalid --screenshot-is-minecraft-window: {error}"))?;
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft calibrate-projection argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftCalibrateProjection {
    frame_path: frame_path.ok_or_else(|| "--frame is required".to_string())?,
    screenshot: screenshot.ok_or_else(|| "--screenshot is required".to_string())?,
    target_block: target_block.ok_or_else(|| "--target-block is required".to_string())?,
    target_semantics,
    screenshot_is_minecraft_window,
    tracing,
  })
}

fn parse_minecraft_live_click(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut telemetry_sample = None;
  let mut screenshot = None;
  let mut target_block = None;
  let mut target_app = None;
  let mut target_title = None;
  let mut post_telemetry_sample = None;
  let mut capture_skew_ms = None;
  let mut screenshot_is_minecraft_window = true;
  let mut tracing = TracingOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_tracing_option(arguments[index].as_str(), arguments.get(index + 1), &mut tracing)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--sample" => {
        telemetry_sample = Some(required_flag_value(arguments, index, "--sample")?);
        index += 2;
      }
      "--post-sample" => {
        post_telemetry_sample = Some(required_flag_value(arguments, index, "--post-sample")?);
        index += 2;
      }
      "--screenshot" => {
        screenshot = Some(required_flag_value(arguments, index, "--screenshot")?);
        index += 2;
      }
      "--target-block" => {
        target_block = Some(required_flag_value(arguments, index, "--target-block")?);
        index += 2;
      }
      "--target-app" => {
        target_app = Some(required_flag_value(arguments, index, "--target-app")?);
        index += 2;
      }
      "--target-title" => {
        target_title = Some(required_flag_value(arguments, index, "--target-title")?);
        index += 2;
      }
      "--capture-skew-ms" => {
        capture_skew_ms = Some(
          required_flag_value(arguments, index, "--capture-skew-ms")?
            .parse::<i64>()
            .map_err(|error| format!("invalid --capture-skew-ms: {error}"))?,
        );
        index += 2;
      }
      "--screenshot-is-minecraft-window" => {
        screenshot_is_minecraft_window = required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
          .parse::<bool>()
          .map_err(|error| format!("invalid --screenshot-is-minecraft-window: {error}"))?;
        index += 2;
      }
      other => return Err(format!("unexpected minecraft live-click argument {other}")),
    }
  }

  Ok(CliCommand::MinecraftLiveClick {
    telemetry_sample: telemetry_sample.ok_or_else(|| "--sample is required".to_string())?,
    screenshot: screenshot.ok_or_else(|| "--screenshot is required".to_string())?,
    target_block: target_block.ok_or_else(|| "--target-block is required".to_string())?,
    target_app: target_app.ok_or_else(|| "--target-app is required".to_string())?,
    target_title: target_title.ok_or_else(|| "--target-title is required".to_string())?,
    post_telemetry_sample,
    capture_skew_ms,
    screenshot_is_minecraft_window,
    tracing,
  })
}

fn parse_mcp(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() != 2 || arguments[1].as_str() != "serve" {
    return Err("usage: auv mcp serve".to_string());
  }
  Ok(CliCommand::McpServe)
}

fn parse_session(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv session serve [--host <host>] [--port <port>] [--store-root <path>]".to_string());
  }
  if arguments[1].as_str() != "serve" {
    return Err("usage: auv session serve [--host <host>] [--port <port>] [--store-root <path>]".to_string());
  }
  parse_session_serve(arguments)
}

fn parse_session_serve(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut host = auv_runtime::api::session_service::transport::DEFAULT_SESSION_API_HOST.to_string();
  let mut port = auv_runtime::api::session_service::transport::DEFAULT_SESSION_API_PORT;
  let mut store_root = None;
  let mut index = 2;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--host" => {
        if index + 1 >= arguments.len() {
          return Err("--host requires a value".to_string());
        }
        host = arguments[index + 1].clone();
        index += 2;
      }
      "--port" => {
        if index + 1 >= arguments.len() {
          return Err("--port requires a value".to_string());
        }
        port = arguments[index + 1].parse::<u16>().map_err(|error| format!("invalid --port value: {error}"))?;
        index += 2;
      }
      "--store-root" => {
        if index + 1 >= arguments.len() {
          return Err("--store-root requires a value".to_string());
        }
        store_root = Some(arguments[index + 1].clone());
        index += 2;
      }
      other => {
        return Err(format!("unexpected session-serve argument {other}"));
      }
    }
  }

  Ok(CliCommand::SessionServe {
    host,
    port,
    store_root,
  })
}

fn required_flag_value(arguments: &[String], index: usize, flag: &str) -> AuvResult<String> {
  arguments.get(index + 1).cloned().ok_or_else(|| format!("{flag} requires a value"))
}
