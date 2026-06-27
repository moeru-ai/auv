use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use auv_inference_common::{
  BoundingBox, Detection, DetectionCoordinateSpace, DetectionSet, ImageSize, ModelId,
  render_annotated_image,
};
use image::ImageReader;
use serde::{Deserialize, Serialize};

use crate::projection::ProjectionArtifact;
use crate::visual_eval::{FrameKey, LabelMap};
use crate::{CapturePhase, VisualTruthManifest};

pub type DatasetResult<T> = Result<T, String>;

const DATASET_SCHEMA_VERSION: u32 = 1;
const DEFAULT_MODEL_ID: &str = "osu-auto-label-truth-v1";
const VISIBILITY_RULE: &str =
  "label frames captured before dispatch or within 128ms after dispatch; skip later frames";
const AFTER_DISPATCH_LABEL_WINDOW_MS: i64 = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetExportInputs {
  pub run_artifact_dir: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatasetExportOutput {
  pub dataset_manifest: DatasetManifest,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatasetManifest {
  pub schema_version: u32,
  pub source_run_artifact_dir: String,
  pub beatmap_path: String,
  pub label_map: Vec<DatasetLabelEntry>,
  pub visibility_rule: String,
  pub coordinate_space: DetectionCoordinateSpace,
  pub projection: ProjectionArtifact,
  pub checked_frames: Vec<String>,
  pub exported_frames: Vec<DatasetFrameRecord>,
  pub skipped_frames: Vec<DatasetSkippedFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetLabelEntry {
  pub class_id: usize,
  pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatasetFrameRecord {
  pub frame: FrameKey,
  pub source_capture_file: String,
  pub image_file: String,
  pub label_file: String,
  pub overlay_file: String,
  pub class_id: usize,
  pub label: String,
  pub bbox: BoundingBox,
  pub image_size: ImageSize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatasetSkippedFrame {
  pub frame: FrameKey,
  pub source_capture_file: String,
  pub reason: String,
}

#[derive(Clone, Debug)]
struct ExportFramePlan {
  frame: FrameKey,
  source_capture_path: PathBuf,
  source_capture_file: String,
  image_file: String,
  label_file: String,
  overlay_file: String,
  class_id: usize,
  label: String,
  bbox: BoundingBox,
  image_size: ImageSize,
}

pub fn export_dataset(inputs: &DatasetExportInputs) -> DatasetResult<DatasetExportOutput> {
  let manifest_path = inputs.run_artifact_dir.join("visual_truth_manifest.json");
  let projection_path = inputs.run_artifact_dir.join("projection.json");
  let manifest = read_json::<VisualTruthManifest>(&manifest_path)?;
  let projection = read_json::<ProjectionArtifact>(&projection_path)?;
  let label_map = LabelMap::default();
  let label_entries = build_label_entries(&label_map)?;
  let capture_paths = index_capture_paths(&inputs.run_artifact_dir, &manifest)?;
  let (plans, skipped_frames, checked_frames) = plan_dataset_export(
    &manifest,
    &projection,
    &label_entries,
    &capture_paths,
    &label_map,
  )?;

  if plans.is_empty() {
    return Err(format!(
      "dataset export produced no labeled frames from {} using visibility rule: {}",
      inputs.run_artifact_dir.display(),
      VISIBILITY_RULE
    ));
  }

  write_dataset_export(
    &inputs.output_dir,
    &plans,
    DatasetManifest {
      schema_version: DATASET_SCHEMA_VERSION,
      source_run_artifact_dir: inputs.run_artifact_dir.display().to_string(),
      beatmap_path: manifest.beatmap_path.clone(),
      label_map: label_entries,
      visibility_rule: VISIBILITY_RULE.to_string(),
      coordinate_space: DetectionCoordinateSpace::SourceImagePixels,
      projection,
      checked_frames,
      exported_frames: Vec::new(),
      skipped_frames,
    },
  )
}

fn plan_dataset_export(
  manifest: &VisualTruthManifest,
  projection: &ProjectionArtifact,
  label_entries: &[DatasetLabelEntry],
  capture_paths: &BTreeMap<String, PathBuf>,
  label_map: &LabelMap,
) -> DatasetResult<(Vec<ExportFramePlan>, Vec<DatasetSkippedFrame>, Vec<String>)> {
  let projection = projection.to_eval_projection()?;
  let mut plans = Vec::new();
  let mut skipped = Vec::new();
  let mut checked_frames = Vec::new();

  let auv_projection = match projection {
    crate::visual_eval::EvalProjection::Unavailable { reason } => {
      return Err(format!(
        "projection unavailable for dataset export: {reason}"
      ));
    }
    crate::visual_eval::EvalProjection::PlayfieldToPixels {
      scale_x,
      scale_y,
      offset_x,
      offset_y,
      match_radius_px,
    } => (scale_x, scale_y, offset_x, offset_y, match_radius_px),
  };

  for frame in &manifest.frames {
    let frame_key = FrameKey::from_parts(
      frame.object_index,
      frame.capture.phase.clone(),
      frame.capture.file_name.clone(),
    );
    checked_frames.push(format!(
      "{}:{}:{}",
      frame_key.object_index, frame_key.phase, frame_key.capture_file_name
    ));

    let capture_path = capture_paths
      .get(&frame.capture.file_name)
      .ok_or_else(|| format!("missing capture image {}", frame.capture.file_name))?
      .clone();

    if !frame_is_visible(&frame.capture.phase, frame.capture.relative_to_dispatch_ms) {
      skipped.push(DatasetSkippedFrame {
        frame: frame_key,
        source_capture_file: frame.capture.file_name.clone(),
        reason: format!(
          "frame excluded by visibility rule (phase={} relative_to_dispatch_ms={})",
          capture_phase_name(&frame.capture.phase),
          frame.capture.relative_to_dispatch_ms
        ),
      });
      continue;
    }

    let label = label_map
      .expected_label(&frame.expected_object.object_kind)
      .ok_or_else(|| {
        format!(
          "no dataset label mapping for object kind {:?}",
          frame.expected_object.object_kind
        )
      })?
      .to_string();
    let class_id = label_entries
      .iter()
      .find(|entry| entry.label == label)
      .map(|entry| entry.class_id)
      .ok_or_else(|| format!("missing class id for label {label}"))?;

    let bbox = derive_bbox(
      frame.expected_object.expected_playfield_x,
      frame.expected_object.expected_playfield_y,
      frame.capture.width,
      frame.capture.height,
      auv_projection,
    )?;

    let file_stem = capture_stem(&frame.capture.file_name)?;
    plans.push(ExportFramePlan {
      frame: frame_key,
      source_capture_path: capture_path,
      source_capture_file: frame.capture.file_name.clone(),
      image_file: format!("{file_stem}.png"),
      label_file: format!("{file_stem}.txt"),
      overlay_file: format!("{file_stem}.png"),
      class_id,
      label,
      bbox,
      image_size: ImageSize {
        width: frame.capture.width,
        height: frame.capture.height,
      },
    });
  }

  Ok((plans, skipped, checked_frames))
}

fn write_dataset_export(
  output_dir: &Path,
  plans: &[ExportFramePlan],
  mut manifest: DatasetManifest,
) -> DatasetResult<DatasetExportOutput> {
  let images_dir = output_dir.join("images");
  let labels_dir = output_dir.join("labels");
  let overlays_dir = output_dir.join("overlays");
  fs::create_dir_all(&images_dir).map_err(|error| {
    format!(
      "failed to create images dir {}: {error}",
      images_dir.display()
    )
  })?;
  fs::create_dir_all(&labels_dir).map_err(|error| {
    format!(
      "failed to create labels dir {}: {error}",
      labels_dir.display()
    )
  })?;
  fs::create_dir_all(&overlays_dir).map_err(|error| {
    format!(
      "failed to create overlays dir {}: {error}",
      overlays_dir.display()
    )
  })?;

  for plan in plans {
    let image_target = images_dir.join(&plan.image_file);
    fs::copy(&plan.source_capture_path, &image_target).map_err(|error| {
      format!(
        "failed to copy capture {} to {}: {error}",
        plan.source_capture_path.display(),
        image_target.display()
      )
    })?;

    let label_target = labels_dir.join(&plan.label_file);
    fs::write(&label_target, format_yolo_label(plan)).map_err(|error| {
      format!(
        "failed to write label file {}: {error}",
        label_target.display()
      )
    })?;

    let source_image = ImageReader::open(&plan.source_capture_path)
      .map_err(|error| {
        format!(
          "failed to open capture image {}: {error}",
          plan.source_capture_path.display()
        )
      })?
      .decode()
      .map_err(|error| {
        format!(
          "failed to decode capture image {}: {error}",
          plan.source_capture_path.display()
        )
      })?
      .to_rgb8();
    let detections = vec![Detection {
      class_id: plan.class_id,
      label: plan.label.clone(),
      confidence: 1.0,
      bbox: plan.bbox,
    }];
    let detection_set = DetectionSet {
      model_id: ModelId(DEFAULT_MODEL_ID.to_string()),
      image_size: plan.image_size,
      detections: detections.clone(),
    };
    let overlay = render_annotated_image(&source_image, &detection_set.detections);
    let overlay_target = overlays_dir.join(&plan.overlay_file);
    overlay.save(&overlay_target).map_err(|error| {
      format!(
        "failed to write overlay image {}: {error}",
        overlay_target.display()
      )
    })?;

    manifest.exported_frames.push(DatasetFrameRecord {
      frame: plan.frame.clone(),
      source_capture_file: plan.source_capture_file.clone(),
      image_file: format!("images/{}", plan.image_file),
      label_file: format!("labels/{}", plan.label_file),
      overlay_file: format!("overlays/{}", plan.overlay_file),
      class_id: plan.class_id,
      label: plan.label.clone(),
      bbox: plan.bbox,
      image_size: plan.image_size,
    });
  }

  let manifest_path = output_dir.join("dataset_manifest.json");
  write_json(&manifest_path, &manifest)?;

  Ok(DatasetExportOutput {
    dataset_manifest: manifest,
    output_dir: output_dir.to_path_buf(),
  })
}

fn frame_is_visible(phase: &CapturePhase, relative_to_dispatch_ms: i64) -> bool {
  match phase {
    CapturePhase::BeforeDispatch => true,
    CapturePhase::AfterDispatch => relative_to_dispatch_ms <= AFTER_DISPATCH_LABEL_WINDOW_MS,
  }
}

fn derive_bbox(
  playfield_x: f32,
  playfield_y: f32,
  image_width: u32,
  image_height: u32,
  projection: (f32, f32, f32, f32, f32),
) -> DatasetResult<BoundingBox> {
  let (scale_x, scale_y, offset_x, offset_y, match_radius_px) = projection;
  let center_x = playfield_x * scale_x + offset_x;
  let center_y = playfield_y * scale_y + offset_y;
  if !center_x.is_finite() || !center_y.is_finite() || !match_radius_px.is_finite() {
    return Err("dataset bbox derivation produced non-finite values".to_string());
  }
  if match_radius_px <= 0.0 {
    return Err(format!(
      "dataset bbox derivation requires positive radius, got {match_radius_px}"
    ));
  }

  let bbox = BoundingBox {
    x1: center_x - match_radius_px,
    y1: center_y - match_radius_px,
    x2: center_x + match_radius_px,
    y2: center_y + match_radius_px,
  };
  validate_bbox_in_bounds(bbox, image_width, image_height)?;
  Ok(bbox)
}

fn validate_bbox_in_bounds(
  bbox: BoundingBox,
  image_width: u32,
  image_height: u32,
) -> DatasetResult<()> {
  let values = [bbox.x1, bbox.y1, bbox.x2, bbox.y2];
  if values.iter().any(|value| !value.is_finite()) {
    return Err("dataset bbox contains non-finite coordinates".to_string());
  }
  if bbox.x1 < 0.0 || bbox.y1 < 0.0 || bbox.x2 > image_width as f32 || bbox.y2 > image_height as f32
  {
    return Err(format!(
      "dataset bbox {:?} exceeds image bounds {}x{}",
      bbox, image_width, image_height
    ));
  }
  if bbox.x1 >= bbox.x2 || bbox.y1 >= bbox.y2 {
    return Err(format!("dataset bbox {:?} is degenerate", bbox));
  }
  Ok(())
}

fn index_capture_paths(
  run_artifact_dir: &Path,
  manifest: &VisualTruthManifest,
) -> DatasetResult<BTreeMap<String, PathBuf>> {
  let mut capture_paths = BTreeMap::new();
  if manifest.frames.is_empty() {
    return Err(format!(
      "visual truth manifest {} contains no frames; capture verification evidence is required",
      run_artifact_dir
        .join("visual_truth_manifest.json")
        .display()
    ));
  }

  for frame in &manifest.frames {
    let capture_path = run_artifact_dir.join(&frame.capture.file_name);
    if !capture_path.exists() {
      return Err(format!(
        "missing capture image {} referenced by visual truth manifest",
        capture_path.display()
      ));
    }
    capture_paths.insert(frame.capture.file_name.clone(), capture_path);
  }

  Ok(capture_paths)
}

fn build_label_entries(label_map: &LabelMap) -> DatasetResult<Vec<DatasetLabelEntry>> {
  let ordered_kinds = [
    crate::ObjectKind::Circle,
    crate::ObjectKind::Slider,
    crate::ObjectKind::Spinner,
    crate::ObjectKind::Hold,
  ];
  ordered_kinds
    .iter()
    .enumerate()
    .map(|(class_id, kind)| {
      let label = label_map
        .expected_label(kind)
        .ok_or_else(|| format!("missing default label mapping for object kind {:?}", kind))?;
      Ok(DatasetLabelEntry {
        class_id,
        label: label.to_string(),
      })
    })
    .collect()
}

fn format_yolo_label(plan: &ExportFramePlan) -> String {
  let width = plan.image_size.width as f32;
  let height = plan.image_size.height as f32;
  let center_x = ((plan.bbox.x1 + plan.bbox.x2) / 2.0) / width;
  let center_y = ((plan.bbox.y1 + plan.bbox.y2) / 2.0) / height;
  let bbox_width = (plan.bbox.x2 - plan.bbox.x1) / width;
  let bbox_height = (plan.bbox.y2 - plan.bbox.y1) / height;
  format!(
    "{} {:.6} {:.6} {:.6} {:.6}\n",
    plan.class_id, center_x, center_y, bbox_width, bbox_height
  )
}

fn capture_phase_name(phase: &CapturePhase) -> &'static str {
  match phase {
    CapturePhase::BeforeDispatch => "before_dispatch",
    CapturePhase::AfterDispatch => "after_dispatch",
  }
}

fn capture_stem(file_name: &str) -> DatasetResult<String> {
  Path::new(file_name)
    .file_stem()
    .and_then(|stem| stem.to_str())
    .map(str::to_string)
    .ok_or_else(|| format!("capture file name {file_name:?} is missing a valid stem"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> DatasetResult<T> {
  let bytes =
    fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes)
    .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> DatasetResult<()> {
  let payload = serde_json::to_vec_pretty(value)
    .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
  fs::write(path, payload).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{CaptureFrame, ExpectedObjectTruth, MapSummary, ObjectKind, VisualTruthFrame};

  fn sample_manifest() -> VisualTruthManifest {
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
        first_object_time_ms: Some(24),
        last_object_time_ms: Some(24),
        approach_rate: 8.0,
        overall_difficulty: 8.0,
        circle_size: 5.0,
        hp_drain_rate: 3.0,
      },
      frames: vec![VisualTruthFrame {
        object_index: 0,
        scheduled_time_ms: 24,
        actual_dispatch_time_ms: 1178,
        dispatch_error_ms: 1154,
        capture: CaptureFrame {
          phase: CapturePhase::BeforeDispatch,
          capture_time_ms: 497,
          relative_to_scheduled_ms: 473,
          relative_to_dispatch_ms: -16,
          file_name: "capture-object-0000-before-16ms.png".to_string(),
          width: 1512,
          height: 949,
          backend: "test".to_string(),
          fallback_reason: None,
        },
        expected_object: ExpectedObjectTruth {
          object_kind: ObjectKind::Circle,
          expected_playfield_x: 98.0,
          expected_playfield_y: 69.0,
          circle_size: 5.0,
          approach_rate: 8.0,
          overall_difficulty: 8.0,
        },
      }],
    }
  }

  fn sample_projection() -> ProjectionArtifact {
    ProjectionArtifact {
      source_window_bounds: crate::ProjectionBounds {
        x: 0.0,
        y: 33.0,
        width: 1512.0,
        height: 949.0,
      },
      capture_bounds: None,
      capture_width: Some(1512),
      capture_height: Some(949),
      capture_scale_factor: Some(1.0),
      scale_x: 2.4713541666666665,
      scale_y: 2.4713541666666665,
      offset_x: 123.33333333333337,
      offset_y: 0.0,
      match_radius_px: 79.083336,
      derivation_method: crate::ProjectionDerivationMethod::LayoutRule,
      verification_reference: Some("before_dispatch capture smoke".to_string()),
    }
  }

  #[test]
  fn derive_bbox_projects_point_and_radius() {
    let bbox = derive_bbox(
      98.0,
      69.0,
      1512,
      949,
      (2.4713542, 2.4713542, 123.333336, 0.0, 79.083336),
    )
    .expect("bbox");

    assert!((bbox.x1 - 286.4377).abs() < 0.1);
    assert!((bbox.y1 - 91.4311).abs() < 0.1);
    assert!((bbox.x2 - 444.6044).abs() < 0.1);
    assert!((bbox.y2 - 249.5978).abs() < 0.1);
  }

  #[test]
  fn derive_bbox_rejects_out_of_bounds_boxes() {
    let error =
      derive_bbox(0.0, 0.0, 100, 100, (1.0, 1.0, 0.0, 0.0, 60.0)).expect_err("bbox should fail");
    assert!(error.contains("exceeds image bounds"));
  }

  #[test]
  fn visibility_rule_skips_late_after_dispatch_frames() {
    assert!(frame_is_visible(&CapturePhase::BeforeDispatch, 999));
    assert!(frame_is_visible(&CapturePhase::AfterDispatch, 128));
    assert!(!frame_is_visible(&CapturePhase::AfterDispatch, 129));
  }

  #[test]
  fn index_capture_paths_fails_when_capture_file_is_missing() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let error = index_capture_paths(temp_dir.path(), &sample_manifest()).expect_err("missing file");
    assert!(error.contains("missing capture image"));
  }

  #[test]
  fn read_json_reports_read_error_for_directory_path() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let error = read_json::<serde_json::Value>(temp_dir.path()).expect_err("directory should fail");

    assert!(error.contains("failed to read"));
  }

  #[test]
  fn build_label_entries_uses_default_label_order() {
    let entries = build_label_entries(&LabelMap::default()).expect("entries");
    assert_eq!(entries[0].label, "hit_circle");
    assert_eq!(entries[1].label, "slider");
    assert_eq!(entries[2].label, "spinner");
    assert_eq!(entries[3].label, "hold");
  }

  #[test]
  fn format_yolo_label_normalizes_bbox() {
    let plan = ExportFramePlan {
      frame: FrameKey::from_parts(0, CapturePhase::BeforeDispatch, "capture.png"),
      source_capture_path: PathBuf::from("capture.png"),
      source_capture_file: "capture.png".to_string(),
      image_file: "capture.png".to_string(),
      label_file: "capture.txt".to_string(),
      overlay_file: "capture.png".to_string(),
      class_id: 0,
      label: "hit_circle".to_string(),
      bbox: BoundingBox {
        x1: 10.0,
        y1: 20.0,
        x2: 30.0,
        y2: 60.0,
      },
      image_size: ImageSize {
        width: 100,
        height: 200,
      },
    };

    assert_eq!(
      format_yolo_label(&plan),
      "0 0.200000 0.200000 0.200000 0.200000\n"
    );
  }

  #[test]
  fn sample_projection_is_valid_for_dataset_export() {
    sample_projection()
      .to_eval_projection()
      .expect("projection should adapt to eval projection");
  }
}
