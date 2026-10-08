use super::*;

fn test_capture_store() -> CaptureStore {
  CaptureStore::new(CaptureStoreOptions::default())
}

#[tokio::test]
async fn streamed_mouse_motion_rejects_cancel_before_begin() {
  let mut requests = tokio_stream::iter([Ok(proto::StreamMouseMotionRequest {
    event: Some(proto::stream_mouse_motion_request::Event::Cancel(proto::StreamMouseMotionCancel {})),
  })]);
  let (sender, _receiver) = tokio::sync::mpsc::channel(1);

  let status = collect_mouse_motion(&auv_driver::open_local().unwrap(), &mut requests, &sender).await.expect_err("cancel must follow begin");

  assert_eq!(status.code(), tonic::Code::InvalidArgument);
  assert_eq!(status.message(), "moveMouse cancel requires begin");
}

#[test]
fn overlay_thread_guard_rejects_a_different_execution_thread_without_ui() {
  let owner = std::thread::current().id();
  ensure_overlay_owner_thread(owner).expect("owner thread");
  let status =
    std::thread::spawn(move || ensure_overlay_owner_thread(owner).expect_err("different thread must fail")).join().expect("thread");
  assert_eq!(status.code(), tonic::Code::FailedPrecondition);
}

#[test]
fn overlay_mapper_uses_owner_defaults_for_absent_optional_messages() {
  let overlay = overlay_from_proto(proto::Overlay {
    layers: vec![proto::OverlayLayer {
      layer: Some(proto::overlay_layer::Layer::Outline(proto::Outline {
        rect: Some(proto::ScreenRect {
          x: 10.0,
          y: 20.0,
          width: 30.0,
          height: 40.0,
        }),
        label: None,
        label_visible: false,
        style: None,
      })),
    }],
  })
  .expect("owner defaults");
  assert_eq!(overlay.layers().len(), 1);
  assert_eq!(overlay_options_from_proto(None).expect("default options"), auv_driver::overlay::ShowOptions::new());
}

#[test]
fn overlay_mapper_rejects_malformed_values_before_native_rendering() {
  let invalid_point = proto::Overlay {
    layers: vec![proto::OverlayLayer {
      layer: Some(proto::overlay_layer::Layer::Cursor(proto::Cursor {
        point: Some(proto::ScreenPoint {
          x: f64::NAN,
          y: 0.0,
        }),
        ..Default::default()
      })),
    }],
  };
  assert_eq!(overlay_from_proto(invalid_point).expect_err("nonfinite point").code(), tonic::Code::InvalidArgument);

  let oversized_svg = proto::Overlay {
    layers: vec![proto::OverlayLayer {
      layer: Some(proto::overlay_layer::Layer::Cursor(proto::Cursor {
        point: Some(proto::ScreenPoint { x: 0.0, y: 0.0 }),
        image: Some(proto::CursorImage {
          image: Some(proto::cursor_image::Image::Svg("x".repeat(256 * 1024 + 1))),
        }),
        ..Default::default()
      })),
    }],
  };
  assert_eq!(overlay_from_proto(oversized_svg).expect_err("SVG bound").code(), tonic::Code::InvalidArgument);

  let unknown_easing = proto::ShowOptions {
    motion: Some(proto::MotionOptions {
      duration: None,
      easing: Some(999),
    }),
    lifecycle: None,
  };
  assert_eq!(overlay_options_from_proto(Some(unknown_easing)).expect_err("unknown easing").code(), tonic::Code::InvalidArgument);
  let negative_duration = proto::ShowOptions {
    motion: Some(proto::MotionOptions {
      duration: Some(prost_types::Duration {
        seconds: -1,
        nanos: 0,
      }),
      easing: None,
    }),
    lifecycle: None,
  };
  assert_eq!(overlay_options_from_proto(Some(negative_duration)).expect_err("negative duration").code(), tonic::Code::InvalidArgument);
}

#[test]
fn permission_probe_mapper_preserves_every_status() {
  let mapped = permission_probe_to_proto(auv_driver::PermissionProbe {
    screen_recording: auv_driver::PermissionStatus::Granted,
    screen_capture_kit: auv_driver::PermissionStatus::Missing,
    accessibility: auv_driver::PermissionStatus::Unknown,
    automation_to_system_events: auv_driver::PermissionStatus::Granted,
  });
  assert_eq!(mapped.screen_recording, macos_proto::PermissionStatus::Granted as i32);
  assert_eq!(mapped.screen_capture_kit, macos_proto::PermissionStatus::Missing as i32);
  assert_eq!(mapped.accessibility, macos_proto::PermissionStatus::Unknown as i32);
  assert_eq!(mapped.automation_to_system_events, macos_proto::PermissionStatus::Granted as i32);
}

#[test]
fn application_activation_mapper_preserves_each_verification_variant() {
  use auv_api_proto::auv::api::driver::macos::v1::application_activation_verification::Verification;

  let cases = [
    auv_driver::ApplicationActivationVerification::VerifiedForeground {
      observed_bundle_id: "com.example.Verified".to_string(),
    },
    auv_driver::ApplicationActivationVerification::ForegroundMismatch {
      observed_bundle_id: "com.example.Other".to_string(),
    },
    auv_driver::ApplicationActivationVerification::Unavailable {
      reason: "update unavailable".to_string(),
    },
  ];
  for verification in cases {
    let mapped = application_activation_to_proto(auv_driver::ApplicationActivationResult {
      requested_bundle_id: "com.example.Requested".to_string(),
      verification,
    });
    assert_eq!(mapped.requested_bundle_id, "com.example.Requested");
    assert!(matches!(
      mapped.verification.and_then(|verification| verification.verification),
      Some(Verification::VerifiedForeground(_) | Verification::ForegroundMismatch(_) | Verification::Unavailable(_))
    ));
  }
}

#[test]
fn application_request_validation_rejects_blank_bundle_and_invalid_duration() {
  assert_eq!(
    duration_from_proto(
      Some(prost_types::Duration {
        seconds: -1,
        nanos: 0,
      }),
      std::time::Duration::from_millis(150),
      "settle",
    )
    .expect_err("negative settle must fail before activation")
    .code(),
    tonic::Code::InvalidArgument
  );
  assert_eq!(application_bundle_id("  ").expect_err("blank bundle id").code(), tonic::Code::InvalidArgument);
}

