use super::*;

#[derive(Debug)]
struct LargeCaptureService;

const LARGE_CAPTURE_ID: &str = "cap-test-1";

fn large_capture_metadata() -> proto::CapturedFrame {
  proto::CapturedFrame {
    r#ref: Some(proto::CaptureRef {
      capture_id: LARGE_CAPTURE_ID.to_string(),
    }),
    origin: None,
    pixel_size: Some(auv_api_proto::auv::api::image::v1::PixelSize {
      width: 1280,
      height: 1024,
    }),
    bounds: Some(proto::ScreenRect {
      width: 1280.0,
      height: 1024.0,
      ..Default::default()
    }),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  }
}

#[tonic::async_trait]
impl proto::capture_service_server::CaptureService for LargeCaptureService {
  async fn capture_window(
    &self,
    _request: tonic::Request<proto::CaptureWindowRequest>,
  ) -> Result<tonic::Response<proto::CaptureWindowResponse>, tonic::Status> {
    Err(tonic::Status::unimplemented("not used by this regression"))
  }

  async fn capture_display(
    &self,
    _request: tonic::Request<proto::CaptureDisplayRequest>,
  ) -> Result<tonic::Response<proto::CaptureDisplayResponse>, tonic::Status> {
    Ok(tonic::Response::new(proto::CaptureDisplayResponse {
      display: Some(proto::Display {
        display_id: "primary".to_string(),
        frame: Some(proto::ScreenRect {
          width: 1280.0,
          height: 1024.0,
          ..Default::default()
        }),
        ..Default::default()
      }),
      capture: Some(large_capture_metadata()),
    }))
  }

  async fn capture_region(
    &self,
    _request: tonic::Request<proto::CaptureRegionRequest>,
  ) -> Result<tonic::Response<proto::CaptureRegionResponse>, tonic::Status> {
    Err(tonic::Status::unimplemented("not used by this regression"))
  }

  async fn get_capture_image(
    &self,
    request: tonic::Request<proto::GetCaptureImageRequest>,
  ) -> Result<tonic::Response<proto::GetCaptureImageResponse>, tonic::Status> {
    let request = request.into_inner();
    if request.capture.map(|capture| capture.capture_id).as_deref() != Some(LARGE_CAPTURE_ID) {
      return Err(tonic::Status::not_found("capture was not found"));
    }
    Ok(tonic::Response::new(proto::GetCaptureImageResponse {
      image: Some(auv_api_proto::auv::api::image::v1::EncodedImage {
        encoding: auv_api_proto::auv::api::image::v1::ImageEncoding::Rgba as i32,
        width: 1280,
        height: 1024,
        data: vec![0; 1280 * 1024 * 4],
      }),
    }))
  }
}

fn disconnected_client() -> GrpcClient {
  let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:9").connect_lazy();
  GrpcClient::from_channel(channel)
}

fn route() -> auv_api_client::RunnerRoute {
  auv_api_client::RunnerRoute {
    device_id: Some("device_test".to_string()),
    run_id: Some("run_test".to_string()),
    runner_class: "auv.core.local".to_string(),
  }
}

#[tokio::test]
async fn runner_hierarchy_rejects_an_empty_class_before_any_transport_call() {
  let error = RunnerClient::new(
    disconnected_client(),
    auv_api_client::RunnerRoute {
      runner_class: String::new(),
      device_id: None,
      run_id: None,
    },
  )
  .expect_err("empty RunnerClass must fail");
  assert!(matches!(error, CapabilityError::InvalidArgument(_)));
}

async fn serve_large_capture_fixture() -> (RunnerClient, tokio::task::JoinHandle<Result<(), tonic::transport::Error>>) {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind capture fixture");
  let address = listener.local_addr().expect("capture fixture address");
  drop(listener);
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(
        proto::capture_service_server::CaptureServiceServer::new(LargeCaptureService)
          .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT),
      )
      .serve(address)
      .await
  });
  tokio::task::yield_now().await;
  let grpc = GrpcClient::connect(format!("http://{address}").parse().expect("fixture URI")).await.expect("connect capture fixture");
  (RunnerClient::new(grpc, route()).expect("runner client"), server)
}

