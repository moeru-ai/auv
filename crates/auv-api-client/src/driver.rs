//! Hierarchical clients for one admitted Driver Runner.
//!
//! Each child keeps the Runner lease and any resolved resource reference. The
//! public hierarchy is independent of whether the daemon reaches a local child
//! process or a paired remote Device.

use auv_api_proto::auv::api::core::v1::RunnerLeaseRef;
use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
use auv_api_proto::auv::api::driver::v1 as proto;
use auv_api_proto::auv::api::image::v1::NormalizedRect;
use auv_api_proto::auv::api::inference::v1 as inference_proto;

use crate::Client;

// Placement is selected by `AuvClient`/`RunClient` before this lease-bound
// hierarchy is constructed. `AuvClient::local()` is the explicit local-only
// constraint; ordinary placement may resolve either a local or paired Device.

#[derive(Clone, Debug)]
pub struct RunnerClient {
  client: Client,
  lease: RunnerLeaseRef,
  runner: Option<auv_api_proto::auv::api::core::v1::Runner>,
}

impl RunnerClient {
  pub(crate) fn new(client: Client, lease: RunnerLeaseRef) -> Result<Self, tonic::Status> {
    if lease.lease_id.trim().is_empty() {
      return Err(tonic::Status::invalid_argument("Runner lease must include lease_id"));
    }
    Ok(Self {
      client,
      lease,
      runner: None,
    })
  }

  pub(crate) fn from_claim(
    client: Client,
    runner: auv_api_proto::auv::api::core::v1::Runner,
    lease: RunnerLeaseRef,
  ) -> Result<Self, tonic::Status> {
    let mut client = Self::new(client, lease)?;
    client.runner = Some(runner);
    Ok(client)
  }

  pub fn lease(&self) -> &RunnerLeaseRef {
    &self.lease
  }

  pub fn resource(&self) -> Option<&auv_api_proto::auv::api::core::v1::Runner> {
    self.runner.as_ref()
  }

  /// Builds the routed transport for an application-owned generated protobuf
  /// client while keeping the lease out of that application's messages.
  pub fn transport(&self) -> Result<crate::RunnerTransport, tonic::Status> {
    self.client.runner_transport(self.lease.clone())
  }

  pub fn displays(&self) -> DisplaysClient {
    DisplaysClient {
      runner: self.clone(),
    }
  }

  pub fn windows(&self) -> WindowsClient {
    WindowsClient {
      runner: self.clone(),
    }
  }

  pub fn input(&self) -> InputClient {
    InputClient {
      runner: self.clone(),
    }
  }

  pub fn overlay(&self) -> OverlayClient {
    OverlayClient {
      runner: self.clone(),
    }
  }

  pub fn macos(&self) -> MacosClient {
    MacosClient {
      runner: self.clone(),
    }
  }

  pub fn inference(&self) -> InferenceClient {
    InferenceClient {
      runner: self.clone(),
    }
  }

  /// Runs OCR against a capture already obtained from this Runner.
  pub async fn recognize_text(
    &self,
    capture: proto::CapturedFrame,
    region: Option<NormalizedRect>,
    custom_words: Vec<String>,
    recognition_languages: Vec<String>,
  ) -> Result<proto::RecognizeTextResponse, tonic::Status> {
    let mut client = self.client.clone();
    client
      .recognize_runner_text(
        self.lease.clone(),
        proto::RecognizeTextRequest {
          lease: None,
          capture: Some(capture),
          region,
          custom_words,
          recognition_languages,
        },
      )
      .await
  }

  pub async fn release(mut self) -> Result<bool, tonic::Status> {
    self.client.release_runner_lease(self.lease).await
  }
}

#[derive(Clone, Debug)]
pub struct OverlayClient {
  runner: RunnerClient,
}

impl OverlayClient {
  pub async fn show(
    &self,
    overlay: &auv_driver_overlay_common::Overlay,
    options: auv_driver_overlay_common::ShowOptions,
  ) -> Result<(), tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .show_runner_overlay(
        self.runner.lease.clone(),
        proto::ShowOverlayRequest {
          lease: None,
          overlay: Some(overlay_to_proto(overlay)?),
          options: Some(overlay_options_to_proto(options)?),
        },
      )
      .await?;
    Ok(())
  }

  pub async fn remove(&self) -> Result<(), tonic::Status> {
    let mut client = self.runner.client.clone();
    client.remove_runner_overlay(self.runner.lease.clone()).await?;
    Ok(())
  }
}

