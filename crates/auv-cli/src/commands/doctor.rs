use clap::Args;

/// Inspect local desktop automation diagnostics.
#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # Print a human-readable diagnostics report\n  auv doctor\n\n  # Print a machine-readable diagnostics report\n  auv doctor --json"
)]
pub struct DoctorArgs {
  /// Render the doctor report as JSON.
  #[arg(long)]
  pub json: bool,
  /// Install AUV's user desktop identity (Linux only; grants no permissions).
  #[arg(long)]
  pub portal_setup: bool,
  /// Request and persist Portal capture/input authorization (Linux only).
  #[arg(long)]
  pub portal_authorize: bool,
  /// Explicitly allow AUV through KDE's permanent application rule.
  #[arg(long, conflicts_with = "portal_kde_revoke")]
  pub portal_kde_allow: bool,
  /// Remove AUV's KDE application rule; existing sessions/grants remain separate.
  #[arg(long)]
  pub portal_kde_revoke: bool,
  /// Portal state directory; for a daemon use <store-root>/runner-state/auv.core.local/portal.
  #[arg(long)]
  pub portal_state_root: Option<std::path::PathBuf>,
}

#[cfg(target_os = "macos")]
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

/// Read-only diagnostics for the process running `auv doctor` on Windows: the
/// three signals [`auv_driver_windows::permission`] already probes (elevation,
/// UIAccess, nonzero session). They describe this process only, never a
/// selected Runner or remote Device. `nonzero_session` is the raw fact that
/// the process's session ID is not 0; it is not evidence that an interactive
/// desktop is reachable, and no single signal proves or refutes that
/// automation will work.
#[cfg(any(target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct WindowsDiagnosticReport {
  platform: &'static str,
  process_id: u32,
  executable: Option<String>,
  elevated: auv_driver::PermissionStatus,
  ui_access: auv_driver::PermissionStatus,
  nonzero_session: auv_driver::PermissionStatus,
  notes: Vec<&'static str>,
}

#[cfg(any(target_os = "windows", test))]
const WINDOWS_RUNNER_SCOPE_NOTE: &str =
  "these signals diagnose the process running auv doctor only; they say nothing about a selected Runner or a remote Device";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_POSTURE_NOTE: &str = "a non-elevated process without UIAccess is the normal automation posture; UIPI only blocks input into windows of higher-integrity processes";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_SESSION_NOTE: &str = "nonzero_session only reports that this process's session ID is not 0; it does not prove that an interactive desktop is present, unlocked, connected, or accepting input";
#[cfg(any(target_os = "windows", test))]
const WINDOWS_SESSION_ZERO_NOTE: &str =
  "this process runs in Session 0 (non-interactive); input and capture cannot reach the user desktop from it";

pub async fn run(args: DoctorArgs) -> Result<i32, String> {
  #[cfg(target_os = "linux")]
  {
    return run_linux(args);
  }
  #[cfg(not(target_os = "linux"))]
  if args.portal_setup || args.portal_authorize || args.portal_kde_allow || args.portal_kde_revoke || args.portal_state_root.is_some() {
    return Err("Portal setup is available only on Linux".into());
  }
  #[cfg(target_os = "macos")]
  {
    let report = collect_permission_check()?;
    if args.json {
      println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode permission report: {error}"))?);
    } else {
      print_report(&report);
    }
    Ok(0)
  }
  #[cfg(target_os = "windows")]
  {
    let report = collect_windows_diagnostics();
    if args.json {
      println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode doctor report: {error}"))?);
    } else {
      let stdout = std::io::stdout();
      write_windows_report(&report, &mut stdout.lock()).map_err(|error| format!("failed to write doctor report: {error}"))?;
    }
    Ok(0)
  }
  #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
  {
    Err("permission check is currently implemented only for macOS".to_string())
  }
}

