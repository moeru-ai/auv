// File: src/cli.rs
use std::collections::BTreeMap;

use auv_cli::candidate_action_decision::CandidateActionKind;
use auv_cli::model::{AuvResult, DisturbanceClass, ExecutionTarget, InvokeRequest};

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
  ListCommands,
  ListDrivers,
  AppProbe {
    bundle_id: String,
    output_dir: Option<String>,
  },
  AppAnalyze {
    query: String,
  },
  AppDistill {
    query: String,
    output_dir: Option<String>,
  },
  AppValidate {
    query: String,
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
  SkillList,
  SkillShow {
    query: String,
  },
  SkillCasesList,
  SkillCasesShow {
    query: String,
  },
  SkillCasesReport {
    query: String,
  },
  SkillCasesRun {
    query: String,
    dry_run: bool,
    max_disturbance: Option<DisturbanceClass>,
    only_case_ids: Vec<String>,
    include_nonvalidated: bool,
    inspect: InspectClientOptions,
  },
  SkillRun {
    query: String,
    dry_run: bool,
    max_disturbance: Option<DisturbanceClass>,
    overrides: BTreeMap<String, String>,
    inspect: InspectClientOptions,
  },
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
    per_page_after_observe_recipe: Option<String>,
    per_list_item_candidate_recipe: Option<String>,
    on_stop_candidate_recipe: Option<String>,
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
    "list-commands" => Ok(CliCommand::ListCommands),
    "list-drivers" => Ok(CliCommand::ListDrivers),
    "app" => parse_app(arguments),
    "inspect" => parse_inspect(arguments),
    "mcp" => parse_mcp(arguments),
    "invoke" => parse_invoke(arguments),
    "scan" => parse_scan(arguments),
    "skill" => parse_skill(arguments),
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
  auv-cli list-commands
  auv-cli list-drivers
  auv-cli doctor [--json]
  auv-cli permissions check [--json]
  auv-cli app probe <bundle-id> [--output-dir <dir>]
  auv-cli app analyze <probe-dir-or-probe-json>
  auv-cli app distill <analysis-dir-or-analysis-json> [--output-dir <dir>]
  auv-cli app validate <distill-dir-or-distillation-json>
  auv-cli invoke <command-id> [--dry-run] [--target <application-id>] [--label <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli inspect <run-id>
  auv-cli inspect serve [--host <host>] [--port <port>] [--store-root <path>] [--enable-write] [--write-token <token>] [--write-token-file <path>] [--no-write-token]
  auv-cli mcp serve
  auv-cli scan window-region --target <application-id> --region <left,top,right,bottom> [--direction up|down|left|right] [--max-pages <n>] [--max-scrolls <n>]
  auv-cli skill list
  auv-cli skill show <skill-id-or-path>
  auv-cli skill cases list
  auv-cli skill cases show <matrix-id-or-path>
  auv-cli skill cases report <matrix-id-or-path>
  auv-cli skill cases run <matrix-id-or-path> [--case <case-id>] [--all-statuses] [--dry-run] [--max-disturbance <class>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli skill run <skill-id-or-path> [--dry-run] [--max-disturbance <class>] [--set key=value] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
  auv-cli candidate-action run --target-app <bundle-id> [(--query <text> --role <ax-role> [--action click|type-text] [--text <content>]) | (--intent <text> [--proposer-model <id>] [--proposer-base-url <url>])] [(--dev-self-minted-consent --granted-by <who>) | (--human-gesture-consent [--granted-by <who>] [--human-gesture-timeout-ms <ms>])] [--reveal-shortcut <shortcut>] [--reveal-settle-ms <ms>] [--stable-frames <n>] [--stable-frame-delay-ms <ms>] [--max-centroid-drift-px <px>] [--require-stable-text true|false] [--proposal-id <id>] [--promotion-id <id>] [--decision-id <id>] [--execution-id <id>] [--promotion-scope-note <text>] [--promotion-evidence-note <text>] [--execution-scope-note <text>] [--execution-evidence-note <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]

NOTES
  - Names are provisional and reflect the current phase-0/1 runtime skeleton.
  - The CLI is a thin frontend over the library runtime in src/lib.rs.
  - `debug.captureDisplay`, `debug.listDisplays`, `debug.listWindows`, `debug.projectScreenshotPoint`, `debug.identifyPoint`, `debug.probeCoordinateReadiness`, `debug.captureAxTree`, `debug.probePermissions`, `debug.focusTextInput`, `debug.pressButton`, `verify.musicNowPlaying`, `verify.axText`, `debug.clickPoint`, and `debug.scrollPoint` are the current desktop donor entrypoints.
  - `debug.overlayShowCursor`, `debug.overlayHideCursor`, and `debug.overlayShutdown` are visual-only macOS overlay probes; standalone `invoke` calls run in separate Rust processes, so use `--hold_ms` on show when manually observing the overlay.
  - `debug.captureAxTree`, `debug.focusTextInput`, and `debug.pressButton` accept `--reveal_shortcut cmd+f`-style hints when an app hides the target UI until a keyboard shortcut reveals it.
  - `skill run` is a temporary JSON recipe compatibility entrypoint pending runtime legacy retirement; app-local Rust commands are the active workflow direction.
  - `candidate-action run` is a frozen archived macOS AX copilot vertical kept for recovery and reference. It stays buildable, but it is not the active AUV roadmap or the default product path.
  - By default `candidate-action run` does not self-mint consent; without an external consent source it records promotion refusal honestly. `--dev-self-minted-consent` exists only for local development smoke. `--human-gesture-consent` mints one local human-approved consent through a native macOS approval prompt.
  - `candidate-action run --intent ...` remains proposer-only inside that archived vertical: it chooses one observed AX item and one action, records that proposal, then feeds the existing refusal-first candidate-action spine unchanged.
  - `--reveal_settle_ms <millis>` can be used to make the reveal step explicit instead of depending on hard-coded timing assumptions.
  - `debug.typeText` supports `--replace_existing true`, `--submit_key return`, and `--submit_settle_ms 800` for repeatable text-entry flows.
  - `debug.pressKey` supports both special keys like `Return` and shortcuts like `cmd+f`, with optional `--settle_ms`.
  - `debug.clickWindowPoint` accepts either `--offset_x/--offset_y` or `--relative_x/--relative_y` against the target window bounds.
  - `debug.teachClick` captures a target window before a human-taught click, opens a small Ready prompt, records the next click as global/window-local coordinates, then captures follow-up frames at `--first_after_ms` and `--second_after_ms` (defaults 150/250).
  - `debug.findScreenText` and `debug.clickScreenText` use macOS Vision OCR over a captured screenshot and operate in screenshot-pixel anchors projected back to logical points.
  - `debug.waitForScreenText` polls that same OCR path until a filtered anchor appears or the timeout expires; use it when result-page readiness is the real problem instead of guessing longer sleeps.
  - `debug.findScreenRows`, `debug.waitForScreenRows`, and `debug.clickScreenRow` treat OCR observations as grouped visible rows, which is the current fallback direction when exact text anchors are visually present but not OCR-reliable.
  - `debug.findImageText` runs the same OCR matching over an existing image artifact, which is useful for verifying captured evidence without recapturing the live desktop.
  - `verify.musicNowPlaying` prefers AX tree matching for player-title verification, which is the current direction for native playback disambiguation.
  - `verify.axText` is the generic AX-tree text verification contract for native apps with reliable text-bearing nodes.
  - `debug.clickScreenText` supports `--match_index` and `--click_count` when the query resolves to multiple OCR anchors.
  - `skill cases run` replays validated case-matrix entries serially; this is the current narrow-skill coverage entrypoint for productization.
  - `app probe` is the deterministic raw-facts entrypoint for phase-2 distillation work; it records app identity plus runtime-backed surface probes into `.auv/app-probes/.../probe.json`.
  - `app analyze` turns one of those probe directories into `analysis.json` and `report.md`; use that as the input to later candidate-skill distillation instead of free-form chat summaries.
  - `app distill` turns one analyzed app surface into candidate recipe/case-matrix scaffolds that already pass the current skill validators; they are candidate outputs, not validated skills.
  - `app validate` turns one distillation directory into `validation.json` and `validation-report.md`; `validated` means the generated case matrix ran live, while `verification_mode=evidence-only` still means human review is required.
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
    return Err("usage: auv-cli app <probe|analyze|distill|validate> ...".to_string());
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
    "distill" => parse_app_distill(arguments),
    "validate" => {
      if arguments.len() != 3 {
        return Err("usage: auv-cli app validate <distill-dir-or-distillation-json>".to_string());
      }
      Ok(CliCommand::AppValidate {
        query: arguments[2].clone(),
      })
    }
    other => Err(format!(
      "unknown app subcommand {other}; use `auv-cli app probe`, `auv-cli app analyze`, `auv-cli app distill`, or `auv-cli app validate`"
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

fn parse_app_distill(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err(
      "usage: auv-cli app distill <analysis-dir-or-analysis-json> [--output-dir <dir>]".to_string(),
    );
  }
  let query = arguments[2].clone();
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
        return Err(format!("unexpected app-distill argument {other}"));
      }
    }
  }
  Ok(CliCommand::AppDistill { query, output_dir })
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
  if arguments.len() < 2 {
    return Err(
      "usage: auv-cli invoke <command-id> [--dry-run] [--target <application-id>] [--label <text>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]".to_string(),
    );
  }

  let command_id = arguments[1].clone();
  let mut target = ExecutionTarget::default();
  let mut inputs = BTreeMap::new();
  let mut dry_run = false;
  let mut inspect = InspectClientOptions::default();
  let mut index = 2;

  while index < arguments.len() {
    let argument = &arguments[index];
    if !argument.starts_with("--") {
      return Err(format!("unexpected positional argument {argument}"));
    }
    if argument == "--dry-run" {
      dry_run = true;
      index += 1;
      continue;
    }
    if let Some(consumed) =
      parse_inspect_client_option(argument.as_str(), arguments.get(index + 1), &mut inspect)?
    {
      index += consumed;
      continue;
    }
    if index + 1 >= arguments.len() {
      return Err(format!("flag {argument} requires a value"));
    }

    let value = arguments[index + 1].clone();
    match argument.as_str() {
      "--target" => {
        target.application_id = Some(value);
      }
      "--label" => {
        inputs.insert("label".to_string(), value);
      }
      other => {
        let key = other.trim_start_matches("--");
        inputs.insert(key.to_string(), value);
      }
    }

    index += 2;
  }

  Ok(CliCommand::Invoke {
    request: InvokeRequest {
      command_id,
      target,
      inputs,
      dry_run,
    },
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
  let mut per_page_after_observe_recipe = None;
  let mut per_list_item_candidate_recipe = None;
  let mut on_stop_candidate_recipe = None;

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
      "--per-page-after-observe-recipe" => {
        per_page_after_observe_recipe = Some(required_flag_value(
          arguments,
          index,
          "--per-page-after-observe-recipe",
        )?);
        index += 2;
      }
      "--per-list-item-candidate-recipe" => {
        per_list_item_candidate_recipe = Some(required_flag_value(
          arguments,
          index,
          "--per-list-item-candidate-recipe",
        )?);
        index += 2;
      }
      "--on-stop-candidate-recipe" => {
        on_stop_candidate_recipe = Some(required_flag_value(
          arguments,
          index,
          "--on-stop-candidate-recipe",
        )?);
        index += 2;
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
    per_page_after_observe_recipe,
    per_list_item_candidate_recipe,
    on_stop_candidate_recipe,
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

fn parse_skill(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err("usage: auv-cli skill <list|show|run> ...".to_string());
  }

  match arguments[1].as_str() {
    "list" => {
      if arguments.len() != 2 {
        return Err("usage: auv-cli skill list".to_string());
      }
      Ok(CliCommand::SkillList)
    }
    "cases" => parse_skill_cases(arguments),
    "show" => {
      if arguments.len() != 3 {
        return Err("usage: auv-cli skill show <skill-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillShow {
        query: arguments[2].clone(),
      })
    }
    "bundle" => {
      Err("skill bundle has been removed; use app-local Rust commands instead".to_string())
    }
    "run" => parse_skill_run(arguments),
    other => Err(format!(
      "unknown skill subcommand {other}; use `auv-cli skill list` to inspect the current catalog"
    )),
  }
}

fn parse_skill_cases(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv-cli skill cases <list|show|run> ...".to_string());
  }

  match arguments[2].as_str() {
    "list" => {
      if arguments.len() != 3 {
        return Err("usage: auv-cli skill cases list".to_string());
      }
      Ok(CliCommand::SkillCasesList)
    }
    "show" => {
      if arguments.len() != 4 {
        return Err("usage: auv-cli skill cases show <matrix-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillCasesShow {
        query: arguments[3].clone(),
      })
    }
    "report" => {
      if arguments.len() != 4 {
        return Err("usage: auv-cli skill cases report <matrix-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillCasesReport {
        query: arguments[3].clone(),
      })
    }
    "run" => parse_skill_cases_run(arguments),
    other => Err(format!(
      "unknown skill cases subcommand {other}; use `auv-cli skill cases list`"
    )),
  }
}

fn parse_skill_run(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err(
      "usage: auv-cli skill run <skill-id-or-path> [--dry-run] [--max-disturbance <class>] [--set key=value] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]".to_string(),
    );
  }

  let query = arguments[2].clone();
  let mut dry_run = false;
  let mut max_disturbance = None;
  let mut overrides = BTreeMap::new();
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
      "--dry-run" => {
        dry_run = true;
        index += 1;
      }
      "--max-disturbance" => {
        if index + 1 >= arguments.len() {
          return Err("--max-disturbance requires a value".to_string());
        }
        max_disturbance = Some(DisturbanceClass::parse(&arguments[index + 1])?);
        index += 2;
      }
      "--set" => {
        if index + 1 >= arguments.len() {
          return Err("--set requires key=value".to_string());
        }
        let raw = &arguments[index + 1];
        let Some((key, value)) = raw.split_once('=') else {
          return Err(format!("invalid --set value {raw:?}; expected key=value"));
        };
        if key.trim().is_empty() {
          return Err(format!("invalid --set value {raw:?}; missing key"));
        }
        overrides.insert(key.trim().to_string(), value.to_string());
        index += 2;
      }
      other => {
        return Err(format!("unexpected skill-run argument {other}"));
      }
    }
  }

  Ok(CliCommand::SkillRun {
    query,
    dry_run,
    max_disturbance,
    overrides,
    inspect,
  })
}

