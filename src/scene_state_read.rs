//! Canonical scene-state and scan-coverage artifact readers.

use std::path::PathBuf;

use auv_scan::{
  FrameObservation, LifecycleEvent, SCAN_COVERAGE_SCHEMA_VERSION, ScanCoverageWire, ScanFrame, ScanFrameBundle, SceneStateInput,
  SceneStateInspect, TransitionEvidence, build_scene_state_inspect, format_scene_state_inspect_text,
};
use auv_tracing::{ArtifactMetadata, Context, RunSnapshot, RunStore};
use serde::{Deserialize, Serialize};

use crate::run_read::{
  RootArtifactPublishError, RootArtifactReadError, SCAN_COVERAGE_PURPOSE, SCENE_STATE_INPUT_PURPOSE, publish_json_artifact,
  read_one_json_artifact,
};

pub const SCENE_STATE_INPUT_SCHEMA_VERSION: &str = "scan-scene-state-input-v0";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneStateInputWire {
  pub schema_version: String,
  pub frames: Vec<ScanFrame>,
  pub observations_by_frame: Vec<Vec<FrameObservationWire>>,
  pub lifecycle_events: Option<Vec<LifecycleEventWire>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameObservationWire {
  pub observation_id: String,
  pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleEventWire {
  pub event: String,
  pub observation_id: Option<String>,
  pub track_id: Option<String>,
  pub reason_code: Option<String>,
  pub evidence: TransitionEvidenceWire,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionEvidenceWire {
  pub kind: String,
  pub ref_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneStateReadOutcome {
  Present(SceneStateInspect),
  Missing,
  Unsupported { reason: String },
}

pub async fn publish_scene_state_input(
  context: Option<&Context>,
  value: &SceneStateInputWire,
) -> Result<Option<ArtifactMetadata>, RootArtifactPublishError> {
  publish_json_artifact(context, SCENE_STATE_INPUT_PURPOSE, value, validate_scene_state_wire).await
}

pub async fn read_scene_state_input(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Option<SceneStateInputWire>, RootArtifactReadError> {
  read_one_json_artifact(store, snapshot, SCENE_STATE_INPUT_PURPOSE, validate_scene_state_wire).await
}

pub async fn read_scan_coverage(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<Option<ScanCoverageWire>, RootArtifactReadError> {
  read_one_json_artifact(store, snapshot, SCAN_COVERAGE_PURPOSE, |value: &ScanCoverageWire| {
    if value.schema_version == SCAN_COVERAGE_SCHEMA_VERSION {
      Ok(())
    } else {
      Err(format!("schema version mismatch: found {}", value.schema_version))
    }
  })
  .await
}

pub async fn build_scene_state_inspect_for_run(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<SceneStateReadOutcome, RootArtifactReadError> {
  let Some(wire) = read_scene_state_input(store, snapshot).await? else {
    return Ok(SceneStateReadOutcome::Missing);
  };
  let mut input = match wire_to_scene_state_input(wire) {
    Ok(input) => input,
    Err(reason) => return Ok(SceneStateReadOutcome::Unsupported { reason }),
  };
  input.coverage_wire = read_scan_coverage(store, snapshot).await?;
  Ok(match build_scene_state_inspect(&input) {
    Ok(inspect) => SceneStateReadOutcome::Present(inspect),
    Err(error) => SceneStateReadOutcome::Unsupported {
      reason: error.to_string(),
    },
  })
}

pub fn format_scene_state_read_text(outcome: &SceneStateReadOutcome) -> String {
  match outcome {
    SceneStateReadOutcome::Present(inspect) => {
      let mut text = String::from("\nScene state:\n");
      text.push_str(&format_scene_state_inspect_text(inspect));
      text
    }
    SceneStateReadOutcome::Missing => "Scene state: missing auv.runtime.scene_state_input artifact\n".to_string(),
    SceneStateReadOutcome::Unsupported { reason } => format!("Scene state: unsupported ({reason})\n"),
  }
}

fn validate_scene_state_wire(wire: &SceneStateInputWire) -> Result<(), String> {
  if wire.schema_version != SCENE_STATE_INPUT_SCHEMA_VERSION {
    return Err(format!("schema version mismatch: found {}", wire.schema_version));
  }
  if wire.frames.is_empty() {
    return Err("empty frames".to_string());
  }
  if wire.observations_by_frame.len() != wire.frames.len() {
    return Err("observations_by_frame must have one entry per frame".to_string());
  }
  if let Some(events) = &wire.lifecycle_events {
    for event in events {
      parse_lifecycle_event(event)?;
    }
  }
  Ok(())
}

fn wire_to_scene_state_input(wire: SceneStateInputWire) -> Result<SceneStateInput, String> {
  validate_scene_state_wire(&wire)?;
  let lifecycle_events =
    wire.lifecycle_events.as_ref().map(|events| events.iter().map(parse_lifecycle_event).collect::<Result<Vec<_>, _>>()).transpose()?;
  let observations_by_frame = wire
    .observations_by_frame
    .into_iter()
    .map(|frame| {
      frame
        .into_iter()
        .map(|observation| FrameObservation {
          observation_id: observation.observation_id,
          label: observation.label,
        })
        .collect()
    })
    .collect();
  Ok(SceneStateInput {
    bundle: ScanFrameBundle {
      frames: wire.frames,
      // TODO(scene-state-frame-assets-v1): companion image resolution is not
      // part of this JSON-only slice; add typed image ArtifactUri references
      // when the owner approves a scene-state media contract.
      source_dir: PathBuf::new(),
      loaded_json_paths: Vec::new(),
    },
    observations_by_frame,
    lifecycle_events,
    coverage_wire: None,
  })
}

fn parse_lifecycle_event(raw: &LifecycleEventWire) -> Result<LifecycleEvent, String> {
  let evidence = TransitionEvidence {
    kind: raw.evidence.kind.clone(),
    ref_id: raw.evidence.ref_id.clone(),
  };
  match raw.event.as_str() {
    "observed" => Ok(LifecycleEvent::Observed {
      observation_id: required(&raw.observation_id, "lifecycle observed missing observation_id")?,
      evidence,
    }),
    "association_linked" => Ok(LifecycleEvent::AssociationLinked {
      track_id: required(&raw.track_id, "lifecycle association_linked missing track_id")?,
      evidence,
    }),
    "stale" => Ok(LifecycleEvent::Stale {
      reason_code: required(&raw.reason_code, "lifecycle stale missing reason_code")?,
      evidence,
    }),
    "reacquisition_needed" => Ok(LifecycleEvent::ReacquisitionNeeded {
      track_id: required(&raw.track_id, "lifecycle reacquisition_needed missing track_id")?,
      evidence,
    }),
    "reacquired" => Ok(LifecycleEvent::Reacquired {
      track_id: required(&raw.track_id, "lifecycle reacquired missing track_id")?,
      evidence,
    }),
    "lost" => Ok(LifecycleEvent::Lost {
      track_id: required(&raw.track_id, "lifecycle lost missing track_id")?,
      evidence,
    }),
    "ambiguous_reacquire" => Ok(LifecycleEvent::AmbiguousReacquire {
      track_id: required(&raw.track_id, "lifecycle ambiguous_reacquire missing track_id")?,
      evidence,
    }),
    "observation_failed" => Ok(LifecycleEvent::ObservationFailed {
      reason_code: required(&raw.reason_code, "lifecycle observation_failed missing reason_code")?,
      evidence,
    }),
    other => Err(format!("unknown lifecycle event: {other}")),
  }
}

fn required(value: &Option<String>, message: &str) -> Result<String, String> {
  value.clone().ok_or_else(|| message.to_string())
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_scan::{CompletenessWire, SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanImageRef};
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;
  use crate::run_read::publish_scan_coverage;

  fn scene_input() -> SceneStateInputWire {
    SceneStateInputWire {
      schema_version: SCENE_STATE_INPUT_SCHEMA_VERSION.to_string(),
      frames: vec![ScanFrame {
        schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
        frame_id: "frame-1".to_string(),
        sequence_index: 0,
        captured_at_millis: 10,
        window_bounds: ScanBounds {
          x: 0,
          y: 0,
          width: 100,
          height: 80,
        },
        viewport_bounds: None,
        image: ScanImageRef {
          file_name: "frame-1.png".to_string(),
          width: 100,
          height: 80,
          media_type: "image/png".to_string(),
        },
      }],
      observations_by_frame: vec![vec![FrameObservationWire {
        observation_id: "observation-1".to_string(),
        label: "row".to_string(),
      }]],
      lifecycle_events: None,
    }
  }

  fn coverage() -> ScanCoverageWire {
    ScanCoverageWire {
      schema_version: SCAN_COVERAGE_SCHEMA_VERSION.to_string(),
      entries: Vec::new(),
      open_uncertainty_codes: Vec::new(),
      negative_evidence: Vec::new(),
      completeness: CompletenessWire::Complete,
    }
  }

  #[tokio::test]
  async fn scene_and_coverage_publishers_are_noops_without_context() {
    assert!(publish_scene_state_input(None, &scene_input()).await.expect("disabled scene publication").is_none());
    assert!(publish_scan_coverage(None, &coverage()).await.expect("disabled coverage publication").is_none());
  }

  #[tokio::test]
  async fn scene_and_coverage_round_trip_with_exact_purposes() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let scene = scene_input();
    let coverage = coverage();

    let scene_metadata =
      publish_scene_state_input(Some(&root), &scene).await.expect("publish scene input").expect("scene publication enabled");
    let coverage_metadata =
      publish_scan_coverage(Some(&root), &coverage).await.expect("publish coverage").expect("coverage publication enabled");
    dispatch.flush().await.expect("flush scene artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("scene snapshot");

    assert_eq!(scene_metadata.purpose().as_str(), SCENE_STATE_INPUT_PURPOSE);
    assert_eq!(coverage_metadata.purpose().as_str(), SCAN_COVERAGE_PURPOSE);
    assert_eq!(scene_metadata.content_type().to_string(), "application/json");
    assert_eq!(coverage_metadata.content_type().to_string(), "application/json");
    assert_eq!(read_scene_state_input(store.as_ref(), &snapshot).await.expect("read scene input"), Some(scene));
    assert_eq!(read_scan_coverage(store.as_ref(), &snapshot).await.expect("read coverage"), Some(coverage));
  }
}