#[tokio::test]
async fn capture_returns_a_reference_and_metadata_without_pixels() {
  let (runner, server) = serve_large_capture_fixture().await;
  let response = runner.displays().capture(None).await.expect("capture metadata");
  assert_eq!(response.capture.reference, CaptureRef::new(LARGE_CAPTURE_ID));
  assert_eq!(response.capture.pixel_size, auv_driver::PixelSize::new(1280, 1024));
  assert_eq!(response.capture.bounds, auv_driver::Rect::new(0.0, 0.0, 1280.0, 1024.0));
  server.abort();
}

#[tokio::test]
async fn capture_pixels_accept_desktop_frames_larger_than_tonic_default() {
  // ROOT CAUSE:
  //
  // If a desktop capture exceeded tonic's 4 MiB decoded-message default, the
  // routed client rejected the valid frame before the extension could inspect
  // it. Runner image clients must share the server's image-message policy.
  let (runner, server) = serve_large_capture_fixture().await;
  let capture = runner.displays().capture(None).await.expect("capture metadata").capture;
  let pixels = runner.captures().pixels(&capture).await.expect("decode capture larger than 4 MiB");
  assert!(pixels.image.as_raw().len() > 4 * 1024 * 1024);
  assert_eq!(pixels.bounds, capture.bounds);
  assert_eq!(pixels.backend, "fixture");

  let missing = runner
    .captures()
    .image(&CaptureRef::new("cap-evicted"), CaptureImageOptions::default())
    .await
    .expect_err("an unknown capture must fail");
  assert_eq!(missing.client_kind(), Some(crate::error::ClientErrorKind::NotFound));
  server.abort();
}

#[tokio::test]
async fn resolved_window_child_retains_the_exact_resource_reference() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let child = WindowClient {
    runner,
    window: auv_driver::Window {
      reference: auv_driver::WindowRef {
        id: "window_test".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: None,
      frame: auv_driver::Rect::new(0.0, 0.0, 100.0, 100.0),
      coordinate_space: auv_driver::CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    },
    window_ref: proto::WindowRef {
      window_id: "window_test".to_string(),
    },
  };
  assert_eq!(child.reference().id, "window_test");
  assert_eq!(&child.resource().reference, child.reference());
}

#[tokio::test]
async fn runner_input_exposes_typed_screen_point_click() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let input = runner.input();
  let call = input.click_screen_point(
    auv_driver::Point::new(10.0, 20.0),
    auv_driver::MouseButton::Left,
    auv_driver::Click::Single,
    Default::default(),
  );
  drop(call);
}

#[tokio::test]
async fn runner_input_exposes_typed_mouse_motion() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let input = runner.input();
  let call = input.move_mouse(auv_driver::MoveMouseRequest::direct(auv_driver::Point::new(10.0, 20.0)));
  drop(call);

  let plan = auv_driver::MoveMouseRequest::direct(auv_driver::Point::new(10.0, 20.0));
  let streaming_call = input.stream_mouse_motion(&plan);
  drop(streaming_call);
}

#[test]
fn input_action_projection_preserves_typed_delivery_evidence() {
  let action = input_action_result_from_proto(proto::InputActionResult {
    selected_path: proto::InputDeliveryPath::ForegroundSystemEvents as i32,
    attempts: vec![proto::InputAttempt {
      path: proto::InputDeliveryPath::ForegroundSystemEvents as i32,
      succeeded: true,
      message: None,
    }],
    mouse_disturbance: proto::DisturbanceLevel::None as i32,
    focus_disturbance: proto::DisturbanceLevel::Foreground as i32,
    clipboard_disturbance: proto::DisturbanceLevel::None as i32,
  })
  .expect("valid protobuf delivery evidence");

  assert_eq!(action.selected_path, auv_driver::InputDeliveryPath::ForegroundSystemEvents);
  assert_eq!(
    action.attempts,
    vec![auv_driver::InputAttempt::success(
      auv_driver::InputDeliveryPath::ForegroundSystemEvents
    )]
  );
  assert_eq!(action.focus_disturbance, auv_driver::DisturbanceLevel::Foreground);
}

#[test]
fn input_action_projection_rejects_unspecified_wire_enums() {
  let error = input_action_result_from_proto(proto::InputActionResult {
    selected_path: proto::InputDeliveryPath::Unspecified as i32,
    ..Default::default()
  })
  .expect_err("unspecified path must not become canonical driver evidence");
  assert!(matches!(error, CapabilityError::InvalidResponse(_)));
}

