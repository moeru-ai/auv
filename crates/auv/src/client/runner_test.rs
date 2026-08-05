use super::*;

#[derive(Debug)]
struct LargeCaptureService;

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
      capture: Some(proto::CapturedFrame {
        image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
          width: 1280,
          height: 1024,
          data: vec![0; 1280 * 1024 * 4],
        }),
        bounds: Some(proto::ScreenRect {
          width: 1280.0,
          height: 1024.0,
          ..Default::default()
        }),
        ..Default::default()
      }),
      ..Default::default()
    }))
  }

  async fn capture_region(
    &self,
    _request: tonic::Request<proto::CaptureRegionRequest>,
  ) -> Result<tonic::Response<proto::CaptureRegionResponse>, tonic::Status> {
    Err(tonic::Status::unimplemented("not used by this regression"))
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

#[tokio::test]
async fn capture_client_accepts_desktop_frames_larger_than_tonic_default() {
  // ROOT CAUSE:
  //
  // If a desktop capture exceeded tonic's 4 MiB decoded-message default, the
  // routed client rejected the valid frame before the extension could inspect
  // it. Runner image clients must share the server's image-message policy.
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
  let response =
    RunnerClient::new(grpc, route()).expect("runner client").displays().capture(None).await.expect("decode capture larger than 4 MiB");
  assert!(response.capture.image.as_raw().len() > 4 * 1024 * 1024);

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
  let call = input.click_screen_point(auv_driver::Point::new(10.0, 20.0), auv_driver::Click::Single);
  drop(call);
}

#[tokio::test]
async fn runner_input_exposes_typed_mouse_motion() {
  let runner = RunnerClient::new(disconnected_client(), route()).expect("runner client");
  let input = runner.input();
  let call = input.move_mouse(auv_driver::MouseMotionPlan::direct(auv_driver::Point::new(10.0, 20.0)));
  drop(call);

  let plan = auv_driver::MouseMotionPlan::direct(auv_driver::Point::new(10.0, 20.0));
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
fn capture_projection_preserves_rgba_and_screen_contract() {
  let capture = capture_from_proto(proto::CapturedFrame {
    image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
      width: 2,
      height: 1,
      data: vec![1, 2, 3, 4, 5, 6, 7, 8],
    }),
    bounds: Some(proto::ScreenRect {
      x: -10.0,
      y: 4.0,
      width: 2.0,
      height: 1.0,
    }),
    scale_factor: 1.0,
    backend: "fixture".to_string(),
    fallback_reason: None,
  })
  .expect("valid RGBA capture");

  assert_eq!(capture.image.as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(capture.bounds, auv_driver::Rect::new(-10.0, 4.0, 2.0, 1.0));
  assert_eq!(capture.backend, "fixture");
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
