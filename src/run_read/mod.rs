//! Read-side helpers for stored operation results and observation snapshots.
//!
//! These helpers intentionally sit below `runtime` and the root inspect read
//! projection used by `auv-inspect-server` so both call sites reuse one artifact
//! scan / compatibility policy:
//!
//! - verification claims come from `operation-result` JSON artifacts
//! - canonical scroll-scan payloads are read by purpose and `ArtifactUri`
//! - the legacy observation adapter still reads `scroll-scan` role artifacts
//! - input delivery evidence comes from standalone `input-action-result`
//!   JSON artifacts (`auv_driver::InputActionResult`)
//! - legacy `OperationOutput::Verification` remains readable without
//!   double-counting artifacts that also populate `OperationResult.verifications`

use std::collections::TryReserveError;

use crate::contract::{
  ArtifactRef, ObservationSnapshot, OperationOutput, OperationResult, RecognitionResult, RecognitionSource, VerificationResult,
};
use crate::model::AuvResult;
use crate::scroll_scan::{SCROLL_SCAN_JSON_BYTE_LIMIT, SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE, SCROLL_SCAN_PURPOSE, ScrollScanArtifact};
use auv_driver::{INPUT_ACTION_RESULT_ARTIFACT_ROLE, InputActionResult};
use auv_inspect_model::legacy::{artifact_record_view, is_json_mime, read_artifact_json};
use auv_tracing::{ArtifactReadError, ArtifactUri, AuthorityId, ErrorCode, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  store.read_run(run_id)
}

