//! Osu ordinary run_read helpers for inspect composition.
//!
//! Depends on canonical `auv-tracing` run snapshots only (no `auv-cli`).
//! Query-wired adapters stay in the product crate until Task 22.

use std::collections::TryReserveError;
use std::io::Write;
use std::num::TryFromIntError;

use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactReadError, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, ByteLength, ContentType,
  Context, ErrorCode, NewArtifact, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest, ValidationError,
};
use futures_util::StreamExt;
use futures_util::io::Cursor as AsyncCursor;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.osu.structured_artifact.payload_too_large";

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, thiserror::Error)]
pub enum OsuArtifactPublishError {
  #[error("invalid osu! artifact purpose {value:?}: {source}")]
  InvalidPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid osu! artifact content type {value:?} for {purpose}: {source}")]
  InvalidContentType {
    purpose: ArtifactPurpose,
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("osu! artifact {purpose} failed domain validation: {message}")]
  InvalidPayload {
    purpose: ArtifactPurpose,
    message: String,
  },
  #[error("osu! artifact {purpose} JSON length {actual} cannot be represented as u64: {source}")]
  LengthOutOfRange {
    purpose: ArtifactPurpose,
    actual: u128,
    #[source]
    source: TryFromIntError,
  },
  #[error("{OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE}: {purpose} JSON is {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge {
    purpose: ArtifactPurpose,
    limit: u64,
    actual: u64,
  },
  #[error("failed to allocate {purpose} JSON bytes: {source}")]
  Allocation {
    purpose: ArtifactPurpose,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to serialize {purpose} as JSON: {source}")]
  Serialize {
    purpose: ArtifactPurpose,
    #[source]
    source: serde_json::Error,
  },
  #[error("invalid byte length for osu! artifact {purpose}: {source}")]
  InvalidByteLength {
    purpose: ArtifactPurpose,
    #[source]
    source: ValidationError,
  },
  #[error("failed to publish osu! artifact {purpose}: {source}")]
  Publication {
    purpose: ArtifactPurpose,
    #[source]
    source: ArtifactWriteError,
  },
}

#[derive(Debug, thiserror::Error)]
pub enum OsuArtifactReadError {
  #[error("invalid expected osu! artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("invalid expected osu! artifact content type {value:?}: {source}")]
  InvalidExpectedContentType {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("osu! snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("osu! artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("osu! artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("osu! artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: Box<ArtifactUri>,
    expected: ArtifactPurpose,
    actual: ArtifactPurpose,
  },
  #[error("osu! artifact {uri} has content type {actual}, expected {expected}")]
  WrongContentType {
    uri: Box<ArtifactUri>,
    expected: Box<ContentType>,
    actual: Box<ContentType>,
  },
  #[error("osu! artifact {uri} is {actual} bytes, exceeding the {limit}-byte structured-artifact limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("osu! artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for osu! artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open osu! artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream osu! artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("osu! artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("osu! artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: Box<ArtifactUri>,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("osu! artifact {uri} is not the expected JSON type: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
  #[error("osu! artifact {uri} failed domain validation: {message}")]
  InvalidPayload { uri: ArtifactUri, message: String },
}

impl OsuArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let value = match self {
      Self::InvalidExpectedPurpose { .. } | Self::InvalidExpectedContentType { .. } => "auv.osu.artifact.invalid_reader_contract",
      Self::SnapshotAuthorityMismatch { .. } => "auv.osu.artifact.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.osu.artifact.wrong_owner",
      Self::DanglingUri { .. } => "auv.osu.artifact.dangling_uri",
      Self::WrongPurpose { .. } => "auv.osu.artifact.wrong_purpose",
      Self::WrongContentType { .. } => "auv.osu.artifact.wrong_content_type",
      Self::PayloadTooLarge { .. } => OSU_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::LengthOutOfRange { .. } => "auv.osu.artifact.length_out_of_range",
      Self::Allocation { .. } => "auv.osu.artifact.allocation_failed",
      Self::Open { .. } => "auv.osu.artifact.open_failed",
      Self::Stream { .. } => "auv.osu.artifact.stream_failed",
      Self::LengthMismatch { .. } => "auv.osu.artifact.length_mismatch",
      Self::DigestMismatch { .. } => "auv.osu.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.osu.artifact.malformed_json",
      Self::InvalidPayload { .. } => "auv.osu.artifact.invalid_payload",
    };
    ErrorCode::parse(value).expect("static osu! artifact error code is valid")
  }
}

