use std::collections::HashMap;
use std::path::Path;

use auv_driver::{Capture, InputActionResult, Point, RatioRect, ScreenPoint, TextRecognition};
use auv_inference_common::{ImageSize, InferenceError};
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
use image::RgbImage;
use thiserror::Error;

use auv::client::runner::NormalizedRegion;
use auv::client::{RunClient, RunOptions, RunnerOptions};
use auv::{AuvContext, Client};
use auv_api_client::ConnectEndpoint;
use auv_api_proto::auv::api::image::v1 as image_proto;

use crate::api::v1 as balatro_proto;
use crate::cache::cache_hint_for_detection;
use crate::config::{BalatroModelAsset, BalatroModelConfig, HuggingFaceRepoKind};
use crate::detector::{
  BalatroDetectionSets, BalatroDetectors, CardAttributeDetectionSets, crop_hand_frame, remap_card_attribute_detections,
  remap_hand_detections,
};
use crate::model::{
  BALATRO_STATE_SCHEMA_VERSION, BalatroDiagnostic, BalatroPhase, BalatroState, ButtonTarget, CacheHint, CardAttribute, CardAttributes,
  CardSlot, ConsumableKind, ConsumableSlot, FrameRef, JokerSlot, ObjectEvidence, ObjectZone, Reading, ReadingStatus, RoundState, ScoreState,
  SlotId, StoreItem, StoreItemKind, StoreState,
};

