//! Read-side helpers for stored operation results and observation snapshots.
//!
//! These helpers intentionally sit below `runtime` and the root inspect read
//! projection used by `auv-inspect-server` so both call sites reuse one artifact
//! scan / compatibility policy:
//!
//! - verification claims come from `operation-result` JSON artifacts
//! - observation snapshots come from `scroll-scan` JSON artifacts
//! - legacy `OperationOutput::Verification` remains readable without
//!   double-counting artifacts that also populate `OperationResult.verifications`

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use crate::contract::{
  ArtifactRef, FailureLayer, ObservationSnapshot, OperationOutput, OperationResult, OperationStatus, RecognitionResult, RecognitionSource,
  VerificationMethod, VerificationResult,
};
use crate::minecraft_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
use crate::model::AuvResult;
use crate::osu_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID as OSU_QUERY_WIRED_LIVE_ACTION_OPERATION_ID;
use crate::scroll_scan::ScrollScanArtifact;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;
use serde::de::DeserializeOwned;

mod balatro;
mod minecraft;
mod osu;
mod query_wired_live_action;

pub use self::balatro::*;
pub use self::minecraft::*;
pub use self::osu::*;
pub use self::query_wired_live_action::*;

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  store.read_run(run_id)
}

const DETECTOR_RECOGNITION_ARTIFACT_ROLE: &str = "detector-recognition";

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectorRecognitionLineageStatus {
  Ready,
  MissingCaptureArtifact,
  MissingEvidence,
  CaptureArtifactUnresolved,
  Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DetectorRecognitionArtifactRefLineage {
  pub run_id: auv_tracing_driver::trace::RunId,
  pub artifact_id: auv_tracing_driver::trace::ArtifactId,
  pub span_id: auv_tracing_driver::trace::SpanId,
  pub captured_event_id: Option<auv_tracing_driver::trace::EventId>,
  pub role: Option<String>,
  pub path: Option<String>,
  pub summary: Option<String>,
  pub resolved: bool,
}

pub type ArtifactRefLineage = DetectorRecognitionArtifactRefLineage;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DetectorRecognitionLineage {
  pub artifact: DetectorRecognitionArtifactRefLineage,
  pub status: DetectorRecognitionLineageStatus,
  pub recognition_id: Option<String>,
  pub source: Option<RecognitionSource>,
  pub backend: Option<String>,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
  pub class_label_source_kind: Option<String>,
  pub runtime_projection_kind: Option<String>,
  pub capture_artifact: Option<DetectorRecognitionArtifactRefLineage>,
  pub capture_contract_artifact: Option<DetectorRecognitionArtifactRefLineage>,
  pub evidence_artifacts: Vec<DetectorRecognitionArtifactRefLineage>,
  pub all_count: Option<usize>,
  pub filtered_count: Option<usize>,
  pub best_item_id: Option<String>,
  pub known_limits: Vec<String>,
  pub issue: Option<String>,
}

pub(crate) fn list_verifications(store: &LocalStore, run_id: &str) -> AuvResult<Vec<VerificationResult>> {
  let run = store.read_run(run_id)?;
  extract_verifications(store, &run)
}

pub(crate) fn extract_verifications(store: &LocalStore, run: &CanonicalRun) -> AuvResult<Vec<VerificationResult>> {
  let mut verifications = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let operation_result: OperationResult = read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-result")?;
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

/// Read the persisted `OperationResult` for a run, if one was recorded.
///
/// Scans the run's artifacts for the first `operation-result` JSON record,
/// mirroring the role/mime filter used by [`extract_verifications`]. Returns
/// `Ok(None)` when the run exists but recorded no operation result.
///
/// This is the storage-side half of the API-P4 `GetOperation` read path; the
/// two-source join with the runtime summary lives in
/// `crate::api::session_service::summary`.
pub fn read_operation_result(store: &LocalStore, run_id: &str) -> AuvResult<Option<OperationResult>> {
  let run = store.read_run(run_id)?;
  for artifact in &run.artifacts {
    if artifact.role != "operation-result" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let operation_result: OperationResult = read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-result")?;
    return Ok(Some(operation_result));
  }
  Ok(None)
}

/// Read the persisted `OperationSummary` for a run, if one was recorded (API-P11).
///
/// Scans the run's artifacts for the first `operation-summary` JSON record,
/// mirroring [`read_operation_result`]. Returns `Ok(None)` when the run exists
/// but recorded no operation summary artifact.
pub fn read_operation_summary(store: &LocalStore, run_id: &str) -> AuvResult<Option<auv_cli_invoke::OperationSummary>> {
  use auv_cli_invoke::OperationSummaryRecord;

  let run = store.read_run(run_id)?;
  for artifact in &run.artifacts {
    if artifact.role != crate::contract::OPERATION_SUMMARY_ARTIFACT_ROLE || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let record: OperationSummaryRecord = read_artifact_json(store, run.run.run_id.as_str(), artifact, "operation-summary")?;
    return Ok(Some(auv_cli_invoke::OperationSummary::from_record(record)));
  }
  Ok(None)
}

pub(crate) fn list_observation_snapshots(store: &LocalStore, run_id: &str) -> AuvResult<Vec<ObservationSnapshot>> {
  let run = store.read_run(run_id)?;
  extract_observation_snapshots(store, &run)
}

pub(crate) fn extract_observation_snapshots(store: &LocalStore, run: &CanonicalRun) -> AuvResult<Vec<ObservationSnapshot>> {
  let mut snapshots = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != "scroll-scan" || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let scroll_scan_artifact: ScrollScanArtifact = read_artifact_json(store, run.run.run_id.as_str(), artifact, "scroll-scan")?;
    snapshots.extend(scroll_scan_artifact.snapshots);
  }
  Ok(snapshots)
}

pub(crate) fn list_detector_recognition_lineage(store: &LocalStore, run_id: &str) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  let run = store.read_run(run_id)?;
  extract_detector_recognition_lineage(store, &run)
}

pub(crate) fn extract_detector_recognition_lineage(store: &LocalStore, run: &CanonicalRun) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  let mut lineage = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != DETECTOR_RECOGNITION_ARTIFACT_ROLE {
      continue;
    }

    let detector_artifact = artifact_record_lineage(run.run.run_id.clone(), artifact);
    if !is_json_mime(&artifact.mime_type) {
      lineage.push(DetectorRecognitionLineage {
        artifact: detector_artifact,
        status: DetectorRecognitionLineageStatus::Malformed,
        recognition_id: None,
        source: None,
        backend: None,
        model_id: None,
        execution_provider: None,
        class_label_source_kind: None,
        runtime_projection_kind: None,
        capture_artifact: None,
        capture_contract_artifact: None,
        evidence_artifacts: Vec::new(),
        all_count: None,
        filtered_count: None,
        best_item_id: None,
        known_limits: Vec::new(),
        issue: Some(format!("detector-recognition artifact mime_type {} is not JSON", artifact.mime_type)),
      });
      continue;
    }

    let parsed = read_artifact_json::<RecognitionResult>(store, run.run.run_id.as_str(), artifact, DETECTOR_RECOGNITION_ARTIFACT_ROLE);

    match parsed {
      Ok(recognition) => lineage.push(detector_recognition_lineage_entry(run, artifact, recognition)),
      Err(error) => lineage.push(DetectorRecognitionLineage {
        artifact: detector_artifact,
        status: DetectorRecognitionLineageStatus::Malformed,
        recognition_id: None,
        source: None,
        backend: None,
        model_id: None,
        execution_provider: None,
        class_label_source_kind: None,
        runtime_projection_kind: None,
        capture_artifact: None,
        capture_contract_artifact: None,
        evidence_artifacts: Vec::new(),
        all_count: None,
        filtered_count: None,
        best_item_id: None,
        known_limits: Vec::new(),
        issue: Some(error),
      }),
    }
  }
  Ok(lineage)
}

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

fn detector_recognition_lineage_entry(
  run: &CanonicalRun,
  artifact: &ArtifactRecordV1Alpha1,
  recognition: RecognitionResult,
) -> DetectorRecognitionLineage {
  let capture_artifact = recognition.scope.capture_artifact.as_ref().map(|reference| resolve_artifact_ref(run, reference));
  let capture_contract_artifact = recognition.scope.capture_contract_artifact.as_ref().map(|reference| resolve_artifact_ref(run, reference));
  let evidence_artifacts = recognition.evidence.iter().map(|reference| resolve_artifact_ref(run, reference)).collect::<Vec<_>>();
  let (status, issue) = classify_detector_recognition_lineage(&recognition, capture_artifact.as_ref());

  DetectorRecognitionLineage {
    artifact: artifact_record_lineage(run.run.run_id.clone(), artifact),
    status,
    recognition_id: Some(recognition.recognition_id.clone()),
    source: Some(recognition.source),
    backend: detail_string(&recognition.detail, &["backend"]),
    model_id: detail_string(&recognition.detail, &["model_id"]),
    execution_provider: detail_string(&recognition.detail, &["execution_provider"]),
    class_label_source_kind: detail_string(&recognition.detail, &["class_label_source", "kind"]),
    runtime_projection_kind: detail_string(&recognition.detail, &["runtime_projection", "kind"]),
    capture_artifact,
    capture_contract_artifact,
    evidence_artifacts,
    all_count: Some(recognition.all.len()),
    filtered_count: Some(recognition.filtered.len()),
    best_item_id: recognition.best.as_ref().map(|item| item.item_id.clone()),
    known_limits: recognition.known_limits,
    issue,
  }
}

fn classify_detector_recognition_lineage(
  recognition: &RecognitionResult,
  capture_artifact: Option<&DetectorRecognitionArtifactRefLineage>,
) -> (DetectorRecognitionLineageStatus, Option<String>) {
  if recognition.scope.capture_artifact.is_none() {
    return (DetectorRecognitionLineageStatus::MissingCaptureArtifact, Some("scope.capture_artifact is missing".to_string()));
  }
  if let Some(capture_artifact) = capture_artifact
    && !capture_artifact.resolved
  {
    return (
      DetectorRecognitionLineageStatus::CaptureArtifactUnresolved,
      Some("scope.capture_artifact could not be resolved from recorded run artifacts".to_string()),
    );
  }
  if recognition.evidence.is_empty() {
    return (DetectorRecognitionLineageStatus::MissingEvidence, Some("recognition evidence list is empty".to_string()));
  }
  (DetectorRecognitionLineageStatus::Ready, None)
}

fn artifact_record_lineage(
  run_id: auv_tracing_driver::trace::RunId,
  artifact: &ArtifactRecordV1Alpha1,
) -> DetectorRecognitionArtifactRefLineage {
  DetectorRecognitionArtifactRefLineage {
    run_id,
    artifact_id: artifact.artifact_id.clone(),
    span_id: artifact.span_id.clone(),
    captured_event_id: artifact.event_id.clone(),
    role: Some(artifact.role.clone()),
    path: Some(artifact.path.clone()),
    summary: artifact.summary.clone(),
    resolved: true,
  }
}

fn resolve_artifact_ref(run: &CanonicalRun, reference: &ArtifactRef) -> DetectorRecognitionArtifactRefLineage {
  let resolved = if reference.run_id == run.run.run_id {
    run.artifacts.iter().find(|artifact| artifact.artifact_id == reference.artifact_id && artifact.span_id == reference.span_id)
  } else {
    None
  };

  DetectorRecognitionArtifactRefLineage {
    run_id: reference.run_id.clone(),
    artifact_id: reference.artifact_id.clone(),
    span_id: reference.span_id.clone(),
    captured_event_id: reference.captured_event_id.clone(),
    role: resolved.map(|artifact| artifact.role.clone()),
    path: resolved.map(|artifact| artifact.path.clone()),
    summary: resolved.and_then(|artifact| artifact.summary.clone()),
    resolved: resolved.is_some(),
  }
}

