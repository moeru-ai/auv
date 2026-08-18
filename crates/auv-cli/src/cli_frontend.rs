// Shared frontend for the root `auv` binary.

use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use crate::cli::{CliCommand, TracingOptions, parse_cli_os, version_text};

pub async fn run_root() -> Result<i32, String> {
  let command = parse_cli_os(env::args_os().skip(1))?;
  dispatch(command).await
}

pub fn exit_status(result: Result<i32, String>) -> i32 {
  match result {
    Ok(exit_code) => exit_code,
    Err(error) => {
      eprintln!("error: {error}");
      1
    }
  }
}

pub(crate) async fn dispatch(command: CliCommand) -> Result<i32, String> {
  if matches!(&command, CliCommand::Version) {
    print!("{}", version_text());
    return Ok(0);
  }

  let project_root = env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let CliCommand::XtaskGenerateSwiftBridge = &command {
    let outputs = crate::xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    for output in outputs {
      println!("output: {output}");
    }
    return Ok(0);
  }

  if let CliCommand::McpServe = &command {
    crate::mcp::serve_stdio(project_root.clone()).await?;
    return Ok(0);
  }

  if let CliCommand::PermissionCheck {
    json,
    request_permissions,
  } = &command
  {
    run_permission_check(*json, *request_permissions)?;
    return Ok(0);
  }

  if let CliCommand::SessionServe {
    host,
    port,
    store_root,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let config = crate::session_service::transport::SessionApiServeConfig {
      host: host.clone(),
      port: *port,
      store_root,
    };
    crate::session_service::transport::serve(config).await?;
    return Ok(0);
  }

  let mut exit_code = 0;
  match command {
    CliCommand::Help(help) => {
      print!("{help}");
    }
    CliCommand::Version => unreachable!("version is handled before runtime setup"),
    CliCommand::PermissionCheck { .. } => {
      unreachable!("permission check is handled before runtime setup")
    }
    CliCommand::XtaskGenerateSwiftBridge => unreachable!("xtask is handled before runtime setup"),
    CliCommand::InvokeHelp { command_id } => {
      let registry = auv_cli_invoke::default_registry();
      if let Some(command_id) = command_id {
        let command = registry
          .resolve(&command_id)
          .ok_or_else(|| format!("unknown command {command_id}; use `auv invoke --help` to inspect available entries"))?;
        print!("{}", auv_cli_invoke::render_command_help(command));
      } else {
        print!("{}", auv_cli_invoke::render_help_index(&registry));
      }
    }
    CliCommand::Invoke {
      request,
      typed_args,
      tracing,
      output,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
      let registry = auv_cli_invoke::default_registry();
      let command =
        registry.resolve(&request.command_id).cloned().ok_or_else(|| format!("unknown invoke command: {}", request.command_id))?;
      let input = auv_cli_invoke::InvokeCommandInput {
        command_id: request.command_id,
        target_application_id: request.target.application_id,
        inputs: request.inputs,
        typed_args: Some(typed_args),
        dry_run: request.dry_run,
        cancellation: auv_cli_invoke::InvokeCancellation::new(),
      };
      let invoked_command = command.clone();
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        invoked_command.invoke(input)
      });
      let direct_result = root.instrument(future).await;
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
        eprintln!("warning: invoke recording failure for run {run_id}: {failure}");
      }
      let artifact_paths = direct_result
        .as_ref()
        .ok()
        .into_iter()
        .flat_map(auv_cli_invoke::InvokeCommandOutput::artifacts)
        .map(|metadata| (metadata.uri().clone(), authority.store.artifact_path(metadata)))
        .collect::<Vec<_>>();
      let result = auv_cli_invoke::InvokeResult::from_command_result(run_id, &command, direct_result).with_artifact_paths(artifact_paths);
      let outcome = auv_cli_invoke::render_invoke_result(&result, output)?;
      exit_code = outcome.exit_code;
    }
    CliCommand::McpServe => {
      unreachable!("mcp serve is handled before runtime setup")
    }
    CliCommand::SessionServe { .. } => {
      unreachable!("session serve is handled before runtime setup")
    }
    CliCommand::PluginList => {
      exit_code = crate::plugin::list()?;
    }
    CliCommand::External {
      command_name,
      arguments,
    } => {
      exit_code = crate::plugin::execute(&command_name, &arguments)?;
    }
  }

  Ok(exit_code)
}

