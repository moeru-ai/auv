//! Scene state run-read bridge (S6b-1): load provisional staging wire from run artifacts.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use auv_scan::{
  FrameObservation, LifecycleEvent, SCAN_COVERAGE_ARTIFACT_ROLE, ScanCoverageWire, ScanFrame,
  ScanFrameBundle, SceneStateInput, SceneStateInspect, TransitionEvidence,
  build_scene_state_inspect, format_scene_state_inspect_text, read_coverage_artifact,
};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;

pub const SCENE_STATE_INPUT_ARTIFACT_ROLE: &str = "scan-scene-state-input-v0";
pub const SCENE_STATE_INPUT_SCHEMA_VERSION: &str = "scan-scene-state-input-v0";

// NOTICE(s6b1): provisional staging only; not scan-scene-state-v0; not a durable contract;
// test-only producer in this slice; no runtime writer.

#[derive(Debug, Deserialize)]
struct SceneStateInputWire {
  schema_version: String,
  frames: Vec<ScanFrame>,
  observations_by_frame: Vec<Vec<FrameObservationWire>>,
  lifecycle_events: Option<Vec<LifecycleEventWire>>,
}

#[derive(Debug, Deserialize)]
struct FrameObservationWire {
  observation_id: String,
  label: String,
}

#[derive(Debug, Deserialize)]
struct LifecycleEventWire {
  event: String,
  observation_id: Option<String>,
  track_id: Option<String>,
  reason_code: Option<String>,
  evidence: TransitionEvidenceWire,
}

