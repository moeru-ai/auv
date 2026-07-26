use auv_driver_common::selector::{App, Window as SelectWindow};

use super::*;

#[test]
fn main_visible_picks_visible_window_without_requiring_main_flag() {
  let snapshot = observed_windows(vec![
    observed_window(1, 10, "com.example.music", "Music", "", 100, 80),
    observed_window(2, 10, "com.example.music", "Music", "Library", 300, 220),
  ]);
  let selector = SelectWindow::main_visible();

  let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

  assert_eq!(resolved.reference.id, "2");
}

#[test]
fn main_visible_owned_by_pid_picks_visible_window_for_owner() {
  let snapshot = observed_windows(vec![
    observed_window(1, 10, "com.example.music", "Music", "Search", 320, 240),
    observed_window(2, 20, "com.example.chat", "Chat", "Conversation", 640, 480),
  ]);
  let selector = SelectWindow::main_visible().owned_by(App::pid(10));

  let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

  assert_eq!(resolved.reference.id, "1");
}

#[test]
fn main_visible_owned_by_bundle_picks_visible_window_without_candidate_display_context() {
  let snapshot = observed_windows(vec![observed_window(
    307,
    15679,
    "com.netease.163music",
    "NetEaseMusic",
    "",
    1389,
    1050,
  )]);
  let selector = SelectWindow::main_visible().owned_by(App::bundle("com.netease.163music"));

  let resolved = resolve_from_observed_windows(&snapshot, &selector).unwrap();

  assert_eq!(resolved.reference.id, "307");
}

#[test]
fn display_selector_matches_native_id_and_compat_display_ref() {
  let targets = vec![display_target(0, "100", "display_0", false)];

  let by_native = resolve_display_target(&targets, Some("100")).unwrap();
  let by_ref = resolve_display_target(&targets, Some("display_0")).unwrap();

  assert_eq!(by_native.display.id, "100");
  assert_eq!(by_ref.display.name.as_deref(), Some("display_0"));
}

#[test]
fn display_selector_defaults_to_primary_display() {
  let targets = vec![
    display_target(0, "100", "display_0", false),
    display_target(1, "200", "display_1", true),
  ];

  let resolved = resolve_display_target(&targets, None).unwrap();

  assert_eq!(resolved.display.id, "200");
}

#[test]
fn display_region_resolution_requires_contained_global_region() {
  let targets = vec![display_target(0, "100", "display_0", true)];

  let resolved = resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 40.0, 50.0)).unwrap();
  let outside = resolve_display_for_global_region(&targets, None, Rect::new(10.0, 20.0, 2000.0, 50.0));

  assert_eq!(resolved.display.id, "100");
  assert!(matches!(outside, Err(DriverError::NotFound { .. })));
}

fn observed_windows(windows: Vec<ObservedWindow>) -> ObservedWindowSnapshot {
  ObservedWindowSnapshot {
    frontmost_app_name: String::new(),
    frontmost_app_bundle_id: String::new(),
    frontmost_window_title: String::new(),
    observed_at: "test".to_string(),
    windows,
  }
}

fn observed_window(
  window_number: i64,
  owner_pid: i64,
  owner_bundle_id: &str,
  app_name: &str,
  title: &str,
  width: i64,
  height: i64,
) -> ObservedWindow {
  ObservedWindow {
    window_number,
    app_name: app_name.to_string(),
    owner_pid,
    owner_bundle_id: owner_bundle_id.to_string(),
    layer: 0,
    title: title.to_string(),
    bounds: ObservedRect {
      x: 0,
      y: 0,
      width,
      height,
    },
  }
}

fn display_target(index: usize, native_id: &str, display_ref: &str, is_primary: bool) -> MacosDisplayTarget {
  MacosDisplayTarget {
    index,
    display: Display {
      id: native_id.to_string(),
      name: Some(display_ref.to_string()),
      frame: Rect::new(0.0, 0.0, 1000.0, 800.0),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 2.0,
      is_primary,
      is_builtin: Some(false),
    },
  }
}

mod no_steal_tests {
  use auv_driver_common::geometry::{ScreenPoint, WindowPoint};

  use super::super::*;