#[cfg(target_os = "linux")]
fn run_linux(args: DoctorArgs) -> Result<i32, String> {
  if args.portal_setup {
    auv::local::setup_portal().map_err(|error| error.to_string())?;
  }
  if args.portal_kde_allow || args.portal_kde_revoke {
    auv::local::set_kde_portal_authorization(args.portal_kde_allow).map_err(|error| error.to_string())?;
  }
  if args.portal_authorize {
    auv::local::authorize_portal(args.portal_state_root.clone()).map_err(|error| error.to_string())?;
  }
  let status = auv::local::portal_status(args.portal_state_root).map_err(|error| error.to_string())?;
  // Keep the full distinction between interface readiness, identity and token
  // presence in both human and machine output; none proves restored consent.
  let json = serde_json::to_string_pretty(&status).map_err(|error| error.to_string())?;
  if !args.json {
    println!("AUV Linux Portal readiness (token presence does not prove authorization)");
  }
  println!("{json}");
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

/// Probes the running process through the existing Windows driver readiness
/// signals. Read-only: it opens this process's own token and maps its session
/// ID; it never elevates, requests permissions, or touches the desktop.
#[cfg(target_os = "windows")]
fn collect_windows_diagnostics() -> WindowsDiagnosticReport {
  let probe = auv_driver_windows::permission::probe();
  WindowsDiagnosticReport {
    platform: "windows",
    process_id: std::process::id(),
    executable: std::env::current_exe().ok().map(|path| path.display().to_string()),
    elevated: probe.elevated,
    ui_access: probe.ui_access,
    nonzero_session: probe.interactive_session,
    notes: windows_notes(probe.interactive_session),
  }
}

/// The interpretation notes for one Windows report: the scope, posture, and
/// session caveats always apply, and a Session 0 answer adds its own note. The
/// collector and the report-format tests share this so human output, JSON, and
/// the tests describe the same report.
#[cfg(any(target_os = "windows", test))]
fn windows_notes(nonzero_session: auv_driver::PermissionStatus) -> Vec<&'static str> {
  let mut notes = vec![
    WINDOWS_RUNNER_SCOPE_NOTE,
    WINDOWS_POSTURE_NOTE,
    WINDOWS_SESSION_NOTE,
  ];
  if nonzero_session == auv_driver::PermissionStatus::Missing {
    notes.push(WINDOWS_SESSION_ZERO_NOTE);
  }
  notes
}

#[cfg(any(target_os = "windows", test))]
fn write_windows_report(report: &WindowsDiagnosticReport, out: &mut impl std::io::Write) -> std::io::Result<()> {
  writeln!(out, "AUV Windows process diagnostics")?;
  writeln!(out, "platform: {}", report.platform)?;
  writeln!(out, "process: {}", report.process_id)?;
  if let Some(executable) = &report.executable {
    writeln!(out, "executable: {executable}")?;
  }
  // Each status is printed as the plain state word. Missing is a plain fact
  // on Windows, not a failure: elevation and UIAccess are diagnostic signals,
  // so the line names the state without an ok/fail verdict and the notes
  // carry the interpretation.
  writeln!(out, "elevated: {}", report.elevated.as_str())?;
  writeln!(out, "ui access: {}", report.ui_access.as_str())?;
  writeln!(out, "nonzero session: {}", report.nonzero_session.as_str())?;
  for note in &report.notes {
    writeln!(out, "note: {note}")?;
  }
  Ok(())
}

#[cfg(any(target_os = "macos", test))]
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

#[cfg(target_os = "macos")]
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