#[derive(Debug, thiserror::Error)]
pub enum ScrollScanReadError {
  #[error("scroll-scan snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("scroll-scan artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("scroll-scan artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("artifact {uri} has purpose {actual}, expected {SCROLL_SCAN_PURPOSE}")]
  WrongPurpose { uri: ArtifactUri, actual: String },
  #[error("artifact {uri} has content type {actual}, expected application/json")]
  WrongContentType { uri: ArtifactUri, actual: String },
  #[error("scroll-scan artifact {uri} is {actual} bytes, exceeding the {limit}-byte payload limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("failed to open scroll-scan artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to reserve {expected} bytes for scroll-scan artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed while streaming scroll-scan artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("artifact {uri} byte length is {actual}, expected committed length {expected}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("artifact {uri} SHA-256 digest is {actual}, expected committed digest {expected}")]
  DigestMismatch {
    uri: ArtifactUri,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("artifact {uri} is not a valid ScrollScanArtifact JSON payload: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
}

impl ScrollScanReadError {
  /// Returns a stable machine-readable category for this read failure.
  pub fn code(&self) -> ErrorCode {
    let value = match self {
      Self::SnapshotAuthorityMismatch { .. } => "auv.runtime.scroll_scan.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.runtime.scroll_scan.wrong_owner",
      Self::DanglingUri { .. } => "auv.runtime.scroll_scan.dangling_uri",
      Self::WrongPurpose { .. } => "auv.runtime.scroll_scan.wrong_purpose",
      Self::WrongContentType { .. } => "auv.runtime.scroll_scan.wrong_content_type",
      Self::PayloadTooLarge { .. } => SCROLL_SCAN_PAYLOAD_TOO_LARGE_CODE,
      Self::Open { .. } => "auv.runtime.scroll_scan.open_failed",
      Self::Allocation { .. } => "auv.runtime.scroll_scan.allocation_failed",
      Self::Stream { .. } => "auv.runtime.scroll_scan.stream_failed",
      Self::LengthMismatch { .. } => "auv.runtime.scroll_scan.length_mismatch",
      Self::DigestMismatch { .. } => "auv.runtime.scroll_scan.digest_mismatch",
      Self::MalformedJson { .. } => "auv.runtime.scroll_scan.malformed_json",
    };
    ErrorCode::parse(value).expect("static scroll-scan read error code is valid")
  }
}

/// Reads one exact `ScrollScanArtifact` from its committed V1 run record.
pub async fn read_scroll_scan(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<ScrollScanArtifact, ScrollScanReadError> {
  if snapshot.authority_id() != store.authority_id() {
    return Err(ScrollScanReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority: store.authority_id(),
    });
  }
  let artifact_run_id = uri.run_id();
  if artifact_run_id != snapshot.run_id() {
    return Err(ScrollScanReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id,
    });
  }
  let metadata = snapshot
    .artifacts()
    .get(uri)
    .map(|published| published.metadata())
    .ok_or_else(|| ScrollScanReadError::DanglingUri { uri: uri.clone() })?;
  if metadata.purpose().as_str() != SCROLL_SCAN_PURPOSE {
    return Err(ScrollScanReadError::WrongPurpose {
      uri: uri.clone(),
      actual: metadata.purpose().as_str().to_string(),
    });
  }
  if metadata.content_type().to_string() != "application/json" {
    return Err(ScrollScanReadError::WrongContentType {
      uri: uri.clone(),
      actual: metadata.content_type().to_string(),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > SCROLL_SCAN_JSON_BYTE_LIMIT {
    return Err(ScrollScanReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: SCROLL_SCAN_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| ScrollScanReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let expected_capacity = usize::try_from(expected_length).map_err(|_| ScrollScanReadError::PayloadTooLarge {
    uri: uri.clone(),
    limit: usize::MAX as u64,
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| ScrollScanReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| ScrollScanReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.saturating_add(chunk.len() as u64);
    if actual_length > SCROLL_SCAN_JSON_BYTE_LIMIT {
      return Err(ScrollScanReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: SCROLL_SCAN_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(ScrollScanReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(ScrollScanReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }

  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(ScrollScanReadError::DigestMismatch {
      uri: uri.clone(),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  serde_json::from_slice(&bytes).map_err(|source| ScrollScanReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })
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

pub use auv_inspect_model::legacy::ArtifactRefView;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DetectorRecognitionLineage {
  pub artifact: ArtifactRefView,
  pub status: DetectorRecognitionLineageStatus,
  pub recognition_id: Option<String>,
  pub source: Option<RecognitionSource>,
  pub backend: Option<String>,
  pub model_id: Option<String>,
  pub execution_provider: Option<String>,
  pub class_label_source_kind: Option<String>,
  pub runtime_projection_kind: Option<String>,
  pub capture_artifact: Option<ArtifactRefView>,
  pub capture_contract_artifact: Option<ArtifactRefView>,
  pub evidence_artifacts: Vec<ArtifactRefView>,
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
  // TODO(scroll-scan-legacy-reader): Keep this role/path adapter for the legacy
  // list/inspect entrypoints until their migration is explicitly approved. V1
  // inspect always supplies observations from canonical scroll-scan artifacts.
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

/// List typed input-delivery records persisted as `input-action-result` artifacts.
///
/// Delivery evidence is a standalone artifact role today; it is not embedded in
/// [`OperationResult`]. Callers must not treat presence or `attempts[*].succeeded`
/// as semantic success — that remains a separate verification claim.
pub(crate) fn list_input_action_results(store: &LocalStore, run_id: &str) -> AuvResult<Vec<InputActionResult>> {
  let run = store.read_run(run_id)?;
  extract_input_action_results(store, &run)
}

/// Scan a loaded run for `input-action-result` JSON artifacts in artifact order.
///
/// Non-matching roles and non-JSON MIME types are skipped. Matching role with
/// malformed JSON returns an error (no silent drop).
pub(crate) fn extract_input_action_results(store: &LocalStore, run: &CanonicalRun) -> AuvResult<Vec<InputActionResult>> {
  let mut results = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != INPUT_ACTION_RESULT_ARTIFACT_ROLE || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let result: InputActionResult = read_artifact_json(store, run.run.run_id.as_str(), artifact, INPUT_ACTION_RESULT_ARTIFACT_ROLE)?;
    results.push(result);
  }
  Ok(results)
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

    let detector_artifact = artifact_record_view(run.run.run_id.clone(), artifact);
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
    artifact: artifact_record_view(run.run.run_id.clone(), artifact),
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
  capture_artifact: Option<&ArtifactRefView>,
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

fn resolve_artifact_ref(run: &CanonicalRun, reference: &ArtifactRef) -> ArtifactRefView {
  let resolved = if reference.run_id == run.run.run_id {
    run.artifacts.iter().find(|artifact| artifact.artifact_id == reference.artifact_id && artifact.span_id == reference.span_id)
  } else {
    None
  };

  ArtifactRefView {
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

pub use crate::view_parser_read::{
  build_view_parser_inspect, build_view_resolution_summary, extract_playlist_select_result_wires, extract_reacquisition_records,
  extract_view_memory_writes, list_view_memory_writes,
};

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::path::{Path, PathBuf};

  use auv_driver::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};
  use auv_tracing::{ArtifactId, ArtifactUri};
  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
    ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId, SpanRecordV1Alpha1, TraceId,
    TraceState, TraceStatusCode,
  };
  use serde::Serialize;

  use super::{ScrollScanReadError, extract_input_action_results, list_input_action_results};
  use auv_driver::INPUT_ACTION_RESULT_ARTIFACT_ROLE;

  fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("auv-run-read-iar-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp root");
    root
  }

  fn stage_file(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    bytes: &[u8],
  ) -> ArtifactRecordV1Alpha1 {
    let source_path = root.join(format!("source-{index}-{preferred_name}"));
    fs::write(&source_path, bytes).expect("write source");
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
      .expect("stage artifact")
  }

  fn stage_json<T: Serialize>(
    store: &LocalStore,
    root: &Path,
    run_id: &RunId,
    span_id: &SpanId,
    index: usize,
    role: &str,
    preferred_name: &str,
    value: &T,
  ) -> ArtifactRecordV1Alpha1 {
    let rendered = serde_json::to_string_pretty(value).expect("serialize") + "\n";
    stage_file(store, root, run_id, span_id, index, role, preferred_name, rendered.as_bytes())
  }

  fn write_run(store: &LocalStore, run_id: &str, artifacts: Vec<ArtifactRecordV1Alpha1>) {
    let run_id = RunId::new(run_id);
    let span_id = SpanId::new("0000000000000001");
    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("trace_iar"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: Some("iar fixture".to_string()),
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id,
          parent_span_id: None,
          name: "auv.command".to_string(),
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
      .expect("write run");
  }

  fn sample_iar(path: InputDeliveryPath) -> InputActionResult {
    InputActionResult {
      selected_path: path,
      attempts: vec![InputAttempt::success(path)],
      fallback_reason: None,
      mouse_disturbance: DisturbanceLevel::None,
      focus_disturbance: DisturbanceLevel::Temporary,
      clipboard_disturbance: DisturbanceLevel::None,
    }
  }

  #[test]
  fn scroll_scan_allocation_error_has_stable_code_and_reserve_source() {
    let mut bytes = Vec::<u8>::new();
    let source = bytes.try_reserve_exact(usize::MAX).expect_err("usize::MAX must exceed Vec capacity");
    let source_message = source.to_string();
    let error = ScrollScanReadError::Allocation {
      uri: ArtifactUri::from_ids(auv_tracing::RunId::new(), ArtifactId::new()),
      expected: 1024,
      source,
    };

    assert_eq!(error.code().as_str(), "auv.runtime.scroll_scan.allocation_failed");
    assert_eq!(std::error::Error::source(&error).map(ToString::to_string).as_deref(), Some(source_message.as_str()));
  }

  #[test]
  fn extract_input_action_results_reads_valid_artifacts_in_order() {
    let root = temp_root("valid");
    let store = LocalStore::new(root.clone()).expect("store");
    let run_id = RunId::new("run_iar_valid");
    let span_id = SpanId::new("0000000000000001");
    let first = sample_iar(InputDeliveryPath::ForegroundSystemEvents);
    let second = sample_iar(InputDeliveryPath::WindowTargetedKeyboard);
    let artifacts = vec![
      stage_json(&store, &root, &run_id, &span_id, 0, INPUT_ACTION_RESULT_ARTIFACT_ROLE, "iar-0.json", &first),
      stage_json(&store, &root, &run_id, &span_id, 1, "operation-result", "op.json", &serde_json::json!({"ignored": true})),
      stage_json(&store, &root, &run_id, &span_id, 2, INPUT_ACTION_RESULT_ARTIFACT_ROLE, "iar-1.json", &second),
    ];
    write_run(&store, "run_iar_valid", artifacts);

    let results = list_input_action_results(&store, "run_iar_valid").expect("list");
    assert_eq!(results, vec![first, second]);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn extract_input_action_results_ignores_unrelated_roles_and_non_json() {
    let root = temp_root("filters");
    let store = LocalStore::new(root.clone()).expect("store");
    let run_id = RunId::new("run_iar_filters");
    let span_id = SpanId::new("0000000000000001");
    let kept = sample_iar(InputDeliveryPath::ClipboardPaste);
    // Non-JSON preferred name → text/plain mime; matching role but ignored by mime filter.
    let artifacts = vec![
      stage_file(
        &store,
        &root,
        &run_id,
        &span_id,
        0,
        INPUT_ACTION_RESULT_ARTIFACT_ROLE,
        "iar.txt",
        br#"{"selected_path":"clipboard_paste"}"#,
      ),
      stage_json(&store, &root, &run_id, &span_id, 1, "scroll-scan", "scan.json", &serde_json::json!({"snapshots": []})),
      stage_json(&store, &root, &run_id, &span_id, 2, INPUT_ACTION_RESULT_ARTIFACT_ROLE, "iar.json", &kept),
    ];
    write_run(&store, "run_iar_filters", artifacts);

    let results = extract_input_action_results(&store, &store.read_run("run_iar_filters").expect("read")).expect("extract");
    assert_eq!(results, vec![kept]);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn extract_input_action_results_errors_on_malformed_matching_json() {
    let root = temp_root("malformed");
    let store = LocalStore::new(root.clone()).expect("store");
    let run_id = RunId::new("run_iar_bad");
    let span_id = SpanId::new("0000000000000001");
    let artifacts = vec![stage_file(
      &store,
      &root,
      &run_id,
      &span_id,
      0,
      INPUT_ACTION_RESULT_ARTIFACT_ROLE,
      "iar-bad.json",
      b"{not-valid-json",
    )];
    write_run(&store, "run_iar_bad", artifacts);

    let error = list_input_action_results(&store, "run_iar_bad").expect_err("malformed must fail");
    assert!(error.contains("input-action-result"), "error={error}");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn extract_input_action_results_returns_empty_when_absent() {
    let root = temp_root("empty");
    let store = LocalStore::new(root.clone()).expect("store");
    write_run(&store, "run_iar_empty", Vec::new());
    let results = list_input_action_results(&store, "run_iar_empty").expect("list");
    assert!(results.is_empty());
    let _ = fs::remove_dir_all(root);
  }
}