fn overlay_to_proto(value: &auv_driver_overlay_common::Overlay) -> Result<proto::Overlay, tonic::Status> {
  use auv_driver_overlay_common::Layer;
  let layers = value
    .layers()
    .iter()
    .map(|layer| {
      let layer = match layer {
        Layer::Cursor(value) => proto::overlay_layer::Layer::Cursor(proto::Cursor {
          point: Some(proto::ScreenPoint {
            x: value.point().point().x,
            y: value.point().point().y,
          }),
          label: value.label().map(ToOwned::to_owned),
          label_visible: value.label_visible(),
          image: Some(cursor_image_to_proto(value.image())?),
          style: Some(cursor_style_to_proto(value.style())),
        }),
        Layer::Outline(value) => proto::overlay_layer::Layer::Outline(proto::Outline {
          rect: Some(proto::ScreenRect {
            x: value.rect().origin.x,
            y: value.rect().origin.y,
            width: value.rect().size.width,
            height: value.rect().size.height,
          }),
          label: value.label().map(ToOwned::to_owned),
          label_visible: value.label_visible(),
          style: Some(outline_style_to_proto(value.style())),
        }),
        Layer::Status(value) => proto::overlay_layer::Layer::Status(proto::Status {
          point: Some(proto::ScreenPoint {
            x: value.point().point().x,
            y: value.point().point().y,
          }),
          text: value.text().to_string(),
          style: Some(status_style_to_proto(value.style())),
        }),
      };
      Ok(proto::OverlayLayer { layer: Some(layer) })
    })
    .collect::<Result<Vec<_>, tonic::Status>>()?;
  Ok(proto::Overlay { layers })
}

fn cursor_image_to_proto(value: &auv_driver_overlay_common::layers::CursorImage) -> Result<proto::CursorImage, tonic::Status> {
  use auv_driver_overlay_common::layers::{BuiltInCursor, CursorImage};
  let image = match value {
    CursorImage::BuiltIn { variant } => proto::cursor_image::Image::BuiltIn(match variant {
      BuiltInCursor::Auv => proto::BuiltInCursor::Auv as i32,
      BuiltInCursor::AuvClick => proto::BuiltInCursor::AuvClick as i32,
      BuiltInCursor::You => proto::BuiltInCursor::You as i32,
    }),
    CursorImage::Svg { source } if source.len() <= 256 * 1024 => proto::cursor_image::Image::Svg(source.clone()),
    CursorImage::Svg { .. } => return Err(tonic::Status::invalid_argument("cursor SVG exceeds 256 KiB")),
  };
  Ok(proto::CursorImage { image: Some(image) })
}

fn color_to_proto(value: auv_driver_overlay_common::style::Color) -> proto::Color {
  proto::Color {
    red: value.red,
    green: value.green,
    blue: value.blue,
    alpha: value.alpha,
  }
}

fn insets_to_proto(value: auv_driver_overlay_common::style::Insets) -> proto::Insets {
  proto::Insets {
    top: value.top,
    right: value.right,
    bottom: value.bottom,
    left: value.left,
  }
}

fn outline_style_to_proto(value: auv_driver_overlay_common::style::OutlineStyle) -> proto::OutlineStyle {
  proto::OutlineStyle {
    stroke: Some(proto::Stroke {
      color: Some(color_to_proto(value.stroke.color)),
      width: value.stroke.width,
    }),
    padding: Some(insets_to_proto(value.padding)),
    corner_radius: value.corner_radius,
  }
}

fn cursor_style_to_proto(value: auv_driver_overlay_common::style::CursorStyle) -> proto::CursorStyle {
  proto::CursorStyle {
    label_foreground: Some(color_to_proto(value.label_foreground)),
    label_background: Some(color_to_proto(value.label_background)),
    label_padding: Some(insets_to_proto(value.label_padding)),
    label_corner_radius: value.label_corner_radius,
    sprite_size: value.sprite_size,
    label_gap: value.label_gap,
  }
}