#[test]
fn runner_capture_projection_requires_a_reference_and_keeps_screen_contract() {
  let mut frame = large_capture_metadata();
  frame.bounds = Some(proto::ScreenRect {
    x: -10.0,
    y: 4.0,
    width: 2.0,
    height: 1.0,
  });
  let capture = capture::runner_capture_from_proto(frame.clone()).expect("valid capture metadata");
  assert_eq!(capture.reference.id(), LARGE_CAPTURE_ID);
  assert_eq!(capture.bounds, auv_driver::Rect::new(-10.0, 4.0, 2.0, 1.0));
  assert_eq!(capture.backend, "fixture");

  frame.r#ref = None;
  let error = capture::runner_capture_from_proto(frame).expect_err("a Runner capture without a reference is malformed");
  assert!(matches!(error, CapabilityError::InvalidResponse(_)));
}

#[test]
fn recognition_source_sends_references_without_pixels() {
  let frame = image_frame_to_proto(auv_driver::Capture {
    origin: None,
    image: image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("valid RGBA fixture"),
    bounds: auv_driver::Rect::new(0.0, 0.0, 2.0, 1.0),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  });
  assert_eq!(frame.image.map(|image| image.data.len()), Some(8), "a caller-owned image carries its pixels");
  assert!(matches!(
    RecognitionSource::from(CaptureRef::new("cap-1")),
    RecognitionSource::Capture(reference) if reference.id() == "cap-1"
  ));
}

#[test]
fn capture_image_converts_to_rgba_only_when_it_is_rgba() {
  let rgba = CaptureImage {
    encoding: CaptureImageEncoding::Rgba,
    size: auv_driver::PixelSize::new(2, 1),
    data: vec![1, 2, 3, 4, 5, 6, 7, 8],
  };
  assert_eq!(rgba.clone().into_rgba_image().expect("RGBA rows").dimensions(), (2, 1));

  let png = CaptureImage {
    encoding: CaptureImageEncoding::Png,
    ..rgba.clone()
  };
  assert!(matches!(png.into_rgba_image(), Err(CapabilityError::InvalidResponse(_))));

  let truncated = CaptureImage {
    data: vec![0; 7],
    ..rgba
  };
  assert!(matches!(truncated.into_rgba_image(), Err(CapabilityError::InvalidResponse(_))));
}

#[test]
fn permission_mapper_preserves_explicit_statuses() {
  let probe = permission_probe_from_proto(macos_proto::ProbePermissionsResponse {
    screen_recording: macos_proto::PermissionStatus::Granted as i32,
    screen_capture_kit: macos_proto::PermissionStatus::Missing as i32,
    accessibility: macos_proto::PermissionStatus::Unknown as i32,
    automation_to_system_events: macos_proto::PermissionStatus::Granted as i32,
  })
  .expect("valid permission projection");
  assert_eq!(probe.screen_recording, auv_driver::PermissionStatus::Granted);
  assert_eq!(probe.screen_capture_kit, auv_driver::PermissionStatus::Missing);
  assert_eq!(probe.accessibility, auv_driver::PermissionStatus::Unknown);
  assert_eq!(probe.automation_to_system_events, auv_driver::PermissionStatus::Granted);
}

#[test]
fn permission_mapper_rejects_unspecified_and_unknown_wire_values() {
  for value in [macos_proto::PermissionStatus::Unspecified as i32, 99] {
    let error = permission_probe_from_proto(macos_proto::ProbePermissionsResponse {
      screen_recording: value,
      screen_capture_kit: macos_proto::PermissionStatus::Unknown as i32,
      accessibility: macos_proto::PermissionStatus::Unknown as i32,
      automation_to_system_events: macos_proto::PermissionStatus::Unknown as i32,
    })
    .expect_err("invalid wire status must not silently become Unknown");
    assert!(matches!(error, CapabilityError::InvalidResponse(_)));
  }
}

