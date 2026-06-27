// File: src/cli.rs
use auv_cli::candidate_action_decision::CandidateActionKind;
use auv_cli::model::{AuvResult, ExecutionTarget, InvokeRequest};
use auv_cli_invoke::InvokeCliParse;

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
      other => Err(format!(
        "invalid inspect write setting {other:?}; expected true, false, or default"
      )),
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
  pub server_token: Option<String>,
  pub server_token_file: Option<String>,
}

impl Default for InspectClientOptions {
  fn default() -> Self {
    Self {
      store_root: None,
      local_write: InspectWriteSetting::Default,
      server_write: InspectWriteSetting::Default,
      require_server_write: false,
      server_url: None,
      server_token: None,
      server_token_file: None,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct InspectServeWriteOptions {
  pub enabled: bool,
  pub token: Option<String>,
  pub token_file: Option<String>,
  pub no_token: bool,
}

#[derive(Debug)]
pub enum CliCommand {
  Help,
  PermissionCheck {
    json: bool,
  },
  CandidateActionRun {
    request: CandidateActionCommandRequest,
    inspect: InspectClientOptions,
  },
  ListCommandsTombstone,
  InvokeHelp {
    command_id: Option<String>,
  },
  AppProbe {
    bundle_id: String,
    output_dir: Option<String>,
  },
  AppAnalyze {
    query: String,
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
  MinecraftQuery3dgsTrainingResult {
    training_result_semantic_manifest_path: String,
    target_block: String,
    target_face: Option<String>,
    target_semantics: String,
    query_command: Option<String>,
    use_checkpoint_native_provider: bool,
    output_dir: String,
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
  },
  Inspect {
    run_id: String,
  },
  InspectServe {
    host: String,
    port: u16,
    store_root: Option<String>,
    write: InspectServeWriteOptions,
  },
  McpServe,
  // REVIEW: `scan window-region` is the first public scan CLI surface; keep
  // the name easy to revisit before treating scan terminology as stable.
  ScanWindowRegion {
    target: String,
    region: String,
    max_pages: usize,
    max_scrolls: usize,
    direction: String,
    scroll_amount: f64,
    settle_ms: u64,
    min_confidence: f64,
    max_observations: i64,
  },
  XtaskGenerateSwiftBridge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CandidateActionCommandRequest {
  pub app_bundle_id: String,
  pub query: Option<String>,
  pub role: Option<String>,
  pub action: Option<CandidateActionKind>,
  pub intent: Option<String>,
  pub proposer_model: Option<String>,
  pub proposer_base_url: Option<String>,
  pub reveal_shortcut: Option<String>,
  pub reveal_settle_ms: u64,
  pub stable_frames: u32,
  pub stable_frame_delay_ms: u64,
  pub max_centroid_drift_px: f64,
  pub require_stable_text: bool,
  pub dev_self_minted_consent: bool,
  pub human_gesture_consent: bool,
  pub human_gesture_timeout_ms: u64,
  pub proposal_id: String,
  pub promotion_id: String,
  pub decision_id: String,
  pub execution_id: String,
  pub granted_by: String,
  pub promotion_scope_note: String,
  pub promotion_evidence_note: String,
  pub execution_scope_note: String,
  pub execution_evidence_note: String,
}

pub fn parse_cli(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.is_empty() {
    return Ok(CliCommand::Help);
  }

  match arguments[0].as_str() {
    "help" | "--help" | "-h" => Ok(CliCommand::Help),
    "doctor" => parse_permission_check(arguments),
    "permissions" => parse_permissions(arguments),
    "--xtask" => parse_xtask(arguments),
    "candidate-action" => parse_candidate_action(arguments),
    "list-commands" => Ok(CliCommand::ListCommandsTombstone),
    "app" => parse_app(arguments),
    "osu" => parse_osu(arguments),
    "inspect" => parse_inspect(arguments),
    "mcp" => parse_mcp(arguments),
    "invoke" => parse_invoke(arguments),
    "minecraft" => parse_minecraft(arguments),
    "scan" => parse_scan(arguments),
    "skill" => {
      Err("skill commands have been removed; use app-local Rust commands instead".to_string())
    }
    other => Err(format!(
      "unknown subcommand {other}; use `help` to see supported commands"
    )),
  }
}

fn parse_xtask(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() != 2 {
    return Err("usage: auv-cli --xtask generate-swift-bridge".to_string());
  }

  match arguments[1].as_str() {
    "generate-swift-bridge" => Ok(CliCommand::XtaskGenerateSwiftBridge),
    other => Err(format!(
      "unknown xtask {other}; supported xtasks: generate-swift-bridge"
    )),
  }
}

pub fn help_text() -> String {
  String::from(
    "\
  auv-cli prototype

USAGE
  auv-cli doctor [--json]
  auv-cli permissions check [--json]
  auv-cli app probe <bundle-id> [--output-dir <dir>]
  auv-cli app analyze <probe-dir-or-probe-json>
  auv-cli osu benchmark <beatmap.osu> [--output-dir <dir>]
  auv-cli osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]
  auv-cli osu export-dataset <run-artifact-dir> --output-dir <dir>
  auv-cli osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]
  auv-cli osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]
  auv-cli invoke <command-id> [--dry-run] [--target <application-id>] [--label <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli inspect <run-id>
  auv-cli inspect serve [--host <host>] [--port <port>] [--store-root <path>] [--enable-write] [--write-token <token>] [--write-token-file <path>] [--no-write-token]
  auv-cli mcp serve
  auv-cli minecraft bridge --sample <telemetry.jsonl> (--screenshot <frame.png> | --capture-target-app <bundle-id> [--capture-target-title <window-title-substring>]) --target-block <x,y,z> [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft calibrate-projection --frame <minecraft-spatial-frame.json> --screenshot <frame.png> --target-block <x,y,z> [--target-semantics hit_face_center|block_center] [--screenshot-is-minecraft-window true|false] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft live-click --sample <telemetry.jsonl> --screenshot <frame.png> --target-block <x,y,z> --target-app <application-id> --target-title <window title> [--post-sample <telemetry.jsonl>] [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft export-spatial-bundle <run-id> --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft export-3dgs-scene-packet --bundle-manifest <bundle/run.json>... --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft export-3dgs-training-package --scene-packet-manifest <scene-packet/run.json> --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft prepare-3dgs-training --training-package-manifest <training-package/run.json> --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft launch-3dgs-training-job --training-launch-plan <training-launch-plan.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--training-job-submit-command <command>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft collect-3dgs-training-job-result --training-job-manifest <training-job.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--training-job-status-command <command>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft fetch-3dgs-training-result-artifacts --training-result-manifest <training-result.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--artifact-fetch-command <command>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft validate-3dgs-training-result --training-result-artifact-manifest <d11-manifest.json> --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft query-3dgs-training-result --training-result-semantic-manifest <semantic.json> --target-block <x,y,z> [--target-face <up|down|north|south|east|west>] [--target-semantics hit_face_center|block_center] [--query-command <command>] --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft inspect-3dgs-training-result-holdout --training-result-semantic-manifest <semantic.json> [--holdout-frame-index <n>] [--holdout-render-command <command>] --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft prepare-texture-sweep --sidecar-run-dir <dir> --output-dir <dir> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft build-texture-sweep-samples --bundle-manifest <bundle/run.json>... --output <samples.json> [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli minecraft eval-texture-sweep --samples <samples.json> --output-dir <dir> [--require-real-source] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli scan window-region --target <application-id> --region <left,top,right,bottom> [--direction up|down|left|right] [--max-pages <n>] [--max-scrolls <n>]
  auv-cli candidate-action run --target-app <bundle-id> [(--query <text> --role <ax-role> [--action click|type-text] [--text <content>]) | (--intent <text> [--proposer-model <id>] [--proposer-base-url <url>])] [(--dev-self-minted-consent --granted-by <who>) | (--human-gesture-consent [--granted-by <who>] [--human-gesture-timeout-ms <ms>])] [--reveal-shortcut <shortcut>] [--reveal-settle-ms <ms>] [--stable-frames <n>] [--stable-frame-delay-ms <ms>] [--max-centroid-drift-px <px>] [--require-stable-text true|false] [--proposal-id <id>] [--promotion-id <id>] [--decision-id <id>] [--execution-id <id>] [--promotion-scope-note <text>] [--promotion-evidence-note <text>] [--execution-scope-note <text>] [--execution-evidence-note <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]

NOTES
  - Names are provisional and reflect the current phase-0/1 runtime skeleton.
  - The CLI is a thin frontend over the library runtime in src/lib.rs.
  - `invoke --help` is the discovery surface for canonical invoke commands in the current C1 scaffold.
  - `list-commands` has been retired; use `auv-cli invoke --help` instead.
  - `overlay.showCursor`, `overlay.hideCursor`, and `overlay.shutdown` are visual-only macOS overlay probes; standalone `invoke` calls run in separate Rust processes, so use `--hold_ms` on show when manually observing the overlay.
  - `window.captureAxTree`, `input.focusText`, and `input.pressButton` accept `--reveal_shortcut cmd+f`-style hints when an app hides the target UI until a keyboard shortcut reveals it.
  - `candidate-action run` is a frozen archived macOS AX copilot vertical kept for recovery and reference. It stays buildable, but it is not the active AUV roadmap or the default product path.
  - By default `candidate-action run` does not self-mint consent; without an external consent source it records promotion refusal honestly. `--dev-self-minted-consent` exists only for local development smoke. `--human-gesture-consent` mints one local human-approved consent through a native macOS approval prompt.
  - `candidate-action run --intent ...` remains proposer-only inside that archived vertical: it chooses one observed AX item and one action, records that proposal, then feeds the existing refusal-first candidate-action spine unchanged.
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

fn parse_permission_check(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut json = false;
  for argument in arguments.iter().skip(1) {
    match argument.as_str() {
      "--json" => json = true,
      other => {
        return Err(format!(
          "unknown doctor option {other}; usage: auv-cli doctor [--json]"
        ));
      }
    }
  }

  Ok(CliCommand::PermissionCheck { json })
}

fn parse_candidate_action(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 || arguments[1] != "run" {
    return Err("usage: auv-cli candidate-action run --target-app <bundle-id> [(--query <text> --role <ax-role> [--action click|type-text] [--text <content>]) | (--intent <text> [--proposer-model <id>] [--proposer-base-url <url>])] [(--dev-self-minted-consent --granted-by <who>) | (--human-gesture-consent [--granted-by <who>] [--human-gesture-timeout-ms <ms>])] [--reveal-shortcut <shortcut>] [--reveal-settle-ms <ms>] [--stable-frames <n>] [--stable-frame-delay-ms <ms>] [--max-centroid-drift-px <px>] [--require-stable-text true|false] [--proposal-id <id>] [--promotion-id <id>] [--decision-id <id>] [--execution-id <id>] [--promotion-scope-note <text>] [--promotion-evidence-note <text>] [--execution-scope-note <text>] [--execution-evidence-note <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]".to_string());
  }

  let mut request = CandidateActionCommandRequest {
    app_bundle_id: String::new(),
    query: None,
    role: None,
    action: None,
    intent: None,
    proposer_model: None,
    proposer_base_url: None,
    reveal_shortcut: None,
    reveal_settle_ms: 250,
    stable_frames: 3,
    stable_frame_delay_ms: 150,
    max_centroid_drift_px: 4.0,
    require_stable_text: true,
    dev_self_minted_consent: false,
    human_gesture_consent: false,
    human_gesture_timeout_ms: 15_000,
    proposal_id: "candidate_proposal".to_string(),
    promotion_id: "candidate_promotion".to_string(),
    decision_id: "candidate_decision".to_string(),
    execution_id: "candidate_execution".to_string(),
    granted_by: String::new(),
    promotion_scope_note: "candidate promotion only".to_string(),
    promotion_evidence_note: "explicit candidate promotion consent".to_string(),
    execution_scope_note: "execute exactly one approved candidate action".to_string(),
    execution_evidence_note: "explicit single-action execution consent".to_string(),
  };
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--target-app" => {
        request.app_bundle_id = required_flag_value(arguments, index, "--target-app")?;
        index += 2;
      }
      "--intent" => {
        request.intent = Some(required_flag_value(arguments, index, "--intent")?);
        index += 2;
      }
      "--proposer-model" => {
        request.proposer_model = Some(required_flag_value(arguments, index, "--proposer-model")?);
        index += 2;
      }
      "--proposer-base-url" => {
        request.proposer_base_url = Some(required_flag_value(
          arguments,
          index,
          "--proposer-base-url",
        )?);
        index += 2;
      }
      "--query" => {
        request.query = Some(required_flag_value(arguments, index, "--query")?);
        index += 2;
      }
      "--role" => {
        request.role = Some(required_flag_value(arguments, index, "--role")?);
        index += 2;
      }
      "--action" => {
        let value = required_flag_value(arguments, index, "--action")?;
        request.action = Some(match value.as_str() {
          "click" => CandidateActionKind::Click,
          "type-text" => CandidateActionKind::TypeText {
            text: String::new(),
          },
          other => {
            return Err(format!(
              "invalid --action {other:?}; expected click or type-text"
            ));
          }
        });
        index += 2;
      }
      "--text" => {
        let value = required_flag_value(arguments, index, "--text")?;
        match request.action.get_or_insert(CandidateActionKind::Click) {
          CandidateActionKind::Click => {
            request.action = Some(CandidateActionKind::TypeText { text: value });
          }
          CandidateActionKind::TypeText { text } => *text = value,
        }
        index += 2;
      }
      "--granted-by" => {
        request.granted_by = required_flag_value(arguments, index, "--granted-by")?;
        index += 2;
      }
      "--dev-self-minted-consent" => {
        request.dev_self_minted_consent = true;
        index += 1;
      }
      "--human-gesture-consent" => {
        request.human_gesture_consent = true;
        index += 1;
      }
      "--human-gesture-timeout-ms" => {
        request.human_gesture_timeout_ms =
          required_flag_value(arguments, index, "--human-gesture-timeout-ms")?
            .parse::<u64>()
            .map_err(|error| format!("invalid --human-gesture-timeout-ms: {error}"))?;
        index += 2;
      }
      "--reveal-shortcut" => {
        request.reveal_shortcut = Some(required_flag_value(arguments, index, "--reveal-shortcut")?);
        index += 2;
      }
      "--reveal-settle-ms" => {
        request.reveal_settle_ms = required_flag_value(arguments, index, "--reveal-settle-ms")?
          .parse::<u64>()
          .map_err(|error| format!("invalid --reveal-settle-ms: {error}"))?;
        index += 2;
      }
      "--stable-frames" => {
        request.stable_frames = required_flag_value(arguments, index, "--stable-frames")?
          .parse::<u32>()
          .map_err(|error| format!("invalid --stable-frames: {error}"))?;
        index += 2;
      }
      "--stable-frame-delay-ms" => {
        request.stable_frame_delay_ms =
          required_flag_value(arguments, index, "--stable-frame-delay-ms")?
            .parse::<u64>()
            .map_err(|error| format!("invalid --stable-frame-delay-ms: {error}"))?;
        index += 2;
      }
      "--max-centroid-drift-px" => {
        request.max_centroid_drift_px =
          required_flag_value(arguments, index, "--max-centroid-drift-px")?
            .parse::<f64>()
            .map_err(|error| format!("invalid --max-centroid-drift-px: {error}"))?;
        index += 2;
      }
      "--require-stable-text" => {
        request.require_stable_text =
          required_flag_value(arguments, index, "--require-stable-text")?
            .parse::<bool>()
            .map_err(|error| format!("invalid --require-stable-text: {error}"))?;
        index += 2;
      }
      "--promotion-id" => {
        request.promotion_id = required_flag_value(arguments, index, "--promotion-id")?;
        index += 2;
      }
      "--proposal-id" => {
        request.proposal_id = required_flag_value(arguments, index, "--proposal-id")?;
        index += 2;
      }
      "--decision-id" => {
        request.decision_id = required_flag_value(arguments, index, "--decision-id")?;
        index += 2;
      }
      "--execution-id" => {
        request.execution_id = required_flag_value(arguments, index, "--execution-id")?;
        index += 2;
      }
      "--promotion-scope-note" => {
        request.promotion_scope_note =
          required_flag_value(arguments, index, "--promotion-scope-note")?;
        index += 2;
      }
      "--promotion-evidence-note" => {
        request.promotion_evidence_note =
          required_flag_value(arguments, index, "--promotion-evidence-note")?;
        index += 2;
      }
      "--execution-scope-note" => {
        request.execution_scope_note =
          required_flag_value(arguments, index, "--execution-scope-note")?;
        index += 2;
      }
      "--execution-evidence-note" => {
        request.execution_evidence_note =
          required_flag_value(arguments, index, "--execution-evidence-note")?;
        index += 2;
      }
      other => return Err(format!("unexpected candidate-action argument {other}")),
    }
  }

  if request.app_bundle_id.trim().is_empty() {
    return Err("--target-app is required".to_string());
  }
  if request.intent.is_some() {
    if request.query.is_some() || request.role.is_some() {
      return Err("--intent cannot be combined with --query or --role".to_string());
    }
  } else {
    if request.query.as_deref().unwrap_or("").trim().is_empty() {
      return Err("--query is required".to_string());
    }
    if request.role.as_deref().unwrap_or("").trim().is_empty() {
      return Err("--role is required".to_string());
    }
    if request.action.is_none() {
      request.action = Some(CandidateActionKind::Click);
    }
  }
  if request.dev_self_minted_consent && request.human_gesture_consent {
    return Err(
      "--dev-self-minted-consent cannot be combined with --human-gesture-consent".to_string(),
    );
  }
  if request.dev_self_minted_consent && request.granted_by.trim().is_empty() {
    return Err("--granted-by is required when --dev-self-minted-consent is set".to_string());
  }
  if request.human_gesture_timeout_ms == 0 {
    return Err("--human-gesture-timeout-ms must be greater than 0".to_string());
  }
  if let Some(CandidateActionKind::TypeText { text }) = &request.action
    && text.trim().is_empty()
  {
    return Err("--text must not be empty when --action type-text".to_string());
  }

  Ok(CliCommand::CandidateActionRun { request, inspect })
}

fn parse_permissions(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv-cli permissions check [--json]".to_string());
  }

  match arguments[1].as_str() {
    "check" => {
      let mut normalized = vec!["doctor".to_string()];
      normalized.extend(arguments.iter().skip(2).cloned());
      parse_permission_check(&normalized)
    }
    other => Err(format!(
      "unknown permissions subcommand {other}; usage: auv-cli permissions check [--json]"
    )),
  }
}

fn parse_app(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv-cli app <probe|analyze> ...".to_string());
  }

  match arguments[1].as_str() {
    "probe" => parse_app_probe(arguments),
    "analyze" => {
      if arguments.len() != 3 {
        return Err("usage: auv-cli app analyze <probe-dir-or-probe-json>".to_string());
      }
      Ok(CliCommand::AppAnalyze {
        query: arguments[2].clone(),
      })
    }
    "distill" | "validate" => Err(
      "app recipe distillation has been removed; use app-local Rust commands instead".to_string(),
    ),
    other => Err(format!(
      "unknown app subcommand {other}; use `auv-cli app probe` or `auv-cli app analyze`"
    )),
  }
}

fn parse_app_probe(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv-cli app probe <bundle-id> [--output-dir <dir>]".to_string());
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

fn parse_osu(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err(
      "usage: auv-cli osu <benchmark|dispatch|export-dataset|eval-detections|vision-demo> ..."
        .to_string(),
    );
  }

  match arguments[1].as_str() {
    "benchmark" => parse_osu_benchmark(arguments),
    "dispatch" => parse_osu_dispatch(arguments),
    "export-dataset" => parse_osu_export_dataset(arguments),
    "eval-detections" => parse_osu_eval_detections(arguments),
    "vision-demo" => parse_osu_vision_demo(arguments),
    other => Err(format!(
      "unknown osu subcommand {other}; use `auv-cli osu benchmark`, `auv-cli osu dispatch`, `auv-cli osu export-dataset`, `auv-cli osu eval-detections`, or `auv-cli osu vision-demo`"
    )),
  }
}

fn parse_osu_benchmark(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv-cli osu benchmark <beatmap.osu> [--output-dir <dir>]".to_string());
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
      "usage: auv-cli osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]".to_string(),
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
        dispatch_limit = Some(
          arguments[index + 1]
            .parse::<usize>()
            .map_err(|error| format!("invalid --dispatch-limit: {error}"))?,
        );
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
    return Err(
      "usage: auv-cli osu export-dataset <run-artifact-dir> --output-dir <dir>".to_string(),
    );
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
    return Err(
      "usage: auv-cli osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]".to_string(),
    );
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
      "usage: auv-cli osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]".to_string(),
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
        dispatch_limit = Some(
          arguments[index + 1]
            .parse::<usize>()
            .map_err(|error| format!("invalid --dispatch-limit: {error}"))?,
        );
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
    return Err(
      "usage: auv-cli inspect <run-id>|serve [--host <host>] [--port <port>]".to_string(),
    );
  }

  if arguments[1] == "serve" {
    return parse_inspect_serve(arguments);
  }

  if arguments.len() != 2 {
    return Err("usage: auv-cli inspect <run-id>".to_string());
  }

  Ok(CliCommand::Inspect {
    run_id: arguments[1].clone(),
  })
}

fn parse_inspect_serve(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut host = auv_cli::inspect_server::DEFAULT_INSPECT_HOST.to_string();
  let mut port = auv_cli::inspect_server::DEFAULT_INSPECT_PORT;
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
        port = arguments[index + 1]
          .parse::<u16>()
          .map_err(|error| format!("invalid --port value: {error}"))?;
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
      "--write-token" => {
        if index + 1 >= arguments.len() {
          return Err("--write-token requires a value".to_string());
        }
        write.enabled = true;
        write.token = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--write-token-file" => {
        if index + 1 >= arguments.len() {
          return Err("--write-token-file requires a value".to_string());
        }
        write.enabled = true;
        write.token_file = Some(arguments[index + 1].clone());
        index += 2;
      }
      "--no-write-token" => {
        write.no_token = true;
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

fn parse_inspect_client_option(
  argument: &str,
  value: Option<&String>,
  inspect: &mut InspectClientOptions,
) -> AuvResult<Option<usize>> {
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
    "--inspect-server-token" => {
      let value = value.ok_or_else(|| "--inspect-server-token requires a value".to_string())?;
      inspect.server_token = Some(value.clone());
      Ok(Some(2))
    }
    "--inspect-server-token-file" => {
      let value =
        value.ok_or_else(|| "--inspect-server-token-file requires a value".to_string())?;
      inspect.server_token_file = Some(value.clone());
      Ok(Some(2))
    }
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
    if let Some(consumed) =
      parse_inspect_client_option(argument, arguments.get(index + 1), &mut inspect)?
    {
      index += consumed;
      continue;
    }

    invoke_arguments.push(arguments[index].clone());
    if matches!(argument, "--dry-run" | "--help" | "-h") {
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
    }),
  }
}

fn parse_minecraft(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err(
      "usage: auv-cli minecraft <bridge|calibrate-projection|live-click|export-spatial-bundle|export-3dgs-scene-packet|export-3dgs-training-package|prepare-3dgs-training|launch-3dgs-training-job|collect-3dgs-training-job-result|fetch-3dgs-training-result-artifacts|validate-3dgs-training-result|query-3dgs-training-result|inspect-3dgs-training-result-holdout|prepare-texture-sweep|build-texture-sweep-samples|eval-texture-sweep> ..."
        .to_string(),
    );
  }

  match arguments[1].as_str() {
    "bridge" => parse_minecraft_bridge(arguments),
    "calibrate-projection" => parse_minecraft_calibrate_projection(arguments),
    "live-click" => parse_minecraft_live_click(arguments),
    "export-spatial-bundle" => parse_minecraft_export_spatial_bundle(arguments),
    "export-3dgs-scene-packet" => parse_minecraft_export_3dgs_scene_packet(arguments),
    "export-3dgs-training-package" => parse_minecraft_export_3dgs_training_package(arguments),
    "prepare-3dgs-training" => parse_minecraft_prepare_3dgs_training(arguments),
    "launch-3dgs-training-job" => parse_minecraft_launch_3dgs_training_job(arguments),
    "collect-3dgs-training-job-result" => {
      parse_minecraft_collect_3dgs_training_job_result(arguments)
    }
    "fetch-3dgs-training-result-artifacts" => {
      parse_minecraft_fetch_3dgs_training_result_artifacts(arguments)
    }
    "validate-3dgs-training-result" => parse_minecraft_validate_3dgs_training_result(arguments),
    "query-3dgs-training-result" => parse_minecraft_query_3dgs_training_result(arguments),
    "inspect-3dgs-training-result-holdout" => {
      parse_minecraft_inspect_3dgs_training_result_holdout(arguments)
    }
    "prepare-texture-sweep" => parse_minecraft_prepare_texture_sweep(arguments),
    "build-texture-sweep-samples" => parse_minecraft_build_texture_sweep_samples(arguments),
    "eval-texture-sweep" => parse_minecraft_eval_texture_sweep(arguments),
    other => Err(format!(
      "unknown minecraft subcommand {other}; expected bridge, calibrate-projection, live-click, export-spatial-bundle, export-3dgs-scene-packet, export-3dgs-training-package, prepare-3dgs-training, launch-3dgs-training-job, collect-3dgs-training-job-result, fetch-3dgs-training-result-artifacts, validate-3dgs-training-result, query-3dgs-training-result, inspect-3dgs-training-result-holdout, prepare-texture-sweep, build-texture-sweep-samples, or eval-texture-sweep"
    )),
  }
}

fn parse_minecraft_export_spatial_bundle(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 5 {
    return Err(
      "usage: auv-cli minecraft export-spatial-bundle <run-id> --output-dir <dir>".to_string(),
    );
  }

  let run_id = arguments[2].clone();
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 3;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft export-spatial-bundle argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        return Err(format!(
          "unexpected minecraft export-3dgs-scene-packet argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--scene-packet-manifest" => {
        scene_packet_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--scene-packet-manifest",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft export-3dgs-training-package argument {other}"
        ));
      }
    }
  }

  Ok(CliCommand::MinecraftExport3dgsTrainingPackage {
    scene_packet_manifest_path: scene_packet_manifest_path
      .ok_or_else(|| "--scene-packet-manifest is required".to_string())?,
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-artifact-manifest" => {
        training_result_artifact_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-result-artifact-manifest",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft validate-3dgs-training-result argument {other}"
        ));
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
    parts[index]
      .parse::<i32>()
      .map_err(|error| format!("invalid target block {label}: {error}"))?;
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
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-result-semantic-manifest",
        )?);
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
            return Err(format!(
              "invalid --target-face {other:?}; expected up, down, north, south, east, or west"
            ));
          }
        }
        index += 2;
      }
      "--target-semantics" => {
        let value = required_flag_value(arguments, index, "--target-semantics")?;
        match value.as_str() {
          "hit_face_center" | "block_center" => target_semantics = value,
          other => {
            return Err(format!(
              "invalid --target-semantics {other:?}; expected hit_face_center or block_center"
            ));
          }
        }
        index += 2;
      }
      "--query-provider" => {
        let value = required_flag_value(arguments, index, "--query-provider")?;
        if value != "checkpoint-native" {
          return Err(format!(
            "invalid --query-provider {value:?}; expected checkpoint-native"
          ));
        }
        use_checkpoint_native_provider = true;
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
        return Err(format!(
          "unexpected minecraft query-3dgs-training-result argument {other}"
        ));
      }
    }
  }

  let target_block = target_block.ok_or_else(|| "--target-block is required".to_string())?;
  validate_target_block_coordinates(&target_block)?;

  if use_checkpoint_native_provider && query_command.is_some() {
    return Err(
      "--query-provider checkpoint-native and --query-command are mutually exclusive".to_string(),
    );
  }

  Ok(CliCommand::MinecraftQuery3dgsTrainingResult {
    training_result_semantic_manifest_path: training_result_semantic_manifest_path
      .ok_or_else(|| "--training-result-semantic-manifest is required".to_string())?,
    target_block,
    target_face,
    target_semantics,
    query_command,
    use_checkpoint_native_provider,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    inspect,
  })
}

