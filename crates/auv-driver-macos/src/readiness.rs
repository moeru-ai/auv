use auv_driver::{PermissionProbe, PermissionStatus, ReadinessCheck, ReadinessProbeInput, ReadinessReport, Rect, Window};

pub fn assess_readiness(
  permissions: &PermissionProbe,
  windows: &[Window],
  frontmost: Option<&Window>,
  input: &ReadinessProbeInput,
) -> ReadinessReport {
  let target = resolve_target_window(windows, input);
  let mut checks = vec![
    permission_check("accessibility", permissions.accessibility),
    permission_check("screen_recording", permissions.screen_recording),
  ];
  if permissions.screen_capture_kit != PermissionStatus::Granted {
    checks.push(ReadinessCheck::unknown(
      "screen_capture_kit",
      format!("screen_capture_kit permission is {}; native window listing may still be usable", permissions.screen_capture_kit.as_str()),
    ));
  } else {
    checks.push(ReadinessCheck::pass("screen_capture_kit", "screen_capture_kit permission granted"));
  }
  checks.push(permission_check("automation_to_system_events", permissions.automation_to_system_events));
  checks.push(match target {
    Some(window) => ReadinessCheck::pass("target_window_present", format!("target window {} is present", window.reference.id)),
    None => ReadinessCheck::fail("target_window_present", "target window is missing or no longer matches the execution plan"),
  });
  if input.require_frontmost {
    checks.push(frontmost_check(frontmost, target, input));
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
      input.app_bundle_id.as_ref().is_none_or(|expected| window.app_bundle_id.as_deref() == Some(expected.as_str()))
        && input.window_title.as_ref().is_none_or(|expected| window.title.as_deref().is_some_and(|title| title.contains(expected)))
    }
  })
}

fn permission_check(name: &str, status: PermissionStatus) -> ReadinessCheck {
  if status == PermissionStatus::Granted {
    ReadinessCheck::pass(name, format!("{name} permission granted"))
  } else {
    ReadinessCheck::fail(name, format!("{name} permission is {}", status.as_str()))
  }
}

fn frontmost_check(frontmost: Option<&Window>, target: Option<&Window>, input: &ReadinessProbeInput) -> ReadinessCheck {
  let Some(frontmost) = frontmost else {
    return ReadinessCheck::fail("target_app_frontmost", "frontmost window could not be resolved");
  };
  if let Some(target) = target
    && frontmost.reference.id != target.reference.id
  {
    return ReadinessCheck::fail(
      "target_app_frontmost",
      format!("frontmost window {} does not match target window {}", frontmost.reference.id, target.reference.id),
    );
  }
  if target.is_none()
    && let Some(expected_bundle) = input.app_bundle_id.as_deref()
    && frontmost.app_bundle_id.as_deref() != Some(expected_bundle)
  {
    return ReadinessCheck::fail(
      "target_app_frontmost",
      format!("frontmost app {:?} does not match target bundle {expected_bundle}", frontmost.app_bundle_id),
    );
  }
  ReadinessCheck::pass("target_app_frontmost", "target app/window is frontmost")
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
mod tests {
  use super::*;
  use auv_driver::{CoordinateSpace, Point, ReadinessStatus, Size, WindowRef};

  fn permissions() -> PermissionProbe {
    PermissionProbe {
      screen_recording: PermissionStatus::Granted,
      screen_capture_kit: PermissionStatus::Granted,
      accessibility: PermissionStatus::Granted,
      automation_to_system_events: PermissionStatus::Granted,
    }
  }

  fn window(id: &str, bundle: &str, title: &str, frame: Rect, is_main: bool) -> Window {
    Window {
      reference: WindowRef { id: id.to_string() },
      title: Some(title.to_string()),
      app_name: Some("TextEdit".to_string()),
      app_bundle_id: Some(bundle.to_string()),
      process_id: Some(42),
      frame,
      coordinate_space: CoordinateSpace::Screen,
      is_main,
      is_visible: true,
    }
  }

  fn input() -> ReadinessProbeInput {
    ReadinessProbeInput {
      window_number: Some(11),
      window_title: Some("Untitled".to_string()),
      app_bundle_id: Some("com.apple.TextEdit".to_string()),
      expected_window_frame: Some(Rect {
        origin: Point { x: 100.0, y: 80.0 },
        size: Size {
          width: 500.0,
          height: 300.0,
        },
      }),
      max_window_frame_drift_px: 2.0,
      require_frontmost: true,
      target_window_x: 10.0,
      target_window_y: 20.0,
    }
  }

  #[test]
  fn readiness_passes_when_permissions_window_and_frontmost_match() {
    let target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);

    let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

    assert!(report.is_ready());
    assert_eq!(report.target_window_ref.as_deref(), Some("11"));
    assert!(report.selected_blocker.is_none());
  }

  #[test]
  fn readiness_blocks_missing_accessibility_before_input_delivery() {
    let target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);
    let mut permissions = permissions();
    permissions.accessibility = PermissionStatus::Missing;

    let report = assess_readiness(&permissions, std::slice::from_ref(&target), Some(&target), &input());

    assert!(!report.is_ready());
    assert_eq!(report.status, ReadinessStatus::NotReady);
    assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("accessibility")));
  }

  #[test]
  fn readiness_blocks_window_drift() {
    let mut actual_frame = input().expected_window_frame.unwrap();
    actual_frame.origin.x += 20.0;
    let target = window("11", "com.apple.TextEdit", "Untitled", actual_frame, true);

    let report = assess_readiness(&permissions(), &[target.clone()], Some(&target), &input());

    assert!(!report.is_ready());
    assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("drift")));
  }

  #[test]
  fn readiness_blocks_missing_expected_window_frame() {
    let target = window(
      "11",
      "com.apple.TextEdit",
      "Untitled",
      Rect {
        origin: Point { x: 100.0, y: 80.0 },
        size: Size {
          width: 500.0,
          height: 300.0,
        },
      },
      true,
    );
    let mut input = input();
    input.expected_window_frame = None;

    let report = assess_readiness(&permissions(), &[target.clone()], Some(&target), &input);

    assert!(!report.is_ready());
    assert!(report.selected_blocker.as_deref().is_some_and(|reason| reason.contains("expected window frame")));
  }

  #[test]
  fn readiness_resolves_window_number_even_when_bundle_metadata_is_missing() {
    let mut target = window("11", "com.apple.TextEdit", "Untitled", input().expected_window_frame.unwrap(), true);
    target.app_bundle_id = None;

    let report = assess_readiness(&permissions(), std::slice::from_ref(&target), Some(&target), &input());

    assert!(report.is_ready());
    assert_eq!(report.target_window_ref.as_deref(), Some("11"));
  }
}