fn detail_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
  let mut cursor = value;
  for key in path {
    cursor = cursor.get(*key)?;
  }
  cursor.as_str().map(str::to_string)
}

fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<T> {
  let (file, artifact_path) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| {
    format!("failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })
}

fn open_artifact_file(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<(fs::File, PathBuf)> {
  let (_, artifact_path) = store.artifact_file_scoped(run_id, artifact.artifact_id.as_str(), Some(artifact.span_id.as_str()))?;
  let file = fs::File::open(&artifact_path).map_err(|error| {
    format!("failed to open {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })?;
  Ok((file, artifact_path))
}

fn read_telemetry_artifact_summary(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<(PathBuf, usize, u64)> {
  let (_, artifact_path) = store.artifact_file_scoped(run_id, artifact.artifact_id.as_str(), Some(artifact.span_id.as_str()))?;
  let metadata = fs::metadata(&artifact_path).map_err(|error| {
    format!("failed to stat {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
  })?;
  let (file, _) = open_artifact_file(store, run_id, artifact, artifact_role)?;
  let line_count = BufReader::new(file).lines().try_fold(0usize, |count, line| {
    let line = line.map_err(|error| {
      format!("failed to read {artifact_role} artifact {} for run {run_id} from {}: {error}", artifact.artifact_id, artifact_path.display())
    })?;
    Ok::<_, String>(count + usize::from(!line.trim().is_empty()))
  })?;
  Ok((artifact_path, line_count, metadata.len()))
}

pub use crate::view_parser_read::{
  build_view_parser_inspect, build_view_resolution_summary, extract_playlist_select_result_wires, extract_reacquisition_records,
  extract_view_memory_writes, list_view_memory_writes,
};

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::{Path, PathBuf};

  use serde::Serialize;
  use serde_json::json;

  use super::{
    ArtifactRefLineage, DETECTOR_RECOGNITION_ARTIFACT_ROLE, DetectorRecognitionLineageStatus, MinecraftHoldoutRenderQualityManifestSummary,
    MinecraftHoldoutRenderQualityMetricsSummary, MinecraftSpatialBundleManifestSummary,
    MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary, MinecraftTrainingResultHoldoutPreviewManifestSummary,
    MinecraftTrainingResultQualityBaselineReportSummary, MinecraftTrainingResultSpatialQueryManifestLineage,
    MinecraftTrainingResultSpatialQueryManifestSummary, derive_minecraft_query_wired_live_action_summary,
    derive_minecraft_training_result_quality_baseline_report, derive_minecraft_training_result_quality_verdict,
    derive_minecraft_training_result_spatial_query_action_readiness, derive_osu_query_wired_live_action_summary,
    extract_detector_recognition_lineage, extract_minecraft_holdout_render_quality_inspect_reports,
    extract_minecraft_holdout_render_quality_manifests, extract_minecraft_training_job_inspect_reports,
    extract_minecraft_training_job_manifests, extract_minecraft_training_launch_inspect_reports,
    extract_minecraft_training_launch_manifests, extract_minecraft_training_package_inspect_reports,
    extract_minecraft_training_package_manifests, extract_minecraft_training_result_artifact_fetch_inspect_reports,
    extract_minecraft_training_result_artifact_fetch_manifests, extract_minecraft_training_result_holdout_preview_inspect_reports,
    extract_minecraft_training_result_holdout_preview_manifests, extract_minecraft_training_result_inspect_reports,
    extract_minecraft_training_result_manifests, extract_minecraft_training_result_semantic_inspect_reports,
    extract_minecraft_training_result_semantic_manifests, extract_minecraft_training_result_spatial_query_manifests,
    extract_observation_snapshots, extract_verifications, list_detector_recognition_lineage,
    list_minecraft_query_wired_live_action_summaries, list_minecraft_spatial_bundle_manifests, list_minecraft_training_job_inspect_reports,
    list_minecraft_training_job_manifests, list_minecraft_training_launch_inspect_reports, list_minecraft_training_launch_manifests,
    list_minecraft_training_package_inspect_reports, list_minecraft_training_package_manifests,
    list_minecraft_training_result_artifact_fetch_inspect_reports, list_minecraft_training_result_artifact_fetch_manifests,
    list_minecraft_training_result_inspect_reports, list_minecraft_training_result_manifests,
    list_minecraft_training_result_semantic_inspect_reports, list_minecraft_training_result_semantic_manifests,
    list_minecraft_training_result_spatial_query_inspect_reports, list_minecraft_training_result_spatial_query_manifests,
    list_observation_snapshots, list_verifications, quality_baseline_profile_v1, quality_baseline_verdict_thresholds_probe_v1,
    quality_baseline_verdict_thresholds_trained_render_v1,
  };
  use crate::contract::{
    ArtifactRef, OBSERVATION_SNAPSHOT_API_VERSION, OPERATION_RESULT_API_VERSION, ObservationSnapshot, ObservationSource, OperationOutput,
    OperationResult, OperationStatus, RecognitionResult, RecognitionScope, RecognitionSource, RecognitionSurface, RecognizedItem,
    VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
  };
  use crate::scroll_scan::{
    CollectionObservation, CompletenessClaim, HookDecisionRecord, ObservationCluster, ScanPageRecord, ScanRegion, ScanTarget,
    ScrollBoundaryCandidate, ScrollScanArtifact, SectionCandidate, StopEvidence, StopPolicy, StopReason,
  };
  use auv_game_minecraft::dataset::{SourceRunSummary, SpatialBundleCounts};
  use auv_game_minecraft::{
    TrainingCompatibilityStatus, TrainingCompatibilityViewReport, TrainingLaunchInspectReport, TrainingLaunchJobBlocker,
    TrainingLaunchJobCounts, TrainingLaunchJobInspectReport, TrainingLaunchJobManifest, TrainingLaunchJobStatus, TrainingLaunchPlanManifest,
    TrainingLaunchReadiness, TrainingLaunchReadinessBlocker, TrainingPackageCounts, TrainingPackageInspectReport, TrainingPackageManifest,
    TrainingResultArtifactFetchInspectReport, TrainingResultArtifactFetchManifest, TrainingResultArtifactFetchReason,
    TrainingResultArtifactFetchStatus, TrainingResultArtifactRecord, TrainingResultInspectReport, TrainingResultManifest,
    TrainingResultNormalizedArtifactKind, TrainingResultReason, TrainingResultSemanticInspectReport, TrainingResultSemanticManifest,
    TrainingResultSemanticStatus, TrainingResultStatus,
  };
  use auv_tracing_driver::ArtifactFileSource;
  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
    ArtifactId, ArtifactRecordV1Alpha1, EventId, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId,
    SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn read_side_extractors_collect_verifications_and_snapshots_from_json_artifacts() {
    let root = temp_dir("run-read-contracts");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_contracts");
    let span = dummy_span(&run.root_span_id);

    let legacy_verification = verification(VerificationMethod::TextVisible, Some("legacy verification".to_string()));
    let top_level_verification = verification(VerificationMethod::SemanticMatch, Some("top-level verification".to_string()));
    let duplicate_legacy_verification =
      verification(VerificationMethod::StateChanged, Some("legacy duplicate should be ignored".to_string()));
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
      stage_json_artifact(&store, &root, &run.run_id, &span.span_id, 0, "operation-result", "verify-legacy.json", &operation_legacy),
      stage_json_artifact(&store, &root, &run.run_id, &span.span_id, 1, "operation-result", "music-result-play.json", &operation_top_level),
      stage_json_artifact(&store, &root, &run.run_id, &span.span_id, 2, "scroll-scan", "scroll-scan.json", &scroll_scan_artifact),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_contracts").expect("run should read back");

    let extracted_verifications = extract_verifications(&store, &canonical).expect("verifications should extract");
    assert_eq!(extracted_verifications, vec![legacy_verification.clone(), top_level_verification.clone()]);
    let listed_verifications = list_verifications(&store, "run_read_contracts").expect("verifications should list");
    assert_eq!(listed_verifications, extracted_verifications);

    let extracted_snapshots = extract_observation_snapshots(&store, &canonical).expect("observation snapshots should extract");
    assert_eq!(extracted_snapshots, vec![observation_snapshot.clone()]);
    let listed_snapshots = list_observation_snapshots(&store, "run_read_contracts").expect("observation snapshots should list");
    assert_eq!(listed_snapshots, extracted_snapshots);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_manifest_extracts_normalized_artifact_rows() {
    let root = temp_dir("run-read-mc7-d11-fetch-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultArtifactFetchManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_job_status: TrainingResultStatus::Submitted,
      source_result_status: TrainingResultStatus::Succeeded,
      source_result_status_reason: None,
      source_result_dir: "/tmp/result/trainer-output".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      normalized_artifacts: vec![
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::Config,
          relative_path: "config.yml".to_string(),
          absolute_path: "/tmp/result/normalized-result/config.yml".to_string(),
          readable: true,
          byte_size: Some(128),
        },
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::ModelsDirectory,
          relative_path: "nerfstudio_models".to_string(),
          absolute_path: "/tmp/result/normalized-result/nerfstudio_models".to_string(),
          readable: true,
          byte_size: None,
        },
        auv_game_minecraft::TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::StatusSnapshot,
          relative_path: "job_status.json".to_string(),
          absolute_path: "/tmp/result/normalized-result/job_status.json".to_string(),
          readable: true,
          byte_size: Some(32),
        },
      ],
      known_limits: vec!["normalized artifacts only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
      "minecraft-3dgs-training-result-artifact-manifest.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_mc7_d11_fetch_manifest").expect("run should read back");

    let extracted = extract_minecraft_training_result_artifact_fetch_manifests(&store, &canonical).expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should be present");
    assert_eq!(summary.normalized_artifacts.len(), 3);
    assert_eq!(summary.normalized_artifacts[0].kind, "config");
    assert_eq!(summary.normalized_artifacts[0].relative_path, "config.yml");
    assert_eq!(summary.normalized_artifacts[0].absolute_path, "/tmp/result/normalized-result/config.yml");
    assert!(summary.normalized_artifacts[0].readable);
    assert_eq!(summary.normalized_artifacts[0].byte_size, Some(128));
    assert_eq!(summary.normalized_artifacts[1].kind, "models_directory");
    assert_eq!(summary.normalized_artifacts[1].byte_size, None);
    assert_eq!(summary.normalized_artifacts[2].kind, "status_snapshot");

    let listed =
      list_minecraft_training_result_artifact_fetch_manifests(&store, "run_read_mc7_d11_fetch_manifest").expect("manifest should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_inspect_extracts_blocked_summary_fields() {
    let root = temp_dir("run-read-mc7-d11-fetch-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultArtifactFetchInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_artifact_fetch_manifest_path: "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_job_status: TrainingResultStatus::Submitted,
      source_result_status: TrainingResultStatus::Blocked,
      source_result_status_reason: Some(TrainingResultReason::RemoteStatusUnavailable.as_str().to_string()),
      fetch_status: TrainingResultArtifactFetchStatus::Blocked,
      fetch_reason: Some(TrainingResultArtifactFetchReason::SourceResultBlocked),
      source_result_dir: "/tmp/result/trainer-output".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      source_result_dir_exists: false,
      required_artifacts_present: false,
      normalized_artifact_count: 0,
      warnings: vec!["remote status probe unavailable".to_string()],
      known_limits: vec!["blocked evidence only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
      "minecraft-3dgs-training-result-artifact-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_mc7_d11_fetch_inspect").expect("run should read back");

    let extracted = extract_minecraft_training_result_artifact_fetch_inspect_reports(&store, &canonical).expect("report should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should be present");
    assert_eq!(summary.fetch_status, "blocked");
    assert_eq!(summary.fetch_reason.as_deref(), Some("source_result_blocked"));
    assert!(!summary.source_result_dir_exists);
    assert!(!summary.required_artifacts_present);
    assert_eq!(summary.normalized_artifact_count, 0);
    assert_eq!(summary.source_result_status_reason.as_deref(), Some("remote_status_unavailable"));

    let listed =
      list_minecraft_training_result_artifact_fetch_inspect_reports(&store, "run_read_mc7_d11_fetch_inspect").expect("report should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn training_result_artifact_fetch_extractors_report_json_issues() {
    let root = temp_dir("run-read-mc7-d11-fetch-issues");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc7_d11_fetch_issues");
    let span = dummy_span(&run.root_span_id);

    let manifest_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_MANIFEST_ROLE,
      "minecraft-3dgs-training-result-artifact-manifest.txt",
      "not json",
    );
    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      1,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_INSPECT_ROLE,
      "minecraft-3dgs-training-result-artifact-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();
    let artifacts = vec![manifest_artifact, inspect_artifact];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_mc7_d11_fetch_issues").expect("run should read back");

    let manifest_lineage =
      extract_minecraft_training_result_artifact_fetch_manifests(&store, &canonical).expect("manifest lineage should extract");
    assert_eq!(manifest_lineage.len(), 1);
    assert!(manifest_lineage[0].manifest.is_none());
    assert!(manifest_lineage[0].issue.as_deref().is_some_and(|issue| issue.contains("mime_type text/plain is not JSON")));

    let report_lineage =
      extract_minecraft_training_result_artifact_fetch_inspect_reports(&store, &canonical).expect("report lineage should extract");
    assert_eq!(report_lineage.len(), 1);
    assert!(report_lineage[0].report.is_none());
    assert!(report_lineage[0].issue.as_deref().unwrap_or_default().contains("failed to parse"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-mc10-semantic-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultSemanticManifest {
      schema_version: 1,
      generated_at_millis: 99,
      source_training_result_artifact_manifest_path: "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string(), "run-b".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      semantic_status: TrainingResultSemanticStatus::Ready,
      semantic_reason: None,
      config_path: "/tmp/result/normalized-result/config.yml".to_string(),
      models_dir_path: "/tmp/result/normalized-result/nerfstudio_models".to_string(),
      status_snapshot_path: Some("/tmp/result/normalized-result/job_status.json".to_string()),
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      checkpoint_files: vec![auv_game_minecraft::TrainingResultSemanticCheckpointRecord {
        relative_path: "step-000001.ckpt".to_string(),
        byte_size: 32,
      }],
      checkpoint_count: 1,
      known_limits: vec!["semantic gate only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
      "minecraft-3dgs-training-result-semantic.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_mc10_semantic_manifest").expect("run should read back");

    let extracted = extract_minecraft_training_result_semantic_manifests(&store, &canonical).expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should be present");
    assert_eq!(summary.semantic_status, "ready");
    assert_eq!(summary.checkpoint_count, 1);
    assert_eq!(summary.checkpoint_files.len(), 1);
    assert_eq!(summary.checkpoint_files[0].relative_path, "step-000001.ckpt");
    let serialized = serde_json::to_string(summary).expect("summary should serialize");
    assert!(!serialized.contains("generated_at_millis"));

    let listed = list_minecraft_training_result_semantic_manifests(&store, "run_read_mc10_semantic_manifest").expect("manifest should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-mc10-semantic-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultSemanticInspectReport {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path: "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      source_result_status: TrainingResultStatus::Succeeded,
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      semantic_status: TrainingResultSemanticStatus::Ready,
      semantic_reason: None,
      config_yaml_parsed: true,
      config_trainer: Some("nerfstudio.splatfacto".to_string()),
      config_backend_matches: true,
      models_dir_readable: true,
      status_snapshot_present: true,
      checkpoint_count: 1,
      warnings: vec![],
      known_limits: vec!["semantic inspect only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
      "minecraft-3dgs-training-result-semantic-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let canonical = store.read_run("run_read_mc10_semantic_inspect").expect("run should read back");

    let extracted = extract_minecraft_training_result_semantic_inspect_reports(&store, &canonical).expect("report should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should be present");
    assert!(summary.config_yaml_parsed);
    assert!(summary.config_backend_matches);
    assert_eq!(summary.checkpoint_count, 1);
    let serialized = serde_json::to_string(summary).expect("summary should serialize");
    assert!(!serialized.contains("generated_at_millis"));

    let listed =
      list_minecraft_training_result_semantic_inspect_reports(&store, "run_read_mc10_semantic_inspect").expect("report should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-mc10-semantic-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_ROLE,
      "minecraft-3dgs-training-result-semantic.txt",
      "not json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_semantic_manifests(&store, "run_read_mc10_semantic_manifest_non_json")
      .expect("manifest lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().is_some_and(|issue| issue.contains("mime_type text/plain is not JSON")));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_semantic_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-mc10-semantic-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc10_semantic_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_SEMANTIC_INSPECT_ROLE,
      "minecraft-3dgs-training-result-semantic-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![inspect_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_semantic_inspect_reports(&store, "run_read_mc10_semantic_inspect_malformed")
      .expect("report lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_holdout_preview_manifest_lineage_reads_summary() {
    use auv_game_minecraft::{
      HoldoutFrameSelection, HoldoutFrameWitness, HoldoutPreviewStatus, TrainingResultHoldoutPreviewInspectReport,
      TrainingResultHoldoutPreviewManifest,
    };
    let root = temp_dir("run-read-mc16-holdout-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc16_holdout_manifest");
    let span = dummy_span(&run.root_span_id);
    let witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let manifest = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt".to_string()),
      holdout_screenshot_path: Some(witness.screenshot_path.clone()),
      reference_overlay_path: Some("/tmp/holdout/holdout_overlay_frame_000006.png".to_string()),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec!["holdout preview only".to_string()],
    };
    let inspect_report = TrainingResultHoldoutPreviewInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_holdout_preview_manifest_path: "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
      training_result_semantic_manifest_path: manifest.training_result_semantic_manifest_path.clone(),
      source_training_result_artifact_manifest_path: manifest.source_training_result_artifact_manifest_path.clone(),
      source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: manifest.source_run_ids.clone(),
      trainer_backend: manifest.trainer_backend.clone(),
      job_backend: manifest.job_backend.clone(),
      normalized_result_dir: manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness),
      basis_checkpoint_path: manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: manifest.holdout_screenshot_path.clone(),
      reference_overlay_path: manifest.reference_overlay_path.clone(),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      holdout_frame_selection: HoldoutFrameSelection::LastInGame,
      checkpoint_count: 1,
      scene_packet_frame_count: 6,
      warnings: vec![],
      known_limits: vec!["holdout inspect only".to_string()],
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_ROLE,
        "minecraft-3dgs-training-result-holdout-preview.json",
        &manifest,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_HOLDOUT_PREVIEW_INSPECT_ROLE,
        "minecraft-3dgs-training-result-holdout-preview-inspect.json",
        &inspect_report,
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

    let canonical = store.read_run("run_read_mc16_holdout_manifest").expect("run should read back");
    let extracted = extract_minecraft_training_result_holdout_preview_manifests(&store, &canonical).expect("extract holdout manifests");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary");
    assert_eq!(summary.status, "ready");
    assert_eq!(summary.holdout_frame_index, 6);
    assert_eq!(summary.basis_checkpoint_path.as_deref(), Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt"));

    let reports = extract_minecraft_training_result_holdout_preview_inspect_reports(&store, &canonical).expect("extract holdout inspect");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].report.as_ref().unwrap().holdout_frame_selection, "last_in_game");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_holdout_render_quality_manifest_lineage_reads_summary() {
    use auv_game_minecraft::{
      HoldoutFrameWitness, HoldoutPreviewStatus, HoldoutRenderQualityBackend, HoldoutRenderQualityMetrics, HoldoutRenderQualityStatus,
      HoldoutRenderQualityVerdict, TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityInspectReport,
      TrainingResultHoldoutRenderQualityManifest,
    };
    let root = temp_dir("run-read-mc17-holdout-quality-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc17_holdout_quality_manifest");
    let span = dummy_span(&run.root_span_id);
    let witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let holdout_preview_manifest = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/normalized".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: Some("/tmp/normalized/nerfstudio_models/step-000001.ckpt".to_string()),
      holdout_screenshot_path: Some(witness.screenshot_path.clone()),
      reference_overlay_path: Some("/tmp/holdout/holdout_overlay_frame_000006.png".to_string()),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec!["holdout preview only".to_string()],
    };
    let manifest = TrainingResultHoldoutRenderQualityManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path: holdout_preview_manifest.training_result_semantic_manifest_path.clone(),
      holdout_preview_manifest_path: "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: holdout_preview_manifest.source_training_result_artifact_manifest_path.clone(),
      source_training_result_manifest_path: holdout_preview_manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: holdout_preview_manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: holdout_preview_manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: holdout_preview_manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: holdout_preview_manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: holdout_preview_manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: holdout_preview_manifest.source_run_ids.clone(),
      trainer_backend: holdout_preview_manifest.trainer_backend.clone(),
      job_backend: holdout_preview_manifest.job_backend.clone(),
      normalized_result_dir: holdout_preview_manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness.clone()),
      basis_checkpoint_path: holdout_preview_manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: holdout_preview_manifest.holdout_screenshot_path.clone(),
      rendered_image_path: Some("/tmp/holdout/rendered_frame_000006.png".to_string()),
      render_backend: HoldoutRenderQualityBackend::ExternalCommand,
      image_size_match: true,
      source_image_size: None,
      rendered_image_size: None,
      metrics: Some(HoldoutRenderQualityMetrics {
        l1_mean: Some(0.01),
        mse: Some(0.002),
        psnr: Some(27.0),
        ssim: None,
      }),
      status: HoldoutRenderQualityStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      known_limits: vec!["render quality evidence only".to_string()],
    };
    let inspect_report = TrainingResultHoldoutRenderQualityInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_holdout_render_quality_manifest_path: "/tmp/holdout/minecraft-3dgs-holdout-render-quality.json".to_string(),
      training_result_semantic_manifest_path: manifest.training_result_semantic_manifest_path.clone(),
      holdout_preview_manifest_path: manifest.holdout_preview_manifest_path.clone(),
      source_training_result_artifact_manifest_path: manifest.source_training_result_artifact_manifest_path.clone(),
      source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: manifest.source_run_ids.clone(),
      trainer_backend: manifest.trainer_backend.clone(),
      job_backend: manifest.job_backend.clone(),
      normalized_result_dir: manifest.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(witness),
      basis_checkpoint_path: manifest.basis_checkpoint_path.clone(),
      holdout_screenshot_path: manifest.holdout_screenshot_path.clone(),
      rendered_image_path: manifest.rendered_image_path.clone(),
      render_backend: manifest.render_backend.clone(),
      image_size_match: true,
      l1_mean_available: true,
      mse_available: true,
      psnr_available: true,
      ssim_available: false,
      metrics: manifest.metrics.clone(),
      status: HoldoutRenderQualityStatus::Ready,
      reason: None,
      verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      warnings: vec![],
      known_limits: vec!["render quality inspect only".to_string()],
    };

    let artifacts = vec![
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        0,
        crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_ROLE,
        "minecraft-3dgs-holdout-render-quality.json",
        &manifest,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        crate::minecraft::MINECRAFT_3DGS_HOLDOUT_RENDER_QUALITY_INSPECT_ROLE,
        "minecraft-3dgs-holdout-render-quality-inspect.json",
        &inspect_report,
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

    let canonical = store.read_run("run_read_mc17_holdout_quality_manifest").expect("run should read back");
    let extracted =
      extract_minecraft_holdout_render_quality_manifests(&store, &canonical).expect("extract holdout render quality manifests");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary");
    assert_eq!(summary.status, "ready");
    assert_eq!(summary.verdict, "measured_only");
    assert_eq!(summary.image_size_match, true);
    assert_eq!(summary.metrics.as_ref().unwrap().l1_mean, Some(0.01));

    let reports =
      extract_minecraft_holdout_render_quality_inspect_reports(&store, &canonical).expect("extract holdout render quality inspect");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].report.as_ref().unwrap().holdout_frame_index, 6);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_spatial_query_manifest_lineage_reads_summary() {
    use auv_driver::geometry::Point;
    use auv_game_minecraft::{
      BlockPosition, TrainingResultSpatialQueryBackend, TrainingResultSpatialQueryComparisonVerdict, TrainingResultSpatialQueryKind,
      TrainingResultSpatialQueryManifest, TrainingResultSpatialQueryStatus,
    };
    let root = temp_dir("run-read-mc13-query-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/d11/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/d7/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/d6/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/d5/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      query_kind: TrainingResultSpatialQueryKind::BlockProjection,
      target_block: BlockPosition::new(511, 73, 728),
      target_face: None,
      target_semantics: auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter,
      selected_backend: Some(TrainingResultSpatialQueryBackend::ProjectionReference),
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      visibility: Some(auv_game_minecraft::ProjectionVisibility::Visible),
      screen_point: Some(Point { x: 854.0, y: 480.0 }),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some(TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly),
      known_limits: vec!["projection_reference only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted =
      extract_minecraft_training_result_spatial_query_manifests(&store, &store.read_run("run_read_mc13_query_manifest").expect("run"))
        .expect("manifest should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should be present");
    assert_eq!(summary.status, "answered");
    assert_eq!(summary.target_block, "511,73,728");
    assert_eq!(summary.selected_backend.as_deref(), Some("projection_reference"));
    assert_eq!(summary.visibility.as_deref(), Some("visible"));
    assert!(summary.screen_point.is_some());
    assert_eq!(list_minecraft_training_result_spatial_query_manifests(&store, "run_read_mc13_query_manifest").expect("list").len(), 1);

    let readiness = derive_minecraft_training_result_spatial_query_action_readiness(&extracted[0]);
    assert_eq!(readiness.action_eligibility, "click_ready");
    assert!(readiness.window_point.is_some());
    assert!(readiness.refusal_reason.is_none());
    assert!(readiness.issue.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_visual_truth_spatial_query_action_readiness_three_states() {
    use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

    fn lineage(
      artifact_id: &str,
      summary: super::OsuVisualTruthSpatialQueryManifestSummary,
    ) -> super::OsuVisualTruthSpatialQueryManifestLineage {
      super::OsuVisualTruthSpatialQueryManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: RunId::new("run_osu_readiness"),
          artifact_id: ArtifactId::new(artifact_id),
          span_id: SpanId::new("span_osu_readiness"),
          captured_event_id: None,
          role: Some(crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE.to_string()),
          path: Some(format!("artifacts/{artifact_id}.json")),
          summary: Some("osu spatial query manifest".to_string()),
          resolved: true,
        },
        manifest: Some(summary),
        issue: None,
      }
    }

    fn base_summary() -> super::OsuVisualTruthSpatialQueryManifestSummary {
      super::OsuVisualTruthSpatialQueryManifestSummary {
        schema_version: 1,
        visual_truth_semantic_manifest_path: "/tmp/semantic.json".to_string(),
        source_run_artifact_dir: "/tmp/run".to_string(),
        object_index: 0,
        capture_phase: "before_dispatch".to_string(),
        object_kind: Some("circle".to_string()),
        query_backend: "playfield_projection_reference".to_string(),
        status: "answered".to_string(),
        reason: None,
        pixel_visibility: Some("inside_capture".to_string()),
        pixel_x: Some(400.0),
        pixel_y: Some(300.0),
        match_radius_px: Some(20.0),
        capture_width: Some(800),
        capture_height: Some(600),
        known_limits: vec![],
      }
    }

    let click_ready = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage("artifact_click_ready", base_summary()));
    assert_eq!(click_ready.action_eligibility, "click_ready");
    assert!(click_ready.pixel_point.is_some());

    let mut outside = base_summary();
    outside.pixel_visibility = Some("outside_capture".to_string());
    let outside_capture = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage("artifact_outside_capture", outside));
    assert_eq!(outside_capture.action_eligibility, "answer_non_clickable");

    let mut failed = base_summary();
    failed.status = "failed".to_string();
    failed.reason = Some("target_absent_from_visual_truth".to_string());
    failed.pixel_visibility = None;
    failed.pixel_x = None;
    failed.pixel_y = None;
    let not_consumable = super::derive_osu_visual_truth_spatial_query_action_readiness(&lineage("artifact_failed", failed));
    assert_eq!(not_consumable.action_eligibility, "not_consumable");
  }

  #[test]
  fn minecraft_training_result_spatial_query_action_readiness_inherits_manifest_issue() {
    use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

    let lineage = MinecraftTrainingResultSpatialQueryManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: RunId::new("run_issue"),
        artifact_id: ArtifactId::new("artifact_issue"),
        span_id: SpanId::new("span_issue"),
        captured_event_id: None,
        role: Some(crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE.to_string()),
        path: Some("artifacts/query.json".to_string()),
        summary: Some("spatial query manifest".to_string()),
        resolved: true,
      },
      manifest: None,
      issue: Some("minecraft training result spatial query manifest mime_type text/plain is not JSON".to_string()),
    };

    let readiness = derive_minecraft_training_result_spatial_query_action_readiness(&lineage);
    assert_eq!(readiness.action_eligibility, "n/a");
    assert!(readiness.window_point.is_none());
    assert!(readiness.issue.as_deref().is_some_and(|issue| issue.contains("mime_type")));
  }

  #[test]
  fn minecraft_training_result_spatial_query_inspect_report_lineage_reads_summary() {
    use auv_game_minecraft::{
      BlockPosition, TrainingResultSpatialQueryBackend, TrainingResultSpatialQueryComparisonVerdict,
      TrainingResultSpatialQueryInspectReport, TrainingResultSpatialQueryKind, TrainingResultSpatialQueryManifest,
      TrainingResultSpatialQueryStatus,
    };
    let root = temp_dir("run-read-mc13-query-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_inspect");
    let span = dummy_span(&run.root_span_id);

    let shared_manifest = TrainingResultSpatialQueryManifest {
      schema_version: 1,
      generated_at_millis: 99,
      training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/d11/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/d7/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/d6/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/d5/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run-a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir: "/tmp/result/normalized-result".to_string(),
      query_kind: TrainingResultSpatialQueryKind::BlockProjection,
      target_block: BlockPosition::new(511, 73, 728),
      target_face: None,
      target_semantics: auv_game_minecraft::MinecraftTargetSemantics::HitFaceCenter,
      selected_backend: Some(TrainingResultSpatialQueryBackend::ProjectionReference),
      status: TrainingResultSpatialQueryStatus::Answered,
      reason: None,
      visibility: Some(auv_game_minecraft::ProjectionVisibility::Visible),
      screen_point: None,
      match_radius_px: None,
      confidence: None,
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some(TrainingResultSpatialQueryComparisonVerdict::ReferenceOnly),
      known_limits: vec!["fixture".to_string()],
    };

    let inspect = TrainingResultSpatialQueryInspectReport {
      schema_version: shared_manifest.schema_version,
      generated_at_millis: shared_manifest.generated_at_millis,
      training_result_spatial_query_manifest_path: "/tmp/query/minecraft-3dgs-training-result-query.json".to_string(),
      training_result_semantic_manifest_path: shared_manifest.training_result_semantic_manifest_path.clone(),
      source_training_result_artifact_manifest_path: shared_manifest.source_training_result_artifact_manifest_path.clone(),
      source_training_result_manifest_path: shared_manifest.source_training_result_manifest_path.clone(),
      source_training_job_manifest_path: shared_manifest.source_training_job_manifest_path.clone(),
      source_training_launch_plan_path: shared_manifest.source_training_launch_plan_path.clone(),
      source_training_package_manifest_path: shared_manifest.source_training_package_manifest_path.clone(),
      source_scene_packet_manifest_path: shared_manifest.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: shared_manifest.source_bundle_manifest_paths.clone(),
      source_run_ids: shared_manifest.source_run_ids.clone(),
      trainer_backend: shared_manifest.trainer_backend.clone(),
      job_backend: shared_manifest.job_backend.clone(),
      normalized_result_dir: shared_manifest.normalized_result_dir.clone(),
      query_kind: shared_manifest.query_kind,
      target_block: shared_manifest.target_block,
      target_face: shared_manifest.target_face,
      target_semantics: shared_manifest.target_semantics,
      selected_backend: shared_manifest.selected_backend,
      status: shared_manifest.status,
      reason: shared_manifest.reason,
      visibility: shared_manifest.visibility,
      screen_point: shared_manifest.screen_point,
      match_radius_px: shared_manifest.match_radius_px,
      confidence: shared_manifest.confidence,
      basis_frame_id: shared_manifest.basis_frame_id.clone(),
      comparison_verdict: shared_manifest.comparison_verdict,
      provider_status: TrainingResultSpatialQueryStatus::Blocked,
      provider_reason: None,
      provider_message: None,
      reference_status: TrainingResultSpatialQueryStatus::Answered,
      reference_reason: None,
      reference_basis_frame_id: Some("frame-355416".to_string()),
      reference_source_frame_json_path: Some("/tmp/scene-packet/frames/frame_000001.json".to_string()),
      reference_screenshot_path: None,
      scene_packet_frame_count: 12,
      warnings: vec!["provider not configured".to_string()],
      known_limits: shared_manifest.known_limits.clone(),
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
      "minecraft-3dgs-training-result-query-inspect.json",
      &inspect,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted =
      list_minecraft_training_result_spatial_query_inspect_reports(&store, "run_read_mc13_query_inspect").expect("inspect should list");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should be present");
    assert_eq!(summary.provider_status, "blocked");
    assert_eq!(summary.reference_status, "answered");
    assert_eq!(summary.scene_packet_frame_count, 12);
    assert_eq!(summary.target_block, "511,73,728");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_spatial_query_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-mc13-query-manifest-mime");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_manifest_mime");
    let span = dummy_span(&run.root_span_id);

    let mut manifest_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      "{}",
    );
    manifest_artifact.mime_type = "text/plain".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![manifest_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_spatial_query_manifests(&store, "run_read_mc13_query_manifest_mime")
      .expect("manifest lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("mime_type"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_spatial_query_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-mc13-query-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_mc13_query_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let mut inspect_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_INSPECT_ROLE,
      "minecraft-3dgs-training-result-query-inspect.json",
      "{ malformed",
    );
    inspect_artifact.mime_type = "application/json".to_string();

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![inspect_artifact],
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_spatial_query_inspect_reports(&store, "run_read_mc13_query_inspect_malformed")
      .expect("report lineage should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn detector_recognition_lineage_extracts_ready_and_error_states() {
    let root = temp_dir("run-read-detector-recognition");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_detector_recognition");
    let span = dummy_span(&run.root_span_id);

    let capture_artifact =
      stage_text_artifact(&store, &root, &run.run_id, &span.span_id, 0, "capture-image", "capture.png", "fake capture body");
    let ready_recognition = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      "recognition_ready",
    );
    let missing_capture = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      None,
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }],
      "recognition_missing_capture",
    );
    let missing_evidence = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: capture_artifact.artifact_id.clone(),
        span_id: span.span_id.clone(),
        captured_event_id: capture_artifact.event_id.clone(),
      }),
      Vec::new(),
      "recognition_missing_evidence",
    );
    let unresolved_capture = detector_recognition_result(
      &run.run_id,
      &span.span_id,
      Some(ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }),
      vec![ArtifactRef {
        run_id: run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_missing_capture"),
        span_id: span.span_id.clone(),
        captured_event_id: Some(EventId::new("event_missing_capture")),
      }],
      "recognition_unresolved_capture",
    );

    let artifacts = vec![
      capture_artifact,
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        1,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-ready.json",
        &ready_recognition,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        2,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-missing-capture.json",
        &missing_capture,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        3,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-missing-evidence.json",
        &missing_evidence,
      ),
      stage_json_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        4,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-unresolved-capture.json",
        &unresolved_capture,
      ),
      stage_text_artifact(
        &store,
        &root,
        &run.run_id,
        &span.span_id,
        5,
        DETECTOR_RECOGNITION_ARTIFACT_ROLE,
        "detector-malformed.json",
        "{ not valid json",
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

    let canonical = store.read_run("run_read_detector_recognition").expect("run should read back");
    let extracted = extract_detector_recognition_lineage(&store, &canonical).expect("detector recognition lineage should extract");
    assert_eq!(extracted.len(), 5);
    assert_eq!(extracted[0].status, DetectorRecognitionLineageStatus::Ready);
    assert_eq!(extracted[0].backend.as_deref(), Some("ultralytics-inference"));
    assert_eq!(extracted[0].model_id.as_deref(), Some("games-balatro-ui"));
    assert_eq!(extracted[0].all_count, Some(2));
    assert_eq!(extracted[0].filtered_count, Some(1));
    assert_eq!(extracted[0].capture_artifact.as_ref().and_then(|artifact| artifact.role.as_deref()), Some("capture-image"));
    assert_eq!(extracted[1].status, DetectorRecognitionLineageStatus::MissingCaptureArtifact);
    assert_eq!(extracted[2].status, DetectorRecognitionLineageStatus::MissingEvidence);
    assert_eq!(extracted[3].status, DetectorRecognitionLineageStatus::CaptureArtifactUnresolved);
    assert_eq!(extracted[4].status, DetectorRecognitionLineageStatus::Malformed);
    assert!(extracted[4].issue.as_deref().unwrap_or_default().contains("failed to parse detector-recognition artifact"));

    let listed =
      list_detector_recognition_lineage(&store, "run_read_detector_recognition").expect("detector recognition lineage should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_spatial_bundle_manifest_lineage_reads_summary_without_artifact_payload() {
    let root = temp_dir("run-read-minecraft-spatial-bundle");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_spatial_bundle");
    let span = dummy_span(&run.root_span_id);

    let bundle_manifest = MinecraftSpatialBundleManifestSummary {
      schema_version: 1,
      source_run: SourceRunSummary {
        source_run_id: "source_run_1".to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: SpatialBundleCounts {
        screenshots: 2,
        spatial_frames: 3,
        actions: 4,
        verification: 5,
        overlays: 6,
        skipped: 7,
      },
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
      "bundle-run.json",
      &json!({
        "schema_version": bundle_manifest.schema_version,
        "source_run": bundle_manifest.source_run,
        "counts": bundle_manifest.counts,
        "artifacts": [
          {
            "artifact_id": "artifact_0001",
            "role": "minecraft-spatial-frame",
            "source_path": "artifacts/frame.json",
            "bundle_path": "spatial_frames/artifact_0001-frame.json",
            "directory": "spatial_frames",
            "summary": "big payload should be ignored by read-side summary"
          }
        ],
        "known_limits": ["fixture"]
      }),
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let listed =
      list_minecraft_spatial_bundle_manifests(&store, "run_read_minecraft_spatial_bundle").expect("spatial bundle manifests should list");
    assert_eq!(listed.len(), 1);
    let manifest = listed[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(manifest.source_run.source_run_id, "source_run_1");
    assert_eq!(manifest.counts.spatial_frames, 3);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-package-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingPackageManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec![
        "/tmp/bundle-a/run.json".to_string(),
        "/tmp/bundle-b/run.json".to_string(),
      ],
      source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
      counts: TrainingPackageCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      frames: Vec::new(),
      compatibility_views: vec![TrainingCompatibilityViewReport {
        view_name: "nerfstudio".to_string(),
        status: TrainingCompatibilityStatus::Partial,
        exported_frame_count: 4,
        skipped_frame_count: 2,
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        exported_frame_indices: vec![1, 2, 3, 4],
        frame_decisions: Vec::new(),
        skip_reason_counts: Vec::new(),
        warnings: vec!["missing screenshot on frame 6".to_string()],
        used_legacy_view_translation_fallback_frame_indices: vec![2],
        known_limits: vec!["legacy translation fallback used".to_string()],
      }],
      known_limits: vec!["canonical package only; no trainer output".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-run.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_package_manifests(
      &store,
      &store.read_run("run_read_minecraft_training_package_manifest").expect("run should read back"),
    )
    .expect("training package manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(summary.source_scene_packet_manifest_path, "/tmp/scene-packet/run.json");
    assert_eq!(summary.counts.frames, 6);
    assert_eq!(summary.compatibility_views[0].view_name, "nerfstudio");

    let listed = list_minecraft_training_package_manifests(&store, "run_read_minecraft_training_package_manifest")
      .expect("training package manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-package-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingPackageInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_package_manifest_path: "/tmp/package/run.json".to_string(),
      scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      counts: TrainingPackageCounts {
        frames: 3,
        images: 2,
        compatibility_exported_frames: 2,
        compatibility_skipped_frames: 1,
      },
      compatibility_views: vec![TrainingCompatibilityViewReport {
        view_name: "nerfstudio".to_string(),
        status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 2,
        skipped_frame_count: 1,
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        exported_frame_indices: vec![1, 2],
        frame_decisions: Vec::new(),
        skip_reason_counts: Vec::new(),
        warnings: Vec::new(),
        used_legacy_view_translation_fallback_frame_indices: Vec::new(),
        known_limits: vec!["manual smoke required".to_string()],
      }],
      warnings: vec!["frame 3 skipped: missing_screenshot".to_string()],
      known_limits: vec!["synthetic validation only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_package_inspect_reports(
      &store,
      &store.read_run("run_read_minecraft_training_package_inspect").expect("run should read back"),
    )
    .expect("training package inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.training_package_manifest_path, "/tmp/package/run.json");
    assert_eq!(summary.counts.compatibility_exported_frames, 2);
    assert_eq!(summary.warnings.len(), 1);

    let listed = list_minecraft_training_package_inspect_reports(&store, "run_read_minecraft_training_package_inspect")
      .expect("training package inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-package-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_package_manifests(&store, "run_read_minecraft_training_package_manifest_non_json")
      .expect("training package manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("mime_type"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_package_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-package-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_package_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-package-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_package_inspect_reports(&store, "run_read_minecraft_training_package_inspect_malformed")
      .expect("training package inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse minecraft-3dgs-training-package-inspect artifact"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-launch-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingLaunchPlanManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec![
        "/tmp/bundle-a/run.json".to_string(),
        "/tmp/bundle-b/run.json".to_string(),
      ],
      source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
      counts: TrainingPackageCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: "/tmp/output/trainer-output/nerfstudio-splatfacto".to_string(),
      launch_command:
        "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/output/trainer-output/nerfstudio-splatfacto".to_string(),
      known_limits: vec!["launch prep only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-plan.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_launch_manifests(
      &store,
      &store.read_run("run_read_minecraft_training_launch_manifest").expect("run should read back"),
    )
    .expect("training launch manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(summary.source_training_package_manifest_path, "/tmp/package/run.json");
    assert_eq!(summary.counts.frames, 6);
    assert_eq!(summary.compatibility_view_name, "nerfstudio");
    assert_eq!(summary.trainer_backend, "nerfstudio.splatfacto");

    let listed = list_minecraft_training_launch_manifests(&store, "run_read_minecraft_training_launch_manifest")
      .expect("training launch manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-launch-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingLaunchInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_launch_manifest_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      compatibility_status: TrainingCompatibilityStatus::Partial,
      trainer_readiness: TrainingLaunchReadiness::Blocked,
      readiness_blocker: Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable),
      probe_command: "ns-train --help".to_string(),
      probe_succeeded: false,
      exported_frame_count: 2,
      skipped_frame_count: 1,
      transforms_present: true,
      warnings: vec!["ns-train unavailable".to_string()],
      known_limits: vec!["synthetic validation only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_launch_inspect_reports(
      &store,
      &store.read_run("run_read_minecraft_training_launch_inspect").expect("run should read back"),
    )
    .expect("training launch inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.training_launch_manifest_path, "/tmp/launch/minecraft-3dgs-training-launch-plan.json");
    assert_eq!(summary.compatibility_status, "Partial");
    assert_eq!(summary.trainer_readiness, "Blocked");
    assert_eq!(summary.readiness_blocker.as_deref(), Some("TrainerCommandUnavailable"));

    let listed = list_minecraft_training_launch_inspect_reports(&store, "run_read_minecraft_training_launch_inspect")
      .expect("training launch inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-job-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingLaunchJobManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      counts: TrainingLaunchJobCounts {
        frames: 6,
        images: 6,
        compatibility_exported_frames: 4,
        compatibility_skipped_frames: 2,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      provider_backend: "remote-command-provider".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      job_submission_command: "submit-training-job".to_string(),
      submission_recorded_at_millis: Some(1),
      accepted_by_provider: true,
      training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      launch_command: "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/job/trainer-output/nerfstudio-splatfacto"
        .to_string(),
      status: TrainingLaunchJobStatus::Submitted,
      job_id: Some("job-123".to_string()),
      job_url: Some("https://jobs.example/job-123".to_string()),
      readiness_blocker: None,
      known_limits: vec!["remote submission only".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_job_manifests(
      &store,
      &store.read_run("run_read_minecraft_training_job_manifest").expect("run should read back"),
    )
    .expect("training job manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(summary.source_training_launch_plan_path, "/tmp/launch/minecraft-3dgs-training-launch-plan.json");
    assert_eq!(summary.status, "submitted");
    assert_eq!(summary.job_backend, "remote");
    assert_eq!(summary.counts.compatibility_exported_frames, 4);

    let listed =
      list_minecraft_training_job_manifests(&store, "run_read_minecraft_training_job_manifest").expect("training job manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-job-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingLaunchJobInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_launch_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      provider_backend: "remote-command-provider".to_string(),
      job_backend: "remote".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      job_submission_command: "submit-training-job".to_string(),
      submission_recorded_at_millis: None,
      accepted_by_provider: false,
      status: TrainingLaunchJobStatus::Blocked,
      job_id: None,
      job_url: None,
      readiness_blocker: Some(TrainingLaunchJobBlocker::MissingAuthentication),
      probe_command: "submit-training-job --help".to_string(),
      probe_succeeded: true,
      exported_frame_count: 4,
      skipped_frame_count: 2,
      transforms_present: true,
      warnings: vec!["token missing".to_string()],
      known_limits: vec!["job execution not consumed here".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_job_inspect_reports(
      &store,
      &store.read_run("run_read_minecraft_training_job_inspect").expect("run should read back"),
    )
    .expect("training job inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.status, "blocked");
    assert_eq!(summary.readiness_blocker.as_deref(), Some("MissingAuthentication"));
    assert_eq!(summary.exported_frame_count, 4);

    let listed = list_minecraft_training_job_inspect_reports(&store, "run_read_minecraft_training_job_inspect")
      .expect("training job inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-job-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_job_manifests(&store, "run_read_minecraft_training_job_manifest_non_json")
      .expect("training job manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("mime_type"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_job_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-job-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_job_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_JOB_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-job-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_job_inspect_reports(&store, "run_read_minecraft_training_job_inspect_malformed")
      .expect("training job inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse minecraft-3dgs-training-job-inspect artifact"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_manifest_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-result-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_manifest");
    let span = dummy_span(&run.root_span_id);

    let manifest = TrainingResultManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      source_job_status: TrainingLaunchJobStatus::Submitted,
      status: TrainingResultStatus::Succeeded,
      status_message: Some("provider succeeded".to_string()),
      job_id: "job-123".to_string(),
      job_url: Some("https://jobs.example/job-123".to_string()),
      result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      exported_frame_count: 4,
      skipped_frame_count: 2,
      result_artifacts: vec![TrainingResultArtifactRecord {
        relative_path: "config.yml".to_string(),
        absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/config.yml".to_string(),
        readable: true,
        byte_size: Some(128),
      }],
      known_limits: vec!["quality not graded".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result.json",
      &manifest,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_result_manifests(
      &store,
      &store.read_run("run_read_minecraft_training_result_manifest").expect("run should read back"),
    )
    .expect("training result manifests should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].manifest.as_ref().expect("summary should parse");
    assert_eq!(summary.status, "succeeded");
    assert_eq!(summary.status_message.as_deref(), Some("provider succeeded"));
    assert_eq!(summary.source_job_status, "submitted");
    assert_eq!(summary.result_artifacts.len(), 1);

    let listed = list_minecraft_training_result_manifests(&store, "run_read_minecraft_training_result_manifest")
      .expect("training result manifests should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_inspect_report_lineage_reads_summary() {
    let root = temp_dir("run-read-minecraft-training-result-inspect");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_inspect");
    let span = dummy_span(&run.root_span_id);

    let report = TrainingResultInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example/api".to_string(),
      source_job_status: TrainingLaunchJobStatus::Submitted,
      status: TrainingResultStatus::Failed,
      status_message: Some("legacy adapter failure".to_string()),
      status_reason: Some(TrainingResultReason::ResultArtifactsMissing),
      job_id: "job-123".to_string(),
      job_url: Some("https://jobs.example/job-123".to_string()),
      result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
      result_dir_exists: true,
      key_result_artifacts_present: false,
      result_artifact_count: 0,
      warnings: vec!["models directory missing".to_string()],
      known_limits: vec!["quality not graded".to_string()],
    };

    let artifacts = vec![stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result-inspect.json",
      &report,
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = extract_minecraft_training_result_inspect_reports(
      &store,
      &store.read_run("run_read_minecraft_training_result_inspect").expect("run should read back"),
    )
    .expect("training result inspect reports should extract");
    assert_eq!(extracted.len(), 1);
    let summary = extracted[0].report.as_ref().expect("summary should parse");
    assert_eq!(summary.status, "failed");
    assert_eq!(summary.status_message.as_deref(), Some("legacy adapter failure"));
    assert_eq!(summary.status_reason.as_deref(), Some("result_artifacts_missing"));
    assert!(!summary.key_result_artifacts_present);

    let listed = list_minecraft_training_result_inspect_reports(&store, "run_read_minecraft_training_result_inspect")
      .expect("training result inspect reports should list");
    assert_eq!(listed, extracted);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-result-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_manifests(&store, "run_read_minecraft_training_result_manifest_non_json")
      .expect("training result manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("mime_type"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_manifest_lineage_reports_non_json_mime() {
    let root = temp_dir("run-read-minecraft-training-launch-manifest-non-json");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_manifest_non_json");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_PLAN_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-plan.txt",
      "plain text payload",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_launch_manifests(&store, "run_read_minecraft_training_launch_manifest_non_json")
      .expect("training launch manifests should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].manifest.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("mime_type"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_launch_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-launch-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_launch_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_LAUNCH_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-launch-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_launch_inspect_reports(&store, "run_read_minecraft_training_launch_inspect_malformed")
      .expect("training launch inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse minecraft-3dgs-training-launch-inspect artifact"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_training_result_inspect_report_lineage_reports_parse_failure() {
    let root = temp_dir("run-read-minecraft-training-result-inspect-malformed");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run = dummy_run("run_read_minecraft_training_result_inspect_malformed");
    let span = dummy_span(&run.root_span_id);

    let artifacts = vec![stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span.span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_INSPECT_ARTIFACT_ROLE,
      "minecraft-3dgs-training-result-inspect.json",
      "{ not valid json",
    )];

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts,
      })
      .expect("run snapshot should persist");

    let extracted = list_minecraft_training_result_inspect_reports(&store, "run_read_minecraft_training_result_inspect_malformed")
      .expect("training result inspect reports should list");
    assert_eq!(extracted.len(), 1);
    assert!(extracted[0].report.is_none());
    assert!(extracted[0].issue.as_deref().unwrap_or_default().contains("failed to parse minecraft-3dgs-training-result-inspect artifact"));

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

  fn verification(method: VerificationMethod, observed_label: Option<String>) -> VerificationResult {
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
    let rendered = serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
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

  fn stage_text_artifact(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    content: &str,
  ) -> ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    fs::write(&source_path, content).expect("artifact source should write");
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

  fn detector_recognition_result(
    run_id: &RunId,
    span_id: &SpanId,
    capture_artifact: Option<ArtifactRef>,
    evidence: Vec<ArtifactRef>,
    recognition_id: &str,
  ) -> RecognitionResult {
    RecognitionResult {
      recognition_id: recognition_id.to_string(),
      source: RecognitionSource::Custom,
      scope: RecognitionScope {
        surface: RecognitionSurface::Region,
        display_ref: Some("display-main".to_string()),
        native_display_id: Some("69733248".to_string()),
        app_bundle_id: Some("com.playstack.balatro".to_string()),
        window_title: Some("Balatro".to_string()),
        window_number: Some(7),
        region_hint: None,
        capture_artifact,
        capture_contract_artifact: Some(ArtifactRef {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: span_id.clone(),
          captured_event_id: Some(EventId::new("event_contract")),
        }),
      },
      best: None,
      filtered: vec![RecognizedItem {
        item_id: "detector:games-balatro-ui:0".to_string(),
        kind: "ui_button_play".to_string(),
        box_: crate::contract::RecognitionBox {
          x: 10,
          y: 20,
          width: 30,
          height: 40,
        },
        text: None,
        provider_score: Some(0.98),
        detail: json!({}),
      }],
      all: vec![
        RecognizedItem {
          item_id: "detector:games-balatro-ui:0".to_string(),
          kind: "ui_button_play".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
          },
          text: None,
          provider_score: Some(0.98),
          detail: json!({}),
        },
        RecognizedItem {
          item_id: "detector:games-balatro-ui:1".to_string(),
          kind: "ui_score".to_string(),
          box_: crate::contract::RecognitionBox {
            x: 50,
            y: 60,
            width: 70,
            height: 80,
          },
          text: None,
          provider_score: Some(0.87),
          detail: json!({}),
        },
      ],
      detail: json!({
        "backend": "ultralytics-inference",
        "model_id": "games-balatro-ui",
        "execution_provider": "cpu",
        "class_label_source": { "kind": "override_file" },
        "runtime_projection": { "kind": "identity_source_image_pixels" },
      }),
      evidence,
      known_limits: vec![
        "projection basis is unavailable outside capture-integrated runtime".to_string(),
        "detector RecognitionResult is recognition evidence only, not candidate-ready output".to_string(),
      ],
    }
  }

  fn mc19_query_manifest_json(
    target_block: (i32, i32, i32),
    status: &str,
    visibility: Option<&str>,
    screen_point: Option<serde_json::Value>,
    reason: Option<&str>,
    selected_backend: Option<&str>,
  ) -> serde_json::Value {
    json!({
      "schema_version": 1,
      "generated_at_millis": 1,
      "training_result_semantic_manifest_path": "/tmp/semantic.json",
      "source_training_result_artifact_manifest_path": "/tmp/artifact.json",
      "source_training_result_manifest_path": "/tmp/result.json",
      "source_training_job_manifest_path": "/tmp/job.json",
      "source_training_launch_plan_path": "/tmp/launch.json",
      "source_training_package_manifest_path": "/tmp/package.json",
      "source_scene_packet_manifest_path": "/tmp/scene-packet.json",
      "source_bundle_manifest_paths": ["/tmp/bundle.json"],
      "source_run_ids": ["run-a"],
      "trainer_backend": "nerfstudio.splatfacto",
      "job_backend": "remote",
      "normalized_result_dir": "/tmp/normalized",
      "query_kind": "block_projection",
      "target_block": {"x": target_block.0, "y": target_block.1, "z": target_block.2},
      "target_face": null,
      "target_semantics": "hit_face_center",
      "selected_backend": selected_backend,
      "status": status,
      "reason": reason,
      "visibility": visibility,
      "screen_point": screen_point,
      "match_radius_px": 8.0,
      "confidence": 0.9,
      "basis_frame_id": "frame-1",
      "comparison_verdict": "reference_only",
      "known_limits": []
    })
  }

  fn mc19_operation_result(run_id: &RunId, query_artifact_id: &str, status: OperationStatus, message: &str) -> OperationResult {
    let query_ref = ArtifactRef {
      artifact_id: ArtifactId::new(query_artifact_id),
      run_id: run_id.clone(),
      span_id: SpanId::new("0000000000000001"),
      captured_event_id: None,
    };
    OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run_id.clone(),
      status,
      operation_id: crate::minecraft_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID.to_string(),
      evidence_artifacts: vec![query_ref.clone()],
      output: OperationOutput::Acknowledged {
        message: Some(message.to_string()),
      },
      verifications: Vec::new(),
      freshness_basis: Some(crate::contract::FreshnessBasis {
        source_artifact: Some(query_ref),
        source_operation_id: Some("auv.minecraft.query_3dgs_training_result".to_string()),
        notes: vec!["MC-12 spatial query manifest staged in the same run".to_string()],
      }),
      known_limits: vec!["mc19_v1_d4_query_wired_live_action_non_stub_click_no_gameplay_verification".to_string()],
    }
  }

  fn dummy_mc19_event(span_id: &SpanId, name: &str, message: &str) -> auv_tracing_driver::trace::EventRecordV1Alpha1 {
    auv_tracing_driver::trace::EventRecordV1Alpha1 {
      api_version: auv_tracing_driver::trace::EVENT_API_VERSION.to_string(),
      event_id: EventId::new(format!("event_{name}")),
      span_id: span_id.clone(),
      name: name.to_string(),
      timestamp_millis: 150,
      attributes: BTreeMap::new(),
      message: Some(message.to_string()),
      artifact_ids: Vec::new(),
    }
  }

  fn write_mc19_run_snapshot(
    store: &LocalStore,
    root: &Path,
    run_id: &str,
    events: Vec<auv_tracing_driver::trace::EventRecordV1Alpha1>,
    artifacts: Vec<ArtifactRecordV1Alpha1>,
  ) {
    let run = dummy_run(run_id);
    let span = dummy_span(&run.root_span_id);
    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events,
        artifacts,
      })
      .expect("run snapshot should persist");
    let _ = root;
  }

  #[test]
  fn query_wired_live_action_verification_projection_maps_semantic_pass_and_absent() {
    use crate::contract::{OperationStatus, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult};

    let run_id = RunId::new("run_verification_projection");
    let absent = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&mc19_operation_result(&run_id, "artifact_query", OperationStatus::Completed, "dispatched")),
      run_id.as_str(),
      None,
    );
    assert_eq!(absent.verification_outcome, "absent");
    assert!(absent.verification_reason.as_deref().is_some_and(|reason| reason.contains("mc19_v1_d4")));

    let mut operation_result = mc19_operation_result(&run_id, "artifact_query", OperationStatus::Completed, "dispatched");
    operation_result.verifications.push(VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
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
      observed_label: Some("world diff matched".to_string()),
    });
    let passed = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(passed.verification_outcome, "passed");
    let mut unreliable_operation_result = mc19_operation_result(&run_id, "artifact_query", OperationStatus::Completed, "dispatched");
    unreliable_operation_result.verifications.push(VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: false,
      semantic_matched: None,
      failure_layer: Some(crate::contract::FailureLayer::VerificationUnreliable),
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: None,
    });
    unreliable_operation_result.known_limits = vec![auv_game_minecraft::MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT.to_string()];
    let unreliable = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&unreliable_operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(unreliable.verification_outcome, "unreliable");

    assert_eq!(passed.verification_reason.as_deref(), Some("world diff matched"));

    let mut inconclusive_operation_result = mc19_operation_result(&run_id, "artifact_query", OperationStatus::Completed, "dispatched");
    inconclusive_operation_result.verifications.push(VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: None,
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("tick advanced".to_string()),
    });
    let inconclusive = super::resolve_query_wired_live_action_verification_projection(
      true,
      Some("artifact_op"),
      Some(&inconclusive_operation_result),
      run_id.as_str(),
      None,
    );
    assert_eq!(inconclusive.verification_outcome, "inconclusive");
    assert_eq!(inconclusive.verification_reason.as_deref(), Some("tick advanced"));

    let not_attempted =
      super::resolve_query_wired_live_action_verification_projection(false, None, None, run_id.as_str(), Some("visibility=outside_window"));
    assert_eq!(not_attempted.verification_outcome, "not_attempted");
    assert_eq!(not_attempted.verification_reason.as_deref(), Some("visibility=outside_window"));
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_click_ready_gate() {
    let root = temp_dir("run-read-mc19-click-ready");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_click_ready";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json(
        (511, 73, 728),
        "answered",
        Some("visible"),
        Some(json!({"x": 854.0, "y": 480.0})),
        None,
        Some("projection_reference"),
      ),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, query_artifact_id.as_str(), OperationStatus::Completed, "mock live click dispatched"),
    );
    let events = vec![
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.inputs",
        "training_result_semantic_manifest=/tmp/semantic.json target_block=511,73,728 target_app=net.minecraft.client target_title=Minecraft checkpoint_native_provider=false closed_scene_toy_provider=true closed_scene_fixture=/tmp/fixture.json output_dir=/tmp/out",
      ),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=true action_eligibility=click_ready refusal_reason=none query_manifest_path=/tmp/query.json",
      ),
      dummy_mc19_event(&span_id, "command.resolved", "resolved input.clickWindowPoint"),
      dummy_mc19_event(&span_id, "command.failed", "main visible window was not found"),
    ];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(summary.attempted);
    assert_eq!(summary.action_eligibility, "click_ready");
    assert_eq!(summary.readiness_class.as_deref(), Some("ready"));
    assert_eq!(summary.dispatch_command.as_deref(), Some("input.clickWindowPoint"));
    assert!(summary.dispatch_outcome.as_deref().is_some_and(|v| v.starts_with("failed:")));
    assert_eq!(summary.mc14_action_eligibility.as_deref(), Some("click_ready"));
    assert!(summary.window_point.is_some());
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=query_manifest artifact_id={} run_id={}", query_artifact_id.as_str(), run_id).as_str())
    );
    assert_eq!(list_minecraft_query_wired_live_action_summaries(&store, run_id).expect("list").len(), 1);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_answer_non_clickable_gate() {
    let root = temp_dir("run-read-mc19-outside-window");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_outside_window";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json((511, 73, 728), "answered", Some("outside_window"), None, None, Some("command_provider")),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, query_artifact_id.as_str(), OperationStatus::Completed, "visibility=outside_window"),
    );
    let events = vec![
      dummy_mc19_event(&span_id, "minecraft.query_wired_live_action.inputs", "target_app=net.minecraft.client target_title=Minecraft"),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=false action_eligibility=answer_non_clickable refusal_reason=visibility=outside_window query_manifest_path=/tmp/query.json",
      ),
    ];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(!summary.attempted);
    assert_eq!(summary.action_eligibility, "answer_non_clickable");
    assert_eq!(summary.readiness_class.as_deref(), Some("non_actionable"));
    assert_eq!(summary.refusal_reason.as_deref(), Some("visibility=outside_window"));
    assert!(summary.dispatch_command.is_none());
    assert!(summary.dispatch_outcome.is_none());
    assert_eq!(summary.mc14_action_eligibility.as_deref(), Some("answer_non_clickable"));
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=query_manifest artifact_id={} run_id={}", query_artifact_id.as_str(), run_id).as_str())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_not_consumable_gate() {
    let root = temp_dir("run-read-mc19-not-consumable");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_not_consumable";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &mc19_query_manifest_json((9, 9, 9), "failed", None, None, Some("target_block_absent_from_scene_packet"), None),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(
        &run.run_id,
        query_artifact_id.as_str(),
        OperationStatus::Completed,
        "status=failed reason=target_block_absent_from_scene_packet",
      ),
    );
    let events = vec![
      dummy_mc19_event(&span_id, "minecraft.query_wired_live_action.inputs", "target_app=net.minecraft.client target_title=Minecraft"),
      dummy_mc19_event(
        &span_id,
        "minecraft.query_wired_live_action.outcome",
        "attempted=false action_eligibility=not_consumable refusal_reason=status=failed reason=target_block_absent_from_scene_packet query_manifest_path=/tmp/query.json",
      ),
    ];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(!summary.attempted);
    assert_eq!(summary.action_eligibility, "not_consumable");
    assert_eq!(summary.readiness_class.as_deref(), Some("not_consumable"));
    assert_eq!(summary.refusal_reason.as_deref(), Some("status=failed reason=target_block_absent_from_scene_packet"));
    assert!(summary.dispatch_command.is_none());
    assert!(summary.dispatch_outcome.is_none());
    assert_eq!(summary.mc14_action_eligibility.as_deref(), Some("not_consumable"));
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=query_manifest artifact_id={} run_id={}", query_artifact_id.as_str(), run_id).as_str())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_event_only_source_readiness_ref() {
    let root = temp_dir("run-read-mc19-event-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_event_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=answer_non_clickable refusal_reason=visibility=outside_window",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert_eq!(summary.source_readiness_ref.as_deref(), Some("kind=outcome_event event=minecraft.query_wired_live_action.outcome"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_manifest_parse_failure_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-manifest-parse-failure");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_manifest_parse_failure";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      "{not valid json",
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, query_artifact.artifact_id.as_str(), OperationStatus::Completed, "manifest unreadable"),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=manifest unreadable",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());
    assert!(summary.source_readiness_ref.as_deref().is_none_or(|value| !value.contains("kind=derived_readiness")));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_clean_miss_derived_readiness_ref() {
    let root = temp_dir("run-read-mc19-clean-miss");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_clean_miss";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let missing_query_id = "artifact_missing_query_manifest";
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, missing_query_id, OperationStatus::Completed, "query manifest absent from run"),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=query manifest absent from run",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=derived_readiness query_artifact_id={missing_query_id} run_id={run_id}").as_str())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_partial_manifest_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-partial-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_partial_manifest";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &json!({"status": "answered"}),
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, query_artifact.artifact_id.as_str(), OperationStatus::Completed, "partial manifest"),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=partial manifest",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn minecraft_query_wired_live_action_summary_schema_status_only_source_readiness_ref_none() {
    let root = temp_dir("run-read-mc19-schema-status-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_mc19_schema_status_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::minecraft::MINECRAFT_3DGS_TRAINING_RESULT_QUERY_ROLE,
      "minecraft-3dgs-training-result-query.json",
      &json!({"schema_version": 1, "status": "answered"}),
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &mc19_operation_result(&run.run_id, query_artifact.artifact_id.as_str(), OperationStatus::Completed, "schema status only manifest"),
    );
    let events = vec![dummy_mc19_event(
      &span_id,
      "minecraft.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=schema status only manifest",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary =
      derive_minecraft_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  fn osu_query_manifest_json(status: &str, pixel_visibility: Option<&str>) -> serde_json::Value {
    let (pixel_x, pixel_y) = if pixel_visibility == Some("inside_capture") {
      (Some(400.0), Some(300.0))
    } else {
      (None, None)
    };
    json!({
      "schema_version": 1,
      "generated_at_millis": 1,
      "visual_truth_semantic_manifest_path": "/tmp/semantic.json",
      "source_run_artifact_dir": "/tmp/run",
      "source_visual_truth_manifest_path": "/tmp/vt.json",
      "source_projection_path": "/tmp/proj.json",
      "object_index": 0,
      "capture_phase": "before_dispatch",
      "object_kind": "circle",
      "query_backend": "playfield_projection_reference",
      "status": status,
      "pixel_visibility": pixel_visibility,
      "pixel_x": pixel_x,
      "pixel_y": pixel_y,
      "match_radius_px": 20.0,
      "capture_width": 800,
      "capture_height": 600,
      "known_limits": []
    })
  }

  fn osu_operation_result(run_id: &RunId, query_artifact_id: &str, status: OperationStatus, message: &str) -> OperationResult {
    let query_ref = ArtifactRef {
      artifact_id: ArtifactId::new(query_artifact_id),
      run_id: run_id.clone(),
      span_id: SpanId::new("0000000000000001"),
      captured_event_id: None,
    };
    OperationResult {
      api_version: OPERATION_RESULT_API_VERSION.to_string(),
      run_id: run_id.clone(),
      status,
      operation_id: crate::osu_query_live_action::QUERY_WIRED_LIVE_ACTION_OPERATION_ID.to_string(),
      evidence_artifacts: vec![query_ref.clone()],
      output: OperationOutput::Acknowledged {
        message: Some(message.to_string()),
      },
      verifications: Vec::new(),
      freshness_basis: Some(crate::contract::FreshnessBasis {
        source_artifact: Some(query_ref),
        source_operation_id: Some("auv.osu.query_visual_truth_spatial".to_string()),
        notes: vec!["osu visual truth spatial query manifest staged in the same run".to_string()],
      }),
      known_limits: vec!["osu_query_wired_live_action_capture_space_readiness_live_window_dispatch_no_gameplay_verification".to_string()],
    }
  }

  fn dummy_osu_event(span_id: &SpanId, name: &str, message: &str) -> auv_tracing_driver::trace::EventRecordV1Alpha1 {
    dummy_mc19_event(span_id, name, message)
  }

  #[test]
  fn osu_query_wired_live_action_summary_event_only_source_readiness_ref() {
    let root = temp_dir("run-read-osu-event-only");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_event_only";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=answer_non_clickable refusal_reason=pixel_visibility=outside_capture",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![]);

    let summary = derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert_eq!(summary.source_readiness_ref.as_deref(), Some("kind=outcome_event event=osu.query_wired_live_action.outcome"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_manifest_parse_failure_source_readiness_ref_none() {
    let root = temp_dir("run-read-osu-manifest-parse-failure");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_manifest_parse_failure";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_text_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
      "osu-visual-truth-spatial-query.json",
      "{not valid json",
    );
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(&run.run_id, query_artifact.artifact_id.as_str(), OperationStatus::Completed, "manifest unreadable"),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=manifest unreadable",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary = derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert!(summary.source_readiness_ref.is_none());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_clean_miss_derived_readiness_ref() {
    let root = temp_dir("run-read-osu-clean-miss");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_clean_miss";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let missing_query_id = "artifact_missing_osu_query_manifest";
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(&run.run_id, missing_query_id, OperationStatus::Completed, "query manifest absent from run"),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=false action_eligibility=not_consumable refusal_reason=query manifest absent from run",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![operation_result]);

    let summary = derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=derived_readiness query_artifact_id={missing_query_id} run_id={run_id}").as_str())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn osu_query_wired_live_action_summary_query_manifest_source_readiness_ref() {
    let root = temp_dir("run-read-osu-query-manifest");
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = "run_read_osu_query_manifest";
    let run = dummy_run(run_id);
    let span_id = run.root_span_id.clone();
    let query_artifact = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      0,
      crate::osu::OSU_VISUAL_TRUTH_SPATIAL_QUERY_ROLE,
      "osu-visual-truth-spatial-query.json",
      &osu_query_manifest_json("answered", Some("inside_capture")),
    );
    let query_artifact_id = query_artifact.artifact_id.as_str().to_string();
    let operation_result = stage_json_artifact(
      &store,
      &root,
      &run.run_id,
      &span_id,
      1,
      "operation-result",
      "operation-result.json",
      &osu_operation_result(&run.run_id, query_artifact_id.as_str(), OperationStatus::Completed, "mock dispatch"),
    );
    let events = vec![dummy_osu_event(
      &span_id,
      "osu.query_wired_live_action.outcome",
      "attempted=true action_eligibility=click_ready refusal_reason=none pixel_point=400,300",
    )];
    write_mc19_run_snapshot(&store, &root, run_id, events, vec![query_artifact, operation_result]);

    let summary = derive_osu_query_wired_live_action_summary(&store, &store.read_run(run_id).expect("run")).expect("summary should derive");
    assert_eq!(
      summary.source_readiness_ref.as_deref(),
      Some(format!("kind=query_manifest artifact_id={} run_id={}", query_artifact_id.as_str(), run_id).as_str())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn quality_baseline_profile_v1_fixture_loads() {
    let profile = quality_baseline_profile_v1().expect("profile v1 fixture should parse");
    assert_eq!(profile.profile_id, "mc17-d2-primary-v1");
    assert_eq!(profile.query_target_block, "511,73,728");
    assert_eq!(profile.holdout_frame_index, 6);
  }

  #[test]
  fn quality_baseline_derive_complete_when_all_stages_match_profile() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let semantic = profile.training_result_semantic_manifest_path.clone();
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
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
      query_kind: "block_projection".to_string(),
      target_block: profile.query_target_block.clone(),
      target_face: profile.query_target_face.clone(),
      target_semantics: profile.query_target_semantics.clone(),
      selected_backend: Some("projection_reference".to_string()),
      status: "answered".to_string(),
      reason: None,
      visibility: Some("visible".to_string()),
      screen_point: Some("854.0,480.0".to_string()),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some("reference_only".to_string()),
      known_limits: vec![],
    };
    let holdout = MinecraftTrainingResultHoldoutPreviewManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
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
      holdout_frame_index: profile.holdout_frame_index,
      holdout_frame: Some(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
        frame_index: 6,
        spatial_frame_id: "frame-355416".to_string(),
        screenshot_path: "/tmp/frame_000006.png".to_string(),
        frame_json_path: "/tmp/frame_000006.json".to_string(),
      }),
      basis_checkpoint_path: Some(format!("/tmp/normalized/nerfstudio_models/{}", profile.basis_checkpoint_suffix)),
      holdout_screenshot_path: Some("/tmp/frame_000006.png".to_string()),
      reference_overlay_path: None,
      status: "ready".to_string(),
      reason: None,
      known_limits: vec![],
    };
    let render = MinecraftHoldoutRenderQualityManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic,
      holdout_preview_manifest_path: "/tmp/holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_run_ids: vec![],
      holdout_frame_index: profile.holdout_frame_index,
      basis_checkpoint_path: holdout.basis_checkpoint_path.clone(),
      rendered_image_path: Some("/tmp/rendered.png".to_string()),
      image_size_match: true,
      metrics: Some(MinecraftHoldoutRenderQualityMetricsSummary {
        l1_mean: Some(0.0),
        mse: Some(0.0),
        psnr: None,
      }),
      status: "ready".to_string(),
      reason: None,
      verdict: "measured_only".to_string(),
      known_limits: vec!["metrics evidence only".to_string()],
    };

    let report = derive_minecraft_training_result_quality_baseline_report(&profile, Some(&spatial), Some(&holdout), Some(&render), &[]);
    assert_eq!(report.evidence_coverage, "complete");
    assert!(report.issue.is_none());
    assert!(report.spatial_query.is_some());
    assert!(report.holdout_witness.is_some());
    assert!(report.render_quality.is_some());
    assert!(report.trust_notes.iter().any(|note| note.contains("projection_reference")));
  }

  #[test]
  fn quality_baseline_derive_partial_when_stage_missing() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let report = derive_minecraft_training_result_quality_baseline_report(&profile, None, None, None, &[]);
    assert_eq!(report.evidence_coverage, "missing_stage");
  }

  #[test]
  fn quality_baseline_derive_partial_on_profile_mismatch() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: "/tmp/other-semantic.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
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
      query_kind: "block_projection".to_string(),
      target_block: "9,9,9".to_string(),
      target_face: None,
      target_semantics: "hit_face_center".to_string(),
      selected_backend: None,
      status: "failed".to_string(),
      reason: Some("target_block_absent_from_scene_packet".to_string()),
      visibility: None,
      screen_point: None,
      match_radius_px: None,
      confidence: None,
      basis_frame_id: None,
      comparison_verdict: None,
      known_limits: vec![],
    };
    let report = derive_minecraft_training_result_quality_baseline_report(&profile, Some(&spatial), None, None, &[]);
    assert_eq!(report.evidence_coverage, "partial");
    assert!(report.issue.is_some());
  }
  #[test]
  fn quality_baseline_derive_surfaces_collection_issues() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let report = derive_minecraft_training_result_quality_baseline_report(
      &profile,
      None,
      None,
      None,
      &["read holdout preview manifest at /missing/path: no such file".to_string()],
    );
    assert_eq!(report.evidence_coverage, "missing_stage");
    assert!(report.issue.as_ref().is_some_and(|issue| issue.contains("read holdout preview manifest")));
  }

  fn sample_complete_quality_baseline_report(
    l1_mean: f64,
    mse: f64,
    render_verdict: &str,
    spatial_visibility: Option<&str>,
  ) -> MinecraftTrainingResultQualityBaselineReportSummary {
    let profile = quality_baseline_profile_v1().expect("profile");
    let semantic = profile.training_result_semantic_manifest_path.clone();
    let spatial = MinecraftTrainingResultSpatialQueryManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
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
      query_kind: "block_projection".to_string(),
      target_block: profile.query_target_block.clone(),
      target_face: profile.query_target_face.clone(),
      target_semantics: profile.query_target_semantics.clone(),
      selected_backend: Some("projection_reference".to_string()),
      status: "answered".to_string(),
      reason: None,
      visibility: spatial_visibility.map(str::to_string),
      screen_point: Some("854.0,480.0".to_string()),
      match_radius_px: Some(8.0),
      confidence: Some(0.9),
      basis_frame_id: Some("frame-355416".to_string()),
      comparison_verdict: Some("reference_only".to_string()),
      known_limits: vec![],
    };
    let holdout = MinecraftTrainingResultHoldoutPreviewManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic.clone(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
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
      holdout_frame_index: profile.holdout_frame_index,
      holdout_frame: Some(MinecraftTrainingResultHoldoutPreviewFrameWitnessSummary {
        frame_index: 6,
        spatial_frame_id: "frame-355416".to_string(),
        screenshot_path: "/tmp/frame_000006.png".to_string(),
        frame_json_path: "/tmp/frame_000006.json".to_string(),
      }),
      basis_checkpoint_path: Some(format!("/tmp/normalized/nerfstudio_models/{}", profile.basis_checkpoint_suffix)),
      holdout_screenshot_path: Some("/tmp/frame_000006.png".to_string()),
      reference_overlay_path: None,
      status: "ready".to_string(),
      reason: None,
      known_limits: vec![],
    };
    let render = MinecraftHoldoutRenderQualityManifestSummary {
      schema_version: 1,
      training_result_semantic_manifest_path: semantic,
      holdout_preview_manifest_path: "/tmp/holdout-preview.json".to_string(),
      source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_run_ids: vec![],
      holdout_frame_index: profile.holdout_frame_index,
      basis_checkpoint_path: holdout.basis_checkpoint_path.clone(),
      rendered_image_path: Some("/tmp/rendered.png".to_string()),
      image_size_match: true,
      metrics: Some(MinecraftHoldoutRenderQualityMetricsSummary {
        l1_mean: Some(l1_mean),
        mse: Some(mse),
        psnr: None,
      }),
      status: "ready".to_string(),
      reason: None,
      verdict: render_verdict.to_string(),
      known_limits: vec![],
    };
    derive_minecraft_training_result_quality_baseline_report(&profile, Some(&spatial), Some(&holdout), Some(&render), &[])
  }

  #[test]
  fn quality_baseline_verdict_probe_passes_on_complete_zero_metric_baseline() {
    let baseline = sample_complete_quality_baseline_report(0.0, 0.0, "measured_only", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "pass");
    assert_eq!(verdict.render_evidence_mode, "screenshot_copy_probe");
  }

  #[test]
  fn quality_baseline_verdict_fails_when_l1_exceeds_probe_max() {
    let baseline = sample_complete_quality_baseline_report(0.42, 0.0, "measured_only", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "fail");
    assert!(verdict.stage_checks.iter().any(|check| check.stage == "render_quality" && check.outcome == "fail"));
  }

  #[test]
  fn quality_baseline_verdict_blocks_on_partial_evidence_coverage() {
    let profile = quality_baseline_profile_v1().expect("profile");
    let baseline = derive_minecraft_training_result_quality_baseline_report(&profile, None, None, None, &[]);
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "blocked");
  }

  #[test]
  fn quality_baseline_verdict_partial_on_metric_partial_render() {
    let baseline = sample_complete_quality_baseline_report(0.0, 0.0, "metric_partial", Some("visible"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "partial");
  }

  #[test]
  fn quality_baseline_verdict_partial_on_spatial_outside_window() {
    let baseline = sample_complete_quality_baseline_report(0.0, 0.0, "measured_only", Some("outside_window"));
    let thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe thresholds");
    let verdict = derive_minecraft_training_result_quality_verdict(&baseline, &thresholds);
    assert_eq!(verdict.quality_verdict, "partial");
    assert!(verdict.stage_checks.iter().any(|check| check.stage == "spatial_query" && check.outcome == "fail"));
  }

  #[test]
  fn quality_baseline_verdict_threshold_fixtures_load() {
    let probe = quality_baseline_verdict_thresholds_probe_v1().expect("probe");
    let trained = quality_baseline_verdict_thresholds_trained_render_v1().expect("trained");
    assert_eq!(probe.render_evidence_mode, "screenshot_copy_probe");
    assert_eq!(trained.render_evidence_mode, "trained_render");
    assert_eq!(probe.profile_id, "mc17-d2-primary-v1");
  }
}
