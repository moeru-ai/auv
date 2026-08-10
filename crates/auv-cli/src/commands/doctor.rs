use clap::Args;

/// Inspect the permissions needed by local desktop automation.
#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # Print a human-readable permission report\n  auv doctor\n\n  # Print a machine-readable permission report\n  auv doctor --json"
)]
pub struct DoctorArgs {
  /// Render the permission report as JSON.
  #[arg(long)]
  pub json: bool,
}

#[derive(serde::Serialize)]
struct PermissionCheckReport {
  platform: &'static str,
  process_id: u32,
  executable: Option<String>,
  accessibility: &'static str,
  screen_recording_preflight: &'static str,
  screen_capture_kit: &'static str,
  screen_capture_kit_error: Option<String>,
  all_ok: bool,
  warnings: Vec<String>,
  recommendation: String,
}

pub async fn run(args: DoctorArgs) -> Result<i32, String> {
  let report = collect_permission_check()?;
  if args.json {
    println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode permission report: {error}"))?);
  } else {
    print_report(&report);
  }
  Ok(0)
}

#[cfg(target_os = "macos")]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  let native = auv_driver_macos::native::permission::probe_native_permissions()?;
  let all_ok = native.accessibility == "granted" && native.screen_capture_kit == "granted";
  let mut warnings = Vec::new();
  if native.screen_recording == "missing" && native.screen_capture_kit == "granted" {
    warnings.push("CGPreflightScreenCaptureAccess reports missing, but the ScreenCaptureKit probe works; this can happen when the launch host owns TCC attribution.".to_string());
  }
  if matches!(native.screen_capture_kit, "timed_out" | "failed") {
    warnings.push(
      "The ScreenCaptureKit probe did not establish a permission result; the current Screen Recording grant may be unchanged.".to_string(),
    );
  }
  let recommendation = recommendation(native.accessibility, native.screen_capture_kit);
  Ok(PermissionCheckReport {
    platform: "macos",
    process_id: std::process::id(),
    executable: std::env::current_exe().ok().map(|path| path.display().to_string()),
    accessibility: native.accessibility,
    screen_recording_preflight: native.screen_recording,
    screen_capture_kit: native.screen_capture_kit,
    screen_capture_kit_error: native.screen_capture_kit_error,
    all_ok,
    warnings,
    recommendation,
  })
}

#[cfg(not(target_os = "macos"))]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  Err("permission check is currently implemented only for macOS".to_string())
}

fn recommendation(accessibility: &str, screen_capture_kit: &str) -> String {
  match (accessibility, screen_capture_kit) {
    ("granted", "granted") => "AUV has the macOS permissions needed for capture and AX-backed automation.".to_string(),
    ("missing", "missing") => {
      "Grant Accessibility and Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string()
    }
    ("missing", _) => "Grant Accessibility to the terminal or app that launches auv, then rerun this check.".to_string(),
    (_, "missing") => "Grant Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string(),
    (_, "timed_out") => {
      "Retry the ScreenCaptureKit probe; a timeout does not establish that Screen Recording permission is missing.".to_string()
    }
    (_, "failed") => {
      "Review the ScreenCaptureKit probe detail and retry; a probe failure does not establish that permission is missing.".to_string()
    }
    _ => "Review the permission statuses above before running desktop automation.".to_string(),
  }
}

fn print_report(report: &PermissionCheckReport) {
  println!("AUV permission check");
  println!("platform: {}", report.platform);
  println!("process: {}", report.process_id);
  if let Some(executable) = &report.executable {
    println!("executable: {executable}");
  }
  println!("accessibility: {}", status_line(report.accessibility));
  println!("screen recording preflight: {}", status_line(report.screen_recording_preflight));
  println!("screen capture kit probe: {}", status_line(report.screen_capture_kit));
  if let Some(error) = &report.screen_capture_kit_error {
    println!("screen capture kit detail: {error}");
  }
  for warning in &report.warnings {
    println!("warning: {warning}");
  }
  println!("all ok: {}", report.all_ok);
  println!("recommendation: {}", report.recommendation);
}

fn status_line(status: &str) -> String {
  match status {
    "granted" => "[ok] granted".to_string(),
    "missing" => "[missing] missing".to_string(),
    other => format!("[unknown] {other}"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn timed_out_probe_does_not_recommend_granting_permission() {
    assert_eq!(status_line("timed_out"), "[unknown] timed_out");
    assert!(recommendation("granted", "timed_out").contains("does not establish"));
  }

  #[test]
  fn failed_probe_does_not_recommend_granting_permission() {
    assert_eq!(status_line("failed"), "[unknown] failed");
    assert!(recommendation("granted", "failed").contains("does not establish"));
  }
}
