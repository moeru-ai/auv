//! Hierarchical clients for one routed Driver Runner.
//!
//! Each child keeps the route and any resolved resource reference. The
//! public hierarchy is independent of whether the daemon reaches a local child
//! process or a paired remote Device.

use auv_api_client::protocol::grpc::Client as GrpcClient;
use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
use auv_api_proto::auv::api::driver::v1 as proto;

use crate::error::ClientError;

/// Message-size policy for Runner RPCs that carry raw image frames.
///
/// Tonic defaults decoded responses to 4 MiB, which is smaller than one
/// ordinary desktop RGBA capture. Runner servers already use this project-wide
/// limit; core and extension-owned generated clients must apply the same value.
pub const IMAGE_RPC_MESSAGE_SIZE_LIMIT: usize = auv_api_proto::GRPC_MESSAGE_SIZE_UNLIMITED;

/// Protocol-neutral failure from a routed Runner capability operation.
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
  /// The routed daemon client request failed.
  #[error(transparent)]
  Client(#[from] ClientError),
  /// A domain input cannot be represented by the capability request.
  #[error("Runner request is invalid: {0}")]
  InvalidArgument(String),
  /// The capability response violates its typed domain contract.
  #[error("Runner response is invalid: {0}")]
  InvalidResponse(String),
}

fn capability_status(status: tonic::Status) -> CapabilityError {
  ClientError::from_status("Runner capability RPC", status).into()
}

impl CapabilityError {
  /// Returns the protocol-neutral transport category when the failure came
  /// from the remote capability service.
  pub fn client_kind(&self) -> Option<crate::error::ClientErrorKind> {
    match self {
      Self::Client(error) => Some(error.kind()),
      Self::InvalidArgument(_) | Self::InvalidResponse(_) => None,
    }
  }
}

/// A normalized rectangle whose coordinates are relative to an image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedRegion {
  /// Horizontal origin in normalized coordinates.
  pub x: f64,
  /// Vertical origin in normalized coordinates.
  pub y: f64,
  /// Normalized width.
  pub width: f64,
  /// Normalized height.
  pub height: f64,
}

/// Selects a display by its canonical driver ID or exact name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisplaySelector {
  /// Select by canonical driver display identity.
  Id(String),
  /// Select by exact display name.
  Name(String),
}

/// Typed result of finding text on a display.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayTextRecognition {
  /// Display used for recognition.
  pub display: auv_driver::Display,
  /// Query matches.
  pub matches: auv_driver::OcrMatches,
  /// Source capture used as evidence.
  pub capture: auv_driver::Capture,
}

/// Typed result of finding text in a resolved window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowTextRecognition {
  /// Resolved source window.
  pub window: auv_driver::Window,
  /// Query matches.
  pub matches: auv_driver::OcrMatches,
  /// Source capture used as evidence.
  pub capture: auv_driver::Capture,
}

/// Typed capture of a resolved window.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowCapture {
  /// Resolved source window.
  pub window: auv_driver::Window,
  /// Captured pixels and bounds.
  pub capture: auv_driver::Capture,
}

/// Typed result of a delivered screen-point click.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenPointClick {
  /// Delivered screen point.
  pub point: auv_driver::Point,
  /// Typed input-delivery evidence.
  pub action: auv_driver::InputActionResult,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MouseMotionEvent {
  Started {
    resolved_start: auv_driver::Point,
    planned_sample_count: u32,
    duration: std::time::Duration,
  },
  Progress {
    sample_index: u32,
    point: auv_driver::Point,
    scheduled_elapsed: std::time::Duration,
  },
  Completed {
    point: auv_driver::Point,
    action: auv_driver::InputActionResult,
  },
  Accepted {
    next_sequence: u32,
  },
  Cancelled,
}

pub struct MouseMotionStream {
  inner: tonic::Streaming<proto::MoveMouseStreamResponse>,
}

impl MouseMotionStream {
  pub async fn next(&mut self) -> Result<Option<MouseMotionEvent>, CapabilityError> {
    self.inner.message().await.map_err(capability_status)?.map(move_mouse_event_from_proto).transpose()
  }
}

pub struct MouseMotionSession {
  requests: tokio::sync::mpsc::Sender<proto::StreamMouseMotionRequest>,
  responses: tonic::Streaming<proto::StreamMouseMotionResponse>,
}

impl MouseMotionSession {
  pub async fn append(&self, sequence: u32, segments: Vec<auv_driver::MouseCubicBezierSegment>) -> Result<(), CapabilityError> {
    self
      .requests
      .send(proto::StreamMouseMotionRequest {
        event: Some(proto::stream_mouse_motion_request::Event::Append(proto::StreamMouseMotionAppend {
          sequence,
          segments: segments.into_iter().map(mouse_segment_to_proto).collect(),
        })),
      })
      .await
      .map_err(|_| CapabilityError::InvalidResponse("StreamMouseMotion request stream closed".into()))
  }

  pub async fn finish(&self) -> Result<(), CapabilityError> {
    self.send_terminal(proto::stream_mouse_motion_request::Event::Finish(proto::StreamMouseMotionFinish {})).await
  }

  pub async fn cancel(&self) -> Result<(), CapabilityError> {
    self.send_terminal(proto::stream_mouse_motion_request::Event::Cancel(proto::StreamMouseMotionCancel {})).await
  }

  pub async fn next(&mut self) -> Result<Option<MouseMotionEvent>, CapabilityError> {
    self.responses.message().await.map_err(capability_status)?.map(stream_mouse_motion_event_from_proto).transpose()
  }

  async fn send_terminal(&self, event: proto::stream_mouse_motion_request::Event) -> Result<(), CapabilityError> {
    self
      .requests
      .send(proto::StreamMouseMotionRequest { event: Some(event) })
      .await
      .map_err(|_| CapabilityError::InvalidResponse("StreamMouseMotion request stream closed".into()))
  }
}

/// Typed result of a delivered window-local click.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowPointClick {
  /// Resolved target window.
  pub window: auv_driver::Window,
  /// Delivered window-local point.
  pub point: auv_driver::WindowPoint,
  /// Typed input-delivery evidence.
  pub action: auv_driver::InputActionResult,
}