  fn sample_window() -> Window {
    Window {
      reference: WindowRef {
        id: "42".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: Some(123),
      frame: Rect::new(100.0, 200.0, 800.0, 600.0),
      coordinate_space: CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    }
  }

  #[test]
  fn window_point_converts_to_screen_point() {
    let window = sample_window();

    let point = screen_point_for_window_point(&window, WindowPoint::new(25.0, 30.0));

    assert_eq!(point, ScreenPoint::new(125.0, 230.0));
  }

  #[test]
  fn screen_point_converts_to_window_point() {
    let window = sample_window();

    let point = window_point_for_screen_point(&window, ScreenPoint::new(125.0, 230.0));

    assert_eq!(point, WindowPoint::new(25.0, 30.0));
  }

  #[test]
  fn window_number_parses_native_window_id() {
    let window = sample_window();

    let number = window_number(&window).expect("window number");

    assert_eq!(number, 42);
  }

  #[test]
  fn window_number_rejects_missing_or_invalid_native_window_id() {
    let mut missing = sample_window();
    missing.reference.id.clear();
    let mut invalid = sample_window();
    invalid.reference.id = "not-a-window-number".to_string();

    assert!(matches!(window_number(&missing), Err(DriverError::InvalidInput { .. })));
    assert!(matches!(window_number(&invalid), Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn window_pid_requires_owner_process_id() {
    let mut window = sample_window();
    window.process_id = None;

    assert!(matches!(window_pid(&window), Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn click_parts_converts_click_count_and_interval() {
    assert_eq!(click_parts(&Click::Single).expect("single click"), (1, 0));
    assert_eq!(
      click_parts(&Click::Double {
        interval: Duration::from_millis(75),
      })
      .expect("double click"),
      (2, 75)
    );
  }

  #[test]
  fn global_click_returns_typed_input_action_result() {
    let _: fn(&InputApi<'static>, Point, Click) -> DriverResult<InputActionResult> = InputApi::click_at;
  }

  #[test]
  fn paste_text_returns_typed_input_action_result() {
    let _: fn(&InputApi<'static>, PasteTextOptions) -> DriverResult<InputActionResult> = InputApi::paste_text;
  }

  #[test]
  fn type_text_parts_validate_submit_and_delay_without_delivery() {
    let parts = type_text_parts(TypeTextOptions {
      submit: TextSubmit::Return,
      inter_char_delay: Duration::from_millis(12),
      ..TypeTextOptions::default()
    })
    .expect("type text parts");

    assert_eq!(parts, (Some(36), 12));

    assert!(matches!(
      type_text_parts(TypeTextOptions {
        submit: TextSubmit::Search,
        ..TypeTextOptions::default()
      }),
      Err(DriverError::InvalidInput { .. })
    ));
    assert!(matches!(
      type_text_parts(TypeTextOptions {
        inter_char_delay: Duration::MAX,
        ..TypeTextOptions::default()
      }),
      Err(DriverError::InvalidInput { .. })
    ));
  }

  #[test]
  fn foreground_type_text_rejects_background_only_policy() {
    let error = type_text_foreground(
      "hello",
      TypeTextOptions {
        policy: InputPolicy::BackgroundOnly,
        ..TypeTextOptions::default()
      },
    )
    .expect_err("background-only foreground typing should be invalid");

    assert!(matches!(error, DriverError::InvalidInput { .. }));
  }

  #[test]
  fn special_key_code_supports_legacy_foreground_keys() {
    assert_eq!(special_key_code("return").expect("return"), 36);
    assert_eq!(special_key_code("enter").expect("enter"), 76);
    assert_eq!(special_key_code("tab").expect("tab"), 48);
    assert_eq!(special_key_code("backspace").expect("backspace"), 51);
    assert_eq!(special_key_code("esc").expect("esc"), 53);
    assert_eq!(special_key_code("space").expect("space"), 49);
  }

  #[test]
  fn permission_status_from_label_handles_native_labels() {
    assert_eq!(permission_status_from_label("granted"), PermissionStatus::Granted);
    assert_eq!(permission_status_from_label("missing"), PermissionStatus::Missing);
    assert_eq!(permission_status_from_label("new-native-status"), PermissionStatus::Unknown);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn empty_display_monitor_list_reports_permission_context() {
    let error = display_targets_from_monitors(&[]).expect_err("empty xcap monitor list should explain the likely permission boundary");

    match error {
      DriverError::PermissionDenied {
        permission,
        message,
        recovery,
      } => {
        assert_eq!(permission, "screen_recording");
        assert_eq!(message, None);
        assert!(recovery.as_deref().is_some_and(|message| message.contains("auv doctor --json")));
      }
      other => panic!("unexpected error: {other}"),
    }
  }

  #[test]
  fn foreground_text_keystroke_lines_keep_spaces_as_separate_events() {
    let mut lines = Vec::new();
    push_text_keystroke_lines(&mut lines, "For_Me", 20);

    assert_eq!(
      lines,
      vec![
        "keystroke \"F\"",
        "delay 0.020",
        "keystroke \"o\"",
        "delay 0.020",
        "keystroke \"r\"",
        "delay 0.020",
        "key code 27 using {shift down}",
        "delay 0.020",
        "keystroke \"M\"",
        "delay 0.020",
        "keystroke \"e\"",
        "delay 0.020",
      ]
    );
  }

  #[test]
  fn parse_shortcut_normalizes_supported_modifiers() {
    let parsed = parse_shortcut("cmd+shift+p").expect("shortcut");

    assert_eq!(
      parsed,
      ParsedShortcut {
        key: "p".to_string(),
        modifiers: vec!["command down", "shift down"],
      }
    );
  }

  #[test]
  fn parse_shortcut_rejects_multi_character_key() {
    assert!(matches!(parse_shortcut("cmd+return"), Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn input_api_exposes_explicit_global_hid_scroll_method() {
    // NOTICE(compile-only-api-check): nested fn is type-checked but never called (no HID side effects).
    fn assert_api_compiles(session: MacosDriverSession) {
      let _ = session.input().scroll_global_hid(Point::new(20.0, 30.0), Scroll::new(0.0, -120.0), Duration::ZERO);
    }
    let _ = assert_api_compiles;
  }

  #[test]
  fn input_api_exposes_foreground_text_and_key_methods() {
    fn assert_api_compiles(session: MacosDriverSession) {
      let _ = session.input().type_text(
        "hello",
        TypeTextOptions {
          policy: InputPolicy::ForegroundPreferred,
          ..TypeTextOptions::default()
        },
      );
      let _ = session.input().press_key(KeyPressOptions {
        key: "return".to_string(),
        settle: Duration::ZERO,
      });
    }
    let _ = assert_api_compiles;
  }

  #[test]
  fn permission_api_exposes_probe_method() {
    fn assert_api_compiles(session: MacosDriverSession) {
      let _ = session.permission().probe();
    }
    let _ = assert_api_compiles;
  }

  #[test]
  fn scroll_attempt_candidates_background_preferred_keep_background_before_foreground() {
    let candidates = scroll_attempt_candidates(&ScrollOptions::default());

    assert_eq!(
      candidates,
      vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ]
    );
  }

  #[test]
  fn scroll_attempt_candidates_foreground_preferred_uses_foreground_hid_first() {
    let candidates = scroll_attempt_candidates(&ScrollOptions {
      policy: InputPolicy::ForegroundPreferred,
      ..ScrollOptions::default()
    });

    assert_eq!(candidates, vec![ScrollDeliveryCandidate::ForegroundHid]);
  }

  #[test]
  fn scroll_attempt_candidates_background_only_drops_foreground_hid() {
    let candidates = scroll_attempt_candidates(&ScrollOptions {
      policy: InputPolicy::BackgroundOnly,
      ..ScrollOptions::default()
    });

    assert_eq!(
      candidates,
      vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
      ]
    );
  }

  #[test]
  fn prepare_for_input_rejects_unimplemented_focus_guard_without_activation() {
    let session = MacosDriverSession { _private: () };
    let window = sample_window();
    let options = PrepareForInputOptions {
      activation: ActivationPolicy::NoChange,
      preserve_frontmost: false,
      install_focus_guard: true,
      settle: Duration::ZERO,
    };

    let result = session.window().prepare_for_input(&window, options);

    assert!(matches!(
      result,
      Err(DriverError::Unsupported {
        operation: "focus_guard"
      })
    ));
  }

  #[test]
  fn foreground_input_with_default_restore_options_is_unsupported() {
    let session = MacosDriverSession { _private: () };
    let window = sample_window();
    let options = PrepareForInputOptions {
      activation: ActivationPolicy::Foreground {
        settle: Duration::ZERO,
      },
      ..PrepareForInputOptions::default()
    };

    let result = session.window().prepare_for_input(&window, options);

    assert!(matches!(
      result,
      Err(DriverError::Unsupported {
        operation: "foreground_restore"
      })
    ));
  }

  #[test]
  fn window_mutation_candidates_use_native_candidate_for_kind() {
    let candidates = window_mutation_candidates(&WindowMutationOptions::default());

    assert_eq!(
      candidates,
      vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ]
    );
    assert!(candidate_supports_window_mutation(
      candidates[0],
      WindowMutationKind::MoveTo {
        point: Point::new(10.0, 20.0),
      }
    ));
    assert!(!candidate_supports_window_mutation(
      candidates[1],
      WindowMutationKind::MoveTo {
        point: Point::new(10.0, 20.0),
      }
    ));

    assert!(candidate_supports_window_mutation(candidates[1], WindowMutationKind::Minimize,));
  }

  #[test]
  fn window_mutation_foreground_policy_is_explicit_deferred_candidate() {
    let candidates = window_mutation_candidates(&WindowMutationOptions {
      policy: WindowMutationPolicy::ForegroundPreferred,
      ..WindowMutationOptions::default()
    });

    assert_eq!(candidates, vec![WindowMutationCandidate::ForegroundSystemEvents]);
  }

  #[test]
  fn window_mutation_native_only_preserves_explicit_foreground_candidate() {
    let candidates = window_mutation_candidates(&WindowMutationOptions {
      policy: WindowMutationPolicy::NativeOnly,
      strategy: auv_driver_common::WindowMutationStrategy {
        candidates: vec![WindowMutationCandidate::ForegroundSystemEvents],
      },
      ..WindowMutationOptions::default()
    });

    assert_eq!(candidates, vec![WindowMutationCandidate::ForegroundSystemEvents]);
  }

  #[test]
  fn decoded_window_mutation_request_rounds_geometry_for_native_bridge() {
    let request = decoded_window_mutation_request(
      123,
      42,
      "Library".to_string(),
      WindowMutationKind::SetFrame {
        frame: Rect::new(10.4, 20.5, 800.2, 600.8),
      },
    )
    .expect("request");

    assert_eq!(request.pid, 123);
    assert_eq!(request.window_number, 42);
    assert_eq!(request.title, "Library");
    assert_eq!(request.kind, crate::native::window::DecodedWindowMutationKind::SetFrame);
    assert_eq!((request.x, request.y, request.width, request.height), (10, 21, 800, 601));
  }

  #[test]
  fn decoded_window_mutation_request_rejects_non_positive_size() {
    let result = decoded_window_mutation_request(
      123,
      42,
      String::new(),
      WindowMutationKind::Resize {
        size: Size::new(0.0, 100.0),
      },
    );

    assert!(matches!(result, Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn window_mutation_result_maps_native_frames_and_disturbance() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAttribute,
      vec![WindowMutationAttempt::success(
        WindowMutationPath::AxWindowAttribute,
        "set AXPosition",
      )],
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "move_to".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 30,
        after_y: 40,
        after_width: 800,
        after_height: 600,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    assert_eq!(result.selected_path, WindowMutationPath::AxWindowAttribute);
    assert_eq!(result.before_frame, Some(Rect::new(10.0, 20.0, 800.0, 600.0)));
    assert_eq!(result.after_frame, Some(Rect::new(30.0, 40.0, 800.0, 600.0)));
    assert_eq!(result.focus_disturbance, DisturbanceLevel::None);
    assert_eq!(result.mouse_disturbance, DisturbanceLevel::None);
  }

  #[test]
  fn window_mutation_frame_verification_rejects_clamped_frame() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAttribute,
      Vec::new(),
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "resize".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 10,
        after_y: 20,
        after_width: 400,
        after_height: 300,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    let error = verify_window_mutation(
      WindowMutationKind::Resize {
        size: Size::new(800.0, 600.0),
      },
      &WindowMutationVerification::FrameTolerance { points: 2.0 },
      &result,
    )
    .expect_err("clamped frame should fail verification");

    assert!(error.to_string().contains("frame.size.width"));
  }

  #[test]
  fn window_mutation_state_verification_rejects_failed_minimize() {
    let result = window_mutation_result(
      WindowMutationPath::AxWindowAction,
      Vec::new(),
      crate::native::window::DecodedWindowMutationResponse {
        performed_action: "minimize".to_string(),
        path: "pid=123 window_number=42".to_string(),
        before_x: 10,
        before_y: 20,
        before_width: 800,
        before_height: 600,
        after_x: 10,
        after_y: 20,
        after_width: 800,
        after_height: 600,
        was_minimized: false,
        is_minimized: false,
        error_message: None,
        recovery_hint: None,
      },
    );

    let error = verify_window_mutation(WindowMutationKind::Minimize, &WindowMutationVerification::BestEffortState, &result)
      .expect_err("failed minimize should fail verification");

    assert!(error.to_string().contains("not minimized"));
  }

  #[test]
  fn window_mutation_failure_preserves_attempt_messages() {
    let error = window_mutation_failure(vec![
      WindowMutationAttempt::failure(WindowMutationPath::AxWindowAttribute, "stale window"),
      WindowMutationAttempt::failure(WindowMutationPath::ForegroundSystemEvents, "foreground fallback deferred"),
    ]);

    let message = error.to_string();
    assert!(message.contains("stale window"));
    assert!(message.contains("foreground fallback deferred"));
  }
}
