//! First-party local driver Runner served only on a daemon-inherited stream.

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
use tonic::{Request, Response, Status};

use auv_driver::WindowInput as _;

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
    let request = request.into_inner();
    let point = screen_point_from_proto(request.point.ok_or_else(|| Status::invalid_argument("point is required"))?)?;
    let click = screen_click_options_from_proto(request.options)?;
    let action = self.session.input().click_at(point.point(), click).map_err(driver_status)?;
    Ok(Response::new(proto::ClickScreenPointResponse {
      point: Some(screen_point_to_proto(point)),
      action: Some(input_action_to_proto(action)?),
    }))
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
pub async fn serve_inherited() -> Result<(), String> {
  let (incoming, parent_disconnected) = auv_runner_protocol::inherited_transport()?.into_parts();

  let runtime = auv_runner_protocol::RuntimeControl::ready(auv_runner_protocol::RuntimeMetadata {
    runner_class: "auv.core.local".to_string(),
    display_name: "AUV local driver".to_string(),
    labels: Default::default(),
    operation_capacity: 16,
  })?;
  let runtime_service = runtime.service();

  let session = auv_driver::open_local().map_err(|error| format!("failed to open local driver: {error}"))?;
  let display = DisplayServiceServer::new(LocalDisplayService {
    session: session.clone(),
  });
  let window = WindowServiceServer::new(LocalWindowService {
    session: session.clone(),
  });
  let text_recognition = TextRecognitionServiceServer::new(LocalTextRecognitionService {
    session: session.clone(),
  })
  .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
  .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES);
  let input = InputServiceServer::new(LocalInputService {
    session: session.clone(),
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
    CaptureServiceServer::new(LocalCaptureService { session }).max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<DisplayServiceServer<LocalDisplayService>>().await;
  health_reporter.set_serving::<WindowServiceServer<LocalWindowService>>().await;
  health_reporter.set_serving::<CaptureServiceServer<LocalCaptureService>>().await;
  health_reporter.set_serving::<TextRecognitionServiceServer<LocalTextRecognitionService>>().await;
  health_reporter.set_serving::<InputServiceServer<LocalInputService>>().await;
  health_reporter
    .set_serving::<auv_api_proto::auv::api::runner::v1::runner_runtime_service_server::RunnerRuntimeServiceServer<
      auv_runner_protocol::RuntimeControl,
    >>()
    .await;
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
  let descriptor_set = auv_runner_protocol::RuntimeControl::descriptor_set_for_services(&served_services)?;
  let reflection = auv_runner_protocol::reflection_service(&descriptor_set)?;

  tonic::transport::Server::builder()
    .add_service(health)
    .add_service(reflection)
    .add_service(runtime_service)
    .add_service(runtime.track(display))
    .add_service(runtime.track(window))
    .add_service(runtime.track(capture))
    .add_service(runtime.track(text_recognition))
    .add_service(runtime.track(input))
    .add_service(runtime.track(permission))
    .add_service(runtime.track(application))
    .add_service(runtime.track(accessibility))
    .add_service(runtime.track(media_control))
    .add_service(runtime.track(overlay))
    .serve_with_incoming_shutdown(incoming, parent_disconnected)
    .await
    .map_err(|error| format!("Runner transport failed: {error}"))
}

#[cfg(not(unix))]
pub async fn serve_inherited() -> Result<(), String> {
  Err("the first local driver Runner requires Unix inherited-stream IPC".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

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
}