#[test]
fn accessibility_mapper_preserves_ax_identity_and_delivery_evidence() {
  let result = ax_focus_result_from_proto(macos_proto::FocusTextResponse {
    result: Some(macos_proto::AxFocusResult {
      app: "com.example.Editor".to_string(),
      pid: 42,
      path: "root/AXTextArea[0]".to_string(),
      role: "AXTextArea".to_string(),
      title: "Document".to_string(),
      value: "draft".to_string(),
      // Exact-path selection intentionally has no query in the owner result.
      query: String::new(),
      action: Some(proto::InputActionResult {
        selected_path: proto::InputDeliveryPath::AxFocus as i32,
        attempts: vec![proto::InputAttempt {
          path: proto::InputDeliveryPath::AxFocus as i32,
          succeeded: true,
          message: None,
        }],
        mouse_disturbance: proto::DisturbanceLevel::None as i32,
        focus_disturbance: proto::DisturbanceLevel::Temporary as i32,
        clipboard_disturbance: proto::DisturbanceLevel::None as i32,
      }),
    }),
  })
  .expect("valid AX focus projection");

  assert_eq!(result.path, "root/AXTextArea[0]");
  assert!(result.query.is_empty());
  assert_eq!(result.input_action_result.selected_path, auv_driver::InputDeliveryPath::AxFocus);
}

#[test]
fn accessibility_mapper_rejects_missing_result_before_rendering() {
  let error = ax_focus_result_from_proto(macos_proto::FocusTextResponse::default()).expect_err("missing focus result");
  assert!(matches!(error, CapabilityError::InvalidResponse(_)));
}

#[test]
fn application_activation_mapper_preserves_typed_verification() {
  use macos_proto::application_activation_verification::Verification;

  let result = activation_result_from_proto(macos_proto::ActivateBundleIdResponse {
    requested_bundle_id: "com.example.Requested".to_string(),
    verification: Some(macos_proto::ApplicationActivationVerification {
      verification: Some(Verification::ForegroundMismatch(macos_proto::ForegroundMismatch {
        observed_bundle_id: "com.example.Other".to_string(),
      })),
    }),
  })
  .expect("typed activation result");
  assert_eq!(result.requested_bundle_id, "com.example.Requested");
  assert_eq!(
    result.verification,
    auv_driver::ApplicationActivationVerification::ForegroundMismatch {
      observed_bundle_id: "com.example.Other".to_string(),
    }
  );
}

#[test]
fn application_activation_mapper_rejects_missing_or_empty_evidence() {
  let missing = activation_result_from_proto(macos_proto::ActivateBundleIdResponse {
    requested_bundle_id: "com.example.Requested".to_string(),
    verification: None,
  })
  .expect_err("missing verification must fail closed");
  assert!(matches!(missing, CapabilityError::InvalidResponse(_)));

  let empty = activation_result_from_proto(macos_proto::ActivateBundleIdResponse {
    requested_bundle_id: "com.example.Requested".to_string(),
    verification: Some(macos_proto::ApplicationActivationVerification {
      verification: Some(macos_proto::application_activation_verification::Verification::Unavailable(
        macos_proto::VerificationUnavailable::default(),
      )),
    }),
  })
  .expect_err("empty reason must fail closed");
  assert!(matches!(empty, CapabilityError::InvalidResponse(_)));
}

#[tokio::test]
async fn runner_exposes_hierarchical_macos_permission_client() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let permissions = runner.macos().permissions();
  let call = permissions.probe();
  drop(call);
}

#[test]
fn now_playing_mapper_preserves_exact_owner_state() {
  let state = now_playing_from_proto(macos_proto::GetNowPlayingResponse {
    state: Some(macos_proto::NowPlayingState {
      present: true,
      is_playing: false,
      source_bundle_id: Some("com.apple.Music".to_string()),
      title: Some("Current Song".to_string()),
      artist: None,
      album: Some("Album".to_string()),
      duration_seconds: Some(245.5),
      elapsed_seconds: Some(61.25),
      playback_rate: Some(0.0),
      content_item_id: Some("track-42".to_string()),
      supports_like: None,
      is_liked: Some(false),
    }),
  })
  .expect("valid wire state");
  assert!(state.present);
  assert!(!state.is_playing);
  assert_eq!(state.source_bundle_id.as_deref(), Some("com.apple.Music"));
  assert_eq!(state.title.as_deref(), Some("Current Song"));
  assert_eq!(state.artist, None);
  assert_eq!(state.album.as_deref(), Some("Album"));
  assert_eq!(state.duration_seconds, Some(245.5));
  assert_eq!(state.elapsed_seconds, Some(61.25));
  assert_eq!(state.playback_rate, Some(0.0));
  assert_eq!(state.content_item_id.as_deref(), Some("track-42"));
  assert_eq!(state.supports_like, None);
  assert_eq!(state.is_liked, Some(false));
}

