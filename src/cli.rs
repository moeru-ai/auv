use std::collections::BTreeMap;

use auv_cli::model::{AuvResult, DisturbanceClass, ExecutionTarget, InvokeRequest};

#[derive(Debug)]
pub enum CliCommand {
  Help,
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
  Invoke(InvokeRequest),
  Inspect {
    run_id: String,
  },
  InspectServe {
    host: String,
    port: u16,
  },
  SkillList,
  SkillShow {
    query: String,
  },
  SkillBundleList,
  SkillBundleShow {
    query: String,
  },
  SkillBundleCoverage {
    query: String,
  },
  SkillBundleVerify {
    query: String,
  },
  SkillBundleExport {
    query: String,
    output_dir: String,
  },
  SkillBundlePackageVerify {
    package_dir: String,
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
  },
  SkillRun {
    query: String,
    dry_run: bool,
    max_disturbance: Option<DisturbanceClass>,
    overrides: BTreeMap<String, String>,
  },
}

pub fn parse_cli(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.is_empty() {
    return Ok(CliCommand::Help);
  }

  match arguments[0].as_str() {
    "help" | "--help" | "-h" => Ok(CliCommand::Help),
    "list-commands" => Ok(CliCommand::ListCommands),
    "list-drivers" => Ok(CliCommand::ListDrivers),
    "app" => parse_app(arguments),
    "inspect" => parse_inspect(arguments),
    "invoke" => parse_invoke(arguments),
    "skill" => parse_skill(arguments),
    other => Err(format!(
      "unknown subcommand {other}; use `help` to see supported commands"
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
  auv-cli app probe <bundle-id> [--output-dir <dir>]
  auv-cli app analyze <probe-dir-or-probe-json>
  auv-cli app distill <analysis-dir-or-analysis-json> [--output-dir <dir>]
  auv-cli app validate <distill-dir-or-distillation-json>
  auv-cli invoke <command-id> [--target <application-id>] [--label <text>]
  auv-cli inspect <run-id>
  auv-cli inspect serve [--host <host>] [--port <port>]
  auv-cli skill list
  auv-cli skill show <skill-id-or-path>
  auv-cli skill bundle list
  auv-cli skill bundle show <bundle-id-or-path>
  auv-cli skill bundle coverage <bundle-id-or-path>
  auv-cli skill bundle verify <bundle-id-or-path>
  auv-cli skill bundle export <bundle-id-or-path> <output-dir>
  auv-cli skill bundle package verify <package-dir>
  auv-cli skill cases list
  auv-cli skill cases show <matrix-id-or-path>
  auv-cli skill cases report <matrix-id-or-path>
  auv-cli skill cases run <matrix-id-or-path> [--case <case-id>] [--all-statuses] [--dry-run] [--max-disturbance <class>]
  auv-cli skill run <skill-id-or-path> [--dry-run] [--max-disturbance <class>] [--set key=value]

NOTES
  - Names are provisional and reflect the current phase-0/1 runtime skeleton.
  - The CLI is a thin frontend over the library runtime in src/lib.rs.
  - `debug.captureDisplay`, `debug.listDisplays`, `debug.projectScreenshotPoint`, `debug.identifyPoint`, `debug.probeCoordinateReadiness`, `debug.observeWindows`, `debug.observeWindowTree`, `debug.probePermissions`, `debug.focusTextInput`, `debug.pressButton`, `debug.verifyNowPlayingTitle`, `debug.verifyAxText`, `debug.clickPoint`, and `debug.scrollPoint` are the current desktop donor entrypoints.
  - `debug.observeWindowTree`, `debug.focusTextInput`, and `debug.pressButton` accept `--reveal_shortcut cmd+f`-style hints when an app hides the target UI until a keyboard shortcut reveals it.
  - `--reveal_settle_ms <millis>` can be used to make the reveal step explicit instead of depending on hard-coded timing assumptions.
  - `debug.typeText` supports `--replace_existing true`, `--submit_key return`, and `--submit_settle_ms 800` for repeatable text-entry flows.
  - `debug.pressKey` supports both special keys like `Return` and shortcuts like `cmd+f`, with optional `--settle_ms`.
  - `debug.clickWindowPoint` accepts either `--offset_x/--offset_y` or `--relative_x/--relative_y` against the target window bounds.
  - `debug.findScreenText` and `debug.clickScreenText` use macOS Vision OCR over a captured screenshot and operate in screenshot-pixel anchors projected back to logical points.
  - `debug.waitForScreenText` polls that same OCR path until a filtered anchor appears or the timeout expires; use it when result-page readiness is the real problem instead of guessing longer sleeps.
  - `debug.findScreenRows`, `debug.waitForScreenRows`, and `debug.clickScreenRow` treat OCR observations as grouped visible rows, which is the current fallback direction when exact text anchors are visually present but not OCR-reliable.
  - `debug.findImageText` runs the same OCR matching over an existing image artifact, which is useful for verifying captured evidence without recapturing the live desktop.
  - `debug.verifyNowPlayingTitle` prefers AX tree matching for player-title verification, which is the current direction for QQ音乐 playback disambiguation.
  - `debug.verifyAxText` is the generic AX-tree text verification contract for native apps with reliable text-bearing nodes.
  - `debug.clickScreenText` supports `--match_index` and `--click_count` when the query resolves to multiple OCR anchors.
  - `skill run` is the product-facing recipe entrypoint: it resolves a recipe manifest from `recipes/`, validates disturbance policy, replays steps through the shared runtime, and carries step artifact paths into later verification steps.
  - `skill cases run` replays validated case-matrix entries serially; this is the current narrow-skill coverage entrypoint for QQ音乐 productization.
  - `app probe` is the deterministic raw-facts entrypoint for phase-2 distillation work; it records app identity plus runtime-backed surface probes into `.auv/app-probes/.../probe.json`.
  - `app analyze` turns one of those probe directories into `analysis.json` and `report.md`; use that as the input to later candidate-skill distillation instead of free-form chat summaries.
  - `app distill` turns one analyzed app surface into candidate recipe/case-matrix scaffolds that already pass the current skill validators; they are candidate outputs, not validated skills.
  - `app validate` turns one distillation directory into `validation.json` and `validation-report.md`; `validated` means the generated case matrix ran live, while `verification_mode=evidence-only` still means human review is required.
",
  )
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
      other => {
        return Err(format!("unexpected inspect-serve argument {other}"));
      }
    }
  }

  Ok(CliCommand::InspectServe { host, port })
}

