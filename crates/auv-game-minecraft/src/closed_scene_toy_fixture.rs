use std::fs;
use std::path::Path;

use auv_driver::geometry::Point;
use serde::{Deserialize, Serialize};

use crate::training_result_spatial_query::{
  TrainingResultSpatialQueryAnswer, TrainingResultSpatialQueryReason,
  TrainingResultSpatialQueryRequest, TrainingResultSpatialQueryStatus,
};
use crate::types::{BlockFace, BlockPosition, MinecraftTargetSemantics, ProjectionVisibility};

pub const CLOSED_SCENE_TOY_FIXTURE_SCHEMA_VERSION: u32 = 1;

const MC18_V1_TOY_ANSWER_MESSAGE: &str = "MC-18 closed_scene_toy provider answered via closed-label fixture lookup; not Gaussian inference";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClosedSceneToyFixture {
  pub schema_version: u32,
  pub fixture_id: String,
  pub generated_at: String,
  pub labels: Vec<ClosedSceneToyLabel>,
  pub frames: Vec<ClosedSceneToyFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedSceneToyLabel {
  pub id: String,
  pub block: BlockPosition,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub face: Option<BlockFace>,
  pub semantics: MinecraftTargetSemantics,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClosedSceneToyFrame {
  pub basis_frame_id: String,
  pub answers: Vec<ClosedSceneToyFrameAnswer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClosedSceneToyFrameAnswer {
  pub label_id: String,
  pub visibility: ProjectionVisibility,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub screen_point: Option<Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClosedSceneToyFixtureLoadError {
  Io(String),
  Parse(String),
  UnsupportedSchemaVersion { found: u32 },
  EmptyFixture,
}

impl ClosedSceneToyFixtureLoadError {
  pub fn message(&self) -> String {
    match self {
      Self::Io(error) => format!("failed to read closed-scene fixture: {error}"),
      Self::Parse(error) => format!("failed to parse closed-scene fixture JSON: {error}"),
      Self::UnsupportedSchemaVersion { found } => format!(
        "unsupported closed-scene fixture schema_version {found}; expected {CLOSED_SCENE_TOY_FIXTURE_SCHEMA_VERSION}"
      ),
      Self::EmptyFixture => "closed-scene fixture has no frames".to_string(),
    }
  }
}

pub fn load_closed_scene_fixture(
  path: &Path,
) -> Result<ClosedSceneToyFixture, ClosedSceneToyFixtureLoadError> {
  let bytes =
    fs::read(path).map_err(|error| ClosedSceneToyFixtureLoadError::Io(error.to_string()))?;
  let fixture: ClosedSceneToyFixture = serde_json::from_slice(&bytes)
    .map_err(|error| ClosedSceneToyFixtureLoadError::Parse(error.to_string()))?;
  if fixture.schema_version != CLOSED_SCENE_TOY_FIXTURE_SCHEMA_VERSION {
    return Err(ClosedSceneToyFixtureLoadError::UnsupportedSchemaVersion {
      found: fixture.schema_version,
    });
  }
  if fixture.frames.is_empty() {
    return Err(ClosedSceneToyFixtureLoadError::EmptyFixture);
  }
  Ok(fixture)
}

pub fn resolve_closed_label_answer(
  fixture: &ClosedSceneToyFixture,
  request: &TrainingResultSpatialQueryRequest,
) -> TrainingResultSpatialQueryAnswer {
  let Some(label) = find_matching_label(fixture, request) else {
    return TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Blocked,
      reason: Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket),
      message: Some(
        "MC-18 closed_scene_toy provider blocked: target not in closed label set (fixture lookup only)"
          .to_string(),
      ),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    };
  };

  let Some((_frame, answer)) = find_label_answer(fixture, &label.id) else {
    return TrainingResultSpatialQueryAnswer {
      status: TrainingResultSpatialQueryStatus::Blocked,
      reason: Some(TrainingResultSpatialQueryReason::ProviderOutputInvalid),
      message: Some(format!(
        "MC-18 closed_scene_toy provider blocked: no fixture answer row for label `{}`",
        label.id
      )),
      basis_frame_id: None,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
    };
  };

  TrainingResultSpatialQueryAnswer {
    status: TrainingResultSpatialQueryStatus::Answered,
    reason: None,
    message: Some(MC18_V1_TOY_ANSWER_MESSAGE.to_string()),
    basis_frame_id: Some(format!(
      "closed_scene_toy:{}:{}",
      fixture.fixture_id, _frame.basis_frame_id
    )),
    visibility: Some(answer.visibility),
    screen_point: answer.screen_point,
    match_radius_px: None,
    confidence: None,
  }
}

fn find_matching_label<'a>(
  fixture: &'a ClosedSceneToyFixture,
  request: &TrainingResultSpatialQueryRequest,
) -> Option<&'a ClosedSceneToyLabel> {
  fixture.labels.iter().find(|label| {
    label.block == request.target_block
      && label.semantics == request.target_semantics
      && label
        .face
        .is_none_or(|label_face| request.target_face == Some(label_face))
  })
}