#[test]
fn accessibility_request_validation_rejects_malformed_selector_before_native_capture() {
  for request in [
    macos_proto::FocusTextRequest::default(),
    macos_proto::FocusTextRequest {
      application: "com.example.Editor".to_string(),
      selector: Some(macos_proto::focus_text_request::Selector::Query("".to_string())),
      ..Default::default()
    },
    macos_proto::FocusTextRequest {
      application: "com.example.Editor".to_string(),
      selector: Some(macos_proto::focus_text_request::Selector::Path("  ".to_string())),
      ..Default::default()
    },
    macos_proto::FocusTextRequest {
      application: "com.example.Editor".to_string(),
      selector: Some(macos_proto::focus_text_request::Selector::Query("Search".to_string())),
      expected_role: Some("".to_string()),
      ..Default::default()
    },
  ] {
    assert_eq!(focus_text_options_from_proto(request).expect_err("malformed focus request").code(), tonic::Code::InvalidArgument);
  }
}

#[test]
fn now_playing_mapper_preserves_owner_state_and_optional_presence() {
  let mapped = now_playing_to_proto(auv_media_macos::NowPlayingState {
    present: true,
    is_playing: true,
    source_bundle_id: Some("com.apple.Music".to_string()),
    title: Some("Current Song".to_string()),
    artist: Some("The Artist".to_string()),
    album: None,
    duration_seconds: Some(245.5),
    elapsed_seconds: Some(61.25),
    playback_rate: Some(1.0),
    content_item_id: Some("track-42".to_string()),
    supports_like: Some(true),
    is_liked: None,
  })
  .expect("finite owner state");
  assert!(mapped.present);
  assert!(mapped.is_playing);
  assert_eq!(mapped.source_bundle_id.as_deref(), Some("com.apple.Music"));
  assert_eq!(mapped.title.as_deref(), Some("Current Song"));
  assert_eq!(mapped.artist.as_deref(), Some("The Artist"));
  assert_eq!(mapped.album, None);
  assert_eq!(mapped.duration_seconds, Some(245.5));
  assert_eq!(mapped.elapsed_seconds, Some(61.25));
  assert_eq!(mapped.playback_rate, Some(1.0));
  assert_eq!(mapped.content_item_id.as_deref(), Some("track-42"));
  assert_eq!(mapped.supports_like, Some(true));
  assert_eq!(mapped.is_liked, None);
}

#[test]
fn now_playing_mapper_rejects_non_finite_backend_numbers() {
  for (field, state) in [
    (
      "duration_seconds",
      auv_media_macos::NowPlayingState {
        duration_seconds: Some(f64::NAN),
        ..Default::default()
      },
    ),
    (
      "elapsed_seconds",
      auv_media_macos::NowPlayingState {
        elapsed_seconds: Some(f64::INFINITY),
        ..Default::default()
      },
    ),
    (
      "playback_rate",
      auv_media_macos::NowPlayingState {
        playback_rate: Some(f64::NEG_INFINITY),
        ..Default::default()
      },
    ),
  ] {
    let error = now_playing_to_proto(state).expect_err("non-finite backend value must fail closed");
    assert_eq!(error.code(), tonic::Code::Internal);
    assert!(error.message().contains(field));
  }
}

#[test]
fn unsupported_media_backend_maps_to_unimplemented() {
  assert_eq!(media_status(auv_media_macos::MediaError::Unsupported).code(), tonic::Code::Unimplemented);
}

#[test]
fn uncertain_media_control_failure_is_not_exposed_as_retryable_unavailable() {
  let status = media_control_status(auv_media_macos::MediaError::Native {
    message: "verification read failed".to_string(),
    recovery_hint: "inspect state before retrying".to_string(),
  });
  assert_eq!(status.code(), tonic::Code::Unknown);
  assert!(status.message().contains("do not retry automatically"));
}

#[test]
fn media_control_outcome_mapper_preserves_before_after_and_verification() {
  let before = auv_media_macos::NowPlayingState {
    present: true,
    title: Some("Before".to_string()),
    is_playing: false,
    ..Default::default()
  };
  let after = auv_media_macos::NowPlayingState {
    present: true,
    title: Some("After".to_string()),
    is_playing: true,
    ..Default::default()
  };
  let mapped = media_control_outcome_to_proto(auv_media_macos::output::MediaControlOutcome {
    command: "play",
    before: auv_media_macos::output::build_now_playing_output(&before),
    after: auv_media_macos::output::build_now_playing_output(&after),
    verified: true,
  })
  .expect("valid outcome");
  assert_eq!(mapped.before.and_then(|state| state.title).as_deref(), Some("Before"));
  assert_eq!(mapped.after.and_then(|state| state.title).as_deref(), Some("After"));
  assert!(mapped.verified);
}

#[test]
fn image_frame_preserves_alpha_and_screen_bounds() {
  let capture = auv_driver::Capture {
    origin: Some(auv_driver::Position::in_window(
      &auv_driver::WindowRef {
        id: "window-7".into(),
      },
      auv_driver::WindowPoint::new(0.0, 0.0),
    )),
    image: image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("valid RGBA fixture"),
    bounds: auv_driver::Rect::new(10.0, 20.0, 1.0, 0.5),
    scale_factor: 2.0,
    backend: "fixture".to_string(),
    fallback_reason: Some("fallback".to_string()),
  };

  let frame = image_frame_to_proto(capture.clone());
  assert_eq!(image_frame_from_proto(frame.clone()).unwrap(), capture);

  assert_eq!(frame.image.as_ref().expect("image").data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(
    frame.bounds,
    Some(proto::ScreenRect {
      x: 10.0,
      y: 20.0,
      width: 1.0,
      height: 0.5
    })
  );
  assert_eq!(frame.scale_factor, 2.0);
  assert_eq!(frame.backend, "fixture");
  assert_eq!(frame.fallback_reason.as_deref(), Some("fallback"));
}

#[test]
fn text_recognition_image_rejects_malformed_rgba_before_ocr() {
  let error = image_frame_from_proto(proto::ImageFrame {
    origin: None,
    image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
      width: 2,
      height: 1,
      data: vec![0; 7],
    }),
    bounds: Some(proto::ScreenRect {
      x: 0.0,
      y: 0.0,
      width: 2.0,
      height: 1.0,
    }),
    scale_factor: 1.0,
    ..Default::default()
  })
  .expect_err("malformed RGBA frame");
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("expected 8"));
}

#[test]
fn text_recognition_region_must_stay_inside_normalized_bounds() {
  let error = ratio_rect_from_proto(Some(auv_api_proto::auv::api::image::v1::NormalizedRect {
    x: 0.8,
    y: 0.0,
    width: 0.3,
    height: 1.0,
  }))
  .expect_err("out-of-bounds region");
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert_eq!(ratio_rect_from_proto(None).unwrap(), auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0));
}