fn status_style_to_proto(value: auv_driver_overlay_common::style::StatusStyle) -> proto::StatusStyle {
  proto::StatusStyle {
    foreground: Some(color_to_proto(value.foreground)),
    background: Some(color_to_proto(value.background)),
    padding: Some(insets_to_proto(value.padding)),
    corner_radius: value.corner_radius,
  }
}

fn overlay_options_to_proto(value: auv_driver_overlay_common::ShowOptions) -> Result<proto::ShowOptions, tonic::Status> {
  let duration = |value: std::time::Duration| -> Result<prost_types::Duration, tonic::Status> {
    Ok(prost_types::Duration {
      seconds: i64::try_from(value.as_secs()).map_err(|_| tonic::Status::invalid_argument("overlay duration is too large"))?,
      nanos: value.subsec_nanos() as i32,
    })
  };
  let removal = match value.lifecycle().removal() {
    auv_driver_overlay_common::Removal::Manual => proto::lifecycle_options::Removal::Manual(()),
    auv_driver_overlay_common::Removal::AutoAfter(value) => proto::lifecycle_options::Removal::AutoAfter(duration(value)?),
  };
  Ok(proto::ShowOptions {
    motion: Some(proto::MotionOptions {
      duration: Some(duration(value.motion().duration())?),
      easing: Some(proto::Easing::EaseInOutExpo as i32),
    }),
    lifecycle: Some(proto::LifecycleOptions {
      removal: Some(removal),
    }),
  })
}

#[derive(Clone, Debug)]
pub struct InferenceClient {
  runner: RunnerClient,
}

impl InferenceClient {
  pub async fn detect_objects(
    &self,
    detector: inference_proto::ObjectDetectorSpec,
    frame: auv_api_proto::auv::api::image::v1::RgbFrame,
  ) -> Result<inference_proto::DetectObjectsResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .detect_runner_objects(
        self.runner.lease.clone(),
        inference_proto::DetectObjectsRequest {
          lease: None,
          detector: Some(detector),
          frame: Some(frame),
        },
      )
      .await
  }
}

#[derive(Clone, Debug)]
pub struct DisplaysClient {
  runner: RunnerClient,
}

impl DisplaysClient {
  pub async fn list(&self) -> Result<Vec<proto::Display>, tonic::Status> {
    let mut client = self.runner.client.clone();
    client.list_runner_displays(self.runner.lease.clone()).await
  }

  pub async fn capture(&self, selector: Option<proto::DisplaySelector>) -> Result<proto::CaptureDisplayResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client.capture_runner_display(self.runner.lease.clone(), selector).await
  }

  pub async fn capture_region(
    &self,
    region: proto::ScreenRect,
    selector: Option<proto::DisplaySelector>,
  ) -> Result<proto::CaptureRegionResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client.capture_runner_region(self.runner.lease.clone(), region, selector).await
  }

  pub async fn find_text(
    &self,
    selector: Option<proto::DisplaySelector>,
    query: impl Into<String>,
  ) -> Result<proto::FindDisplayTextResponse, tonic::Status> {
    self.find_text_with(selector, query, FindTextOptions::default()).await
  }

  pub async fn find_text_with(
    &self,
    selector: Option<proto::DisplaySelector>,
    query: impl Into<String>,
    options: FindTextOptions,
  ) -> Result<proto::FindDisplayTextResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .find_runner_display_text(
        self.runner.lease.clone(),
        proto::FindDisplayTextRequest {
          lease: None,
          selector,
          query: query.into(),
          region: options.region,
          custom_words: options.custom_words,
          recognition_languages: options.recognition_languages,
        },
      )
      .await
  }
}

#[derive(Clone, Debug)]
pub struct WindowsClient {
  runner: RunnerClient,
}

impl WindowsClient {
  pub async fn list(&self) -> Result<Vec<proto::Window>, tonic::Status> {
    let mut client = self.runner.client.clone();
    client.list_runner_windows(self.runner.lease.clone()).await
  }