fn find_label_answer<'a>(
  fixture: &'a ClosedSceneToyFixture,
  label_id: &str,
) -> Option<(&'a ClosedSceneToyFrame, &'a ClosedSceneToyFrameAnswer)> {
  for frame in &fixture.frames {
    if let Some(answer) = frame
      .answers
      .iter()
      .find(|answer| answer.label_id == label_id)
    {
      return Some((frame, answer));
    }
  }
  None
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  fn sample_fixture() -> ClosedSceneToyFixture {
    ClosedSceneToyFixture {
      schema_version: CLOSED_SCENE_TOY_FIXTURE_SCHEMA_VERSION,
      fixture_id: "mc18-test-v1".to_string(),
      generated_at: "2026-06-27T00:00:00Z".to_string(),
      labels: vec![ClosedSceneToyLabel {
        id: "door-north".to_string(),
        block: BlockPosition::new(511, 73, 728),
        face: Some(BlockFace::North),
        semantics: MinecraftTargetSemantics::HitFaceCenter,
      }],
      frames: vec![ClosedSceneToyFrame {
        basis_frame_id: "frame-0003".to_string(),
        answers: vec![ClosedSceneToyFrameAnswer {
          label_id: "door-north".to_string(),
          visibility: ProjectionVisibility::Visible,
          screen_point: Some(Point::new(640.0, 360.0)),
        }],
      }],
    }
  }

  fn request_for(
    block: BlockPosition,
    face: Option<BlockFace>,
  ) -> TrainingResultSpatialQueryRequest {
    TrainingResultSpatialQueryRequest {
      source_training_result_artifact_manifest_path: "/tmp/d11.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec![],
      source_run_ids: vec![],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      query_kind:
        crate::training_result_spatial_query::TrainingResultSpatialQueryKind::BlockProjection,
      target_block: block,
      target_face: face,
      target_semantics: MinecraftTargetSemantics::HitFaceCenter,
    }
  }

  #[test]
  fn load_fixture_rejects_unsupported_schema_version() {
    let temp = TempDir::new().expect("tempdir");
    let path = temp.path().join("fixture.json");
    let mut fixture = sample_fixture();
    fixture.schema_version = 99;
    fs::write(
      &path,
      serde_json::to_vec_pretty(&fixture).expect("serialize"),
    )
    .expect("write");

    let error = load_closed_scene_fixture(&path).expect_err("unsupported schema");
    assert!(matches!(
      error,
      ClosedSceneToyFixtureLoadError::UnsupportedSchemaVersion { found: 99 }
    ));
  }

  #[test]
  fn resolve_visible_label_answers_with_toy_basis_frame_id() {
    let fixture = sample_fixture();
    let answer = resolve_closed_label_answer(
      &fixture,
      &request_for(BlockPosition::new(511, 73, 728), Some(BlockFace::North)),
    );

    assert_eq!(answer.status, TrainingResultSpatialQueryStatus::Answered);
    assert_eq!(
      answer.basis_frame_id.as_deref(),
      Some("closed_scene_toy:mc18-test-v1:frame-0003")
    );
    assert_eq!(answer.visibility, Some(ProjectionVisibility::Visible));
    assert!(answer.screen_point.is_some());
  }

  #[test]
  fn resolve_absent_label_blocks_honestly() {
    let fixture = sample_fixture();
    let answer =
      resolve_closed_label_answer(&fixture, &request_for(BlockPosition::new(9, 9, 9), None));

    assert_eq!(answer.status, TrainingResultSpatialQueryStatus::Blocked);
    assert_eq!(
      answer.reason,
      Some(TrainingResultSpatialQueryReason::TargetBlockAbsentFromScenePacket)
    );
  }
}
