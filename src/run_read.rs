//! Read-side helpers for stored operation results and observation snapshots.
//!
//! These helpers intentionally sit below `runtime` and `inspect_server` so
//! both call sites reuse one artifact scan / compatibility policy:
//!
//! - verification claims come from `operation-result` JSON artifacts
//! - observation snapshots come from `scroll-scan` JSON artifacts
//! - legacy `OperationOutput::Verification` remains readable without
//!   double-counting artifacts that also populate `OperationResult.verifications`

use std::fs;

use serde::de::DeserializeOwned;

use crate::contract::{ObservationSnapshot, OperationOutput, OperationResult, VerificationResult};
use crate::model::AuvResult;
use crate::scroll_scan::ScrollScanArtifact;
use crate::store::{CanonicalRun, LocalStore};
use crate::trace::ArtifactRecordV1Alpha1;

pub(crate) fn list_verifications(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<VerificationResult>> {
  let run = store.read_run(run_id)?;
  extract_verifications(store, &run)
}

pub(crate) fn extract_verifications(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<VerificationResult>> {
  let mut verifications = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let operation_result: OperationResult =
      read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-result")?;
    if !operation_result.verifications.is_empty() {
      verifications.extend(operation_result.verifications);
      continue;
    }
    if let OperationOutput::Verification { verification } = operation_result.output {
      verifications.push(*verification);
    }
  }
  Ok(verifications)
}

pub(crate) fn list_observation_snapshots(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<ObservationSnapshot>> {
  let run = store.read_run(run_id)?;
  extract_observation_snapshots(store, &run)
}

pub(crate) fn extract_observation_snapshots(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<ObservationSnapshot>> {
  let mut snapshots = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "scroll-scan" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let scroll_scan_artifact: ScrollScanArtifact =
      read_artifact_json(store, run.run.run_id.as_str(), artifact, "scroll-scan")?;
    snapshots.extend(scroll_scan_artifact.snapshots);
  }
  Ok(snapshots)
}

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<T> {
  let (_, artifact_path) = store.artifact_file_scoped(
    run_id,
    artifact.artifact_id.as_str(),
    Some(artifact.span_id.as_str()),
  )?;
  let bytes = fs::read(&artifact_path).map_err(|error| {
    format!(
      "failed to read {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })?;
  serde_json::from_slice(&bytes).map_err(|error| {
    format!(
      "failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      artifact_path.display()
    )
  })
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::{Path, PathBuf};

  use serde::Serialize;
  use serde_json::json;

  use super::{
    extract_observation_snapshots, extract_verifications, list_observation_snapshots,
    list_verifications,
  };
  use crate::contract::{
    OBSERVATION_SNAPSHOT_API_VERSION, OPERATION_RESULT_API_VERSION, ObservationSnapshot,
    ObservationSource, OperationOutput, OperationResult, OperationStatus, RecognitionScope,
    RecognitionSurface, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
  };
  use crate::scroll_scan::{
    CollectionObservation, CompletenessClaim, HookDecisionRecord, ObservationCluster,
    ScanPageRecord, ScanRegion, ScanTarget, ScrollBoundaryCandidate, ScrollScanArtifact,
    SectionCandidate, StopEvidence, StopPolicy, StopReason,
  };
  use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
  use crate::trace::{
    ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn read_side_extractors_collect_verifications_and_snapshots_from_json_artifacts() {
    let root = temp_dir("run-read-contracts");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_contracts");
    let span = dummy_span(&run.root_span_id);

    let legacy_verification = verification(
      VerificationMethod::TextVisible,
      Some("legacy verification".to_string()),
    );
    let top_level_verification = verification(
      VerificationMethod::SemanticMatch,
      Some("top-level verification".to_string()),
    );
    let duplicate_legacy_verification = verification(
      VerificationMethod::StateChanged,
      Some("legacy duplicate should be ignored".to_string()),
    );
    let observation_snapshot = dummy_observation_snapshot(&run.run_id, &span.span_id);

    let operation_legacy = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run.run_id.clone(),
      status: OperationStatus::Completed,
      operation_id: "verify.legacy".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Verification {
        verification: Box::new(legacy_verification.clone()),
      },
      verifications: Vec::new(),
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let operation_top_level = OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run.run_id.clone(),
      status: OperationStatus::Completed,
      operation_id: "music.result.play".to_string(),
      evidence_artifacts: Vec::new(),
      output: OperationOutput::Verification {
        verification: Box::new(duplicate_legacy_verification),
      },
      verifications: vec![top_level_verification.clone()],
      freshness_basis: None,
      known_limits: Vec::new(),
    };
    let scroll_scan_artifact = ScrollScanArtifact {
      scan_id: "scan_contracts".to_string(),
      target: ScanTarget {
        application_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        region: ScanRegion {
          left_ratio: 0.1,
          top_ratio: 0.2,
          right_ratio: 0.9,
          bottom_ratio: 0.8,
        },
      },
      stop_policy: StopPolicy::Bounded {
        max_pages: 1,
        max_scrolls: 0,
      },
      pages: Vec::<ScanPageRecord>::new(),
      observations: Vec::<CollectionObservation>::new(),
      nodes: Vec::new(),
      snapshots: vec![observation_snapshot.clone()],
      clusters: Vec::<ObservationCluster>::new(),
      section_candidates: Vec::<SectionCandidate>::new(),
      scroll_boundary_candidates: Vec::<ScrollBoundaryCandidate>::new(),
      hook_decisions: Vec::<HookDecisionRecord>::new(),
      stop_evidence: StopEvidence {
        reason: StopReason::MaxPages,
        message: "bounded for test".to_string(),
        page_index: 0,
      },
      completeness_claim: CompletenessClaim::PartialMaxPages,
      warnings: Vec::new(),
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        "operation-result",
        "verify-legacy.json",
        &operation_legacy,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        "operation-result",
        "music-result-play.json",
        &operation_top_level,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        "scroll-scan",
        "scroll-scan.json",
        &scroll_scan_artifact,
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store
      .read_run("run_read_contracts")
      .expect("run should read back");

    let extracted_verifications =
      extract_verifications(&store, &canonical).expect("verifications should extract");
    assert_eq!(
      extracted_verifications,
      vec![legacy_verification.clone(), top_level_verification.clone()]
    );
    let listed_verifications =
      list_verifications(&store, "run_read_contracts").expect("verifications should list");
    assert_eq!(listed_verifications, extracted_verifications);

    let extracted_snapshots = extract_observation_snapshots(&store, &canonical)
      .expect("observation snapshots should extract");
    assert_eq!(extracted_snapshots, vec![observation_snapshot.clone()]);
    let listed_snapshots = list_observation_snapshots(&store, "run_read_contracts")
      .expect("observation snapshots should list");
    assert_eq!(listed_snapshots, extracted_snapshots);

    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
    let root_span_id = SpanId::new("0000000000000001");
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new(run_id),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      root_span_id,
      attributes: BTreeMap::new(),
      summary: Some("done".to_string()),
      failure: None,
    }
  }

  fn dummy_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
    SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: span_id.clone(),
      parent_span_id: None,
      name: "auv.run.read".to_string(),
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      attributes: BTreeMap::new(),
      summary: None,
      failure: None,
    }
  }

  fn verification(
    method: VerificationMethod,
    observed_label: Option<String>,
  ) -> VerificationResult {
    VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label,
    }
  }

  fn dummy_observation_snapshot(run_id: &RunId, span_id: &SpanId) -> ObservationSnapshot {
    ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_contracts".to_string(),
      run_id: run_id.clone(),
      span_id: span_id.clone(),
      captured_at_millis: 150,
      source: ObservationSource::Visual,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      capture_contract_ref: None,
      evidence: Vec::new(),
      nodes: Vec::new(),
      detail: json!({"producer": "scroll_scan"}),
      known_limits: vec!["visual only".to_string()],
    }
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
  ) -> ArtifactRecordV1Alpha1 {
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
        ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }
}