pub(crate) async fn publish_json_artifact<T, V>(
  context: Option<&Context>,
  purpose: &'static str,
  value: &T,
  validate: V,
) -> Result<Option<ArtifactMetadata>, OsuArtifactPublishError>
where
  T: Serialize,
  V: FnOnce(&T) -> Result<(), String>,
{
  let Some(context) = context.filter(|context| context.can_publish_artifacts()) else {
    return Ok(None);
  };

  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| OsuArtifactPublishError::InvalidPurpose {
    value: purpose,
    source,
  })?;
  validate(value).map_err(|message| OsuArtifactPublishError::InvalidPayload {
    purpose: purpose.clone(),
    message,
  })?;
  let body = serialize_json_bounded(&purpose, value)?;
  let byte_length = u64::try_from(body.len()).map_err(|source| OsuArtifactPublishError::LengthOutOfRange {
    purpose: purpose.clone(),
    actual: body.len() as u128,
    source,
  })?;
  let artifact = NewArtifact::new(
    purpose.clone(),
    ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| OsuArtifactPublishError::InvalidContentType {
      purpose: purpose.clone(),
      value: JSON_CONTENT_TYPE,
      source,
    })?,
    ByteLength::new(byte_length).map_err(|source| OsuArtifactPublishError::InvalidByteLength {
      purpose: purpose.clone(),
      source,
    })?,
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    AsyncCursor::new(body),
  );
  context.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.map_err(|source| OsuArtifactPublishError::Publication { purpose, source })
}