// Placement is selected by `Client`/`RunClient` before this route-bound
// hierarchy is constructed. `Client::local()` is the explicit local-only
// constraint; ordinary placement may resolve either a local or paired Device.

/// Capability client routed to one RunnerClass within a Run.
#[derive(Clone, Debug)]
pub struct RunnerClient {
  client: GrpcClient,
  route: auv_api_client::RunnerRoute,
}

impl RunnerClient {
  pub(crate) fn new(client: GrpcClient, route: auv_api_client::RunnerRoute) -> Result<Self, CapabilityError> {
    if route.runner_class.trim().is_empty() {
      return Err(CapabilityError::InvalidArgument("Runner route must include runner_class".to_string()));
    }
    Ok(Self { client, route })
  }

  /// Builds the routed transport for an application-owned generated protobuf
  /// client while keeping daemon lifecycle resources out of that application's
  /// messages and metadata.
  pub fn extension_transport(&self) -> Result<auv_api_client::RoutedTransport, CapabilityError> {
    self.client.routed_transport(self.route.clone()).map_err(capability_status)
  }

  fn transport(&self) -> Result<auv_api_client::RoutedTransport, CapabilityError> {
    self.extension_transport()
  }

  /// Returns display observation and capture capabilities.
  pub fn displays(&self) -> DisplaysClient {
    DisplaysClient {
      runner: self.clone(),
    }
  }

  /// Returns window observation and input capabilities.
  pub fn windows(&self) -> WindowsClient {
    WindowsClient {
      runner: self.clone(),
    }
  }

  /// Returns global input-delivery capabilities.
  pub fn input(&self) -> InputClient {
    InputClient {
      runner: self.clone(),
    }
  }

  /// Returns visual overlay capabilities.
  pub fn overlay(&self) -> OverlayClient {
    OverlayClient {
      runner: self.clone(),
    }
  }

  /// Returns macOS-specific capabilities.
  pub fn macos(&self) -> MacosClient {
    MacosClient {
      runner: self.clone(),
    }
  }