fn parse_skill_cases_run(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 4 {
    return Err(
      "usage: auv-cli skill cases run <matrix-id-or-path> [--case <case-id>] [--all-statuses] [--dry-run] [--max-disturbance <class>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]".to_string(),
    );
  }

  let query = arguments[3].clone();
  let mut dry_run = false;
  let mut max_disturbance = None;
  let mut only_case_ids = Vec::new();
  let mut include_nonvalidated = false;
  let mut inspect = InspectClientOptions::default();
  let mut index = 4;

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
      "--dry-run" => {
        dry_run = true;
        index += 1;
      }
      "--all-statuses" => {
        include_nonvalidated = true;
        index += 1;
      }
      "--case" => {
        if index + 1 >= arguments.len() {
          return Err("--case requires a value".to_string());
        }
        only_case_ids.push(arguments[index + 1].clone());
        index += 2;
      }
      "--max-disturbance" => {
        if index + 1 >= arguments.len() {
          return Err("--max-disturbance requires a value".to_string());
        }
        max_disturbance = Some(DisturbanceClass::parse(&arguments[index + 1])?);
        index += 2;
      }
      other => {
        return Err(format!("unexpected skill-cases-run argument {other}"));
      }
    }
  }

  Ok(CliCommand::SkillCasesRun {
    query,
    dry_run,
    max_disturbance,
    only_case_ids,
    include_nonvalidated,
    inspect,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_skill_bundle_commands_are_removed() {
    let error = parse_cli(&[
      "skill".to_string(),
      "bundle".to_string(),
      "list".to_string(),
    ])
    .expect_err("skill bundle should be removed");

    assert!(
      error.contains("skill bundle has been removed"),
      "unexpected error: {error}"
    );
  }

  #[test]
  fn help_text_no_longer_lists_skill_bundle_commands() {
    let help = help_text();

    assert!(!help.contains("skill bundle"));
    assert!(help.contains("auv-cli skill run"));
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
        per_list_item_candidate_recipe,
        ..
      } => {
        assert_eq!(target, "com.example.App");
        assert_eq!(region, "0.1,0.2,0.9,0.8");
        assert_eq!(max_pages, 3);
        assert!(per_list_item_candidate_recipe.is_none());
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_scan_window_region_accepts_per_list_item_candidate_recipe() {
    let command = parse_cli(&[
      "scan".to_string(),
      "window-region".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
      "--region".to_string(),
      "0.1,0.2,0.9,0.8".to_string(),
      "--per-list-item-candidate-recipe".to_string(),
      "scan.fixture.list_item_candidate_continue.v0".to_string(),
    ])
    .expect("scan window-region command should parse");

    match command {
      CliCommand::ScanWindowRegion {
        per_list_item_candidate_recipe,
        ..
      } => {
        assert_eq!(
          per_list_item_candidate_recipe.as_deref(),
          Some("scan.fixture.list_item_candidate_continue.v0")
        );
      }
      other => panic!("unexpected command: {other:?}"),
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
  fn parse_app_distill_command() {
    let command = parse_cli(&[
      "app".to_string(),
      "distill".to_string(),
      "/tmp/analysis".to_string(),
      "--output-dir".to_string(),
      "/tmp/out".to_string(),
    ])
    .expect("app distill command should parse");

    match command {
      CliCommand::AppDistill { query, output_dir } => {
        assert_eq!(query, "/tmp/analysis");
        assert_eq!(output_dir.as_deref(), Some("/tmp/out"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_app_validate_command() {
    let command = parse_cli(&[
      "app".to_string(),
      "validate".to_string(),
      "/tmp/distill".to_string(),
    ])
    .expect("app validate command should parse");
    match command {
      CliCommand::AppValidate { query } => {
        assert_eq!(query, "/tmp/distill");
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
  fn parse_skill_run_inspect_write_options() {
    let command = parse_cli(&[
      "skill".to_string(),
      "run".to_string(),
      "recipe.id".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
      "--inspect-local-write".to_string(),
      "false".to_string(),
      "--inspect-server-write".to_string(),
      "true".to_string(),
      "--require-inspect-server-write".to_string(),
      "--inspect-server-url".to_string(),
      "http://127.0.0.1:8765".to_string(),
      "--inspect-server-token".to_string(),
      "secret".to_string(),
    ])
    .expect("skill run inspect options should parse");

    match command {
      CliCommand::SkillRun { inspect, .. } => {
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
        assert_eq!(inspect.local_write, InspectWriteSetting::Disabled);
        assert_eq!(inspect.server_write, InspectWriteSetting::Enabled);
        assert!(inspect.require_server_write);
        assert_eq!(inspect.server_url.as_deref(), Some("http://127.0.0.1:8765"));
        assert_eq!(inspect.server_token.as_deref(), Some("secret"));
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_invoke_inspect_write_options() {
    let command = parse_cli(&[
      "invoke".to_string(),
      "debug.captureDisplay".to_string(),
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
        assert_eq!(request.command_id, "debug.captureDisplay");
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
  fn parse_skill_cases_run_inspect_write_options() {
    let command = parse_cli(&[
      "skill".to_string(),
      "cases".to_string(),
      "run".to_string(),
      "matrix.id".to_string(),
      "--case".to_string(),
      "case-1".to_string(),
      "--store-root".to_string(),
      "/tmp/auv-store".to_string(),
      "--inspect-server-url".to_string(),
      "http://127.0.0.1:8765".to_string(),
      "--require-inspect-server-write".to_string(),
    ])
    .expect("skill cases run inspect options should parse");

    match command {
      CliCommand::SkillCasesRun {
        only_case_ids,
        inspect,
        ..
      } => {
        assert_eq!(only_case_ids, vec!["case-1"]);
        assert_eq!(inspect.store_root.as_deref(), Some("/tmp/auv-store"));
        assert!(inspect.require_server_write);
        assert_eq!(inspect.server_url.as_deref(), Some("http://127.0.0.1:8765"));
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