fn parse_invoke(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 2 {
    return Err(
      "usage: auv-cli invoke <command-id> [--target <application-id>] [--label <text>]".to_string(),
    );
  }

  let command_id = arguments[1].clone();
  let mut target = ExecutionTarget::default();
  let mut inputs = BTreeMap::new();
  let mut index = 2;

  while index < arguments.len() {
    let argument = &arguments[index];
    if !argument.starts_with("--") {
      return Err(format!("unexpected positional argument {argument}"));
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

  Ok(CliCommand::Invoke(InvokeRequest {
    command_id,
    target,
    inputs,
  }))
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
    "bundle" => parse_skill_bundle(arguments),
    "run" => parse_skill_run(arguments),
    other => Err(format!(
      "unknown skill subcommand {other}; use `auv-cli skill list` to inspect the current catalog"
    )),
  }
}

fn parse_skill_bundle(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 3 {
    return Err("usage: auv-cli skill bundle <list|show|coverage|verify|export> ...".to_string());
  }

  match arguments[2].as_str() {
    "list" => {
      if arguments.len() != 3 {
        return Err("usage: auv-cli skill bundle list".to_string());
      }
      Ok(CliCommand::SkillBundleList)
    }
    "show" => {
      if arguments.len() != 4 {
        return Err("usage: auv-cli skill bundle show <bundle-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillBundleShow {
        query: arguments[3].clone(),
      })
    }
    "coverage" => {
      if arguments.len() != 4 {
        return Err("usage: auv-cli skill bundle coverage <bundle-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillBundleCoverage {
        query: arguments[3].clone(),
      })
    }
    "verify" => {
      if arguments.len() != 4 {
        return Err("usage: auv-cli skill bundle verify <bundle-id-or-path>".to_string());
      }
      Ok(CliCommand::SkillBundleVerify {
        query: arguments[3].clone(),
      })
    }
    "export" => {
      if arguments.len() != 5 {
        return Err(
          "usage: auv-cli skill bundle export <bundle-id-or-path> <output-dir>".to_string(),
        );
      }
      Ok(CliCommand::SkillBundleExport {
        query: arguments[3].clone(),
        output_dir: arguments[4].clone(),
      })
    }
    "package" => {
      if arguments.len() != 5 || arguments[3].as_str() != "verify" {
        return Err("usage: auv-cli skill bundle package verify <package-dir>".to_string());
      }
      Ok(CliCommand::SkillBundlePackageVerify {
        package_dir: arguments[4].clone(),
      })
    }
    other => Err(format!(
      "unknown skill bundle subcommand {other}; use `auv-cli skill bundle list`"
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
      "usage: auv-cli skill run <skill-id-or-path> [--dry-run] [--max-disturbance <class>] [--set key=value]".to_string(),
    );
  }

  let query = arguments[2].clone();
  let mut dry_run = false;
  let mut max_disturbance = None;
  let mut overrides = BTreeMap::new();
  let mut index = 3;

  while index < arguments.len() {
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
  })
}

fn parse_skill_cases_run(arguments: &[String]) -> AuvResult<CliCommand> {
  if arguments.len() < 4 {
    return Err(
      "usage: auv-cli skill cases run <matrix-id-or-path> [--case <case-id>] [--all-statuses] [--dry-run] [--max-disturbance <class>]".to_string(),
    );
  }

  let query = arguments[3].clone();
  let mut dry_run = false;
  let mut max_disturbance = None;
  let mut only_case_ids = Vec::new();
  let mut include_nonvalidated = false;
  let mut index = 4;

  while index < arguments.len() {
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
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_skill_bundle_coverage_command() {
    let command = parse_cli(&[
      "skill".to_string(),
      "bundle".to_string(),
      "coverage".to_string(),
      "native.app.skill-tree.v0".to_string(),
    ])
    .expect("bundle coverage command should parse");

    match command {
      CliCommand::SkillBundleCoverage { query } => {
        assert_eq!(query, "native.app.skill-tree.v0");
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }

  #[test]
  fn parse_app_probe_command() {
    let command = parse_cli(&[
      "app".to_string(),
      "probe".to_string(),
      "com.tencent.QQMusicMac".to_string(),
      "--output-dir".to_string(),
      "/tmp/probe".to_string(),
    ])
    .expect("app probe command should parse");

    match command {
      CliCommand::AppProbe {
        bundle_id,
        output_dir,
      } => {
        assert_eq!(bundle_id, "com.tencent.QQMusicMac");
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
      CliCommand::InspectServe { host, port } => {
        assert_eq!(host, "0.0.0.0");
        assert_eq!(port, 0);
      }
      other => panic!("unexpected command: {other:?}"),
    }
  }
}
