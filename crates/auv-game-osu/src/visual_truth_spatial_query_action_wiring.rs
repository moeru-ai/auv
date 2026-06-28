use std::path::Path;

use auv_driver::geometry::WindowPoint;
use auv_file::{JsonFileReadError, read_json_file as read_json_file_helper};

use crate::benchmark::{CapturePhase, ObjectKind};
use crate::projection::PlayfieldProjection;
use crate::visual_truth::VisualTruthManifest;
use crate::visual_truth_spatial_query::{
  VisualTruthSpatialQueryManifest, VisualTruthSpatialQueryStatus,
};
use crate::visual_truth_spatial_query_action::{
  VisualTruthSpatialQueryActionEligibility, VisualTruthSpatialQueryActionReadiness,
  derive_visual_truth_spatial_query_action_readiness,
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
  pub click_summary: Option<String>,
  pub known_limits: Vec<String>,
}

pub trait VisualTruthQueryLiveClickExecutor {
  fn attempt_click(
    &self,
    window_point: WindowPoint,
    lineage: &VisualTruthQueryActionWiringLineage,
  ) -> Result<String, String>;
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
  wire_readiness_to_action(
    manifest,
    &readiness,
    lineage,
    live_projection,
    known_limits,
    executor,
  )
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
          refusal_reason: Some(
            "click_ready eligibility missing live window_point from playfield projection; defensive refusal"
              .to_string(),
          ),
          pixel_point,
          window_point: None,
          click_summary: None,
          known_limits,
        };
      };

      match executor.attempt_click(window_point, lineage) {
        Ok(summary) => VisualTruthQueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: None,
          pixel_point,
          window_point: Some(window_point),
          click_summary: Some(summary),
          known_limits,
        },
        Err(message) => VisualTruthQueryActionWiringOutcome {
          attempted: true,
          action_eligibility: readiness.eligibility,
          refusal_reason: Some(message),
          pixel_point,
          window_point: Some(window_point),
          click_summary: None,
          known_limits,
        },
      }
    }
    VisualTruthSpatialQueryActionEligibility::AnswerNonClickable
    | VisualTruthSpatialQueryActionEligibility::NotConsumable => VisualTruthQueryActionWiringOutcome {
      attempted: false,
      action_eligibility: readiness.eligibility,
      refusal_reason: readiness.refusal_reason.clone(),
      pixel_point,
      window_point: None,
      click_summary: None,
      known_limits,
    },
  }
}

