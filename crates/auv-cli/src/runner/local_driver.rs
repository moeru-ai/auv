//! First-party local Driver Runner served on a daemon-inherited stream.

use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
use auv_api_proto::auv::api::driver::macos::v1::accessibility_service_server::{AccessibilityService, AccessibilityServiceServer};
use auv_api_proto::auv::api::driver::macos::v1::application_service_server::{ApplicationService, ApplicationServiceServer};
use auv_api_proto::auv::api::driver::macos::v1::media_control_service_server::{MediaControlService, MediaControlServiceServer};
use auv_api_proto::auv::api::driver::macos::v1::permission_service_server::{PermissionService, PermissionServiceServer};
use auv_api_proto::auv::api::driver::v1 as proto;
use auv_api_proto::auv::api::driver::v1::capture_service_server::{CaptureService, CaptureServiceServer};
use auv_api_proto::auv::api::driver::v1::display_service_server::{DisplayService, DisplayServiceServer};
use auv_api_proto::auv::api::driver::v1::input_service_server::{InputService, InputServiceServer};
use auv_api_proto::auv::api::driver::v1::overlay_service_server::{OverlayService, OverlayServiceServer};
use auv_api_proto::auv::api::driver::v1::text_recognition_service_server::{TextRecognitionService, TextRecognitionServiceServer};
use auv_api_proto::auv::api::driver::v1::window_service_server::{WindowService, WindowServiceServer};
use std::pin::Pin;

use futures_core::Stream;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use auv_driver::{Driver as _, WindowInput as _};

struct LocalDisplayService {
  session: auv_driver::LocalDriverSession,
}

struct LocalWindowService {
  session: auv_driver::LocalDriverSession,
}

struct LocalCaptureService {
  session: auv_driver::LocalDriverSession,
}

struct LocalTextRecognitionService {
  session: auv_driver::LocalDriverSession,
}

struct LocalInputService {
  session: auv_driver::LocalDriverSession,
  /// Serializes global pointer ownership so concurrent trajectories and
  /// screen clicks cannot interleave samples on one desktop.
  mouse_motion: std::sync::Arc<tokio::sync::Mutex<()>>,
}

struct LocalOverlayService {
  session: auv_driver::LocalDriverSession,
  owner_thread: std::thread::ThreadId,
}

impl LocalOverlayService {
  fn ensure_owner_thread(&self) -> Result<(), Status> {
    ensure_overlay_owner_thread(self.owner_thread)
  }
}

fn ensure_overlay_owner_thread(owner_thread: std::thread::ThreadId) -> Result<(), Status> {
  if std::thread::current().id() != owner_thread {
    return Err(Status::failed_precondition("OverlayService must execute on the local-driver process main thread"));
  }
  Ok(())
}

#[tonic::async_trait]
impl OverlayService for LocalOverlayService {
  async fn show_overlay(&self, request: Request<proto::ShowOverlayRequest>) -> Result<Response<proto::ShowOverlayResponse>, Status> {
    self.ensure_owner_thread()?;
    let request = request.into_inner();
    let overlay = overlay_from_proto(request.overlay.ok_or_else(|| Status::invalid_argument("overlay is required"))?)?;
    if overlay.layers().is_empty() {
      return Err(Status::invalid_argument("overlay.layers must not be empty"));
    }
    let options = overlay_options_from_proto(request.options)?;
    #[cfg(target_os = "macos")]
    self.session.overlay().show(&overlay, options).map_err(driver_status)?;
    #[cfg(not(target_os = "macos"))]
    return Err(Status::unimplemented("OverlayService is unavailable on this local Driver platform"));
    Ok(Response::new(proto::ShowOverlayResponse {}))
  }

  async fn remove_overlay(&self, _request: Request<proto::RemoveOverlayRequest>) -> Result<Response<proto::RemoveOverlayResponse>, Status> {
    self.ensure_owner_thread()?;
    // TODO(overlay-handles): RemoveOverlay currently matches the owner API and
    // removes all AUV-owned layers. Per-scene handles/replace semantics remain
    // deferred until the owner exposes stable presentation identity.
    #[cfg(target_os = "macos")]
    self.session.overlay().remove().map_err(driver_status)?;
    #[cfg(not(target_os = "macos"))]
    return Err(Status::unimplemented("OverlayService is unavailable on this local Driver platform"));
    Ok(Response::new(proto::RemoveOverlayResponse {}))
  }
}

fn overlay_from_proto(value: proto::Overlay) -> Result<auv_driver::overlay::Overlay, Status> {
  use auv_driver::overlay::layers::{Cursor, CursorImage, Outline, Status as OverlayStatus};
  use proto::overlay_layer::Layer;
  let mut overlay = auv_driver::overlay::Overlay::new();
  for layer in value.layers {
    overlay = match layer.layer.ok_or_else(|| Status::invalid_argument("overlay layer is required"))? {
      Layer::Cursor(value) => {
        let point = screen_point_from_proto(value.point.ok_or_else(|| Status::invalid_argument("cursor.point is required"))?)?;
        let mut cursor = Cursor::new(point).with_style(value.style.map(cursor_style_from_proto).transpose()?.unwrap_or_default());
        if let Some(label) = value.label {
          cursor = cursor.with_label(label);
        }
        if value.label_visible {
          cursor = cursor.with_label_visible();
        }
        if let Some(image) = value.image.and_then(|image| image.image) {
          cursor = cursor.with_image(match image {
            proto::cursor_image::Image::BuiltIn(value) => CursorImage::built_in(match proto::BuiltInCursor::try_from(value) {
              Ok(proto::BuiltInCursor::Auv) => auv_driver::overlay::layers::BuiltInCursor::Auv,
              Ok(proto::BuiltInCursor::AuvClick) => auv_driver::overlay::layers::BuiltInCursor::AuvClick,
              Ok(proto::BuiltInCursor::You) => auv_driver::overlay::layers::BuiltInCursor::You,
              _ => return Err(Status::invalid_argument("cursor.image.built_in is unknown")),
            }),
            proto::cursor_image::Image::Svg(source) if source.len() <= 256 * 1024 => CursorImage::svg(source),
            proto::cursor_image::Image::Svg(_) => return Err(Status::invalid_argument("cursor SVG exceeds 256 KiB")),
          });
        }
        overlay.with_layer(cursor)
      }
      Layer::Outline(value) => {
        let rect = screen_rect_from_proto(value.rect.ok_or_else(|| Status::invalid_argument("outline.rect is required"))?)?;
        let mut outline = Outline::new(rect).with_style(value.style.map(outline_style_from_proto).transpose()?.unwrap_or_default());
        if let Some(label) = value.label {
          outline = outline.with_label(label);
        }
        if value.label_visible {
          outline = outline.with_label_visible();
        }
        overlay.with_layer(outline)
      }
      Layer::Status(value) => {
        if value.text.is_empty() {
          return Err(Status::invalid_argument("status.text is required"));
        }
        let point = screen_point_from_proto(value.point.ok_or_else(|| Status::invalid_argument("status.point is required"))?)?;
        overlay.with_layer(
          OverlayStatus::new(point, value.text).with_style(value.style.map(status_style_from_proto).transpose()?.unwrap_or_default()),
        )
      }
    };
  }
  Ok(overlay)
}

fn overlay_options_from_proto(value: Option<proto::ShowOptions>) -> Result<auv_driver::overlay::ShowOptions, Status> {
  use proto::lifecycle_options::Removal;
  let defaults = auv_driver::overlay::ShowOptions::new();
  let Some(value) = value else {
    return Ok(defaults);
  };
  let duration = if let Some(motion) = value.motion {
    if let Some(easing) = motion.easing
      && !matches!(proto::Easing::try_from(easing), Ok(proto::Easing::EaseInOutExpo))
    {
      return Err(Status::invalid_argument("options.motion.easing is unknown"));
    }
    duration_from_proto(motion.duration, defaults.motion().duration(), "options.motion.duration")?
  } else {
    defaults.motion().duration()
  };
  let lifecycle = match value.lifecycle.and_then(|value| value.removal) {
    None => defaults.lifecycle(),
    Some(Removal::Manual(_)) => auv_driver::overlay::LifecycleOptions::manual(),
    Some(Removal::AutoAfter(value)) => auv_driver::overlay::LifecycleOptions::new().with_auto_removal_after(duration_from_proto(
      Some(value),
      std::time::Duration::ZERO,
      "options.lifecycle.auto_after",
    )?),
  };
  Ok(
    auv_driver::overlay::ShowOptions::new()
      .with_motion_ease(duration, auv_driver::overlay::Easing::EaseInOutExpo)
      .with_lifecycle_options(lifecycle),
  )
}

