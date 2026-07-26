use auv_task_object_detection::{BoundingBox, DetectionResult};
use serde::{Deserialize, Serialize};

use crate::{CapturePhase, ObjectKind, VisualTruthManifest};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FrameKey {
  pub object_index: usize,
  pub phase: String,
  pub capture_file_name: String,
}

impl FrameKey {
  pub fn from_parts(object_index: usize, phase: CapturePhase, capture_file_name: impl Into<String>) -> Self {
    Self {
      object_index,
      phase: capture_phase_key(&phase).to_string(),
      capture_file_name: capture_file_name.into(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameDetections {
  pub frame: FrameKey,
  pub detections: DetectionResult,
}

impl FrameDetections {
  pub fn new(frame: FrameKey, detections: DetectionResult) -> Self {
    Self { frame, detections }
  }
}

/// Maps an osu [`ObjectKind`] to the detection label a visual model is expected
/// to emit for that object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelMap {
  entries: Vec<(ObjectKind, String)>,
}

impl LabelMap {
  pub fn new(entries: Vec<(ObjectKind, String)>) -> Self {
    Self { entries }
  }

  pub fn expected_label(&self, kind: &ObjectKind) -> Option<&str> {
    self.entries.iter().find(|(entry_kind, _)| entry_kind == kind).map(|(_, label)| label.as_str())
  }
}

impl Default for LabelMap {
  fn default() -> Self {
    Self {
      entries: vec![
        (ObjectKind::Circle, "hit_circle".to_string()),
        (ObjectKind::Slider, "slider".to_string()),
        (ObjectKind::Spinner, "spinner".to_string()),
        (ObjectKind::Hold, "hold".to_string()),
      ],
    }
  }
}

/// Whether playfield-space truth can be projected into the capture pixel space
/// the detections live in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvalProjection {
  Unavailable {
    reason: String,
  },
  PlayfieldToPixels {
    scale_x: f32,
    scale_y: f32,
    offset_x: f32,
    offset_y: f32,
    match_radius_px: f32,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameLabelOutcome {
  Matched,
  Missing,
  Unmapped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameSpatialOutcome {
  Matched,
  Missing,
  NotScored,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameEvaluation {
  pub frame: FrameKey,
  pub object_kind: ObjectKind,
  pub expected_label: Option<String>,
  pub label_outcome: FrameLabelOutcome,
  pub spatial_outcome: FrameSpatialOutcome,
  pub spurious_detection_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectorEvalProvenance {
  pub model_id: String,
  pub label_map_source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualEvalReport {
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub label_missing_frames: usize,
  pub label_unmapped_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_missing_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection: EvalProjection,
  pub frames: Vec<FrameEvaluation>,
  pub known_limits: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub detector_provenance: Option<DetectorEvalProvenance>,
}

impl VisualEvalReport {
  pub fn label_recall(&self) -> Option<f32> {
    let scorable = self.label_matched_frames + self.label_missing_frames;
    if scorable == 0 {
      None
    } else {
      Some(self.label_matched_frames as f32 / scorable as f32)
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayfieldPixelPoint {
  pub x: f32,
  pub y: f32,
  pub match_radius_px: f32,
}

pub fn pixel_point_inside_capture(point: &PlayfieldPixelPoint, capture_width: u32, capture_height: u32) -> bool {
  point.x.is_finite()
    && point.y.is_finite()
    && point.x >= 0.0
    && point.y >= 0.0
    && point.x <= capture_width as f32
    && point.y <= capture_height as f32
}

pub fn project_playfield_point(playfield_x: f32, playfield_y: f32, projection: &EvalProjection) -> Option<PlayfieldPixelPoint> {
  match projection {
    EvalProjection::Unavailable { .. } => None,
    EvalProjection::PlayfieldToPixels {
      scale_x,
      scale_y,
      offset_x,
      offset_y,
      match_radius_px,
    } => Some(PlayfieldPixelPoint {
      x: playfield_x * scale_x + offset_x,
      y: playfield_y * scale_y + offset_y,
      match_radius_px: *match_radius_px,
    }),
  }
}

pub fn evaluate_visual_truth(
  manifest: &VisualTruthManifest,
  detections_by_frame: &[FrameDetections],
  projection: &EvalProjection,
  label_map: &LabelMap,
) -> VisualEvalReport {
  evaluate_visual_truth_with_provenance(manifest, detections_by_frame, projection, label_map, None)
}

pub fn evaluate_visual_truth_with_provenance(
  manifest: &VisualTruthManifest,
  detections_by_frame: &[FrameDetections],
  projection: &EvalProjection,
  label_map: &LabelMap,
  detector_provenance: Option<DetectorEvalProvenance>,
) -> VisualEvalReport {
  let detections_lookup =
    detections_by_frame.iter().map(|entry| (entry.frame.clone(), &entry.detections)).collect::<std::collections::BTreeMap<_, _>>();

  let empty = Vec::new();
  let mut frames = Vec::with_capacity(manifest.frames.len());
  let mut label_matched_frames = 0;
  let mut label_missing_frames = 0;
  let mut label_unmapped_frames = 0;
  let mut spatial_matched_frames = 0;
  let mut spatial_missing_frames = 0;
  let mut spatial_unscored_frames = 0;
  let mut spurious_detection_count = 0;

  for frame in &manifest.frames {
    let frame_key = FrameKey::from_parts(frame.object_index, frame.capture.phase.clone(), frame.capture.file_name.clone());
    let detections = detections_lookup.get(&frame_key).map(|set| &set.detections).unwrap_or(&empty);
    let expected_label = label_map.expected_label(&frame.expected_object.object_kind).map(str::to_string);

    let matched_detection_index =
      expected_label.as_ref().and_then(|label| detections.iter().position(|detection| detection.label == *label));

    let label_outcome = match (&expected_label, matched_detection_index) {
      (None, _) => FrameLabelOutcome::Unmapped,
      (Some(_), Some(_)) => FrameLabelOutcome::Matched,
      (Some(_), None) => FrameLabelOutcome::Missing,
    };

    let frame_spurious = match matched_detection_index {
      Some(_) => detections.len().saturating_sub(1),
      None => detections.len(),
    };

    let spatial_outcome = match (projection, &expected_label, matched_detection_index) {
      (EvalProjection::Unavailable { .. }, _, _) | (_, None, _) => FrameSpatialOutcome::NotScored,
      (
        EvalProjection::PlayfieldToPixels {
          scale_x,
          scale_y,
          offset_x,
          offset_y,
          match_radius_px,
        },
        Some(_),
        Some(index),
      ) => {
        let target_x = frame.expected_object.expected_playfield_x * scale_x + offset_x;
        let target_y = frame.expected_object.expected_playfield_y * scale_y + offset_y;
        let detection = &detections[index];
        if center_distance(&detection.bbox, target_x, target_y) <= *match_radius_px {
          FrameSpatialOutcome::Matched
        } else {
          FrameSpatialOutcome::Missing
        }
      }
      (EvalProjection::PlayfieldToPixels { .. }, Some(_), None) => FrameSpatialOutcome::Missing,
    };

    match label_outcome {
      FrameLabelOutcome::Matched => label_matched_frames += 1,
      FrameLabelOutcome::Missing => label_missing_frames += 1,
      FrameLabelOutcome::Unmapped => label_unmapped_frames += 1,
    }
    match spatial_outcome {
      FrameSpatialOutcome::Matched => spatial_matched_frames += 1,
      FrameSpatialOutcome::Missing => spatial_missing_frames += 1,
      FrameSpatialOutcome::NotScored => spatial_unscored_frames += 1,
    }
    spurious_detection_count += frame_spurious;

    frames.push(FrameEvaluation {
      frame: frame_key,
      object_kind: frame.expected_object.object_kind.clone(),
      expected_label,
      label_outcome,
      spatial_outcome,
      spurious_detection_count: frame_spurious,
    });
  }

  VisualEvalReport {
    total_frames: manifest.frames.len(),
    label_matched_frames,
    label_missing_frames,
    label_unmapped_frames,
    spatial_matched_frames,
    spatial_missing_frames,
    spatial_unscored_frames,
    spurious_detection_count,
    projection: projection.clone(),
    frames,
    known_limits: build_known_limits(projection),
    detector_provenance,
  }
}

fn capture_phase_key(phase: &CapturePhase) -> &'static str {
  match phase {
    CapturePhase::BeforeDispatch => "before_dispatch",
    CapturePhase::AfterDispatch => "after_dispatch",
  }
}

fn build_known_limits(projection: &EvalProjection) -> Vec<String> {
  let mut known_limits =
    vec!["label-presence scoring confirms a detection label exists in a frame, not that it is the correct object instance".to_string()];
  match projection {
    EvalProjection::Unavailable { reason } => {
      known_limits.push(format!("spatial scoring skipped: no playfield-to-pixel calibration ({reason})"));
    }
    EvalProjection::PlayfieldToPixels { .. } => {
      known_limits.push("spatial scoring uses a linear playfield-to-pixel projection; accuracy depends on calibration quality".to_string());
    }
  }
  known_limits
}

fn center_distance(bbox: &BoundingBox, target_x: f32, target_y: f32) -> f32 {
  let center_x = (bbox.x1 + bbox.x2) / 2.0;
  let center_y = (bbox.y1 + bbox.y2) / 2.0;
  let dx = center_x - target_x;
  let dy = center_y - target_y;
  (dx * dx + dy * dy).sqrt()
}

pub fn iou(a: &BoundingBox, b: &BoundingBox) -> f32 {
  let inter_x1 = a.x1.max(b.x1);
  let inter_y1 = a.y1.max(b.y1);
  let inter_x2 = a.x2.min(b.x2);
  let inter_y2 = a.y2.min(b.y2);
  let inter_w = (inter_x2 - inter_x1).max(0.0);
  let inter_h = (inter_y2 - inter_y1).max(0.0);
  let intersection = inter_w * inter_h;
  if intersection <= 0.0 {
    return 0.0;
  }
  let union = a.area() + b.area() - intersection;
  if union <= 0.0 {
    0.0
  } else {
    intersection / union
  }
}