#[test]
fn now_playing_mapper_rejects_missing_or_non_finite_wire_state() {
  let missing = now_playing_from_proto(macos_proto::GetNowPlayingResponse::default()).expect_err("state is required");
  assert!(matches!(missing, CapabilityError::InvalidResponse(_)));
  let invalid = now_playing_from_proto(macos_proto::GetNowPlayingResponse {
    state: Some(macos_proto::NowPlayingState {
      duration_seconds: Some(f64::NAN),
      ..Default::default()
    }),
  })
  .expect_err("non-finite wire value must fail closed");
  assert!(matches!(invalid, CapabilityError::InvalidResponse(_)));
}

#[test]
fn media_control_mapper_preserves_owner_outcome_and_method_identity() {
  let state = macos_proto::NowPlayingState {
    present: true,
    is_playing: true,
    title: Some("Song".to_string()),
    playback_rate: Some(1.0),
    ..Default::default()
  };
  let outcome = media_control_outcome_from_proto(
    Some(macos_proto::MediaControlOutcome {
      before: Some(macos_proto::NowPlayingState {
        is_playing: false,
        playback_rate: Some(0.0),
        ..state.clone()
      }),
      after: Some(state),
      verified: true,
    }),
    "play",
  )
  .expect("valid outcome");
  assert_eq!(outcome.command, "play");
  assert!(!outcome.before.is_playing);
  assert!(outcome.after.is_playing);
  assert!(outcome.verified);
}

#[test]
fn media_control_mapper_rejects_missing_or_malformed_evidence() {
  assert!(matches!(media_control_outcome_from_proto(None, "play").expect_err("outcome required"), CapabilityError::InvalidResponse(_)));
  assert!(matches!(
    media_control_outcome_from_proto(Some(macos_proto::MediaControlOutcome::default()), "play").expect_err("before required"),
    CapabilityError::InvalidResponse(_)
  ));
  let malformed = macos_proto::MediaControlOutcome {
    before: Some(macos_proto::NowPlayingState::default()),
    after: Some(macos_proto::NowPlayingState {
      elapsed_seconds: Some(f64::NAN),
      ..Default::default()
    }),
    verified: false,
  };
  assert!(matches!(
    media_control_outcome_from_proto(Some(malformed), "next").expect_err("finite evidence required"),
    CapabilityError::InvalidResponse(_)
  ));
}

#[tokio::test]
async fn runner_exposes_hierarchical_macos_media_client() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let media = runner.macos().media();
  drop(media.now_playing());
  drop(media.play());
  drop(media.pause());
  drop(media.toggle_play_pause());
  drop(media.next_track());
  drop(media.previous_track());
}

#[test]
fn overlay_cursor_shadow_serializes_without_scaling_native_dimensions() {
  let shadow = auv_driver_overlay_common::style::Shadow {
    color: auv_driver_overlay_common::style::Color::rgba(1.0, 0.6, 0.15, 0.65),
    blur_radius: 8.0,
    offset_x: 0.0,
    offset_y: 2.0,
  };
  let encoded = super::cursor_style_to_proto(auv_driver_overlay_common::style::CursorStyle {
    shadow: Some(shadow),
    ..Default::default()
  });
  assert_eq!(encoded.sprite_size, 24.0);
  assert_eq!(encoded.shadow.as_ref().unwrap().blur_radius, 8.0);
  assert_eq!(encoded.shadow.unwrap().color.unwrap().alpha, 0.65);
}

