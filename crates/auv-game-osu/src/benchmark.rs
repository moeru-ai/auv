use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rosu_map::Beatmap;
use rosu_map::section::hit_objects::HitObjectKind;
use serde::{Deserialize, Serialize};

use crate::projection::{PlayfieldProjection, ProjectionArtifact};
use crate::visual_eval::{
  DetectorEvalProvenance, FrameDetections, FrameKey, LabelMap, VisualEvalReport,
  evaluate_visual_truth_with_provenance,
};
use crate::visual_truth::{VisualTruthManifest, build_visual_truth_manifest};
use auv_driver::InputActionResult;
use auv_inference_common::{ClassLabelSource, DetectionEvidenceManifest, DetectionSet};

#[cfg(target_os = "macos")]
use auv_driver::capture::Capture;
#[cfg(target_os = "macos")]
use auv_driver::{
  App, Click, ClickOptions, Driver, InputPolicy, WindowClickStrategy, WindowPoint, WindowSelector,
};
#[cfg(target_os = "macos")]
use auv_driver_macos::MacosDriver;

pub type OsuResult<T> = Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
  DryRun,
  TypedDispatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkInputs {
  pub beatmap_path: PathBuf,
  pub output_dir: PathBuf,
  pub lead_in_ms: u64,
  pub run_mode: RunMode,
  pub target_app: Option<String>,
  pub dispatch_limit: Option<usize>,
  pub capture_verify: bool,
  pub pre_capture_offset_ms: u64,
  pub post_capture_offsets_ms: Vec<u64>,
}

impl BenchmarkInputs {
  pub fn new(beatmap_path: PathBuf, output_dir: PathBuf) -> Self {
    Self {
      beatmap_path,
      output_dir,
      lead_in_ms: 25,
      run_mode: RunMode::DryRun,
      target_app: None,
      dispatch_limit: None,
      capture_verify: false,
      pre_capture_offset_ms: default_pre_capture_offset_ms(),
      post_capture_offsets_ms: default_post_capture_offsets_ms(),
    }
  }