  pub async fn resolve(&self, selector: proto::WindowSelector) -> Result<WindowClient, tonic::Status> {
    let mut client = self.runner.client.clone();
    let window = client.resolve_runner_window(self.runner.lease.clone(), selector).await?;
    let window_ref = window
      .r#ref
      .clone()
      .filter(|window_ref| !window_ref.window_id.trim().is_empty())
      .ok_or_else(|| tonic::Status::internal("ResolveWindow response omitted WindowRef"))?;
    Ok(WindowClient {
      runner: self.runner.clone(),
      window,
      window_ref,
    })
  }
}

#[derive(Clone, Debug)]
pub struct WindowClient {
  runner: RunnerClient,
  window: proto::Window,
  window_ref: proto::WindowRef,
}

impl WindowClient {
  pub fn resource(&self) -> &proto::Window {
    &self.window
  }

  pub fn reference(&self) -> &proto::WindowRef {
    &self.window_ref
  }

  pub async fn capture(&self) -> Result<proto::CaptureWindowResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client.capture_runner_window(self.runner.lease.clone(), self.window_ref.clone()).await
  }

  pub async fn find_text(&self, query: impl Into<String>) -> Result<proto::FindWindowTextResponse, tonic::Status> {
    self.find_text_with(query, FindTextOptions::default()).await
  }

  pub async fn find_text_with(
    &self,
    query: impl Into<String>,
    options: FindTextOptions,
  ) -> Result<proto::FindWindowTextResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .find_runner_window_text(
        self.runner.lease.clone(),
        proto::FindWindowTextRequest {
          lease: None,
          window: Some(self.window_ref.clone()),
          query: query.into(),
          region: options.region,
          custom_words: options.custom_words,
          recognition_languages: options.recognition_languages,
        },
      )
      .await
  }

  pub async fn click(
    &self,
    point: proto::WindowPoint,
    options: Option<proto::ClickOptions>,
  ) -> Result<proto::ClickWindowPointResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .click_runner_window_point(
        self.runner.lease.clone(),
        proto::ClickWindowPointRequest {
          lease: None,
          window: Some(self.window_ref.clone()),
          point: Some(point),
          options,
        },
      )
      .await
  }
}

#[derive(Clone, Debug)]
pub struct InputClient {
  runner: RunnerClient,
}

#[derive(Clone, Debug)]
pub struct MacosClient {
  runner: RunnerClient,
}

impl MacosClient {
  pub fn permissions(&self) -> PermissionClient {
    PermissionClient {
      runner: self.runner.clone(),
    }
  }

  pub fn media(&self) -> MediaControlClient {
    MediaControlClient {
      runner: self.runner.clone(),
    }
  }

  pub fn applications(&self) -> ApplicationClient {
    ApplicationClient {
      runner: self.runner.clone(),
    }
  }

  pub fn accessibility(&self) -> AccessibilityClient {
    AccessibilityClient {
      runner: self.runner.clone(),
    }
  }
}

#[derive(Clone, Debug)]
pub struct PermissionClient {
  runner: RunnerClient,
}

#[derive(Clone, Debug)]
pub struct MediaControlClient {
  runner: RunnerClient,
}

#[derive(Clone, Debug)]
pub struct ApplicationClient {
  runner: RunnerClient,
}

#[derive(Clone, Debug)]
pub struct AccessibilityClient {
  runner: RunnerClient,
}

impl AccessibilityClient {
  pub async fn focus_text(&self, options: auv_driver::FocusTextOptions) -> Result<auv_driver::AxFocusResult, tonic::Status> {
    let selector = match options.selector {
      auv_driver::AxTextSelector::Query(query) => macos_proto::focus_text_request::Selector::Query(query),
      auv_driver::AxTextSelector::Path(path) => macos_proto::focus_text_request::Selector::Path(path),
    };
    let mut client = self.runner.client.clone();
    let response = client
      .focus_runner_text(
        self.runner.lease.clone(),
        macos_proto::FocusTextRequest {
          lease: None,
          application: options.app,
          selector: Some(selector),
          expected_role: options.expected_role,
        },
      )
      .await?;
    ax_focus_result_from_proto(response)
  }
}