fn overlay_color(value: Option<proto::Color>, field: &str) -> Result<auv_driver::overlay::style::Color, Status> {
  let value = value.ok_or_else(|| Status::invalid_argument(format!("{field} is required")))?;
  let channels = [value.red, value.green, value.blue, value.alpha];
  if channels.iter().any(|value| !value.is_finite() || !(0.0..=1.0).contains(value)) {
    return Err(Status::invalid_argument(format!("{field} channels must be finite and in [0, 1]")));
  }
  Ok(auv_driver::overlay::style::Color::rgba(value.red, value.green, value.blue, value.alpha))
}

fn overlay_insets(value: Option<proto::Insets>, field: &str) -> Result<auv_driver::overlay::style::Insets, Status> {
  let value = value.ok_or_else(|| Status::invalid_argument(format!("{field} is required")))?;
  let values = [value.top, value.right, value.bottom, value.left];
  if values.iter().any(|value| !value.is_finite() || *value < 0.0) {
    return Err(Status::invalid_argument(format!("{field} must be finite and non-negative")));
  }
  Ok(auv_driver::overlay::style::Insets {
    top: value.top,
    right: value.right,
    bottom: value.bottom,
    left: value.left,
  })
}

fn finite_non_negative(value: f64, field: &str) -> Result<f64, Status> {
  if !value.is_finite() || value < 0.0 {
    return Err(Status::invalid_argument(format!("{field} must be finite and non-negative")));
  }
  Ok(value)
}

fn outline_style_from_proto(value: proto::OutlineStyle) -> Result<auv_driver::overlay::style::OutlineStyle, Status> {
  let stroke = value.stroke.ok_or_else(|| Status::invalid_argument("outline.style.stroke is required"))?;
  Ok(auv_driver::overlay::style::OutlineStyle {
    stroke: auv_driver::overlay::style::Stroke::new(
      overlay_color(stroke.color, "outline.style.stroke.color")?,
      finite_non_negative(stroke.width, "outline.style.stroke.width")?,
    ),
    padding: overlay_insets(value.padding, "outline.style.padding")?,
    corner_radius: finite_non_negative(value.corner_radius, "outline.style.corner_radius")?,
  })
}

fn cursor_style_from_proto(value: proto::CursorStyle) -> Result<auv_driver::overlay::style::CursorStyle, Status> {
  Ok(auv_driver::overlay::style::CursorStyle {
    label_foreground: overlay_color(value.label_foreground, "cursor.style.label_foreground")?,
    label_background: overlay_color(value.label_background, "cursor.style.label_background")?,
    label_padding: overlay_insets(value.label_padding, "cursor.style.label_padding")?,
    label_corner_radius: finite_non_negative(value.label_corner_radius, "cursor.style.label_corner_radius")?,
    sprite_size: finite_non_negative(value.sprite_size, "cursor.style.sprite_size")?,
    label_gap: finite_non_negative(value.label_gap, "cursor.style.label_gap")?,
  })
}

fn status_style_from_proto(value: proto::StatusStyle) -> Result<auv_driver::overlay::style::StatusStyle, Status> {
  Ok(auv_driver::overlay::style::StatusStyle {
    foreground: overlay_color(value.foreground, "status.style.foreground")?,
    background: overlay_color(value.background, "status.style.background")?,
    padding: overlay_insets(value.padding, "status.style.padding")?,
    corner_radius: finite_non_negative(value.corner_radius, "status.style.corner_radius")?,
  })
}

struct LocalPermissionService {
  session: auv_driver::LocalDriverSession,
}

struct LocalApplicationService {
  session: auv_driver::LocalDriverSession,
}

struct LocalAccessibilityService {
  session: auv_driver::LocalDriverSession,
}

#[tonic::async_trait]
impl AccessibilityService for LocalAccessibilityService {
  async fn focus_text(&self, request: Request<macos_proto::FocusTextRequest>) -> Result<Response<macos_proto::FocusTextResponse>, Status> {
    let options = focus_text_options_from_proto(request.into_inner())?;
    #[cfg(target_os = "macos")]
    {
      let result = self.session.accessibility().focus_text(options).map_err(driver_status)?;
      Ok(Response::new(macos_proto::FocusTextResponse {
        result: Some(ax_focus_result_to_proto(result)?),
      }))
    }
    #[cfg(not(target_os = "macos"))]
    {
      let _ = (&self.session, options);
      Err(Status::unimplemented("macOS AccessibilityService is unavailable on this local Driver platform"))
    }
  }
}

fn focus_text_options_from_proto(request: macos_proto::FocusTextRequest) -> Result<auv_driver::FocusTextOptions, Status> {
  use macos_proto::focus_text_request::Selector;

  let selector = match request.selector {
    Some(Selector::Query(query)) if !query.trim().is_empty() => auv_driver::AxTextSelector::Query(query),
    Some(Selector::Path(path)) if !path.trim().is_empty() => auv_driver::AxTextSelector::Path(path),
    Some(Selector::Query(_)) => return Err(Status::invalid_argument("query must be non-empty")),
    Some(Selector::Path(_)) => return Err(Status::invalid_argument("path must be non-empty")),
    None => return Err(Status::invalid_argument("selector is required")),
  };
  let options = auv_driver::FocusTextOptions {
    app: request.application,
    selector,
    expected_role: request.expected_role,
  };
  options.validate().map_err(driver_status)?;
  Ok(options)
}

fn ax_focus_result_to_proto(result: auv_driver::AxFocusResult) -> Result<macos_proto::AxFocusResult, Status> {
  Ok(macos_proto::AxFocusResult {
    app: result.app,
    pid: result.pid,
    path: result.path,
    role: result.role,
    title: result.title,
    value: result.value,
    query: result.query,
    action: Some(input_action_to_proto(result.input_action_result)?),
  })
}

#[tonic::async_trait]
impl ApplicationService for LocalApplicationService {
  async fn activate_bundle_id(
    &self,
    request: Request<macos_proto::ActivateBundleIdRequest>,
  ) -> Result<Response<macos_proto::ActivateBundleIdResponse>, Status> {
    let request = request.into_inner();
    let bundle_id = application_bundle_id(&request.bundle_id)?;
    let settle = duration_from_proto(request.settle, std::time::Duration::from_millis(150), "settle")?;
    #[cfg(target_os = "macos")]
    {
      use auv_driver_macos::ApplicationControl as _;

      let result = self.session.activate_bundle_id(bundle_id, settle).map_err(driver_status)?;
      Ok(Response::new(application_activation_to_proto(result)))
    }
    #[cfg(not(target_os = "macos"))]
    {
      let _ = (&self.session, settle);
      Err(Status::unimplemented("macOS ApplicationService is unavailable on this local Driver platform"))
    }
  }
}

fn application_bundle_id(bundle_id: &str) -> Result<&str, Status> {
  let bundle_id = bundle_id.trim();
  if bundle_id.is_empty() {
    return Err(Status::invalid_argument("bundle_id is required"));
  }
  Ok(bundle_id)
}

fn application_activation_to_proto(result: auv_driver::ApplicationActivationResult) -> macos_proto::ActivateBundleIdResponse {
  use auv_driver::ApplicationActivationVerification as Domain;
  use macos_proto::application_activation_verification::Verification;

  let verification = match result.verification {
    Domain::VerifiedForeground { observed_bundle_id } => {
      Verification::VerifiedForeground(macos_proto::VerifiedForeground { observed_bundle_id })
    }
    Domain::ForegroundMismatch { observed_bundle_id } => {
      Verification::ForegroundMismatch(macos_proto::ForegroundMismatch { observed_bundle_id })
    }
    Domain::Unavailable { reason } => Verification::Unavailable(macos_proto::VerificationUnavailable { reason }),
  };
  macos_proto::ActivateBundleIdResponse {
    requested_bundle_id: result.requested_bundle_id,
    verification: Some(macos_proto::ApplicationActivationVerification {
      verification: Some(verification),
    }),
  }
}

#[derive(Default)]
struct LocalMediaControlService;

#[tonic::async_trait]
impl MediaControlService for LocalMediaControlService {
  async fn get_now_playing(
    &self,
    _request: Request<macos_proto::GetNowPlayingRequest>,
  ) -> Result<Response<macos_proto::GetNowPlayingResponse>, Status> {
    let state = auv_media_macos::now_playing().map_err(media_status)?;
    Ok(Response::new(macos_proto::GetNowPlayingResponse {
      state: Some(now_playing_to_proto(state)?),
    }))
  }

  async fn play(&self, _request: Request<macos_proto::PlayRequest>) -> Result<Response<macos_proto::PlayResponse>, Status> {
    Ok(Response::new(macos_proto::PlayResponse {
      outcome: Some(media_control_outcome_to_proto(control_media(auv_media_macos::MediaCommand::Play)?)?),
    }))
  }

  async fn pause(&self, _request: Request<macos_proto::PauseRequest>) -> Result<Response<macos_proto::PauseResponse>, Status> {
    Ok(Response::new(macos_proto::PauseResponse {
      outcome: Some(media_control_outcome_to_proto(control_media(auv_media_macos::MediaCommand::Pause)?)?),
    }))
  }