fn parse_minecraft_inspect_3dgs_training_result_holdout(
  arguments: &[String],
) -> AuvResult<CliCommand> {
  let mut training_result_semantic_manifest_path = None;
  let mut holdout_frame_index = None;
  let mut holdout_render_command = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-semantic-manifest" => {
        training_result_semantic_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-result-semantic-manifest",
        )?);
        index += 2;
      }
      "--holdout-frame-index" => {
        let value = required_flag_value(arguments, index, "--holdout-frame-index")?;
        holdout_frame_index = Some(
          value
            .parse::<usize>()
            .map_err(|error| format!("invalid --holdout-frame-index: {error}"))?,
        );
        index += 2;
      }
      "--holdout-render-command" => {
        holdout_render_command = Some(required_flag_value(
          arguments,
          index,
          "--holdout-render-command",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft inspect-3dgs-training-result-holdout argument {other}"
        ));
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

fn parse_minecraft_prepare_texture_sweep(arguments: &[String]) -> AuvResult<CliCommand> {
  let mut sidecar_run_dir = None;
  let mut output_dir = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        return Err(format!(
          "unexpected minecraft prepare-texture-sweep argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-package-manifest" => {
        training_package_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-package-manifest",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft prepare-3dgs-training argument {other}"
        ));
      }
    }
  }

  Ok(CliCommand::MinecraftPrepare3dgsTraining {
    training_package_manifest_path: training_package_manifest_path
      .ok_or_else(|| "--training-package-manifest is required".to_string())?,
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-launch-plan" => {
        training_launch_plan_path = Some(required_flag_value(
          arguments,
          index,
          "--training-launch-plan",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(
          arguments,
          index,
          "--training-job-endpoint",
        )?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(
          arguments,
          index,
          "--training-job-token",
        )?);
        index += 2;
      }
      "--training-job-submit-command" => {
        training_job_submit_command = Some(required_flag_value(
          arguments,
          index,
          "--training-job-submit-command",
        )?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft launch-3dgs-training-job argument {other}"
        ));
      }
    }
  }

  Ok(CliCommand::MinecraftLaunch3dgsTrainingJob {
    training_launch_plan_path: training_launch_plan_path
      .ok_or_else(|| "--training-launch-plan is required".to_string())?,
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-job-manifest" => {
        training_job_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-job-manifest",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(
          arguments,
          index,
          "--training-job-endpoint",
        )?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(
          arguments,
          index,
          "--training-job-token",
        )?);
        index += 2;
      }
      "--training-job-status-command" => {
        training_job_status_command = Some(required_flag_value(
          arguments,
          index,
          "--training-job-status-command",
        )?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft collect-3dgs-training-job-result argument {other}"
        ));
      }
    }
  }

  Ok(CliCommand::MinecraftCollect3dgsTrainingJobResult {
    training_job_manifest_path: training_job_manifest_path
      .ok_or_else(|| "--training-job-manifest is required".to_string())?,
    output_dir: output_dir.ok_or_else(|| "--output-dir is required".to_string())?,
    training_job_endpoint,
    training_job_token,
    training_job_status_command,
    inspect,
  })
}