#[test]
fn coordinate_origins_survive_transport_and_map_decode_errors() {
  for coordinate_space in [
    auv_driver::CoordinateSpace::Screen,
    auv_driver::CoordinateSpace::Display("display-1".into()),
    auv_driver::CoordinateSpace::Window("window-1".into()),
  ] {
    let origin = auv_driver::Position {
      point: auv_driver::Point::new(-200.0, 300.0),
      coordinate_space,
    };
    let wire = position_to_proto(origin.clone());
    assert_eq!(position_from_proto(wire.clone()).unwrap(), origin);
    let recognized = text_recognition_from_proto(proto::RecognizeTextResponse {
      origin: Some(wire),
      ..Default::default()
    })
    .unwrap();
    assert_eq!(recognized.origin, Some(origin));
  }
  assert!(text_recognition_from_proto(proto::RecognizeTextResponse::default()).unwrap().origin.is_none());
  assert!(matches!(position_from_proto(proto::Position::default()), Err(CapabilityError::InvalidResponse(_))));
}

#[test]
fn scroll_options_projection_keeps_policy_candidate_order_and_settle() {
  let options = scroll_options_to_proto(auv_driver::ScrollOptions {
    policy: auv_driver::InputPolicy::BackgroundOnly,
    delivery_strategy: auv_driver::ScrollDeliveryStrategy {
      candidates: vec![
        auv_driver::ScrollDeliveryCandidate::WindowTargetedWheel,
        auv_driver::ScrollDeliveryCandidate::AxScroll,
      ],
    },
    settle: std::time::Duration::from_millis(120),
  })
  .expect("valid scroll options");

  assert_eq!(options.policy, proto::InputPolicy::BackgroundOnly as i32);
  assert_eq!(
    options.delivery_candidates,
    vec![
      proto::ScrollDeliveryCandidate::WindowTargetedWheel as i32,
      proto::ScrollDeliveryCandidate::AxScroll as i32,
    ]
  );
  assert_eq!(options.settle.expect("settle").nanos, 120_000_000);
}

#[test]
fn scroll_motion_projection_keeps_total_duration_function_and_rate() {
  let motion = scroll_motion_to_proto(auv_driver::ScrollMotion {
    total: auv_driver::Scroll::new(-20.0, 480.0),
    timing: auv_driver::MotionTiming::FixedDuration {
      duration: std::time::Duration::from_millis(750),
      function: auv_driver::TimingFunction::CubicBezier {
        x1: 0.2,
        y1: 0.8,
        x2: 0.2,
        y2: 1.0,
      },
    },
    sample_rate_hz: 90,
  })
  .expect("valid motion");
  assert_eq!(motion.total.unwrap().delta_y, 480.0);
  assert_eq!(motion.sample_rate_hz, 90);
  let Some(proto::scroll_motion::Timing::FixedDuration(timing)) = motion.timing else {
    panic!("fixed duration timing");
  };
  assert_eq!(timing.duration.unwrap().nanos, 750_000_000);
  assert_eq!(
    timing.function.unwrap().function,
    Some(proto::motion_timing_function::Function::CubicBezier(proto::CubicBezierMotionTimingFunction {
      x1: 0.2,
      y1: 0.8,
      x2: 0.2,
      y2: 1.0,
    }))
  );
}

#[test]
fn scroll_motion_events_require_their_payloads() {
  let completed = scroll_motion_event_from_proto(proto::ScrollWindowPointMotionResponse {
    event: Some(proto::scroll_window_point_motion_response::Event::Completed(proto::ScrollMotionCompleted {
      delivered: Some(proto::Scroll {
        delta_x: 0.0,
        delta_y: 300.0,
      }),
      action: None,
    })),
  })
  .expect_err("completed without delivery evidence");
  assert!(matches!(completed, CapabilityError::InvalidResponse(_)));
}

#[test]
fn scroll_stream_completion_maps_reason_and_optional_action() {
  let completed = scroll_stream_event_from_proto(proto::StreamScrollResponse {
    event: Some(proto::stream_scroll_response::Event::Completed(proto::StreamScrollCompleted {
      delivered: Some(proto::Scroll::default()),
      action: None,
      reason: proto::ScrollStreamStopReason::LeaseExpired as i32,
      elapsed: Some(prost_types::Duration {
        seconds: 1,
        nanos: 0,
      }),
    })),
  })
  .expect("idle stream completion");
  assert_eq!(
    completed,
    ScrollStreamEvent::Completed {
      delivered: auv_driver::Scroll::new(0.0, 0.0),
      action: None,
      reason: auv_driver::ScrollStreamStopReason::LeaseExpired,
      elapsed: std::time::Duration::from_secs(1),
    }
  );
  let unknown = scroll_stream_event_from_proto(proto::StreamScrollResponse {
    event: Some(proto::stream_scroll_response::Event::Completed(proto::StreamScrollCompleted {
      delivered: Some(proto::Scroll::default()),
      action: None,
      reason: proto::ScrollStreamStopReason::Unspecified as i32,
      elapsed: Some(prost_types::Duration::default()),
    })),
  })
  .expect_err("unspecified reason");
  assert!(matches!(unknown, CapabilityError::InvalidResponse(_)));
}