  pub fn typed_dispatch(
    beatmap_path: PathBuf,
    output_dir: PathBuf,
    target_app: impl Into<String>,
  ) -> Self {
    Self {
      beatmap_path,
      output_dir,
      lead_in_ms: 25,
      run_mode: RunMode::TypedDispatch,
      target_app: Some(target_app.into()),
      dispatch_limit: Some(8),
      capture_verify: false,
      pre_capture_offset_ms: default_pre_capture_offset_ms(),
      post_capture_offsets_ms: default_post_capture_offsets_ms(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
  Circle,
  Slider,
  Spinner,
  Hold,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapSummary {
  pub beatmap_path: String,
  pub mode: u8,
  pub total_objects: usize,
  pub circle_count: usize,
  pub slider_count: usize,
  pub spinner_count: usize,
  pub hold_count: usize,
  pub first_object_time_ms: Option<u64>,
  pub last_object_time_ms: Option<u64>,
  pub approach_rate: f32,
  pub overall_difficulty: f32,
  pub circle_size: f32,
  pub hp_drain_rate: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduledAction {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub x: f32,
  pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DispatchSample {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub actual_dispatch_time_ms: u64,
  pub dispatch_error_ms: i64,
  pub x: f32,
  pub y: f32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub delivery_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureSample {
  pub phase: CapturePhase,
  pub capture_time_ms: u64,
  pub relative_to_scheduled_ms: i64,
  pub relative_to_dispatch_ms: i64,
  pub file_name: String,
  pub width: u32,
  pub height: u32,
  pub backend: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePhase {
  BeforeDispatch,
  AfterDispatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureTraceSample {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub actual_dispatch_time_ms: u64,
  pub dispatch_error_ms: i64,
  pub captures: Vec<CaptureSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationSummary {
  pub capture_enabled: bool,
  pub captured_action_count: usize,
  pub missing_frame_count: usize,
  pub max_capture_delay_ms: i64,
  pub suspicious_time_inversion_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatencyReport {
  pub run_mode: RunMode,
  pub total_actions: usize,
  pub mean_error_ms: f64,
  pub p50_error_ms: i64,
  pub p95_error_ms: i64,
  pub p99_error_ms: i64,
  pub max_error_ms: i64,
  pub jitter_ms: i64,
  pub missed_schedule_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkEvidenceSummary {
  pub dispatch_sample_count: usize,
  pub capture_artifact_count: usize,
  pub has_projection_artifact: bool,
  pub has_visual_truth_manifest: bool,
  pub has_visual_eval_report: bool,
  pub missed_schedule_count: usize,
  pub verification_captured_action_count: usize,
  pub verification_missing_frame_count: usize,
  pub verification_suspicious_time_inversion_count: usize,
  pub evidence_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkOutput {
  pub map_summary: MapSummary,
  pub schedule: Vec<ScheduledAction>,
  pub dispatch_trace: Vec<DispatchSample>,
  pub capture_trace: Vec<CaptureTraceSample>,
  pub latency_report: LatencyReport,
  pub verification_summary: Option<VerificationSummary>,
  pub visual_truth_manifest: Option<VisualTruthManifest>,
  pub projection: Option<ProjectionArtifact>,
  pub visual_eval_report: Option<VisualEvalReport>,
  pub evidence_summary: BenchmarkEvidenceSummary,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectionEvalInputs {
  pub run_artifact_dir: PathBuf,
  pub detections_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalManifest {
  pub source_run_artifact_dir: String,
  pub detections_path: String,
  pub detector_model_id: String,
  pub label_map_source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionEvalOutput {
  pub output_dir: PathBuf,
  pub visual_eval_report: VisualEvalReport,
  pub detection_eval_manifest: DetectionEvalManifest,
}

#[derive(Clone, Debug, Deserialize)]
struct DetectionFileRecord {
  capture_file_name: String,
  detection_set: DetectionSet,
}

pub fn run_benchmark(inputs: &BenchmarkInputs) -> OsuResult<BenchmarkOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create osu benchmark output dir {}: {error}",
      inputs.output_dir.display()
    )
  })?;

  let beatmap = Beatmap::from_path(&inputs.beatmap_path).map_err(|error| {
    format!(
      "failed to parse beatmap {}: {error}",
      inputs.beatmap_path.display()
    )
  })?;

  let schedule = build_schedule(&beatmap);
  if schedule.is_empty() {
    return Err(format!(
      "beatmap {} contains no hit objects",
      inputs.beatmap_path.display()
    ));
  }

  let map_summary = build_map_summary(&inputs.beatmap_path, &beatmap, &schedule);
  let (dispatch_trace, capture_trace, verification_summary, projection) = match inputs.run_mode {
    RunMode::DryRun => (
      run_dry_schedule(&schedule, inputs.lead_in_ms),
      Vec::new(),
      None,
      None,
    ),
    RunMode::TypedDispatch => run_typed_dispatch(schedule.as_slice(), &map_summary, inputs)?,
  };
  let latency_report = build_latency_report(inputs.run_mode.clone(), &dispatch_trace);
  let visual_truth_manifest = if inputs.capture_verify {
    Some(build_visual_truth_manifest(
      &map_summary,
      &schedule,
      &dispatch_trace,
      &capture_trace,
    )?)
  } else {
    None
  };

  let visual_eval_report = None;

  let evidence_summary = build_evidence_summary(
    &dispatch_trace,
    &capture_trace,
    &latency_report,
    verification_summary.as_ref(),
    visual_truth_manifest.as_ref(),
    projection.as_ref(),
    visual_eval_report.as_ref(),
  );

  write_json(
    inputs.output_dir.join("parsed_map_summary.json"),
    &map_summary,
  )?;
  write_json(inputs.output_dir.join("action_schedule.json"), &schedule)?;
  write_json(
    inputs.output_dir.join("dispatch_trace.json"),
    &dispatch_trace,
  )?;
  write_json(
    inputs.output_dir.join("latency_report.json"),
    &latency_report,
  )?;
  if inputs.capture_verify {
    write_json(inputs.output_dir.join("capture_trace.json"), &capture_trace)?;
    write_json(
      inputs.output_dir.join("verification_summary.json"),
      &verification_summary,
    )?;
    if let Some(manifest) = &visual_truth_manifest {
      write_json(
        inputs.output_dir.join("visual_truth_manifest.json"),
        manifest,
      )?;
    }
  }
  if let Some(projection) = &projection {
    write_json(inputs.output_dir.join("projection.json"), projection)?;
  }
  if let Some(report) = &visual_eval_report {
    write_json(inputs.output_dir.join("visual_eval_report.json"), report)?;
  }
  write_json(
    inputs.output_dir.join("evidence_summary.json"),
    &evidence_summary,
  )?;

  Ok(BenchmarkOutput {
    map_summary,
    schedule,
    dispatch_trace,
    capture_trace,
    latency_report,
    verification_summary,
    visual_truth_manifest,
    projection,
    visual_eval_report,
    evidence_summary,
    output_dir: inputs.output_dir.clone(),
  })
}

#[cfg(test)]
fn build_visual_eval_report(
  manifest: &VisualTruthManifest,
  projection_artifact: &ProjectionArtifact,
  detections_by_frame: &[FrameDetections],
) -> OsuResult<VisualEvalReport> {
  let projection = projection_artifact
    .to_eval_projection()
    .map_err(|error| format!("failed to adapt projection artifact for visual eval: {error}"))?;
  Ok(crate::visual_eval::evaluate_visual_truth(
    manifest,
    detections_by_frame,
    &projection,
    &LabelMap::default(),
  ))
}

pub fn evaluate_detection_fixture(inputs: &DetectionEvalInputs) -> OsuResult<DetectionEvalOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create detection eval output dir {}: {error}",
      inputs.output_dir.display()
    )
  })?;

  let manifest =
    read_json::<VisualTruthManifest>(&inputs.run_artifact_dir.join("visual_truth_manifest.json"))?;
  let projection_artifact =
    read_json::<ProjectionArtifact>(&inputs.run_artifact_dir.join("projection.json"))?;
  let loaded = load_detection_inputs(&inputs.detections_path)?;
  let label_map = LabelMap::default();
  let projection = projection_artifact
    .to_eval_projection()
    .map_err(|error| format!("failed to adapt projection artifact for visual eval: {error}"))?;
  let report = evaluate_visual_truth_with_provenance(
    &manifest,
    &expand_frame_detections(&manifest, &loaded.detections_by_capture)?,
    &projection,
    &label_map,
    Some(DetectorEvalProvenance {
      model_id: loaded.model_id.clone(),
      label_map_source: loaded.label_map_source.clone(),
    }),
  );

  let detection_eval_manifest = DetectionEvalManifest {
    source_run_artifact_dir: inputs.run_artifact_dir.display().to_string(),
    detections_path: inputs.detections_path.display().to_string(),
    detector_model_id: loaded.model_id,
    label_map_source: loaded.label_map_source,
  };

  write_json(inputs.output_dir.join("visual_eval_report.json"), &report)?;
  write_json(
    inputs.output_dir.join("detection_eval_manifest.json"),
    &detection_eval_manifest,
  )?;

  Ok(DetectionEvalOutput {
    output_dir: inputs.output_dir.clone(),
    visual_eval_report: report,
    detection_eval_manifest,
  })
}

fn build_schedule(beatmap: &Beatmap) -> Vec<ScheduledAction> {
  beatmap
    .hit_objects
    .iter()
    .enumerate()
    .map(|(index, object)| {
      let (object_kind, x, y) = scheduled_target(&object.kind);
      ScheduledAction {
        object_index: index,
        object_kind,
        scheduled_time_ms: object.start_time.max(0.0).round() as u64,
        x,
        y,
      }
    })
    .collect()
}

struct LoadedDetectionInputs {
  model_id: String,
  label_map_source: String,
  detections_by_capture: BTreeMap<String, DetectionSet>,
}

fn load_detection_inputs(path: &Path) -> OsuResult<LoadedDetectionInputs> {
  if path.is_dir() {
    return load_detection_dir(path);
  }

  if path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
  {
    if let Ok(manifest) = read_json::<DetectionEvidenceManifest>(path) {
      let capture_file_name = detection_capture_file_name_from_manifest(&manifest)?;
      let mut detections_by_capture = BTreeMap::new();
      detections_by_capture.insert(capture_file_name, manifest.detection_set.clone());
      return Ok(LoadedDetectionInputs {
        model_id: manifest.model_run.model_id.0,
        label_map_source: class_label_source_name(&manifest.model_run.class_label_source),
        detections_by_capture,
      });
    }
  }

  let record = read_json::<DetectionFileRecord>(path)?;
  let model_id = record.detection_set.model_id.0.clone();
  let mut detections_by_capture = BTreeMap::new();
  detections_by_capture.insert(record.capture_file_name, record.detection_set);
  Ok(LoadedDetectionInputs {
    model_id,
    label_map_source: "inline_fixture".to_string(),
    detections_by_capture,
  })
}

fn load_detection_dir(path: &Path) -> OsuResult<LoadedDetectionInputs> {
  let mut detections_by_capture = BTreeMap::new();
  let mut model_id = None;
  let mut label_map_source = None;
  for entry in fs::read_dir(path)
    .map_err(|error| format!("failed to read detections dir {}: {error}", path.display()))?
  {
    let entry = entry.map_err(|error| format!("failed to read detections dir entry: {error}"))?;
    let entry_path = entry.path();
    if !entry_path.is_file() {
      continue;
    }
    let extension = entry_path.extension().and_then(|value| value.to_str());
    if !extension.is_some_and(|value| value.eq_ignore_ascii_case("json")) {
      continue;
    }
    let record = read_json::<DetectionFileRecord>(&entry_path)?;
    let record_model_id = record.detection_set.model_id.0.clone();
    if let Some(existing) = &model_id {
      if existing != &record_model_id {
        return Err(format!(
          "detection fixture directory mixes model ids {} and {}",
          existing, record_model_id
        ));
      }
    } else {
      model_id = Some(record_model_id);
    }
    detections_by_capture.insert(record.capture_file_name, record.detection_set);
    label_map_source.get_or_insert_with(|| "inline_fixture_dir".to_string());
  }

  if detections_by_capture.is_empty() {
    return Err(format!(
      "detections path {} contains no JSON detection fixtures",
      path.display()
    ));
  }

  Ok(LoadedDetectionInputs {
    model_id: model_id
      .ok_or_else(|| format!("detections path {} contains no model id", path.display()))?,
    label_map_source: label_map_source.unwrap_or_else(|| "inline_fixture_dir".to_string()),
    detections_by_capture,
  })
}

fn expand_frame_detections(
  manifest: &VisualTruthManifest,
  detections_by_capture: &BTreeMap<String, DetectionSet>,
) -> OsuResult<Vec<FrameDetections>> {
  manifest
    .frames
    .iter()
    .map(|frame| {
      let detection_set = detections_by_capture
        .get(&frame.capture.file_name)
        .ok_or_else(|| {
          format!(
            "missing detection fixture for capture file {}",
            frame.capture.file_name
          )
        })?
        .clone();
      Ok(FrameDetections::new(
        FrameKey::from_parts(
          frame.object_index,
          frame.capture.phase.clone(),
          frame.capture.file_name.clone(),
        ),
        detection_set,
      ))
    })
    .collect()
}

fn detection_capture_file_name_from_manifest(
  manifest: &DetectionEvidenceManifest,
) -> OsuResult<String> {
  match &manifest.source_image.source_image_ref {
    auv_inference_common::SourceImageRef::LocalPath { path } => path
      .file_name()
      .and_then(|name| name.to_str())
      .map(str::to_string)
      .ok_or_else(|| {
        format!(
          "detection evidence manifest source image path {} has invalid file name",
          path.display()
        )
      }),
    auv_inference_common::SourceImageRef::OpaqueId { id } => Ok(id.clone()),
  }
}

fn class_label_source_name(source: &ClassLabelSource) -> String {
  match source {
    ClassLabelSource::OverrideFile { .. } => "override_file",
    ClassLabelSource::EmbeddedModelMetadata => "embedded_model_metadata",
    ClassLabelSource::InlineList => "inline_list",
    ClassLabelSource::Unknown => "unknown",
  }
  .to_string()
}

fn build_map_summary(
  beatmap_path: &Path,
  beatmap: &Beatmap,
  schedule: &[ScheduledAction],
) -> MapSummary {
  let mut circle_count = 0usize;
  let mut slider_count = 0usize;
  let mut spinner_count = 0usize;
  let mut hold_count = 0usize;

  for action in schedule {
    match action.object_kind {
      ObjectKind::Circle => circle_count += 1,
      ObjectKind::Slider => slider_count += 1,
      ObjectKind::Spinner => spinner_count += 1,
      ObjectKind::Hold => hold_count += 1,
    }
  }

  MapSummary {
    beatmap_path: beatmap_path.display().to_string(),
    mode: beatmap.mode as u8,
    total_objects: schedule.len(),
    circle_count,
    slider_count,
    spinner_count,
    hold_count,
    first_object_time_ms: schedule.first().map(|action| action.scheduled_time_ms),
    last_object_time_ms: schedule.last().map(|action| action.scheduled_time_ms),
    approach_rate: beatmap.approach_rate,
    overall_difficulty: beatmap.overall_difficulty,
    circle_size: beatmap.circle_size,
    hp_drain_rate: beatmap.hp_drain_rate,
  }
}

fn run_dry_schedule(schedule: &[ScheduledAction], lead_in_ms: u64) -> Vec<DispatchSample> {
  let start = Instant::now() + Duration::from_millis(lead_in_ms);
  let mut trace = Vec::with_capacity(schedule.len());

  for action in schedule {
    wait_until_due(start, action.scheduled_time_ms);
    let actual_dispatch_time_ms = start.elapsed().as_millis() as u64;
    let dispatch_error_ms = actual_dispatch_time_ms as i64 - action.scheduled_time_ms as i64;
    trace.push(DispatchSample {
      object_index: action.object_index,
      object_kind: action.object_kind.clone(),
      scheduled_time_ms: action.scheduled_time_ms,
      actual_dispatch_time_ms,
      dispatch_error_ms,
      x: action.x,
      y: action.y,
      delivery_path: None,
      fallback_reason: None,
    });
  }

  trace
}

#[cfg(target_os = "macos")]
fn run_typed_dispatch(
  schedule: &[ScheduledAction],
  map_summary: &MapSummary,
  inputs: &BenchmarkInputs,
) -> OsuResult<(
  Vec<DispatchSample>,
  Vec<CaptureTraceSample>,
  Option<VerificationSummary>,
  Option<ProjectionArtifact>,
)> {
  let target_app = inputs
    .target_app
    .as_deref()
    .ok_or_else(|| "typed dispatch requires target_app".to_string())?;
  let dispatch_limit = inputs.dispatch_limit.unwrap_or(8).min(schedule.len());
  let driver = MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(
      WindowSelector::default()
        .owned_by(App::name(target_app.to_string()))
        .title_contains("osu"),
    )
    .map_err(|error| error.to_string())?;
  let window_projection = PlayfieldProjection::for_window(&window, map_summary.circle_size)?;
  let mut projection_artifact = Some(ProjectionArtifact::from_window_projection(
    &window,
    &window_projection,
    inputs
      .capture_verify
      .then_some("before_dispatch capture smoke".to_string()),
  ));
  let mut trace = Vec::with_capacity(dispatch_limit);
  let mut capture_trace = if inputs.capture_verify {
    Vec::with_capacity(dispatch_limit)
  } else {
    Vec::new()
  };

  if !inputs.capture_verify {
    warm_up_typed_dispatch_path(
      &session,
      &window,
      schedule.first().expect("non-empty schedule"),
      &window_projection,
    )?;
  }

  // NOTICE(p5-dispatch-latency): The measured dispatch clock starts only after
  // the window-targeted click path is warm, so first-object latency no longer
  // includes one-time driver/window setup cost.
  let start = Instant::now() + Duration::from_millis(inputs.lead_in_ms);

  for action in schedule.iter().take(dispatch_limit) {
    let mut captures = Vec::new();
    if inputs.capture_verify {
      let before_capture_time_ms = action
        .scheduled_time_ms
        .saturating_sub(inputs.pre_capture_offset_ms);
      wait_until_due(start, before_capture_time_ms);
      captures.push(capture_sample(
        &session,
        &window,
        &inputs.output_dir,
        action,
        CapturePhase::BeforeDispatch,
        &start,
        action.scheduled_time_ms,
        None,
        inputs.pre_capture_offset_ms,
      )?);
      if projection_artifact
        .as_ref()
        .is_some_and(|artifact| artifact.capture_bounds.is_none())
      {
        if let Some(capture) = captures.last() {
          let capture_projection = PlayfieldProjection::for_capture(
            f64::from(capture.width),
            f64::from(capture.height),
            map_summary.circle_size,
          )?;
          projection_artifact = projection_artifact.take().map(|artifact| {
            artifact.with_capture(
              window.frame,
              capture.width,
              capture.height,
              f64::from(capture.width) / window.frame.size.width,
              &capture_projection,
            )
          });
        }
      }
    }
    wait_until_due(start, action.scheduled_time_ms);
    let (window_x, window_y) = window_projection.to_window_point(action.x, action.y);
    let window_point = WindowPoint::new(window_x, window_y);
    let result = session
      .window()
      .click(
        &window,
        window_point,
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click: Click::Single,
          window_strategy: WindowClickStrategy::PidTargeted,
        },
      )
      .map_err(|error| {
        format!(
          "typed dispatch failed at object {}: {error}",
          action.object_index
        )
      })?;
    let actual_dispatch_time_ms = start.elapsed().as_millis() as u64;
    let dispatch_error_ms = actual_dispatch_time_ms as i64 - action.scheduled_time_ms as i64;
    trace.push(dispatch_sample_from_result(
      action,
      actual_dispatch_time_ms,
      dispatch_error_ms,
      result,
    ));

    if inputs.capture_verify {
      for offset_ms in &inputs.post_capture_offsets_ms {
        let post_capture_due =
          start + Duration::from_millis(actual_dispatch_time_ms.saturating_add(*offset_ms));
        wait_until_instant(post_capture_due);
        captures.push(capture_sample(
          &session,
          &window,
          &inputs.output_dir,
          action,
          CapturePhase::AfterDispatch,
          &start,
          action.scheduled_time_ms,
          Some(actual_dispatch_time_ms),
          *offset_ms,
        )?);
      }
      capture_trace.push(CaptureTraceSample {
        object_index: action.object_index,
        object_kind: action.object_kind.clone(),
        scheduled_time_ms: action.scheduled_time_ms,
        actual_dispatch_time_ms,
        dispatch_error_ms,
        captures,
      });
    }
  }

  let verification_summary = inputs
    .capture_verify
    .then(|| build_verification_summary(dispatch_limit, &capture_trace));

  Ok((
    trace,
    capture_trace,
    verification_summary,
    projection_artifact,
  ))
}

#[cfg(target_os = "macos")]
fn warm_up_typed_dispatch_path(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  action: &ScheduledAction,
  projection: &PlayfieldProjection,
) -> OsuResult<()> {
  let (window_x, window_y) = projection.to_window_point(action.x, action.y);
  let window_point = WindowPoint::new(window_x, window_y);
  session
    .window()
    .click(
      window,
      window_point,
      ClickOptions {
        policy: InputPolicy::ForegroundPreferred,
        click: Click::Single,
        window_strategy: WindowClickStrategy::PidTargeted,
      },
    )
    .map(|_| ())
    .map_err(|error| {
      format!(
        "typed dispatch warm-up failed at object {}: {error}",
        action.object_index
      )
    })
}

#[cfg(target_os = "macos")]
fn capture_sample(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  output_dir: &Path,
  action: &ScheduledAction,
  phase: CapturePhase,
  start: &Instant,
  scheduled_time_ms: u64,
  actual_dispatch_time_ms: Option<u64>,
  phase_offset_ms: u64,
) -> OsuResult<CaptureSample> {
  let capture = session.window().capture(window).map_err(|error| {
    format!(
      "window capture failed at object {}: {error}",
      action.object_index
    )
  })?;
  capture_to_sample(
    output_dir,
    action,
    phase,
    start,
    scheduled_time_ms,
    actual_dispatch_time_ms,
    phase_offset_ms,
    capture,
  )
}

#[cfg(target_os = "macos")]
fn capture_to_sample(
  output_dir: &Path,
  action: &ScheduledAction,
  phase: CapturePhase,
  start: &Instant,
  scheduled_time_ms: u64,
  actual_dispatch_time_ms: Option<u64>,
  phase_offset_ms: u64,
  capture: Capture,
) -> OsuResult<CaptureSample> {
  let capture_time_ms = start.elapsed().as_millis() as u64;
  let file_name = format!(
    "capture-object-{:04}-{}-{}ms.png",
    action.object_index,
    match phase {
      CapturePhase::BeforeDispatch => "before",
      CapturePhase::AfterDispatch => "after",
    },
    phase_offset_ms
  );
  let file_path = output_dir.join(&file_name);
  capture
    .image
    .save(&file_path)
    .map_err(|error| format!("failed to save {}: {error}", file_path.display()))?;
  Ok(CaptureSample {
    phase,
    capture_time_ms,
    relative_to_scheduled_ms: capture_time_ms as i64 - scheduled_time_ms as i64,
    relative_to_dispatch_ms: actual_dispatch_time_ms
      .map(|dispatch_time_ms| capture_time_ms as i64 - dispatch_time_ms as i64)
      .unwrap_or(capture_time_ms as i64 - scheduled_time_ms as i64),
    file_name,
    width: capture.image.width(),
    height: capture.image.height(),
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

#[cfg(not(target_os = "macos"))]
fn run_typed_dispatch(
  _schedule: &[ScheduledAction],
  _map_summary: &MapSummary,
  _inputs: &BenchmarkInputs,
) -> OsuResult<(
  Vec<DispatchSample>,
  Vec<CaptureTraceSample>,
  Option<VerificationSummary>,
  Option<ProjectionArtifact>,
)> {
  Err("typed dispatch is currently implemented only for macOS".to_string())
}

fn dispatch_sample_from_result(
  action: &ScheduledAction,
  actual_dispatch_time_ms: u64,
  dispatch_error_ms: i64,
  result: InputActionResult,
) -> DispatchSample {
  DispatchSample {
    object_index: action.object_index,
    object_kind: action.object_kind.clone(),
    scheduled_time_ms: action.scheduled_time_ms,
    actual_dispatch_time_ms,
    dispatch_error_ms,
    x: action.x,
    y: action.y,
    delivery_path: Some(format!("{:?}", result.selected_path)),
    fallback_reason: result.fallback_reason,
  }
}

fn wait_until_due(start: Instant, scheduled_time_ms: u64) {
  let due = start + Duration::from_millis(scheduled_time_ms);
  wait_until_instant(due);
}

fn wait_until_instant(due: Instant) {
  loop {
    let now = Instant::now();
    if now >= due {
      break;
    }
    let remaining = due.saturating_duration_since(now);
    if remaining > Duration::from_millis(2) {
      thread::sleep(Duration::from_millis(1));
    } else {
      std::hint::spin_loop();
    }
  }
}

fn build_latency_report(run_mode: RunMode, dispatch_trace: &[DispatchSample]) -> LatencyReport {
  let total_actions = dispatch_trace.len();
  let mut errors = dispatch_trace
    .iter()
    .map(|sample| sample.dispatch_error_ms)
    .collect::<Vec<_>>();
  errors.sort_unstable();

  let mean_error_ms = if total_actions == 0 {
    0.0
  } else {
    errors.iter().map(|error| *error as f64).sum::<f64>() / total_actions as f64
  };

  let p50_error_ms = percentile(&errors, 0.50);
  let p95_error_ms = percentile(&errors, 0.95);
  let p99_error_ms = percentile(&errors, 0.99);
  let max_error_ms = errors.iter().copied().max().unwrap_or(0);
  let jitter_ms = max_error_ms - errors.iter().copied().min().unwrap_or(0);
  let missed_schedule_count = dispatch_trace
    .iter()
    .filter(|sample| sample.dispatch_error_ms > 0)
    .count();

  LatencyReport {
    run_mode,
    total_actions,
    mean_error_ms,
    p50_error_ms,
    p95_error_ms,
    p99_error_ms,
    max_error_ms,
    jitter_ms,
    missed_schedule_count,
  }
}

fn build_verification_summary(
  expected_actions: usize,
  capture_trace: &[CaptureTraceSample],
) -> VerificationSummary {
  let captured_action_count = capture_trace
    .iter()
    .filter(|sample| !sample.captures.is_empty())
    .count();
  let missing_frame_count = expected_actions.saturating_sub(captured_action_count);
  let max_capture_delay_ms = capture_trace
    .iter()
    .flat_map(|sample| {
      sample
        .captures
        .iter()
        .filter(|capture| matches!(capture.phase, CapturePhase::AfterDispatch))
        .map(|capture| capture.relative_to_dispatch_ms)
    })
    .max()
    .unwrap_or(0);
  let suspicious_time_inversion_count = capture_trace
    .iter()
    .flat_map(|sample| sample.captures.iter())
    .filter(|capture| {
      matches!(capture.phase, CapturePhase::AfterDispatch) && capture.relative_to_dispatch_ms < 0
    })
    .count();

  VerificationSummary {
    capture_enabled: true,
    captured_action_count,
    missing_frame_count,
    max_capture_delay_ms,
    suspicious_time_inversion_count,
  }
}

fn build_evidence_summary(
  dispatch_trace: &[DispatchSample],
  capture_trace: &[CaptureTraceSample],
  latency_report: &LatencyReport,
  verification_summary: Option<&VerificationSummary>,
  visual_truth_manifest: Option<&VisualTruthManifest>,
  projection: Option<&ProjectionArtifact>,
  visual_eval_report: Option<&VisualEvalReport>,
) -> BenchmarkEvidenceSummary {
  let mut evidence_notes = Vec::new();

  if dispatch_trace.is_empty() {
    evidence_notes.push("no dispatch samples were recorded".to_string());
  }
  if capture_trace.is_empty() {
    evidence_notes.push("no capture artifacts were recorded".to_string());
  }
  if projection.is_none() {
    evidence_notes.push("projection artifact is missing".to_string());
  }
  if visual_truth_manifest.is_none() {
    evidence_notes.push("visual truth manifest is missing".to_string());
  }
  if latency_report.missed_schedule_count > 0 {
    evidence_notes.push(format!(
      "{} scheduled actions missed their target time",
      latency_report.missed_schedule_count
    ));
  }

  let (
    verification_captured_action_count,
    verification_missing_frame_count,
    verification_suspicious_time_inversion_count,
  ) = if let Some(summary) = verification_summary {
    if summary.captured_action_count == 0 {
      evidence_notes.push("verification captured zero actions".to_string());
    }
    if summary.missing_frame_count > 0 {
      evidence_notes.push(format!(
        "{} verification frames are missing",
        summary.missing_frame_count
      ));
    }
    if summary.suspicious_time_inversion_count > 0 {
      evidence_notes.push(format!(
        "{} verification captures inverted dispatch timing",
        summary.suspicious_time_inversion_count
      ));
    }
    (
      summary.captured_action_count,
      summary.missing_frame_count,
      summary.suspicious_time_inversion_count,
    )
  } else {
    evidence_notes.push("verification summary is missing".to_string());
    (0, 0, 0)
  };

  if let Some(report) = visual_eval_report {
    if report.total_frames == 0 {
      evidence_notes.push("visual eval report contains zero frames".to_string());
    }
  }

  BenchmarkEvidenceSummary {
    dispatch_sample_count: dispatch_trace.len(),
    capture_artifact_count: capture_trace.len(),
    has_projection_artifact: projection.is_some(),
    has_visual_truth_manifest: visual_truth_manifest.is_some(),
    has_visual_eval_report: visual_eval_report.is_some(),
    missed_schedule_count: latency_report.missed_schedule_count,
    verification_captured_action_count,
    verification_missing_frame_count,
    verification_suspicious_time_inversion_count,
    evidence_notes,
  }
}

fn percentile(sorted_errors: &[i64], percentile: f64) -> i64 {
  if sorted_errors.is_empty() {
    return 0;
  }

  let last_index = sorted_errors.len() - 1;
  let index = ((last_index as f64) * percentile).round() as usize;
  sorted_errors[index.min(last_index)]
}

fn scheduled_target(kind: &HitObjectKind) -> (ObjectKind, f32, f32) {
  match kind {
    HitObjectKind::Circle(circle) => (ObjectKind::Circle, circle.pos.x, circle.pos.y),
    HitObjectKind::Slider(slider) => (ObjectKind::Slider, slider.pos.x, slider.pos.y),
    HitObjectKind::Spinner(spinner) => (ObjectKind::Spinner, spinner.pos.x, spinner.pos.y),
    HitObjectKind::Hold(hold) => (ObjectKind::Hold, hold.pos_x, 192.0),
  }
}

fn write_json(path: PathBuf, value: &impl Serialize) -> OsuResult<()> {
  let mut rendered = serde_json::to_string_pretty(value)
    .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
  rendered.push('\n');
  fs::write(&path, rendered).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> OsuResult<T> {
  let bytes =
    fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes)
    .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn default_pre_capture_offset_ms() -> u64 {
  16
}

fn default_post_capture_offsets_ms() -> Vec<u64> {
  vec![16, 48]
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};
  use auv_inference_common::{BoundingBox, Detection, DetectionSet, ImageSize, ModelId};

  fn sample_visual_truth_manifest() -> VisualTruthManifest {
    VisualTruthManifest {
      schema_version: 1,
      beatmap_path: "map.osu".to_string(),
      map_summary: MapSummary {
        beatmap_path: "map.osu".to_string(),
        mode: 0,
        total_objects: 1,
        circle_count: 1,
        slider_count: 0,
        spinner_count: 0,
        hold_count: 0,
        first_object_time_ms: Some(100),
        last_object_time_ms: Some(100),
        approach_rate: 9.0,
        overall_difficulty: 8.0,
        circle_size: 4.0,
        hp_drain_rate: 5.0,
      },
      frames: vec![crate::VisualTruthFrame {
        object_index: 0,
        scheduled_time_ms: 100,
        actual_dispatch_time_ms: 104,
        dispatch_error_ms: 4,
        capture: crate::CaptureFrame {
          phase: CapturePhase::AfterDispatch,
          capture_time_ms: 120,
          relative_to_scheduled_ms: 20,
          relative_to_dispatch_ms: 16,
          file_name: "frame-0.png".to_string(),
          width: 640,
          height: 480,
          backend: "test".to_string(),
          fallback_reason: None,
        },
        expected_object: crate::ExpectedObjectTruth {
          object_kind: ObjectKind::Circle,
          expected_playfield_x: 100.0,
          expected_playfield_y: 100.0,
          circle_size: 4.0,
          approach_rate: 9.0,
          overall_difficulty: 8.0,
        },
      }],
    }
  }

  fn sample_projection_artifact() -> ProjectionArtifact {
    ProjectionArtifact {
      source_window_bounds: crate::ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 640.0,
        height: 480.0,
      },
      capture_bounds: None,
      capture_width: Some(640),
      capture_height: Some(480),
      capture_scale_factor: Some(1.0),
      scale_x: 1.0,
      scale_y: 1.0,
      offset_x: 0.0,
      offset_y: 0.0,
      match_radius_px: 20.0,
      derivation_method: crate::ProjectionDerivationMethod::LayoutRule,
      verification_reference: Some("frame-0.png".to_string()),
    }
  }

  fn sample_frame_detections() -> Vec<FrameDetections> {
    vec![FrameDetections::new(
      crate::FrameKey::from_parts(0, CapturePhase::AfterDispatch, "frame-0.png"),
      DetectionSet {
        model_id: ModelId("test-osu-detector".to_string()),
        image_size: ImageSize {
          width: 640,
          height: 480,
        },
        detections: vec![Detection {
          class_id: 0,
          label: "hit_circle".to_string(),
          confidence: 0.9,
          bbox: BoundingBox {
            x1: 90.0,
            y1: 90.0,
            x2: 110.0,
            y2: 110.0,
          },
        }],
      },
    )]
  }

  #[test]
  fn build_visual_eval_report_uses_projection_artifact() {
    let report = build_visual_eval_report(
      &sample_visual_truth_manifest(),
      &sample_projection_artifact(),
      &sample_frame_detections(),
    )
    .expect("visual eval report");

    assert_eq!(report.total_frames, 1);
    assert_eq!(report.label_matched_frames, 1);
    assert_eq!(report.spatial_matched_frames, 1);
    assert_eq!(report.spatial_missing_frames, 0);
    assert_eq!(report.spatial_unscored_frames, 0);
  }

  #[test]
  fn benchmark_writes_visual_eval_report_when_capture_verify_and_projection_exist() {
    let temp_dir = std::env::temp_dir().join(format!(
      "auv-osu-visual-eval-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let report = build_visual_eval_report(
      &sample_visual_truth_manifest(),
      &sample_projection_artifact(),
      &sample_frame_detections(),
    )
    .expect("visual eval report");

    let report_path = temp_dir.join("visual_eval_report.json");
    write_json(report_path.clone(), &report).expect("write visual eval report");

    assert!(report_path.exists());

    std::fs::remove_dir_all(&temp_dir).expect("remove temp dir");
  }

  #[test]
  fn benchmark_does_not_write_visual_eval_report_without_detector_input() {
    let temp_dir = std::env::temp_dir().join(format!(
      "auv-osu-no-detector-visual-eval-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let beatmap_path = std::env::temp_dir().join(format!(
      "sample-beatmap-{}.osu",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos()
    ));
    std::fs::write(
      &beatmap_path,
      r#"osu file format v14

[General]
AudioFilename: audio.mp3
Mode: 0

[Difficulty]
HPDrainRate:3
CircleSize:4
OverallDifficulty:5
ApproachRate:7

[TimingPoints]
0,500,4,2,1,100,1,0

[HitObjects]
256,192,1000,1,0,0:0:0:0:
"#,
    )
    .expect("write beatmap");

    let output_dir = temp_dir.join("output");
    let result = run_benchmark(&BenchmarkInputs::new(
      beatmap_path.clone(),
      output_dir.clone(),
    ))
    .expect("benchmark should succeed");

    assert!(result.visual_eval_report.is_none());
    assert!(!output_dir.join("visual_eval_report.json").exists());

    std::fs::remove_file(&beatmap_path).expect("remove beatmap");
    std::fs::remove_dir_all(&temp_dir).expect("remove temp dir");
  }

  #[test]
  fn benchmark_writes_smoke_evidence_summary_without_capture_verify() {
    let temp_dir = std::env::temp_dir().join(format!(
      "auv-osu-evidence-summary-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");

    let beatmap_path = temp_dir.join("fixture.osu");
    std::fs::write(&beatmap_path, TEST_BEATMAP).expect("write beatmap fixture");

    let output_dir = temp_dir.join("output");
    let result = run_benchmark(&BenchmarkInputs::new(
      beatmap_path.clone(),
      output_dir.clone(),
    ))
    .expect("benchmark should succeed");

    assert!(result.verification_summary.is_none());
    assert!(result.projection.is_none());
    assert!(!result.evidence_summary.evidence_notes.is_empty());
    assert!(output_dir.join("evidence_summary.json").exists());
    assert!(!output_dir.join("verification_summary.json").exists());
    assert!(!output_dir.join("projection.json").exists());

    std::fs::remove_dir_all(&temp_dir).expect("remove temp dir");
  }

  #[test]
  fn latency_report_aggregates_percentiles() {
    let dispatch_trace = vec![
      DispatchSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 10,
        actual_dispatch_time_ms: 11,
        dispatch_error_ms: 1,
        x: 1.0,
        y: 2.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 1,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 20,
        actual_dispatch_time_ms: 23,
        dispatch_error_ms: 3,
        x: 3.0,
        y: 4.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 2,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 30,
        actual_dispatch_time_ms: 32,
        dispatch_error_ms: 2,
        x: 5.0,
        y: 6.0,
        delivery_path: None,
        fallback_reason: None,
      },
    ];

    let report = build_latency_report(RunMode::DryRun, &dispatch_trace);
    assert_eq!(report.total_actions, 3);
    assert_eq!(report.p50_error_ms, 2);
    assert_eq!(report.p95_error_ms, 3);
    assert_eq!(report.p99_error_ms, 3);
    assert_eq!(report.max_error_ms, 3);
    assert_eq!(report.jitter_ms, 2);
    assert_eq!(report.missed_schedule_count, 3);
  }

  #[test]
  fn latency_report_counts_positive_errors_as_missed_schedule() {
    let dispatch_trace = vec![
      DispatchSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 10,
        actual_dispatch_time_ms: 9,
        dispatch_error_ms: -1,
        x: 1.0,
        y: 2.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 1,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 20,
        actual_dispatch_time_ms: 20,
        dispatch_error_ms: 0,
        x: 3.0,
        y: 4.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 2,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 30,
        actual_dispatch_time_ms: 33,
        dispatch_error_ms: 3,
        x: 5.0,
        y: 6.0,
        delivery_path: None,
        fallback_reason: None,
      },
    ];

    let report = build_latency_report(RunMode::DryRun, &dispatch_trace);
    assert_eq!(report.missed_schedule_count, 1);
    assert_eq!(report.jitter_ms, 4);
  }

  #[test]
  fn warm_up_path_preserves_single_click_defaults() {
    let inputs =
      BenchmarkInputs::typed_dispatch(PathBuf::from("map.osu"), PathBuf::from("out"), "osu!");

    assert_eq!(inputs.run_mode, RunMode::TypedDispatch);
    assert_eq!(inputs.dispatch_limit, Some(8));
    assert!(!inputs.capture_verify);
  }

  #[test]
  fn scheduled_target_maps_variants() {
    let circle = HitObjectKind::Circle(rosu_map::section::hit_objects::HitObjectCircle {
      pos: rosu_map::util::Pos::new(10.0, 20.0),
      new_combo: false,
      combo_offset: 0,
    });
    let (kind, x, y) = scheduled_target(&circle);
    assert_eq!(kind, ObjectKind::Circle);
    assert_eq!((x, y), (10.0, 20.0));
  }

  #[test]
  fn benchmark_inputs_default_to_dry_run() {
    let inputs = BenchmarkInputs::new(PathBuf::from("map.osu"), PathBuf::from("out"));
    assert_eq!(inputs.run_mode, RunMode::DryRun);
    assert_eq!(inputs.lead_in_ms, 25);
    assert_eq!(inputs.target_app, None);
    assert_eq!(inputs.dispatch_limit, None);
    assert!(!inputs.capture_verify);
    assert_eq!(inputs.pre_capture_offset_ms, 16);
    assert_eq!(inputs.post_capture_offsets_ms, vec![16, 48]);
  }

  #[test]
  fn typed_dispatch_inputs_set_defaults() {
    let inputs =
      BenchmarkInputs::typed_dispatch(PathBuf::from("map.osu"), PathBuf::from("out"), "osu!");
    assert_eq!(inputs.run_mode, RunMode::TypedDispatch);
    assert_eq!(inputs.target_app.as_deref(), Some("osu!"));
    assert_eq!(inputs.dispatch_limit, Some(8));
    assert!(!inputs.capture_verify);
    assert_eq!(inputs.pre_capture_offset_ms, 16);
    assert_eq!(inputs.post_capture_offsets_ms, vec![16, 48]);
  }

  #[test]
  fn dispatch_sample_captures_input_result_metadata() {
    let action = ScheduledAction {
      object_index: 4,
      object_kind: ObjectKind::Circle,
      scheduled_time_ms: 120,
      x: 256.0,
      y: 192.0,
    };
    let sample = dispatch_sample_from_result(
      &action,
      125,
      5,
      InputActionResult {
        selected_path: InputDeliveryPath::WindowTargetedMouse,
        attempts: vec![],
        fallback_reason: Some("fallback".to_string()),
        mouse_disturbance: auv_driver::DisturbanceLevel::Temporary,
        focus_disturbance: auv_driver::DisturbanceLevel::Foreground,
        clipboard_disturbance: auv_driver::DisturbanceLevel::None,
      },
    );
    assert_eq!(sample.delivery_path.as_deref(), Some("WindowTargetedMouse"));
    assert_eq!(sample.fallback_reason.as_deref(), Some("fallback"));
  }

  #[test]
  fn verification_summary_aggregates_capture_readiness() {
    let summary = build_verification_summary(
      2,
      &[
        CaptureTraceSample {
          object_index: 0,
          object_kind: ObjectKind::Circle,
          scheduled_time_ms: 10,
          actual_dispatch_time_ms: 11,
          dispatch_error_ms: 1,
          captures: vec![
            CaptureSample {
              phase: CapturePhase::BeforeDispatch,
              capture_time_ms: 9,
              relative_to_scheduled_ms: -1,
              relative_to_dispatch_ms: -2,
              file_name: "a.png".to_string(),
              width: 100,
              height: 100,
              backend: "test".to_string(),
              fallback_reason: None,
            },
            CaptureSample {
              phase: CapturePhase::AfterDispatch,
              capture_time_ms: 25,
              relative_to_scheduled_ms: 15,
              relative_to_dispatch_ms: 14,
              file_name: "b.png".to_string(),
              width: 100,
              height: 100,
              backend: "test".to_string(),
              fallback_reason: None,
            },
          ],
        },
        CaptureTraceSample {
          object_index: 1,
          object_kind: ObjectKind::Circle,
          scheduled_time_ms: 20,
          actual_dispatch_time_ms: 20,
          dispatch_error_ms: 0,
          captures: vec![],
        },
      ],
    );
    assert_eq!(summary.captured_action_count, 1);
    assert_eq!(summary.missing_frame_count, 1);
    assert_eq!(summary.max_capture_delay_ms, 14);
    assert_eq!(summary.suspicious_time_inversion_count, 0);
  }

  #[test]
  fn verification_summary_flags_after_dispatch_time_inversion() {
    let summary = build_verification_summary(
      1,
      &[CaptureTraceSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 30,
        actual_dispatch_time_ms: 35,
        dispatch_error_ms: 5,
        captures: vec![CaptureSample {
          phase: CapturePhase::AfterDispatch,
          capture_time_ms: 34,
          relative_to_scheduled_ms: 4,
          relative_to_dispatch_ms: -1,
          file_name: "after.png".to_string(),
          width: 100,
          height: 100,
          backend: "test".to_string(),
          fallback_reason: None,
        }],
      }],
    );

    assert_eq!(summary.captured_action_count, 1);
    assert_eq!(summary.missing_frame_count, 0);
    assert_eq!(summary.max_capture_delay_ms, -1);
    assert_eq!(summary.suspicious_time_inversion_count, 1);
  }

  #[test]
  fn benchmark_writes_visual_truth_manifest_when_capture_verify_is_enabled() {
    let temp_dir = std::env::temp_dir().join(format!(
      "auv-osu-visual-truth-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_nanos()
    ));
    let beatmap_path = temp_dir.join("fixture.osu");
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    std::fs::write(&beatmap_path, TEST_BEATMAP).expect("write beatmap fixture");

    let mut inputs = BenchmarkInputs::new(beatmap_path.clone(), temp_dir.clone());
    inputs.capture_verify = true;

    let output = run_benchmark(&inputs).expect("benchmark should succeed");

    assert!(temp_dir.join("visual_truth_manifest.json").exists());
    let manifest = output
      .visual_truth_manifest
      .expect("visual truth manifest should exist");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.frames.len(), 0);

    std::fs::remove_dir_all(&temp_dir).expect("remove temp dir");
  }

  const TEST_BEATMAP: &str = r"osu file format v14

[General]
AudioFilename: test.mp3
Mode: 0

[Difficulty]
HPDrainRate:5
CircleSize:4
OverallDifficulty:7
ApproachRate:8

[TimingPoints]
0,500,4,2,0,100,1,0

[HitObjects]
256,192,1000,1,0,0:0:0:0:
";
}