  /// Runs OCR against a capture already obtained from this Runner.
  pub async fn recognize_text(
    &self,
    capture: auv_driver::Capture,
    region: Option<NormalizedRegion>,
    custom_words: Vec<String>,
    recognition_languages: Vec<String>,
  ) -> Result<auv_driver::TextRecognition, CapabilityError> {
    let response = proto::text_recognition_service_client::TextRecognitionServiceClient::new(self.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .recognize_text(proto::RecognizeTextRequest {
        capture: Some(capture_to_proto(capture)?),
        region: region.map(normalized_region_to_proto),
        custom_words,
        recognition_languages,
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    text_recognition_from_proto(response)
  }
}

/// Visual overlay operations for one routed Runner.
#[derive(Clone, Debug)]
pub struct OverlayClient {
  runner: RunnerClient,
}

impl OverlayClient {
  /// Shows or replaces the current overlay.
  pub async fn show(
    &self,
    overlay: &auv_driver_overlay_common::Overlay,
    options: auv_driver_overlay_common::ShowOptions,
  ) -> Result<(), CapabilityError> {
    proto::overlay_service_client::OverlayServiceClient::new(self.runner.transport()?)
      .show_overlay(proto::ShowOverlayRequest {
        overlay: Some(overlay_to_proto(overlay)?),
        options: Some(overlay_options_to_proto(options)?),
      })
      .await
      .map_err(capability_status)?;
    Ok(())
  }

  /// Removes the current overlay.
  pub async fn remove(&self) -> Result<(), CapabilityError> {
    proto::overlay_service_client::OverlayServiceClient::new(self.runner.transport()?)
      .remove_overlay(proto::RemoveOverlayRequest {})
      .await
      .map_err(capability_status)?;
    Ok(())
  }
}

fn overlay_to_proto(value: &auv_driver_overlay_common::Overlay) -> Result<proto::Overlay, CapabilityError> {
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
    .collect::<Result<Vec<_>, CapabilityError>>()?;
  Ok(proto::Overlay { layers })
}

fn cursor_image_to_proto(value: &auv_driver_overlay_common::layers::CursorImage) -> Result<proto::CursorImage, CapabilityError> {
  use auv_driver_overlay_common::layers::{BuiltInCursor, CursorImage};
  let image = match value {
    CursorImage::BuiltIn { variant } => proto::cursor_image::Image::BuiltIn(match variant {
      BuiltInCursor::Auv => proto::BuiltInCursor::Auv as i32,
      BuiltInCursor::AuvClick => proto::BuiltInCursor::AuvClick as i32,
      BuiltInCursor::You => proto::BuiltInCursor::You as i32,
    }),
    CursorImage::Svg { source } if source.len() <= 256 * 1024 => proto::cursor_image::Image::Svg(source.clone()),
    CursorImage::Svg { .. } => return Err(CapabilityError::InvalidArgument("cursor SVG exceeds 256 KiB".to_string())),
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

fn overlay_options_to_proto(value: auv_driver_overlay_common::ShowOptions) -> Result<proto::ShowOptions, CapabilityError> {
  let duration = |value: std::time::Duration| -> Result<prost_types::Duration, CapabilityError> {
    Ok(prost_types::Duration {
      seconds: i64::try_from(value.as_secs()).map_err(|_| CapabilityError::InvalidArgument("overlay duration is too large".to_string()))?,
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

/// Display observation and capture operations.
#[derive(Clone, Debug)]
pub struct DisplaysClient {
  runner: RunnerClient,
}

impl DisplaysClient {
  /// Lists observed displays.
  pub async fn list(&self) -> Result<auv_driver::ObservedDisplays, CapabilityError> {
    let displays = proto::display_service_client::DisplayServiceClient::new(self.runner.transport()?)
      .list_displays(proto::ListDisplaysRequest {})
      .await
      .map_err(capability_status)?
      .into_inner()
      .displays;
    Ok(auv_driver::ObservedDisplays {
      displays: displays.into_iter().map(display_from_proto).collect::<Result<_, _>>()?,
    })
  }

  /// Captures one selected or primary display.
  pub async fn capture(&self, selector: Option<DisplaySelector>) -> Result<auv_driver::DisplayCapture, CapabilityError> {
    let response = proto::capture_service_client::CaptureServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .capture_display(proto::CaptureDisplayRequest {
        selector: selector.map(display_selector_to_proto),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(auv_driver::DisplayCapture {
      display: display_from_proto(required(response.display, "CaptureDisplay response omitted Display")?)?,
      capture: capture_from_proto(required(response.capture, "CaptureDisplay response omitted CapturedFrame")?)?,
    })
  }

  /// Captures a screen-coordinate region on one display.
  pub async fn capture_region(
    &self,
    region: auv_driver::Rect,
    selector: Option<DisplaySelector>,
  ) -> Result<auv_driver::RegionCapture, CapabilityError> {
    let response = proto::capture_service_client::CaptureServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .capture_region(proto::CaptureRegionRequest {
        region: Some(rect_to_proto(region)),
        selector: selector.map(display_selector_to_proto),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(auv_driver::RegionCapture {
      display: display_from_proto(required(response.display, "CaptureRegion response omitted Display")?)?,
      capture: capture_from_proto(required(response.capture, "CaptureRegion response omitted CapturedFrame")?)?,
    })
  }

  /// Finds text using default recognition options.
  pub async fn find_text(
    &self,
    selector: Option<DisplaySelector>,
    query: impl Into<String>,
  ) -> Result<DisplayTextRecognition, CapabilityError> {
    self.find_text_with(selector, query, FindTextOptions::default()).await
  }

  /// Finds text using explicit recognition options.
  pub async fn find_text_with(
    &self,
    selector: Option<DisplaySelector>,
    query: impl Into<String>,
    options: FindTextOptions,
  ) -> Result<DisplayTextRecognition, CapabilityError> {
    let response = proto::text_recognition_service_client::TextRecognitionServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .find_display_text(proto::FindDisplayTextRequest {
        selector: selector.map(display_selector_to_proto),
        query: query.into(),
        region: options.region.map(normalized_region_to_proto),
        custom_words: options.custom_words,
        recognition_languages: options.recognition_languages,
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(DisplayTextRecognition {
      display: display_from_proto(required(response.display, "FindDisplayText response omitted Display")?)?,
      matches: ocr_matches_from_proto(response.matches)?,
      capture: capture_from_proto(required(response.capture, "FindDisplayText response omitted source capture")?)?,
    })
  }
}

/// Window inventory and resolution operations.
#[derive(Clone, Debug)]
pub struct WindowsClient {
  runner: RunnerClient,
}

impl WindowsClient {
  /// Lists observed windows.
  pub async fn list(&self) -> Result<Vec<auv_driver::Window>, CapabilityError> {
    let windows = proto::window_service_client::WindowServiceClient::new(self.runner.transport()?)
      .list_windows(proto::ListWindowsRequest {})
      .await
      .map_err(capability_status)?
      .into_inner()
      .windows;
    windows.into_iter().map(window_from_proto).collect()
  }

  /// Resolves one window and returns a route-bound child client.
  pub async fn resolve(&self, selector: auv_driver::WindowSelector) -> Result<WindowClient, CapabilityError> {
    let window = proto::window_service_client::WindowServiceClient::new(self.runner.transport()?)
      .resolve_window(proto::ResolveWindowRequest {
        selector: Some(window_selector_to_proto(selector)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner()
      .window
      .ok_or_else(|| CapabilityError::InvalidResponse("ResolveWindow response omitted Window".to_string()))?;
    let window_ref = window
      .r#ref
      .clone()
      .filter(|window_ref| !window_ref.window_id.trim().is_empty())
      .ok_or_else(|| CapabilityError::InvalidResponse("ResolveWindow response omitted WindowRef".to_string()))?;
    let resource = window_from_proto(window)?;
    Ok(WindowClient {
      runner: self.runner.clone(),
      window: resource,
      window_ref,
    })
  }
}

/// Capability client bound to one resolved WindowRef.
#[derive(Clone, Debug)]
pub struct WindowClient {
  runner: RunnerClient,
  window: auv_driver::Window,
  window_ref: proto::WindowRef,
}

impl WindowClient {
  /// Returns the resolved typed Window.
  pub fn resource(&self) -> &auv_driver::Window {
    &self.window
  }

  /// Returns the stable WindowRef retained by this child client.
  pub fn reference(&self) -> &auv_driver::WindowRef {
    &self.window.reference
  }

  /// Captures the resolved window.
  pub async fn capture(&self) -> Result<WindowCapture, CapabilityError> {
    let response = proto::capture_service_client::CaptureServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .capture_window(proto::CaptureWindowRequest {
        window: Some(self.window_ref.clone()),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(WindowCapture {
      window: window_from_proto(required(response.window, "CaptureWindow response omitted Window")?)?,
      capture: capture_from_proto(required(response.capture, "CaptureWindow response omitted CapturedFrame")?)?,
    })
  }

  /// Finds text using default recognition options.
  pub async fn find_text(&self, query: impl Into<String>) -> Result<WindowTextRecognition, CapabilityError> {
    self.find_text_with(query, FindTextOptions::default()).await
  }

  /// Finds text using explicit recognition options.
  pub async fn find_text_with(&self, query: impl Into<String>, options: FindTextOptions) -> Result<WindowTextRecognition, CapabilityError> {
    let response = proto::text_recognition_service_client::TextRecognitionServiceClient::new(self.runner.transport()?)
      .max_decoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .find_window_text(proto::FindWindowTextRequest {
        window: Some(self.window_ref.clone()),
        query: query.into(),
        region: options.region.map(normalized_region_to_proto),
        custom_words: options.custom_words,
        recognition_languages: options.recognition_languages,
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(WindowTextRecognition {
      window: window_from_proto(required(response.window, "FindWindowText response omitted Window")?)?,
      matches: ocr_matches_from_proto(response.matches)?,
      capture: capture_from_proto(required(response.capture, "FindWindowText response omitted source capture")?)?,
    })
  }

  /// Delivers a click in window-local coordinates.
  pub async fn click(&self, point: auv_driver::WindowPoint, options: auv_driver::ClickOptions) -> Result<WindowPointClick, CapabilityError> {
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .click_window_point(proto::ClickWindowPointRequest {
        window: Some(self.window_ref.clone()),
        point: Some(proto::WindowPoint {
          x: point.point().x,
          y: point.point().y,
        }),
        options: Some(click_options_to_proto(options)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    let point = required(response.point, "ClickWindowPoint response omitted WindowPoint")?;
    Ok(WindowPointClick {
      window: window_from_proto(required(response.window, "ClickWindowPoint response omitted Window")?)?,
      point: auv_driver::WindowPoint::new(point.x, point.y),
      action: input_action_result_from_proto(required(response.action, "ClickWindowPoint response omitted InputActionResult")?)?,
    })
  }
}

/// Global input-delivery operations.
#[derive(Clone, Debug)]
pub struct InputClient {
  runner: RunnerClient,
}

/// macOS-specific capability groups.
#[derive(Clone, Debug)]
pub struct MacosClient {
  runner: RunnerClient,
}

impl MacosClient {
  /// Returns permission inspection operations.
  pub fn permissions(&self) -> PermissionClient {
    PermissionClient {
      runner: self.runner.clone(),
    }
  }

  /// Returns system-wide media-control operations.
  pub fn media(&self) -> MediaControlClient {
    MediaControlClient {
      runner: self.runner.clone(),
    }
  }

  /// Returns application activation operations.
  pub fn applications(&self) -> ApplicationClient {
    ApplicationClient {
      runner: self.runner.clone(),
    }
  }

  /// Returns accessibility operations.
  pub fn accessibility(&self) -> AccessibilityClient {
    AccessibilityClient {
      runner: self.runner.clone(),
    }
  }
}

/// macOS permission inspection operations.
#[derive(Clone, Debug)]
pub struct PermissionClient {
  runner: RunnerClient,
}

/// macOS system-wide media-control operations.
#[derive(Clone, Debug)]
pub struct MediaControlClient {
  runner: RunnerClient,
}

/// macOS application lifecycle operations.
#[derive(Clone, Debug)]
pub struct ApplicationClient {
  runner: RunnerClient,
}

/// macOS accessibility operations.
#[derive(Clone, Debug)]
pub struct AccessibilityClient {
  runner: RunnerClient,
}

impl AccessibilityClient {
  /// Focuses a text element using typed accessibility selection.
  pub async fn focus_text(&self, options: auv_driver::FocusTextOptions) -> Result<auv_driver::AxFocusResult, CapabilityError> {
    let selector = match options.selector {
      auv_driver::AxTextSelector::Query(query) => macos_proto::focus_text_request::Selector::Query(query),
      auv_driver::AxTextSelector::Path(path) => macos_proto::focus_text_request::Selector::Path(path),
    };
    let response = macos_proto::accessibility_service_client::AccessibilityServiceClient::new(self.runner.transport()?)
      .focus_text(macos_proto::FocusTextRequest {
        application: options.app,
        selector: Some(selector),
        expected_role: options.expected_role,
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    ax_focus_result_from_proto(response)
  }
}

fn ax_focus_result_from_proto(response: macos_proto::FocusTextResponse) -> Result<auv_driver::AxFocusResult, CapabilityError> {
  let result = required(response.result, "FocusText response omitted AxFocusResult")?;
  if result.app.trim().is_empty() || result.path.trim().is_empty() || result.role.trim().is_empty() {
    return Err(CapabilityError::InvalidResponse("FocusText response omitted resolved AX identity".to_string()));
  }
  Ok(auv_driver::AxFocusResult {
    app: result.app,
    pid: result.pid,
    path: result.path,
    role: result.role,
    title: result.title,
    value: result.value,
    query: result.query,
    input_action_result: input_action_result_from_proto(required(result.action, "FocusText response omitted InputActionResult")?)?,
  })
}

/// Converts the Driver Runner wire result into the shared typed input-delivery
/// contract used by app-owned operations.
fn input_action_result_from_proto(action: proto::InputActionResult) -> Result<auv_driver::InputActionResult, CapabilityError> {
  fn path(value: i32) -> Result<auv_driver::InputDeliveryPath, CapabilityError> {
    use proto::InputDeliveryPath as Wire;
    Ok(match Wire::try_from(value).map_err(|_| CapabilityError::InvalidResponse("unknown InputDeliveryPath".to_string()))? {
      Wire::Unspecified => return Err(CapabilityError::InvalidResponse("InputDeliveryPath was unspecified".to_string())),
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
  fn disturbance(value: i32) -> Result<auv_driver::DisturbanceLevel, CapabilityError> {
    use proto::DisturbanceLevel as Wire;
    Ok(match Wire::try_from(value).map_err(|_| CapabilityError::InvalidResponse("unknown DisturbanceLevel".to_string()))? {
      Wire::Unspecified => return Err(CapabilityError::InvalidResponse("DisturbanceLevel was unspecified".to_string())),
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
      .collect::<Result<Vec<_>, CapabilityError>>()?,
    // TODO(input-action-result-wire-verification): the current protobuf shape
    // cannot carry semantic verification. Keep remote projections false until
    // an owner-approved producer/reader schema slice adds that evidence.
    verified: false,
    mouse_disturbance: disturbance(action.mouse_disturbance)?,
    focus_disturbance: disturbance(action.focus_disturbance)?,
    clipboard_disturbance: disturbance(action.clipboard_disturbance)?,
  };
  action.validate().map_err(CapabilityError::InvalidResponse)?;
  Ok(action)
}

impl ApplicationClient {
  /// Activates one application and returns explicit foreground verification.
  pub async fn activate_bundle_id(
    &self,
    bundle_id: impl Into<String>,
    settle: std::time::Duration,
  ) -> Result<auv_driver::ApplicationActivationResult, CapabilityError> {
    let response = macos_proto::application_service_client::ApplicationServiceClient::new(self.runner.transport()?)
      .activate_bundle_id(macos_proto::ActivateBundleIdRequest {
        bundle_id: bundle_id.into(),
        settle: Some(duration_to_proto(settle)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    activation_result_from_proto(response)
  }
}

fn activation_result_from_proto(
  response: macos_proto::ActivateBundleIdResponse,
) -> Result<auv_driver::ApplicationActivationResult, CapabilityError> {
  use macos_proto::application_activation_verification::Verification;

  if response.requested_bundle_id.trim().is_empty() {
    return Err(CapabilityError::InvalidResponse("ActivateBundleId response omitted requested_bundle_id".to_string()));
  }
  let verification = response
    .verification
    .and_then(|verification| verification.verification)
    .ok_or_else(|| CapabilityError::InvalidResponse("ActivateBundleId response omitted verification".to_string()))?;
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
    _ => return Err(CapabilityError::InvalidResponse("ActivateBundleId response contained empty verification evidence".to_string())),
  };
  Ok(auv_driver::ApplicationActivationResult {
    requested_bundle_id: response.requested_bundle_id,
    verification,
  })
}

impl MediaControlClient {
  /// Reads the current system-wide now-playing state.
  pub async fn now_playing(&self) -> Result<auv_media_macos::NowPlayingState, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .get_now_playing(macos_proto::GetNowPlayingRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    now_playing_from_proto(response)
  }

  /// Requests playback and returns before/after evidence.
  pub async fn play(&self) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .play(macos_proto::PlayRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    media_control_outcome_from_proto(response.outcome, "play")
  }

  /// Requests pause and returns before/after evidence.
  pub async fn pause(&self) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .pause(macos_proto::PauseRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    media_control_outcome_from_proto(response.outcome, "pause")
  }

  /// Toggles playback and returns before/after evidence.
  pub async fn toggle_play_pause(&self) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .toggle_play_pause(macos_proto::TogglePlayPauseRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    media_control_outcome_from_proto(response.outcome, "toggle")
  }

  /// Advances to the next track and returns before/after evidence.
  pub async fn next_track(&self) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .next_track(macos_proto::NextTrackRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    media_control_outcome_from_proto(response.outcome, "next")
  }

  /// Returns to the previous track and returns before/after evidence.
  pub async fn previous_track(&self) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
    let response = macos_proto::media_control_service_client::MediaControlServiceClient::new(self.runner.transport()?)
      .previous_track(macos_proto::PreviousTrackRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    media_control_outcome_from_proto(response.outcome, "previous")
  }
}

fn now_playing_from_proto(response: macos_proto::GetNowPlayingResponse) -> Result<auv_media_macos::NowPlayingState, CapabilityError> {
  let state = required(response.state, "GetNowPlaying response omitted state")?;
  now_playing_state_from_proto(state)
}

fn now_playing_state_from_proto(state: macos_proto::NowPlayingState) -> Result<auv_media_macos::NowPlayingState, CapabilityError> {
  for (field, value) in [
    ("duration_seconds", state.duration_seconds),
    ("elapsed_seconds", state.elapsed_seconds),
    ("playback_rate", state.playback_rate),
  ] {
    if value.is_some_and(|value| !value.is_finite()) {
      return Err(CapabilityError::InvalidResponse(format!("GetNowPlaying returned non-finite {field}")));
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
) -> Result<auv_media_macos::output::MediaControlOutcome, CapabilityError> {
  let outcome = required(outcome, "media control response omitted outcome")?;
  let before = now_playing_state_from_proto(required(outcome.before, "media control outcome omitted before state")?)?;
  let after = now_playing_state_from_proto(required(outcome.after, "media control outcome omitted after state")?)?;
  Ok(auv_media_macos::output::MediaControlOutcome {
    command,
    before: auv_media_macos::output::build_now_playing_output(&before),
    after: auv_media_macos::output::build_now_playing_output(&after),
    verified: outcome.verified,
  })
}

impl PermissionClient {
  /// Probes the macOS permissions required by local driver capabilities.
  pub async fn probe(&self) -> Result<auv_driver::PermissionProbe, CapabilityError> {
    let response = macos_proto::permission_service_client::PermissionServiceClient::new(self.runner.transport()?)
      .probe_permissions(macos_proto::ProbePermissionsRequest {})
      .await
      .map_err(capability_status)?
      .into_inner();
    permission_probe_from_proto(response)
  }
}

fn permission_probe_from_proto(response: macos_proto::ProbePermissionsResponse) -> Result<auv_driver::PermissionProbe, CapabilityError> {
  Ok(auv_driver::PermissionProbe {
    screen_recording: permission_status_from_proto(response.screen_recording, "screen_recording")?,
    screen_capture_kit: permission_status_from_proto(response.screen_capture_kit, "screen_capture_kit")?,
    accessibility: permission_status_from_proto(response.accessibility, "accessibility")?,
    automation_to_system_events: permission_status_from_proto(response.automation_to_system_events, "automation_to_system_events")?,
  })
}

fn permission_status_from_proto(value: i32, field: &'static str) -> Result<auv_driver::PermissionStatus, CapabilityError> {
  match macos_proto::PermissionStatus::try_from(value) {
    Ok(macos_proto::PermissionStatus::Granted) => Ok(auv_driver::PermissionStatus::Granted),
    Ok(macos_proto::PermissionStatus::Missing) => Ok(auv_driver::PermissionStatus::Missing),
    Ok(macos_proto::PermissionStatus::Unknown) => Ok(auv_driver::PermissionStatus::Unknown),
    Ok(macos_proto::PermissionStatus::Unspecified) | Err(_) => {
      Err(CapabilityError::InvalidResponse(format!("ProbePermissions returned invalid {field} status")))
    }
  }
}

impl InputClient {
  /// Executes one complete mouse motion plan and returns its progress stream.
  pub async fn move_mouse(&self, plan: auv_driver::MouseMotionPlan) -> Result<MouseMotionStream, CapabilityError> {
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .move_mouse(proto::MoveMouseRequest {
        plan: Some(mouse_motion_plan_to_proto(plan)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(MouseMotionStream { inner: response })
  }

  /// Opens a bidirectional mouse motion stream. The server validates and
  /// executes the complete curve after `finish`.
  pub async fn stream_mouse_motion(&self, plan: &auv_driver::MouseMotionPlan) -> Result<MouseMotionSession, CapabilityError> {
    let (requests, receiver) = tokio::sync::mpsc::channel(16);
    requests
      .send(proto::StreamMouseMotionRequest {
        event: Some(proto::stream_mouse_motion_request::Event::Begin(proto::StreamMouseMotionBegin {
          start: Some(mouse_start_to_proto(plan.start)),
          curve_start: Some(mouse_curve_point_to_proto(plan.curve.start)),
          mapping: Some(mouse_mapping_to_proto(plan.mapping)),
          options: Some(mouse_options_to_proto(plan.options)?),
        })),
      })
      .await
      .map_err(|_| CapabilityError::InvalidResponse("StreamMouseMotion request stream failed to open".into()))?;
    let responses = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .stream_mouse_motion(tokio_stream::wrappers::ReceiverStream::new(receiver))
      .await
      .map_err(capability_status)?
      .into_inner();
    Ok(MouseMotionSession {
      requests,
      responses,
    })
  }

  /// Delivers a click in screen coordinates.
  pub async fn click_screen_point(&self, point: auv_driver::Point, click: auv_driver::Click) -> Result<ScreenPointClick, CapabilityError> {
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .click_screen_point(proto::ClickScreenPointRequest {
        point: Some(proto::ScreenPoint {
          x: point.x,
          y: point.y,
        }),
        options: Some(proto::ScreenClickOptions {
          click: Some(click_to_proto(click)?),
        }),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    let point = required(response.point, "ClickScreenPoint response omitted ScreenPoint")?;
    Ok(ScreenPointClick {
      point: auv_driver::Point::new(point.x, point.y),
      action: input_action_result_from_proto(required(response.action, "ClickScreenPoint response omitted InputActionResult")?)?,
    })
  }

  /// Types text using the supplied delivery policy.
  pub async fn type_text(
    &self,
    text: impl Into<String>,
    options: auv_driver::TypeTextOptions,
  ) -> Result<auv_driver::InputActionResult, CapabilityError> {
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .type_text(proto::TypeTextRequest {
        text: text.into(),
        options: Some(type_text_options_to_proto(options)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    input_action_result_from_proto(required(response.action, "TypeText response omitted InputActionResult")?)
  }

  /// Pastes text using the supplied clipboard policy.
  pub async fn paste_text(&self, options: auv_driver::PasteTextOptions) -> Result<auv_driver::InputActionResult, CapabilityError> {
    let text = options.text.clone();
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .paste_text(proto::PasteTextRequest {
        text,
        options: Some(paste_text_options_to_proto(options)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    input_action_result_from_proto(required(response.action, "PasteText response omitted InputActionResult")?)
  }

  /// Delivers one key press.
  pub async fn press_key(&self, options: auv_driver::KeyPressOptions) -> Result<auv_driver::InputActionResult, CapabilityError> {
    let response = proto::input_service_client::InputServiceClient::new(self.runner.transport()?)
      .press_key(proto::PressKeyRequest {
        key: options.key,
        settle: Some(duration_to_proto(options.settle)?),
      })
      .await
      .map_err(capability_status)?
      .into_inner();
    input_action_result_from_proto(required(response.action, "PressKey response omitted InputActionResult")?)
  }
}

/// Optional OCR configuration shared by display and window searches.
#[derive(Clone, Debug, Default)]
pub struct FindTextOptions {
  /// Optional normalized search region.
  pub region: Option<NormalizedRegion>,
  /// Extra recognition vocabulary.
  pub custom_words: Vec<String>,
  /// Ordered recognition language identifiers.
  pub recognition_languages: Vec<String>,
}

fn required<T>(value: Option<T>, message: &'static str) -> Result<T, CapabilityError> {
  value.ok_or_else(|| CapabilityError::InvalidResponse(message.to_string()))
}

fn duration_to_proto(value: std::time::Duration) -> Result<prost_types::Duration, CapabilityError> {
  Ok(prost_types::Duration {
    seconds: i64::try_from(value.as_secs())
      .map_err(|_| CapabilityError::InvalidArgument("duration exceeds the protocol range".to_string()))?,
    nanos: i32::try_from(value.subsec_nanos()).expect("subsecond nanoseconds fit i32"),
  })
}

fn duration_from_proto(value: prost_types::Duration, field: &'static str) -> Result<std::time::Duration, CapabilityError> {
  value.try_into().map_err(|_| CapabilityError::InvalidResponse(format!("{field} returned an invalid duration")))
}

fn mouse_motion_plan_to_proto(plan: auv_driver::MouseMotionPlan) -> Result<proto::MouseMotionPlan, CapabilityError> {
  Ok(proto::MouseMotionPlan {
    start: Some(mouse_start_to_proto(plan.start)),
    curve: Some(proto::MouseCurve {
      start: Some(mouse_curve_point_to_proto(plan.curve.start)),
      segments: plan.curve.segments.into_iter().map(mouse_segment_to_proto).collect(),
    }),
    mapping: Some(mouse_mapping_to_proto(plan.mapping)),
    options: Some(mouse_options_to_proto(plan.options)?),
  })
}

fn mouse_start_to_proto(value: auv_driver::MouseStart) -> proto::MouseStart {
  let source = match value {
    auv_driver::MouseStart::Current => proto::mouse_start::Source::Current(proto::MouseCurrentPosition {}),
    auv_driver::MouseStart::Screen(point) => proto::mouse_start::Source::Point(proto::ScreenPoint {
      x: point.x,
      y: point.y,
    }),
  };
  proto::MouseStart {
    source: Some(source),
  }
}

fn mouse_curve_point_to_proto(point: auv_driver::Point) -> proto::MouseCurvePoint {
  proto::MouseCurvePoint {
    x: point.x,
    y: point.y,
  }
}

fn mouse_segment_to_proto(value: auv_driver::MouseCubicBezierSegment) -> proto::MouseCubicBezierSegment {
  proto::MouseCubicBezierSegment {
    control_1: Some(mouse_curve_point_to_proto(value.control_1)),
    control_2: Some(mouse_curve_point_to_proto(value.control_2)),
    end: Some(mouse_curve_point_to_proto(value.end)),
  }
}

fn mouse_mapping_to_proto(value: auv_driver::MouseCurveMapping) -> proto::MouseCurveMapping {
  proto::MouseCurveMapping {
    width: value.width,
    height: value.height,
  }
}

fn mouse_options_to_proto(value: auv_driver::MouseMotionOptions) -> Result<proto::MouseMotionOptions, CapabilityError> {
  Ok(proto::MouseMotionOptions {
    duration: Some(duration_to_proto(value.duration)?),
    sample_rate_hz: value.sample_rate_hz,
  })
}

fn move_mouse_event_from_proto(value: proto::MoveMouseStreamResponse) -> Result<MouseMotionEvent, CapabilityError> {
  match required(value.event, "MoveMouse response omitted event")? {
    proto::move_mouse_stream_response::Event::Started(value) => mouse_started_from_proto(value),
    proto::move_mouse_stream_response::Event::Progress(value) => mouse_progress_from_proto(value),
    proto::move_mouse_stream_response::Event::Completed(value) => mouse_completed_from_proto(value),
  }
}

fn stream_mouse_motion_event_from_proto(value: proto::StreamMouseMotionResponse) -> Result<MouseMotionEvent, CapabilityError> {
  match required(value.event, "StreamMouseMotion response omitted event")? {
    proto::stream_mouse_motion_response::Event::Accepted(value) => Ok(MouseMotionEvent::Accepted {
      next_sequence: value.next_sequence,
    }),
    proto::stream_mouse_motion_response::Event::Started(value) => mouse_started_from_proto(value),
    proto::stream_mouse_motion_response::Event::Progress(value) => mouse_progress_from_proto(value),
    proto::stream_mouse_motion_response::Event::Completed(value) => mouse_completed_from_proto(value),
    proto::stream_mouse_motion_response::Event::Cancelled(_) => Ok(MouseMotionEvent::Cancelled),
  }
}

fn mouse_started_from_proto(value: proto::MouseMotionStarted) -> Result<MouseMotionEvent, CapabilityError> {
  let start = required(value.resolved_start, "mouse started event omitted resolved_start")?;
  Ok(MouseMotionEvent::Started {
    resolved_start: auv_driver::Point::new(start.x, start.y),
    planned_sample_count: value.planned_sample_count,
    duration: duration_from_proto(required(value.duration, "mouse started event omitted duration")?, "mouse duration")?,
  })
}

fn mouse_progress_from_proto(value: proto::MouseMotionProgress) -> Result<MouseMotionEvent, CapabilityError> {
  let point = required(value.point, "mouse progress event omitted point")?;
  Ok(MouseMotionEvent::Progress {
    sample_index: value.sample_index,
    point: auv_driver::Point::new(point.x, point.y),
    scheduled_elapsed: duration_from_proto(
      required(value.scheduled_elapsed, "mouse progress event omitted scheduled_elapsed")?,
      "mouse scheduled_elapsed",
    )?,
  })
}

fn mouse_completed_from_proto(value: proto::MouseMotionCompleted) -> Result<MouseMotionEvent, CapabilityError> {
  let point = required(value.point, "mouse completed event omitted point")?;
  Ok(MouseMotionEvent::Completed {
    point: auv_driver::Point::new(point.x, point.y),
    action: input_action_result_from_proto(required(value.action, "mouse completed event omitted InputActionResult")?)?,
  })
}

fn click_to_proto(value: auv_driver::Click) -> Result<proto::Click, CapabilityError> {
  let (count, interval) = match value {
    auv_driver::Click::Single => (1, None),
    auv_driver::Click::Double { interval } => (2, Some(duration_to_proto(interval)?)),
    auv_driver::Click::Repeated { count, interval } => (u32::from(count), Some(duration_to_proto(interval)?)),
  };
  if count > 1 && interval.as_ref().is_none_or(|value| value.seconds == 0 && value.nanos == 0) {
    return Err(CapabilityError::InvalidArgument("repeated click interval must be positive".to_string()));
  }
  Ok(proto::Click { count, interval })
}

fn input_policy_to_proto(value: auv_driver::InputPolicy) -> proto::InputPolicy {
  match value {
    auv_driver::InputPolicy::BackgroundOnly => proto::InputPolicy::BackgroundOnly,
    auv_driver::InputPolicy::BackgroundPreferred => proto::InputPolicy::BackgroundPreferred,
    auv_driver::InputPolicy::ForegroundPreferred => proto::InputPolicy::ForegroundPreferred,
  }
}

fn text_submit_to_proto(value: auv_driver::TextSubmit) -> proto::TextSubmit {
  match value {
    auv_driver::TextSubmit::No => proto::TextSubmit::None,
    auv_driver::TextSubmit::Return => proto::TextSubmit::Return,
    auv_driver::TextSubmit::Search => proto::TextSubmit::Search,
    auv_driver::TextSubmit::Done => proto::TextSubmit::Done,
    auv_driver::TextSubmit::Go => proto::TextSubmit::Go,
  }
}

fn click_options_to_proto(value: auv_driver::ClickOptions) -> Result<proto::ClickOptions, CapabilityError> {
  Ok(proto::ClickOptions {
    policy: input_policy_to_proto(value.policy) as i32,
    click: Some(click_to_proto(value.click)?),
    window_strategy: match value.window_strategy {
      auv_driver::WindowClickStrategy::ChromiumCompatible => proto::WindowClickStrategy::ChromiumCompatible,
      auv_driver::WindowClickStrategy::PidTargeted => proto::WindowClickStrategy::PidTargeted,
    } as i32,
  })
}

fn type_text_options_to_proto(value: auv_driver::TypeTextOptions) -> Result<proto::TypeTextOptions, CapabilityError> {
  Ok(proto::TypeTextOptions {
    policy: input_policy_to_proto(value.policy) as i32,
    replace_existing: value.replace_existing,
    submit: text_submit_to_proto(value.submit) as i32,
    inter_char_delay: Some(duration_to_proto(value.inter_char_delay)?),
    allow_clipboard_fallback: value.allow_clipboard_fallback,
    settle: Some(duration_to_proto(value.settle)?),
  })
}

fn paste_text_options_to_proto(value: auv_driver::PasteTextOptions) -> Result<proto::PasteTextOptions, CapabilityError> {
  Ok(proto::PasteTextOptions {
    replace_existing: value.replace_existing,
    submit: text_submit_to_proto(value.submit) as i32,
    settle: Some(duration_to_proto(value.settle)?),
  })
}

fn rect_to_proto(value: auv_driver::Rect) -> proto::ScreenRect {
  proto::ScreenRect {
    x: value.origin.x,
    y: value.origin.y,
    width: value.size.width,
    height: value.size.height,
  }
}

fn normalized_region_to_proto(value: NormalizedRegion) -> auv_api_proto::auv::api::image::v1::NormalizedRect {
  auv_api_proto::auv::api::image::v1::NormalizedRect {
    x: value.x,
    y: value.y,
    width: value.width,
    height: value.height,
  }
}

fn display_selector_to_proto(value: DisplaySelector) -> proto::DisplaySelector {
  let selector = match value {
    DisplaySelector::Id(display_id) => proto::display_selector::Selector::Display(proto::DisplayRef { display_id }),
    DisplaySelector::Name(name) => proto::display_selector::Selector::Name(name),
  };
  proto::DisplaySelector {
    selector: Some(selector),
  }
}

fn window_selector_to_proto(value: auv_driver::WindowSelector) -> Result<proto::WindowSelector, CapabilityError> {
  use auv_driver::TextMatcher;
  use proto::window_selector::{Application, Window};

  let app = value.app.ok_or_else(|| CapabilityError::InvalidArgument("Window selector must identify an application".to_string()))?;
  let application = match (app.bundle, app.name, app.process_id, app.frontmost) {
    (Some(TextMatcher::Exact(value)), None, None, false) => Application::ApplicationBundleId(value),
    (None, Some(TextMatcher::Exact(value)), None, false) => Application::ApplicationName(value),
    (None, None, Some(value), false) if value > 0 => Application::ProcessId(value),
    (None, None, None, true) => Application::FrontmostApplication(true),
    _ => {
      return Err(CapabilityError::InvalidArgument(
        "Window application selector must contain exactly one supported exact match".to_string(),
      ));
    }
  };
  let window = match (value.title, value.main_visible) {
    (Some(TextMatcher::Exact(value)), false) => Window::TitleExact(value),
    (Some(TextMatcher::Contains(value)), false) => Window::TitleContains(value),
    (None, true) => Window::MainVisible(true),
    _ => return Err(CapabilityError::InvalidArgument("Window selector must identify exactly one window".to_string())),
  };
  Ok(proto::WindowSelector {
    application: Some(application),
    window: Some(window),
  })
}

fn display_from_proto(display: proto::Display) -> Result<auv_driver::Display, CapabilityError> {
  let frame = required(display.frame, "Display omitted its screen frame")?;
  Ok(auv_driver::Display {
    id: display.display_id,
    name: display.name,
    frame: auv_driver::Rect::new(frame.x, frame.y, frame.width, frame.height),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    scale_factor: display.scale_factor,
    is_primary: display.primary,
    is_builtin: display.builtin,
  })
}

fn window_from_proto(window: proto::Window) -> Result<auv_driver::Window, CapabilityError> {
  let reference = required(window.r#ref, "Window omitted its reference")?;
  let frame = required(window.frame, "Window omitted its screen frame")?;
  Ok(auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: reference.window_id,
    },
    title: window.title,
    app_name: window.application_name,
    app_bundle_id: window.application_bundle_id,
    process_id: window.process_id,
    frame: auv_driver::Rect::new(frame.x, frame.y, frame.width, frame.height),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: window.is_main,
    is_visible: window.is_visible,
  })
}

fn capture_from_proto(capture: proto::CapturedFrame) -> Result<auv_driver::Capture, CapabilityError> {
  let image = required(capture.image, "CapturedFrame omitted its RGBA image")?;
  let bounds = required(capture.bounds, "CapturedFrame omitted its screen bounds")?;
  let image = image::RgbaImage::from_raw(image.width, image.height, image.data)
    .ok_or_else(|| CapabilityError::InvalidResponse("CapturedFrame contains malformed RGBA8 data".to_string()))?;
  Ok(auv_driver::Capture {
    image,
    bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
    scale_factor: capture.scale_factor,
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

fn capture_to_proto(capture: auv_driver::Capture) -> Result<proto::CapturedFrame, CapabilityError> {
  let width = capture.image.width();
  let height = capture.image.height();
  Ok(proto::CapturedFrame {
    image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
      width,
      height,
      data: capture.image.into_raw(),
    }),
    bounds: Some(rect_to_proto(capture.bounds)),
    scale_factor: capture.scale_factor,
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

fn ocr_matches_from_proto(matches: Vec<proto::TextMatch>) -> Result<auv_driver::OcrMatches, CapabilityError> {
  Ok(auv_driver::OcrMatches {
    matches: matches
      .into_iter()
      .map(|matched| {
        let bounds = required(matched.bounds, "text match omitted its screen bounds")?;
        Ok(auv_driver::OcrMatch {
          text: matched.text,
          confidence: matched.confidence,
          bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
        })
      })
      .collect::<Result<_, CapabilityError>>()?,
  })
}

fn text_recognition_from_proto(response: proto::RecognizeTextResponse) -> Result<auv_driver::TextRecognition, CapabilityError> {
  Ok(auv_driver::TextRecognition {
    text: response.text,
    regions: response
      .regions
      .into_iter()
      .map(|region| {
        let bounds = required(region.bounds, "recognized text omitted its bounds")?;
        Ok(auv_driver::RecognizedText {
          text: region.text,
          bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
          confidence: region.confidence,
        })
      })
      .collect::<Result<_, CapabilityError>>()?,
  })
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;