  async fn toggle_play_pause(
    &self,
    _request: Request<macos_proto::TogglePlayPauseRequest>,
  ) -> Result<Response<macos_proto::TogglePlayPauseResponse>, Status> {
    Ok(Response::new(macos_proto::TogglePlayPauseResponse {
      outcome: Some(media_control_outcome_to_proto(control_media(auv_media_macos::MediaCommand::TogglePlayPause)?)?),
    }))
  }

  async fn next_track(&self, _request: Request<macos_proto::NextTrackRequest>) -> Result<Response<macos_proto::NextTrackResponse>, Status> {
    Ok(Response::new(macos_proto::NextTrackResponse {
      outcome: Some(media_control_outcome_to_proto(control_media(auv_media_macos::MediaCommand::NextTrack)?)?),
    }))
  }

  async fn previous_track(
    &self,
    _request: Request<macos_proto::PreviousTrackRequest>,
  ) -> Result<Response<macos_proto::PreviousTrackResponse>, Status> {
    Ok(Response::new(macos_proto::PreviousTrackResponse {
      outcome: Some(media_control_outcome_to_proto(control_media(auv_media_macos::MediaCommand::PreviousTrack)?)?),
    }))
  }
}

fn control_media(command: auv_media_macos::MediaCommand) -> Result<auv_media_macos::output::MediaControlOutcome, Status> {
  auv_media_macos::control(command).map_err(media_control_status)
}

fn media_status(error: auv_media_macos::MediaError) -> Status {
  match error {
    auv_media_macos::MediaError::Unsupported => Status::unimplemented(error.to_string()),
    auv_media_macos::MediaError::Native { .. } => Status::unavailable(error.to_string()),
  }
}

fn media_control_status(error: auv_media_macos::MediaError) -> Status {
  match error {
    auv_media_macos::MediaError::Unsupported => Status::unimplemented(error.to_string()),
    auv_media_macos::MediaError::Native { .. } => {
      Status::unknown(format!("media command outcome is uncertain; do not retry automatically: {error}"))
    }
  }
}

fn media_control_outcome_to_proto(
  outcome: auv_media_macos::output::MediaControlOutcome,
) -> Result<macos_proto::MediaControlOutcome, Status> {
  Ok(macos_proto::MediaControlOutcome {
    before: Some(now_playing_output_to_proto(outcome.before)?),
    after: Some(now_playing_output_to_proto(outcome.after)?),
    verified: outcome.verified,
  })
}

fn now_playing_output_to_proto(state: auv_media_macos::output::NowPlayingOutput) -> Result<macos_proto::NowPlayingState, Status> {
  now_playing_to_proto(auv_media_macos::NowPlayingState {
    present: state.present,
    is_playing: state.is_playing,
    source_bundle_id: state.source_bundle_id,
    title: state.title,
    artist: state.artist,
    album: state.album,
    duration_seconds: state.duration_seconds,
    elapsed_seconds: state.elapsed_seconds,
    playback_rate: state.playback_rate,
    content_item_id: state.content_item_id,
    supports_like: state.supports_like,
    is_liked: state.is_liked,
  })
}

fn now_playing_to_proto(state: auv_media_macos::NowPlayingState) -> Result<macos_proto::NowPlayingState, Status> {
  for (field, value) in [
    ("duration_seconds", state.duration_seconds),
    ("elapsed_seconds", state.elapsed_seconds),
    ("playback_rate", state.playback_rate),
  ] {
    if value.is_some_and(|value| !value.is_finite()) {
      return Err(Status::internal(format!("macOS now-playing backend returned non-finite {field}")));
    }
  }
  Ok(macos_proto::NowPlayingState {
    present: state.present,
    is_playing: state.is_playing,
    source_bundle_id: state.source_bundle_id,
    title: state.title,
    artist: state.artist,
    album: state.album,
    duration_seconds: state.duration_seconds,
    elapsed_seconds: state.elapsed_seconds,
    playback_rate: state.playback_rate,
    content_item_id: state.content_item_id,
    supports_like: state.supports_like,
    is_liked: state.is_liked,
  })
}

#[tonic::async_trait]
impl PermissionService for LocalPermissionService {
  async fn probe_permissions(
    &self,
    _request: Request<macos_proto::ProbePermissionsRequest>,
  ) -> Result<Response<macos_proto::ProbePermissionsResponse>, Status> {
    #[cfg(target_os = "macos")]
    {
      let probe = self.session.permission().probe().map_err(driver_status)?;
      Ok(Response::new(permission_probe_to_proto(probe)))
    }
    #[cfg(not(target_os = "macos"))]
    {
      let _ = &self.session;
      Err(Status::unimplemented("macOS PermissionService is unavailable on this local Driver platform"))
    }
  }
}

fn permission_probe_to_proto(probe: auv_driver::PermissionProbe) -> macos_proto::ProbePermissionsResponse {
  macos_proto::ProbePermissionsResponse {
    screen_recording: permission_status_to_proto(probe.screen_recording) as i32,
    screen_capture_kit: permission_status_to_proto(probe.screen_capture_kit) as i32,
    accessibility: permission_status_to_proto(probe.accessibility) as i32,
    automation_to_system_events: permission_status_to_proto(probe.automation_to_system_events) as i32,
  }
}

fn permission_status_to_proto(status: auv_driver::PermissionStatus) -> macos_proto::PermissionStatus {
  match status {
    auv_driver::PermissionStatus::Granted => macos_proto::PermissionStatus::Granted,
    auv_driver::PermissionStatus::Missing => macos_proto::PermissionStatus::Missing,
    auv_driver::PermissionStatus::Unknown => macos_proto::PermissionStatus::Unknown,
  }
}

#[tonic::async_trait]
impl InputService for LocalInputService {
  type MoveMouseStream = Pin<Box<dyn Stream<Item = Result<proto::MoveMouseStreamResponse, Status>> + Send>>;
  type StreamMouseMotionStream = Pin<Box<dyn Stream<Item = Result<proto::StreamMouseMotionResponse, Status>> + Send>>;

  async fn click_window_point(
    &self,
    request: Request<proto::ClickWindowPointRequest>,
  ) -> Result<Response<proto::ClickWindowPointResponse>, Status> {
    let request = request.into_inner();
    let window_ref = request.window.ok_or_else(|| Status::invalid_argument("window is required"))?;
    let point = window_point_from_proto(request.point.ok_or_else(|| Status::invalid_argument("point is required"))?)?;
    let options = click_options_from_proto(request.options)?;
    let window = resolve_window_ref(&self.session, window_ref)?;
    let size = window.frame.size;
    let raw = point.point();
    if raw.x < 0.0 || raw.y < 0.0 || raw.x > size.width || raw.y > size.height {
      return Err(Status::invalid_argument("point must be inside the current Window bounds"));
    }
    let action = self.session.window().click(&window, point, options).map_err(driver_status)?;
    Ok(Response::new(proto::ClickWindowPointResponse {
      window: Some(window_to_proto(window)),
      point: Some(window_point_to_proto(point)),
      action: Some(input_action_to_proto(action)?),
    }))
  }

  async fn click_screen_point(
    &self,
    request: Request<proto::ClickScreenPointRequest>,
  ) -> Result<Response<proto::ClickScreenPointResponse>, Status> {
    let _motion = self.mouse_motion.lock().await;
    let request = request.into_inner();
    let point = screen_point_from_proto(request.point.ok_or_else(|| Status::invalid_argument("point is required"))?)?;
    let click = screen_click_options_from_proto(request.options)?;
    let action = self.session.input().click_at(point.point(), click).map_err(driver_status)?;
    Ok(Response::new(proto::ClickScreenPointResponse {
      point: Some(screen_point_to_proto(point)),
      action: Some(input_action_to_proto(action)?),
    }))
  }