pub fn ax_focus_result_from_proto(response: macos_proto::FocusTextResponse) -> Result<auv_driver::AxFocusResult, tonic::Status> {
  let result = response.result.ok_or_else(|| tonic::Status::data_loss("FocusText response omitted AxFocusResult"))?;
  if result.app.trim().is_empty() || result.path.trim().is_empty() || result.role.trim().is_empty() {
    return Err(tonic::Status::data_loss("FocusText response omitted resolved AX identity"));
  }
  Ok(auv_driver::AxFocusResult {
    app: result.app,
    pid: result.pid,
    path: result.path,
    role: result.role,
    title: result.title,
    value: result.value,
    query: result.query,
    input_action_result: input_action_result_from_proto(
      result.action.ok_or_else(|| tonic::Status::data_loss("FocusText response omitted InputActionResult"))?,
    )?,
  })
}

fn input_action_result_from_proto(action: proto::InputActionResult) -> Result<auv_driver::InputActionResult, tonic::Status> {
  fn path(value: i32) -> Result<auv_driver::InputDeliveryPath, tonic::Status> {
    use proto::InputDeliveryPath as Wire;
    Ok(match Wire::try_from(value).map_err(|_| tonic::Status::data_loss("unknown InputDeliveryPath"))? {
      Wire::Unspecified => return Err(tonic::Status::data_loss("InputDeliveryPath was unspecified")),
      Wire::Noop => auv_driver::InputDeliveryPath::Noop,
      Wire::AxPress => auv_driver::InputDeliveryPath::AxPress,
      Wire::AxFocus => auv_driver::InputDeliveryPath::AxFocus,
      Wire::AxSetValue => auv_driver::InputDeliveryPath::AxSetValue,
      Wire::AxScroll => auv_driver::InputDeliveryPath::AxScroll,
      Wire::AxSelectedText => auv_driver::InputDeliveryPath::AxSelectedText,
      Wire::WindowTargetedMouse => auv_driver::InputDeliveryPath::WindowTargetedMouse,
      Wire::WindowTargetedWheel => auv_driver::InputDeliveryPath::WindowTargetedWheel,
      Wire::WindowTargetedKeyboard => auv_driver::InputDeliveryPath::WindowTargetedKeyboard,
      Wire::WindowTargetedKeyboardScroll => auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll,
      Wire::ClipboardPaste => auv_driver::InputDeliveryPath::ClipboardPaste,
      Wire::ForegroundSystemEvents => auv_driver::InputDeliveryPath::ForegroundSystemEvents,
      Wire::Unsupported => auv_driver::InputDeliveryPath::Unsupported,
    })
  }
  fn disturbance(value: i32) -> Result<auv_driver::DisturbanceLevel, tonic::Status> {
    use proto::DisturbanceLevel as Wire;
    Ok(match Wire::try_from(value).map_err(|_| tonic::Status::data_loss("unknown DisturbanceLevel"))? {
      Wire::Unspecified => return Err(tonic::Status::data_loss("DisturbanceLevel was unspecified")),
      Wire::None => auv_driver::DisturbanceLevel::None,
      Wire::Temporary => auv_driver::DisturbanceLevel::Temporary,
      Wire::Foreground => auv_driver::DisturbanceLevel::Foreground,
      Wire::Unknown => auv_driver::DisturbanceLevel::Unknown,
    })
  }

  let action = auv_driver::InputActionResult {
    selected_path: path(action.selected_path)?,
    attempts: action
      .attempts
      .into_iter()
      .map(|attempt| {
        Ok(auv_driver::InputAttempt {
          path: path(attempt.path)?,
          succeeded: attempt.succeeded,
          message: attempt.message,
        })
      })
      .collect::<Result<Vec<_>, tonic::Status>>()?,
    mouse_disturbance: disturbance(action.mouse_disturbance)?,
    focus_disturbance: disturbance(action.focus_disturbance)?,
    clipboard_disturbance: disturbance(action.clipboard_disturbance)?,
  };
  action.validate().map_err(|error| tonic::Status::data_loss(error.to_string()))?;
  Ok(action)
}

impl ApplicationClient {
  pub async fn activate_bundle_id(
    &self,
    bundle_id: impl Into<String>,
    settle: Option<prost_types::Duration>,
  ) -> Result<auv_driver::ApplicationActivationResult, tonic::Status> {
    let mut client = self.runner.client.clone();
    activation_result_from_proto(client.activate_runner_bundle_id(self.runner.lease.clone(), bundle_id, settle).await?)
  }
}

