use std::path::Path;

use auv_driver::{InputActionResult, geometry::WindowPoint};
use auv_file::{JsonFileReadError, read_json_file as read_json_file_helper};

use crate::benchmark::{CapturePhase, ObjectKind};
use crate::projection::PlayfieldProjection;
use crate::visual_truth::VisualTruthManifest;
use crate::visual_truth_spatial_query::{VisualTruthSpatialQueryManifest, VisualTruthSpatialQueryStatus};
use crate::visual_truth_spatial_query_action::{
  VisualTruthSpatialQueryActionEligibility, VisualTruthSpatialQueryActionReadiness, derive_visual_truth_spatial_query_action_readiness,
};

pub const OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT: &str =
  "osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification";

#[derive(Clone, Debug, PartialEq)]
pub struct VisualTruthQueryActionWiringLineage {
  pub manifest_path: String,
  pub visual_truth_semantic_manifest_path: String,
  pub object_index: usize,
  pub capture_phase: CapturePhase,
  pub status: VisualTruthSpatialQueryStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualTruthQueryActionWiringOutcome {
  pub attempted: bool,
  pub action_eligibility: VisualTruthSpatialQueryActionEligibility,
  pub refusal_reason: Option<String>,
  pub pixel_point: Option<(f32, f32)>,
  pub window_point: Option<WindowPoint>,
  pub input_action: Option<InputActionResult>,
  pub known_limits: Vec<String>,
}

pub trait VisualTruthQueryLiveClickExecutor {
  fn attempt_click(&self, window_point: WindowPoint, lineage: &VisualTruthQueryActionWiringLineage) -> Result<InputActionResult, String>;
}

pub fn wire_visual_truth_spatial_query_manifest_to_action(
  manifest: &VisualTruthSpatialQueryManifest,
  lineage: &VisualTruthQueryActionWiringLineage,
  live_projection: &PlayfieldProjection,
  executor: &impl VisualTruthQueryLiveClickExecutor,
) -> VisualTruthQueryActionWiringOutcome {
  let readiness = derive_visual_truth_spatial_query_action_readiness(manifest);
  let mut known_limits = manifest.known_limits.clone();
  known_limits.push(OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT.to_string());
  wire_readiness_to_action(manifest, &readiness, lineage, live_projection, known_limits, executor)
}

pub fn visual_truth_query_action_wiring_lineage_from_manifest(
  manifest: &VisualTruthSpatialQueryManifest,
  manifest_path: &Path,
) -> VisualTruthQueryActionWiringLineage {
  VisualTruthQueryActionWiringLineage {
    manifest_path: manifest_path.display().to_string(),
    visual_truth_semantic_manifest_path: manifest.visual_truth_semantic_manifest_path.clone(),
    object_index: manifest.object_index,
    capture_phase: manifest.capture_phase.clone(),
    status: manifest.status,
  }
}

fn wire_readiness_to_action(
  manifest: &VisualTruthSpatialQueryManifest,
  readiness: &VisualTruthSpatialQueryActionReadiness,
  lineage: &VisualTruthQueryActionWiringLineage,
  live_projection: &PlayfieldProjection,
  known_limits: Vec<String>,
  executor: &impl VisualTruthQueryLiveClickExecutor,
) -> VisualTruthQueryActionWiringOutcome {
  let pixel_point = readiness.pixel_point;
  match readiness.eligibility {
    VisualTruthSpatialQueryActionEligibility::ClickReady => {
      let Some(window_point) = resolve_live_window_point(manifest, live_projection) else {
        return VisualTruthQueryActionWiringOutcome {
          attempted: false,
          action_eligibility: readiness.eligibility,
          refusal_reason: Some("click_ready eligibility missing live window_point from playfield projection; defensive refusal".to_string()),
          pixel_point,
          window_point: None,
          input_action: None,
          known_limits,
        };
      };

      match executor.attempt_click(window_point, lineage) {
        Ok(input_action) => VisualTruthQueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: None,
          pixel_point,
          window_point: Some(window_point),
          input_action: Some(input_action),
          known_limits,
        },
        Err(message) => VisualTruthQueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: Some(message),
          pixel_point,
          window_point: Some(window_point),
          input_action: None,
          known_limits,
        },
      }
    }
    VisualTruthSpatialQueryActionEligibility::AnswerNonClickable | VisualTruthSpatialQueryActionEligibility::NotConsumable => {
      VisualTruthQueryActionWiringOutcome {
        attempted: false,
        action_eligibility: readiness.eligibility,
        refusal_reason: readiness.refusal_reason.clone(),
        pixel_point,
        window_point: None,
        input_action: None,
        known_limits,
      }
    }
  }
}

fn resolve_live_window_point(manifest: &VisualTruthSpatialQueryManifest, live_projection: &PlayfieldProjection) -> Option<WindowPoint> {
  let visual_truth_manifest =
    read_json_file::<VisualTruthManifest>(Path::new(&manifest.source_visual_truth_manifest_path), "osu visual truth manifest").ok()?;
  let frame = find_target_frame(&visual_truth_manifest, manifest.object_index, &manifest.capture_phase, manifest.object_kind.as_ref())?;
  let (window_x, window_y) =
    live_projection.to_window_point(frame.expected_object.expected_playfield_x, frame.expected_object.expected_playfield_y);
  Some(WindowPoint::new(window_x, window_y))
}

fn find_target_frame<'a>(
  manifest: &'a VisualTruthManifest,
  object_index: usize,
  capture_phase: &CapturePhase,
  object_kind: Option<&ObjectKind>,
) -> Option<&'a crate::visual_truth::VisualTruthFrame> {
  manifest.frames.iter().find(|frame| {
    frame.object_index == object_index
      && frame.capture.phase == *capture_phase
      && object_kind.is_none_or(|kind| frame.expected_object.object_kind == *kind)
  })
}

fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T, String> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}