#[test]
fn recognized_text_mapper_preserves_screen_bounds_and_confidence() {
  let response = recognition_to_proto(auv_driver::TextRecognition {
    origin: Some(auv_driver::Position::in_window(
      &auv_driver::WindowRef {
        id: "window-7".into(),
      },
      auv_driver::WindowPoint::new(-10.0, -20.0),
    )),
    text: "hello".to_string(),
    regions: vec![auv_driver::RecognizedText {
      text: "hello".to_string(),
      bounds: auv_driver::Rect::new(10.0, 20.0, 30.0, 40.0),
      confidence: Some(0.75),
    }],
  });
  assert_eq!(
    position_from_proto(response.origin.unwrap()).unwrap().coordinate_space,
    auv_driver::CoordinateSpace::Window("window-7".into())
  );
  assert_eq!(response.text, "hello");
  assert_eq!(response.regions[0].confidence, Some(0.75));
  assert_eq!(response.regions[0].bounds.as_ref().map(|bounds| bounds.x), Some(10.0));
}

#[test]
fn input_options_reject_malformed_values_before_delivery() {
  let count_error = click_options_from_proto(Some(proto::ClickOptions {
    click: Some(proto::Click {
      count: 256,
      interval: Some(prost_types::Duration {
        seconds: 0,
        nanos: 75_000_000,
      }),
    }),
    ..Default::default()
  }))
  .expect_err("click count outside the driver u8 contract");
  assert_eq!(count_error.code(), tonic::Code::InvalidArgument);

  let duration_error = type_text_options_from_proto(Some(proto::TypeTextOptions {
    inter_char_delay: Some(prost_types::Duration {
      seconds: -1,
      nanos: 0,
    }),
    ..Default::default()
  }))
  .expect_err("negative protobuf duration");
  assert_eq!(duration_error.code(), tonic::Code::InvalidArgument);

  let point_error = position_from_proto(proto::Position {
    x: f64::NAN,
    y: 0.0,
    coordinate_space: Some(proto::position::CoordinateSpace::WindowId("window-1".to_string())),
  })
  .expect_err("non-finite point");
  assert_eq!(point_error.code(), tonic::Code::InvalidArgument);

  let screen_point_error = screen_point_from_proto(proto::ScreenPoint {
    x: 0.0,
    y: f64::INFINITY,
  })
  .expect_err("non-finite screen point must fail before native input delivery");
  assert_eq!(screen_point_error.code(), tonic::Code::InvalidArgument);

  let empty_paste = paste_text_options_from_proto(String::new(), Some(Default::default()))
    .expect_err("empty paste text must fail before clipboard capture or mutation");
  assert_eq!(empty_paste.code(), tonic::Code::InvalidArgument);

  let unknown_submit = paste_text_options_from_proto(
    "text".to_string(),
    Some(proto::PasteTextOptions {
      submit: 99,
      ..Default::default()
    }),
  )
  .expect_err("unknown paste submit enum must fail before clipboard mutation");
  assert_eq!(unknown_submit.code(), tonic::Code::InvalidArgument);

  let negative_settle = paste_text_options_from_proto(
    "text".to_string(),
    Some(proto::PasteTextOptions {
      settle: Some(prost_types::Duration {
        seconds: -1,
        nanos: 0,
      }),
      ..Default::default()
    }),
  )
  .expect_err("negative paste settle must fail before clipboard mutation");
  assert_eq!(negative_settle.code(), tonic::Code::InvalidArgument);
}

#[test]
fn click_rpc_preserves_modifiers_for_window_and_screen_delivery() {
  let modifiers = proto::ClickModifiers {
    shift: true,
    control: true,
    alt: true,
    meta: true,
  };
  let window = click_options_from_proto(Some(proto::ClickOptions {
    modifiers: Some(modifiers),
    ..Default::default()
  }))
  .unwrap();
  let screen = global_click_options_from_proto(Some(proto::ClickOptions {
    modifiers: Some(modifiers),
    ..Default::default()
  }))
  .unwrap()
  .modifiers;
  assert_eq!(
    window.modifiers,
    auv_driver::ClickModifiers {
      shift: true,
      control: true,
      alt: true,
      meta: true
    }
  );
  assert_eq!(screen, window.modifiers);
  assert!(click_options_from_proto(None).unwrap().modifiers.is_empty());
  assert!(global_click_options_from_proto(None).unwrap().modifiers.is_empty());
}

#[test]
fn global_click_rejects_window_delivery_options() {
  // A screen or display click has no target window. Window policy and
  // strategy are rejected instead of being silently ignored by the driver.
  let policy = global_click_options_from_proto(Some(proto::ClickOptions {
    policy: proto::InputPolicy::BackgroundOnly as i32,
    ..Default::default()
  }))
  .unwrap_err();
  assert_eq!(policy.code(), tonic::Code::InvalidArgument);
  let strategy = global_click_options_from_proto(Some(proto::ClickOptions {
    window_strategy: proto::WindowClickStrategy::PidTargeted as i32,
    ..Default::default()
  }))
  .unwrap_err();
  assert_eq!(strategy.code(), tonic::Code::InvalidArgument);
}