  async fn move_mouse(&self, request: Request<proto::MoveMouseRequest>) -> Result<Response<Self::MoveMouseStream>, Status> {
    let plan = mouse_motion_plan_from_proto(request.into_inner().plan.ok_or_else(|| Status::invalid_argument("plan is required"))?)?;
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let session = self.session.clone();
    let mouse_motion = self.mouse_motion.clone();
    tokio::spawn(async move {
      let _motion = mouse_motion.lock().await;
      run_mouse_motion(session, plan, |event| async { sender.send(event.map(move_mouse_stream_event)).await.map_err(|_| ()) }).await;
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
  }

  async fn stream_mouse_motion(
    &self,
    request: Request<tonic::Streaming<proto::StreamMouseMotionRequest>>,
  ) -> Result<Response<Self::StreamMouseMotionStream>, Status> {
    let mut requests = request.into_inner();
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    let session = self.session.clone();
    let mouse_motion = self.mouse_motion.clone();
    tokio::spawn(async move {
      let result = collect_mouse_motion(&mut requests, &sender).await;
      match result {
        Ok(Some(plan)) => {
          let _motion = mouse_motion.lock().await;
          run_mouse_motion(session, plan, |event| async { sender.send(event.map(stream_mouse_motion_event)).await.map_err(|_| ()) }).await;
        }
        Ok(None) => {}
        Err(status) => {
          let _ = sender.send(Err(status)).await;
        }
      }
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
  }

  async fn type_text(&self, request: Request<proto::TypeTextRequest>) -> Result<Response<proto::TypeTextResponse>, Status> {
    let request = request.into_inner();
    if request.text.is_empty() {
      return Err(Status::invalid_argument("text is required"));
    }
    let options = type_text_options_from_proto(request.options)?;
    let action = self.session.input().type_text(&request.text, options).map_err(driver_status)?;
    Ok(Response::new(proto::TypeTextResponse {
      action: Some(input_action_to_proto(action)?),
    }))
  }

  async fn paste_text(&self, request: Request<proto::PasteTextRequest>) -> Result<Response<proto::PasteTextResponse>, Status> {
    let request = request.into_inner();
    let options = paste_text_options_from_proto(request.text, request.options)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
      let action = self.session.input().paste_text(options).map_err(driver_status)?;
      Ok(Response::new(proto::PasteTextResponse {
        action: Some(input_action_to_proto(action)?),
      }))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
      let _ = options;
      Err(Status::unimplemented("PasteText is unavailable on this local Driver platform"))
    }
  }

  async fn press_key(&self, request: Request<proto::PressKeyRequest>) -> Result<Response<proto::PressKeyResponse>, Status> {
    let request = request.into_inner();
    if request.key.trim().is_empty() {
      return Err(Status::invalid_argument("key is required"));
    }
    let settle = duration_from_proto(request.settle, std::time::Duration::ZERO, "settle")?;
    let action = self
      .session
      .input()
      .press_key(auv_driver::KeyPressOptions {
        key: request.key,
        settle,
      })
      .map_err(driver_status)?;
    Ok(Response::new(proto::PressKeyResponse {
      action: Some(input_action_to_proto(action)?),
    }))
  }
}

fn resolve_window_ref(session: &auv_driver::LocalDriverSession, window_ref: proto::WindowRef) -> Result<auv_driver::Window, Status> {
  if window_ref.window_id.trim().is_empty() {
    return Err(Status::invalid_argument("window.window_id is required"));
  }
  session
    .window()
    .list()
    .map_err(driver_status)?
    .into_iter()
    .find(|window| window.reference.id == window_ref.window_id)
    .ok_or_else(|| Status::not_found(format!("unknown Window: {}", window_ref.window_id)))
}

fn window_point_from_proto(point: proto::WindowPoint) -> Result<auv_driver::WindowPoint, Status> {
  if !point.x.is_finite() || !point.y.is_finite() {
    return Err(Status::invalid_argument("point coordinates must be finite"));
  }
  Ok(auv_driver::WindowPoint::new(point.x, point.y))
}

fn screen_point_from_proto(point: proto::ScreenPoint) -> Result<auv_driver::ScreenPoint, Status> {
  if !point.x.is_finite() || !point.y.is_finite() {
    return Err(Status::invalid_argument("point coordinates must be finite"));
  }
  Ok(auv_driver::ScreenPoint::new(point.x, point.y))
}

enum MouseMotionEvent {
  Started(proto::MouseMotionStarted),
  Progress(proto::MouseMotionProgress),
  Completed(proto::MouseMotionCompleted),
}

async fn run_mouse_motion<F, Fut>(session: auv_driver::LocalDriverSession, plan: auv_driver::MouseMotionPlan, mut send: F)
where
  F: FnMut(Result<MouseMotionEvent, Status>) -> Fut,
  Fut: std::future::Future<Output = Result<(), ()>>,
{
  let resolved_start = match plan.start {
    auv_driver::MouseStart::Current => match tokio::task::spawn_blocking({
      let session = session.clone();
      move || session.input().current_position()
    })
    .await
    .map_err(|error| Status::internal(format!("mouse position task failed: {error}")))
    .and_then(|result| result.map_err(driver_status))
    {
      Ok(point) => point,
      Err(status) => {
        let _ = send(Err(status)).await;
        return;
      }
    },
    auv_driver::MouseStart::Screen(point) => point,
  };
  let samples = match plan.samples(resolved_start).map_err(driver_status) {
    Ok(samples) => samples,
    Err(status) => {
      let _ = send(Err(status)).await;
      return;
    }
  };
  if send(Ok(MouseMotionEvent::Started(proto::MouseMotionStarted {
    resolved_start: Some(raw_screen_point_to_proto(resolved_start)),
    planned_sample_count: samples.len() as u32,
    duration: Some(duration_to_proto(plan.options.duration)),
  })))
  .await
  .is_err()
  {
    return;
  }

  let started = tokio::time::Instant::now();
  let mut final_action = None;
  let mut next_index = 0;
  while next_index < samples.len() {
    let index = latest_due_mouse_sample(&samples, next_index, started.elapsed());
    let sample = &samples[index];
    tokio::time::sleep_until(started + sample.elapsed).await;
    let action = match tokio::task::spawn_blocking({
      let session = session.clone();
      let point = sample.point;
      move || session.input().move_to(point)
    })
    .await
    .map_err(|error| Status::internal(format!("mouse delivery task failed: {error}")))
    .and_then(|result| result.map_err(driver_status))
    {
      Ok(action) => action,
      Err(status) => {
        let _ = send(Err(status)).await;
        return;
      }
    };
    final_action = Some(action);
    if send(Ok(MouseMotionEvent::Progress(proto::MouseMotionProgress {
      sample_index: index as u32,
      point: Some(raw_screen_point_to_proto(sample.point)),
      scheduled_elapsed: Some(duration_to_proto(sample.elapsed)),
    })))
    .await
    .is_err()
    {
      return;
    }
    next_index = index + 1;
  }
  let Some(last) = samples.last() else { return };
  let action = match final_action {
    Some(action) => match input_action_to_proto(action) {
      Ok(action) => action,
      Err(status) => {
        let _ = send(Err(status)).await;
        return;
      }
    },
    None => {
      let _ = send(Err(Status::internal("mouse motion completed without delivery evidence"))).await;
      return;
    }
  };
  let _ = send(Ok(MouseMotionEvent::Completed(proto::MouseMotionCompleted {
    point: Some(raw_screen_point_to_proto(last.point)),
    action: Some(action),
  })))
  .await;
}

fn latest_due_mouse_sample(samples: &[auv_driver::MouseMotionSample], next_index: usize, elapsed: std::time::Duration) -> usize {
  let mut index = next_index;
  while index + 1 < samples.len() && samples[index + 1].elapsed <= elapsed {
    index += 1;
  }
  index
}

fn move_mouse_stream_event(event: MouseMotionEvent) -> proto::MoveMouseStreamResponse {
  use proto::move_mouse_stream_response::Event;
  proto::MoveMouseStreamResponse {
    event: Some(match event {
      MouseMotionEvent::Started(value) => Event::Started(value),
      MouseMotionEvent::Progress(value) => Event::Progress(value),
      MouseMotionEvent::Completed(value) => Event::Completed(value),
    }),
  }
}

fn stream_mouse_motion_event(event: MouseMotionEvent) -> proto::StreamMouseMotionResponse {
  use proto::stream_mouse_motion_response::Event;
  proto::StreamMouseMotionResponse {
    event: Some(match event {
      MouseMotionEvent::Started(value) => Event::Started(value),
      MouseMotionEvent::Progress(value) => Event::Progress(value),
      MouseMotionEvent::Completed(value) => Event::Completed(value),
    }),
  }
}

async fn collect_mouse_motion<S>(
  requests: &mut S,
  sender: &tokio::sync::mpsc::Sender<Result<proto::StreamMouseMotionResponse, Status>>,
) -> Result<Option<auv_driver::MouseMotionPlan>, Status>
where
  S: Stream<Item = Result<proto::StreamMouseMotionRequest, Status>> + Unpin,
{
  // TODO(mouse-motion-live-batches): V1 buffers until finish because duration
  // applies to the complete curve. Execute batches before finish only after a
  // per-batch or absolute timing contract receives owner approval.
  let mut begin = None;
  let mut segments = Vec::new();
  let mut next_sequence = 0;
  while let Some(request) = requests.next().await.transpose()? {
    use proto::stream_mouse_motion_request::Event;
    match request.event.ok_or_else(|| Status::invalid_argument("moveMouse event is required"))? {
      Event::Begin(value) if begin.is_none() => {
        begin = Some(value);
        sender
          .send(Ok(proto::StreamMouseMotionResponse {
            event: Some(proto::stream_mouse_motion_response::Event::Accepted(proto::StreamMouseMotionAccepted { next_sequence })),
          }))
          .await
          .map_err(|_| Status::cancelled("moveMouse client disconnected"))?;
      }
      Event::Append(value) if begin.is_some() => {
        if value.sequence != next_sequence {
          return Err(Status::invalid_argument(format!("moveMouse append sequence must be {next_sequence}")));
        }
        if segments.len() + value.segments.len() > auv_driver::MOUSE_MOTION_MAX_SEGMENTS {
          return Err(Status::invalid_argument("moveMouse curve has too many segments"));
        }
        for segment in value.segments {
          segments.push(mouse_segment_from_proto(segment)?);
        }
        next_sequence += 1;
        sender
          .send(Ok(proto::StreamMouseMotionResponse {
            event: Some(proto::stream_mouse_motion_response::Event::Accepted(proto::StreamMouseMotionAccepted { next_sequence })),
          }))
          .await
          .map_err(|_| Status::cancelled("moveMouse client disconnected"))?;
      }
      Event::Finish(_) if begin.is_some() => {
        let begin = begin.take().expect("guarded begin");
        return Ok(Some(mouse_motion_plan_from_parts(begin, segments)?));
      }
      Event::Cancel(_) if begin.is_some() => {
        let _ = sender
          .send(Ok(proto::StreamMouseMotionResponse {
            event: Some(proto::stream_mouse_motion_response::Event::Cancelled(proto::MouseMotionCancelled {})),
          }))
          .await;
        return Ok(None);
      }
      Event::Begin(_) => return Err(Status::invalid_argument("moveMouse begin must be the first and only begin event")),
      Event::Append(_) => return Err(Status::invalid_argument("moveMouse append requires begin")),
      Event::Finish(_) => return Err(Status::invalid_argument("moveMouse finish requires begin")),
      Event::Cancel(_) => return Err(Status::invalid_argument("moveMouse cancel requires begin")),
    }
  }
  Err(Status::invalid_argument("moveMouse stream ended before finish or cancel"))
}

fn mouse_motion_plan_from_proto(value: proto::MouseMotionPlan) -> Result<auv_driver::MouseMotionPlan, Status> {
  let curve = value.curve.ok_or_else(|| Status::invalid_argument("plan.curve is required"))?;
  Ok(auv_driver::MouseMotionPlan {
    start: mouse_start_from_proto(value.start.ok_or_else(|| Status::invalid_argument("plan.start is required"))?)?,
    curve: auv_driver::MouseCurve {
      start: mouse_curve_point_from_proto(curve.start.ok_or_else(|| Status::invalid_argument("plan.curve.start is required"))?),
      segments: curve.segments.into_iter().map(mouse_segment_from_proto).collect::<Result<_, _>>()?,
    },
    mapping: mouse_mapping_from_proto(value.mapping.ok_or_else(|| Status::invalid_argument("plan.mapping is required"))?),
    options: mouse_options_from_proto(value.options.ok_or_else(|| Status::invalid_argument("plan.options is required"))?)?,
  })
}

fn mouse_motion_plan_from_parts(
  value: proto::StreamMouseMotionBegin,
  segments: Vec<auv_driver::MouseCubicBezierSegment>,
) -> Result<auv_driver::MouseMotionPlan, Status> {
  Ok(auv_driver::MouseMotionPlan {
    start: mouse_start_from_proto(value.start.ok_or_else(|| Status::invalid_argument("begin.start is required"))?)?,
    curve: auv_driver::MouseCurve {
      start: mouse_curve_point_from_proto(value.curve_start.ok_or_else(|| Status::invalid_argument("begin.curve_start is required"))?),
      segments,
    },
    mapping: mouse_mapping_from_proto(value.mapping.ok_or_else(|| Status::invalid_argument("begin.mapping is required"))?),
    options: mouse_options_from_proto(value.options.ok_or_else(|| Status::invalid_argument("begin.options is required"))?)?,
  })
}

fn mouse_start_from_proto(value: proto::MouseStart) -> Result<auv_driver::MouseStart, Status> {
  match value.source.ok_or_else(|| Status::invalid_argument("mouse start source is required"))? {
    proto::mouse_start::Source::Point(point) => Ok(auv_driver::MouseStart::Screen(screen_point_from_proto(point)?.point())),
    proto::mouse_start::Source::Current(_) => Ok(auv_driver::MouseStart::Current),
  }
}

fn mouse_curve_point_from_proto(value: proto::MouseCurvePoint) -> auv_driver::Point {
  auv_driver::Point::new(value.x, value.y)
}

fn mouse_segment_from_proto(value: proto::MouseCubicBezierSegment) -> Result<auv_driver::MouseCubicBezierSegment, Status> {
  Ok(auv_driver::MouseCubicBezierSegment {
    control_1: mouse_curve_point_from_proto(value.control_1.ok_or_else(|| Status::invalid_argument("segment.control_1 is required"))?),
    control_2: mouse_curve_point_from_proto(value.control_2.ok_or_else(|| Status::invalid_argument("segment.control_2 is required"))?),
    end: mouse_curve_point_from_proto(value.end.ok_or_else(|| Status::invalid_argument("segment.end is required"))?),
  })
}

fn mouse_mapping_from_proto(value: proto::MouseCurveMapping) -> auv_driver::MouseCurveMapping {
  auv_driver::MouseCurveMapping {
    width: value.width,
    height: value.height,
  }
}

fn mouse_options_from_proto(value: proto::MouseMotionOptions) -> Result<auv_driver::MouseMotionOptions, Status> {
  Ok(auv_driver::MouseMotionOptions {
    duration: duration_from_proto(value.duration, std::time::Duration::ZERO, "mouse options.duration")?,
    sample_rate_hz: value.sample_rate_hz,
  })
}

fn raw_screen_point_to_proto(point: auv_driver::Point) -> proto::ScreenPoint {
  proto::ScreenPoint {
    x: point.x,
    y: point.y,
  }
}

fn duration_to_proto(value: std::time::Duration) -> prost_types::Duration {
  prost_types::Duration {
    seconds: value.as_secs() as i64,
    nanos: value.subsec_nanos() as i32,
  }
}

fn screen_rect_from_proto(rect: proto::ScreenRect) -> Result<auv_driver::Rect, Status> {
  if !rect.x.is_finite()
    || !rect.y.is_finite()
    || !rect.width.is_finite()
    || !rect.height.is_finite()
    || rect.width <= 0.0
    || rect.height <= 0.0
  {
    return Err(Status::invalid_argument("overlay rect must have finite coordinates and positive finite dimensions"));
  }
  Ok(auv_driver::Rect::new(rect.x, rect.y, rect.width, rect.height))
}

fn screen_point_to_proto(point: auv_driver::ScreenPoint) -> proto::ScreenPoint {
  let point = point.point();
  proto::ScreenPoint {
    x: point.x,
    y: point.y,
  }
}

fn window_point_to_proto(point: auv_driver::WindowPoint) -> proto::WindowPoint {
  let point = point.point();
  proto::WindowPoint {
    x: point.x,
    y: point.y,
  }
}

fn click_options_from_proto(options: Option<proto::ClickOptions>) -> Result<auv_driver::ClickOptions, Status> {
  let Some(options) = options else {
    return Ok(auv_driver::ClickOptions::default());
  };
  let click = click_from_proto(options.click)?;
  Ok(auv_driver::ClickOptions {
    policy: input_policy_from_proto(options.policy)?,
    click,
    window_strategy: match proto::WindowClickStrategy::try_from(options.window_strategy) {
      Ok(proto::WindowClickStrategy::Unspecified | proto::WindowClickStrategy::ChromiumCompatible) => {
        auv_driver::WindowClickStrategy::ChromiumCompatible
      }
      Ok(proto::WindowClickStrategy::PidTargeted) => auv_driver::WindowClickStrategy::PidTargeted,
      Err(_) => return Err(Status::invalid_argument("options.window_strategy is unknown")),
    },
  })
}

fn screen_click_options_from_proto(options: Option<proto::ScreenClickOptions>) -> Result<auv_driver::Click, Status> {
  let options = options.ok_or_else(|| Status::invalid_argument("options are required"))?;
  click_from_proto(options.click)
}

fn click_from_proto(click: Option<proto::Click>) -> Result<auv_driver::Click, Status> {
  Ok(match click {
    None => auv_driver::Click::Single,
    Some(click) => {
      if click.count == 0 || click.count > u32::from(u8::MAX) {
        return Err(Status::invalid_argument("options.click.count must be in [1, 255]"));
      }
      match click.count {
        1 => {
          let interval = duration_from_proto(click.interval, std::time::Duration::ZERO, "options.click.interval")?;
          if !interval.is_zero() {
            return Err(Status::invalid_argument("options.click.interval must be absent or zero for a single click"));
          }
          auv_driver::Click::Single
        }
        2 => {
          let interval = required_positive_duration(click.interval, "options.click.interval")?;
          auv_driver::Click::Double { interval }
        }
        count => {
          let interval = required_positive_duration(click.interval, "options.click.interval")?;
          auv_driver::Click::Repeated {
            count: u8::try_from(count).expect("validated click count fits u8"),
            interval,
          }
        }
      }
    }
  })
}

fn type_text_options_from_proto(options: Option<proto::TypeTextOptions>) -> Result<auv_driver::TypeTextOptions, Status> {
  let Some(options) = options else {
    return Ok(auv_driver::TypeTextOptions::default());
  };
  Ok(auv_driver::TypeTextOptions {
    policy: input_policy_from_proto(options.policy)?,
    replace_existing: options.replace_existing,
    submit: match proto::TextSubmit::try_from(options.submit) {
      Ok(proto::TextSubmit::Unspecified | proto::TextSubmit::None) => auv_driver::TextSubmit::No,
      Ok(proto::TextSubmit::Return) => auv_driver::TextSubmit::Return,
      Ok(proto::TextSubmit::Search) => auv_driver::TextSubmit::Search,
      Ok(proto::TextSubmit::Done) => auv_driver::TextSubmit::Done,
      Ok(proto::TextSubmit::Go) => auv_driver::TextSubmit::Go,
      Err(_) => return Err(Status::invalid_argument("options.submit is unknown")),
    },
    inter_char_delay: duration_from_proto(
      options.inter_char_delay,
      auv_driver::TypeTextOptions::default().inter_char_delay,
      "options.inter_char_delay",
    )?,
    allow_clipboard_fallback: options.allow_clipboard_fallback,
    settle: duration_from_proto(options.settle, std::time::Duration::ZERO, "options.settle")?,
  })
}

fn paste_text_options_from_proto(text: String, options: Option<proto::PasteTextOptions>) -> Result<auv_driver::PasteTextOptions, Status> {
  if text.is_empty() {
    return Err(Status::invalid_argument("text is required"));
  }
  let options = options.ok_or_else(|| Status::invalid_argument("options are required"))?;
  let submit = match proto::TextSubmit::try_from(options.submit) {
    Ok(proto::TextSubmit::Unspecified | proto::TextSubmit::None) => auv_driver::TextSubmit::No,
    Ok(proto::TextSubmit::Return) => auv_driver::TextSubmit::Return,
    Ok(proto::TextSubmit::Search) => auv_driver::TextSubmit::Search,
    Ok(proto::TextSubmit::Done) => auv_driver::TextSubmit::Done,
    Ok(proto::TextSubmit::Go) => auv_driver::TextSubmit::Go,
    Err(_) => return Err(Status::invalid_argument("options.submit is unknown")),
  };
  Ok(auv_driver::PasteTextOptions {
    text,
    replace_existing: options.replace_existing,
    submit,
    settle: duration_from_proto(options.settle, std::time::Duration::ZERO, "options.settle")?,
  })
}

fn input_policy_from_proto(policy: i32) -> Result<auv_driver::InputPolicy, Status> {
  match proto::InputPolicy::try_from(policy) {
    Ok(proto::InputPolicy::Unspecified | proto::InputPolicy::BackgroundPreferred) => Ok(auv_driver::InputPolicy::BackgroundPreferred),
    Ok(proto::InputPolicy::BackgroundOnly) => Ok(auv_driver::InputPolicy::BackgroundOnly),
    Ok(proto::InputPolicy::ForegroundPreferred) => Ok(auv_driver::InputPolicy::ForegroundPreferred),
    Err(_) => Err(Status::invalid_argument("options.policy is unknown")),
  }
}

fn required_positive_duration(value: Option<prost_types::Duration>, field: &'static str) -> Result<std::time::Duration, Status> {
  let value = value.ok_or_else(|| Status::invalid_argument(format!("{field} is required for multiple clicks")))?;
  let duration = duration_from_proto(Some(value), std::time::Duration::ZERO, field)?;
  if duration.is_zero() {
    return Err(Status::invalid_argument(format!("{field} must be positive")));
  }
  Ok(duration)
}

fn duration_from_proto(
  value: Option<prost_types::Duration>,
  default: std::time::Duration,
  field: &'static str,
) -> Result<std::time::Duration, Status> {
  let Some(value) = value else {
    return Ok(default);
  };
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(Status::invalid_argument(format!("{field} must be a non-negative protobuf Duration")));
  }
  Ok(std::time::Duration::new(
    u64::try_from(value.seconds).expect("validated seconds are non-negative"),
    u32::try_from(value.nanos).expect("validated nanos fit u32"),
  ))
}

fn input_action_to_proto(action: auv_driver::InputActionResult) -> Result<proto::InputActionResult, Status> {
  action.validate().map_err(|error| Status::internal(format!("driver returned invalid InputActionResult: {error}")))?;
  Ok(proto::InputActionResult {
    selected_path: input_delivery_path_to_proto(action.selected_path) as i32,
    attempts: action
      .attempts
      .into_iter()
      .map(|attempt| proto::InputAttempt {
        path: input_delivery_path_to_proto(attempt.path) as i32,
        succeeded: attempt.succeeded,
        message: attempt.message,
      })
      .collect(),
    mouse_disturbance: disturbance_to_proto(action.mouse_disturbance) as i32,
    focus_disturbance: disturbance_to_proto(action.focus_disturbance) as i32,
    clipboard_disturbance: disturbance_to_proto(action.clipboard_disturbance) as i32,
  })
}

fn input_delivery_path_to_proto(path: auv_driver::InputDeliveryPath) -> proto::InputDeliveryPath {
  match path {
    auv_driver::InputDeliveryPath::Noop => proto::InputDeliveryPath::Noop,
    auv_driver::InputDeliveryPath::AxPress => proto::InputDeliveryPath::AxPress,
    auv_driver::InputDeliveryPath::AxFocus => proto::InputDeliveryPath::AxFocus,
    auv_driver::InputDeliveryPath::AxSetValue => proto::InputDeliveryPath::AxSetValue,
    auv_driver::InputDeliveryPath::AxScroll => proto::InputDeliveryPath::AxScroll,
    auv_driver::InputDeliveryPath::AxSelectedText => proto::InputDeliveryPath::AxSelectedText,
    auv_driver::InputDeliveryPath::WindowTargetedMouse => proto::InputDeliveryPath::WindowTargetedMouse,
    auv_driver::InputDeliveryPath::WindowTargetedWheel => proto::InputDeliveryPath::WindowTargetedWheel,
    auv_driver::InputDeliveryPath::WindowTargetedKeyboard => proto::InputDeliveryPath::WindowTargetedKeyboard,
    auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll => proto::InputDeliveryPath::WindowTargetedKeyboardScroll,
    auv_driver::InputDeliveryPath::ClipboardPaste => proto::InputDeliveryPath::ClipboardPaste,
    auv_driver::InputDeliveryPath::ForegroundSystemEvents => proto::InputDeliveryPath::ForegroundSystemEvents,
    auv_driver::InputDeliveryPath::Unsupported => proto::InputDeliveryPath::Unsupported,
  }
}

fn disturbance_to_proto(level: auv_driver::DisturbanceLevel) -> proto::DisturbanceLevel {
  match level {
    auv_driver::DisturbanceLevel::None => proto::DisturbanceLevel::None,
    auv_driver::DisturbanceLevel::Temporary => proto::DisturbanceLevel::Temporary,
    auv_driver::DisturbanceLevel::Foreground => proto::DisturbanceLevel::Foreground,
    auv_driver::DisturbanceLevel::Unknown => proto::DisturbanceLevel::Unknown,
  }
}

#[tonic::async_trait]
impl TextRecognitionService for LocalTextRecognitionService {
  async fn recognize_text(&self, request: Request<proto::RecognizeTextRequest>) -> Result<Response<proto::RecognizeTextResponse>, Status> {
    let request = request.into_inner();
    let capture = capture_from_proto(request.capture.ok_or_else(|| Status::invalid_argument("capture is required"))?)?;
    let region = ratio_rect_from_proto(request.region)?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(&capture, region, recognition_options(request.custom_words, request.recognition_languages))
      .map_err(driver_status)?;
    Ok(Response::new(recognition_to_proto(recognition)))
  }

  async fn find_window_text(
    &self,
    request: Request<proto::FindWindowTextRequest>,
  ) -> Result<Response<proto::FindWindowTextResponse>, Status> {
    let request = request.into_inner();
    if request.query.trim().is_empty() {
      return Err(Status::invalid_argument("query is required"));
    }
    let region = ratio_rect_from_proto(request.region)?;
    let window = resolve_window_ref(&self.session, request.window.ok_or_else(|| Status::invalid_argument("window is required"))?)?;
    let capture = self.session.window().capture(&window).map_err(driver_status)?;
    let matches = self
      .session
      .vision()
      .find_text_in_capture_with_options(
        &capture,
        &request.query,
        region,
        recognition_options(request.custom_words, request.recognition_languages),
      )
      .map_err(driver_status)?;
    Ok(Response::new(proto::FindWindowTextResponse {
      window: Some(window_to_proto(window)),
      matches: matches
        .matches
        .into_iter()
        .map(|matched| proto::TextMatch {
          text: matched.text,
          bounds: Some(rect_to_proto(matched.bounds)),
          confidence: matched.confidence,
        })
        .collect(),
      capture: Some(capture_to_proto(capture)),
    }))
  }

  async fn find_display_text(
    &self,
    request: Request<proto::FindDisplayTextRequest>,
  ) -> Result<Response<proto::FindDisplayTextResponse>, Status> {
    let request = request.into_inner();
    if request.query.trim().is_empty() {
      return Err(Status::invalid_argument("query is required"));
    }
    let display = display_selector_from_proto(request.selector)?;
    let region = ratio_rect_from_proto(request.region)?;
    let captured = self
      .session
      .display()
      .capture(auv_driver::CaptureOptions {
        display,
        ..Default::default()
      })
      .map_err(driver_status)?;
    let matches = self
      .session
      .vision()
      .find_text_in_capture_with_options(
        &captured.capture,
        &request.query,
        region,
        recognition_options(request.custom_words, request.recognition_languages),
      )
      .map_err(driver_status)?;
    Ok(Response::new(proto::FindDisplayTextResponse {
      display: Some(display_to_proto(captured.display)),
      matches: matches
        .matches
        .into_iter()
        .map(|matched| proto::TextMatch {
          text: matched.text,
          bounds: Some(rect_to_proto(matched.bounds)),
          confidence: matched.confidence,
        })
        .collect(),
      capture: Some(capture_to_proto(captured.capture)),
    }))
  }
}

fn recognition_options(custom_words: Vec<String>, recognition_languages: Vec<String>) -> auv_driver::TextRecognitionOptions {
  auv_driver::TextRecognitionOptions {
    custom_words,
    recognition_languages: (!recognition_languages.is_empty()).then_some(recognition_languages),
  }
}

fn recognition_to_proto(recognition: auv_driver::TextRecognition) -> proto::RecognizeTextResponse {
  proto::RecognizeTextResponse {
    text: recognition.text,
    regions: recognition
      .regions
      .into_iter()
      .map(|region| proto::RecognizedText {
        text: region.text,
        bounds: Some(rect_to_proto(region.bounds)),
        confidence: region.confidence,
      })
      .collect(),
  }
}

fn capture_from_proto(capture: proto::CapturedFrame) -> Result<auv_driver::Capture, Status> {
  let image = capture.image.ok_or_else(|| Status::invalid_argument("capture.image is required"))?;
  let expected = usize::try_from(image.width)
    .ok()
    .and_then(|width| usize::try_from(image.height).ok().and_then(|height| width.checked_mul(height)))
    .and_then(|pixels| pixels.checked_mul(4))
    .ok_or_else(|| Status::invalid_argument("capture.image dimensions overflow"))?;
  if image.data.len() != expected {
    return Err(Status::invalid_argument(format!(
      "capture.image.data has {} bytes; expected {expected} for {}x{} RGBA8",
      image.data.len(),
      image.width,
      image.height
    )));
  }
  let image = image::RgbaImage::from_raw(image.width, image.height, image.data)
    .ok_or_else(|| Status::invalid_argument("capture.image is not valid RGBA8"))?;
  let bounds = rect_from_proto(capture.bounds.ok_or_else(|| Status::invalid_argument("capture.bounds is required"))?, "capture.bounds")?;
  if !capture.scale_factor.is_finite() || capture.scale_factor <= 0.0 {
    return Err(Status::invalid_argument("capture.scale_factor must be finite and positive"));
  }
  Ok(auv_driver::Capture {
    image,
    bounds,
    scale_factor: capture.scale_factor,
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

fn ratio_rect_from_proto(region: Option<auv_api_proto::auv::api::image::v1::NormalizedRect>) -> Result<auv_driver::RatioRect, Status> {
  let Some(region) = region else {
    return Ok(auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0));
  };
  let values = [region.x, region.y, region.width, region.height];
  if values.iter().any(|value| !value.is_finite())
    || region.x < 0.0
    || region.y < 0.0
    || region.width <= 0.0
    || region.height <= 0.0
    || region.x + region.width > 1.0
    || region.y + region.height > 1.0
  {
    return Err(Status::invalid_argument("region must be a finite, positive rectangle inside normalized image bounds"));
  }
  Ok(auv_driver::RatioRect::new(region.x, region.y, region.width, region.height))
}

fn rect_from_proto(rect: proto::ScreenRect, field: &'static str) -> Result<auv_driver::Rect, Status> {
  let values = [rect.x, rect.y, rect.width, rect.height];
  if values.iter().any(|value| !value.is_finite()) || rect.width <= 0.0 || rect.height <= 0.0 {
    return Err(Status::invalid_argument(format!("{field} must be finite with positive width and height")));
  }
  Ok(auv_driver::Rect::new(rect.x, rect.y, rect.width, rect.height))
}

fn rect_to_proto(rect: auv_driver::Rect) -> proto::ScreenRect {
  proto::ScreenRect {
    x: rect.origin.x,
    y: rect.origin.y,
    width: rect.size.width,
    height: rect.size.height,
  }
}

#[tonic::async_trait]
impl CaptureService for LocalCaptureService {
  async fn capture_window(&self, request: Request<proto::CaptureWindowRequest>) -> Result<Response<proto::CaptureWindowResponse>, Status> {
    let request = request.into_inner();
    let window = resolve_window_ref(&self.session, request.window.ok_or_else(|| Status::invalid_argument("window is required"))?)?;
    let capture = self.session.window().capture(&window).map_err(driver_status)?;
    Ok(Response::new(proto::CaptureWindowResponse {
      window: Some(window_to_proto(window)),
      capture: Some(capture_to_proto(capture)),
    }))
  }

  async fn capture_display(
    &self,
    request: Request<proto::CaptureDisplayRequest>,
  ) -> Result<Response<proto::CaptureDisplayResponse>, Status> {
    let display = display_selector_from_proto(request.into_inner().selector)?;
    let captured = self
      .session
      .display()
      .capture(auv_driver::CaptureOptions {
        display,
        ..Default::default()
      })
      .map_err(driver_status)?;
    Ok(Response::new(proto::CaptureDisplayResponse {
      display: Some(display_to_proto(captured.display)),
      capture: Some(capture_to_proto(captured.capture)),
    }))
  }

  async fn capture_region(&self, request: Request<proto::CaptureRegionRequest>) -> Result<Response<proto::CaptureRegionResponse>, Status> {
    let request = request.into_inner();
    let display = display_selector_from_proto(request.selector)?;
    let region = rect_from_proto(request.region.ok_or_else(|| Status::invalid_argument("region is required"))?, "region")?;
    let captured = self
      .session
      .display()
      .capture_region(auv_driver::CaptureOptions {
        display,
        region: Some(region),
        ..Default::default()
      })
      .map_err(driver_status)?;
    Ok(Response::new(proto::CaptureRegionResponse {
      display: Some(display_to_proto(captured.display)),
      capture: Some(capture_to_proto(captured.capture)),
    }))
  }
}

fn display_selector_from_proto(selector: Option<proto::DisplaySelector>) -> Result<Option<String>, Status> {
  match selector.and_then(|selector| selector.selector) {
    None => Ok(None),
    Some(proto::display_selector::Selector::Display(display)) if !display.display_id.trim().is_empty() => Ok(Some(display.display_id)),
    Some(proto::display_selector::Selector::Name(name)) if !name.trim().is_empty() => Ok(Some(name)),
    _ => Err(Status::invalid_argument("display selector must contain a non-empty display id or name")),
  }
}

#[tonic::async_trait]
impl WindowService for LocalWindowService {
  async fn list_windows(&self, _request: Request<proto::ListWindowsRequest>) -> Result<Response<proto::ListWindowsResponse>, Status> {
    let windows = self.session.window().list().map_err(driver_status)?.into_iter().map(window_to_proto).collect();
    Ok(Response::new(proto::ListWindowsResponse { windows }))
  }

  async fn resolve_window(&self, request: Request<proto::ResolveWindowRequest>) -> Result<Response<proto::ResolveWindowResponse>, Status> {
    let selector = selector_from_proto(request.into_inner().selector.ok_or_else(|| Status::invalid_argument("selector is required"))?)?;
    let window = self.session.window().resolve(selector).map_err(driver_status)?;
    Ok(Response::new(proto::ResolveWindowResponse {
      window: Some(window_to_proto(window)),
    }))
  }
}

fn selector_from_proto(selector: proto::WindowSelector) -> Result<auv_driver::WindowSelector, Status> {
  let app = match selector.application {
    Some(proto::window_selector::Application::ApplicationBundleId(value)) if !value.trim().is_empty() => auv_driver::AppSelector {
      bundle: Some(auv_driver::TextMatcher::Exact(value)),
      ..Default::default()
    },
    Some(proto::window_selector::Application::ApplicationName(value)) if !value.trim().is_empty() => auv_driver::AppSelector {
      name: Some(auv_driver::TextMatcher::Exact(value)),
      ..Default::default()
    },
    Some(proto::window_selector::Application::ProcessId(value)) if value > 0 => auv_driver::AppSelector {
      process_id: Some(value),
      ..Default::default()
    },
    Some(proto::window_selector::Application::FrontmostApplication(true)) => auv_driver::AppSelector {
      frontmost: true,
      ..Default::default()
    },
    _ => return Err(Status::invalid_argument("selector.application must identify an application")),
  };
  let mut result = auv_driver::WindowSelector {
    app: Some(app),
    ..Default::default()
  };
  match selector.window {
    Some(proto::window_selector::Window::TitleExact(value)) if !value.trim().is_empty() => {
      result.title = Some(auv_driver::TextMatcher::Exact(value));
    }
    Some(proto::window_selector::Window::TitleContains(value)) if !value.trim().is_empty() => {
      result.title = Some(auv_driver::TextMatcher::Contains(value));
    }
    Some(proto::window_selector::Window::MainVisible(true)) => result.main_visible = true,
    _ => return Err(Status::invalid_argument("selector.window must identify a window")),
  }
  Ok(result)
}

fn window_to_proto(window: auv_driver::Window) -> proto::Window {
  proto::Window {
    r#ref: Some(proto::WindowRef {
      window_id: window.reference.id,
    }),
    title: window.title,
    application_name: window.app_name,
    application_bundle_id: window.app_bundle_id,
    process_id: window.process_id,
    frame: Some(proto::ScreenRect {
      x: window.frame.origin.x,
      y: window.frame.origin.y,
      width: window.frame.size.width,
      height: window.frame.size.height,
    }),
    is_main: window.is_main,
    is_visible: window.is_visible,
  }
}

fn display_to_proto(display: auv_driver::Display) -> proto::Display {
  proto::Display {
    display_id: display.id,
    name: display.name,
    frame: Some(proto::ScreenRect {
      x: display.frame.origin.x,
      y: display.frame.origin.y,
      width: display.frame.size.width,
      height: display.frame.size.height,
    }),
    scale_factor: display.scale_factor,
    primary: display.is_primary,
    builtin: display.is_builtin,
  }
}

fn capture_to_proto(capture: auv_driver::Capture) -> proto::CapturedFrame {
  let width = capture.image.width();
  let height = capture.image.height();
  proto::CapturedFrame {
    image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
      width,
      height,
      data: capture.image.into_raw(),
    }),
    bounds: Some(proto::ScreenRect {
      x: capture.bounds.origin.x,
      y: capture.bounds.origin.y,
      width: capture.bounds.size.width,
      height: capture.bounds.size.height,
    }),
    scale_factor: capture.scale_factor,
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  }
}

