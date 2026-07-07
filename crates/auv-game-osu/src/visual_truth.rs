use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{CaptureSample, CaptureTraceSample, DispatchSample, MapSummary, ScheduledAction};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualTruthManifest {
  pub schema_version: u32,
  pub beatmap_path: String,
  pub map_summary: MapSummary,
  pub frames: Vec<VisualTruthFrame>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VisualTruthFrame {
  pub object_index: usize,
  pub scheduled_time_ms: u64,
  pub actual_dispatch_time_ms: u64,
  pub dispatch_error_ms: i64,
  pub capture: CaptureFrame,
  pub expected_object: ExpectedObjectTruth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureFrame {
  pub phase: crate::CapturePhase,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExpectedObjectTruth {
  pub object_kind: crate::ObjectKind,
  pub expected_playfield_x: f32,
  pub expected_playfield_y: f32,
  pub circle_size: f32,
  pub approach_rate: f32,
  pub overall_difficulty: f32,
}

pub fn build_visual_truth_manifest(
  map_summary: &MapSummary,
  schedule: &[ScheduledAction],
  dispatch_trace: &[DispatchSample],
  capture_trace: &[CaptureTraceSample],
) -> Result<VisualTruthManifest, String> {
  let schedule_by_object = schedule.iter().map(|action| (action.object_index, action)).collect::<HashMap<_, _>>();
  let dispatch_by_object = dispatch_trace.iter().map(|sample| (sample.object_index, sample)).collect::<HashMap<_, _>>();
  let capture_by_object = capture_trace.iter().map(|sample| (sample.object_index, sample)).collect::<HashMap<_, _>>();
  let mut frames = Vec::new();

  for capture_sample in capture_trace {
    let scheduled_action = schedule_by_object
      .get(&capture_sample.object_index)
      .ok_or_else(|| format!("capture trace references missing scheduled action for object {}", capture_sample.object_index))?;
    let dispatch_sample = dispatch_by_object
      .get(&capture_sample.object_index)
      .ok_or_else(|| format!("capture trace references missing dispatch sample for object {}", capture_sample.object_index))?;

    if scheduled_action.object_kind != capture_sample.object_kind {
      return Err(format!("object {} kind mismatch between schedule and capture trace", capture_sample.object_index));
    }
    if dispatch_sample.object_kind != capture_sample.object_kind {
      return Err(format!("object {} kind mismatch between dispatch trace and capture trace", capture_sample.object_index));
    }
    if scheduled_action.scheduled_time_ms != capture_sample.scheduled_time_ms {
      return Err(format!("object {} scheduled time mismatch between schedule and capture trace", capture_sample.object_index));
    }
    if dispatch_sample.scheduled_time_ms != capture_sample.scheduled_time_ms {
      return Err(format!("object {} scheduled time mismatch between dispatch trace and capture trace", capture_sample.object_index));
    }
    if dispatch_sample.actual_dispatch_time_ms != capture_sample.actual_dispatch_time_ms {
      return Err(format!("object {} dispatch time mismatch between dispatch trace and capture trace", capture_sample.object_index));
    }
    if dispatch_sample.dispatch_error_ms != capture_sample.dispatch_error_ms {
      return Err(format!("object {} dispatch error mismatch between dispatch trace and capture trace", capture_sample.object_index));
    }

    for capture in &capture_sample.captures {
      frames.push(VisualTruthFrame {
        object_index: capture_sample.object_index,
        scheduled_time_ms: capture_sample.scheduled_time_ms,
        actual_dispatch_time_ms: capture_sample.actual_dispatch_time_ms,
        dispatch_error_ms: capture_sample.dispatch_error_ms,
        capture: capture_frame(capture),
        expected_object: ExpectedObjectTruth {
          object_kind: scheduled_action.object_kind.clone(),
          expected_playfield_x: scheduled_action.x,
          expected_playfield_y: scheduled_action.y,
          circle_size: map_summary.circle_size,
          approach_rate: map_summary.approach_rate,
          overall_difficulty: map_summary.overall_difficulty,
        },
      });
    }
  }

  for object_index in capture_by_object.keys() {
    if !schedule_by_object.contains_key(object_index) {
      return Err(format!("capture trace references missing scheduled action for object {}", object_index));
    }
    if !dispatch_by_object.contains_key(object_index) {
      return Err(format!("capture trace references missing dispatch sample for object {}", object_index));
    }
  }

  Ok(VisualTruthManifest {
    schema_version: 1,
    beatmap_path: map_summary.beatmap_path.clone(),
    map_summary: map_summary.clone(),
    frames,
  })
}

fn capture_frame(capture: &CaptureSample) -> CaptureFrame {
  CaptureFrame {
    phase: capture.phase.clone(),
    capture_time_ms: capture.capture_time_ms,
    relative_to_scheduled_ms: capture.relative_to_scheduled_ms,
    relative_to_dispatch_ms: capture.relative_to_dispatch_ms,
    file_name: capture.file_name.clone(),
    width: capture.width,
    height: capture.height,
    backend: capture.backend.clone(),
    fallback_reason: capture.fallback_reason.clone(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{CapturePhase, ObjectKind};

  fn test_map_summary() -> MapSummary {
    MapSummary {
      beatmap_path: "map.osu".to_string(),
      mode: 0,
      total_objects: 1,
      circle_count: 1,
      slider_count: 0,
      spinner_count: 0,
      hold_count: 0,
      first_object_time_ms: Some(100),
      last_object_time_ms: Some(100),
      approach_rate: 9.5,
      overall_difficulty: 8.0,
      circle_size: 4.0,
      hp_drain_rate: 6.0,
    }
  }

  fn test_schedule() -> Vec<ScheduledAction> {
    vec![ScheduledAction {
      object_index: 0,
      object_kind: ObjectKind::Circle,
      scheduled_time_ms: 100,
      x: 256.0,
      y: 192.0,
    }]
  }

  fn test_dispatch_trace() -> Vec<DispatchSample> {
    vec![DispatchSample {
      object_index: 0,
      object_kind: ObjectKind::Circle,
      scheduled_time_ms: 100,
      actual_dispatch_time_ms: 104,
      dispatch_error_ms: 4,
      x: 256.0,
      y: 192.0,
      delivery_path: Some("WindowTargetedMouse".to_string()),
      fallback_reason: None,
    }]
  }

  #[test]
  fn visual_truth_manifest_expands_multiple_captures_into_frames() {
    let manifest = build_visual_truth_manifest(
      &test_map_summary(),
      &test_schedule(),
      &test_dispatch_trace(),
      &[CaptureTraceSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 100,
        actual_dispatch_time_ms: 104,
        dispatch_error_ms: 4,
        captures: vec![
          CaptureSample {
            phase: CapturePhase::BeforeDispatch,
            capture_time_ms: 84,
            relative_to_scheduled_ms: -16,
            relative_to_dispatch_ms: -20,
            file_name: "before.png".to_string(),
            width: 640,
            height: 480,
            backend: "test".to_string(),
            fallback_reason: None,
          },
          CaptureSample {
            phase: CapturePhase::AfterDispatch,
            capture_time_ms: 120,
            relative_to_scheduled_ms: 20,
            relative_to_dispatch_ms: 16,
            file_name: "after.png".to_string(),
            width: 640,
            height: 480,
            backend: "test".to_string(),
            fallback_reason: Some("fallback".to_string()),
          },
        ],
      }],
    )
    .expect("manifest should build");

    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.frames.len(), 2);
    assert_eq!(manifest.frames[0].capture.phase, CapturePhase::BeforeDispatch);
    assert_eq!(manifest.frames[0].expected_object.expected_playfield_x, 256.0);
    assert_eq!(manifest.frames[0].expected_object.expected_playfield_y, 192.0);
    assert_eq!(manifest.frames[0].expected_object.circle_size, 4.0);
    assert_eq!(manifest.frames[0].expected_object.approach_rate, 9.5);
    assert_eq!(manifest.frames[0].expected_object.overall_difficulty, 8.0);
    assert_eq!(manifest.frames[1].capture.file_name, "after.png");
    assert_eq!(manifest.frames[1].capture.relative_to_dispatch_ms, 16);
    assert_eq!(manifest.frames[1].capture.fallback_reason.as_deref(), Some("fallback"));
  }

  #[test]
  fn visual_truth_manifest_rejects_dispatch_capture_mismatch() {
    let error = build_visual_truth_manifest(
      &test_map_summary(),
      &test_schedule(),
      &test_dispatch_trace(),
      &[CaptureTraceSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 100,
        actual_dispatch_time_ms: 105,
        dispatch_error_ms: 5,
        captures: vec![],
      }],
    )
    .expect_err("mismatch should fail");

    assert!(error.contains("dispatch time mismatch"));
  }
}