#[test]
fn window_scoped_positions_convert_with_the_current_window_frame() {
  let window = auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: "window-1".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: auv_driver::Rect::new(100.0, 50.0, 400.0, 300.0),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let no_display = |id: &str| -> Result<auv_driver::Point, Status> { panic!("unexpected display lookup for {id}") };
  let at = |x, y, coordinate_space| auv_driver::Position {
    point: auv_driver::Point::new(x, y),
    coordinate_space,
  };

  let local = window_point_for_position(&window, &at(10.0, 20.0, auv_driver::CoordinateSpace::Window("window-1".into())), no_display);
  assert_eq!(local.unwrap(), auv_driver::WindowPoint::new(10.0, 20.0));

  let screen = window_point_for_position(&window, &at(110.0, 70.0, auv_driver::CoordinateSpace::Screen), no_display);
  assert_eq!(screen.unwrap(), auv_driver::WindowPoint::new(10.0, 20.0));

  let display = window_point_for_position(&window, &at(10.0, 20.0, auv_driver::CoordinateSpace::Display("d2".into())), |id| {
    assert_eq!(id, "d2");
    Ok(auv_driver::Point::new(-1000.0, 0.0))
  });
  assert_eq!(display.unwrap(), auv_driver::WindowPoint::new(-1090.0, -30.0));

  let other = window_point_for_position(&window, &at(10.0, 20.0, auv_driver::CoordinateSpace::Window("window-2".into())), no_display);
  assert_eq!(other.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[test]
fn input_action_mapper_preserves_attempts_and_disturbance() {
  let action = input_action_to_proto(auv_driver::InputActionResult {
    selected_path: auv_driver::InputDeliveryPath::ClipboardPaste,
    attempts: vec![
      auv_driver::InputAttempt::failure(auv_driver::InputDeliveryPath::WindowTargetedKeyboard, "background unavailable"),
      auv_driver::InputAttempt::success(auv_driver::InputDeliveryPath::ClipboardPaste),
    ],
    verified: false,
    mouse_disturbance: auv_driver::DisturbanceLevel::None,
    focus_disturbance: auv_driver::DisturbanceLevel::Foreground,
    clipboard_disturbance: auv_driver::DisturbanceLevel::Temporary,
  })
  .expect("valid canonical action");

  assert_eq!(action.selected_path, proto::InputDeliveryPath::ClipboardPaste as i32);
  assert_eq!(action.attempts.len(), 2);
  assert_eq!(action.attempts[0].message.as_deref(), Some("background unavailable"));
  assert_eq!(action.focus_disturbance, proto::DisturbanceLevel::Foreground as i32);
  assert_eq!(action.clipboard_disturbance, proto::DisturbanceLevel::Temporary as i32);
}

#[test]
fn driver_errors_keep_their_grpc_semantics() {
  assert_eq!(driver_status(auv_driver::DriverError::unsupported("vision.ocr")).code(), tonic::Code::Unimplemented);
  assert_eq!(
    driver_status(auv_driver::DriverError::PermissionDenied {
      permission: "screen-recording",
      message: None,
      recovery: None,
    })
    .code(),
    tonic::Code::PermissionDenied
  );
}

#[test]
fn overlay_shadow_mapper_preserves_native_dimensions_and_rejects_invalid_blur() {
  let make_style = |blur_radius| proto::CursorStyle {
    label_foreground: Some(proto::Color {
      red: 1.0,
      green: 1.0,
      blue: 1.0,
      alpha: 1.0,
    }),
    label_background: Some(proto::Color {
      red: 0.0,
      green: 0.0,
      blue: 0.0,
      alpha: 1.0,
    }),
    label_padding: Some(proto::Insets::default()),
    sprite_size: 24.0,
    shadow: Some(proto::Shadow {
      color: Some(proto::Color {
        red: 1.0,
        green: 0.6,
        blue: 0.15,
        alpha: 0.65,
      }),
      blur_radius,
      offset_x: -1.0,
      offset_y: 2.0,
    }),
    ..Default::default()
  };
  let style = cursor_style_from_proto(make_style(8.0)).unwrap();
  assert_eq!(style.sprite_size, 24.0);
  assert_eq!(style.shadow.unwrap().offset_x, -1.0);
  assert_eq!(style.shadow.unwrap().blur_radius, 8.0);
  assert_eq!(cursor_style_from_proto(make_style(-1.0)).unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn keyboard_rpc_requires_explicit_recipient_before_delivery() {
  let service = LocalInputService {
    session: auv_driver::open_local().unwrap(),
    captures: test_capture_store(),
  };
  let error = service
    .input_keyboard(Request::new(proto::InputKeyboardRequest {
      inputs: vec![keyboard_press_request("a", 1)],
      ..Default::default()
    }))
    .await
    .expect_err("missing target must not become global input");
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert_eq!(error.message(), "target is required");
}

fn keyboard_press_request(key: &str, count: u32) -> proto::KeyboardInput {
  proto::KeyboardInput {
    action: Some(proto::keyboard_input::Action::Press(proto::KeyboardPress {
      policy: proto::InputPolicy::ForegroundPreferred as i32,
      options: Some(proto::PressKeysOptions {
        keys: vec![key.into()],
        count: Some(count),
        ..Default::default()
      }),
    })),
  }
}

// An observed Window must retain its owner. A changed pid fails before native
// activation, including during dry-runs.
#[tokio::test]
#[ignore = "requires a live macOS WindowServer"]
#[cfg(target_os = "macos")]
async fn targeted_keyboard_rpc_rejects_changed_window_owner() {
  let session = auv_driver::open_local().unwrap();
  let window = session.window().list().unwrap().into_iter().find(|window| window.process_id.is_some()).unwrap();
  let service = LocalInputService {
    session,
    captures: test_capture_store(),
  };
  for dry_run in [false, true] {
    let error = service
      .input_keyboard(Request::new(proto::InputKeyboardRequest {
        target: Some(proto::InputTarget {
          recipient: Some(proto::input_target::Recipient::Window(proto::Window {
            r#ref: Some(proto::WindowRef {
              window_id: window.reference.id.clone(),
            }),
            process_id: Some(window.process_id.unwrap() + 1),
            ..Default::default()
          })),
        }),
        dry_run,
        inputs: vec![keyboard_press_request("a", 1)],
      }))
      .await
      .unwrap_err();
    assert_eq!(error.code(), tonic::Code::FailedPrecondition);
    assert_eq!(error.message(), "target window owner changed; resolve the target again");
  }
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn keyboard_rpc_retains_failed_action_index_before_any_delivery() {
  use prost::Message;
  let service = LocalInputService {
    session: auv_driver::open_local().unwrap(),
    captures: test_capture_store(),
  };
  let error = service
    .input_keyboard(Request::new(proto::InputKeyboardRequest {
      target: Some(proto::InputTarget {
        recipient: Some(proto::input_target::Recipient::Foreground(true)),
      }),
      inputs: vec![
        keyboard_press_request("a", 1),
        keyboard_press_request("not-a-key", 1),
      ],
      dry_run: false,
    }))
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  let progress = proto::KeyboardInputProgress::decode(error.details()).unwrap();
  assert_eq!(progress.action_index, Some(1));
  assert_eq!(progress.completed_presses, 0);
  assert!(progress.completed.is_empty());
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn press_keys_rpc_uses_the_keyboard_repeat_validation_contract() {
  use prost::Message;
  let service = LocalInputService {
    session: auv_driver::open_local().unwrap(),
    captures: test_capture_store(),
  };
  let error = service
    .press_keys(Request::new(proto::PressKeysRequest {
      target: Some(proto::InputTarget {
        recipient: Some(proto::input_target::Recipient::Foreground(true)),
      }),
      options: Some(proto::PressKeysOptions {
        keys: vec!["a".into()],
        count: Some(2),
        ..Default::default()
      }),
      policy: proto::InputPolicy::ForegroundPreferred as i32,
      dry_run: true,
    }))
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert!(error.message().contains("interval"));
  assert_eq!(proto::KeyboardInputProgress::decode(error.details()).unwrap().action_index, Some(0));
}

#[tokio::test]
async fn keyboard_hold_rpc_rejects_missing_deadline_before_delivery() {
  let service = LocalInputService {
    session: auv_driver::open_local().unwrap(),
    captures: test_capture_store(),
  };
  let target = proto::InputTarget {
    recipient: Some(proto::input_target::Recipient::Foreground(true)),
  };
  let down = service
    .key_down(Request::new(proto::KeyDownRequest {
      target: Some(target.clone()),
      keys: vec!["shift".into()],
      policy: proto::InputPolicy::ForegroundPreferred as i32,
      timeout: None,
    }))
    .await
    .unwrap_err();
  assert_eq!(down.code(), tonic::Code::InvalidArgument);
  assert!(down.message().contains("timeout"));

  let hold = service
    .hold_keys(Request::new(proto::HoldKeysRequest {
      target: Some(target),
      keys: vec!["shift".into()],
      policy: proto::InputPolicy::ForegroundPreferred as i32,
      duration: None,
    }))
    .await
    .unwrap_err();
  assert_eq!(hold.code(), tonic::Code::InvalidArgument);
  assert!(hold.message().contains("duration"));
}

#[tokio::test]
async fn keyboard_rpc_reports_wire_validation_position_without_delivering_prefix() {
  use prost::Message;
  let service = LocalInputService {
    session: auv_driver::open_local().unwrap(),
    captures: test_capture_store(),
  };
  let error = service
    .input_keyboard(Request::new(proto::InputKeyboardRequest {
      target: Some(proto::InputTarget {
        recipient: Some(proto::input_target::Recipient::Foreground(true)),
      }),
      inputs: vec![
        keyboard_press_request("a", 1),
        proto::KeyboardInput { action: None },
      ],
      dry_run: false,
    }))
    .await
    .unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  let progress = proto::KeyboardInputProgress::decode(error.details()).unwrap();
  assert_eq!(progress.action_index, Some(1));
  assert!(progress.completed.is_empty());
}

#[cfg(target_os = "linux")]
mod linux_keyboard_tests {
  use super::super::*;

  #[tokio::test]
  async fn keyboard_rpc_reaches_linux_batch_validation_and_preserves_error_progress() {
    use auv_driver::Driver;
    let service = LocalInputService {
      session: auv_driver::LocalDriver::new().open_local().unwrap(),
      captures: super::test_capture_store(),
    };
    let press = |key: &str| proto::KeyboardInput {
      action: Some(proto::keyboard_input::Action::Press(proto::KeyboardPress {
        options: Some(proto::PressKeysOptions {
          keys: vec![key.into()],
          count: Some(1),
          ..Default::default()
        }),
        policy: proto::InputPolicy::ForegroundPreferred as i32,
      })),
    };
    let target = Some(proto::InputTarget {
      recipient: Some(proto::input_target::Recipient::Foreground(true)),
    });
    let result = service
      .input_keyboard(Request::new(proto::InputKeyboardRequest {
        target: target.clone(),
        inputs: vec![press("a")],
        dry_run: true,
      }))
      .await
      .unwrap();
    assert!(result.into_inner().actions.is_empty());
    let error = service
      .input_keyboard(Request::new(proto::InputKeyboardRequest {
        target,
        inputs: vec![press("a"), press("invalid-key")],
        dry_run: false,
      }))
      .await
      .unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    use prost::Message;
    let progress = proto::KeyboardInputProgress::decode(error.details()).unwrap();
    assert_eq!(progress.action_index, Some(1));
    assert_eq!(progress.completed_presses, 0);
    assert!(progress.completed.is_empty());
  }
}

#[test]
fn click_rpc_decodes_all_buttons_and_rejects_unknown_before_delivery() {
  for (wire, button) in [
    (proto::MouseButton::Unspecified, auv_driver::MouseButton::Left),
    (proto::MouseButton::Left, auv_driver::MouseButton::Left),
    (proto::MouseButton::Right, auv_driver::MouseButton::Right),
    (proto::MouseButton::Middle, auv_driver::MouseButton::Middle),
  ] {
    assert_eq!(
      click_options_from_proto(Some(proto::ClickOptions {
        button: wire as i32,
        ..Default::default()
      }))
      .unwrap()
      .button,
      button
    );
    assert_eq!(
      global_click_options_from_proto(Some(proto::ClickOptions {
        button: wire as i32,
        ..Default::default()
      }))
      .unwrap()
      .button,
      button
    );
  }
  assert_eq!(click_options_from_proto(None).unwrap().button, auv_driver::MouseButton::Left);
  assert_eq!(
    click_options_from_proto(Some(proto::ClickOptions {
      button: 99,
      ..Default::default()
    }))
    .unwrap_err()
    .code(),
    tonic::Code::InvalidArgument
  );
  assert_eq!(
    global_click_options_from_proto(Some(proto::ClickOptions {
      button: 99,
      ..Default::default()
    }))
    .unwrap_err()
    .code(),
    tonic::Code::InvalidArgument
  );
}

#[test]
fn scroll_rpc_accepts_only_finite_non_zero_deltas_before_delivery() {
  let scroll = scroll_from_proto(Some(proto::Scroll {
    delta_x: -40.0,
    delta_y: 300.0,
  }))
  .unwrap();
  assert_eq!(scroll, auv_driver::Scroll::new(-40.0, 300.0));

  for malformed in [
    None,
    Some(proto::Scroll::default()),
    Some(proto::Scroll {
      delta_x: 0.0,
      delta_y: f64::NAN,
    }),
    Some(proto::Scroll {
      delta_x: f64::INFINITY,
      delta_y: 10.0,
    }),
  ] {
    assert_eq!(scroll_from_proto(malformed).unwrap_err().code(), tonic::Code::InvalidArgument);
  }
}

#[test]
fn scroll_rpc_preserves_candidate_order_and_rejects_unknown_or_repeated_candidates() {
  assert_eq!(scroll_options_from_proto(None).unwrap(), auv_driver::ScrollOptions::default());

  let options = scroll_options_from_proto(Some(proto::ScrollOptions {
    policy: proto::InputPolicy::BackgroundOnly as i32,
    delivery_candidates: vec![
      proto::ScrollDeliveryCandidate::WindowTargetedWheel as i32,
      proto::ScrollDeliveryCandidate::AxScroll as i32,
    ],
    settle: Some(prost_types::Duration {
      seconds: 0,
      nanos: 250_000_000,
    }),
  }))
  .unwrap();
  assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundOnly);
  assert_eq!(
    options.delivery_strategy.candidates,
    vec![
      auv_driver::ScrollDeliveryCandidate::WindowTargetedWheel,
      auv_driver::ScrollDeliveryCandidate::AxScroll,
    ]
  );
  assert_eq!(options.settle, std::time::Duration::from_millis(250));

  let empty = scroll_options_from_proto(Some(proto::ScrollOptions::default())).unwrap();
  assert_eq!(empty.delivery_strategy, auv_driver::ScrollDeliveryStrategy::default());

  for candidates in [
    vec![proto::ScrollDeliveryCandidate::Unspecified as i32],
    vec![99],
    vec![
      proto::ScrollDeliveryCandidate::ForegroundHid as i32,
      proto::ScrollDeliveryCandidate::ForegroundHid as i32,
    ],
  ] {
    let error = scroll_options_from_proto(Some(proto::ScrollOptions {
      delivery_candidates: candidates,
      ..Default::default()
    }))
    .unwrap_err();
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
  }
}

#[test]
fn scroll_motion_rpc_decodes_timing_and_rejects_invalid_plans_before_delivery() {
  let motion = scroll_motion_from_proto(Some(proto::ScrollMotion {
    total: Some(proto::Scroll {
      delta_x: 0.0,
      delta_y: 600.0,
    }),
    timing: Some(proto::scroll_motion::Timing::FixedDuration(proto::FixedDurationMotionTiming {
      duration: Some(prost_types::Duration {
        seconds: 0,
        nanos: 400_000_000,
      }),
      function: Some(proto::MotionTimingFunction {
        function: Some(proto::motion_timing_function::Function::CubicBezier(proto::CubicBezierMotionTimingFunction {
          x1: 0.25,
          y1: 0.1,
          x2: 0.25,
          y2: 1.0,
        })),
      }),
    })),
    sample_rate_hz: 120,
  }))
  .unwrap();
  assert_eq!(motion.total, auv_driver::Scroll::new(0.0, 600.0));
  assert_eq!(motion.sample_rate_hz, 120);
  assert_eq!(
    motion.timing,
    auv_driver::MotionTiming::FixedDuration {
      duration: std::time::Duration::from_millis(400),
      function: auv_driver::TimingFunction::CubicBezier {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
      },
    }
  );

  // An absent function selects linear timing.
  let linear = scroll_motion_from_proto(Some(proto::ScrollMotion {
    total: Some(proto::Scroll {
      delta_x: 10.0,
      delta_y: 0.0,
    }),
    timing: Some(proto::scroll_motion::Timing::FixedDuration(Default::default())),
    sample_rate_hz: 0,
  }))
  .unwrap();
  assert!(matches!(
    linear.timing,
    auv_driver::MotionTiming::FixedDuration {
      function: auv_driver::TimingFunction::Linear,
      ..
    }
  ));

  let invalid_plans = [
    None,
    Some(proto::ScrollMotion {
      total: Some(proto::Scroll {
        delta_x: 0.0,
        delta_y: 10.0,
      }),
      timing: None,
      sample_rate_hz: 60,
    }),
    Some(proto::ScrollMotion {
      total: Some(proto::Scroll::default()),
      timing: Some(proto::scroll_motion::Timing::FixedDuration(Default::default())),
      sample_rate_hz: 60,
    }),
    Some(proto::ScrollMotion {
      total: Some(proto::Scroll {
        delta_x: 0.0,
        delta_y: 10.0,
      }),
      timing: Some(proto::scroll_motion::Timing::FixedDuration(proto::FixedDurationMotionTiming {
        duration: None,
        function: Some(proto::MotionTimingFunction {
          function: Some(proto::motion_timing_function::Function::Standard(proto::StandardMotionTimingFunction::Unspecified as i32)),
        }),
      })),
      sample_rate_hz: 60,
    }),
    Some(proto::ScrollMotion {
      total: Some(proto::Scroll {
        delta_x: 0.0,
        delta_y: 10.0,
      }),
      timing: Some(proto::scroll_motion::Timing::FixedDuration(proto::FixedDurationMotionTiming {
        duration: None,
        function: Some(proto::MotionTimingFunction {
          function: Some(proto::motion_timing_function::Function::CubicBezier(proto::CubicBezierMotionTimingFunction {
            x1: 1.5,
            y1: 0.0,
            x2: 0.5,
            y2: 1.0,
          })),
        }),
      })),
      sample_rate_hz: 60,
    }),
  ];
  for plan in invalid_plans {
    assert_eq!(scroll_motion_from_proto(plan).unwrap_err().code(), tonic::Code::InvalidArgument);
  }
}

#[test]
fn scroll_until_rpc_decodes_step_condition_and_region() {
  let request = scroll_until_request_from_proto(proto::ScrollUntilBegin {
    step: Some(proto::scroll_until_begin::Step::Instant(proto::Scroll {
      delta_x: 0.0,
      delta_y: 600.0,
    })),
    condition: Some(proto::scroll_until_begin::Condition::TextVisible(proto::ScrollUntilTextVisible {
      query: "Load more".to_string(),
    })),
    max_steps: 40,
    settle: Some(prost_types::Duration {
      seconds: 0,
      nanos: 400_000_000,
    }),
    no_motion_confirmations: 2,
    motion_region: Some(auv_api_proto::auv::api::image::v1::NormalizedRect {
      x: 0.0,
      y: 0.1,
      width: 1.0,
      height: 0.8,
    }),
    ..Default::default()
  })
  .unwrap();
  assert_eq!(
    request.step,
    auv_scan::ScrollUntilStep::Instant {
      delta: auv_driver::Scroll::new(0.0, 600.0)
    }
  );
  assert_eq!(
    request.condition,
    auv_scan::ScrollUntilCondition::TextVisible {
      query: "Load more".to_string()
    }
  );
  assert_eq!(request.settle, std::time::Duration::from_millis(400));
  assert_eq!(request.motion_region, Some(auv_driver::RatioRect::new(0.0, 0.1, 1.0, 0.8)));
  assert_eq!(request.output, auv_scan::ScrollUntilOutputOptions { text: true }, "payloads are opt-out");
  assert!(request.validate().is_ok());

  let opted_out = scroll_until_request_from_proto(proto::ScrollUntilBegin {
    step: Some(proto::scroll_until_begin::Step::Instant(proto::Scroll {
      delta_x: 0.0,
      delta_y: 10.0,
    })),
    condition: Some(proto::scroll_until_begin::Condition::End(proto::ScrollUntilEnd {})),
    output: Some(proto::ScrollUntilOutputOptions { omit_text: true }),
    ..Default::default()
  })
  .unwrap();
  assert_eq!(opted_out.output, auv_scan::ScrollUntilOutputOptions { text: false });

  for malformed in [
    proto::ScrollUntilBegin {
      condition: Some(proto::scroll_until_begin::Condition::End(proto::ScrollUntilEnd {})),
      ..Default::default()
    },
    proto::ScrollUntilBegin {
      step: Some(proto::scroll_until_begin::Step::Instant(proto::Scroll {
        delta_x: 0.0,
        delta_y: 10.0,
      })),
      ..Default::default()
    },
    proto::ScrollUntilBegin {
      step: Some(proto::scroll_until_begin::Step::Instant(proto::Scroll {
        delta_x: 0.0,
        delta_y: 10.0,
      })),
      condition: Some(proto::scroll_until_begin::Condition::End(proto::ScrollUntilEnd {})),
      motion_region: Some(auv_api_proto::auv::api::image::v1::NormalizedRect {
        x: 0.5,
        y: 0.0,
        width: 0.6,
        height: 1.0,
      }),
      ..Default::default()
    },
  ] {
    assert_eq!(scroll_until_request_from_proto(malformed).unwrap_err().code(), tonic::Code::InvalidArgument);
  }
}

#[test]
fn scroll_until_update_carries_capture_ref_text_and_stop_reason() {
  let captures = test_capture_store();
  let update = |capture_bytes: u32, text, stop| auv_scan::ScrollUntilUpdate {
    steps: 3,
    delivered: auv_driver::Scroll::new(0.0, 1500.0),
    motion: Some(auv_scan::ViewportPixelMotion {
      estimated_shift: 0,
      normalized_diff: 0.0,
      no_motion: true,
    }),
    no_motion_streak: 2,
    capture: auv_driver::Capture {
      origin: None,
      image: image::RgbaImage::new(capture_bytes, 1),
      bounds: auv_driver::Rect::new(10.0, 20.0, 2.0, 1.0),
      scale_factor: 1.0,
      backend: "test".to_string(),
      fallback_reason: None,
    },
    text,
    stop,
  };
  let proto = scroll_until_update_to_proto(
    update(
      2,
      Some(auv_driver::TextRecognition {
        origin: None,
        text: "END OF FEED".to_string(),
        regions: Vec::new(),
      }),
      Some(auv_scan::ScrollUntilStopReason::EndByNoVisualProgress),
    ),
    false,
    &captures,
  );
  assert_eq!((proto.steps, proto.no_motion_streak), (3, 2));
  assert_eq!(proto.stop, proto::ScrollUntilStopReason::EndByNoVisualProgress as i32);
  let capture = proto.capture.expect("capture");
  let reference = capture.r#ref.expect("capture ref").capture_id;
  assert_eq!(captures.get(&reference).map(|stored| stored.image.dimensions()), Some((2, 1)));
  assert_eq!(proto.text.map(|text| text.text).as_deref(), Some("END OF FEED"));
  assert!(!proto.awaiting_decision);

  let proto = scroll_until_update_to_proto(update(1, None, None), true, &captures);
  assert_eq!(proto.stop, proto::ScrollUntilStopReason::Unspecified as i32);
  assert!(proto.awaiting_decision && proto.text.is_none());
}

fn gradient_capture(width: u32, height: u32) -> auv_driver::Capture {
  auv_driver::Capture {
    origin: None,
    image: image::RgbaImage::from_fn(width, height, |x, y| image::Rgba([x as u8, y as u8, 0, 255])),
    bounds: auv_driver::Rect::new(0.0, 0.0, f64::from(width) / 2.0, f64::from(height) / 2.0),
    scale_factor: 2.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  }
}

#[test]
fn capture_image_crops_outward_and_fits_inside_max_size() {
  use auv_api_proto::auv::api::image::v1 as image_proto;
  let capture = gradient_capture(10, 4);
  // x 0.25..0.55 of 10 px covers pixels 2.5..5.5, rounded outward to 2..6.
  let region = auv_driver::RatioRect::new(0.25, 0.5, 0.3, 0.5);
  let response = capture_image_to_proto(&capture, region, None, image_proto::ImageEncoding::Rgba).unwrap();
  let frame = response.image.expect("image");
  assert_eq!(frame.encoding, image_proto::ImageEncoding::Rgba as i32, "RGBA is an encoding like the others");
  assert_eq!((frame.width, frame.height), (4, 2));
  assert_eq!(&frame.data[..4], &[2, 2, 0, 255], "the crop starts at the outward-rounded pixel");

  let full = auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let bounded = capture_image_to_proto(
    &capture,
    full,
    Some(image_proto::PixelSize {
      width: 5,
      height: 5,
    }),
    image_proto::ImageEncoding::Png,
  )
  .unwrap();
  let png = bounded.image.expect("image");
  assert_eq!((png.encoding, png.width, png.height), (image_proto::ImageEncoding::Png as i32, 5, 2));
  assert_eq!(image::load_from_memory(&png.data).unwrap().to_rgba8().dimensions(), (5, 2));

  let enlarged = capture_image_to_proto(
    &capture,
    full,
    Some(image_proto::PixelSize {
      width: 100,
      height: 100,
    }),
    image_proto::ImageEncoding::Jpeg,
  )
  .unwrap();
  let jpeg = enlarged.image.expect("image");
  assert_eq!((jpeg.width, jpeg.height), (10, 4), "max_size never enlarges");
  assert_eq!(image::load_from_memory(&jpeg.data).unwrap().to_rgb8().dimensions(), (10, 4));
}

#[test]
fn capture_image_webp_is_lossless() {
  use auv_api_proto::auv::api::image::v1 as image_proto;
  let capture = gradient_capture(10, 4);
  let full = auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let response = capture_image_to_proto(&capture, full, None, image_proto::ImageEncoding::Webp).unwrap();
  let webp = response.image.expect("image");
  assert_eq!((webp.encoding, webp.width, webp.height), (image_proto::ImageEncoding::Webp as i32, 10, 4));
  assert_eq!(image::load_from_memory(&webp.data).unwrap().to_rgba8(), capture.image, "WebP keeps every pixel");
}

#[test]
fn unknown_capture_reference_is_not_found_with_a_recapture_hint() {
  let captures = test_capture_store();
  let stored = captures.insert(gradient_capture(2, 2));
  assert!(stored_capture(&captures, proto::CaptureRef { capture_id: stored }).is_ok());

  let missing = stored_capture(
    &captures,
    proto::CaptureRef {
      capture_id: "cap-0-404".into(),
    },
  )
  .unwrap_err();
  assert_eq!(missing.code(), tonic::Code::NotFound);
  assert!(missing.message().contains("capture again"), "{}", missing.message());
  let empty = stored_capture(&captures, proto::CaptureRef::default()).unwrap_err();
  assert_eq!(empty.code(), tonic::Code::InvalidArgument);
}

#[test]
fn malformed_position_is_an_invalid_argument() {
  let error = position_from_proto(proto::Position::default()).unwrap_err();
  assert_eq!(error.code(), tonic::Code::InvalidArgument);
  assert_eq!(error.message(), auv::protocol::position::DecodeError::InvalidCoordinateSpace.to_string());
}

#[tokio::test]
async fn slow_feedback_does_not_block_native_motion_or_completion() {
  let (native_done_tx, native_done_rx) = tokio::sync::oneshot::channel();
  let allow_feedback = std::sync::Arc::new(tokio::sync::Notify::new());
  let gate = allow_feedback.clone();
  let completed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
  let observed = completed.clone();
  let relay = tokio::spawn(run_mouse_motion(
    move |mut notify| {
      let point = auv_driver::Point::new(1.0, 2.0);
      assert!(notify(auv_driver::mouse_input::MotionEvent::Started {
        point,
        samples: 10_000,
        duration: std::time::Duration::ZERO
      }));
      for index in 0..10_000 {
        assert!(notify(auv_driver::mouse_input::MotionEvent::Progress {
          index,
          sample: auv_driver::MouseMotionSample {
            point,
            elapsed: std::time::Duration::ZERO
          }
        }));
      }
      let _ = native_done_tx.send(());
      Ok((point, auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::Noop)))
    },
    move |event| {
      let gate = gate.clone();
      let observed = observed.clone();
      async move {
        match event.unwrap() {
          MouseMotionEvent::Started(_) => gate.notified().await,
          MouseMotionEvent::Completed(_) => observed.store(true, std::sync::atomic::Ordering::Release),
          _ => {}
        }
        Ok(())
      }
    },
  ));
  tokio::time::timeout(std::time::Duration::from_secs(5), native_done_rx).await.unwrap().unwrap();
  assert!(!relay.is_finished());
  allow_feedback.notify_one();
  relay.await.unwrap();
  assert!(completed.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn dropped_feedback_relay_wakes_and_releases_native_hold() {
  struct Receiver {
    pressed: tokio::sync::Notify,
    released: tokio::sync::Notify,
  }
  impl auv_driver::mouse_input::MouseBackend for Receiver {
    fn move_to(&self, _: auv_driver::Point, _: Option<auv_driver::MouseButton>) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
      Ok(auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::Noop))
    }
    fn button(
      &self,
      _: auv_driver::Point,
      _: auv_driver::MouseButton,
      down: bool,
    ) -> auv_driver::DriverResult<auv_driver::InputActionResult> {
      if down {
        self.pressed.notify_one();
      } else {
        self.released.notify_one();
      }
      Ok(auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::Noop))
    }
  }
  let receiver = std::sync::Arc::new(Receiver {
    pressed: tokio::sync::Notify::new(),
    released: tokio::sync::Notify::new(),
  });
  let backend = receiver.clone();
  let relay = tokio::spawn(run_mouse_motion(
    move |mut notify| {
      let coordinator = std::sync::Arc::new(auv_driver::mouse_input::MouseCoordinator::default());
      let point = auv_driver::Point::new(1.0, 2.0);
      notify(auv_driver::mouse_input::MotionEvent::Started {
        point,
        samples: 1,
        duration: std::time::Duration::from_secs(3600),
      });
      coordinator.hold(0, point, auv_driver::MouseButton::Left, std::time::Duration::from_secs(3600), backend).map(|action| (point, action))
    },
    |_| std::future::pending::<Result<(), ()>>(),
  ));
  tokio::time::timeout(std::time::Duration::from_secs(5), receiver.pressed.notified()).await.unwrap();
  relay.abort();
  assert!(relay.await.unwrap_err().is_cancelled());
  tokio::time::timeout(std::time::Duration::from_secs(5), receiver.released.notified()).await.unwrap();
}

#[test]
fn screen_regions_map_into_the_image_and_clip_to_it() {
  // A window capture at (100, 200), 400x300 points.
  let bounds = auv_driver::Rect::new(100.0, 200.0, 400.0, 300.0);
  let screen = |x, y, width, height| {
    Some(proto::ScreenRect {
      x,
      y,
      width,
      height,
    })
  };

  assert_eq!(
    image_region_from_proto(None, screen(200.0, 260.0, 100.0, 150.0), bounds).unwrap(),
    auv_driver::RatioRect::new(0.25, 0.2, 0.25, 0.5)
  );
  // Clipped to the image: only the overlapping right half remains.
  assert_eq!(
    image_region_from_proto(None, screen(400.0, 200.0, 400.0, 300.0), bounds).unwrap(),
    auv_driver::RatioRect::new(0.75, 0.0, 0.25, 1.0)
  );
  assert_eq!(image_region_from_proto(None, None, bounds).unwrap(), auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0));

  let outside = image_region_from_proto(None, screen(0.0, 0.0, 50.0, 50.0), bounds).unwrap_err();
  assert_eq!(outside.code(), tonic::Code::InvalidArgument);
  let both = image_region_from_proto(
    Some(auv_api_proto::auv::api::image::v1::NormalizedRect {
      x: 0.0,
      y: 0.0,
      width: 1.0,
      height: 1.0,
    }),
    screen(100.0, 200.0, 10.0, 10.0),
    bounds,
  )
  .unwrap_err();
  assert!(both.message().contains("exclusive"), "{}", both.message());
}

