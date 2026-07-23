// File: src/cli.rs
use auv_cli_invoke::InvokeCliParse;
use auv_runtime::model::{AuvResult, ExecutionTarget, InvokeRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectWriteSetting {
  Default,
  Enabled,
  Disabled,
}

impl InspectWriteSetting {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw {
      "default" => Ok(Self::Default),
      "true" => Ok(Self::Enabled),
      "false" => Ok(Self::Disabled),
      other => Err(format!("invalid inspect write setting {other:?}; expected true, false, or default")),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectClientOptions {
  pub store_root: Option<String>,
  pub local_write: InspectWriteSetting,
  pub server_write: InspectWriteSetting,
  pub require_server_write: bool,
  pub server_url: Option<String>,
}

impl Default for InspectClientOptions {
  fn default() -> Self {
    Self {
      store_root: None,
      local_write: InspectWriteSetting::Default,
      server_write: InspectWriteSetting::Default,
      require_server_write: false,
      server_url: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InspectServeWriteOptions {
  pub enabled: bool,
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
  AppProbe {
    bundle_id: String,
    output_dir: Option<String>,
  },
  AppAnalyze {
    query: String,
  },
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
    inspect: InspectClientOptions,
  },
  MinecraftCalibrateProjection {
    frame_path: String,
    screenshot: String,
    target_block: String,
    target_semantics: String,
    screenshot_is_minecraft_window: bool,
    inspect: InspectClientOptions,
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
    inspect: InspectClientOptions,
  },
  MinecraftExportSpatialBundle {
    run_id: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftExport3dgsScenePacket {
    bundle_manifest_paths: Vec<String>,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftExport3dgsTrainingPackage {
    scene_packet_manifest_path: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftPrepare3dgsTraining {
    training_package_manifest_path: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftLaunch3dgsTrainingJob {
    training_launch_plan_path: String,
    output_dir: String,
    training_job_endpoint: Option<String>,
    training_job_token: Option<String>,
    training_job_submit_command: Option<String>,
    inspect: InspectClientOptions,
  },
  MinecraftCollect3dgsTrainingJobResult {
    training_job_manifest_path: String,
    output_dir: String,
    training_job_endpoint: Option<String>,
    training_job_token: Option<String>,
    training_job_status_command: Option<String>,
    inspect: InspectClientOptions,
  },
  MinecraftFetch3dgsTrainingResultArtifacts {
    training_result_manifest_path: String,
    output_dir: String,
    training_job_endpoint: Option<String>,
    training_job_token: Option<String>,
    artifact_fetch_command: Option<String>,
    inspect: InspectClientOptions,
  },
  MinecraftValidate3dgsTrainingResult {
    training_result_artifact_manifest_path: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftInspect3dgsTrainingResultHoldout {
    training_result_semantic_manifest_path: String,
    holdout_frame_index: Option<usize>,
    holdout_render_command: Option<String>,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftMeasure3dgsHoldoutRenderQuality {
    training_result_semantic_manifest_path: String,
    holdout_preview_manifest_path: String,
    render_command: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftQuery3dgsTrainingResult {
    training_result_semantic_manifest_path: String,
    target_block: String,
    target_face: Option<String>,
    target_semantics: String,
    query_command: Option<String>,
    use_checkpoint_native_provider: bool,
    use_closed_scene_toy_provider: bool,
    closed_scene_fixture_path: Option<String>,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftQueryWiredLiveClick {
    training_result_semantic_manifest_path: String,
    target_block: String,
    target_face: Option<String>,
    target_semantics: String,
    query_command: Option<String>,
    use_checkpoint_native_provider: bool,
    use_closed_scene_toy_provider: bool,
    closed_scene_fixture_path: Option<String>,
    output_dir: String,
    target_app: String,
    target_title: String,
    telemetry_sample: Option<String>,
    post_telemetry_sample: Option<String>,
    verification_expected_item_id: Option<String>,
    inspect: InspectClientOptions,
  },
  MinecraftPrepareTextureSweep {
    sidecar_run_dir: String,
    output_dir: String,
    inspect: InspectClientOptions,
  },
  MinecraftBuildTextureSweepSamples {
    bundle_manifest_paths: Vec<String>,
    output_path: String,
    inspect: InspectClientOptions,
  },
  MinecraftEvalTextureSweep {
    samples_path: String,
    output_dir: String,
    require_real_source: bool,
    inspect: InspectClientOptions,
  },
  Invoke {
    request: InvokeRequest,
    inspect: InspectClientOptions,
    output: auv_cli_invoke::InvokeOutputOptions,
  },
  Inspect {
    run_id: String,
    store_root: Option<String>,
  },
  InspectServe {
    host: String,
    port: u16,
    store_root: Option<String>,
    write: InspectServeWriteOptions,
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
    "app" => parse_app(arguments),
    "godot" => parse_godot(arguments),
    "osu" => parse_osu(arguments),
    "inspect" => parse_inspect(arguments),
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
  auv app probe <bundle-id> [--output-dir <dir>]
  auv app analyze <probe-dir-or-probe-json>
  auv-godot … (see `auv-godot --help`)
  auv-osu … (see `auv-osu --help`)
  auv-minecraft … (see `auv-minecraft --help`)
  auv invoke <command-id> [--dry-run] [--target <application-id>] [--label <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>]
  auv inspect <run-id> [--store-root <path>]
  auv inspect serve [--host <host>] [--port <port>] [--store-root <path>] [--enable-write]
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
  - `app probe` is the deterministic raw-facts entrypoint for typed app-surface evidence; it records app identity plus runtime-backed surface probes into `.auv/app-probes/.../probe.json`.
  - `app analyze` turns one of those probe directories into `analysis.json` and `report.md`; use that as typed evidence instead of free-form chat summaries.
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

fn parse_app(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv app <probe|analyze> ...".to_string());
  }

  match arguments[1].as_str() {
    "probe" => parse_app_probe(arguments),
    "analyze" => {
      if arguments.len() != 3 {
        return Err("usage: auv app analyze <probe-dir-or-probe-json>".to_string());
      }
      Ok(CliCommand::AppAnalyze {
        query: arguments[2].clone(),
      })
    }
    "distill" | "validate" => Err("app recipe distillation has been removed; use app-local Rust commands instead".to_string()),
    other => Err(format!("unknown app subcommand {other}; use `auv app probe` or `auv app analyze`")),
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

fn parse_app_probe(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv app probe <bundle-id> [--output-dir <dir>]".to_string());
  }

  let bundle_id = arguments[2].clone();
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
        return Err(format!("unexpected app-probe argument {other}"));
      }
    }
  }

  Ok(CliCommand::AppProbe {
    bundle_id,
    output_dir,
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

fn parse_inspect(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv inspect <run-id> [--store-root <path>]|serve [--host <host>] [--port <port>]".to_string());
  }

  if arguments[1] == "serve" {
    return parse_inspect_serve(arguments);
  }

  let run_id = arguments[1].clone();
  let mut store_root = None;
  let mut index = 2;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--store-root" => {
        store_root = Some(required_flag_value(arguments, index, "--store-root")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected auv inspect argument {other}"));
      }
    }
  }

  Ok(CliCommand::Inspect { run_id, store_root })
}

fn parse_inspect_serve(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut host = auv_inspect_server::DEFAULT_INSPECT_HOST.to_string();
  let mut port = auv_inspect_server::DEFAULT_INSPECT_PORT;
  let mut store_root = None;
  let mut write = InspectServeWriteOptions::default();
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
      "--enable-write" => {
        write.enabled = true;
        index += 1;
      }
      other => {
        return Err(format!("unexpected inspect-serve argument {other}"));
      }
    }
  }

  Ok(CliCommand::InspectServe {
    host,
    port,
    store_root,
    write,
  })
}

fn parse_inspect_client_option(argument: &str, value: Option<&String>, inspect: &mut InspectClientOptions) -> AuvResult<Option<usize>> {
  match argument {
    "--store-root" => {
      let value = value.ok_or_else(|| "--store-root requires a value".to_string())?;
      inspect.store_root = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-local-write" => {
      let value = value.ok_or_else(|| "--inspect-local-write requires a value".to_string())?;
      inspect.local_write = InspectWriteSetting::parse(value)?;
      Ok(Some(2))
    }
    "--inspect-server-write" => {
      let value = value.ok_or_else(|| "--inspect-server-write requires a value".to_string())?;
      inspect.server_write = InspectWriteSetting::parse(value)?;
      Ok(Some(2))
    }
    "--require-inspect-server-write" => {
      inspect.require_server_write = true;
      Ok(Some(1))
    }
    "--inspect-server-url" => {
      let value = value.ok_or_else(|| "--inspect-server-url requires a value".to_string())?;
      inspect.server_url = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-server-token" | "--inspect-server-token-file" => Err(format!(
      "{argument} was removed with the legacy inspect-server write-token transport; configure authentication at the server boundary"
    )),
    _ => Ok(None),
  }
}

fn parse_invoke(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut inspect = InspectClientOptions::default();
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
    if let Some(consumed) = parse_inspect_client_option(argument, arguments.get(index + 1), &mut inspect)? {
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
          target_label: None,
        },
        inputs,
        dry_run,
      },
      inspect,
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
    Some("query-wired-live-click") => parse_minecraft_query_wired_live_click(arguments),
    Some("export-spatial-bundle") => parse_minecraft_export_spatial_bundle(arguments),
    Some("export-3dgs-scene-packet") => parse_minecraft_export_3dgs_scene_packet(arguments),
    Some("export-3dgs-training-package") => parse_minecraft_export_3dgs_training_package(arguments),
    Some("prepare-3dgs-training") => parse_minecraft_prepare_3dgs_training(arguments),
    Some("launch-3dgs-training-job") => parse_minecraft_launch_3dgs_training_job(arguments),
    Some("collect-3dgs-training-job-result") => parse_minecraft_collect_3dgs_training_job_result(arguments),
    Some("fetch-3dgs-training-result-artifacts") => parse_minecraft_fetch_3dgs_training_result_artifacts(arguments),
    Some("validate-3dgs-training-result") => parse_minecraft_validate_3dgs_training_result(arguments),
    Some("query-3dgs-training-result") => parse_minecraft_query_3dgs_training_result(arguments),
    Some("inspect-3dgs-training-result-holdout") => parse_minecraft_inspect_3dgs_training_result_holdout(arguments),
    Some("measure-3dgs-holdout-render-quality") => parse_minecraft_measure_3dgs_holdout_render_quality(arguments),
    Some("prepare-texture-sweep") => parse_minecraft_prepare_texture_sweep(arguments),
    Some("build-texture-sweep-samples") => parse_minecraft_build_texture_sweep_samples(arguments),
    Some("eval-texture-sweep") => parse_minecraft_eval_texture_sweep(arguments),
    Some(other) => Err(format!("unknown minecraft subcommand {other}; use `auv-minecraft --help` for full usage")),
    None => unreachable!("help-only minecraft invocations return before subcommand match"),
  }
}

fn parse_minecraft_export_spatial_bundle(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err("usage: auv-minecraft export-spatial-bundle <run-id> --output-dir <dir>".to_string());
  }

  let run_id = arguments[2].clone();
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 3;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft export-spatial-bundle argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftExportSpatialBundle {
    run_id,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_export_3dgs_scene_packet(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut bundle_manifest_paths = Vec::new();
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
  })
}

fn parse_minecraft_export_3dgs_training_package(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut scene_packet_manifest_path = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--scene-packet-manifest" => {
        scene_packet_manifest_path = Some(required_flag_value(arguments, index, "--scene-packet-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft export-3dgs-training-package argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftExport3dgsTrainingPackage {
    scene_packet_manifest_path: scene_packet_manifest_path.ok_or_else(|| "--scene-packet-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_validate_3dgs_training_result(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_artifact_manifest_path = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-artifact-manifest" => {
        training_result_artifact_manifest_path = Some(required_flag_value(arguments, index, "--training-result-artifact-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft validate-3dgs-training-result argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftValidate3dgsTrainingResult {
    training_result_artifact_manifest_path: training_result_artifact_manifest_path
      .ok_or_else(|| "--training-result-artifact-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn validate_target_block_coordinates(raw: &str) -> AuvResult<()> {
  let parts = raw.split(',').map(str::trim).collect::<Vec<_>>();
  if parts.len() != 3 {
    return Err(format!("invalid --target-block {raw:?}; expected x,y,z"));
  }
  for (index, label) in [(0, "x"), (1, "y"), (2, "z")] {
    parts[index].parse::<i32>().map_err(|error| format!("invalid target block {label}: {error}"))?;
  }
  Ok(())
}

fn parse_minecraft_query_3dgs_training_result(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_semantic_manifest_path = None;
  let mut target_block = None;
  let mut target_face = None;
  let mut target_semantics = "hit_face_center".to_string();
  let mut query_command = None;
  let mut use_checkpoint_native_provider = false;
  let mut use_closed_scene_toy_provider = false;
  let mut closed_scene_fixture_path = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(arguments, index, "--training-result-semantic-manifest")?);
        index += 2;
      }
      "--target-block" => {
        target_block = Some(required_flag_value(arguments, index, "--target-block")?);
        index += 2;
      }
      "--target-face" => {
        let value = required_flag_value(arguments, index, "--target-face")?;
        match value.as_str() {
          "up" | "down" | "north" | "south" | "east" | "west" => target_face = Some(value),
          other => {
            return Err(format!("invalid --target-face {other:?}; expected up, down, north, south, east, or west"));
          }
        }
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
      "--query-provider" => {
        let value = required_flag_value(arguments, index, "--query-provider")?;
        match value.as_str() {
          "checkpoint-native" => use_checkpoint_native_provider = true,
          "closed-scene-toy" => use_closed_scene_toy_provider = true,
          other => {
            return Err(format!("invalid --query-provider {other:?}; expected checkpoint-native or closed-scene-toy"));
          }
        }
        index += 2;
      }
      "--closed-scene-fixture" => {
        closed_scene_fixture_path = Some(required_flag_value(arguments, index, "--closed-scene-fixture")?);
        index += 2;
      }
      "--query-command" => {
        query_command = Some(required_flag_value(arguments, index, "--query-command")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft query-3dgs-training-result argument {other}"));
      }
    }
  }

  let target_block = target_block.ok_or_else(|| "--target-block is required".to_string())?;
  validate_target_block_coordinates(&target_block)?;

  if use_checkpoint_native_provider && use_closed_scene_toy_provider {
    return Err("--query-provider checkpoint-native and --query-provider closed-scene-toy are mutually exclusive".to_string());
  }

  if use_checkpoint_native_provider && query_command.is_some() {
    return Err("--query-provider checkpoint-native and --query-command are mutually exclusive".to_string());
  }

  if use_closed_scene_toy_provider && query_command.is_some() {
    return Err("--query-provider closed-scene-toy and --query-command are mutually exclusive".to_string());
  }

  if use_closed_scene_toy_provider && closed_scene_fixture_path.is_none() {
    return Err("--closed-scene-fixture is required when --query-provider closed-scene-toy".to_string());
  }

  Ok(CliCommand::MinecraftQuery3dgsTrainingResult {
    training_result_semantic_manifest_path: training_result_semantic_manifest_path
      .ok_or_else(|| "--training-result-semantic-manifest is required".to_string())?,
    target_block,
    target_face,
    target_semantics,
    query_command,
    use_checkpoint_native_provider,
    use_closed_scene_toy_provider,
    closed_scene_fixture_path,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_query_wired_live_click(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_semantic_manifest_path = None;
  let mut target_block = None;
  let mut target_face = None;
  let mut target_semantics = "hit_face_center".to_string();
  let mut query_command = None;
  let mut use_checkpoint_native_provider = false;
  let mut use_closed_scene_toy_provider = false;
  let mut closed_scene_fixture_path = None;
  let mut output_dir = None;
  let mut target_app = None;
  let mut target_title = None;
  let mut telemetry_sample = None;
  let mut post_telemetry_sample = None;
  let mut verification_expected_item_id = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(arguments, index, "--training-result-semantic-manifest")?);
        index += 2;
      }
      "--target-block" => {
        target_block = Some(required_flag_value(arguments, index, "--target-block")?);
        index += 2;
      }
      "--target-face" => {
        let value = required_flag_value(arguments, index, "--target-face")?;
        match value.as_str() {
          "up" | "down" | "north" | "south" | "east" | "west" => target_face = Some(value),
          other => {
            return Err(format!("invalid --target-face {other:?}; expected up, down, north, south, east, or west"));
          }
        }
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
      "--query-provider" => {
        let value = required_flag_value(arguments, index, "--query-provider")?;
        match value.as_str() {
          "checkpoint-native" => use_checkpoint_native_provider = true,
          "closed-scene-toy" => use_closed_scene_toy_provider = true,
          other => {
            return Err(format!("invalid --query-provider {other:?}; expected checkpoint-native or closed-scene-toy"));
          }
        }
        index += 2;
      }
      "--closed-scene-fixture" => {
        closed_scene_fixture_path = Some(required_flag_value(arguments, index, "--closed-scene-fixture")?);
        index += 2;
      }
      "--query-command" => {
        query_command = Some(required_flag_value(arguments, index, "--query-command")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
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
      "--sample" => {
        telemetry_sample = Some(required_flag_value(arguments, index, "--sample")?);
        index += 2;
      }
      "--post-sample" => {
        post_telemetry_sample = Some(required_flag_value(arguments, index, "--post-sample")?);
        index += 2;
      }
      "--verification-expected-item-id" => {
        verification_expected_item_id = Some(required_flag_value(arguments, index, "--verification-expected-item-id")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft query-wired-live-click argument {other}"));
      }
    }
  }

  let target_block = target_block.ok_or_else(|| "--target-block is required".to_string())?;
  validate_target_block_coordinates(&target_block)?;

  if use_checkpoint_native_provider && use_closed_scene_toy_provider {
    return Err("--query-provider checkpoint-native and --query-provider closed-scene-toy are mutually exclusive".to_string());
  }

  if use_checkpoint_native_provider && query_command.is_some() {
    return Err("--query-provider checkpoint-native and --query-command are mutually exclusive".to_string());
  }

  if use_closed_scene_toy_provider && query_command.is_some() {
    return Err("--query-provider closed-scene-toy and --query-command are mutually exclusive".to_string());
  }

  if use_closed_scene_toy_provider && closed_scene_fixture_path.is_none() {
    return Err("--closed-scene-fixture is required when --query-provider closed-scene-toy".to_string());
  }

  if closed_scene_fixture_path.is_some() && !use_closed_scene_toy_provider {
    return Err("--closed-scene-fixture requires --query-provider closed-scene-toy".to_string());
  }

  if post_telemetry_sample.is_some() && telemetry_sample.is_none() {
    return Err("--post-sample requires --sample".to_string());
  }

  if verification_expected_item_id.is_some() && telemetry_sample.is_none() {
    return Err("--verification-expected-item-id requires --sample".to_string());
  }

  Ok(CliCommand::MinecraftQueryWiredLiveClick {
    training_result_semantic_manifest_path: training_result_semantic_manifest_path
      .ok_or_else(|| "--training-result-semantic-manifest is required".to_string())?,
    target_block,
    target_face,
    target_semantics,
    query_command,
    use_checkpoint_native_provider,
    use_closed_scene_toy_provider,
    closed_scene_fixture_path,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    target_app: target_app.ok_or_else(|| "--target-app is required".to_string())?,
    target_title: target_title.ok_or_else(|| "--target-title is required".to_string())?,
    telemetry_sample,
    post_telemetry_sample,
    verification_expected_item_id,
    inspect,
  })
}
fn parse_minecraft_inspect_3dgs_training_result_holdout(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_semantic_manifest_path = None;
  let mut holdout_frame_index = None;
  let mut holdout_render_command = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(arguments, index, "--training-result-semantic-manifest")?);
        index += 2;
      }
      "--holdout-frame-index" => {
        let value = required_flag_value(arguments, index, "--holdout-frame-index")?;
        holdout_frame_index = Some(value.parse::<usize>().map_err(|error| format!("invalid --holdout-frame-index: {error}"))?);
        index += 2;
      }
      "--holdout-render-command" => {
        holdout_render_command = Some(required_flag_value(arguments, index, "--holdout-render-command")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft inspect-3dgs-training-result-holdout argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftInspect3dgsTrainingResultHoldout {
    training_result_semantic_manifest_path: training_result_semantic_manifest_path
      .ok_or_else(|| "--training-result-semantic-manifest is required".to_string())?,
    holdout_frame_index,
    holdout_render_command,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_measure_3dgs_holdout_render_quality(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_semantic_manifest_path = None;
  let mut holdout_preview_manifest_path = None;
  let mut render_command = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(arguments, index, "--training-result-semantic-manifest")?);
        index += 2;
      }
      "--holdout-preview-manifest" => {
        holdout_preview_manifest_path = Some(required_flag_value(arguments, index, "--holdout-preview-manifest")?);
        index += 2;
      }
      "--render-command" => {
        render_command = Some(required_flag_value(arguments, index, "--render-command")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft measure-3dgs-holdout-render-quality argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftMeasure3dgsHoldoutRenderQuality {
    training_result_semantic_manifest_path: training_result_semantic_manifest_path
      .ok_or_else(|| "--training-result-semantic-manifest is required".to_string())?,
    holdout_preview_manifest_path: holdout_preview_manifest_path.ok_or_else(|| "--holdout-preview-manifest is required".to_string())?,
    render_command: render_command.ok_or_else(|| "--render-command is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_prepare_texture_sweep(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut sidecar_run_dir = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
  })
}

fn parse_minecraft_prepare_3dgs_training(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_package_manifest_path = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-package-manifest" => {
        training_package_manifest_path = Some(required_flag_value(arguments, index, "--training-package-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft prepare-3dgs-training argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftPrepare3dgsTraining {
    training_package_manifest_path: training_package_manifest_path.ok_or_else(|| "--training-package-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_launch_3dgs_training_job(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_launch_plan_path = None;
  let mut output_dir = None;
  let mut training_job_endpoint = None;
  let mut training_job_token = None;
  let mut training_job_submit_command = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-launch-plan" => {
        training_launch_plan_path = Some(required_flag_value(arguments, index, "--training-launch-plan")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(arguments, index, "--training-job-endpoint")?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(arguments, index, "--training-job-token")?);
        index += 2;
      }
      "--training-job-submit-command" => {
        training_job_submit_command = Some(required_flag_value(arguments, index, "--training-job-submit-command")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft launch-3dgs-training-job argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftLaunch3dgsTrainingJob {
    training_launch_plan_path: training_launch_plan_path.ok_or_else(|| "--training-launch-plan is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    training_job_endpoint,
    training_job_token,
    training_job_submit_command,
    inspect,
  })
}

fn parse_minecraft_collect_3dgs_training_job_result(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_job_manifest_path = None;
  let mut output_dir = None;
  let mut training_job_endpoint = None;
  let mut training_job_token = None;
  let mut training_job_status_command = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-job-manifest" => {
        training_job_manifest_path = Some(required_flag_value(arguments, index, "--training-job-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(arguments, index, "--training-job-endpoint")?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(arguments, index, "--training-job-token")?);
        index += 2;
      }
      "--training-job-status-command" => {
        training_job_status_command = Some(required_flag_value(arguments, index, "--training-job-status-command")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft collect-3dgs-training-job-result argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftCollect3dgsTrainingJobResult {
    training_job_manifest_path: training_job_manifest_path.ok_or_else(|| "--training-job-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    training_job_endpoint,
    training_job_token,
    training_job_status_command,
    inspect,
  })
}

fn parse_minecraft_fetch_3dgs_training_result_artifacts(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut training_result_manifest_path = None;
  let mut output_dir = None;
  let mut training_job_endpoint = None;
  let mut training_job_token = None;
  let mut artifact_fetch_command = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-manifest" => {
        training_result_manifest_path = Some(required_flag_value(arguments, index, "--training-result-manifest")?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(arguments, index, "--training-job-endpoint")?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(arguments, index, "--training-job-token")?);
        index += 2;
      }
      "--artifact-fetch-command" => {
        artifact_fetch_command = Some(required_flag_value(arguments, index, "--artifact-fetch-command")?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected minecraft fetch-3dgs-training-result-artifacts argument {other}"));
      }
    }
  }

  Ok(CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
    training_result_manifest_path: training_result_manifest_path.ok_or_else(|| "--training-result-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    training_job_endpoint,
    training_job_token,
    artifact_fetch_command,
    inspect,
  })
}

fn parse_minecraft_build_texture_sweep_samples(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut bundle_manifest_paths = Vec::new();
  let mut output_path = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
  })
}

fn parse_minecraft_eval_texture_sweep(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut samples_path = None;
  let mut output_dir = None;
  let mut require_real_source = false;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
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
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
  })
}

fn parse_minecraft_calibrate_projection(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut frame_path = None;
  let mut screenshot = None;
  let mut target_block = None;
  let mut target_semantics = "hit_face_center".to_string();
  let mut screenshot_is_minecraft_window = true;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
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
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(arguments[index].as_str(), arguments.get(index + 1), &mut inspect)? {
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
    inspect,
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_skill_commands_are_removed() {
    for args in [
      vec!["skill"],
      vec!["skill", "list"],
      vec!["skill", "show", "macos.textedit.create_and_verify_text.v0"],
      vec![
        "skill",
        "run",
        "recipes/macos/textedit/create-and-verify-text.v0.json",
      ],
      vec!["skill", "cases", "list"],
      vec![
        "skill",
        "cases",
        "run",
        "recipes/macos/textedit/create-and-verify-text.cases.v0.json",
      ],
    ] {
      let args = args.into_iter().map(String::from).collect::<Vec<_>>();
      let error = parse_cli(&args).expect_err("skill command should be removed");
      assert!(error.contains("skill commands have been removed"), "unexpected error for {args:?}: {error}");
    }
  }

  #[test]
  fn parse_app_distill_and_validate_are_removed() {
    for args in [
      vec!["app", "distill", ".auv/app-probes/example/analysis.json"],
      vec![
        "app",
        "validate",
        ".auv/app-probes/example/distillation.json",
      ],
    ] {
      let args = args.into_iter().map(String::from).collect::<Vec<_>>();
      let error = parse_cli(&args).expect_err("recipe-producing app command should be removed");
      assert!(error.contains("app recipe distillation has been removed"), "unexpected error for {args:?}: {error}");
    }
  }

  #[test]
  fn parse_godot_capability_query() {
    let command = parse_cli(&[
      "godot".to_string(),
      "capability-query".to_string(),
      "--json".to_string(),
    ])
    .expect("godot capability query should parse");

    assert!(matches!(command, CliCommand::GodotCapabilityQuery { json: true }));
  }

  #[test]
  fn parse_godot_render_observe() {
    let command = parse_cli(&[
      "godot".to_string(),
      "render-observe".to_string(),
      "--output-dir".to_string(),
      "artifacts/godot-observe".to_string(),
      "--stage".to_string(),
      "final".to_string(),
      "--stage".to_string(),
      "avatar-edge-mask".to_string(),
      "--json".to_string(),
    ])
    .expect("godot render observe should parse");

    let CliCommand::GodotRenderObserve {
      output_dir,
      stages,
      json,
    } = command
    else {
      panic!("unexpected command");
    };
    assert_eq!(output_dir, "artifacts/godot-observe");
    assert_eq!(stages, vec!["final", "avatar-edge-mask"]);
    assert!(json);
  }

  #[test]
  fn help_text_lists_list_commands_tombstone() {
    let help = help_text();

    assert!(help.contains("list-commands"));
    assert!(help.contains("auv invoke --help"));
    assert!(help.contains("retired"));
  }

  #[test]
  fn help_text_keeps_core_paths_visible() {
    let help = help_text();

    for expected in [
      "auv --version",
      "auv doctor [--json]",
      "auv permissions check [--json]",
      "auv app probe <bundle-id> [--output-dir <dir>]",
      "auv app analyze <probe-dir-or-probe-json>",
      "auv-godot",
      "auv-osu",
      "auv-minecraft",
      "auv invoke <command-id>",
      "auv inspect <run-id> [--store-root <path>]",
      "auv inspect serve [--host <host>] [--port <port>] [--store-root <path>] [--enable-write]",
      "auv session serve [--host <host>] [--port <port>] [--store-root <path>]",
      "auv mcp serve",
    ] {
      assert!(help.contains(expected), "top-level help should keep core path visible: {expected}");
    }
    assert!(!help.contains("--inspect-server-token"));
    for retired in ["--write-token", "--write-token-file", "--no-write-token"] {
      assert!(!help.contains(retired), "top-level help must omit retired Inspect serve option {retired}");
    }
  }

  #[test]
  fn parse_root_version() {
    let command = parse_cli(&["--version".to_string()]).expect("root --version should parse");

    assert!(matches!(command, CliCommand::Version));
  }

  #[test]
  fn root_version_request_requires_only_the_version_flag() {
    assert!(root_version_requested(&["--version".to_string()]));
    assert!(root_version_requested(&["-V".to_string()]));
    assert!(!root_version_requested(&["--version".to_string(), "extra".to_string()]));
  }

  #[test]
  fn parse_root_version_rejects_trailing_arguments() {
    let error = parse_cli(&["--version".to_string(), "extra".to_string()]).expect_err("root --version extra should fail");

    assert_eq!(error, "usage: auv --version");
  }

  #[test]
  fn version_text_names_the_package_version() {
    assert_eq!(version_text(), format!("auv {}\n", env!("CARGO_PKG_VERSION")));
  }

  #[test]
  fn help_text_is_core_only() {
    let help = help_text();

    // Root help may name separate app bins, but must not expand their live
    // subcommands under `auv <app> …`.
    for omitted in [
      "auv minecraft bridge",
      "auv minecraft calibrate-projection",
      "auv osu benchmark",
      "auv osu dispatch",
      "auv godot capability-query",
    ] {
      assert!(!help.contains(omitted), "top-level help should not expand app command: {omitted}");
    }

    assert!(
      help.contains("tombstone") || help.contains("has been removed") || help.contains("use `auv-minecraft`"),
      "top-level help should point donors at separate bins"
    );
  }

  #[test]
  fn parse_minecraft_help_command() {
    let command = parse_cli(&["minecraft".to_string(), "--help".to_string()]).expect("minecraft --help should parse");
    assert!(matches!(command, CliCommand::MinecraftHelp));
  }

  #[test]
  fn parse_minecraft_bare_command_as_help() {
    let command = parse_cli(&["minecraft".to_string()]).expect("bare minecraft should parse as help");
    assert!(matches!(command, CliCommand::MinecraftHelp));
  }

  #[test]
  fn parse_minecraft_help_rejects_trailing_arguments() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "--help".to_string(),
      "junk".to_string(),
    ])
    .expect_err("minecraft --help junk should fail");
    assert!(error.contains("unexpected minecraft help argument"));
  }

  #[test]
  fn parse_osu_help_command() {
    let command = parse_cli(&["osu".to_string(), "--help".to_string()]).expect("osu --help should parse");
    assert!(matches!(command, CliCommand::OsuHelp));
  }

  #[test]
  fn parse_godot_help_command() {
    let command = parse_cli(&["godot".to_string(), "--help".to_string()]).expect("godot --help should parse");
    assert!(matches!(command, CliCommand::GodotHelp));
  }

  #[test]
  fn parse_godot_bare_command_as_help() {
    let command = parse_cli(&["godot".to_string()]).expect("bare godot should parse as help");
    assert!(matches!(command, CliCommand::GodotHelp));
  }

  #[test]
  fn parse_osu_bare_command_as_help() {
    let command = parse_cli(&["osu".to_string()]).expect("bare osu should parse as help");
    assert!(matches!(command, CliCommand::OsuHelp));
  }

  #[test]
  fn parse_osu_help_rejects_trailing_arguments() {
    let error = parse_cli(&["osu".to_string(), "help".to_string(), "extra".to_string()]).expect_err("osu help extra should fail");
    assert!(error.contains("unexpected osu help argument"));
  }

  #[test]
  fn help_text_does_not_mention_candidate_action() {
    let help = help_text();

    assert!(!help.contains("candidate-action"));
    assert!(!help.contains("candidate_action"));
  }

  #[test]
  fn list_drivers_command_is_removed() {
    let error = parse_cli(&["list-drivers".to_string()]).expect_err("list-drivers should be removed");
    assert!(error.contains("unknown subcommand list-drivers"));

    let help = help_text();
    assert!(!help.contains("auv list-drivers"));
  }

  #[test]
  fn scan_command_is_removed() {
    let error = parse_cli(&["scan".to_string()]).expect_err("scan should be removed");
    assert!(error.contains("unknown subcommand scan"));

    let help = help_text();
    assert!(!help.contains("auv scan"));
  }

  #[test]
  fn verticals_command_is_removed() {
    let error = parse_cli(&["verticals".to_string()]).expect_err("verticals should be removed");
    assert!(error.contains("unknown subcommand verticals"));

    let help = help_text();
    assert!(!help.contains("auv verticals"));
  }

  #[test]
  fn parse_osu_eval_detections_command() {
    let command = parse_cli(&[
      "osu".to_string(),
      "eval-detections".to_string(),
      "/tmp/run".to_string(),
      "--detections".to_string(),
      "/tmp/detections".to_string(),
      "--output-dir".to_string(),
      "/tmp/output".to_string(),
    ])
    .expect("osu eval-detections command should parse");

    match command {
      CliCommand::OsuEvalDetections {
        run_artifact_dir,
        detections_path,
        output_dir,
      } => {
        assert_eq!(run_artifact_dir, "/tmp/run");
        assert_eq!(detections_path, "/tmp/detections");
        assert_eq!(output_dir.as_deref(), Some("/tmp/output"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_osu_eval_detections_requires_detections() {
    let error = parse_cli(&[
      "osu".to_string(),
      "eval-detections".to_string(),
      "/tmp/run".to_string(),
      "--output-dir".to_string(),
      "/tmp/output".to_string(),
    ])
    .expect_err("osu eval-detections should require --detections");

    assert!(error.contains("--detections is required"));
  }

  #[test]
  fn parse_osu_eval_detections_accepts_default_output_dir() {
    let command = parse_cli(&[
      "osu".to_string(),
      "eval-detections".to_string(),
      "/tmp/run".to_string(),
      "--detections".to_string(),
      "/tmp/detections.json".to_string(),
    ])
    .expect("osu eval-detections should allow omitted output dir");

    match command {
      CliCommand::OsuEvalDetections { output_dir, .. } => {
        assert_eq!(output_dir, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_osu_vision_demo_command() {
    let command = parse_cli(&[
      "osu".to_string(),
      "vision-demo".to_string(),
      "/tmp/map.osu".to_string(),
      "--target-app".to_string(),
      "osu!".to_string(),
      "--output-dir".to_string(),
      "/tmp/output".to_string(),
      "--dispatch-limit".to_string(),
      "3".to_string(),
      "--capture-verify".to_string(),
    ])
    .expect("osu vision-demo command should parse");

    match command {
      CliCommand::OsuVisionDemo {
        beatmap_path,
        target_app,
        output_dir,
        dispatch_limit,
        capture_verify,
      } => {
        assert_eq!(beatmap_path, "/tmp/map.osu");
        assert_eq!(target_app, "osu!");
        assert_eq!(output_dir.as_deref(), Some("/tmp/output"));
        assert_eq!(dispatch_limit, Some(3));
        assert!(capture_verify);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_osu_vision_demo_caps_dispatch_limit() {
    let command = parse_cli(&[
      "osu".to_string(),
      "vision-demo".to_string(),
      "/tmp/map.osu".to_string(),
      "--target-app".to_string(),
      "osu!".to_string(),
      "--dispatch-limit".to_string(),
      "99".to_string(),
    ])
    .expect("osu vision-demo command should parse with large dispatch limit");

    match command {
      CliCommand::OsuVisionDemo { dispatch_limit, .. } => {
        assert_eq!(dispatch_limit, Some(99));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_osu_vision_demo_requires_target_app() {
    let error = parse_cli(&[
      "osu".to_string(),
      "vision-demo".to_string(),
      "/tmp/map.osu".to_string(),
      "--output-dir".to_string(),
      "/tmp/output".to_string(),
    ])
    .expect_err("osu vision-demo should require --target-app");

    assert!(error.contains("--target-app is required") || error.contains("usage:"));
  }

  #[test]
  fn parse_osu_vision_demo_accepts_default_output_dir() {
    let command = parse_cli(&[
      "osu".to_string(),
      "vision-demo".to_string(),
      "/tmp/map.osu".to_string(),
      "--target-app".to_string(),
      "osu!".to_string(),
    ])
    .expect("osu vision-demo should allow omitted output dir");

    match command {
      CliCommand::OsuVisionDemo {
        output_dir,
        dispatch_limit,
        capture_verify,
        ..
      } => {
        assert_eq!(output_dir, None);
        assert_eq!(dispatch_limit, None);
        assert!(!capture_verify);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_bridge_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "bridge".to_string(),
      "--sample".to_string(),
      "/tmp/telemetry.jsonl".to_string(),
      "--screenshot".to_string(),
      "/tmp/frame.png".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--capture-skew-ms".to_string(),
      "120".to_string(),
      "--screenshot-is-minecraft-window".to_string(),
      "false".to_string(),
    ])
    .expect("minecraft bridge command should parse");

    match command {
      CliCommand::MinecraftProjectionBridge {
        telemetry_sample,
        screenshot,
        capture_target_app,
        capture_target_title,
        target_block,
        capture_skew_ms,
        screenshot_is_minecraft_window,
        ..
      } => {
        assert_eq!(telemetry_sample, "/tmp/telemetry.jsonl");
        assert_eq!(screenshot.as_deref(), Some("/tmp/frame.png"));
        assert_eq!(capture_target_app, None);
        assert_eq!(capture_target_title, None);
        assert_eq!(target_block, "1,2,3");
        assert_eq!(capture_skew_ms, Some(120));
        assert_eq!(screenshot_is_minecraft_window, false);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_live_click_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "live-click".to_string(),
      "--sample".to_string(),
      "/tmp/pre.jsonl".to_string(),
      "--post-sample".to_string(),
      "/tmp/post.jsonl".to_string(),
      "--screenshot".to_string(),
      "/tmp/frame.png".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft 1.21.5".to_string(),
      "--capture-skew-ms".to_string(),
      "120".to_string(),
      "--screenshot-is-minecraft-window".to_string(),
      "false".to_string(),
    ])
    .expect("minecraft live-click command should parse");

    match command {
      CliCommand::MinecraftLiveClick {
        telemetry_sample,
        screenshot,
        target_block,
        target_app,
        target_title,
        post_telemetry_sample,
        capture_skew_ms,
        screenshot_is_minecraft_window,
        ..
      } => {
        assert_eq!(telemetry_sample, "/tmp/pre.jsonl");
        assert_eq!(post_telemetry_sample.as_deref(), Some("/tmp/post.jsonl"));
        assert_eq!(screenshot, "/tmp/frame.png");
        assert_eq!(target_block, "1,2,3");
        assert_eq!(target_app, "com.mojang.minecraft");
        assert_eq!(target_title, "Minecraft 1.21.5");
        assert_eq!(capture_skew_ms, Some(120));
        assert_eq!(screenshot_is_minecraft_window, false);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_bridge_requires_required_flags() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "bridge".to_string(),
      "--sample".to_string(),
      "/tmp/telemetry.jsonl".to_string(),
    ])
    .expect_err("minecraft bridge should require screenshot/capture target and target");

    assert!(error.contains("requires either --screenshot or --capture-target-app") || error.contains("--target-block is required"));
  }

  #[test]
  fn parse_minecraft_bridge_capture_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "bridge".to_string(),
      "--sample".to_string(),
      "/tmp/telemetry.jsonl".to_string(),
      "--capture-target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--capture-target-title".to_string(),
      "Minecraft".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
    ])
    .expect("minecraft bridge capture command should parse");

    match command {
      CliCommand::MinecraftProjectionBridge {
        screenshot,
        capture_target_app,
        capture_target_title,
        ..
      } => {
        assert_eq!(screenshot, None);
        assert_eq!(capture_target_app.as_deref(), Some("com.mojang.minecraft"));
        assert_eq!(capture_target_title.as_deref(), Some("Minecraft"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_bridge_rejects_mixed_capture_modes() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "bridge".to_string(),
      "--sample".to_string(),
      "/tmp/telemetry.jsonl".to_string(),
      "--screenshot".to_string(),
      "/tmp/frame.png".to_string(),
      "--capture-target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
    ])
    .expect_err("mixed capture modes should fail");

    assert!(error.contains("--screenshot cannot be combined"));
  }

  #[test]
  fn parse_minecraft_calibrate_projection_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "calibrate-projection".to_string(),
      "--frame".to_string(),
      "/tmp/frame.json".to_string(),
      "--screenshot".to_string(),
      "/tmp/frame.png".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--target-semantics".to_string(),
      "block_center".to_string(),
      "--screenshot-is-minecraft-window".to_string(),
      "false".to_string(),
    ])
    .expect("minecraft calibrate-projection should parse");

    match command {
      CliCommand::MinecraftCalibrateProjection {
        frame_path,
        screenshot,
        target_block,
        target_semantics,
        screenshot_is_minecraft_window,
        ..
      } => {
        assert_eq!(frame_path, "/tmp/frame.json");
        assert_eq!(screenshot, "/tmp/frame.png");
        assert_eq!(target_block, "1,2,3");
        assert_eq!(target_semantics, "block_center");
        assert!(!screenshot_is_minecraft_window);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_export_spatial_bundle_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "export-spatial-bundle".to_string(),
      "run_123".to_string(),
      "--output-dir".to_string(),
      "/tmp/mc6-bundle".to_string(),
    ])
    .expect("minecraft export-spatial-bundle command should parse");

    match command {
      CliCommand::MinecraftExportSpatialBundle {
        run_id, output_dir, ..
      } => {
        assert_eq!(run_id, "run_123");
        assert_eq!(output_dir, "/tmp/mc6-bundle");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_export_spatial_bundle_accepts_store_root() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "export-spatial-bundle".to_string(),
      "run_123".to_string(),
      "--output-dir".to_string(),
      "/tmp/mc6-bundle".to_string(),
      "--store-root".to_string(),
      "/tmp/store".to_string(),
    ])
    .expect("minecraft export-spatial-bundle should parse store root");

    match command {
      CliCommand::MinecraftExportSpatialBundle { inspect, .. } => {
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/store"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_export_3dgs_scene_packet_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "export-3dgs-scene-packet".to_string(),
      "--bundle-manifest".to_string(),
      "/tmp/rich/run.json".to_string(),
      "--bundle-manifest".to_string(),
      "/tmp/flat/run.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/scene".to_string(),
    ])
    .expect("minecraft export-3dgs-scene-packet command should parse");

    match command {
      CliCommand::MinecraftExport3dgsScenePacket {
        bundle_manifest_paths,
        output_dir,
        ..
      } => {
        assert_eq!(bundle_manifest_paths, vec!["/tmp/rich/run.json", "/tmp/flat/run.json"]);
        assert_eq!(output_dir, "/tmp/scene");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_export_3dgs_training_package_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "export-3dgs-training-package".to_string(),
      "--scene-packet-manifest".to_string(),
      "/tmp/scene/run.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/training".to_string(),
    ])
    .expect("minecraft export-3dgs-training-package command should parse");

    match command {
      CliCommand::MinecraftExport3dgsTrainingPackage {
        scene_packet_manifest_path,
        output_dir,
        ..
      } => {
        assert_eq!(scene_packet_manifest_path, "/tmp/scene/run.json");
        assert_eq!(output_dir, "/tmp/training");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_prepare_3dgs_training_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "prepare-3dgs-training".to_string(),
      "--training-package-manifest".to_string(),
      "/tmp/training/run.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/launch".to_string(),
    ])
    .expect("minecraft prepare-3dgs-training command should parse");

    match command {
      CliCommand::MinecraftPrepare3dgsTraining {
        training_package_manifest_path,
        output_dir,
        ..
      } => {
        assert_eq!(training_package_manifest_path, "/tmp/training/run.json");
        assert_eq!(output_dir, "/tmp/launch");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_launch_3dgs_training_job_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "launch-3dgs-training-job".to_string(),
      "--training-launch-plan".to_string(),
      "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/job".to_string(),
    ])
    .expect("minecraft launch-3dgs-training-job command should parse");

    match command {
      CliCommand::MinecraftLaunch3dgsTrainingJob {
        training_launch_plan_path,
        output_dir,
        training_job_endpoint,
        training_job_token,
        training_job_submit_command,
        ..
      } => {
        assert_eq!(training_launch_plan_path, "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json");
        assert_eq!(output_dir, "/tmp/job");
        assert_eq!(training_job_endpoint, None);
        assert_eq!(training_job_token, None);
        assert_eq!(training_job_submit_command, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_collect_3dgs_training_job_result_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "collect-3dgs-training-job-result".to_string(),
      "--training-job-manifest".to_string(),
      "/tmp/training-job/minecraft-3dgs-training-job.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/result".to_string(),
    ])
    .expect("minecraft collect-3dgs-training-job-result command should parse");

    match command {
      CliCommand::MinecraftCollect3dgsTrainingJobResult {
        training_job_manifest_path,
        output_dir,
        training_job_endpoint,
        training_job_token,
        ..
      } => {
        assert_eq!(training_job_manifest_path, "/tmp/training-job/minecraft-3dgs-training-job.json");
        assert_eq!(output_dir, "/tmp/result");
        assert_eq!(training_job_endpoint, None);
        assert_eq!(training_job_token, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_collect_3dgs_training_job_result_requires_manifest() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "collect-3dgs-training-job-result".to_string(),
      "--output-dir".to_string(),
      "/tmp/result".to_string(),
    ])
    .expect_err("missing training job manifest should fail");

    assert_eq!(error, "--training-job-manifest is required");
  }

  #[test]
  fn parse_minecraft_launch_3dgs_training_job_command_with_remote_config_flags() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "launch-3dgs-training-job".to_string(),
      "--training-launch-plan".to_string(),
      "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/job".to_string(),
      "--training-job-endpoint".to_string(),
      "https://jobs.example.test/v1".to_string(),
      "--training-job-token".to_string(),
      "secret-token".to_string(),
      "--training-job-submit-command".to_string(),
      "remote-submit --json".to_string(),
    ])
    .expect("minecraft launch-3dgs-training-job command with remote config should parse");

    match command {
      CliCommand::MinecraftLaunch3dgsTrainingJob {
        training_job_endpoint,
        training_job_token,
        training_job_submit_command,
        ..
      } => {
        assert_eq!(training_job_endpoint.as_deref(), Some("https://jobs.example.test/v1"));
        assert_eq!(training_job_token.as_deref(), Some("secret-token"));
        assert_eq!(training_job_submit_command.as_deref(), Some("remote-submit --json"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_launch_3dgs_training_job_requires_output_dir() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "launch-3dgs-training-job".to_string(),
      "--training-launch-plan".to_string(),
      "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json".to_string(),
    ])
    .expect_err("missing output dir should fail");

    assert_eq!(error, "--output-dir is required");
  }

  #[test]
  fn parse_minecraft_collect_3dgs_training_job_result_command_with_remote_config_flags() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "collect-3dgs-training-job-result".to_string(),
      "--training-job-manifest".to_string(),
      "/tmp/training-job/minecraft-3dgs-training-job.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/result".to_string(),
      "--training-job-endpoint".to_string(),
      "https://jobs.example.test/v1".to_string(),
      "--training-job-token".to_string(),
      "secret-token".to_string(),
    ])
    .expect("minecraft collect-3dgs-training-job-result command with remote config should parse");

    match command {
      CliCommand::MinecraftCollect3dgsTrainingJobResult {
        training_job_endpoint,
        training_job_token,
        training_job_status_command,
        ..
      } => {
        assert_eq!(training_job_endpoint.as_deref(), Some("https://jobs.example.test/v1"));
        assert_eq!(training_job_token.as_deref(), Some("secret-token"));
        assert_eq!(training_job_status_command, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_collect_3dgs_training_job_result_command_with_status_command_flag() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "collect-3dgs-training-job-result".to_string(),
      "--training-job-manifest".to_string(),
      "/tmp/training-job/minecraft-3dgs-training-job.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/result".to_string(),
      "--training-job-status-command".to_string(),
      "python3 -c \"print(1)\"".to_string(),
    ])
    .expect("minecraft collect-3dgs-training-job-result command with status command should parse");

    match command {
      CliCommand::MinecraftCollect3dgsTrainingJobResult {
        training_job_status_command,
        ..
      } => {
        assert_eq!(training_job_status_command.as_deref(), Some("python3 -c \"print(1)\""));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--target-face".to_string(),
      "north".to_string(),
      "--target-semantics".to_string(),
      "block_center".to_string(),
      "--query-provider".to_string(),
      "closed-scene-toy".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
      "--sample".to_string(),
      "/tmp/pre.jsonl".to_string(),
      "--post-sample".to_string(),
      "/tmp/post.jsonl".to_string(),
    ])
    .expect("query-wired-live-click should parse");

    match command {
      CliCommand::MinecraftQueryWiredLiveClick {
        training_result_semantic_manifest_path,
        target_block,
        target_face,
        target_semantics,
        use_closed_scene_toy_provider,
        closed_scene_fixture_path,
        output_dir,
        target_app,
        target_title,
        telemetry_sample,
        post_telemetry_sample,
        verification_expected_item_id,
        query_command,
        ..
      } => {
        assert_eq!(training_result_semantic_manifest_path, "/tmp/semantic.json");
        assert_eq!(target_block, "1,2,3");
        assert_eq!(target_face.as_deref(), Some("north"));
        assert_eq!(target_semantics, "block_center");
        assert!(use_closed_scene_toy_provider);
        assert_eq!(closed_scene_fixture_path.as_deref(), Some("/tmp/fixture.json"));
        assert_eq!(output_dir, "/tmp/query");
        assert_eq!(target_app, "com.mojang.minecraft");
        assert_eq!(target_title, "Minecraft");
        assert_eq!(telemetry_sample.as_deref(), Some("/tmp/pre.jsonl"));
        assert_eq!(post_telemetry_sample.as_deref(), Some("/tmp/post.jsonl"));
        assert!(verification_expected_item_id.is_none());
        assert!(query_command.is_none());
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_requires_target_app() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("target-app should be required");

    assert!(error.contains("--target-app is required"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_accepts_verification_expected_item_id() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
      "--sample".to_string(),
      "/tmp/pre.jsonl".to_string(),
      "--verification-expected-item-id".to_string(),
      "minecraft:stone".to_string(),
    ])
    .expect("verification expected item id should parse");

    match command {
      CliCommand::MinecraftQueryWiredLiveClick {
        telemetry_sample,
        verification_expected_item_id,
        ..
      } => {
        assert_eq!(telemetry_sample.as_deref(), Some("/tmp/pre.jsonl"));
        assert_eq!(verification_expected_item_id.as_deref(), Some("minecraft:stone"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_expected_item_without_sample() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
      "--verification-expected-item-id".to_string(),
      "minecraft:stone".to_string(),
    ])
    .expect_err("expected item id without sample should fail");

    assert!(error.contains("--verification-expected-item-id requires --sample"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_post_sample_without_sample() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
      "--post-sample".to_string(),
      "/tmp/post.jsonl".to_string(),
    ])
    .expect_err("post-sample without sample should fail");

    assert!(error.contains("--post-sample requires --sample"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_unexpected_argument() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
      "--extra".to_string(),
    ])
    .expect_err("unexpected flag should fail");

    assert!(error.contains("unexpected minecraft query-wired-live-click argument --extra"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_conflicting_provider_flags() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--query-command".to_string(),
      "python3 query.py".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("conflicting provider flags should fail");

    assert!(error.contains("mutually exclusive"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_orphan_closed_scene_fixture() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/mc18-fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("orphan closed-scene fixture should fail");

    assert!(error.contains("--closed-scene-fixture requires --query-provider closed-scene-toy"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_closed_scene_toy_without_fixture() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "closed-scene-toy".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("closed-scene-toy without fixture should fail");

    assert!(error.contains("--closed-scene-fixture is required"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_dual_query_providers() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--query-provider".to_string(),
      "closed-scene-toy".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/mc18-fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("dual providers should fail");

    assert!(error.contains("mutually exclusive"));
  }

  #[test]
  fn parse_minecraft_query_wired_live_click_command_rejects_checkpoint_native_with_fixture() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-wired-live-click".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/mc18-fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--target-app".to_string(),
      "com.mojang.minecraft".to_string(),
      "--target-title".to_string(),
      "Minecraft".to_string(),
    ])
    .expect_err("checkpoint-native with fixture should fail");

    assert!(error.contains("--closed-scene-fixture requires --query-provider closed-scene-toy"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_requires_semantic_manifest() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("semantic manifest should be required");

    assert!(error.contains("--training-result-semantic-manifest is required"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_requires_target_block() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("target block should be required");

    assert!(error.contains("--target-block is required"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_rejects_invalid_target_block() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "bad".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("invalid target block should fail in main parser");

    assert!(error.contains("invalid --target-block"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_rejects_invalid_target_face() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--target-face".to_string(),
      "diagonal".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("invalid target face should fail");

    assert!(error.contains("invalid --target-face"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_rejects_unexpected_argument() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
      "--extra".to_string(),
      "nope".to_string(),
    ])
    .expect_err("unexpected argument should fail");

    assert!(error.contains("unexpected minecraft query-3dgs-training-result argument --extra"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_parses_optional_flags() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--target-face".to_string(),
      "north".to_string(),
      "--target-semantics".to_string(),
      "block_center".to_string(),
      "--query-command".to_string(),
      "python3 query.py".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect("query command should parse");

    match command {
      CliCommand::MinecraftQuery3dgsTrainingResult {
        training_result_semantic_manifest_path,
        target_block,
        target_face,
        target_semantics,
        query_command,
        use_checkpoint_native_provider,
        output_dir,
        ..
      } => {
        assert_eq!(training_result_semantic_manifest_path, "/tmp/semantic.json");
        assert_eq!(target_block, "1,2,3");
        assert_eq!(target_face.as_deref(), Some("north"));
        assert_eq!(target_semantics, "block_center");
        assert_eq!(query_command.as_deref(), Some("python3 query.py"));
        assert!(!use_checkpoint_native_provider);
        assert_eq!(output_dir, "/tmp/query");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_parses_checkpoint_native_provider() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect("checkpoint-native provider should parse");

    match command {
      CliCommand::MinecraftQuery3dgsTrainingResult {
        use_checkpoint_native_provider,
        query_command,
        ..
      } => {
        assert!(use_checkpoint_native_provider);
        assert!(query_command.is_none());
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_rejects_conflicting_provider_flags() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--query-command".to_string(),
      "python3 query.py".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("conflicting provider flags should fail");

    assert!(error.contains("mutually exclusive"));
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_parses_closed_scene_toy_provider() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "511,73,728".to_string(),
      "--target-face".to_string(),
      "north".to_string(),
      "--query-provider".to_string(),
      "closed-scene-toy".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/mc18-fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect("closed-scene-toy provider should parse");

    match command {
      CliCommand::MinecraftQuery3dgsTrainingResult {
        use_closed_scene_toy_provider,
        closed_scene_fixture_path,
        query_command,
        ..
      } => {
        assert!(use_closed_scene_toy_provider);
        assert_eq!(closed_scene_fixture_path.as_deref(), Some("/tmp/mc18-fixture.json"));
        assert!(query_command.is_none());
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_query_3dgs_training_result_command_rejects_dual_query_providers() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "query-3dgs-training-result".to_string(),
      "--training-result-semantic-manifest".to_string(),
      "/tmp/semantic.json".to_string(),
      "--target-block".to_string(),
      "1,2,3".to_string(),
      "--query-provider".to_string(),
      "checkpoint-native".to_string(),
      "--query-provider".to_string(),
      "closed-scene-toy".to_string(),
      "--closed-scene-fixture".to_string(),
      "/tmp/mc18-fixture.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/query".to_string(),
    ])
    .expect_err("dual providers should fail");

    assert!(error.contains("mutually exclusive"));
  }

  #[test]
  fn parse_minecraft_validate_3dgs_training_result_command_requires_manifest() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "validate-3dgs-training-result".to_string(),
      "--output-dir".to_string(),
      "/tmp/semantic".to_string(),
    ])
    .expect_err("manifest should be required");

    assert!(error.contains("--training-result-artifact-manifest is required"));
  }

  #[test]
  fn parse_minecraft_validate_3dgs_training_result_command_parses_flags() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "validate-3dgs-training-result".to_string(),
      "--training-result-artifact-manifest".to_string(),
      "/tmp/training-result-artifacts/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/semantic".to_string(),
    ])
    .expect("validate command should parse");

    match command {
      CliCommand::MinecraftValidate3dgsTrainingResult {
        training_result_artifact_manifest_path,
        output_dir,
        ..
      } => {
        assert!(training_result_artifact_manifest_path.ends_with("minecraft-3dgs-training-result-artifact-manifest.json"));
        assert_eq!(output_dir, "/tmp/semantic");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_fetch_3dgs_training_result_artifacts_command_with_artifact_fetch_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "fetch-3dgs-training-result-artifacts".to_string(),
      "--training-result-manifest".to_string(),
      "/tmp/training-result/minecraft-3dgs-training-result.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/result-artifacts".to_string(),
      "--artifact-fetch-command".to_string(),
      "python3 fetch.py".to_string(),
    ])
    .expect("minecraft fetch-3dgs-training-result-artifacts command should parse");

    match command {
      CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
        training_result_manifest_path,
        output_dir,
        training_job_endpoint,
        training_job_token,
        artifact_fetch_command,
        ..
      } => {
        assert_eq!(training_result_manifest_path, "/tmp/training-result/minecraft-3dgs-training-result.json");
        assert_eq!(output_dir, "/tmp/result-artifacts");
        assert_eq!(training_job_endpoint, None);
        assert_eq!(training_job_token, None);
        assert_eq!(artifact_fetch_command.as_deref(), Some("python3 fetch.py"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_fetch_3dgs_training_result_artifacts_command_with_remote_config_flags() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "fetch-3dgs-training-result-artifacts".to_string(),
      "--training-result-manifest".to_string(),
      "/tmp/training-result/minecraft-3dgs-training-result.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/result-artifacts".to_string(),
      "--training-job-endpoint".to_string(),
      "https://jobs.example.test/v1".to_string(),
      "--training-job-token".to_string(),
      "secret-token".to_string(),
      "--artifact-fetch-command".to_string(),
      "python3 fetch.py".to_string(),
    ])
    .expect("minecraft fetch command with remote config should parse");

    match command {
      CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
        training_result_manifest_path,
        output_dir,
        training_job_endpoint,
        training_job_token,
        artifact_fetch_command,
        ..
      } => {
        assert_eq!(training_result_manifest_path, "/tmp/training-result/minecraft-3dgs-training-result.json");
        assert_eq!(output_dir, "/tmp/result-artifacts");
        assert_eq!(training_job_endpoint.as_deref(), Some("https://jobs.example.test/v1"));
        assert_eq!(training_job_token.as_deref(), Some("secret-token"));
        assert_eq!(artifact_fetch_command.as_deref(), Some("python3 fetch.py"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_prepare_3dgs_training_requires_manifest() {
    let error = parse_cli(&[
      "minecraft".to_string(),
      "prepare-3dgs-training".to_string(),
      "--output-dir".to_string(),
      "/tmp/launch".to_string(),
    ])
    .expect_err("missing training package manifest should fail");

    assert_eq!(error, "--training-package-manifest is required");
  }

  #[test]
  fn parse_minecraft_prepare_texture_sweep_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "prepare-texture-sweep".to_string(),
      "--sidecar-run-dir".to_string(),
      "devtools/auv-game-minecraft/run".to_string(),
      "--output-dir".to_string(),
      ".tmp-mc6-prep".to_string(),
    ])
    .expect("minecraft prepare-texture-sweep command should parse");

    match command {
      CliCommand::MinecraftPrepareTextureSweep {
        sidecar_run_dir,
        output_dir,
        ..
      } => {
        assert_eq!(sidecar_run_dir, "devtools/auv-game-minecraft/run");
        assert_eq!(output_dir, ".tmp-mc6-prep");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_build_texture_sweep_samples_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "build-texture-sweep-samples".to_string(),
      "--bundle-manifest".to_string(),
      "/tmp/rich/run.json".to_string(),
      "--bundle-manifest".to_string(),
      "/tmp/flat/run.json".to_string(),
      "--output".to_string(),
      "/tmp/samples.json".to_string(),
    ])
    .expect("minecraft build-texture-sweep-samples command should parse");

    match command {
      CliCommand::MinecraftBuildTextureSweepSamples {
        bundle_manifest_paths,
        output_path,
        ..
      } => {
        assert_eq!(bundle_manifest_paths, vec!["/tmp/rich/run.json", "/tmp/flat/run.json"]);
        assert_eq!(output_path, "/tmp/samples.json");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_eval_texture_sweep_command() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "eval-texture-sweep".to_string(),
      "--samples".to_string(),
      "/tmp/samples.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/mc6-sweep".to_string(),
    ])
    .expect("minecraft eval-texture-sweep command should parse");

    match command {
      CliCommand::MinecraftEvalTextureSweep {
        samples_path,
        output_dir,
        require_real_source,
        ..
      } => {
        assert_eq!(samples_path, "/tmp/samples.json");
        assert_eq!(output_dir, "/tmp/mc6-sweep");
        assert!(!require_real_source);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_minecraft_eval_texture_sweep_real_source_gate() {
    let command = parse_cli(&[
      "minecraft".to_string(),
      "eval-texture-sweep".to_string(),
      "--samples".to_string(),
      "/tmp/samples.json".to_string(),
      "--output-dir".to_string(),
      "/tmp/mc6-sweep".to_string(),
      "--require-real-source".to_string(),
    ])
    .expect("minecraft eval-texture-sweep command should parse real-source gate");

    match command {
      CliCommand::MinecraftEvalTextureSweep {
        require_real_source,
        ..
      } => assert!(require_real_source),
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_session_serve_command() {
    let command = parse_cli(&[
      "session".to_string(),
      "serve".to_string(),
      "--host".to_string(),
      "127.0.0.1".to_string(),
      "--port".to_string(),
      "9847".to_string(),
    ])
    .expect("session serve command should parse");

    match command {
      CliCommand::SessionServe {
        host,
        port,
        store_root,
      } => {
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9847);
        assert_eq!(store_root, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_session_serve_store_root_option() {
    let command = parse_cli(&[
      "session".to_string(),
      "serve".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-session-store".to_string(),
    ])
    .expect("session serve options should parse");

    match command {
      CliCommand::SessionServe {
        host,
        port,
        store_root,
      } => {
        assert_eq!(host, auv_runtime::api::session_service::transport::DEFAULT_SESSION_API_HOST);
        assert_eq!(port, auv_runtime::api::session_service::transport::DEFAULT_SESSION_API_PORT);
        assert_eq!(store_root.as_deref(), Some("/tmp/auv-session-store"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_session_serve_rejects_unknown_argument() {
    let error = parse_cli(&[
      "session".to_string(),
      "serve".to_string(),
      "--enable-write".to_string(),
    ])
    .expect_err("unexpected session serve flag should fail");

    assert!(error.contains("unexpected session-serve argument --enable-write"));
  }

  #[test]
  fn parse_mcp_command() {
    let command = parse_cli(&["mcp".to_string(), "serve".to_string()]).expect("mcp serve command should parse");

    match command {
      CliCommand::McpServe => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_app_probe_command() {
    let command = parse_cli(&[
      "app".to_string(),
      "probe".to_string(),
      "com.example.music".to_string(),
      "--output-dir".to_string(),
      "/tmp/probe".to_string(),
    ])
    .expect("app probe command should parse");

    match command {
      CliCommand::AppProbe {
        bundle_id,
        output_dir,
      } => {
        assert_eq!(bundle_id, "com.example.music");
        assert_eq!(output_dir.as_deref(), Some("/tmp/probe"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_inspect_command_accepts_store_root() {
    let command = parse_cli(&[
      "inspect".to_string(),
      "run_test_1".to_string(),
      "--store-root".to_string(),
      "/tmp/mc20-store".to_string(),
    ])
    .expect("inspect with store-root should parse");

    match command {
      CliCommand::Inspect { run_id, store_root } => {
        assert_eq!(run_id, "run_test_1");
        assert_eq!(store_root.as_deref(), Some("/tmp/mc20-store"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_inspect_command_rejects_unexpected_argument() {
    let error = parse_cli(&[
      "inspect".to_string(),
      "run_test_1".to_string(),
      "--extra".to_string(),
    ])
    .expect_err("unexpected inspect flag should fail");

    assert!(error.contains("unexpected auv inspect argument --extra"));
  }

  #[test]
  fn parse_inspect_serve_command() {
    let command = parse_cli(&[
      "inspect".to_string(),
      "serve".to_string(),
      "--host".to_string(),
      "0.0.0.0".to_string(),
      "--port".to_string(),
      "0".to_string(),
    ])
    .expect("inspect serve command should parse");

    match command {
      CliCommand::InspectServe {
        host,
        port,
        store_root,
        write,
      } => {
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 0);
        assert_eq!(store_root, None);
        assert_eq!(write, InspectServeWriteOptions::default());
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_inspect_serve_enable_write_option() {
    let command = parse_cli(&[
      "inspect".to_string(),
      "serve".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
      "--enable-write".to_string(),
    ])
    .expect("inspect serve options should parse");

    match command {
      CliCommand::InspectServe {
        host,
        port,
        store_root,
        write,
      } => {
        assert_eq!(host, auv_inspect_server::DEFAULT_INSPECT_HOST);
        assert_eq!(port, auv_inspect_server::DEFAULT_INSPECT_PORT);
        assert_eq!(store_root.as_deref(), Some("/tmp/auv-store"));
        assert!(write.enabled);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_inspect_serve_rejects_retired_write_token_flags_as_unknown() {
    for flag in ["--write-token", "--write-token-file", "--no-write-token"] {
      let mut arguments = vec!["inspect".to_string(), "serve".to_string(), flag.to_string()];
      if flag != "--no-write-token" {
        arguments.push("secret".to_string());
      }

      let error = parse_cli(&arguments).expect_err("retired Inspect serve token flag must fail");

      assert_eq!(error, format!("unexpected inspect-serve argument {flag}"));
    }
  }

  #[test]
  fn parse_list_commands_tombstone() {
    let command = parse_cli(&["list-commands".to_string()]).expect("list-commands should parse to tombstone command");
    match command {
      CliCommand::ListCommandsTombstone => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn root_donor_subcommand_tombstones() {
    assert!(root_donor_tombstone(&["godot".to_string()]).unwrap().contains("auv-godot"));
    assert!(root_donor_tombstone(&["osu".to_string()]).unwrap().contains("auv-osu"));
    assert!(root_donor_tombstone(&["minecraft".to_string()]).unwrap().contains("auv-minecraft"));
    assert!(root_donor_tombstone(&["inspect".to_string()]).is_none());
  }

  #[test]
  fn parse_donor_cli_godot_capability_query() {
    let command = parse_donor_cli("godot", &["capability-query".to_string(), "--json".to_string()]).expect("donor parse");
    match command {
      CliCommand::GodotCapabilityQuery { json: true } => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_help_without_command_id() {
    let command = parse_cli(&["invoke".to_string(), "--help".to_string()]).expect("invoke --help should parse");
    match command {
      CliCommand::InvokeHelp { command_id } => assert!(command_id.is_none()),
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_help_with_command_id() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "window.capture".to_string(),
      "--help".to_string(),
    ])
    .expect("invoke <command> --help should parse");
    match command {
      CliCommand::InvokeHelp { command_id } => {
        assert_eq!(command_id.as_deref(), Some("window.capture"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_inspect_write_options() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "window.capture".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
      "--inspect-local-write".to_string(),
      "default".to_string(),
      "--inspect-server-write".to_string(),
      "false".to_string(),
    ])
    .expect("invoke inspect options should parse");

    match command {
      CliCommand::Invoke {
        request, inspect, ..
      } => {
        assert_eq!(request.command_id, "window.capture");
        assert!(!request.dry_run);
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
        assert_eq!(inspect.local_write, InspectWriteSetting::Default);
        assert_eq!(inspect.server_write, InspectWriteSetting::Disabled);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_rejects_retired_inspect_server_token_flags() {
    for flag in ["--inspect-server-token", "--inspect-server-token-file"] {
      let error = parse_cli(&[
        "invoke".to_string(),
        "window.capture".to_string(),
        flag.to_string(),
        "secret".to_string(),
      ])
      .expect_err("retired inspect server token flag must fail");

      assert!(error.contains(flag), "{error}");
      assert!(error.contains("removed"), "{error}");
    }
  }

  #[test]
  fn parse_invoke_preserves_inspect_like_command_input_values() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "input.typeText".to_string(),
      "--text".to_string(),
      "--store-root".to_string(),
      "--label".to_string(),
      "literal-label".to_string(),
    ])
    .expect("invoke input value that looks like an inspect option should parse");

    match command {
      CliCommand::Invoke {
        request, inspect, ..
      } => {
        assert_eq!(request.command_id, "input.typeText");
        assert_eq!(request.inputs.get("text").map(String::as_str), Some("--store-root"));
        assert_eq!(request.inputs.get("label").map(String::as_str), Some("literal-label"));
        assert_eq!(inspect.store_root, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_dry_run_flag() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "qqmusic.playVisibleAnchor.v0".to_string(),
      "--dry-run".to_string(),
      "--target".to_string(),
      "com.tencent.QQMusicMac".to_string(),
    ])
    .expect("invoke dry-run should parse");

    match command {
      CliCommand::Invoke { request, .. } => {
        assert_eq!(request.command_id, "qqmusic.playVisibleAnchor.v0");
        assert!(request.dry_run);
        assert_eq!(request.target.application_id.as_deref(), Some("com.tencent.QQMusicMac"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_output_options_before_and_after_command() {
    let before = parse_cli(&[
      "invoke".to_string(),
      "--json".to_string(),
      "display.list".to_string(),
      "--detail".to_string(),
    ])
    .expect("invoke output options before command should parse");
    let after = parse_cli(&[
      "invoke".to_string(),
      "display.list".to_string(),
      "--detail".to_string(),
      "--json".to_string(),
    ])
    .expect("invoke output options after command should parse");

    match (before, after) {
      (
        CliCommand::Invoke {
          request: before_request,
          output: before_output,
          ..
        },
        CliCommand::Invoke {
          request: after_request,
          output: after_output,
          ..
        },
      ) => {
        assert_eq!(before_request.command_id, "display.list");
        assert_eq!(after_request.command_id, "display.list");
        assert_eq!(before_output, after_output);
        assert!(before_output.json);
        assert!(before_output.detail);
      }
      other => panic!("unexpected commands: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_output_flag_does_not_consume_command_input_flag() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "input.key".to_string(),
      "--json".to_string(),
      "--key".to_string(),
      "Cmd+L".to_string(),
    ])
    .expect("invoke output flag followed by command input should parse");

    match command {
      CliCommand::Invoke {
        request, output, ..
      } => {
        assert!(output.json);
        assert_eq!(request.command_id, "input.key");
        assert_eq!(request.inputs.get("key").map(String::as_str), Some("Cmd+L"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_xtask_generate_swift_bridge_command() {
    let command = parse_cli(&["--xtask".to_string(), "generate-swift-bridge".to_string()]).expect("xtask command should parse");

    match command {
      CliCommand::XtaskGenerateSwiftBridge => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_doctor_permission_check_command() {
    let command = parse_cli(&["doctor".to_string(), "--json".to_string()]).expect("doctor should parse");

    match command {
      CliCommand::PermissionCheck { json } => assert!(json),
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_permissions_check_command() {
    let command = parse_cli(&[
      "permissions".to_string(),
      "check".to_string(),
      "--json".to_string(),
    ])
    .expect("permissions check should parse");

    match command {
      CliCommand::PermissionCheck { json } => assert!(json),
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_osu_dispatch_with_capture_verify() {
    let command = parse_cli(&[
      "osu".to_string(),
      "dispatch".to_string(),
      "map.osu".to_string(),
      "--target-app".to_string(),
      "osu!".to_string(),
      "--dispatch-limit".to_string(),
      "3".to_string(),
      "--capture-verify".to_string(),
    ])
    .expect("osu dispatch with capture verification should parse");

    match command {
      CliCommand::OsuBenchmarkDispatch {
        beatmap_path,
        target_app,
        dispatch_limit,
        capture_verify,
        ..
      } => {
        assert_eq!(beatmap_path, "map.osu");
        assert_eq!(target_app, "osu!");
        assert_eq!(dispatch_limit, Some(3));
        assert!(capture_verify);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }
}
