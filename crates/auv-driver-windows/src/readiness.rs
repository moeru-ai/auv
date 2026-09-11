//! Target-aware readiness assessment: combines the process-level automation
//! probe ([`WindowsPermissionProbe`]) with window presence, foreground, and
//! frame-drift checks.
//!
//! Mirrors the macOS driver's `assess_readiness`, substituting Windows'
//! process/session signals for macOS's TCC permissions. Windows has no
//! app-bundle/app-activation concept (see `window.rs`'s NOTICE on
//! `app_bundle_id`), so target resolution and the frontmost check operate at
//! the window level instead of the app level.

use auv_driver_common::permission::PermissionStatus;
use auv_driver_common::{ReadinessCheck, ReadinessProbeInput, ReadinessReport, Rect, Window};

use crate::permission::WindowsPermissionProbe;

pub fn assess_readiness(
  permissions: &WindowsPermissionProbe,
  windows: &[Window],
  frontmost: Option<&Window>,
  input: &ReadinessProbeInput,
) -> ReadinessReport {
  let target = resolve_target_window(windows, input);
  let mut checks = vec![
    interactive_session_check(permissions.interactive_session),
    informational_status_check(
      "process_elevated",
      permissions.elevated,
      "process runs with an elevated (administrator) token",
      "process does not run elevated",
    ),
    informational_status_check(
      "ui_access",
      permissions.ui_access,
      "process holds the UIAccess privilege",
      "process does not hold the UIAccess privilege",
    ),
  ];
  checks.push(match target {
    Some(window) => ReadinessCheck::pass("target_window_present", format!("target window {} is present", window.reference.id)),
    None => ReadinessCheck::fail("target_window_present", "target window is missing or no longer matches the execution plan"),
  });
  if input.require_frontmost {
    checks.push(frontmost_check(frontmost, target));
  }
  if let (Some(expected), Some(window)) = (input.expected_window_frame, target) {
    let drift = max_frame_drift(expected, window.frame);
    if drift <= input.max_window_frame_drift_px {
      checks.push(ReadinessCheck::pass("window_bounds_stable", format!("window frame drift {drift:.2}px within tolerance")));
    } else {
      checks.push(ReadinessCheck::fail(
        "window_bounds_stable",
        format!("window frame drift {drift:.2}px exceeds tolerance {:.2}px", input.max_window_frame_drift_px),
      ));
    }
  } else {
    checks.push(ReadinessCheck::fail("window_bounds_stable", "expected window frame was not supplied; cannot prove bounds stability"));
  }
  checks.push(match target {
    Some(window) if point_inside_window(input.target_window_x, input.target_window_y, window) => {
      ReadinessCheck::pass("input_injection_target", "target point is inside target window")
    }
    Some(_) => ReadinessCheck::fail("input_injection_target", "target point is outside the current target window bounds"),
    None => ReadinessCheck::fail("input_injection_target", "cannot assess input injection without a target window"),
  });

  ReadinessReport::from_checks(checks, target.map(|window| window.reference.id.clone()), target.map(|window| window.frame), None)
}

pub fn resolve_target_window<'a>(windows: &'a [Window], input: &ReadinessProbeInput) -> Option<&'a Window> {
  windows.iter().find(|window| {
    if let Some(expected) = input.window_number {
      window.reference.id == expected.to_string()
        && input
          .window_title
          .as_ref()
          .is_none_or(|expected_title| window.title.as_deref().is_some_and(|title| title.contains(expected_title)))
    } else {
      // NOTICE: app_bundle_id is a macOS concept with no Windows equivalent
      // (see `window.rs`), so matching without a window number relies on
      // window_title alone.
      input.window_title.as_ref().is_none_or(|expected| window.title.as_deref().is_some_and(|title| title.contains(expected)))
    }
  })
}

fn interactive_session_check(status: PermissionStatus) -> ReadinessCheck {
  match status {
    PermissionStatus::Granted => ReadinessCheck::pass("interactive_session", "process runs in an interactive session"),
    PermissionStatus::Missing => ReadinessCheck::fail(
      "interactive_session",
      "process runs in a non-interactive session (Session 0); input and capture cannot reach the desktop",
    ),
    PermissionStatus::Unknown => ReadinessCheck::unknown("interactive_session", "interactive session state could not be determined"),
  }
}

// Elevation and UIAccess are diagnostic signals, not blockers on their own: a
// non-elevated, non-UIAccess process is the normal automation posture, so
// only an undetermined query result is surfaced as `Unknown` rather than
// failing readiness outright.
fn informational_status_check(name: &str, status: PermissionStatus, granted_reason: &str, missing_reason: &str) -> ReadinessCheck {
  match status {
    PermissionStatus::Granted => ReadinessCheck::pass(name, granted_reason),
    PermissionStatus::Missing => ReadinessCheck::pass(name, missing_reason),
    PermissionStatus::Unknown => ReadinessCheck::unknown(name, format!("{name} state could not be determined")),
  }
}

fn frontmost_check(frontmost: Option<&Window>, target: Option<&Window>) -> ReadinessCheck {
  let Some(frontmost) = frontmost else {
    return ReadinessCheck::fail("target_window_frontmost", "frontmost window could not be resolved");
  };
  match target {
    Some(target) if frontmost.reference.id == target.reference.id => {
      ReadinessCheck::pass("target_window_frontmost", "target window is frontmost")
    }
    Some(target) => ReadinessCheck::fail(
      "target_window_frontmost",
      format!("frontmost window {} does not match target window {}", frontmost.reference.id, target.reference.id),
    ),
    None => ReadinessCheck::fail("target_window_frontmost", "cannot assess frontmost state without a target window"),
  }
}

fn max_frame_drift(expected: Rect, actual: Rect) -> f64 {
  [
    (expected.origin.x - actual.origin.x).abs(),
    (expected.origin.y - actual.origin.y).abs(),
    (expected.size.width - actual.size.width).abs(),
    (expected.size.height - actual.size.height).abs(),
  ]
  .into_iter()
  .fold(0.0, f64::max)
}

fn point_inside_window(x: f64, y: f64, window: &Window) -> bool {
  x >= 0.0 && y >= 0.0 && x <= window.frame.size.width && y <= window.frame.size.height
}

#[cfg(test)]
#[path = "readiness_test.rs"]
mod tests;