pub fn activation_result_from_proto(
  response: macos_proto::ActivateBundleIdResponse,
) -> Result<auv_driver::ApplicationActivationResult, tonic::Status> {
  use macos_proto::application_activation_verification::Verification;

  if response.requested_bundle_id.trim().is_empty() {
    return Err(tonic::Status::data_loss("ActivateBundleId response omitted requested_bundle_id"));
  }
  let verification = response
    .verification
    .and_then(|verification| verification.verification)
    .ok_or_else(|| tonic::Status::data_loss("ActivateBundleId response omitted verification"))?;
  let verification = match verification {
    Verification::VerifiedForeground(value) if !value.observed_bundle_id.trim().is_empty() => {
      auv_driver::ApplicationActivationVerification::VerifiedForeground {
        observed_bundle_id: value.observed_bundle_id,
      }
    }
    Verification::ForegroundMismatch(value) if !value.observed_bundle_id.trim().is_empty() => {
      auv_driver::ApplicationActivationVerification::ForegroundMismatch {
        observed_bundle_id: value.observed_bundle_id,
      }
    }
    Verification::Unavailable(value) if !value.reason.trim().is_empty() => auv_driver::ApplicationActivationVerification::Unavailable {
      reason: value.reason,
    },
    _ => return Err(tonic::Status::data_loss("ActivateBundleId response contained empty verification evidence")),
  };
  Ok(auv_driver::ApplicationActivationResult {
    requested_bundle_id: response.requested_bundle_id,
    verification,
  })
}

impl MediaControlClient {
  pub async fn now_playing(&self) -> Result<auv_media_macos::NowPlayingState, tonic::Status> {
    let mut client = self.runner.client.clone();
    now_playing_from_proto(client.get_runner_now_playing(self.runner.lease.clone()).await?)
  }

  pub async fn play(&self) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
    let mut client = self.runner.client.clone();
    media_control_outcome_from_proto(client.play_runner_media(self.runner.lease.clone()).await?.outcome, "play")
  }

  pub async fn pause(&self) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
    let mut client = self.runner.client.clone();
    media_control_outcome_from_proto(client.pause_runner_media(self.runner.lease.clone()).await?.outcome, "pause")
  }

  pub async fn toggle_play_pause(&self) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
    let mut client = self.runner.client.clone();
    media_control_outcome_from_proto(client.toggle_runner_media_play_pause(self.runner.lease.clone()).await?.outcome, "toggle")
  }

  pub async fn next_track(&self) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
    let mut client = self.runner.client.clone();
    media_control_outcome_from_proto(client.next_runner_media_track(self.runner.lease.clone()).await?.outcome, "next")
  }

  pub async fn previous_track(&self) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
    let mut client = self.runner.client.clone();
    media_control_outcome_from_proto(client.previous_runner_media_track(self.runner.lease.clone()).await?.outcome, "previous")
  }
}

pub fn now_playing_from_proto(response: macos_proto::GetNowPlayingResponse) -> Result<auv_media_macos::NowPlayingState, tonic::Status> {
  let state = response.state.ok_or_else(|| tonic::Status::data_loss("GetNowPlaying response omitted state"))?;
  now_playing_state_from_proto(state)
}