#[derive(Debug, Error)]
pub enum ObservationError {
  #[error("inference error: {0}")]
  Inference(#[from] InferenceError),
  #[error("I/O error: {0}")]
  Io(#[from] std::io::Error),
  #[error("image error: {0}")]
  Image(#[from] image::ImageError),
  #[error("daemon API error: {0}")]
  Api(String),
}

#[derive(Clone, Debug)]
pub struct HoverReadRequest {
  pub point: Point,
  pub region: RatioRect,
  pub custom_words: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct HoverReadObservation {
  pub point: ScreenPoint,
  pub delivery: InputActionResult,
  pub recognition: TextRecognition,
  pub frame_source: String,
}

/// Observes an image through an inference Runner attached to one AUV Run.
pub async fn observe_image_via_api(
  image_path: impl AsRef<Path>,
  config: &BalatroModelConfig,
  endpoint: Option<ConnectEndpoint>,
  no_cache: bool,
) -> Result<BalatroState, ObservationError> {
  let image_path = image_path.as_ref();
  let image = image::open(image_path)?.to_rgb8();
  let image_size = ImageSize {
    width: image.width(),
    height: image.height(),
  };
  let frame = image_proto::RgbFrame {
    width: image.width(),
    height: image.height(),
    data: image.as_raw().clone(),
  };
  let (entities_classes, ui_classes) = load_remote_class_names(config).await?;
  let auv = connect_auv(endpoint).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let balatro = match run.runner(balatro_runner_options()).await {
    Ok(balatro) => balatro,
    Err(error) => return finish_run(run, Err(api_error(error))).await,
  };
  let result = detect_via_api(&balatro, config, entities_classes, ui_classes, frame)
    .await
    .map(|detections| build_state_from_detections(image_path.display().to_string(), image_size, &image, detections, no_cache));
  finish_run(run, result).await
}

/// Captures, recognizes, and detects one live Balatro frame through Driver and
/// inference Runners attached to the same AUV Run.
pub async fn observe_live_via_api(
  target: &str,
  config: &BalatroModelConfig,
  endpoint: Option<ConnectEndpoint>,
  no_cache: bool,
) -> Result<BalatroState, ObservationError> {
  let (entities_classes, ui_classes) = load_remote_class_names(config).await?;
  let auv = connect_auv(endpoint).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let driver = match run.runner(driver_runner_options()).await {
    Ok(driver) => driver,
    Err(error) => return finish_run(run, Err(api_error(error))).await,
  };
  let balatro = match run.runner(balatro_runner_options()).await {
    Ok(balatro) => balatro,
    Err(error) => {
      return finish_run(run, Err(api_error(error))).await;
    }
  };
  let result = observe_live_with_runners(&driver, &balatro, target, config, entities_classes, ui_classes, no_cache).await;
  finish_run(run, result).await
}

/// Delivers a click for a point measured in an observed primary-display frame.
///
/// The returned point is in the Device screen's logical coordinate space; the
/// caller retains the typed delivery result and verifies game state separately.
pub async fn click_display_frame_point_via_api(
  frame: &FrameRef,
  point: Point,
) -> Result<(ScreenPoint, InputActionResult), ObservationError> {
  validate_primary_display_frame(frame, "remote screen click")?;

  let auv = connect_auv(None).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let result = async {
    let driver = run.runner(driver_runner_options()).await.map_err(api_error)?;
    let displays = driver.displays().list().await.map_err(api_error)?;
    let display = displays
      .displays
      .iter()
      .find(|display| display.is_primary)
      .or_else(|| displays.displays.first())
      .ok_or_else(|| ObservationError::Api("Driver Runner returned no displays".to_string()))?;
    let screen_point = project_display_frame_point(frame, display.frame, point);
    let response = driver.input().click_screen_point(screen_point.point(), auv_driver::Click::Single).await.map_err(api_error)?;
    #[cfg(feature = "tracing")]
    crate::run_read::emit_json_artifact(auv_driver::INPUT_ACTION_RESULT_PURPOSE, &response.action);
    Ok((screen_point, response.action))
  }
  .await;
  finish_run(run, result).await
}

/// Moves the pointer for a point measured in an observed display frame.
pub async fn move_display_frame_point_via_api(frame: &FrameRef, point: Point) -> Result<(ScreenPoint, InputActionResult), ObservationError> {
  validate_primary_display_frame(frame, "remote screen move")?;
  let auv = connect_auv(None).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let result = async {
    let driver = run.runner(driver_runner_options()).await.map_err(api_error)?;
    let displays = driver.displays().list().await.map_err(api_error)?;
    let display = displays
      .displays
      .iter()
      .find(|display| display.is_primary)
      .or_else(|| displays.displays.first())
      .ok_or_else(|| ObservationError::Api("Driver Runner returned no displays".to_string()))?;
    let screen_point = project_display_frame_point(frame, display.frame, point);
    let action = move_mouse_to(&driver, screen_point.point()).await?;
    #[cfg(feature = "tracing")]
    crate::run_read::emit_json_artifact(auv_driver::INPUT_ACTION_RESULT_PURPOSE, &action);
    Ok((screen_point, action))
  }
  .await;
  finish_run(run, result).await
}

/// Hovers observed display-frame points and OCRs the resulting tooltip frames.
///
/// One Driver Runner is retained for the entire batch so pointer delivery,
/// capture, and OCR share the same routed session.
pub async fn hover_read_display_frame_points_via_api(
  frame: &FrameRef,
  requests: &[HoverReadRequest],
) -> Result<Vec<HoverReadObservation>, ObservationError> {
  validate_primary_display_frame(frame, "remote hover read")?;
  let auv = connect_auv(None).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let result = async {
    let driver = run.runner(driver_runner_options()).await.map_err(api_error)?;
    let displays = driver.displays().list().await.map_err(api_error)?;
    let display = displays
      .displays
      .iter()
      .find(|display| display.is_primary)
      .or_else(|| displays.displays.first())
      .ok_or_else(|| ObservationError::Api("Driver Runner returned no displays".to_string()))?;
    let mut observations = Vec::with_capacity(requests.len());
    for (index, request) in requests.iter().enumerate() {
      let screen_point = project_display_frame_point(frame, display.frame, request.point);
      let moved = move_mouse_to(&driver, screen_point.point()).await?;
      tokio::time::sleep(std::time::Duration::from_millis(450)).await;
      let capture = driver.displays().capture(None).await.map_err(api_error)?.capture;
      let recognition = driver
        .recognize_text(
          capture.clone(),
          Some(NormalizedRegion {
            x: request.region.x,
            y: request.region.y,
            width: request.region.width,
            height: request.region.height,
          }),
          request.custom_words.clone(),
          vec!["zh-Hans".to_string(), "en-US".to_string()],
        )
        .await
        .map_err(api_error)?;
      let frame_source = format!("daemon://display/primary?hover_read={index}");
      #[cfg(feature = "tracing")]
      {
        crate::run_read::emit_json_artifact(auv_driver::INPUT_ACTION_RESULT_PURPOSE, &moved);
        crate::run_read::emit_png_artifact("auv.balatro.object_hover.capture", &frame_source, &capture.image);
      }
      observations.push(HoverReadObservation {
        point: screen_point,
        delivery: moved,
        recognition,
        frame_source,
      });
    }
    Ok(observations)
  }
  .await;
  finish_run(run, result).await
}

async fn move_mouse_to(driver: &auv::client::runner::RunnerClient, point: Point) -> Result<InputActionResult, ObservationError> {
  let mut stream = driver.input().move_mouse(auv_driver::MouseMotionPlan::direct(point)).await.map_err(api_error)?;
  while let Some(event) = stream.next().await.map_err(api_error)? {
    if let auv::client::runner::MouseMotionEvent::Completed { action, .. } = event {
      return Ok(action);
    }
  }
  Err(ObservationError::Api("MoveMouse ended without completion evidence".to_string()))
}

fn validate_primary_display_frame(frame: &FrameRef, operation: &str) -> Result<(), ObservationError> {
  if !frame.source.starts_with("daemon://display/primary") {
    // TODO(balatro-remote-window-projection): project resolved Window captures
    // after a production Linux Window consumer exists; live Linux currently
    // supplies the explicit primary-display fallback source.
    return Err(ObservationError::Api(format!("{operation} requires a primary-display frame, received {:?}", frame.source)));
  }
  if frame.image_size.width == 0 || frame.image_size.height == 0 {
    return Err(ObservationError::Api(format!("{operation} requires non-zero frame dimensions")));
  }
  Ok(())
}

fn project_display_frame_point(frame: &FrameRef, bounds: auv_driver::Rect, point: Point) -> ScreenPoint {
  ScreenPoint::new(
    bounds.origin.x + point.x / f64::from(frame.image_size.width) * bounds.size.width,
    bounds.origin.y + point.y / f64::from(frame.image_size.height) * bounds.size.height,
  )
}

async fn connect_auv(endpoint: Option<ConnectEndpoint>) -> Result<Client, ObservationError> {
  match std::env::var("AUV_CONTEXT") {
    Ok(_) => Client::from_context(AuvContext::from_env().map_err(api_error)?).await.map_err(api_error),
    Err(std::env::VarError::NotPresent) => match endpoint {
      Some(endpoint) => Client::from_context(AuvContext {
        daemon_endpoint: Some(endpoint.to_string()),
        ..AuvContext::default()
      })
      .await
      .map_err(api_error),
      None => Client::from_env_or_local().await.map_err(api_error),
    },
    Err(error) => Err(api_error(error)),
  }
}

fn balatro_run_options() -> RunOptions {
  RunOptions {
    labels: HashMap::from([("auv.dev/app".to_string(), "balatro".to_string())]),
    ..RunOptions::default()
  }
}

fn driver_runner_options() -> RunnerOptions {
  RunnerOptions {
    runner_class: "auv.core.local".parse().expect("the local RunnerClass ID is valid"),
    ..RunnerOptions::default()
  }
}

fn balatro_runner_options() -> RunnerOptions {
  RunnerOptions {
    runner_class: "auv.game.balatro".parse().expect("the Balatro RunnerClass ID is valid"),
    ..RunnerOptions::default()
  }
}

async fn finish_run<T>(run: RunClient, result: Result<T, ObservationError>) -> Result<T, ObservationError> {
  let outcome = if result.is_ok() {
    auv::runs::RunOutcome::Succeeded
  } else {
    auv::runs::RunOutcome::Failed
  };
  merge_cleanup(result, run.finish_if_owned(outcome).await.map(|_| ()).map_err(api_error))
}

fn merge_cleanup<T>(result: Result<T, ObservationError>, cleanup: Result<(), ObservationError>) -> Result<T, ObservationError> {
  match (result, cleanup) {
    (Ok(value), Ok(())) => Ok(value),
    (Err(primary), Ok(())) => Err(primary),
    (Ok(_), Err(cleanup)) => Err(cleanup),
    (Err(primary), Err(cleanup)) => Err(ObservationError::Api(format!("{primary}; cleanup also failed: {cleanup}"))),
  }
}

fn api_error(error: impl std::fmt::Display) -> ObservationError {
  ObservationError::Api(error.to_string())
}

async fn detect_via_api(
  balatro: &auv::client::runner::RunnerClient,
  config: &BalatroModelConfig,
  entities_classes: Vec<String>,
  ui_classes: Vec<String>,
  frame: image_proto::RgbFrame,
) -> Result<BalatroDetectionSets, ObservationError> {
  let balatro =
    balatro_proto::balatro_detection_service_client::BalatroDetectionServiceClient::new(balatro.extension_transport().map_err(api_error)?)
      .max_decoding_message_size(auv::client::runner::IMAGE_RPC_MESSAGE_SIZE_LIMIT)
      .max_encoding_message_size(auv::client::runner::IMAGE_RPC_MESSAGE_SIZE_LIMIT);
  let full_image_size = ImageSize {
    width: frame.width,
    height: frame.height,
  };
  let image = RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
    .ok_or_else(|| ObservationError::Api("card attribute detectors received an invalid RGB frame".to_string()))?;
  let (hand_crop, hand_frame) = crop_hand_frame(&image);
  let hand_frame = image_proto::RgbFrame {
    width: hand_frame.image.width(),
    height: hand_frame.image.height(),
    data: hand_frame.image.into_raw(),
  };
  let full_frame_detectors = vec![
    detector_spec("balatro-entities", &config.entities_model, &config.device, 640, entities_classes.clone())?,
    detector_spec("balatro-ui", &config.ui_model, &config.device, 640, ui_classes)?,
  ];
  let mut hand_frame_detectors = vec![
    detector_spec("balatro-card-identity", &config.card_identity_model, &config.device, 960, Vec::new())?,
    detector_spec("balatro-card-enhancement", &config.card_enhancement_model, &config.device, 640, Vec::new())?,
    detector_spec("balatro-card-edition", &config.card_edition_model, &config.device, 640, Vec::new())?,
    detector_spec("balatro-card-seal", &config.card_seal_model, &config.device, 640, Vec::new())?,
  ];
  if let Some(cards_model) = &config.cards_model {
    hand_frame_detectors.push(detector_spec("balatro-cards", cards_model, &config.device, 640, entities_classes)?);
  }

  let detect_batch = |mut client: balatro_proto::balatro_detection_service_client::BalatroDetectionServiceClient<_>, detectors, frame| async move {
    client
      .detect_objects_batch(balatro_proto::DetectObjectsBatchRequest {
        detectors,
        frame: Some(frame),
      })
      .await
      .map(|response| response.into_inner())
      .map_err(api_error)
  };
  let (full_frame_batch, hand_frame_batch) = tokio::try_join!(
    detect_batch(balatro.clone(), full_frame_detectors, frame),
    detect_batch(balatro.clone(), hand_frame_detectors, hand_frame),
  )?;
  let mut full_frame_batch = batch_results(full_frame_batch)?;
  let mut hand_frame_batch = batch_results(hand_frame_batch)?;
  let cards = config
    .cards_model
    .as_ref()
    .map(|_| {
      take_batch_result(&mut hand_frame_batch, "balatro-cards")
        .and_then(detection_result_from_proto)
        .map(|result| remap_hand_detections(result, hand_crop, full_image_size))
    })
    .transpose()?;
  let mut card_attribute = |detector_id| {
    take_batch_result(&mut hand_frame_batch, detector_id)
      .and_then(detection_result_from_proto)
      .map(|result| remap_card_attribute_detections(result, hand_crop, full_image_size))
  };
  Ok(BalatroDetectionSets {
    entities: detection_result_from_proto(take_batch_result(&mut full_frame_batch, "balatro-entities")?)?,
    cards,
    card_attributes: CardAttributeDetectionSets {
      identity: Some(card_attribute("balatro-card-identity")?),
      enhancement: Some(card_attribute("balatro-card-enhancement")?),
      edition: Some(card_attribute("balatro-card-edition")?),
      seal: Some(card_attribute("balatro-card-seal")?),
    },
    ui: detection_result_from_proto(take_batch_result(&mut full_frame_batch, "balatro-ui")?)?,
  })
}

fn batch_results(
  batch: balatro_proto::DetectObjectsBatchResponse,
) -> Result<HashMap<String, balatro_proto::DetectObjectsResponse>, ObservationError> {
  batch
    .results
    .into_iter()
    .map(|entry| {
      let result = entry.result.ok_or_else(|| ObservationError::Api(format!("batch result {} omitted result", entry.detector_id)))?;
      Ok((entry.detector_id, result))
    })
    .collect()
}

fn take_batch_result(
  results: &mut HashMap<String, balatro_proto::DetectObjectsResponse>,
  detector_id: &str,
) -> Result<balatro_proto::DetectObjectsResponse, ObservationError> {
  results.remove(detector_id).ok_or_else(|| ObservationError::Api(format!("batch response omitted detector {detector_id}")))
}

async fn observe_live_with_runners(
  driver: &auv::client::runner::RunnerClient,
  balatro: &auv::client::runner::RunnerClient,
  target: &str,
  config: &BalatroModelConfig,
  entities_classes: Vec<String>,
  ui_classes: Vec<String>,
  no_cache: bool,
) -> Result<BalatroState, ObservationError> {
  // TODO(balatro-window-resolution-cache): cache a resolved Window only after
  // Linux exposes a typed move/resize/close invalidation signal; stale window
  // identity or geometry is worse than the current per-observation lookup.
  let window = driver
    .windows()
    .resolve(auv_driver::WindowSelector {
      app: Some(auv_driver::App::name(target)),
      main_visible: true,
      ..Default::default()
    })
    .await;
  let (source, captured) = match window {
    Ok(window) => (format!("daemon://window/{}", window.reference().id), window.capture().await.map_err(api_error)?.capture),
    Err(error) if matches!(error.client_kind(), Some(auv::error::ClientErrorKind::NotFound | auv::error::ClientErrorKind::Unsupported)) => {
      ("daemon://display/primary?fallback=window_unavailable".to_string(), driver.displays().capture(None).await.map_err(api_error)?.capture)
    }
    Err(error) => return Err(api_error(error)),
  };
  let capture = captured;
  #[cfg(feature = "tracing")]
  crate::run_read::emit_png_artifact("auv.balatro.observation.capture", &source, &capture.image);
  let image = image::DynamicImage::ImageRgba8(capture.image.clone()).to_rgb8();
  let frame = image_proto::RgbFrame {
    width: image.width(),
    height: image.height(),
    data: image.as_raw().clone(),
  };
  let image_size = ImageSize {
    width: frame.width,
    height: frame.height,
  };
  let detections = detect_via_api(balatro, config, entities_classes, ui_classes, frame).await?;
  let recognition = match ocr_capture_for_ui(&capture, &detections.ui) {
    Some(ocr_capture) => {
      driver.recognize_text(ocr_capture, None, Vec::new(), vec!["zh-Hans".to_string(), "en-US".to_string()]).await.map_err(api_error)?
    }
    None => Default::default(),
  };
  let mut state = build_state_from_detections(source, image_size, &image, detections, no_cache);
  enrich_ui_numeric_readings_from_recognition(&mut state, &capture, recognition);
  #[cfg(feature = "tracing")]
  crate::run_read::emit_json_artifact("auv.balatro.observation.state", &state);
  Ok(state)
}

fn ocr_region_for_ui(ui: &DetectionResult) -> Option<NormalizedRegion> {
  if ui.image_size.width == 0 || ui.image_size.height == 0 {
    return None;
  }
  let mut numeric = ui.detections.iter().filter(|detection| is_numeric_ui_label(&detection.label));
  let first = numeric.next()?;
  let (mut x1, mut y1, mut x2, mut y2) = (first.bbox.x1, first.bbox.y1, first.bbox.x2, first.bbox.y2);
  for detection in numeric {
    x1 = x1.min(detection.bbox.x1);
    y1 = y1.min(detection.bbox.y1);
    x2 = x2.max(detection.bbox.x2);
    y2 = y2.max(detection.bbox.y2);
  }

  let image_width = f64::from(ui.image_size.width);
  let image_height = f64::from(ui.image_size.height);
  // NOTICE: One percent of the frame retains glyph strokes that touch a UI
  // detector boundary without expanding OCR back into the full display.
  let x1 = (f64::from(x1) / image_width - 0.01).clamp(0.0, 1.0);
  let y1 = (f64::from(y1) / image_height - 0.01).clamp(0.0, 1.0);
  let x2 = (f64::from(x2) / image_width + 0.01).clamp(0.0, 1.0);
  let y2 = (f64::from(y2) / image_height + 0.01).clamp(0.0, 1.0);
  (x2 > x1 && y2 > y1).then_some(NormalizedRegion {
    x: x1,
    y: y1,
    width: x2 - x1,
    height: y2 - y1,
  })
}

fn ocr_capture_for_ui(capture: &Capture, ui: &DetectionResult) -> Option<Capture> {
  let region = ocr_region_for_ui(ui)?;
  let image_width = capture.image.width();
  let image_height = capture.image.height();
  if image_width == 0 || image_height == 0 {
    return None;
  }
  let x = (region.x * f64::from(image_width)).round().clamp(0.0, f64::from(image_width)) as u32;
  let y = (region.y * f64::from(image_height)).round().clamp(0.0, f64::from(image_height)) as u32;
  let width = (region.width * f64::from(image_width)).round().clamp(0.0, f64::from(image_width - x)) as u32;
  let height = (region.height * f64::from(image_height)).round().clamp(0.0, f64::from(image_height - y)) as u32;
  if width == 0 || height == 0 {
    return None;
  }

  let x_scale = capture.bounds.size.width / f64::from(image_width);
  let y_scale = capture.bounds.size.height / f64::from(image_height);
  Some(Capture {
    image: image::imageops::crop_imm(&capture.image, x, y, width, height).to_image(),
    bounds: auv_driver::Rect::new(
      capture.bounds.origin.x + f64::from(x) * x_scale,
      capture.bounds.origin.y + f64::from(y) * y_scale,
      f64::from(width) * x_scale,
      f64::from(height) * y_scale,
    ),
    scale_factor: capture.scale_factor,
    backend: capture.backend.clone(),
    fallback_reason: capture.fallback_reason.clone(),
  })
}

fn detector_spec(
  detector_id: &str,
  model: &BalatroModelAsset,
  device: &auv_inference_ultralytics::InferenceDevice,
  input_size: u32,
  class_names: Vec<String>,
) -> Result<balatro_proto::ObjectDetectorSpec, ObservationError> {
  let (kind, index) = match device {
    auv_inference_ultralytics::InferenceDevice::Cpu => (balatro_proto::InferenceDeviceKind::Cpu, None),
    auv_inference_ultralytics::InferenceDevice::Cuda(index) => (balatro_proto::InferenceDeviceKind::Cuda, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::CoreMl => (balatro_proto::InferenceDeviceKind::CoreMl, None),
    auv_inference_ultralytics::InferenceDevice::DirectMl(index) => (balatro_proto::InferenceDeviceKind::DirectMl, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::OpenVino => (balatro_proto::InferenceDeviceKind::OpenVino, None),
    auv_inference_ultralytics::InferenceDevice::Xnnpack => (balatro_proto::InferenceDeviceKind::Xnnpack, None),
    auv_inference_ultralytics::InferenceDevice::TensorRt(index) => (balatro_proto::InferenceDeviceKind::TensorRt, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::Rocm(index) => (balatro_proto::InferenceDeviceKind::Rocm, Some(*index)),
  };
  let index = index
    .map(u32::try_from)
    .transpose()
    .map_err(|_| ObservationError::Api("inference device index exceeds the protobuf uint32 range".to_string()))?;
  Ok(balatro_proto::ObjectDetectorSpec {
    detector_id: detector_id.to_string(),
    source: Some(model_source(model)?),
    input_size: Some(input_size),
    confidence_threshold: Some(0.25),
    iou_threshold: Some(0.45),
    max_detections: Some(300),
    device: Some(balatro_proto::InferenceDevice {
      kind: kind as i32,
      index,
    }),
    class_names,
  })
}

fn model_source(model: &BalatroModelAsset) -> Result<balatro_proto::object_detector_spec::Source, ObservationError> {
  Ok(match model {
    BalatroModelAsset::Local(path) => balatro_proto::object_detector_spec::Source::RunnerPath(
      path.to_str().ok_or_else(|| ObservationError::Api(format!("Runner model path is not valid UTF-8: {}", path.display())))?.to_string(),
    ),
    BalatroModelAsset::HuggingFace {
      repo_kind,
      owner,
      repo,
      filename,
    } => balatro_proto::object_detector_spec::Source::HuggingFace(balatro_proto::HuggingFaceAsset {
      repository_kind: match repo_kind {
        HuggingFaceRepoKind::Model => balatro_proto::hugging_face_asset::RepositoryKind::Model as i32,
        HuggingFaceRepoKind::Dataset => balatro_proto::hugging_face_asset::RepositoryKind::Dataset as i32,
      },
      owner: (*owner).to_string(),
      repository: (*repo).to_string(),
      filename: (*filename).to_string(),
    }),
  })
}

async fn load_remote_class_names(config: &BalatroModelConfig) -> Result<(Vec<String>, Vec<String>), ObservationError> {
  let entities = config.entities_classes.clone();
  let ui = config.ui_classes.clone();
  tokio::task::spawn_blocking(move || {
    let load = |asset: BalatroModelAsset| -> Result<Vec<String>, ObservationError> {
      let path = asset.resolve_path().map_err(|error| ObservationError::Api(error.to_string()))?;
      Ok(crate::config::load_class_names(&path)?)
    };
    Ok((load(entities)?, load(ui)?))
  })
  .await
  .map_err(|error| ObservationError::Api(format!("class-asset task failed: {error}")))?
}

fn detection_result_from_proto(response: balatro_proto::DetectObjectsResponse) -> Result<DetectionResult, ObservationError> {
  let size = response.image_size.ok_or_else(|| ObservationError::Api("DetectObjects response omitted image_size".to_string()))?;
  let detections = response
    .detections
    .into_iter()
    .map(|detection| {
      let bbox = detection.bounding_box.ok_or_else(|| ObservationError::Api("DetectObjects detection omitted bounding_box".to_string()))?;
      Ok(Detection {
        class_id: detection.class_id as usize,
        label: detection.label,
        confidence: detection.confidence,
        bbox: BoundingBox {
          x1: bbox.x1,
          y1: bbox.y1,
          x2: bbox.x2,
          y2: bbox.y2,
        },
      })
    })
    .collect::<Result<Vec<_>, ObservationError>>()?;
  Ok(DetectionResult {
    image_size: ImageSize {
      width: size.width,
      height: size.height,
    },
    detections,
  })
}

fn enrich_ui_numeric_readings_from_recognition(
  state: &mut BalatroState,
  capture: &auv_driver::Capture,
  recognition: auv_driver::TextRecognition,
) {
  let capture_bounds = capture.bounds;
  if capture_bounds.size.width <= 0.0 || capture_bounds.size.height <= 0.0 {
    return;
  }
  let x_scale = f64::from(capture.image.width()) / capture_bounds.size.width;
  let y_scale = f64::from(capture.image.height()) / capture_bounds.size.height;
  let readings = state
    .raw_ui
    .iter()
    .filter(|evidence| {
      is_numeric_ui_label(&evidence.detection.label)
        && (matches!(state.phase, BalatroPhase::Playing | BalatroPhase::CashOut) || !is_score_ui_label(&evidence.detection.label))
    })
    .filter_map(|evidence| {
      let bbox = evidence.detection.bbox;
      let pad_x = f64::from(bbox.width().max(1.0)) * 0.12;
      let pad_y = f64::from(bbox.height().max(1.0)) * 0.16;
      let mut matches = recognition
        .regions
        .iter()
        .filter_map(|region| {
          let bounds = region.bounds;
          let x = (bounds.origin.x + bounds.size.width / 2.0 - capture_bounds.origin.x) * x_scale;
          let y = (bounds.origin.y + bounds.size.height / 2.0 - capture_bounds.origin.y) * y_scale;
          (x >= f64::from(bbox.x1) - pad_x
            && x <= f64::from(bbox.x2) + pad_x
            && y >= f64::from(bbox.y1) - pad_y
            && y <= f64::from(bbox.y2) + pad_y)
            .then_some((x, region.text.as_str()))
        })
        .collect::<Vec<_>>();
      matches.sort_by(|left, right| left.0.partial_cmp(&right.0).unwrap_or(std::cmp::Ordering::Equal));
      (!matches.is_empty())
        .then(|| (evidence.detection.label.clone(), matches.into_iter().map(|(_, text)| text).collect::<Vec<_>>().join("")))
    })
    .collect::<Vec<_>>();
  for (label, text) in readings {
    apply_ui_numeric_reading(&label, &text, &mut state.scores, &mut state.rounds);
  }
}

pub(crate) fn is_numeric_ui_label(label: &str) -> bool {
  matches!(
    label,
    "ui_score_chips"
      | "ui_score_current"
      | "ui_score_mult"
      | "ui_score_round_score"
      | "ui_score_target_score"
      | "ui_data_cash"
      | "ui_data_discards_left"
      | "ui_data_hands_left"
      | "ui_round_ante_current"
      | "ui_round_ante_left"
      | "ui_round_round_current"
      | "ui_round_round_left"
  )
}

pub(crate) fn is_score_ui_label(label: &str) -> bool {
  matches!(label, "ui_score_chips" | "ui_score_current" | "ui_score_mult" | "ui_score_round_score" | "ui_score_target_score")
}

pub(crate) fn is_single_ui_digit_label(label: &str) -> bool {
  matches!(
    label,
    "ui_data_discards_left"
      | "ui_data_hands_left"
      | "ui_round_ante_current"
      | "ui_round_ante_left"
      | "ui_round_round_current"
      | "ui_round_round_left"
  )
}

pub(crate) fn apply_ui_numeric_reading(label: &str, text: &str, scores: &mut ScoreState, rounds: &mut RoundState) {
  let Some(value) = normalize_ui_numeric_text_for_label(label, text) else {
    return;
  };
  match label {
    "ui_score_chips" => scores.chips = Some(value),
    "ui_score_current" => scores.current_score = Some(value),
    "ui_score_mult" => scores.mult = Some(value),
    "ui_score_round_score" => scores.round_score = Some(value),
    "ui_score_target_score" => scores.target_score = Some(value),
    "ui_data_cash" => rounds.cash = Some(value),
    "ui_data_discards_left" => rounds.discards_left = Some(value),
    "ui_data_hands_left" => rounds.hands_left = Some(value),
    "ui_round_ante_current" => rounds.ante_current = Some(value),
    "ui_round_ante_left" => rounds.ante_left = Some(value),
    "ui_round_round_current" => rounds.round_current = Some(value),
    "ui_round_round_left" => rounds.round_left = Some(value),
    _ => {}
  }
}

fn normalize_ui_numeric_text_for_label(label: &str, text: &str) -> Option<String> {
  let value = normalize_ui_numeric_text(text)?;
  if is_single_ui_digit_label(label) {
    return value.chars().find(|character| character.is_ascii_digit()).map(|character| character.to_string());
  }
  Some(value)
}

fn normalize_ui_numeric_text(text: &str) -> Option<String> {
  let normalized = text
    .chars()
    .filter_map(|character| match character {
      '0'..='9' | '$' | '/' | '+' | '-' | '.' => Some(character),
      'x' | 'X' | '×' => Some('x'),
      'O' | 'o' | '〇' | '○' => Some('0'),
      ',' | ' ' | '\n' | '\r' | '\t' => None,
      _ => None,
    })
    .collect::<String>();
  (!normalized.is_empty()).then_some(normalized)
}

pub fn observe_image(image_path: impl AsRef<Path>, config: &BalatroModelConfig, no_cache: bool) -> Result<BalatroState, ObservationError> {
  let image_path = image_path.as_ref();
  let image = image::open(image_path)?.to_rgb8();
  let image_size = ImageSize {
    width: image.width(),
    height: image.height(),
  };
  let detectors = BalatroDetectors::load(config.clone())?;
  let detections = detectors.detect_path(image_path)?;

  Ok(build_state_from_detections(image_path.display().to_string(), image_size, &image, detections, no_cache))
}

pub fn build_state_from_detections(
  source: impl Into<String>,
  image_size: ImageSize,
  image: &RgbImage,
  detections: BalatroDetectionSets,
  no_cache: bool,
) -> BalatroState {
  let mut raw_entities = evidence_from_result(&detections.entities, "balatro-entities");
  if let Some(cards) = &detections.cards {
    raw_entities.extend(evidence_from_result(cards, "balatro-cards"));
  }
  if let Some(identity) = &detections.card_attributes.identity {
    raw_entities.extend(evidence_from_result(identity, "balatro-card-identity"));
  }
  if let Some(enhancement) = &detections.card_attributes.enhancement {
    raw_entities.extend(evidence_from_result(enhancement, "balatro-card-enhancement"));
  }
  if let Some(edition) = &detections.card_attributes.edition {
    raw_entities.extend(evidence_from_result(edition, "balatro-card-edition"));
  }
  if let Some(seal) = &detections.card_attributes.seal {
    raw_entities.extend(evidence_from_result(seal, "balatro-card-seal"));
  }
  let raw_ui = evidence_from_result(&detections.ui, "balatro-ui");
  let mut entity_detections = detections.entities.detections;
  let mut hand_detections = detections.cards.map(|cards| cards.detections);
  let card_attributes = detections.card_attributes;
  let mut ui_detections = detections.ui.detections;

  entity_detections.sort_by(compare_left_to_right);
  if let Some(cards) = &mut hand_detections {
    cards.sort_by(compare_left_to_right);
  }
  ui_detections.sort_by(compare_left_to_right);

  let hand = card_slots(
    // TODO(card-detector-empty-result): A configured specialized detector is
    // authoritative even when it returns no boxes; an entities fallback would
    // silently restore the false positives this path removes. Add an explicit
    // fallback reason only with an owner-approved observation contract.
    hand_detections
      .as_deref()
      .unwrap_or(&entity_detections)
      .iter()
      .filter(|detection| matches_label(detection, &["poker_card_front", "poker_card_back"])),
    ObjectZone::Hand,
    image,
    no_cache,
    &card_attributes,
  );
  let jokers = entity_detections
    .iter()
    .filter(|detection| detection.label == "joker_card")
    .filter(|detection| is_owned_joker_candidate(detection, image.width(), image.height()))
    .enumerate()
    .map(|(index, detection)| JokerSlot {
      slot: SlotId::new(ObjectZone::Joker, index as u32),
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect();
  let consumables = entity_detections
    .iter()
    .filter_map(|detection| consumable_kind(&detection.label).map(|kind| (detection, kind)))
    .filter(|(detection, _)| is_owned_consumable_candidate(detection, image.width(), image.height()))
    .enumerate()
    .map(|(index, (detection, kind))| ConsumableSlot {
      slot: SlotId::new(ObjectZone::Consumable, index as u32),
      kind,
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect();
  let buttons: Vec<ButtonTarget> = ui_detections
    .iter()
    .filter(|detection| detection.label.starts_with("button_"))
    .map(|detection| ButtonTarget {
      id: detection.label.clone(),
      label: detection.label.strip_prefix("button_").unwrap_or(&detection.label).to_owned(),
      bbox: detection.bbox,
      confidence: detection.confidence,
    })
    .collect();
  let store = store_state(&entity_detections, &ui_detections, image, no_cache);
  let phase = infer_phase(&entity_detections, &ui_detections);
  let diagnostics = diagnostics_for_detections(&entity_detections, &ui_detections);

  // TODO(balatro-phase-zones): hand detections are not phase-gated yet, so a
  // menu or game-over frame can expose detector false positives as hand slots.
  // Add the gate only after the state contract defines whether raw evidence or
  // an explicit fallback reason remains visible outside `playing`; see
  // `2026-08-05-linux-live-gameplay-evidence.md`.
  BalatroState {
    schema_version: BALATRO_STATE_SCHEMA_VERSION.to_owned(),
    frame: FrameRef {
      source: source.into(),
      image_size,
    },
    phase,
    scores: ScoreState::default(),
    rounds: RoundState::default(),
    hand,
    jokers,
    consumables,
    store,
    buttons,
    diagnostics,
    raw_entities,
    raw_ui,
  }
}

fn is_owned_joker_candidate(detection: &Detection, image_width: u32, image_height: u32) -> bool {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  let center_x = center_x(detection) / width;
  let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0 / height;

  // NOTICE: Owned jokers occupy the persistent upper inventory row. Store
  // jokers use the same detector label, so zone promotion must remain
  // geometry-gated until the detector exposes an explicit entity zone.
  (0.20..=0.82).contains(&center_x) && center_y <= 0.34
}

fn is_owned_consumable_candidate(detection: &Detection, image_width: u32, image_height: u32) -> bool {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  let center_x = center_x(detection) / width;
  let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0 / height;

  // NOTICE: These normalized bounds come from the persistent top-right
  // consumable row in live Balatro frames. They prevent store/pack cards and a
  // confirmed right deck-stack false positive from becoming owned inventory.
  // Replace them when the detector exposes an explicit entity-zone contract.
  (0.50..=0.90).contains(&center_x) && center_y <= 0.34
}

fn diagnostics_for_detections(entities: &[Detection], ui: &[Detection]) -> Vec<BalatroDiagnostic> {
  if entities.is_empty() && ui.is_empty() {
    return vec![BalatroDiagnostic {
      code: "empty_detection_sets".to_string(),
      message: "Balatro detectors returned no entity or UI boxes for this frame".to_string(),
    }];
  }
  Vec::new()
}

fn evidence_from_result(result: &DetectionResult, model: &str) -> Vec<ObjectEvidence> {
  result
    .detections
    .iter()
    .cloned()
    .map(|detection| ObjectEvidence {
      model: model.to_owned(),
      detection,
    })
    .collect()
}

fn card_slots<'a>(
  detections: impl Iterator<Item = &'a Detection>,
  zone: ObjectZone,
  image: &RgbImage,
  no_cache: bool,
  attributes: &CardAttributeDetectionSets,
) -> Vec<CardSlot> {
  let detections = detections.collect::<Vec<_>>();
  let identities =
    match_card_attributes(&detections, attributes.identity.as_ref().map(|result| result.detections.as_slice()).unwrap_or_default());
  let enhancements =
    match_card_attributes(&detections, attributes.enhancement.as_ref().map(|result| result.detections.as_slice()).unwrap_or_default());
  let editions =
    match_card_attributes(&detections, attributes.edition.as_ref().map(|result| result.detections.as_slice()).unwrap_or_default());
  let seals = match_card_attributes(&detections, attributes.seal.as_ref().map(|result| result.detections.as_slice()).unwrap_or_default());
  detections
    .into_iter()
    .enumerate()
    .map(|(index, detection)| {
      let identity = identities[index];
      let reading = match identity {
        Some(identity) if !matches!(identity.label.as_str(), "card_back" | "rank_suit_unreadable") => Reading {
          status: ReadingStatus::Read,
          text: Some(identity.label.clone()),
          confidence: Some(identity.confidence),
        },
        _ => Reading::unread(),
      };
      let kind = match identity.map(|identity| identity.label.as_str()) {
        Some("card_back") => "poker_card_back".to_string(),
        Some("rank_suit_unreadable") | Some(_) => "poker_card_front".to_string(),
        None => detection.label.clone(),
      };
      CardSlot {
        slot: SlotId::new(zone, index as u32),
        kind,
        bbox: detection.bbox,
        confidence: detection.confidence,
        reading,
        attributes: CardAttributes {
          enhancement: card_attribute(enhancements[index]),
          edition: card_attribute(editions[index]),
          seal: card_attribute(seals[index]),
        },
        cache: cache_hint_for_detection(detection, image, no_cache),
      }
    })
    .collect()
}

fn card_attribute(attribute: Option<&Detection>) -> Option<CardAttribute> {
  attribute.map(|attribute| CardAttribute {
    label: attribute.label.clone(),
    confidence: attribute.confidence,
  })
}

fn match_card_attributes<'a>(cards: &[&Detection], attributes: &'a [Detection]) -> Vec<Option<&'a Detection>> {
  let mut candidates = cards
    .iter()
    .enumerate()
    .flat_map(|(card_index, card)| {
      attributes.iter().enumerate().filter_map(move |(attribute_index, attribute)| {
        let overlap = bbox_iou(card.bbox, attribute.bbox);
        (overlap >= 0.2).then_some((card_index, attribute_index, overlap))
      })
    })
    .collect::<Vec<_>>();
  candidates.sort_by(|left, right| right.2.partial_cmp(&left.2).unwrap_or(std::cmp::Ordering::Equal));

  let mut matches = vec![None; cards.len()];
  let mut used_attributes = vec![false; attributes.len()];
  for (card_index, attribute_index, _) in candidates {
    if matches[card_index].is_none() && !used_attributes[attribute_index] {
      matches[card_index] = Some(&attributes[attribute_index]);
      used_attributes[attribute_index] = true;
    }
  }
  matches
}

fn bbox_iou(left: BoundingBox, right: BoundingBox) -> f32 {
  let intersection_width = (left.x2.min(right.x2) - left.x1.max(right.x1)).max(0.0);
  let intersection_height = (left.y2.min(right.y2) - left.y1.max(right.y1)).max(0.0);
  let intersection = intersection_width * intersection_height;
  let left_area = left.width().max(0.0) * left.height().max(0.0);
  let right_area = right.width().max(0.0) * right.height().max(0.0);
  let union = left_area + right_area - intersection;
  if union <= 0.0 {
    0.0
  } else {
    intersection / union
  }
}

fn store_state(entities: &[Detection], ui: &[Detection], image: &RgbImage, no_cache: bool) -> StoreState {
  let can_reroll = ui.iter().any(|detection| detection.label == "button_store_reroll");
  let can_next_round = ui.iter().any(|detection| detection.label == "button_store_next_round");
  let is_store = can_reroll || can_next_round || ui.iter().any(is_store_control);
  let items = if is_store {
    store_items_for_store_context(entities, image, no_cache)
  } else {
    Vec::new()
  };
  let item_count = items.len() as u32;

  StoreState {
    is_store,
    item_count,
    can_reroll,
    can_next_round,
    items,
  }
}

fn store_items_for_store_context(entities: &[Detection], image: &RgbImage, no_cache: bool) -> Vec<StoreItem> {
  let candidates = entities
    .iter()
    .filter_map(|detection| store_item_kind(&detection.label).map(|kind| (detection, kind)))
    .filter(|(detection, kind)| is_store_item_candidate(detection, kind, image.width(), image.height()))
    .collect::<Vec<_>>();
  let mut distinct = Vec::<(&Detection, StoreItemKind)>::new();
  for (detection, kind) in candidates {
    if let Some(index) = distinct.iter().position(|(existing, _)| bbox_iou(existing.bbox, detection.bbox) >= 0.75) {
      if detection.confidence > distinct[index].0.confidence {
        distinct[index] = (detection, kind);
      }
    } else {
      distinct.push((detection, kind));
    }
  }
  distinct.sort_by(|(left, _), (right, _)| compare_left_to_right(left, right));
  let mut items = distinct
    .into_iter()
    .enumerate()
    .map(|(index, (detection, kind))| StoreItem {
      slot: SlotId::new(ObjectZone::Store, index as u32),
      kind,
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect::<Vec<_>>();

  append_voucher_layout_candidate(&mut items, image.width(), image.height());
  items
}

fn is_store_item_candidate(detection: &Detection, kind: &StoreItemKind, image_width: u32, image_height: u32) -> bool {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  let center_x = center_x(detection) / width;
  let center_y = (detection.bbox.y1 + detection.bbox.y2) / 2.0 / height;

  let (max_x, min_y, max_y) = match kind {
    StoreItemKind::CardPack => (0.90, 0.22, 0.96),
    _ => (0.82, 0.32, 0.75),
  };

  // Thresholds are normalized from live Balatro store captures: keep store
  // products while excluding top owned joker/consumable rows and the visually
  // anchored right deck stack. Card packs can sit in the lower shop row at
  // smaller window scales, so their vertical band is intentionally taller than
  // ordinary card products.
  (0.20..=max_x).contains(&center_x) && (min_y..=max_y).contains(&center_y)
}

fn infer_phase(entities: &[Detection], ui: &[Detection]) -> BalatroPhase {
  if ui.iter().any(is_store_control) {
    BalatroPhase::Store
  } else if ui.iter().any(|detection| detection.label == "button_cash_out") {
    BalatroPhase::CashOut
  } else if ui.iter().any(|detection| matches_label(detection, &["button_play", "button_discard"]))
    || (entities.iter().any(is_hand_card) && ui.iter().any(is_hand_sort_control))
  {
    BalatroPhase::Playing
  } else if ui.iter().any(|detection| {
    matches_label(
      detection,
      &[
        "button_select_blind",
        "button_skip_blind",
        "button_level_select",
      ],
    )
  }) {
    BalatroPhase::BlindSelect
  } else if ui.iter().any(|detection| detection.label == "button_new_run")
    && ui.iter().any(|detection| detection.label == "button_main_menu")
  {
    BalatroPhase::GameOver
  } else if ui.iter().any(|detection| {
    matches_label(
      detection,
      &[
        "button_main_menu_play",
        "button_new_run",
        "button_new_run_play",
      ],
    )
  }) {
    BalatroPhase::MainMenu
  } else {
    BalatroPhase::Unknown
  }
}

fn is_hand_card(detection: &Detection) -> bool {
  matches_label(detection, &["poker_card_front", "poker_card_back"])
}

fn is_hand_sort_control(detection: &Detection) -> bool {
  matches_label(detection, &["button_sort_hand_rank", "button_sort_hand_suits"])
}

fn is_store_control(detection: &Detection) -> bool {
  matches_label(
    detection,
    &[
      "button_store_reroll",
      "button_store_next_round",
      "button_purchase",
    ],
  )
}

fn consumable_kind(label: &str) -> Option<ConsumableKind> {
  match label {
    "tarot_card" => Some(ConsumableKind::Tarot),
    "planet_card" => Some(ConsumableKind::Planet),
    "spectral_card" => Some(ConsumableKind::Spectral),
    _ => None,
  }
}

fn store_item_kind(label: &str) -> Option<StoreItemKind> {
  match label {
    "joker_card" => Some(StoreItemKind::Joker),
    "tarot_card" => Some(StoreItemKind::Tarot),
    "planet_card" => Some(StoreItemKind::Planet),
    "spectral_card" => Some(StoreItemKind::Spectral),
    "card_pack" => Some(StoreItemKind::CardPack),
    "poker_card_front" => Some(StoreItemKind::PlayingCard),
    // TODO(voucher-detection): detector-backed voucher labels are deferred
    // until the entities dataset grows that class. Store observation currently
    // adds a low-confidence layout fallback candidate instead.
    _ => None,
  }
}

fn append_voucher_layout_candidate(items: &mut Vec<StoreItem>, image_width: u32, image_height: u32) {
  if image_width < 600 || image_height < 400 {
    return;
  }
  if !items.iter().any(|item| item.kind != StoreItemKind::CardPack) {
    return;
  }
  let bbox = voucher_layout_bbox(image_width, image_height);
  if items.iter().any(|item| bbox_overlap_ratio(item.bbox, bbox) > 0.25) {
    return;
  }
  let slot = SlotId::new(ObjectZone::Store, items.len() as u32);
  items.push(StoreItem {
    slot,
    kind: StoreItemKind::Voucher,
    bbox,
    confidence: 0.35,
    reading: Reading::unread(),
    cache: CacheHint {
      needs_reading: true,
      visual_fingerprint: None,
      changed_since_last_read: false,
    },
  });
}

fn voucher_layout_bbox(image_width: u32, image_height: u32) -> BoundingBox {
  let width = image_width.max(1) as f32;
  let height = image_height.max(1) as f32;
  BoundingBox {
    x1: width * 0.22,
    y1: height * 0.58,
    x2: width * 0.39,
    y2: height * 0.86,
  }
}

fn bbox_overlap_ratio(left: BoundingBox, right: BoundingBox) -> f32 {
  let x1 = left.x1.max(right.x1);
  let y1 = left.y1.max(right.y1);
  let x2 = left.x2.min(right.x2);
  let y2 = left.y2.min(right.y2);
  let overlap_width = (x2 - x1).max(0.0);
  let overlap_height = (y2 - y1).max(0.0);
  let overlap_area = overlap_width * overlap_height;
  let right_area = ((right.x2 - right.x1).max(0.0) * (right.y2 - right.y1).max(0.0)).max(1.0);
  overlap_area / right_area
}

fn matches_label(detection: &Detection, labels: &[&str]) -> bool {
  labels.iter().any(|label| detection.label == *label)
}

fn compare_left_to_right(left: &Detection, right: &Detection) -> std::cmp::Ordering {
  center_x(left).partial_cmp(&center_x(right)).unwrap_or(std::cmp::Ordering::Equal)
}

fn center_x(detection: &Detection) -> f32 {
  (detection.bbox.x1 + detection.bbox.x2) / 2.0
}

#[cfg(test)]
#[path = "observation_test.rs"]
mod tests;