#[derive(Debug, Deserialize)]
struct TransitionEvidenceWire {
  kind: String,
  ref_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneStateReadOutcome {
  Present(SceneStateInspect),
  Missing,
  Unsupported { reason: String },
}

#[derive(Debug, Error)]
pub enum SceneStateReadError {
  #[error("failed to read scene state artifact: {0}")]
  Read(String),
}

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

fn parse_lifecycle_event(raw: &LifecycleEventWire) -> Result<LifecycleEvent, String> {
  let evidence = TransitionEvidence {
    kind: raw.evidence.kind.clone(),
    ref_id: raw.evidence.ref_id.clone(),
  };
  match raw.event.as_str() {
    "observed" => Ok(LifecycleEvent::Observed {
      observation_id: raw
        .observation_id
        .clone()
        .ok_or_else(|| "lifecycle observed missing observation_id".to_string())?,
      evidence,
    }),
    "association_linked" => Ok(LifecycleEvent::AssociationLinked {
      track_id: raw
        .track_id
        .clone()
        .ok_or_else(|| "lifecycle association_linked missing track_id".to_string())?,
      evidence,
    }),
    "stale" => Ok(LifecycleEvent::Stale {
      reason_code: raw
        .reason_code
        .clone()
        .ok_or_else(|| "lifecycle stale missing reason_code".to_string())?,
      evidence,
    }),
    "reacquisition_needed" => Ok(LifecycleEvent::ReacquisitionNeeded {
      track_id: raw
        .track_id
        .clone()
        .ok_or_else(|| "lifecycle reacquisition_needed missing track_id".to_string())?,
      evidence,
    }),
    "reacquired" => Ok(LifecycleEvent::Reacquired {
      track_id: raw
        .track_id
        .clone()
        .ok_or_else(|| "lifecycle reacquired missing track_id".to_string())?,
      evidence,
    }),
    "lost" => Ok(LifecycleEvent::Lost {
      track_id: raw
        .track_id
        .clone()
        .ok_or_else(|| "lifecycle lost missing track_id".to_string())?,
      evidence,
    }),
    "ambiguous_reacquire" => Ok(LifecycleEvent::AmbiguousReacquire {
      track_id: raw
        .track_id
        .clone()
        .ok_or_else(|| "lifecycle ambiguous_reacquire missing track_id".to_string())?,
      evidence,
    }),
    "observation_failed" => Ok(LifecycleEvent::ObservationFailed {
      reason_code: raw
        .reason_code
        .clone()
        .ok_or_else(|| "lifecycle observation_failed missing reason_code".to_string())?,
      evidence,
    }),
    other => Err(format!("unknown lifecycle event: {other}")),
  }
}

fn wire_to_scene_state_input(
  wire: SceneStateInputWire,
  source_dir: PathBuf,
) -> Result<SceneStateInput, String> {
  if wire.schema_version != SCENE_STATE_INPUT_SCHEMA_VERSION {
    return Err(format!(
      "schema version mismatch: found {}",
      wire.schema_version
    ));
  }
  if wire.frames.is_empty() {
    return Err("empty frames".to_string());
  }
  let lifecycle_events = match wire.lifecycle_events {
    None => None,
    Some(events) => {
      let mut parsed = Vec::with_capacity(events.len());
      for event in &events {
        parsed.push(parse_lifecycle_event(event)?);
      }
      Some(parsed)
    }
  };
  let observations_by_frame = wire
    .observations_by_frame
    .into_iter()
    .map(|frame| {
      frame
        .into_iter()
        .map(|obs| FrameObservation {
          observation_id: obs.observation_id,
          label: obs.label,
        })
        .collect()
    })
    .collect();
  Ok(SceneStateInput {
    bundle: ScanFrameBundle {
      frames: wire.frames,
      source_dir,
      loaded_json_paths: Vec::new(),
    },
    observations_by_frame,
    lifecycle_events,
    coverage_wire: None,
  })
}

fn matching_coverage_artifacts(run: &CanonicalRun) -> Vec<&ArtifactRecordV1Alpha1> {
  run
    .artifacts
    .iter()
    .filter(|artifact| {
      artifact.role == SCAN_COVERAGE_ARTIFACT_ROLE && is_json_mime(&artifact.mime_type)
    })
    .collect()
}

fn resolve_coverage_wire_for_run(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<Option<ScanCoverageWire>, String> {
  let matches = matching_coverage_artifacts(run);
  match matches.len() {
    0 => Ok(None),
    1 => {
      let (_record, path) = store
        .artifact_file(run.run.run_id.as_str(), matches[0].artifact_id.as_str())
        .map_err(|error| format!("open coverage artifact: {error}"))?;
      read_coverage_artifact(&path).map(Some).map_err(|error| {
        format!(
          "failed to read {} for run {}: {error}",
          path.display(),
          run.run.run_id
        )
      })
    }
    _ => Err("multiple scan-coverage-v0 artifacts".to_string()),
  }
}

fn matching_scene_state_artifacts(run: &CanonicalRun) -> Vec<&ArtifactRecordV1Alpha1> {
  run
    .artifacts
    .iter()
    .filter(|artifact| {
      artifact.role == SCENE_STATE_INPUT_ARTIFACT_ROLE && is_json_mime(&artifact.mime_type)
    })
    .collect()
}

fn read_scene_state_input_from_artifact(
  store: &LocalStore,
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
) -> Result<Result<SceneStateInput, String>, SceneStateReadError> {
  let (_record, path) = store
    .artifact_file(run.run.run_id.as_str(), artifact.artifact_id.as_str())
    .map_err(|error| SceneStateReadError::Read(format!("open artifact: {error}")))?;
  let source_dir = path
    .parent()
    .map(std::path::Path::to_path_buf)
    .ok_or_else(|| SceneStateReadError::Read("artifact path has no parent".to_string()))?;
  let file = File::open(&path).map_err(|error| {
    SceneStateReadError::Read(format!(
      "open {} for run {}: {error}",
      path.display(),
      run.run.run_id
    ))
  })?;
  let wire: SceneStateInputWire = match serde_json::from_reader(BufReader::new(file)) {
    Ok(wire) => wire,
    Err(error) => {
      return Ok(Err(format!(
        "failed to parse {} for run {}: {error}",
        path.display(),
        run.run.run_id
      )));
    }
  };
  Ok(wire_to_scene_state_input(wire, source_dir))
}

pub fn build_scene_state_inspect_for_run(
  store: &LocalStore,
  run: &CanonicalRun,
) -> Result<SceneStateReadOutcome, SceneStateReadError> {
  let matches = matching_scene_state_artifacts(run);
  match matches.len() {
    0 => Ok(SceneStateReadOutcome::Missing),
    1 => match read_scene_state_input_from_artifact(store, run, matches[0])? {
      Ok(mut input) => {
        let coverage_wire = match resolve_coverage_wire_for_run(store, run) {
          Ok(wire) => wire,
          Err(reason) => {
            return Ok(SceneStateReadOutcome::Unsupported { reason });
          }
        };
        input.coverage_wire = coverage_wire;
        match build_scene_state_inspect(&input) {
          Ok(inspect) => Ok(SceneStateReadOutcome::Present(inspect)),
          Err(error) => Ok(SceneStateReadOutcome::Unsupported {
            reason: error.to_string(),
          }),
        }
      }
      Err(reason) => Ok(SceneStateReadOutcome::Unsupported { reason }),
    },
    _ => Ok(SceneStateReadOutcome::Unsupported {
      reason: "multiple scan-scene-state-input-v0 artifacts".to_string(),
    }),
  }
}

pub fn format_scene_state_read_text(outcome: &SceneStateReadOutcome) -> String {
  match outcome {
    SceneStateReadOutcome::Present(inspect) => {
      let mut text = String::from("\nScene state:\n");
      text.push_str(&format_scene_state_inspect_text(inspect));
      text
    }
    SceneStateReadOutcome::Missing => {
      "Scene state: missing scan-scene-state-input-v0 artifact\n".to_string()
    }
    SceneStateReadOutcome::Unsupported { reason } => {
      format!("Scene state: unsupported ({reason})\n")
    }
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::path::{Path, PathBuf};

  use serde::{Deserialize, Serialize};

  use auv_scan::{
    CoverageInspectSource, SCAN_COVERAGE_ARTIFACT_FILE_NAME, SCAN_COVERAGE_ARTIFACT_ROLE,
    ScanCoverageWire, build_scene_state_inspect, load_scan_frames_from_dir,
    produce_frames_from_fixture_dir, read_coverage_artifact,
  };
  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId,
    SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  use std::sync::atomic::{AtomicU64, Ordering};

  static WIRE_FIXTURE_SEQ: AtomicU64 = AtomicU64::new(0);

  use crate::inspect::inspect_run;

  use super::{
    SCENE_STATE_INPUT_ARTIFACT_ROLE, SCENE_STATE_INPUT_SCHEMA_VERSION, SceneStateReadOutcome,
    build_scene_state_inspect_for_run, format_scene_state_read_text,
  };

  #[derive(Debug, Deserialize, Serialize)]
  struct ObservationFixture {
    observation_id: String,
    label: String,
  }

  #[derive(Debug, Deserialize)]
  struct SceneStableManifest {
    frame_fixture: String,
    observations_by_frame: Vec<Vec<ObservationFixture>>,
    lifecycle_events: Option<serde_json::Value>,
  }

  #[derive(Serialize)]
  struct SceneStateInputWireFixture {
    schema_version: String,
    frames: Vec<auv_scan::ScanFrame>,
    observations_by_frame: Vec<Vec<ObservationFixture>>,
    lifecycle_events: Option<serde_json::Value>,
  }

  fn stage_json_artifact<T: Serialize>(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    value: &T,
  ) -> auv_tracing_driver::trace::ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    let rendered =
      serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
    fs::write(&source_path, rendered).expect("artifact source should write");
    store
      .stage_artifact_file(
        run_id,
        index,
        span_id,
        None,
        auv_tracing_driver::ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }

  fn scene_manifest_path(scenario: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("crates/auv-scan/tests/fixtures/scan/scene")
      .join(scenario)
      .join("manifest.json")
  }

  fn coverage_golden_scenario_for_scene(scene_scenario: &str) -> Option<&'static str> {
    match scene_scenario {
      "scene_stable_v0" => Some("coverage_stable_v0"),
      "scene_stale_v0" => Some("coverage_no_observation_v0"),
      "scene_ambiguous_v0" => Some("coverage_ambiguous_v0"),
      _ => None,
    }
  }

  fn load_coverage_golden_wire(coverage_scenario: &str) -> ScanCoverageWire {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("crates/auv-scan/tests/fixtures/scan/coverage")
      .join(coverage_scenario)
      .join("golden")
      .join(SCAN_COVERAGE_ARTIFACT_FILE_NAME);
    read_coverage_artifact(&path).expect("read coverage golden")
  }

  fn build_scene_state_wire_from_scene(scenario: &str) -> SceneStateInputWireFixture {
    let manifest_path = scene_manifest_path(scenario);
    let manifest_text = fs::read_to_string(&manifest_path).expect("read scene manifest");
    let manifest: SceneStableManifest =
      serde_json::from_str(&manifest_text).expect("parse scene manifest");
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("crates/auv-scan/tests/fixtures/scan")
      .join(&manifest.frame_fixture);
    let seq = WIRE_FIXTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scene-state-wire-{}-{}-{}-{}",
      scenario.replace('/', "-"),
      std::process::id(),
      crate::model::now_millis(),
      seq,
    ));
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce frames");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load frames");
    let _ = fs::remove_dir_all(&out_dir);
    SceneStateInputWireFixture {
      schema_version: SCENE_STATE_INPUT_SCHEMA_VERSION.to_string(),
      frames: bundle.frames,
      observations_by_frame: manifest.observations_by_frame,
      lifecycle_events: manifest.lifecycle_events,
    }
  }

  fn build_scene_state_wire_from_fixture() -> SceneStateInputWireFixture {
    build_scene_state_wire_from_scene("scene_stable_v0")
  }

  fn write_scene_state_run_fixture(
    store: &LocalStore,
    root: &Path,
    wire: &SceneStateInputWireFixture,
    coverage: Option<&ScanCoverageWire>,
  ) -> String {
    let run_id = RunId::new("run_scene_state_read_proof");
    let span_id = SpanId::new("span_scene_state_read");
    let run_id_str = run_id.as_str().to_string();
    let mut artifacts = vec![stage_json_artifact(
      store,
      root,
      &run_id,
      &span_id,
      0,
      SCENE_STATE_INPUT_ARTIFACT_ROLE,
      "scan-scene-state-input.json",
      wire,
    )];
    if let Some(coverage_wire) = coverage {
      artifacts.push(stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        1,
        SCAN_COVERAGE_ARTIFACT_ROLE,
        SCAN_COVERAGE_ARTIFACT_FILE_NAME,
        coverage_wire,
      ));
    }
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("trace_scene_state_read"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: Some("scene state read proof fixture".to_string()),
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id,
          parent_span_id: None,
          name: "auv.scene_state.span".to_string(),
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          attributes: BTreeMap::new(),
          summary: None,
          failure: None,
        }],
        events: Vec::new(),
        artifacts,
      })
      .expect("scene state fixture run should persist");
    run_id_str
  }

  fn section_markers() -> [&'static str; 7] {
    [
      "[scene.input]",
      "[scene.coverage]",
      "[scene.readiness]",
      "[scene.track]",
      "[scene.recommended]",
      "[scene.diagnostics]",
      "[scene.draft_answers]",
    ]
  }

  fn assert_durable_parity_with_in_memory(scene_scenario: &str) {
    let coverage_scenario = coverage_golden_scenario_for_scene(scene_scenario)
      .unwrap_or_else(|| panic!("no coverage mapping for {scene_scenario}"));
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-durable-{}-{}",
      scene_scenario.replace('/', "-"),
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_scene(scene_scenario);
    let coverage = load_coverage_golden_wire(coverage_scenario);
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, Some(&coverage));
    let canonical = store.read_run(&run_id).expect("run should read");

    let durable_outcome =
      build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    let SceneStateReadOutcome::Present(ref durable_inspect) = durable_outcome else {
      panic!("expected present durable inspect for {scene_scenario}");
    };
    assert_eq!(
      durable_inspect.coverage_source,
      CoverageInspectSource::Durable
    );

    let in_memory_input = super::read_scene_state_input_from_artifact(
      &store,
      &canonical,
      canonical
        .artifacts
        .iter()
        .find(|artifact| artifact.role == SCENE_STATE_INPUT_ARTIFACT_ROLE)
        .expect("scene input artifact"),
    )
    .expect("read scene input")
    .expect("parse scene input");
    let in_memory_inspect = build_scene_state_inspect(&in_memory_input).expect("in-memory inspect");
    assert_eq!(
      in_memory_inspect.coverage_source,
      CoverageInspectSource::InMemory
    );
    assert_eq!(durable_inspect.product, in_memory_inspect.product);
    let text = auv_scan::format_scene_state_inspect_text(durable_inspect);
    assert!(text.contains("[scene.coverage] source=durable"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_present() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-present-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, None);
    let canonical = store.read_run(&run_id).expect("run should read");

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert!(matches!(outcome, SceneStateReadOutcome::Present(_)));
    if let SceneStateReadOutcome::Present(inspect) = &outcome {
      assert_eq!(inspect.coverage_source, CoverageInspectSource::InMemory);
    }

    let text = format_scene_state_read_text(&outcome);
    assert!(text.contains("Scene state:\n"));
    assert!(text.contains("[scene.coverage] source=in_memory"));
    for marker in section_markers() {
      assert!(text.contains(marker), "missing marker {marker}");
    }

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_missing() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-missing-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, None);
    let mut canonical = store.read_run(&run_id).expect("run should read");
    canonical.artifacts.clear();

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert_eq!(outcome, SceneStateReadOutcome::Missing);
    assert_eq!(
      format_scene_state_read_text(&outcome),
      "Scene state: missing scan-scene-state-input-v0 artifact\n"
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_unsupported_bad_schema() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-bad-schema-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let mut wire = build_scene_state_wire_from_fixture();
    wire.schema_version = "scan-scene-state-input-v1".to_string();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, None);
    let canonical = store.read_run(&run_id).expect("run should read");

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert!(
      matches!(outcome, SceneStateReadOutcome::Unsupported { .. }),
      "expected unsupported outcome"
    );
    let text = format_scene_state_read_text(&outcome);
    assert!(text.starts_with("Scene state: unsupported ("));
    assert!(text.contains("schema version mismatch"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_unsupported_multiple_artifacts() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-multiple-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, None);
    let mut canonical = store.read_run(&run_id).expect("run should read");
    let second = canonical.artifacts[0].clone();
    canonical.artifacts.push(second);

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert_eq!(
      outcome,
      SceneStateReadOutcome::Unsupported {
        reason: "multiple scan-scene-state-input-v0 artifacts".to_string(),
      }
    );
    assert!(format_scene_state_read_text(&outcome).contains("multiple"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_with_durable_coverage_stable() {
    assert_durable_parity_with_in_memory("scene_stable_v0");
  }

  #[test]
  fn build_scene_state_inspect_for_run_with_durable_coverage_stale() {
    assert_durable_parity_with_in_memory("scene_stale_v0");
  }

  #[test]
  fn build_scene_state_inspect_for_run_with_durable_coverage_ambiguous() {
    assert_durable_parity_with_in_memory("scene_ambiguous_v0");
  }

  #[test]
  fn build_scene_state_inspect_for_run_unsupported_multiple_coverage_artifacts() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-multiple-coverage-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let coverage = load_coverage_golden_wire("coverage_stable_v0");
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, Some(&coverage));
    let mut canonical = store.read_run(&run_id).expect("run should read");
    let second = canonical
      .artifacts
      .iter()
      .find(|artifact| artifact.role == SCAN_COVERAGE_ARTIFACT_ROLE)
      .expect("coverage artifact")
      .clone();
    canonical.artifacts.push(second);

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert_eq!(
      outcome,
      SceneStateReadOutcome::Unsupported {
        reason: "multiple scan-coverage-v0 artifacts".to_string(),
      }
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_unsupported_bad_coverage_schema() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-bad-coverage-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let mut coverage = load_coverage_golden_wire("coverage_stable_v0");
    coverage.schema_version = "scan-coverage-v1".to_string();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, Some(&coverage));
    let canonical = store.read_run(&run_id).expect("run should read");

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert!(
      matches!(outcome, SceneStateReadOutcome::Unsupported { .. }),
      "expected unsupported outcome"
    );
    let text = format_scene_state_read_text(&outcome);
    assert!(text.contains("schema_version mismatch"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_scene_state_inspect_for_run_prefers_bad_scene_input_over_bad_coverage() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-bad-scene-and-coverage-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let mut wire = build_scene_state_wire_from_fixture();
    wire.schema_version = "scan-scene-state-input-v1".to_string();
    let mut coverage = load_coverage_golden_wire("coverage_stable_v0");
    coverage.schema_version = "scan-coverage-v1".to_string();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, Some(&coverage));
    let canonical = store.read_run(&run_id).expect("run should read");

    let outcome = build_scene_state_inspect_for_run(&store, &canonical).expect("inspect read");
    assert!(
      matches!(outcome, SceneStateReadOutcome::Unsupported { .. }),
      "expected unsupported outcome"
    );
    let text = format_scene_state_read_text(&outcome);
    assert!(text.contains("scan-scene-state-input-v1"));
    assert!(!text.contains("scan-coverage-v1"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn inspect_run_includes_durable_coverage() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-inspect-durable-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let coverage = load_coverage_golden_wire("coverage_stable_v0");
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, Some(&coverage));

    let output = inspect_run(&store, &run_id).expect("inspect_run");
    assert!(output.contains("Scene state:"));
    assert!(output.contains("[scene.coverage] source=durable"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn inspect_run_includes_scene_state_block() {
    let root = std::env::temp_dir().join(format!(
      "auv-scene-state-inspect-run-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let wire = build_scene_state_wire_from_fixture();
    let run_id = write_scene_state_run_fixture(&store, &root, &wire, None);

    let output = inspect_run(&store, &run_id).expect("inspect_run");
    assert!(output.contains("Scene state:"));
    assert!(output.contains("[scene.input]"));

    let _ = fs::remove_dir_all(root);
  }
}