fn resolve_live_window_point(
  manifest: &VisualTruthSpatialQueryManifest,
  live_projection: &PlayfieldProjection,
) -> Option<WindowPoint> {
  let visual_truth_manifest = read_json_file::<VisualTruthManifest>(
    Path::new(&manifest.source_visual_truth_manifest_path),
    "osu visual truth manifest",
  )
  .ok()?;
  let frame = find_target_frame(
    &visual_truth_manifest,
    manifest.object_index,
    &manifest.capture_phase,
    manifest.object_kind.as_ref(),
  )?;
  let (window_x, window_y) = live_projection.to_window_point(
    frame.expected_object.expected_playfield_x,
    frame.expected_object.expected_playfield_y,
  );
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

#[cfg(test)]
mod tests {
  use std::cell::Cell;
  use std::fs;
  use std::path::PathBuf;

  use super::*;
  use crate::benchmark::MapSummary;
  use crate::projection::ProjectionArtifact;
  use crate::visual_truth::{CaptureFrame, ExpectedObjectTruth, VisualTruthFrame};
  use crate::visual_truth_spatial_query::{
    VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryReason,
    VisualTruthSpatialQueryStatus,
  };

  struct CountingExecutor {
    calls: Cell<usize>,
    summary: Option<String>,
    error: Option<String>,
  }

  impl CountingExecutor {
    fn success(summary: impl Into<String>) -> Self {
      Self {
        calls: Cell::new(0),
        summary: Some(summary.into()),
        error: None,
      }
    }
  }

  impl VisualTruthQueryLiveClickExecutor for CountingExecutor {
    fn attempt_click(
      &self,
      _window_point: WindowPoint,
      _lineage: &VisualTruthQueryActionWiringLineage,
    ) -> Result<String, String> {
      self.calls.set(self.calls.get() + 1);
      if let Some(error) = &self.error {
        return Err(error.clone());
      }
      Ok(
        self
          .summary
          .clone()
          .unwrap_or_else(|| "clicked".to_string()),
      )
    }
  }

  fn write_probe_fixture(root: &Path) -> PathBuf {
    let manifest = VisualTruthManifest {
      schema_version: 1,
      beatmap_path: "tests/fixtures/probe.osu".to_string(),
      map_summary: MapSummary {
        beatmap_path: "tests/fixtures/probe.osu".to_string(),
        mode: 0,
        total_objects: 1,
        circle_count: 1,
        slider_count: 0,
        spinner_count: 0,
        hold_count: 0,
        first_object_time_ms: Some(1000),
        last_object_time_ms: Some(1000),
        approach_rate: 8.0,
        overall_difficulty: 7.0,
        circle_size: 4.0,
        hp_drain_rate: 5.0,
      },
      frames: vec![VisualTruthFrame {
        object_index: 0,
        scheduled_time_ms: 1000,
        actual_dispatch_time_ms: 1001,
        dispatch_error_ms: 1,
        capture: CaptureFrame {
          phase: CapturePhase::BeforeDispatch,
          capture_time_ms: 990,
          relative_to_scheduled_ms: -10,
          relative_to_dispatch_ms: -11,
          file_name: "capture-object-0000-before-16ms.png".to_string(),
          width: 800,
          height: 600,
          backend: "fixture".to_string(),
          fallback_reason: None,
        },
        expected_object: ExpectedObjectTruth {
          object_kind: ObjectKind::Circle,
          expected_playfield_x: 256.0,
          expected_playfield_y: 192.0,
          circle_size: 4.0,
          approach_rate: 8.0,
          overall_difficulty: 7.0,
        },
      }],
    };
    let manifest_path = root.join("visual_truth_manifest.json");
    fs::write(
      &manifest_path,
      serde_json::to_string_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");
    let projection = PlayfieldProjection::for_capture(800.0, 600.0, 4.0).expect("projection");
    let projection_artifact = ProjectionArtifact {
      source_window_bounds: crate::projection::ProjectionBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      },
      capture_bounds: None,
      capture_width: Some(800),
      capture_height: Some(600),
      capture_scale_factor: Some(1.0),
      scale_x: projection.scale_x,
      scale_y: projection.scale_y,
      offset_x: projection.offset_x,
      offset_y: projection.offset_y,
      match_radius_px: projection.match_radius_px,
      derivation_method: crate::projection::ProjectionDerivationMethod::LayoutRule,
      verification_reference: None,
    };
    fs::write(
      root.join("projection.json"),
      serde_json::to_string_pretty(&projection_artifact).expect("serialize projection"),
    )
    .expect("write projection");
    manifest_path
  }

  fn base_manifest(visual_truth_manifest_path: &Path) -> VisualTruthSpatialQueryManifest {
    VisualTruthSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 1,
      visual_truth_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_run_artifact_dir: "/tmp/run".to_string(),
      source_visual_truth_manifest_path: visual_truth_manifest_path.display().to_string(),
      source_projection_path: visual_truth_manifest_path
        .parent()
        .expect("parent")
        .join("projection.json")
        .display()
        .to_string(),
      object_index: 0,
      capture_phase: CapturePhase::BeforeDispatch,
      object_kind: None,
      query_backend: VisualTruthSpatialQueryBackend::PlayfieldProjectionReference,
      status: VisualTruthSpatialQueryStatus::Answered,
      reason: None,
      pixel_visibility: Some(VisualTruthPixelVisibility::InsideCapture),
      pixel_x: Some(400.0),
      pixel_y: Some(300.0),
      match_radius_px: Some(20.0),
      capture_width: Some(800),
      capture_height: Some(600),
      known_limits: vec!["upstream limit".to_string()],
    }
  }

  fn lineage_for(manifest: &VisualTruthSpatialQueryManifest) -> VisualTruthQueryActionWiringLineage {
    visual_truth_query_action_wiring_lineage_from_manifest(manifest, Path::new("/tmp/query.json"))
  }

  fn live_projection() -> PlayfieldProjection {
    PlayfieldProjection::for_capture(800.0, 600.0, 4.0).expect("projection")
  }

  #[test]
  fn click_ready_invokes_executor_once_with_live_window_point() {
    let temp = tempfile::tempdir().expect("tempdir");
    let visual_truth_path = write_probe_fixture(temp.path());
    let manifest = base_manifest(&visual_truth_path);
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("live click dispatched");
    let projection = live_projection();

    let outcome = wire_visual_truth_spatial_query_manifest_to_action(
      &manifest,
      &lineage,
      &projection,
      &executor,
    );

    assert_eq!(executor.calls.get(), 1);
    assert!(outcome.attempted);
    assert_eq!(
      outcome.action_eligibility,
      VisualTruthSpatialQueryActionEligibility::ClickReady
    );
    assert_eq!(outcome.pixel_point, Some((400.0, 300.0)));
    assert_eq!(outcome.window_point, Some(WindowPoint::new(400.0, 300.0)));
    assert_eq!(
      outcome.click_summary.as_deref(),
      Some("live click dispatched")
    );
    assert!(outcome.refusal_reason.is_none());
    assert!(
      outcome
        .known_limits
        .iter()
        .any(|limit| limit == OSU_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT)
    );
  }

  #[test]
  fn answer_non_clickable_refuses_without_executor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let visual_truth_path = write_probe_fixture(temp.path());
    let mut manifest = base_manifest(&visual_truth_path);
    manifest.pixel_visibility = Some(VisualTruthPixelVisibility::OutsideCapture);
    manifest.pixel_x = Some(900.0);
    manifest.pixel_y = Some(300.0);
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");
    let projection = live_projection();

    let outcome = wire_visual_truth_spatial_query_manifest_to_action(
      &manifest,
      &lineage,
      &projection,
      &executor,
    );

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(
      outcome.action_eligibility,
      VisualTruthSpatialQueryActionEligibility::AnswerNonClickable
    );
    assert_eq!(
      outcome.refusal_reason.as_deref(),
      Some("pixel_visibility=outside_capture")
    );
    assert!(outcome.click_summary.is_none());
  }

  #[test]
  fn not_consumable_refuses_without_executor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let visual_truth_path = write_probe_fixture(temp.path());
    let mut manifest = base_manifest(&visual_truth_path);
    manifest.status = VisualTruthSpatialQueryStatus::Failed;
    manifest.reason = Some(VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth);
    manifest.pixel_visibility = None;
    manifest.pixel_x = None;
    manifest.pixel_y = None;
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");
    let projection = live_projection();

    let outcome = wire_visual_truth_spatial_query_manifest_to_action(
      &manifest,
      &lineage,
      &projection,
      &executor,
    );

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(
      outcome.action_eligibility,
      VisualTruthSpatialQueryActionEligibility::NotConsumable
    );
    assert_eq!(
      outcome.refusal_reason.as_deref(),
      Some("status=failed reason=target_absent_from_visual_truth")
    );
    assert!(outcome.click_summary.is_none());
  }

  #[test]
  fn click_ready_missing_live_window_point_defensively_refuses() {
    let temp = tempfile::tempdir().expect("tempdir");
    let visual_truth_path = write_probe_fixture(temp.path());
    let mut manifest = base_manifest(&visual_truth_path);
    manifest.source_visual_truth_manifest_path = "/tmp/missing-visual-truth.json".to_string();
    let lineage = lineage_for(&manifest);
    let executor = CountingExecutor::success("should not run");
    let projection = live_projection();

    let outcome = wire_visual_truth_spatial_query_manifest_to_action(
      &manifest,
      &lineage,
      &projection,
      &executor,
    );

    assert_eq!(executor.calls.get(), 0);
    assert!(!outcome.attempted);
    assert_eq!(
      outcome.action_eligibility,
      VisualTruthSpatialQueryActionEligibility::ClickReady
    );
    assert_eq!(
      outcome.refusal_reason.as_deref(),
      Some("click_ready eligibility missing live window_point from playfield projection; defensive refusal")
    );
  }
}
