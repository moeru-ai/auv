use std::collections::HashMap;
use std::path::Path;

use auv_inference_common::{ImageSize, InferenceError};
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult};
use image::RgbImage;
use thiserror::Error;

use auv_api_client::placement::{AuvClient, RunClient, RunOptions, RunnerOptions};
use auv_api_client::{AuvContext, Client, ConnectEndpoint};
use auv_api_proto::auv::api::{
  core::v1 as core_proto, driver::v1 as driver_proto, image::v1 as image_proto, inference::v1 as inference_proto,
};

use crate::cache::cache_hint_for_detection;
use crate::config::BalatroModelConfig;
use crate::detector::{BalatroDetectionSets, BalatroDetectors};
use crate::model::{
  BALATRO_STATE_SCHEMA_VERSION, BalatroDiagnostic, BalatroPhase, BalatroState, ButtonTarget, CacheHint, CardSlot, ConsumableKind,
  ConsumableSlot, FrameRef, JokerSlot, ObjectEvidence, ObjectZone, Reading, RoundState, ScoreState, SlotId, StoreItem, StoreItemKind,
  StoreState,
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

/// Observes an image through an inference Runner attached to one AUV Run.
pub async fn observe_image_via_api(
  image_path: impl AsRef<Path>,
  config: &BalatroModelConfig,
  endpoint: Option<ConnectEndpoint>,
  no_cache: bool,
) -> Result<BalatroState, ObservationError> {
  let image_path = image_path.as_ref();
  let resolved = config.resolve().map_err(|error| ObservationError::Api(error.to_string()))?;
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
  let entities_classes = crate::config::load_class_names(&resolved.entities_classes)?;
  let ui_classes = crate::config::load_class_names(&resolved.ui_classes)?;
  let auv = connect_auv(endpoint).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let inference = match run.runner(inference_runner_options()).await {
    Ok(inference) => inference,
    Err(error) => return finish_run(run, Err(api_error(error))).await,
  };
  let result = detect_via_api(&inference, &resolved, entities_classes, ui_classes, frame)
    .await
    .map(|detections| build_state_from_detections(image_path.display().to_string(), image_size, &image, detections, no_cache));
  let result = release_runner(inference, result).await;
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
  let resolved = config.resolve().map_err(|error| ObservationError::Api(error.to_string()))?;
  let entities_classes = crate::config::load_class_names(&resolved.entities_classes)?;
  let ui_classes = crate::config::load_class_names(&resolved.ui_classes)?;
  let auv = connect_auv(endpoint).await?;
  let run = auv.run(balatro_run_options()).await.map_err(api_error)?;
  let driver = match run.runner(driver_runner_options()).await {
    Ok(driver) => driver,
    Err(error) => return finish_run(run, Err(api_error(error))).await,
  };
  let inference = match run.runner(inference_runner_options()).await {
    Ok(inference) => inference,
    Err(error) => {
      let result = release_runner(driver, Err(api_error(error))).await;
      return finish_run(run, result).await;
    }
  };
  let result = observe_live_with_runners(&driver, &inference, target, &resolved, entities_classes, ui_classes, no_cache).await;
  let result = release_runner(inference, result).await;
  let result = release_runner(driver, result).await;
  finish_run(run, result).await
}

async fn connect_auv(endpoint: Option<ConnectEndpoint>) -> Result<AuvClient, ObservationError> {
  match std::env::var("AUV_CONTEXT") {
    Ok(_) => AuvClient::from_context(AuvContext::from_env().map_err(api_error)?).await.map_err(api_error),
    Err(std::env::VarError::NotPresent) => match endpoint {
      Some(endpoint) => Client::connect(endpoint).await.map(|client| client.placement()).map_err(api_error),
      None => AuvClient::from_env_or_local().await.map_err(api_error),
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
    runner_class: "auv.core.local".to_string(),
    required_capabilities: vec![
      capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
      capability("auv.api.driver.v1.CaptureService", &["CaptureWindow"]),
      capability("auv.api.driver.v1.TextRecognitionService", &["RecognizeText"]),
    ],
    lifecycle: core_proto::RunnerLifecycle::UnlessIdle,
    idle_timeout: Some(runner_idle_timeout()),
    ..RunnerOptions::default()
  }
}

fn inference_runner_options() -> RunnerOptions {
  RunnerOptions {
    runner_class: "auv.inference.ultralytics".to_string(),
    required_capabilities: vec![capability(
      "auv.api.inference.v1.ObjectDetectionService",
      &["DetectObjects"],
    )],
    lifecycle: core_proto::RunnerLifecycle::UnlessIdle,
    idle_timeout: Some(runner_idle_timeout()),
    ..RunnerOptions::default()
  }
}

fn capability(service: &str, methods: &[&str]) -> core_proto::RunnerCapability {
  core_proto::RunnerCapability {
    service: service.to_string(),
    methods: methods.iter().map(|method| (*method).to_string()).collect(),
  }
}

fn runner_idle_timeout() -> prost_types::Duration {
  prost_types::Duration {
    seconds: 60,
    nanos: 0,
  }
}

async fn release_runner<T>(
  runner: auv_api_client::driver::RunnerClient,
  result: Result<T, ObservationError>,
) -> Result<T, ObservationError> {
  merge_cleanup(result, runner.release().await.map(|_| ()).map_err(api_error))
}

async fn finish_run<T>(run: RunClient, result: Result<T, ObservationError>) -> Result<T, ObservationError> {
  let outcome = if result.is_ok() {
    core_proto::RunOutcome::Succeeded
  } else {
    core_proto::RunOutcome::Failed
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

fn rgba_frame_to_rgb(frame: &image_proto::RgbaFrame) -> Result<image_proto::RgbFrame, ObservationError> {
  let pixel_count = usize::try_from(frame.width)
    .ok()
    .and_then(|width| usize::try_from(frame.height).ok().and_then(|height| width.checked_mul(height)))
    .ok_or_else(|| ObservationError::Api("CaptureWindow RGBA dimensions exceed addressable memory".to_string()))?;
  let expected = pixel_count
    .checked_mul(4)
    .ok_or_else(|| ObservationError::Api("CaptureWindow RGBA byte length exceeds addressable memory".to_string()))?;
  if frame.data.len() != expected {
    return Err(ObservationError::Api(format!(
      "CaptureWindow returned malformed RGBA frame: expected {expected} bytes, received {}",
      frame.data.len()
    )));
  }
  let mut data = Vec::with_capacity(pixel_count * 3);
  for pixel in frame.data.chunks_exact(4) {
    data.extend_from_slice(&pixel[..3]);
  }
  Ok(image_proto::RgbFrame {
    width: frame.width,
    height: frame.height,
    data,
  })
}

async fn detect_via_api(
  inference: &auv_api_client::driver::RunnerClient,
  resolved: &crate::config::ResolvedBalatroModelConfig,
  entities_classes: Vec<String>,
  ui_classes: Vec<String>,
  frame: image_proto::RgbFrame,
) -> Result<BalatroDetectionSets, ObservationError> {
  let inference = inference.inference();
  let entities = inference
    .detect_objects(detector_spec("balatro-entities", &resolved.entities_model, &resolved.device, entities_classes)?, frame.clone())
    .await
    .map_err(|error| ObservationError::Api(error.to_string()))?;
  let ui = inference
    .detect_objects(detector_spec("balatro-ui", &resolved.ui_model, &resolved.device, ui_classes)?, frame)
    .await
    .map_err(|error| ObservationError::Api(error.to_string()))?;
  Ok(BalatroDetectionSets {
    entities: detection_result_from_proto(entities)?,
    ui: detection_result_from_proto(ui)?,
  })
}

async fn observe_live_with_runners(
  driver: &auv_api_client::driver::RunnerClient,
  inference: &auv_api_client::driver::RunnerClient,
  target: &str,
  resolved: &crate::config::ResolvedBalatroModelConfig,
  entities_classes: Vec<String>,
  ui_classes: Vec<String>,
  no_cache: bool,
) -> Result<BalatroState, ObservationError> {
  let window = driver
    .windows()
    .resolve(driver_proto::WindowSelector {
      application: Some(driver_proto::window_selector::Application::ApplicationName(target.to_string())),
      window: Some(driver_proto::window_selector::Window::MainVisible(true)),
    })
    .await
    .map_err(api_error)?;
  let captured = window.capture().await.map_err(api_error)?;
  let capture = captured.capture.ok_or_else(|| ObservationError::Api("CaptureWindow response omitted capture".to_string()))?;
  let rgba = capture.image.as_ref().ok_or_else(|| ObservationError::Api("CaptureWindow response omitted image".to_string()))?;
  let frame = rgba_frame_to_rgb(rgba)?;
  let image = RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
    .ok_or_else(|| ObservationError::Api("CaptureWindow returned malformed RGBA frame".to_string()))?;
  let image_size = ImageSize {
    width: frame.width,
    height: frame.height,
  };
  let recognition =
    driver.recognize_text(capture.clone(), None, Vec::new(), vec!["zh-Hans".to_string(), "en-US".to_string()]).await.map_err(api_error)?;
  let detections = detect_via_api(inference, resolved, entities_classes, ui_classes, frame).await?;
  let mut state =
    build_state_from_detections(format!("daemon://window/{}", window.reference().window_id), image_size, &image, detections, no_cache);
  enrich_ui_numeric_readings_from_recognition(&mut state, &capture, recognition);
  Ok(state)
}

fn detector_spec(
  detector_id: &str,
  model_path: &Path,
  device: &auv_inference_ultralytics::InferenceDevice,
  class_names: Vec<String>,
) -> Result<inference_proto::ObjectDetectorSpec, ObservationError> {
  let (kind, index) = match device {
    auv_inference_ultralytics::InferenceDevice::Cpu => (inference_proto::InferenceDeviceKind::Cpu, None),
    auv_inference_ultralytics::InferenceDevice::Cuda(index) => (inference_proto::InferenceDeviceKind::Cuda, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::CoreMl => (inference_proto::InferenceDeviceKind::CoreMl, None),
    auv_inference_ultralytics::InferenceDevice::DirectMl(index) => (inference_proto::InferenceDeviceKind::DirectMl, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::OpenVino => (inference_proto::InferenceDeviceKind::OpenVino, None),
    auv_inference_ultralytics::InferenceDevice::Xnnpack => (inference_proto::InferenceDeviceKind::Xnnpack, None),
    auv_inference_ultralytics::InferenceDevice::TensorRt(index) => (inference_proto::InferenceDeviceKind::TensorRt, Some(*index)),
    auv_inference_ultralytics::InferenceDevice::Rocm(index) => (inference_proto::InferenceDeviceKind::Rocm, Some(*index)),
  };
  let index = index
    .map(u32::try_from)
    .transpose()
    .map_err(|_| ObservationError::Api("inference device index exceeds the protobuf uint32 range".to_string()))?;
  Ok(inference_proto::ObjectDetectorSpec {
    detector_id: detector_id.to_string(),
    model_path: model_path
      .to_str()
      .ok_or_else(|| ObservationError::Api(format!("model path is not valid UTF-8: {}", model_path.display())))?
      .to_string(),
    input_size: Some(640),
    confidence_threshold: Some(0.25),
    iou_threshold: Some(0.45),
    max_detections: Some(300),
    device: Some(inference_proto::InferenceDevice {
      kind: kind as i32,
      index,
    }),
    class_names,
  })
}

fn detection_result_from_proto(response: inference_proto::DetectObjectsResponse) -> Result<DetectionResult, ObservationError> {
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
  capture: &driver_proto::CapturedFrame,
  recognition: driver_proto::RecognizeTextResponse,
) {
  let Some(frame) = capture.image.as_ref() else {
    return;
  };
  let Some(capture_bounds) = capture.bounds.as_ref() else {
    return;
  };
  if capture_bounds.width <= 0.0 || capture_bounds.height <= 0.0 {
    return;
  }
  let x_scale = f64::from(frame.width) / capture_bounds.width;
  let y_scale = f64::from(frame.height) / capture_bounds.height;
  let readings = state
    .raw_ui
    .iter()
    .filter(|evidence| {
      is_numeric_ui_label(&evidence.detection.label)
        && (state.phase == BalatroPhase::Playing || !is_score_ui_label(&evidence.detection.label))
    })
    .filter_map(|evidence| {
      let bbox = evidence.detection.bbox;
      let pad_x = f64::from(bbox.width().max(1.0)) * 0.12;
      let pad_y = f64::from(bbox.height().max(1.0)) * 0.16;
      let mut matches = recognition
        .regions
        .iter()
        .filter_map(|region| {
          let bounds = region.bounds.as_ref()?;
          let x = (bounds.x + bounds.width / 2.0 - capture_bounds.x) * x_scale;
          let y = (bounds.y + bounds.height / 2.0 - capture_bounds.y) * y_scale;
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
  let raw_entities = evidence_from_result(&detections.entities, "balatro-entities");
  let raw_ui = evidence_from_result(&detections.ui, "balatro-ui");
  let mut entity_detections = detections.entities.detections;
  let mut ui_detections = detections.ui.detections;

  entity_detections.sort_by(compare_left_to_right);
  ui_detections.sort_by(compare_left_to_right);

  let hand = card_slots(
    entity_detections.iter().filter(|detection| matches_label(detection, &["poker_card_front", "poker_card_back"])),
    ObjectZone::Hand,
    image,
    no_cache,
  );
  let jokers = entity_detections
    .iter()
    .filter(|detection| detection.label == "joker_card")
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

fn card_slots<'a>(detections: impl Iterator<Item = &'a Detection>, zone: ObjectZone, image: &RgbImage, no_cache: bool) -> Vec<CardSlot> {
  detections
    .enumerate()
    .map(|(index, detection)| CardSlot {
      slot: SlotId::new(zone, index as u32),
      kind: detection.label.clone(),
      bbox: detection.bbox,
      confidence: detection.confidence,
      reading: Reading::unread(),
      cache: cache_hint_for_detection(detection, image, no_cache),
    })
    .collect()
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
  let mut items = entities
    .iter()
    .filter_map(|detection| store_item_kind(&detection.label).map(|kind| (detection, kind)))
    .filter(|(detection, kind)| is_store_item_candidate(detection, kind, image.width(), image.height()))
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