pub(crate) async fn read_json_artifact<T, V>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
  validate: V,
) -> Result<T, OsuArtifactReadError>
where
  T: serde::de::DeserializeOwned,
  V: FnOnce(&T) -> Result<(), String>,
{
  let bytes = read_json_artifact_bytes(store, snapshot, uri, expected_purpose).await?;
  let value = serde_json::from_slice(&bytes).map_err(|source| OsuArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  validate(&value).map_err(|message| OsuArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(value)
}

async fn read_json_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, OsuArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(OsuArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  if uri.run_id() != snapshot.run_id() {
    return Err(OsuArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| OsuArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  let expected_purpose = ArtifactPurpose::parse(expected_purpose).map_err(|source| OsuArtifactReadError::InvalidExpectedPurpose {
    value: expected_purpose,
    source,
  })?;
  if metadata.purpose() != &expected_purpose {
    return Err(OsuArtifactReadError::WrongPurpose {
      uri: Box::new(uri.clone()),
      expected: expected_purpose,
      actual: metadata.purpose().clone(),
    });
  }
  let expected_content_type = ContentType::parse(JSON_CONTENT_TYPE).map_err(|source| OsuArtifactReadError::InvalidExpectedContentType {
    value: JSON_CONTENT_TYPE,
    source,
  })?;
  if metadata.content_type() != &expected_content_type {
    return Err(OsuArtifactReadError::WrongContentType {
      uri: Box::new(uri.clone()),
      expected: Box::new(expected_content_type),
      actual: Box::new(metadata.content_type().clone()),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(OsuArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| OsuArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| OsuArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| OsuArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| OsuArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| OsuArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: u64::MAX,
    })?;
    if actual_length > OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      return Err(OsuArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(OsuArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(OsuArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(OsuArtifactReadError::DigestMismatch {
      uri: Box::new(uri.clone()),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

fn serialize_json_bounded<T: Serialize>(purpose: &ArtifactPurpose, value: &T) -> Result<Vec<u8>, OsuArtifactPublishError> {
  let mut output = BoundedJsonBuffer::new(purpose, OSU_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT);
  let result = serde_json::to_writer(&mut output, value);
  if let Some(failure) = output.failure.take() {
    return Err(match failure {
      JsonBufferFailure::LengthOutOfRange { actual, source } => OsuArtifactPublishError::LengthOutOfRange {
        purpose: purpose.clone(),
        actual,
        source,
      },
      JsonBufferFailure::PayloadTooLarge { actual } => OsuArtifactPublishError::PayloadTooLarge {
        purpose: purpose.clone(),
        limit: output.limit,
        actual,
      },
      JsonBufferFailure::Allocation(source) => OsuArtifactPublishError::Allocation {
        purpose: purpose.clone(),
        source,
      },
    });
  }
  result.map_err(|source| OsuArtifactPublishError::Serialize {
    purpose: purpose.clone(),
    source,
  })?;
  Ok(output.bytes)
}

struct BoundedJsonBuffer<'a> {
  purpose: &'a ArtifactPurpose,
  limit: u64,
  bytes: Vec<u8>,
  failure: Option<JsonBufferFailure>,
}

enum JsonBufferFailure {
  LengthOutOfRange {
    actual: u128,
    source: TryFromIntError,
  },
  PayloadTooLarge {
    actual: u64,
  },
  Allocation(TryReserveError),
}

impl<'a> BoundedJsonBuffer<'a> {
  fn new(purpose: &'a ArtifactPurpose, limit: u64) -> Self {
    Self {
      purpose,
      limit,
      bytes: Vec::new(),
      failure: None,
    }
  }

  fn fail(&mut self, failure: JsonBufferFailure) -> std::io::Error {
    self.failure = Some(failure);
    std::io::Error::other(format!("{} JSON exceeded its bounded buffer", self.purpose))
  }
}

impl Write for BoundedJsonBuffer<'_> {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let Some(next_length) = self.bytes.len().checked_add(buffer.len()) else {
      return Err(self.fail(JsonBufferFailure::PayloadTooLarge { actual: u64::MAX }));
    };
    let next_length = match u64::try_from(next_length) {
      Ok(length) => length,
      Err(source) => {
        return Err(self.fail(JsonBufferFailure::LengthOutOfRange {
          actual: next_length as u128,
          source,
        }));
      }
    };
    if next_length > self.limit {
      return Err(self.fail(JsonBufferFailure::PayloadTooLarge {
        actual: next_length,
      }));
    }
    if let Err(source) = self.bytes.try_reserve(buffer.len()) {
      return Err(self.fail(JsonBufferFailure::Allocation(source)));
    }
    self.bytes.extend_from_slice(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

pub(crate) fn now_millis() -> u64 {
  u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX)
}

use crate::detection_eval_quality::{OSU_DETECTION_EVAL_QUALITY_PURPOSE, read_osu_detection_eval_quality};
use crate::detection_eval_witness::{OSU_DETECTION_EVAL_WITNESS_PURPOSE, read_osu_detection_eval_witness};
use crate::visual_truth_semantic::{OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE, read_osu_visual_truth_semantic};
use crate::visual_truth_spatial_query::{OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE, read_osu_visual_truth_spatial_query};
use crate::{
  DetectionEvalQualityInspectReport, DetectionEvalQualityManifest, DetectionEvalWitnessInspectReport, DetectionEvalWitnessManifest,
  VisualTruthSemanticInspectReport, VisualTruthSemanticManifest, VisualTruthSpatialQueryInspectReport, VisualTruthSpatialQueryManifest,
  derive_visual_truth_spatial_query_action_readiness,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OsuArtifactView {
  pub artifact_id: ArtifactUri,
  pub role: Option<String>,
  pub path: Option<String>,
}

fn artifact_view(uri: &ArtifactUri, purpose: &'static str) -> OsuArtifactView {
  OsuArtifactView {
    artifact_id: uri.clone(),
    role: Some(purpose.to_string()),
    path: None,
  }
}

pub(crate) struct OsuVisualTruthSemanticManifestLineage {
  pub artifact: OsuArtifactView,
  pub manifest: Option<OsuVisualTruthSemanticManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuVisualTruthSemanticInspectReportLineage {
  pub artifact: OsuArtifactView,
  pub report: Option<OsuVisualTruthSemanticInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryManifestLineage {
  pub artifact: OsuArtifactView,
  pub manifest: Option<OsuVisualTruthSpatialQueryManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuVisualTruthSpatialQueryInspectReportLineage {
  pub artifact: OsuArtifactView,
  pub report: Option<OsuVisualTruthSpatialQueryInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct OsuVisualTruthSpatialQueryActionReadinessSummary {
  pub action_eligibility: String,
  pub pixel_point: Option<String>,
  pub refusal_reason: Option<String>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSemanticManifestSummary {
  pub schema_version: u32,
  pub source_run_artifact_dir: String,
  pub source_visual_truth_manifest_path: String,
  pub source_projection_path: String,
  pub beatmap_path: String,
  pub frame_count: usize,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSemanticInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub semantic_status: String,
  pub semantic_reason: Option<String>,
  pub visual_truth_manifest_readable: bool,
  pub projection_readable: bool,
  pub projection_eval_ready: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OsuVisualTruthSpatialQueryManifestSummary {
  pub schema_version: u32,
  pub visual_truth_semantic_manifest_path: String,
  pub source_run_artifact_dir: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub object_kind: Option<String>,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub pixel_x: Option<f32>,
  pub pixel_y: Option<f32>,
  pub match_radius_px: Option<f32>,
  pub capture_width: Option<u32>,
  pub capture_height: Option<u32>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuVisualTruthSpatialQueryInspectReportSummary {
  pub schema_version: u32,
  pub visual_truth_spatial_query_manifest_path: String,
  pub visual_truth_semantic_manifest_path: String,
  pub object_index: usize,
  pub capture_phase: String,
  pub query_backend: String,
  pub status: String,
  pub reason: Option<String>,
  pub pixel_visibility: Option<String>,
  pub semantic_status: String,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalWitnessManifestLineage {
  pub artifact: OsuArtifactView,
  pub manifest: Option<OsuDetectionEvalWitnessManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalWitnessInspectReportLineage {
  pub artifact: OsuArtifactView,
  pub report: Option<OsuDetectionEvalWitnessInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalQualityManifestLineage {
  pub artifact: OsuArtifactView,
  pub manifest: Option<OsuDetectionEvalQualityManifestSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub(crate) struct OsuDetectionEvalQualityInspectReportLineage {
  pub artifact: OsuArtifactView,
  pub report: Option<OsuDetectionEvalQualityInspectReportSummary>,
  pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalWitnessManifestSummary {
  pub schema_version: u32,
  pub source_visual_eval_report_path: String,
  pub source_run_artifact_dir: String,
  pub detector_model_id: Option<String>,
  pub total_frames: usize,
  pub label_matched_frames: usize,
  pub spatial_matched_frames: usize,
  pub spatial_unscored_frames: usize,
  pub spurious_detection_count: usize,
  pub projection_kind: String,
  pub frame_witness_count: usize,
  pub status: String,
  pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalWitnessInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub total_frames: usize,
  pub frame_witness_count: usize,
  pub status: String,
  pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalQualityManifestSummary {
  pub schema_version: u32,
  pub detection_eval_witness_manifest_path: String,
  pub source_visual_eval_report_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall: Option<f32>,
  pub spatial_recall: Option<f32>,
  pub spurious_detection_count: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct OsuDetectionEvalQualityInspectReportSummary {
  pub schema_version: u32,
  pub detection_eval_quality_manifest_path: String,
  pub witness_status: String,
  pub status: String,
  pub verdict: String,
  pub label_recall_available: bool,
  pub spatial_recall_available: bool,
}
pub(crate) fn validate_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), OsuArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(OsuArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  Ok(())
}

pub(crate) fn artifact_uris_for_purpose(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  purpose: &'static str,
) -> Result<Vec<ArtifactUri>, OsuArtifactReadError> {
  validate_snapshot_authority(store, snapshot)?;
  let purpose = ArtifactPurpose::parse(purpose).map_err(|source| OsuArtifactReadError::InvalidExpectedPurpose {
    value: purpose,
    source,
  })?;
  Ok(
    snapshot
      .artifacts()
      .values()
      .filter(|artifact| artifact.metadata().purpose() == &purpose)
      .map(|artifact| artifact.metadata().uri().clone())
      .collect(),
  )
}

pub(crate) async fn extract_osu_visual_truth_semantic_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuVisualTruthSemanticManifestLineage>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE)? {
    let manifest = read_osu_visual_truth_semantic(store, snapshot, &uri).await?;
    manifests.push(OsuVisualTruthSemanticManifestLineage {
      artifact: artifact_view(&uri, OSU_VISUAL_TRUTH_SEMANTIC_PURPOSE),
      manifest: Some(OsuVisualTruthSemanticManifestSummary::from(&manifest)),
      issue: None,
    });
  }
  Ok(manifests)
}

pub async fn extract_osu_visual_truth_spatial_query_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuVisualTruthSpatialQueryManifestLineage>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE)? {
    let manifest = read_osu_visual_truth_spatial_query(store, snapshot, &uri).await?;
    manifests.push(OsuVisualTruthSpatialQueryManifestLineage {
      artifact: artifact_view(&uri, OSU_VISUAL_TRUTH_SPATIAL_QUERY_PURPOSE),
      manifest: Some(OsuVisualTruthSpatialQueryManifestSummary::from(&manifest)),
      issue: None,
    });
  }
  Ok(manifests)
}

pub(crate) async fn extract_osu_detection_eval_witness_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuDetectionEvalWitnessManifestLineage>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, OSU_DETECTION_EVAL_WITNESS_PURPOSE)? {
    let manifest = read_osu_detection_eval_witness(store, snapshot, &uri).await?;
    manifests.push(OsuDetectionEvalWitnessManifestLineage {
      artifact: artifact_view(&uri, OSU_DETECTION_EVAL_WITNESS_PURPOSE),
      manifest: Some(OsuDetectionEvalWitnessManifestSummary::from(&manifest)),
      issue: None,
    });
  }
  Ok(manifests)
}

pub(crate) async fn extract_osu_detection_eval_quality_manifests(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<OsuDetectionEvalQualityManifestLineage>, OsuArtifactReadError> {
  let mut manifests = Vec::new();
  for uri in artifact_uris_for_purpose(store, snapshot, OSU_DETECTION_EVAL_QUALITY_PURPOSE)? {
    let manifest = read_osu_detection_eval_quality(store, snapshot, &uri).await?;
    manifests.push(OsuDetectionEvalQualityManifestLineage {
      artifact: artifact_view(&uri, OSU_DETECTION_EVAL_QUALITY_PURPOSE),
      manifest: Some(OsuDetectionEvalQualityManifestSummary::from(&manifest)),
      issue: None,
    });
  }
  Ok(manifests)
}

pub(crate) fn derive_osu_detection_eval_quality_verdict_summary(lineage: &OsuDetectionEvalQualityManifestLineage) -> String {
  if lineage.issue.is_some() {
    return "n/a".to_string();
  }
  let Some(summary) = &lineage.manifest else {
    return "n/a".to_string();
  };
  summary.verdict.clone()
}

pub fn derive_osu_visual_truth_spatial_query_action_readiness(
  lineage: &OsuVisualTruthSpatialQueryManifestLineage,
) -> OsuVisualTruthSpatialQueryActionReadinessSummary {
  if let Some(issue) = &lineage.issue {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some(issue.clone()),
    };
  }
  let Some(summary) = &lineage.manifest else {
    return OsuVisualTruthSpatialQueryActionReadinessSummary {
      action_eligibility: "n/a".to_string(),
      pixel_point: None,
      refusal_reason: None,
      issue: Some("osu visual truth spatial query manifest summary missing".to_string()),
    };
  };
  let manifest = match osu_spatial_query_manifest_summary_for_action_readiness(summary) {
    Ok(manifest) => manifest,
    Err(error) => {
      return OsuVisualTruthSpatialQueryActionReadinessSummary {
        action_eligibility: "n/a".to_string(),
        pixel_point: None,
        refusal_reason: None,
        issue: Some(error),
      };
    }
  };
  let readiness = derive_visual_truth_spatial_query_action_readiness(&manifest);
  OsuVisualTruthSpatialQueryActionReadinessSummary {
    action_eligibility: readiness.eligibility.as_str().to_string(),
    pixel_point: readiness.pixel_point.map(|(x, y)| format!("{x},{y}")),
    refusal_reason: readiness.refusal_reason,
    issue: None,
  }
}

fn osu_spatial_query_manifest_summary_for_action_readiness(
  summary: &OsuVisualTruthSpatialQueryManifestSummary,
) -> Result<VisualTruthSpatialQueryManifest, String> {
  use crate::{
    CapturePhase, ObjectKind, VisualTruthPixelVisibility, VisualTruthSpatialQueryBackend, VisualTruthSpatialQueryReason,
    VisualTruthSpatialQueryStatus,
  };
  let capture_phase = match summary.capture_phase.as_str() {
    "before_dispatch" => CapturePhase::BeforeDispatch,
    "after_dispatch" => CapturePhase::AfterDispatch,
    other => return Err(format!("unknown capture_phase {other}")),
  };
  let object_kind = match summary.object_kind.as_deref() {
    None => None,
    Some(kind) => Some(match kind {
      "circle" => ObjectKind::Circle,
      "slider" => ObjectKind::Slider,
      "spinner" => ObjectKind::Spinner,
      "hold" => ObjectKind::Hold,
      other => return Err(format!("unknown object_kind {other}")),
    }),
  };
  let status = match summary.status.as_str() {
    "answered" => VisualTruthSpatialQueryStatus::Answered,
    "blocked" => VisualTruthSpatialQueryStatus::Blocked,
    "failed" => VisualTruthSpatialQueryStatus::Failed,
    other => return Err(format!("unknown query status {other}")),
  };
  let reason = match summary.reason.as_deref() {
    None => None,
    Some(reason) => Some(match reason {
      "semantic_source_not_ready" => VisualTruthSpatialQueryReason::SemanticSourceNotReady,
      "target_absent_from_visual_truth" => VisualTruthSpatialQueryReason::TargetAbsentFromVisualTruth,
      "projection_unavailable" => VisualTruthSpatialQueryReason::ProjectionUnavailable,
      other => return Err(format!("unknown query reason {other}")),
    }),
  };
  let pixel_visibility = match summary.pixel_visibility.as_deref() {
    None => None,
    Some(value) => Some(match value {
      "inside_capture" => VisualTruthPixelVisibility::InsideCapture,
      "outside_capture" => VisualTruthPixelVisibility::OutsideCapture,
      other => return Err(format!("unknown pixel_visibility {other}")),
    }),
  };
  let query_backend = match summary.query_backend.as_str() {
    "playfield_projection_reference" => VisualTruthSpatialQueryBackend::PlayfieldProjectionReference,
    other => return Err(format!("unknown query backend {other}")),
  };
  Ok(VisualTruthSpatialQueryManifest {
    schema_version: summary.schema_version,
    generated_at_millis: 0,
    visual_truth_semantic_manifest_path: summary.visual_truth_semantic_manifest_path.clone(),
    source_run_artifact_dir: summary.source_run_artifact_dir.clone(),
    source_visual_truth_manifest_path: String::new(),
    source_projection_path: String::new(),
    object_index: summary.object_index,
    capture_phase,
    object_kind,
    query_backend,
    status,
    reason,
    pixel_visibility,
    pixel_x: summary.pixel_x,
    pixel_y: summary.pixel_y,
    match_radius_px: summary.match_radius_px,
    capture_width: summary.capture_width,
    capture_height: summary.capture_height,
    known_limits: summary.known_limits.clone(),
  })
}

impl From<&DetectionEvalWitnessManifest> for OsuDetectionEvalWitnessManifestSummary {
  fn from(manifest: &DetectionEvalWitnessManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      detector_model_id: manifest.detector_model_id.clone(),
      total_frames: manifest.total_frames,
      label_matched_frames: manifest.label_matched_frames,
      spatial_matched_frames: manifest.spatial_matched_frames,
      spatial_unscored_frames: manifest.spatial_unscored_frames,
      spurious_detection_count: manifest.spurious_detection_count,
      projection_kind: manifest.projection_kind.clone(),
      frame_witness_count: manifest.frame_witnesses.len(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
    }
  }
}

impl From<&DetectionEvalWitnessInspectReport> for OsuDetectionEvalWitnessInspectReportSummary {
  fn from(report: &DetectionEvalWitnessInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_witness_manifest_path: report.detection_eval_witness_manifest_path.clone(),
      total_frames: report.total_frames,
      frame_witness_count: report.frame_witness_count,
      status: report.status.as_str().to_string(),
      warnings: report.warnings.clone(),
    }
  }
}

impl From<&DetectionEvalQualityManifest> for OsuDetectionEvalQualityManifestSummary {
  fn from(manifest: &DetectionEvalQualityManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      detection_eval_witness_manifest_path: manifest.detection_eval_witness_manifest_path.clone(),
      source_visual_eval_report_path: manifest.source_visual_eval_report_path.clone(),
      witness_status: manifest.witness_status.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      verdict: manifest.verdict.as_str().to_string(),
      label_recall: manifest.metrics.as_ref().and_then(|m| m.label_recall),
      spatial_recall: manifest.metrics.as_ref().and_then(|m| m.spatial_recall),
      spurious_detection_count: manifest.metrics.as_ref().map(|m| m.spurious_detection_count),
    }
  }
}

impl From<&DetectionEvalQualityInspectReport> for OsuDetectionEvalQualityInspectReportSummary {
  fn from(report: &DetectionEvalQualityInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      detection_eval_quality_manifest_path: report.detection_eval_quality_manifest_path.clone(),
      witness_status: report.witness_status.as_str().to_string(),
      status: report.status.as_str().to_string(),
      verdict: report.verdict.as_str().to_string(),
      label_recall_available: report.label_recall_available,
      spatial_recall_available: report.spatial_recall_available,
    }
  }
}

impl From<&VisualTruthSemanticManifest> for OsuVisualTruthSemanticManifestSummary {
  fn from(manifest: &VisualTruthSemanticManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      source_visual_truth_manifest_path: manifest.source_visual_truth_manifest_path.clone(),
      source_projection_path: manifest.source_projection_path.clone(),
      beatmap_path: manifest.beatmap_path.clone(),
      frame_count: manifest.frame_count,
      semantic_status: manifest.semantic_status.as_str().to_string(),
      semantic_reason: manifest.semantic_reason.map(|reason| reason.as_str().to_string()),
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSemanticInspectReport> for OsuVisualTruthSemanticInspectReportSummary {
  fn from(report: &VisualTruthSemanticInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: report.source_run_artifact_dir.clone(),
      semantic_status: report.semantic_status.as_str().to_string(),
      semantic_reason: report.semantic_reason.map(|reason| reason.as_str().to_string()),
      visual_truth_manifest_readable: report.visual_truth_manifest_readable,
      projection_readable: report.projection_readable,
      projection_eval_ready: report.projection_eval_ready,
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSpatialQueryManifest> for OsuVisualTruthSpatialQueryManifestSummary {
  fn from(manifest: &VisualTruthSpatialQueryManifest) -> Self {
    Self {
      schema_version: manifest.schema_version,
      visual_truth_semantic_manifest_path: manifest.visual_truth_semantic_manifest_path.clone(),
      source_run_artifact_dir: manifest.source_run_artifact_dir.clone(),
      object_index: manifest.object_index,
      capture_phase: match manifest.capture_phase {
        crate::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        crate::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      object_kind: manifest.object_kind.as_ref().map(|kind| match kind {
        crate::ObjectKind::Circle => "circle".to_string(),
        crate::ObjectKind::Slider => "slider".to_string(),
        crate::ObjectKind::Spinner => "spinner".to_string(),
        crate::ObjectKind::Hold => "hold".to_string(),
      }),
      query_backend: manifest.query_backend.as_str().to_string(),
      status: manifest.status.as_str().to_string(),
      reason: manifest.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: manifest.pixel_visibility.map(|visibility| visibility.as_str().to_string()),
      pixel_x: manifest.pixel_x,
      pixel_y: manifest.pixel_y,
      match_radius_px: manifest.match_radius_px,
      capture_width: manifest.capture_width,
      capture_height: manifest.capture_height,
      known_limits: manifest.known_limits.clone(),
    }
  }
}

impl From<&VisualTruthSpatialQueryInspectReport> for OsuVisualTruthSpatialQueryInspectReportSummary {
  fn from(report: &VisualTruthSpatialQueryInspectReport) -> Self {
    Self {
      schema_version: report.schema_version,
      visual_truth_spatial_query_manifest_path: report.visual_truth_spatial_query_manifest_path.clone(),
      visual_truth_semantic_manifest_path: report.visual_truth_semantic_manifest_path.clone(),
      object_index: report.object_index,
      capture_phase: match report.capture_phase {
        crate::CapturePhase::BeforeDispatch => "before_dispatch".to_string(),
        crate::CapturePhase::AfterDispatch => "after_dispatch".to_string(),
      },
      query_backend: report.query_backend.as_str().to_string(),
      status: report.status.as_str().to_string(),
      reason: report.reason.map(|reason| reason.as_str().to_string()),
      pixel_visibility: report.pixel_visibility.map(|visibility| visibility.as_str().to_string()),
      semantic_status: report.semantic_status.as_str().to_string(),
      warnings: report.warnings.clone(),
      known_limits: report.known_limits.clone(),
    }
  }
}