fn parse_minecraft_fetch_3dgs_training_result_artifacts(
  arguments: &[String],
) -> AuvResult<CliCommand> {
  let mut training_result_manifest_path = None;
  let mut output_dir = None;
  let mut training_job_endpoint = None;
  let mut training_job_token = None;
  let mut artifact_fetch_command = None;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;
  while index < arguments.len() {
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
      index += consumed;
      continue;
    }

    match arguments[index].as_str() {
      "--training-result-manifest" => {
        training_result_manifest_path = Some(required_flag_value(
          arguments,
          index,
          "--training-result-manifest",
        )?);
        index += 2;
      }
      "--output-dir" => {
        output_dir = Some(required_flag_value(arguments, index, "--output-dir")?);
        index += 2;
      }
      "--training-job-endpoint" => {
        training_job_endpoint = Some(required_flag_value(
          arguments,
          index,
          "--training-job-endpoint",
        )?);
        index += 2;
      }
      "--training-job-token" => {
        training_job_token = Some(required_flag_value(
          arguments,
          index,
          "--training-job-token",
        )?);
        index += 2;
      }
      "--artifact-fetch-command" => {
        artifact_fetch_command = Some(required_flag_value(
          arguments,
          index,
          "--artifact-fetch-command",
        )?);
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft fetch-3dgs-training-result-artifacts argument {other}"
        ));
      }
    }
  }

  Ok(CliCommand::MinecraftFetch3dgsTrainingResultArtifacts {
    training_result_manifest_path: training_result_manifest_path
      .ok_or_else(|| "--training-result-manifest is required".to_string())?,
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        return Err(format!(
          "unexpected minecraft build-texture-sweep-samples argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        return Err(format!(
          "unexpected minecraft eval-texture-sweep argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        capture_target_app = Some(required_flag_value(
          arguments,
          index,
          "--capture-target-app",
        )?);
        index += 2;
      }
      "--capture-target-title" => {
        capture_target_title = Some(required_flag_value(
          arguments,
          index,
          "--capture-target-title",
        )?);
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
        screenshot_is_minecraft_window =
          required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
            .parse::<bool>()
            .map_err(|error| format!("invalid --screenshot-is-minecraft-window: {error}"))?;
        index += 2;
      }
      other => return Err(format!("unexpected minecraft bridge argument {other}")),
    }
  }

  if screenshot.is_some() && capture_target_app.is_some() {
    return Err(
      "--screenshot cannot be combined with --capture-target-app/--capture-target-title"
        .to_string(),
    );
  }
  if screenshot.is_none() && capture_target_app.is_none() {
    return Err(
      "minecraft bridge requires either --screenshot or --capture-target-app".to_string(),
    );
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
            return Err(format!(
              "invalid --target-semantics {other:?}; expected hit_face_center or block_center"
            ));
          }
        }
        index += 2;
      }
      "--screenshot-is-minecraft-window" => {
        screenshot_is_minecraft_window =
          required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
            .parse::<bool>()
            .map_err(|error| format!("invalid --screenshot-is-minecraft-window: {error}"))?;
        index += 2;
      }
      other => {
        return Err(format!(
          "unexpected minecraft calibrate-projection argument {other}"
        ));
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
    if let Some(consumed) = parse_inspect_client_option(
      arguments[index].as_str(),
      arguments.get(index + 1),
      &mut inspect,
    )? {
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
        screenshot_is_minecraft_window =
          required_flag_value(arguments, index, "--screenshot-is-minecraft-window")?
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

fn parse_scan(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 || arguments[1] != "window-region" {
    return Err("usage: auv-cli scan window-region --target <application-id> --region <left,top,right,bottom> [--max-pages <n>]".to_string());
  }

  let mut target = None;
  let mut region = None;
  let mut max_pages = 5usize;
  let mut max_scrolls = 4usize;
  let mut direction = "down".to_string();
  let mut scroll_amount = 6.0;
  let mut settle_ms = 250u64;
  let mut min_confidence = 0.0;
  let mut max_observations = 128i64;

  let mut index = 2;
  while index < arguments.len() {
    match arguments[index].as_str() {
      "--target" => {
        target = Some(required_flag_value(arguments, index, "--target")?);
        index += 2;
      }
      "--region" => {
        region = Some(required_flag_value(arguments, index, "--region")?);
        index += 2;
      }
      "--max-pages" => {
        max_pages = required_flag_value(arguments, index, "--max-pages")?
          .parse::<usize>()
          .map_err(|error| format!("invalid --max-pages: {error}"))?;
        if max_pages == 0 {
          return Err("--max-pages must be greater than 0".to_string());
        }
        index += 2;
      }
      "--max-scrolls" => {
        max_scrolls = required_flag_value(arguments, index, "--max-scrolls")?
          .parse::<usize>()
          .map_err(|error| format!("invalid --max-scrolls: {error}"))?;
        index += 2;
      }
      "--direction" => {
        direction = required_flag_value(arguments, index, "--direction")?;
        index += 2;
      }
      "--scroll-amount" => {
        scroll_amount = required_flag_value(arguments, index, "--scroll-amount")?
          .parse::<f64>()
          .map_err(|error| format!("invalid --scroll-amount: {error}"))?;
        index += 2;
      }
      "--settle-ms" => {
        settle_ms = required_flag_value(arguments, index, "--settle-ms")?
          .parse::<u64>()
          .map_err(|error| format!("invalid --settle-ms: {error}"))?;
        index += 2;
      }
      "--min-confidence" => {
        min_confidence = required_flag_value(arguments, index, "--min-confidence")?
          .parse::<f64>()
          .map_err(|error| format!("invalid --min-confidence: {error}"))?;
        index += 2;
      }
      "--max-observations" => {
        max_observations = required_flag_value(arguments, index, "--max-observations")?
          .parse::<i64>()
          .map_err(|error| format!("invalid --max-observations: {error}"))?;
        index += 2;
      }
      "--per-page-after-observe-recipe"
      | "--per-list-item-candidate-recipe"
      | "--on-stop-candidate-recipe" => {
        return Err(
          "scan recipe hooks have been removed; typed interaction hooks will replace them"
            .to_string(),
        );
      }
      other => return Err(format!("unexpected scan window-region argument {other}")),
    }
  }

  Ok(CliCommand::ScanWindowRegion {
    target: target.ok_or_else(|| "--target is required".to_string())?,
    region: region.ok_or_else(|| "--region is required".to_string())?,
    max_pages,
    max_scrolls,
    direction,
    scroll_amount,
    settle_ms,
    min_confidence,
    max_observations,
  })
}

fn parse_mcp(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() != 2 || arguments[1].as_str() != "serve" {
    return Err("usage: auv-cli mcp serve".to_string());
  }
  Ok(CliCommand::McpServe)
}

fn required_flag_value(arguments: &[String], index: usize, flag: &str) -> AuvResult<String> {
  arguments
    .get(index + 1)
    .cloned()
    .ok_or_else(|| format!("{flag} requires a value"))
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
      assert!(
        error.contains("skill commands have been removed"),
        "unexpected error for {args:?}: {error}"
      );
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
      assert!(
        error.contains("app recipe distillation has been removed"),
        "unexpected error for {args:?}: {error}"
      );
    }
  }

  #[test]
  fn help_text_lists_list_commands_tombstone() {
    let help = help_text();

    assert!(help.contains("list-commands"));
    assert!(help.contains("auv-cli invoke --help"));
    assert!(help.contains("retired"));
  }

  #[test]
  fn list_drivers_command_is_removed() {
    let error =
      parse_cli(&["list-drivers".to_string()]).expect_err("list-drivers should be removed");
    assert!(error.contains("unknown subcommand list-drivers"));

    let help = help_text();
    assert!(!help.contains("auv-cli list-drivers"));
  }

  #[test]
  fn parse_scan_window_region_command() {
    let command = parse_cli(&[
      "scan".to_string(),
      "window-region".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
      "--region".to_string(),
      "0.1,0.2,0.9,0.8".to_string(),
      "--max-pages".to_string(),
      "3".to_string(),
    ])
    .expect("scan window-region command should parse");

    match command {
      CliCommand::ScanWindowRegion {
        target,
        region,
        max_pages,
        ..
      } => {
        assert_eq!(target, "com.example.App");
        assert_eq!(region, "0.1,0.2,0.9,0.8");
        assert_eq!(max_pages, 3);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_scan_window_region_rejects_recipe_hooks() {
    for flag in [
      "--per-page-after-observe-recipe",
      "--per-list-item-candidate-recipe",
      "--on-stop-candidate-recipe",
    ] {
      let args = vec![
        "scan".to_string(),
        "window-region".to_string(),
        "--target".to_string(),
        "com.example.App".to_string(),
        "--region".to_string(),
        "0,0,1,1".to_string(),
        flag.to_string(),
        "recipes/scan/list-item-candidate-continue-hook.v0.json".to_string(),
      ];
      let error = parse_cli(&args).expect_err("recipe hook flags should be removed");
      assert!(
        error.contains("scan recipe hooks have been removed"),
        "unexpected error for {flag}: {error}"
      );
    }
  }

  #[test]
  fn parse_scan_window_region_rejects_zero_max_pages() {
    let error = parse_cli(&[
      "scan".to_string(),
      "window-region".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
      "--region".to_string(),
      "0.1,0.2,0.9,0.8".to_string(),
      "--max-pages".to_string(),
      "0".to_string(),
    ])
    .expect_err("zero max pages should fail");

    assert!(error.contains("--max-pages must be greater than 0"));
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

    assert!(
      error.contains("requires either --screenshot or --capture-target-app")
        || error.contains("--target-block is required")
    );
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
        assert_eq!(
          bundle_manifest_paths,
          vec!["/tmp/rich/run.json", "/tmp/flat/run.json"]
        );
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
        assert_eq!(
          training_launch_plan_path,
          "/tmp/training-launch/minecraft-3dgs-training-launch-plan.json"
        );
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
        assert_eq!(
          training_job_manifest_path,
          "/tmp/training-job/minecraft-3dgs-training-job.json"
        );
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
        assert_eq!(
          training_job_endpoint.as_deref(),
          Some("https://jobs.example.test/v1")
        );
        assert_eq!(training_job_token.as_deref(), Some("secret-token"));
        assert_eq!(
          training_job_submit_command.as_deref(),
          Some("remote-submit --json")
        );
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
        assert_eq!(
          training_job_endpoint.as_deref(),
          Some("https://jobs.example.test/v1")
        );
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
        assert_eq!(
          training_job_status_command.as_deref(),
          Some("python3 -c \"print(1)\"")
        );
      }
      other => panic!("unexpected command: {other:?}"),
    }
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
      "/tmp/training-result-artifacts/minecraft-3dgs-training-result-artifact-manifest.json"
        .to_string(),
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
        assert!(
          training_result_artifact_manifest_path
            .ends_with("minecraft-3dgs-training-result-artifact-manifest.json")
        );
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
        assert_eq!(
          training_result_manifest_path,
          "/tmp/training-result/minecraft-3dgs-training-result.json"
        );
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
        assert_eq!(
          training_result_manifest_path,
          "/tmp/training-result/minecraft-3dgs-training-result.json"
        );
        assert_eq!(output_dir, "/tmp/result-artifacts");
        assert_eq!(
          training_job_endpoint.as_deref(),
          Some("https://jobs.example.test/v1")
        );
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
      "sidecar/minecraft-telemetry/run".to_string(),
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
        assert_eq!(sidecar_run_dir, "sidecar/minecraft-telemetry/run");
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
        assert_eq!(
          bundle_manifest_paths,
          vec!["/tmp/rich/run.json", "/tmp/flat/run.json"]
        );
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
  fn parse_mcp_command() {
    let command =
      parse_cli(&["mcp".to_string(), "serve".to_string()]).expect("mcp serve command should parse");

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
  fn parse_inspect_serve_write_options() {
    let command = parse_cli(&[
      "inspect".to_string(),
      "serve".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
      "--enable-write".to_string(),
      "--write-token".to_string(),
      "secret".to_string(),
    ])
    .expect("inspect serve options should parse");

    match command {
      CliCommand::InspectServe {
        host,
        port,
        store_root,
        write,
      } => {
        assert_eq!(host, auv_cli::inspect_server::DEFAULT_INSPECT_HOST);
        assert_eq!(port, auv_cli::inspect_server::DEFAULT_INSPECT_PORT);
        assert_eq!(store_root.as_deref(), Some("/tmp/auv-store"));
        assert!(write.enabled);
        assert_eq!(write.token.as_deref(), Some("secret"));
        assert!(!write.no_token);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_list_commands_tombstone() {
    let command = parse_cli(&["list-commands".to_string()])
      .expect("list-commands should parse to tombstone command");
    match command {
      CliCommand::ListCommandsTombstone => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_help_without_command_id() {
    let command =
      parse_cli(&["invoke".to_string(), "--help".to_string()]).expect("invoke --help should parse");
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
      "--inspect-server-token-file".to_string(),
      "/tmp/token".to_string(),
    ])
    .expect("invoke inspect options should parse");

    match command {
      CliCommand::Invoke { request, inspect } => {
        assert_eq!(request.command_id, "window.capture");
        assert!(!request.dry_run);
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
        assert_eq!(inspect.local_write, InspectWriteSetting::Default);
        assert_eq!(inspect.server_write, InspectWriteSetting::Disabled);
        assert_eq!(inspect.server_token_file.as_deref(), Some("/tmp/token"));
      }
      other => panic!("unexpected command: {other:?}"),
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
      CliCommand::Invoke { request, inspect } => {
        assert_eq!(request.command_id, "input.typeText");
        assert_eq!(
          request.inputs.get("text").map(String::as_str),
          Some("--store-root")
        );
        assert_eq!(
          request.inputs.get("label").map(String::as_str),
          Some("literal-label")
        );
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
        assert_eq!(
          request.target.application_id.as_deref(),
          Some("com.tencent.QQMusicMac")
        );
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_xtask_generate_swift_bridge_command() {
    let command = parse_cli(&["--xtask".to_string(), "generate-swift-bridge".to_string()])
      .expect("xtask command should parse");

    match command {
      CliCommand::XtaskGenerateSwiftBridge => {}
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_doctor_permission_check_command() {
    let command =
      parse_cli(&["doctor".to_string(), "--json".to_string()]).expect("doctor should parse");

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

  #[test]
  fn parse_candidate_action_run_command() {
    let command = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--dev-self-minted-consent".to_string(),
      "--granted-by".to_string(),
      "human-review".to_string(),
      "--stable-frames".to_string(),
      "3".to_string(),
      "--max-centroid-drift-px".to_string(),
      "5.5".to_string(),
      "--require-stable-text".to_string(),
      "false".to_string(),
      "--reveal-shortcut".to_string(),
      "cmd+f".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
    ])
    .expect("candidate-action run should parse");

    match command {
      CliCommand::CandidateActionRun { request, inspect } => {
        assert_eq!(request.app_bundle_id, "com.apple.TextEdit");
        assert_eq!(request.query.as_deref(), Some("Body"));
        assert_eq!(request.role.as_deref(), Some("AXTextArea"));
        assert!(request.dev_self_minted_consent);
        assert_eq!(request.granted_by, "human-review");
        assert_eq!(request.stable_frames, 3);
        assert_eq!(request.max_centroid_drift_px, 5.5);
        assert!(!request.require_stable_text);
        assert_eq!(request.reveal_shortcut.as_deref(), Some("cmd+f"));
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_candidate_action_run_command_without_dev_self_minted_consent_does_not_require_granted_by()
   {
    let command = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
    ])
    .expect("candidate-action run without dev self-minted consent should parse");

    match command {
      CliCommand::CandidateActionRun { request, .. } => {
        assert!(!request.dev_self_minted_consent);
        assert!(!request.human_gesture_consent);
        assert_eq!(request.granted_by, "");
        assert_eq!(request.query.as_deref(), Some("Body"));
        assert_eq!(request.role.as_deref(), Some("AXTextArea"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_candidate_action_run_command_requires_granted_by_when_dev_self_minted_consent_is_enabled()
   {
    let error = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--dev-self-minted-consent".to_string(),
    ])
    .expect_err("dev self-minted consent without granted-by should be rejected");

    assert_eq!(
      error,
      "--granted-by is required when --dev-self-minted-consent is set"
    );
  }

  #[test]
  fn parse_candidate_action_run_command_with_human_gesture_consent() {
    let command = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--human-gesture-consent".to_string(),
      "--human-gesture-timeout-ms".to_string(),
      "4200".to_string(),
    ])
    .expect("candidate-action run with human gesture consent should parse");

    match command {
      CliCommand::CandidateActionRun { request, .. } => {
        assert!(request.human_gesture_consent);
        assert!(!request.dev_self_minted_consent);
        assert_eq!(request.human_gesture_timeout_ms, 4200);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_candidate_action_run_command_with_type_text_action() {
    let command = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--action".to_string(),
      "type-text".to_string(),
      "--text".to_string(),
      "hello from auv".to_string(),
    ])
    .expect("candidate-action run with type-text should parse");

    match command {
      CliCommand::CandidateActionRun { request, .. } => {
        assert_eq!(
          request.action,
          Some(
            auv_cli::candidate_action_decision::CandidateActionKind::TypeText {
              text: "hello from auv".to_string(),
            }
          )
        );
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_candidate_action_run_command_with_model_intent() {
    let command = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--intent".to_string(),
      "type hello into the main text area".to_string(),
      "--proposer-model".to_string(),
      "gpt-5.5".to_string(),
    ])
    .expect("candidate-action run with intent proposer should parse");

    match command {
      CliCommand::CandidateActionRun { request, .. } => {
        assert_eq!(
          request.intent.as_deref(),
          Some("type hello into the main text area")
        );
        assert_eq!(request.proposer_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(request.query, None);
        assert_eq!(request.role, None);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_candidate_action_run_command_rejects_intent_with_query() {
    let error = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--intent".to_string(),
      "type hello".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--proposer-model".to_string(),
      "gpt-5.5".to_string(),
    ])
    .expect_err("intent and query should be mutually exclusive");

    assert_eq!(error, "--intent cannot be combined with --query or --role");
  }

  #[test]
  fn parse_candidate_action_run_command_rejects_combined_consent_flags() {
    let error = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--dev-self-minted-consent".to_string(),
      "--human-gesture-consent".to_string(),
      "--granted-by".to_string(),
      "dev".to_string(),
    ])
    .expect_err("combined consent flags should be rejected");

    assert_eq!(
      error,
      "--dev-self-minted-consent cannot be combined with --human-gesture-consent"
    );
  }

  #[test]
  fn parse_candidate_action_run_command_rejects_zero_human_gesture_timeout() {
    let error = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
      "--human-gesture-timeout-ms".to_string(),
      "0".to_string(),
    ])
    .expect_err("zero human gesture timeout should be rejected");

    assert_eq!(error, "--human-gesture-timeout-ms must be greater than 0");
  }
}