#[test]
fn scroll_until_begin_projection_keeps_step_condition_region_and_opt_outs() {
  let begin = scroll_until_begin_to_proto(
    proto::WindowRef {
      window_id: "window-1".to_string(),
    },
    auv_driver::WindowPoint::new(10.0, 20.0),
    auv_scan::ScrollUntilRequest {
      step: auv_scan::ScrollUntilStep::Instant {
        delta: auv_driver::Scroll::new(0.0, 600.0),
      },
      condition: auv_scan::ScrollUntilCondition::TextVisible {
        query: "Load more".to_string(),
      },
      max_steps: 30,
      settle: std::time::Duration::from_millis(500),
      no_motion_confirmations: 3,
      motion_region: Some(auv_driver::RatioRect::new(0.0, 0.2, 1.0, 0.6)),
      output: auv_scan::ScrollUntilOutputOptions { text: false },
    },
    auv_driver::ScrollOptions::default(),
    true,
  )
  .expect("valid request");
  assert_eq!(
    begin.step,
    Some(proto::scroll_until_begin::Step::Instant(proto::Scroll {
      delta_x: 0.0,
      delta_y: 600.0
    }))
  );
  assert_eq!(
    begin.condition,
    Some(proto::scroll_until_begin::Condition::TextVisible(proto::ScrollUntilTextVisible {
      query: "Load more".to_string()
    }))
  );
  assert_eq!((begin.max_steps, begin.no_motion_confirmations), (30, 3));
  assert_eq!(begin.motion_region.unwrap().height, 0.6);
  assert_eq!(begin.output, Some(proto::ScrollUntilOutputOptions { omit_text: true }));
  assert!(begin.await_decisions);
}

#[test]
fn scroll_until_update_event_decodes_capture_ref_text_and_decision_flag() {
  let event = scroll_until_event_from_proto(proto::ScrollUntilResponse {
    event: Some(proto::scroll_until_response::Event::Update(proto::ScrollUntilUpdate {
      steps: 2,
      delivered: Some(proto::Scroll {
        delta_x: 0.0,
        delta_y: 1000.0,
      }),
      motion: Some(proto::ViewportPixelMotion {
        estimated_shift: 12,
        normalized_diff: 0.2,
        no_motion: false,
      }),
      no_motion_streak: 0,
      capture: Some(large_capture_metadata()),
      text: Some(proto::RecognizeTextResponse {
        text: "row".to_string(),
        ..Default::default()
      }),
      stop: proto::ScrollUntilStopReason::Unspecified as i32,
      awaiting_decision: true,
    })),
  })
  .expect("valid update");
  let ScrollUntilEvent::Update {
    update,
    awaiting_decision,
  } = event
  else {
    panic!("expected an update");
  };
  assert!(awaiting_decision);
  assert_eq!((update.steps, update.stop), (2, None));
  assert_eq!(update.capture.reference.id(), LARGE_CAPTURE_ID);
  assert_eq!(update.text.expect("text").text, "row");

  let completed = scroll_until_event_from_proto(proto::ScrollUntilResponse {
    event: Some(proto::scroll_until_response::Event::Completed(proto::ScrollUntilCompleted {
      reason: proto::ScrollUntilStopReason::PredicateSatisfied as i32,
      delivered: Some(proto::Scroll::default()),
      ..Default::default()
    })),
  })
  .expect("valid completion");
  assert!(matches!(
    completed,
    ScrollUntilEvent::Completed(auv_scan::ScrollUntilResult {
      reason: auv_scan::ScrollUntilStopReason::PredicateSatisfied,
      ..
    })
  ));
}