fn driver_status(error: auv_driver::DriverError) -> Status {
  match error {
    auv_driver::DriverError::Unsupported { .. } => Status::unimplemented(error.to_string()),
    auv_driver::DriverError::NotFound { .. } => Status::not_found(error.to_string()),
    auv_driver::DriverError::PermissionDenied { .. } => Status::permission_denied(error.to_string()),
    auv_driver::DriverError::InvalidInput { .. } => Status::invalid_argument(error.to_string()),
    auv_driver::DriverError::StaleObservation { .. } | auv_driver::DriverError::RoleMismatch { .. } => {
      Status::failed_precondition(error.to_string())
    }
    auv_driver::DriverError::Backend { .. } => Status::unavailable(error.to_string()),
  }
}

#[tonic::async_trait]
impl DisplayService for LocalDisplayService {
  async fn list_displays(&self, _request: Request<proto::ListDisplaysRequest>) -> Result<Response<proto::ListDisplaysResponse>, Status> {
    let observed = self.session.display().list().map_err(driver_status)?;
    let displays = observed.displays.into_iter().map(display_to_proto).collect();
    Ok(Response::new(proto::ListDisplaysResponse { displays }))
  }
}

#[cfg(unix)]
/// Serves the first-party local driver over the daemon-inherited socket.
///
pub(super) async fn serve_inherited() -> Result<(), String> {
  let (incoming, parent_disconnected) = auv_api_server::runner_transport::inherited_transport()?.into_parts();

  let driver = auv_driver::LocalDriver::new();
  #[cfg(target_os = "linux")]
  let driver = match std::env::var_os(super::STATE_ROOT_ENV) {
    Some(root) => driver.with_linux_portal_state_root(std::path::PathBuf::from(root).join("portal")),
    None => driver,
  };
  let session = driver.open_local().map_err(|error| format!("failed to open local driver: {error}"))?;
  let display = DisplayServiceServer::new(LocalDisplayService {
    session: session.clone(),
  });
  let window = WindowServiceServer::new(LocalWindowService {
    session: session.clone(),
  });
  let text_recognition = TextRecognitionServiceServer::new(LocalTextRecognitionService {
    session: session.clone(),
  })
  .max_decoding_message_size(auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED)
  .max_encoding_message_size(auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED);
  let input = InputServiceServer::new(LocalInputService {
    session: session.clone(),
    mouse_motion: std::sync::Arc::new(tokio::sync::Mutex::new(())),
  });
  let permission = PermissionServiceServer::new(LocalPermissionService {
    session: session.clone(),
  });
  let application = ApplicationServiceServer::new(LocalApplicationService {
    session: session.clone(),
  });
  let accessibility = AccessibilityServiceServer::new(LocalAccessibilityService {
    session: session.clone(),
  });
  let media_control = MediaControlServiceServer::new(LocalMediaControlService);
  let overlay = OverlayServiceServer::new(LocalOverlayService {
    session: session.clone(),
    owner_thread: std::thread::current().id(),
  });
  let capture =
    CaptureServiceServer::new(LocalCaptureService { session }).max_encoding_message_size(auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<DisplayServiceServer<LocalDisplayService>>().await;
  health_reporter.set_serving::<WindowServiceServer<LocalWindowService>>().await;
  health_reporter.set_serving::<CaptureServiceServer<LocalCaptureService>>().await;
  health_reporter.set_serving::<TextRecognitionServiceServer<LocalTextRecognitionService>>().await;
  health_reporter.set_serving::<InputServiceServer<LocalInputService>>().await;
  #[cfg(target_os = "macos")]
  health_reporter.set_serving::<PermissionServiceServer<LocalPermissionService>>().await;
  #[cfg(target_os = "macos")]
  health_reporter.set_serving::<ApplicationServiceServer<LocalApplicationService>>().await;
  #[cfg(target_os = "macos")]
  health_reporter.set_serving::<AccessibilityServiceServer<LocalAccessibilityService>>().await;
  #[cfg(target_os = "macos")]
  health_reporter.set_serving::<MediaControlServiceServer<LocalMediaControlService>>().await;
  #[cfg(target_os = "macos")]
  health_reporter.set_serving::<OverlayServiceServer<LocalOverlayService>>().await;
  let mut served_services = vec![
    "auv.api.driver.v1.DisplayService",
    "auv.api.driver.v1.WindowService",
    "auv.api.driver.v1.CaptureService",
    "auv.api.driver.v1.TextRecognitionService",
    "auv.api.driver.v1.InputService",
  ];
  #[cfg(target_os = "macos")]
  served_services.push("auv.api.driver.macos.v1.PermissionService");
  #[cfg(target_os = "macos")]
  served_services.push("auv.api.driver.macos.v1.ApplicationService");
  #[cfg(target_os = "macos")]
  served_services.push("auv.api.driver.macos.v1.AccessibilityService");
  #[cfg(target_os = "macos")]
  served_services.push("auv.api.driver.macos.v1.MediaControlService");
  #[cfg(target_os = "macos")]
  served_services.push("auv.api.driver.v1.OverlayService");
  let descriptor_set = auv_api_proto::descriptor_set_for_services(&served_services)?;
  let reflection = tonic_reflection::server::Builder::configure()
    .register_encoded_file_descriptor_set(&descriptor_set)
    .build_v1()
    .map_err(|error| format!("failed to build local Runner reflection: {error}"))?;

  tonic::transport::Server::builder()
    .add_service(health)
    .add_service(reflection)
    .add_service(display)
    .add_service(window)
    .add_service(capture)
    .add_service(text_recognition)
    .add_service(input)
    .add_service(permission)
    .add_service(application)
    .add_service(accessibility)
    .add_service(media_control)
    .add_service(overlay)
    .serve_with_incoming_shutdown(incoming, parent_disconnected)
    .await
    .map_err(|error| format!("Runner transport failed: {error}"))
}

#[cfg(not(unix))]
pub async fn serve_inherited() -> Result<(), String> {
  Err("the first local driver Runner requires Unix inherited-stream IPC".to_string())
}

#[cfg(test)]
#[path = "local_driver_test.rs"]
mod tests;
