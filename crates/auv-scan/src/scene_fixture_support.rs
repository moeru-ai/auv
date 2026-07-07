use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

use crate::association::FrameObservation;
use crate::coverage_artifact::read_coverage_artifact_from_scan_dir;
use crate::lifecycle::{LifecycleEvent, TransitionEvidence};
use crate::producer::produce_frames_from_fixture_dir;
use crate::reader::{ScanFrameBundle, load_scan_frames_from_dir};
use crate::scene_state::SceneStateInput;

static SCENE_FIXTURE_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
pub(crate) struct ObservationFixture {
  pub(crate) observation_id: String,
  pub(crate) label: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LifecycleEventFixture {
  event: String,
  observation_id: Option<String>,
  track_id: Option<String>,
  reason_code: Option<String>,
  evidence: EvidenceFixture,
}

#[derive(Debug, Deserialize)]
struct EvidenceFixture {
  kind: String,
  ref_id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SceneExpectFixture {
  pub(crate) as_of_frame_id: Option<String>,
  pub(crate) identity: Option<String>,
  pub(crate) last_seen_frame_id: Option<String>,
  pub(crate) latest_observation_present: Option<bool>,
  pub(crate) visibility: Option<String>,
  pub(crate) action_ready: Option<bool>,
  pub(crate) blocking_codes: Option<Vec<String>>,
  pub(crate) lifecycle_blocking: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SceneFixture {
  pub(crate) scenario: String,
  pub(crate) frame_fixture: String,
  pub(crate) observations_by_frame: Vec<Vec<ObservationFixture>>,
  pub(crate) lifecycle_events: Option<Vec<LifecycleEventFixture>>,
  pub(crate) expect: SceneExpectFixture,
}

pub(crate) fn load_scene_fixture(scenario_dir: &str) -> SceneFixture {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/scene").join(scenario_dir).join("manifest.json");
  let text = std::fs::read_to_string(&path).expect("read fixture");
  serde_json::from_str(&text).expect("parse fixture")
}

pub(crate) fn observations_from_fixture(raw: &[Vec<ObservationFixture>]) -> Vec<Vec<FrameObservation>> {
  raw
    .iter()
    .map(|frame| {
      frame
        .iter()
        .map(|obs| FrameObservation {
          observation_id: obs.observation_id.clone(),
          label: obs.label.clone(),
        })
        .collect()
    })
    .collect()
}

pub(crate) fn parse_lifecycle_event(raw: &LifecycleEventFixture) -> LifecycleEvent {
  let evidence = TransitionEvidence {
    kind: raw.evidence.kind.clone(),
    ref_id: raw.evidence.ref_id.clone(),
  };
  match raw.event.as_str() {
    "observed" => LifecycleEvent::Observed {
      observation_id: raw.observation_id.clone().expect("observation_id"),
      evidence,
    },
    "association_linked" => LifecycleEvent::AssociationLinked {
      track_id: raw.track_id.clone().expect("track_id"),
      evidence,
    },
    "stale" => LifecycleEvent::Stale {
      reason_code: raw.reason_code.clone().expect("reason_code"),
      evidence,
    },
    "reacquisition_needed" => LifecycleEvent::ReacquisitionNeeded {
      track_id: raw.track_id.clone().expect("track_id"),
      evidence,
    },
    "reacquired" => LifecycleEvent::Reacquired {
      track_id: raw.track_id.clone().expect("track_id"),
      evidence,
    },
    "lost" => LifecycleEvent::Lost {
      track_id: raw.track_id.clone().expect("track_id"),
      evidence,
    },
    "ambiguous_reacquire" => LifecycleEvent::AmbiguousReacquire {
      track_id: raw.track_id.clone().expect("track_id"),
      evidence,
    },
    "observation_failed" => LifecycleEvent::ObservationFailed {
      reason_code: raw.reason_code.clone().expect("reason_code"),
      evidence,
    },
    other => panic!("unknown lifecycle event: {other}"),
  }
}

pub(crate) fn bundle_from_frame_fixture(scenario_dir: &str, frame_fixture: &str) -> ScanFrameBundle {
  let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan").join(frame_fixture);
  let seq = SCENE_FIXTURE_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
  let out_dir =
    std::env::temp_dir()
      .join(format!("auv-scan-scene-{}-{}-{}-{}", scenario_dir, frame_fixture.replace('/', "-"), std::process::id(), seq,));
  let _ = std::fs::remove_dir_all(&out_dir);
  produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
  let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
  let _ = std::fs::remove_dir_all(&out_dir);
  bundle
}

pub(crate) fn coverage_golden_scenario_for_scene(scene_scenario: &str) -> Option<&'static str> {
  match scene_scenario {
    "scene_stable_v0" => Some("coverage_stable_v0"),
    "scene_stale_v0" => Some("coverage_no_observation_v0"),
    "scene_ambiguous_v0" => Some("coverage_ambiguous_v0"),
    _ => None,
  }
}

pub(crate) fn coverage_wire_from_scene_fixture(scenario_dir: &str) -> Option<crate::coverage_artifact::ScanCoverageWire> {
  let golden = coverage_golden_scenario_for_scene(scenario_dir)?;
  let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/coverage").join(golden).join("golden");
  Some(read_coverage_artifact_from_scan_dir(&dir).expect("read coverage golden"))
}

pub(crate) fn scene_input_from_fixture(scenario_dir: &str) -> SceneStateInput {
  let fixture = load_scene_fixture(scenario_dir);
  let lifecycle_events = fixture.lifecycle_events.as_ref().map(|events| events.iter().map(parse_lifecycle_event).collect::<Vec<_>>());
  SceneStateInput {
    bundle: bundle_from_frame_fixture(scenario_dir, &fixture.frame_fixture),
    observations_by_frame: observations_from_fixture(&fixture.observations_by_frame),
    lifecycle_events,
    coverage_wire: coverage_wire_from_scene_fixture(scenario_dir),
  }
}
