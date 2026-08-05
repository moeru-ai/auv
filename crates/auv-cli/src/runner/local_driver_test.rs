use super::*;

#[test]
fn overdue_mouse_samples_coalesce_but_keep_the_final_sample() {
  let samples = [
    auv_driver::MouseMotionSample {
      point: auv_driver::Point::new(0.0, 0.0),
      elapsed: std::time::Duration::ZERO,
    },
    auv_driver::MouseMotionSample {
      point: auv_driver::Point::new(1.0, 1.0),
      elapsed: std::time::Duration::from_millis(8),
    },
    auv_driver::MouseMotionSample {
      point: auv_driver::Point::new(2.0, 2.0),
      elapsed: std::time::Duration::from_millis(16),
    },
  ];

  assert_eq!(latest_due_mouse_sample(&samples, 0, std::time::Duration::from_millis(12)), 1);
  assert_eq!(latest_due_mouse_sample(&samples, 2, std::time::Duration::from_secs(1)), 2);
}

#[tokio::test]
async fn streamed_mouse_motion_rejects_cancel_before_begin() {
  let mut requests = tokio_stream::iter([Ok(proto::StreamMouseMotionRequest {
    event: Some(proto::stream_mouse_motion_request::Event::Cancel(proto::StreamMouseMotionCancel {})),
  })]);
  let (sender, _receiver) = tokio::sync::mpsc::channel(1);

  let status = collect_mouse_motion(&mut requests, &sender).await.expect_err("cancel must follow begin");

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
      reason: "observation unavailable".to_string(),
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
fn captured_rgba_frame_preserves_alpha_and_screen_bounds() {
  let capture = auv_driver::Capture {
    image: image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("valid RGBA fixture"),
    bounds: auv_driver::Rect::new(10.0, 20.0, 1.0, 0.5),
    scale_factor: 2.0,
    backend: "fixture".to_string(),
    fallback_reason: Some("fallback".to_string()),
  };

  let frame = capture_to_proto(capture);

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
fn text_recognition_capture_rejects_malformed_rgba_before_ocr() {
  let error = capture_from_proto(proto::CapturedFrame {
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
    text: "hello".to_string(),
    regions: vec![auv_driver::RecognizedText {
      text: "hello".to_string(),
      bounds: auv_driver::Rect::new(10.0, 20.0, 30.0, 40.0),
      confidence: Some(0.75),
    }],
  });
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

  let point_error = window_point_from_proto(proto::WindowPoint {
    x: f64::NAN,
    y: 0.0,
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