#[tokio::test]
async fn fetched_images_are_cached_on_the_capture_except_raw_pixels() {
  use auv_api_proto::auv::api::image::v1 as image_proto;
  let captures = test_capture_store();
  let id = captures.insert(gradient_capture(10, 4));
  let service = LocalCaptureService {
    session: auv_driver::open_local().unwrap(),
    captures: captures.clone(),
  };
  let request = |encoding: image_proto::ImageEncoding, max_size: Option<image_proto::PixelSize>| {
    Request::new(proto::GetCaptureImageRequest {
      capture: Some(proto::CaptureRef {
        capture_id: id.clone(),
      }),
      region: None,
      screen_region: None,
      max_size,
      encoding: encoding as i32,
    })
  };

  let thumbnail = service
    .get_capture_image(request(
      image_proto::ImageEncoding::Jpeg,
      Some(image_proto::PixelSize {
        width: 5,
        height: 5,
      }),
    ))
    .await
    .unwrap()
    .into_inner();
  let cached = captures
    .image(&id, &ImageKey::new(RegionKey::new(None, None), Some((5, 5)), image_proto::ImageEncoding::Jpeg as i32))
    .expect("cached JPEG");
  assert_eq!(Some(cached.as_ref()), thumbnail.image.as_ref());

  service.get_capture_image(request(image_proto::ImageEncoding::Rgba, None)).await.unwrap();
  assert!(
    captures.image(&id, &ImageKey::new(RegionKey::new(None, None), None, image_proto::ImageEncoding::Rgba as i32)).is_none(),
    "raw pixels are not cached a second time"
  );
}