#[cfg(any(target_os = "macos", test))]
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

  // The Windows diagnostics must stay testable without Windows token or
  // session FFI, so the report format tests build injectable samples and
  // assert both output shapes. Production collection reuses the existing
  // `WindowsPermissionProbe`; only the report format is pinned here.
  #[cfg(any(target_os = "windows", test))]
  mod windows {
    use super::*;

    fn sample(
      elevated: auv_driver::PermissionStatus,
      ui_access: auv_driver::PermissionStatus,
      nonzero_session: auv_driver::PermissionStatus,
    ) -> WindowsDiagnosticReport {
      WindowsDiagnosticReport {
        platform: "windows",
        process_id: 4242,
        executable: Some("C:\\tools\\auv.exe".to_string()),
        elevated,
        ui_access,
        nonzero_session,
        notes: windows_notes(nonzero_session),
      }
    }

    fn human(report: &WindowsDiagnosticReport) -> String {
      let mut buffer = Vec::new();
      write_windows_report(report, &mut buffer).expect("write report");
      String::from_utf8(buffer).expect("report is utf-8")
    }

    fn json(report: &WindowsDiagnosticReport) -> serde_json::Value {
      serde_json::to_value(report).expect("serialize report")
    }

    // A normal, non-elevated process without UIAccess is the normal automation
    // posture: the human output must state the facts without a failure verdict
    // or an all-ok claim.
    #[test]
    fn normal_user_posture_states_facts_without_failure_or_all_ok_claims() {
      let report =
        sample(auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Granted);
      let text = human(&report);
      assert!(text.contains("elevated: missing"), "{text}");
      assert!(text.contains("ui access: missing"), "{text}");
      assert!(text.contains("nonzero session: granted"), "{text}");
      assert!(!text.contains("all ok"), "{text}");
      assert!(!text.to_lowercase().contains("cannot use auv"), "{text}");
      assert!(text.contains(WINDOWS_POSTURE_NOTE), "{text}");
      assert!(!text.contains(WINDOWS_SESSION_ZERO_NOTE), "{text}");

      let value = json(&report);
      assert_eq!(value["platform"], "windows");
      assert_eq!(value["process_id"], 4242);
      assert_eq!(value["elevated"], "missing");
      assert_eq!(value["ui_access"], "missing");
      assert_eq!(value["nonzero_session"], "granted");
      assert!(value.get("all_ok").is_none(), "{value}");
    }

    #[test]
    fn elevated_process_reports_granted_without_ui_access() {
      let report =
        sample(auv_driver::PermissionStatus::Granted, auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Granted);
      let text = human(&report);
      assert!(text.contains("elevated: granted"), "{text}");
      assert!(text.contains("ui access: missing"), "{text}");
      assert_eq!(json(&report)["elevated"], "granted");
    }

    #[test]
    fn session_zero_reports_missing_nonzero_session_with_the_desktop_note() {
      let report =
        sample(auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Missing);
      let text = human(&report);
      assert!(text.contains("nonzero session: missing"), "{text}");
      assert!(text.contains(WINDOWS_SESSION_ZERO_NOTE), "{text}");

      let value = json(&report);
      assert_eq!(value["nonzero_session"], "missing");
      let notes = value["notes"].as_array().expect("notes array");
      assert!(notes.iter().any(|note| note == WINDOWS_SESSION_ZERO_NOTE), "{value}");
    }

    #[test]
    fn unknown_stays_unknown_in_both_outputs() {
      let report =
        sample(auv_driver::PermissionStatus::Unknown, auv_driver::PermissionStatus::Unknown, auv_driver::PermissionStatus::Unknown);
      let text = human(&report);
      assert!(text.contains("elevated: unknown"), "{text}");
      assert!(text.contains("ui access: unknown"), "{text}");
      assert!(text.contains("nonzero session: unknown"), "{text}");
      assert!(!text.contains("elevated: missing"), "{text}");
      assert!(!text.contains("elevated: granted"), "{text}");
      assert!(!text.contains(WINDOWS_SESSION_ZERO_NOTE), "an unknown session must not claim Session 0: {text}");

      let value = json(&report);
      assert_eq!(value["elevated"], "unknown");
      assert_eq!(value["ui_access"], "unknown");
      assert_eq!(value["nonzero_session"], "unknown");
    }

    #[test]
    fn notes_carry_the_scope_and_session_caveats() {
      let value =
        json(&sample(auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Granted));
      let notes = value["notes"].as_array().expect("notes array");
      assert!(notes.iter().any(|note| note == WINDOWS_RUNNER_SCOPE_NOTE), "{value}");
      assert!(notes.iter().any(|note| note == WINDOWS_SESSION_NOTE), "{value}");
      assert!(notes.iter().any(|note| note == WINDOWS_POSTURE_NOTE), "{value}");
      assert!(!notes.iter().any(|note| note == WINDOWS_SESSION_ZERO_NOTE), "{value}");

      let text =
        human(&sample(auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Missing, auv_driver::PermissionStatus::Granted));
      assert!(text.contains(WINDOWS_RUNNER_SCOPE_NOTE), "{text}");
      assert!(text.contains(WINDOWS_SESSION_NOTE), "{text}");
    }

    // The human and JSON outputs are two renderings of one report: every
    // signal status must read the same in both. All three signals vary over
    // the full status vocabulary.
    #[test]
    fn human_and_json_outputs_agree_on_every_status() {
      for elevated in [
        auv_driver::PermissionStatus::Granted,
        auv_driver::PermissionStatus::Missing,
        auv_driver::PermissionStatus::Unknown,
      ] {
        for ui_access in [
          auv_driver::PermissionStatus::Granted,
          auv_driver::PermissionStatus::Missing,
          auv_driver::PermissionStatus::Unknown,
        ] {
          for nonzero_session in [
            auv_driver::PermissionStatus::Granted,
            auv_driver::PermissionStatus::Missing,
            auv_driver::PermissionStatus::Unknown,
          ] {
            let report = sample(elevated, ui_access, nonzero_session);
            let text = human(&report);
            let value = json(&report);
            assert!(text.contains(&format!("elevated: {}", value["elevated"].as_str().unwrap())), "{text}");
            assert!(text.contains(&format!("ui access: {}", value["ui_access"].as_str().unwrap())), "{text}");
            assert!(text.contains(&format!("nonzero session: {}", value["nonzero_session"].as_str().unwrap())), "{text}");
          }
        }
      }
    }
  }
}