#[derive(serde::Serialize)]
struct PermissionCheckReport {
  platform: &'static str,
  process_id: u32,
  executable: Option<String>,
  accessibility: &'static str,
  screen_recording_preflight: &'static str,
  screen_capture_kit: &'static str,
  all_ok: bool,
  warnings: Vec<String>,
  recommendation: String,
}

fn run_permission_check(json: bool, request_permissions: bool) -> Result<(), String> {
  if request_permissions {
    #[cfg(target_os = "macos")]
    {
      let native = auv_driver_macos::native::permission::probe_native_permissions()?;
      let has_missing_permission =
        native.accessibility == "missing" || native.screen_recording == "missing" || native.screen_capture_kit == "missing";
      if has_missing_permission {
        auv_driver_macos::native::permission::request_native_permissions()?;
      }
    }
  }

  let report = collect_permission_check()?;

  if json {
    println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode permission report: {error}"))?);
  } else {
    print_permission_check_report(&report);
  }

  Ok(())
}

#[cfg(target_os = "macos")]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  let native = auv_driver_macos::native::permission::probe_native_permissions()?;
  let all_ok = native.accessibility == "granted" && native.screen_capture_kit == "granted";
  let mut warnings = Vec::new();

  if native.screen_recording == "missing" && native.screen_capture_kit == "granted" {
    warnings.push(
      "CGPreflightScreenCaptureAccess reports missing, but the ScreenCaptureKit probe works; this can happen when the launch host owns TCC attribution."
        .to_string(),
    );
  }

  Ok(PermissionCheckReport {
    platform: "macos",
    process_id: process::id(),
    executable: env::current_exe().ok().map(|path| path.display().to_string()),
    accessibility: native.accessibility,
    screen_recording_preflight: native.screen_recording,
    screen_capture_kit: native.screen_capture_kit,
    all_ok,
    warnings,
    recommendation: permission_recommendation(native.accessibility, native.screen_recording, native.screen_capture_kit),
  })
}

#[cfg(not(target_os = "macos"))]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  Err("permission check is currently implemented only for macOS".to_string())
}

fn permission_recommendation(accessibility: &str, screen_recording: &str, screen_capture_kit: &str) -> String {
  const REQUEST_COMMAND: &str = "`auv doctor --request-permissions`";

  if accessibility == "granted" && screen_recording == "granted" && screen_capture_kit == "granted" {
    return "AUV has the macOS permissions needed for capture and AX-backed automation.".to_string();
  }

  if accessibility != "missing" && screen_recording == "missing" && screen_capture_kit == "granted" {
    return "Screen Recording preflight is missing for this process, but the ScreenCaptureKit probe works; this can reflect TCC attribution to the app that launched auv. Rerun the check from that same app or restart it if capture still fails.".to_string();
  }

  if accessibility != "missing" && screen_recording == "granted" && screen_capture_kit == "missing" {
    return format!(
      "ScreenCaptureKit is unavailable although Screen Recording preflight is granted. Run {REQUEST_COMMAND}; if it remains missing, restart the app that launches auv, then rerun `auv doctor --json`."
    );
  }

  let mut missing = Vec::new();
  if accessibility == "missing" {
    missing.push("Accessibility");
  }
  if screen_recording == "missing" {
    missing.push("Screen Recording preflight");
  }
  if screen_capture_kit == "missing" {
    missing.push("ScreenCaptureKit");
  }

  if missing.is_empty() {
    return "Review the permission statuses above before running desktop automation.".to_string();
  }

  format!(
    "Missing: {}. Run {REQUEST_COMMAND}, approve the macOS prompts, then rerun `auv doctor --json`. If ScreenCaptureKit remains missing after Screen Recording is granted, restart the app that launches auv.",
    missing.join(", ")
  )
}