fn now_playing_state_from_proto(state: macos_proto::NowPlayingState) -> Result<auv_media_macos::NowPlayingState, tonic::Status> {
  for (field, value) in [
    ("duration_seconds", state.duration_seconds),
    ("elapsed_seconds", state.elapsed_seconds),
    ("playback_rate", state.playback_rate),
  ] {
    if value.is_some_and(|value| !value.is_finite()) {
      return Err(tonic::Status::data_loss(format!("GetNowPlaying returned non-finite {field}")));
    }
  }
  Ok(auv_media_macos::NowPlayingState {
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

fn media_control_outcome_from_proto(
  outcome: Option<macos_proto::MediaControlOutcome>,
  command: &'static str,
) -> Result<auv_media_macos::output::MediaControlOutcome, tonic::Status> {
  let outcome = outcome.ok_or_else(|| tonic::Status::data_loss("media control response omitted outcome"))?;
  let before =
    now_playing_state_from_proto(outcome.before.ok_or_else(|| tonic::Status::data_loss("media control outcome omitted before state"))?)?;
  let after =
    now_playing_state_from_proto(outcome.after.ok_or_else(|| tonic::Status::data_loss("media control outcome omitted after state"))?)?;
  Ok(auv_media_macos::output::MediaControlOutcome {
    command,
    before: auv_media_macos::output::build_now_playing_output(&before),
    after: auv_media_macos::output::build_now_playing_output(&after),
    verified: outcome.verified,
  })
}

impl PermissionClient {
  pub async fn probe(&self) -> Result<auv_driver::PermissionProbe, tonic::Status> {
    let mut client = self.runner.client.clone();
    permission_probe_from_proto(client.probe_runner_permissions(self.runner.lease.clone()).await?)
  }
}

pub fn permission_probe_from_proto(response: macos_proto::ProbePermissionsResponse) -> Result<auv_driver::PermissionProbe, tonic::Status> {
  Ok(auv_driver::PermissionProbe {
    screen_recording: permission_status_from_proto(response.screen_recording, "screen_recording")?,
    screen_capture_kit: permission_status_from_proto(response.screen_capture_kit, "screen_capture_kit")?,
    accessibility: permission_status_from_proto(response.accessibility, "accessibility")?,
    automation_to_system_events: permission_status_from_proto(response.automation_to_system_events, "automation_to_system_events")?,
  })
}

fn permission_status_from_proto(value: i32, field: &'static str) -> Result<auv_driver::PermissionStatus, tonic::Status> {
  match macos_proto::PermissionStatus::try_from(value) {
    Ok(macos_proto::PermissionStatus::Granted) => Ok(auv_driver::PermissionStatus::Granted),
    Ok(macos_proto::PermissionStatus::Missing) => Ok(auv_driver::PermissionStatus::Missing),
    Ok(macos_proto::PermissionStatus::Unknown) => Ok(auv_driver::PermissionStatus::Unknown),
    Ok(macos_proto::PermissionStatus::Unspecified) | Err(_) => {
      Err(tonic::Status::data_loss(format!("ProbePermissions returned invalid {field} status")))
    }
  }
}

impl InputClient {
  pub async fn click_screen_point(
    &self,
    point: proto::ScreenPoint,
    options: Option<proto::ScreenClickOptions>,
  ) -> Result<proto::ClickScreenPointResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .click_runner_screen_point(
        self.runner.lease.clone(),
        proto::ClickScreenPointRequest {
          lease: None,
          point: Some(point),
          options,
        },
      )
      .await
  }

  pub async fn type_text(
    &self,
    text: impl Into<String>,
    options: Option<proto::TypeTextOptions>,
  ) -> Result<proto::TypeTextResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .type_runner_text(
        self.runner.lease.clone(),
        proto::TypeTextRequest {
          lease: None,
          text: text.into(),
          options,
        },
      )
      .await
  }

  pub async fn paste_text(
    &self,
    text: impl Into<String>,
    options: Option<proto::PasteTextOptions>,
  ) -> Result<proto::PasteTextResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .paste_runner_text(
        self.runner.lease.clone(),
        proto::PasteTextRequest {
          lease: None,
          text: text.into(),
          options,
        },
      )
      .await
  }

  pub async fn press_key(
    &self,
    key: impl Into<String>,
    settle: Option<prost_types::Duration>,
  ) -> Result<proto::PressKeyResponse, tonic::Status> {
    let mut client = self.runner.client.clone();
    client
      .press_runner_key(
        self.runner.lease.clone(),
        proto::PressKeyRequest {
          lease: None,
          key: key.into(),
          settle,
        },
      )
      .await
  }
}

#[derive(Clone, Debug, Default)]
pub struct FindTextOptions {
  pub region: Option<NormalizedRect>,
  pub custom_words: Vec<String>,
  pub recognition_languages: Vec<String>,
}

#[cfg(test)]
mod tests {
  use super::*;

  fn disconnected_client() -> Client {
    let channel = tonic::transport::Endpoint::from_static("http://127.0.0.1:9").connect_lazy();
    Client::from_channel(channel)
  }

