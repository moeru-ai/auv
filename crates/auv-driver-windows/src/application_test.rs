use super::*;

#[test]
fn activation_verification_distinguishes_verified_foreground_from_activation_only() {
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("editor.exe".to_string()))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "editor.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("Editor.EXE".to_string()))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "Editor.EXE".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor", Ok(Some("Editor.exe".to_string()))),
    ApplicationActivationVerification::VerifiedForeground {
      observed_application_id: "Editor.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(Some("other.exe".to_string()))),
    ApplicationActivationVerification::ForegroundMismatch {
      observed_application_id: "other.exe".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Ok(None)),
    ApplicationActivationVerification::Unavailable {
      reason: "foreground window observation did not identify an owning process name".to_string(),
    }
  );
  assert_eq!(
    activation_verification("editor.exe", Err("observation denied".to_string())),
    ApplicationActivationVerification::Unavailable {
      reason: "observation denied".to_string(),
    }
  );
}

fn listed_window(
  id: &str,
  title: Option<&str>,
  app: Option<&str>,
  frame: auv_driver_common::geometry::Rect,
  is_main: bool,
) -> auv_driver_common::window::Window {
  auv_driver_common::window::Window {
    reference: auv_driver_common::window::WindowRef { id: id.to_string() },
    title: title.map(str::to_string),
    app_name: app.map(str::to_string),
    application_id: None,
    process_id: Some(42),
    frame,
    coordinate_space: auv_driver_common::geometry::CoordinateSpace::Screen,
    is_main,
    is_visible: true,
  }
}

// ROOT CAUSE (issue #324, Windows producer):
//
// Activation verified only the foreground process, so a frontmost app whose
// window is absent — or an app activated while another app held the
// foreground — left the caller with no window fact at all, and a follow-up
// window.capture failed without warning.
//
// Before the fix, the activation result carried no window observation. The
// observation now names present / absent / unknown as independent facts;
// presence never promises captureability and does not require the foreground.
#[test]
fn activation_window_observation_reports_present_absent_and_preference() {
  use auv_driver_common::geometry::Rect;

  // Present: the app's foreground window wins over its other windows and
  // other apps' windows.
  let listed = vec![
    listed_window("10", Some("Notes"), Some("editor.exe"), Rect::new(0.0, 0.0, 200.0, 100.0), false),
    listed_window("11", Some("Editor"), Some("editor.exe"), Rect::new(0.0, 0.0, 800.0, 600.0), true),
    listed_window("99", Some("Browser"), Some("web.exe"), Rect::new(0.0, 0.0, 900.0, 900.0), false),
  ];
  assert_eq!(
    activation_window_observation("editor.exe", &listed),
    ActivationWindowObservation::Present {
      reference: auv_driver_common::window::WindowRef {
        id: "11".to_string()
      }
    }
  );

  // The same app with no foreground window still reports Present: the
  // observation is independent of who owns the foreground.
  let background = vec![
    listed_window("21", Some("Editor"), Some("editor.exe"), Rect::new(0.0, 0.0, 800.0, 600.0), false),
    listed_window("99", Some("Browser"), Some("web.exe"), Rect::new(0.0, 0.0, 100.0, 100.0), true),
  ];
  assert_eq!(
    activation_window_observation("editor.exe", &background),
    ActivationWindowObservation::Present {
      reference: auv_driver_common::window::WindowRef {
        id: "21".to_string()
      }
    }
  );

  // Without a foreground or titled window, the largest frame is the main one.
  let untitled = vec![
    listed_window("31", None, Some("editor.exe"), Rect::new(0.0, 0.0, 100.0, 100.0), false),
    listed_window("32", None, Some("editor.exe"), Rect::new(0.0, 0.0, 500.0, 500.0), false),
  ];
  assert_eq!(
    activation_window_observation("editor.exe", &untitled),
    ActivationWindowObservation::Present {
      reference: auv_driver_common::window::WindowRef {
        id: "32".to_string()
      }
    }
  );

  // Absent: the observation succeeded and no window matches the app, even
  // though other apps own windows.
  assert_eq!(
    activation_window_observation("editor.exe", &listed[..1]),
    ActivationWindowObservation::Present {
      reference: auv_driver_common::window::WindowRef {
        id: "10".to_string()
      }
    }
  );
  let other_apps = vec![listed_window(
    "99",
    Some("Browser"),
    Some("web.exe"),
    Rect::new(0.0, 0.0, 100.0, 100.0),
    true,
  )];
  assert_eq!(activation_window_observation("editor.exe", &other_apps), ActivationWindowObservation::Absent);
  assert_eq!(activation_window_observation("editor.exe", &[]), ActivationWindowObservation::Absent);

  // The executable rule matches case and the omitted .exe suffix, exactly
  // like window selection and foreground verification.
  let cased = vec![listed_window(
    "41",
    Some("Editor"),
    Some("Editor.EXE"),
    Rect::new(0.0, 0.0, 100.0, 100.0),
    true,
  )];
  assert_eq!(
    activation_window_observation("editor", &cased),
    ActivationWindowObservation::Present {
      reference: auv_driver_common::window::WindowRef {
        id: "41".to_string()
      }
    }
  );
}

#[test]
fn activation_window_observation_absent_is_a_successful_empty_observation() {
  // An enumeration that succeeds with zero windows is Absent, not Unknown:
  // the caller produces Unknown only when the enumeration itself fails.
  let listed = Vec::new();
  assert_eq!(activation_window_observation("editor.exe", &listed), ActivationWindowObservation::Absent);
}