fn print_permission_check_report(report: &PermissionCheckReport) {
  println!("AUV permission check");
  println!("platform: {}", report.platform);
  println!("process: {}", report.process_id);
  if let Some(executable) = &report.executable {
    println!("executable: {executable}");
  }
  println!("accessibility: {}", permission_status_line(report.accessibility));
  println!("screen recording preflight: {}", permission_status_line(report.screen_recording_preflight));
  println!("screen capture kit probe: {}", permission_status_line(report.screen_capture_kit));
  for warning in &report.warnings {
    println!("warning: {warning}");
  }
  println!("all ok: {}", report.all_ok);
  println!("recommendation: {}", report.recommendation);
}

fn permission_status_line(status: &str) -> String {
  match status {
    "granted" => "[ok] granted".to_string(),
    "missing" => "[missing] missing".to_string(),
    other => format!("[unknown] {other}"),
  }
}

#[cfg(test)]
mod tests {
  use super::permission_recommendation;

  #[test]
  fn recommendation_names_screen_capture_kit_when_preflight_is_granted() {
    let recommendation = permission_recommendation("granted", "granted", "missing");

    assert!(recommendation.contains("ScreenCaptureKit is unavailable"));
    assert!(recommendation.contains("auv doctor --request-permissions"));
    assert!(!recommendation.starts_with("Grant Screen Recording"));
  }

  #[test]
  fn recommendation_names_both_missing_permissions() {
    let recommendation = permission_recommendation("missing", "missing", "missing");

    assert!(recommendation.contains("Missing: Accessibility, Screen Recording preflight, ScreenCaptureKit"));
    assert!(recommendation.contains("auv doctor --request-permissions"));
  }

  #[test]
  fn recommendation_reports_accessibility_and_capture_kit_together() {
    let recommendation = permission_recommendation("missing", "granted", "missing");

    assert!(recommendation.contains("Missing: Accessibility, ScreenCaptureKit"));
  }

  #[test]
  fn recommendation_explains_tcc_attribution_mismatch() {
    let recommendation = permission_recommendation("granted", "missing", "granted");

    assert!(recommendation.contains("TCC attribution"));
    assert!(recommendation.contains("app that launched auv"));
  }
}

pub(crate) fn resolve_store_root(project_root: &Path, explicit: Option<&PathBuf>) -> PathBuf {
  explicit.cloned().unwrap_or_else(|| project_root.join(".auv").join("store"))
}

#[derive(Clone)]
struct CliTracing {
  dispatch: auv_tracing::Dispatch,
  store: Arc<auv_tracing::FileTracingStore>,
}

fn build_cli_tracing(project_root: &Path, options: &TracingOptions) -> Result<CliTracing, String> {
  let store_root = resolve_store_root(project_root, options.store_root.as_ref());
  let store = Arc::new(
    auv_tracing::FileTracingStore::open(&store_root)
      .map_err(|error| format!("failed to open tracing store {}: {error}", store_root.display()))?,
  );
  let dispatch =
    auv_tracing::configure().tracing_store(store.clone()).build().map_err(|error| format!("failed to configure invoke tracing: {error}"))?;
  Ok(CliTracing { dispatch, store })
}

#[derive(serde::Serialize)]
struct InvokeFrontendLifecycle {
  frontend: &'static str,
}

impl auv_tracing::EventPayload for InvokeFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

async fn flush_cli_recording(dispatch: &auv_tracing::Dispatch) -> Option<String> {
  dispatch.flush().await.err().map(|error| error.to_string())
}