  #[tokio::test]
  async fn runner_hierarchy_rejects_an_empty_lease_before_any_transport_call() {
    let error = RunnerClient::new(disconnected_client(), RunnerLeaseRef::default()).expect_err("empty lease must fail");
    assert_eq!(error.code(), tonic::Code::InvalidArgument);
  }

  #[tokio::test]
  async fn resolved_window_child_retains_the_exact_resource_reference() {
    let runner = RunnerClient::new(
      disconnected_client(),
      RunnerLeaseRef {
        lease_id: "lease_test".to_string(),
        ..Default::default()
      },
    )
    .expect("runner client");
    let child = WindowClient {
      runner,
      window: proto::Window {
        r#ref: Some(proto::WindowRef {
          window_id: "window_test".to_string(),
        }),
        ..Default::default()
      },
      window_ref: proto::WindowRef {
        window_id: "window_test".to_string(),
      },
    };
    assert_eq!(child.reference().window_id, "window_test");
    assert_eq!(child.resource().r#ref.as_ref(), Some(child.reference()));
  }

  #[tokio::test]
  async fn runner_input_exposes_typed_screen_point_click() {
    let runner = RunnerClient::new(
      disconnected_client(),
      RunnerLeaseRef {
        lease_id: "lease_test".to_string(),
        ..Default::default()
      },
    )
    .expect("runner client");
    let input = runner.input();
    let call = input.click_screen_point(proto::ScreenPoint { x: 10.0, y: 20.0 }, Some(Default::default()));
    drop(call);
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
      assert_eq!(error.code(), tonic::Code::DataLoss);
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
    assert_eq!(error.code(), tonic::Code::DataLoss);
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
    assert_eq!(missing.code(), tonic::Code::DataLoss);

    let empty = activation_result_from_proto(macos_proto::ActivateBundleIdResponse {
      requested_bundle_id: "com.example.Requested".to_string(),
      verification: Some(macos_proto::ApplicationActivationVerification {
        verification: Some(macos_proto::application_activation_verification::Verification::Unavailable(
          macos_proto::VerificationUnavailable::default(),
        )),
      }),
    })
    .expect_err("empty reason must fail closed");
    assert_eq!(empty.code(), tonic::Code::DataLoss);
  }

  #[tokio::test]
  async fn runner_exposes_hierarchical_macos_permission_client() {
    let runner = RunnerClient::new(
      disconnected_client(),
      RunnerLeaseRef {
        lease_id: "lease_test".to_string(),
        ..Default::default()
      },
    )
    .expect("runner client");
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
    assert_eq!(missing.code(), tonic::Code::DataLoss);
    let invalid = now_playing_from_proto(macos_proto::GetNowPlayingResponse {
      state: Some(macos_proto::NowPlayingState {
        duration_seconds: Some(f64::NAN),
        ..Default::default()
      }),
    })
    .expect_err("non-finite wire value must fail closed");
    assert_eq!(invalid.code(), tonic::Code::DataLoss);
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
    assert_eq!(media_control_outcome_from_proto(None, "play").expect_err("outcome required").code(), tonic::Code::DataLoss);
    assert_eq!(
      media_control_outcome_from_proto(Some(macos_proto::MediaControlOutcome::default()), "play").expect_err("before required").code(),
      tonic::Code::DataLoss
    );
    let malformed = macos_proto::MediaControlOutcome {
      before: Some(macos_proto::NowPlayingState::default()),
      after: Some(macos_proto::NowPlayingState {
        elapsed_seconds: Some(f64::NAN),
        ..Default::default()
      }),
      verified: false,
    };
    assert_eq!(
      media_control_outcome_from_proto(Some(malformed), "next").expect_err("finite evidence required").code(),
      tonic::Code::DataLoss
    );
  }

  #[tokio::test]
  async fn runner_exposes_hierarchical_macos_media_client() {
    let runner = RunnerClient::new(
      disconnected_client(),
      RunnerLeaseRef {
        lease_id: "lease_test".to_string(),
        ..Default::default()
      },
    )
    .expect("runner client");
    let media = runner.macos().media();
    drop(media.now_playing());
    drop(media.play());
    drop(media.pause());
    drop(media.toggle_play_pause());
    drop(media.next_track());
    drop(media.previous_track());
  }
}
