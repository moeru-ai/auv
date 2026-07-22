//! NetEase structured artifacts and app-local canonical URI lineage.

use std::collections::TryReserveError;
use std::io::Write;
use std::path::{Path, PathBuf};

use auv_tracing::{
  ArtifactMetadata, ArtifactReadError, ArtifactUri, ArtifactWriteError, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode,
  FileRunStore, NewArtifact, ReadError, RunId, RunSnapshot, RunStore, Sha256Digest,
};
use auv_view::memory::ViewMemory;
use futures_util::StreamExt;
use futures_util::io::Cursor as AsyncCursor;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::commands::playlist::PlaylistSelectResult;
use crate::{Inputs, PlaylistSidebarScan};

pub const PLAYLIST_SIDEBAR_SCAN_PURPOSE: &str = "auv.netease.playlist_sidebar_scan";
pub const VIEW_MEMORY_PURPOSE: &str = "auv.netease.view_memory";
pub const PLAYLIST_SELECT_RESULT_PURPOSE: &str = "auv.netease.playlist_select_result";

pub const VIEW_MEMORY_RUN_LINEAGE_FILE: &str = "view-memory-run-lineage.json";
pub const VIEW_MEMORY_LINEAGE_SCHEMA_VERSION: &str = "view-memory-lineage-v1";

/// NetEase structured artifacts contain OCR/view records, not bulk media.
/// Four MiB leaves ample room above the bounded 12-scroll playlist fixtures
/// while keeping producer and reader allocation independent of the 512 MiB
/// whole-artifact ceiling.
pub const NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.netease.structured_artifact.payload_too_large";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewMemoryRunLineage {
  pub schema_version: String,
  pub scan_uri: ArtifactUri,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub memory_uri: Option<ArtifactUri>,
  pub memory_id: String,
  pub scope_id: String,
  pub app_bundle_id: String,
  pub written_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PersistedLineage {
  pub lineage: ViewMemoryRunLineage,
  pub memory: Option<ViewMemory>,
}

#[derive(Debug, thiserror::Error)]
pub enum LineageManifestError {
  #[error("failed to read NetEase lineage manifest {path}: {source}")]
  Read {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("failed to decode NetEase lineage manifest {path}: {source}")]
  Decode {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("unsupported NetEase lineage schema {actual:?}; expected {VIEW_MEMORY_LINEAGE_SCHEMA_VERSION:?}")]
  UnsupportedSchema { actual: String },
  #[error("NetEase lineage belongs to app {actual:?}, not {expected:?}")]
  WrongApp { expected: String, actual: String },
  #[error("NetEase lineage belongs to scope {actual:?}, not {expected:?}")]
  WrongScope { expected: String, actual: String },
  #[error("failed to create NetEase lineage directory {path}: {source}")]
  CreateDirectory {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("failed to encode NetEase lineage manifest: {0}")]
  Encode(#[source] serde_json::Error),
  #[error("failed to write NetEase lineage manifest {path}: {source}")]
  Write {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("failed to publish NetEase lineage manifest {from} as {to}: {source}")]
  Rename {
    from: PathBuf,
    to: PathBuf,
    #[source]
    source: std::io::Error,
  },
}

pub fn lineage_manifest_path(artifact_dir: &Path) -> PathBuf {
  artifact_dir.join(VIEW_MEMORY_RUN_LINEAGE_FILE)
}

pub fn read_lineage_manifest(artifact_dir: &Path) -> Result<ViewMemoryRunLineage, LineageManifestError> {
  let path = lineage_manifest_path(artifact_dir);
  let bytes = std::fs::read(&path).map_err(|source| LineageManifestError::Read {
    path: path.clone(),
    source,
  })?;
  let lineage: ViewMemoryRunLineage = serde_json::from_slice(&bytes).map_err(|source| LineageManifestError::Decode { path, source })?;
  validate_lineage_schema(&lineage)?;
  Ok(lineage)
}

pub fn read_lineage_manifest_for_inputs(artifact_dir: &Path, inputs: &Inputs) -> Result<ViewMemoryRunLineage, LineageManifestError> {
  let lineage = read_lineage_manifest(artifact_dir)?;
  if lineage.app_bundle_id != inputs.app_id {
    return Err(LineageManifestError::WrongApp {
      expected: inputs.app_id.clone(),
      actual: lineage.app_bundle_id,
    });
  }
  if lineage.scope_id != crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID {
    return Err(LineageManifestError::WrongScope {
      expected: crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
      actual: lineage.scope_id,
    });
  }
  Ok(lineage)
}

pub fn write_lineage_manifest(artifact_dir: &Path, lineage: &ViewMemoryRunLineage) -> Result<(), LineageManifestError> {
  validate_lineage_schema(lineage)?;
  std::fs::create_dir_all(artifact_dir).map_err(|source| LineageManifestError::CreateDirectory {
    path: artifact_dir.to_path_buf(),
    source,
  })?;
  let path = lineage_manifest_path(artifact_dir);
  let bytes = serde_json::to_vec_pretty(lineage).map_err(LineageManifestError::Encode)?;
  let temporary = path.with_extension("json.tmp");
  std::fs::write(&temporary, bytes).map_err(|source| LineageManifestError::Write {
    path: temporary.clone(),
    source,
  })?;
  std::fs::rename(&temporary, &path).map_err(|source| LineageManifestError::Rename {
    from: temporary,
    to: path,
    source,
  })
}

fn validate_lineage_schema(lineage: &ViewMemoryRunLineage) -> Result<(), LineageManifestError> {
  if lineage.schema_version != VIEW_MEMORY_LINEAGE_SCHEMA_VERSION {
    return Err(LineageManifestError::UnsupportedSchema {
      actual: lineage.schema_version.clone(),
    });
  }
  Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum NeteaseArtifactPublishError {
  #[error("NetEase artifact publication is disabled because no caller-owned run authority is current")]
  Disabled,
  #[error("invalid NetEase artifact contract for {purpose}: {message}")]
  InvalidContract {
    purpose: &'static str,
    message: String,
  },
  #[error("{NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE}: {purpose} JSON is {actual} bytes, exceeding the {limit}-byte limit")]
  PayloadTooLarge {
    purpose: &'static str,
    limit: u64,
    actual: u64,
  },
  #[error("failed to serialize {purpose} as JSON: {source}")]
  Serialize {
    purpose: &'static str,
    #[source]
    source: serde_json::Error,
  },
  #[error("failed to allocate {purpose} JSON bytes: {source}")]
  Allocation {
    purpose: &'static str,
    #[source]
    source: TryReserveError,
  },
  #[error("{purpose} JSON serialization changed between measurement and publication")]
  NondeterministicSerialization { purpose: &'static str },
  #[error("failed to publish {purpose}: {source}")]
  Publication {
    purpose: &'static str,
    #[source]
    source: ArtifactWriteError,
  },
}

pub struct PlaylistSelectPublication {
  value: PlaylistSelectResult,
  instrumentation: PlaylistSelectInstrumentation,
}

impl PlaylistSelectPublication {
  pub fn into_parts(self) -> (PlaylistSelectResult, PlaylistSelectInstrumentation) {
    (self.value, self.instrumentation)
  }
}

pub enum PlaylistSelectInstrumentation {
  Published(ArtifactMetadata),
  Disabled,
  Failed(NeteaseArtifactPublishError),
}

impl PlaylistSelectInstrumentation {
  pub fn metadata(&self) -> Option<&ArtifactMetadata> {
    match self {
      Self::Published(metadata) => Some(metadata),
      Self::Disabled | Self::Failed(_) => None,
    }
  }

  pub fn failure(&self) -> Option<&NeteaseArtifactPublishError> {
    match self {
      Self::Failed(error) => Some(error),
      Self::Published(_) | Self::Disabled => None,
    }
  }

  pub fn is_disabled(&self) -> bool {
    matches!(self, Self::Disabled)
  }
}

/// Publishes the scan and its optional view memory into the caller's current
/// run. The domain scan has already completed; publication errors never cause
/// the scan to execute again.
pub async fn persist_playlist_ls_artifacts(
  scan: &PlaylistSidebarScan,
  inputs: &Inputs,
  memory_enabled: bool,
) -> Result<PersistedLineage, NeteaseArtifactPublishError> {
  let scan_metadata = publish_json(PLAYLIST_SIDEBAR_SCAN_PURPOSE, scan).await?.ok_or(NeteaseArtifactPublishError::Disabled)?;
  let scan_uri = scan_metadata.uri().clone();
  let memory = if memory_enabled {
    crate::view_memory::try_build_writable_memory(inputs, scan, &scan_uri)
  } else {
    None
  };
  let memory_uri = match &memory {
    Some(memory) => publish_json(VIEW_MEMORY_PURPOSE, memory).await?.map(|metadata| metadata.uri().clone()),
    None => None,
  };
  Ok(PersistedLineage {
    lineage: ViewMemoryRunLineage {
      schema_version: VIEW_MEMORY_LINEAGE_SCHEMA_VERSION.to_string(),
      scan_uri,
      memory_uri,
      memory_id: auv_view::memory::build_memory_id(&inputs.app_id, crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID),
      scope_id: crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
      app_bundle_id: inputs.app_id.clone(),
      written_at_millis: crate::view_memory::system_time_millis(),
    },
    memory,
  })
}

/// Returns the existing playlist-select result independently from its
/// non-authoritative artifact instrumentation receipt.
pub async fn persist_playlist_select_proof(result: PlaylistSelectResult) -> PlaylistSelectPublication {
  let context = auv_tracing::Context::current();
  let mut artifact_result = result.clone();
  if context.authority_id().is_some()
    && let Some(run_id) = context.run_id()
  {
    artifact_result.run_id = Some(run_id.to_string());
  }
  match publish_json(PLAYLIST_SELECT_RESULT_PURPOSE, &artifact_result).await {
    Ok(Some(metadata)) => PlaylistSelectPublication {
      value: artifact_result,
      instrumentation: PlaylistSelectInstrumentation::Published(metadata),
    },
    Ok(None) => PlaylistSelectPublication {
      value: result,
      instrumentation: PlaylistSelectInstrumentation::Disabled,
    },
    Err(error) => PlaylistSelectPublication {
      value: result,
      instrumentation: PlaylistSelectInstrumentation::Failed(error),
    },
  }
}

pub async fn read_playlist_sidebar_scan(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<PlaylistSidebarScan, NeteaseArtifactReadError> {
  let bytes = read_json_bytes(store, snapshot, uri, PLAYLIST_SIDEBAR_SCAN_PURPOSE).await?;
  let json = std::str::from_utf8(&bytes).map_err(|source| NeteaseArtifactReadError::MalformedJson {
    uri: uri.clone(),
    message: source.to_string(),
  })?;
  crate::decode_playlist_sidebar_scan_json(json).map_err(|message| NeteaseArtifactReadError::MalformedJson {
    uri: uri.clone(),
    message,
  })
}

pub async fn read_view_memory(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<ViewMemory, NeteaseArtifactReadError> {
  read_json(store, snapshot, uri, VIEW_MEMORY_PURPOSE).await
}

pub async fn read_playlist_select_result(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<PlaylistSelectResult, NeteaseArtifactReadError> {
  read_json(store, snapshot, uri, PLAYLIST_SELECT_RESULT_PURPOSE).await
}

/// Resolves the app-local lineage pointer against the explicitly selected
/// file authority. There is no artifact-directory scan fallback.
pub fn try_load_scan_cache_with_limits(inputs: &Inputs) -> (Option<PlaylistSidebarScan>, Vec<String>) {
  match load_scan_for_inputs(inputs) {
    Ok(scan) => (Some(scan), Vec::new()),
    Err(error) => (None, vec![error]),
  }
}

pub fn try_load_view_memory(inputs: &Inputs) -> Option<ViewMemory> {
  load_memory_for_inputs(inputs).ok().flatten()
}

fn load_scan_for_inputs(inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  let store_root = inputs.store_root.as_ref().ok_or_else(|| "canonical playlist scan loading requires --store-root".to_string())?;
  let lineage = read_lineage_manifest_for_inputs(&inputs.artifact_dir, inputs).map_err(|error| error.to_string())?;
  let store =
    FileRunStore::open(store_root).map_err(|error| format!("failed to open NetEase run authority {}: {error}", store_root.display()))?;
  let snapshot = futures_executor::block_on(store.load_snapshot(lineage.scan_uri.run_id()))
    .map_err(|error| format!("failed to load NetEase scan snapshot: {error}"))?
    .ok_or_else(|| format!("NetEase scan snapshot {} is missing", lineage.scan_uri.run_id()))?;
  futures_executor::block_on(read_playlist_sidebar_scan(&store, &snapshot, &lineage.scan_uri)).map_err(|error| error.to_string())
}

fn load_memory_for_inputs(inputs: &Inputs) -> Result<Option<ViewMemory>, String> {
  let store_root = inputs.store_root.as_ref().ok_or_else(|| "canonical view-memory loading requires --store-root".to_string())?;
  let lineage = read_lineage_manifest_for_inputs(&inputs.artifact_dir, inputs).map_err(|error| error.to_string())?;
  let Some(memory_uri) = lineage.memory_uri.as_ref() else {
    return Ok(None);
  };
  let store =
    FileRunStore::open(store_root).map_err(|error| format!("failed to open NetEase run authority {}: {error}", store_root.display()))?;
  let snapshot = futures_executor::block_on(store.load_snapshot(memory_uri.run_id()))
    .map_err(|error| format!("failed to load NetEase view-memory snapshot: {error}"))?
    .ok_or_else(|| format!("NetEase view-memory snapshot {} is missing", memory_uri.run_id()))?;
  futures_executor::block_on(read_view_memory(&store, &snapshot, memory_uri)).map(Some).map_err(|error| error.to_string())
}

async fn read_json<T: DeserializeOwned>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  purpose: &'static str,
) -> Result<T, NeteaseArtifactReadError> {
  let bytes = read_json_bytes(store, snapshot, uri, purpose).await?;
  serde_json::from_slice(&bytes).map_err(|source| NeteaseArtifactReadError::MalformedJson {
    uri: uri.clone(),
    message: source.to_string(),
  })
}

async fn read_json_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, NeteaseArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(NeteaseArtifactReadError::SnapshotAuthorityMismatch {
      snapshot_authority: snapshot.authority_id(),
      store_authority,
    });
  }
  if uri.run_id() != snapshot.run_id() {
    return Err(NeteaseArtifactReadError::WrongOwner {
      snapshot_run_id: snapshot.run_id(),
      artifact_run_id: uri.run_id(),
    });
  }
  let metadata = snapshot.artifacts().get(uri).ok_or_else(|| NeteaseArtifactReadError::DanglingUri { uri: uri.clone() })?.metadata();
  if metadata.purpose().as_str() != expected_purpose {
    return Err(NeteaseArtifactReadError::WrongPurpose {
      uri: uri.clone(),
      expected: expected_purpose,
      actual: metadata.purpose().as_str().to_string(),
    });
  }
  if metadata.content_type().to_string() != "application/json" {
    return Err(NeteaseArtifactReadError::WrongContentType {
      uri: uri.clone(),
      actual: metadata.content_type().to_string(),
    });
  }

  let expected_length = metadata.byte_length().get();
  if expected_length > NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(NeteaseArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: expected_length,
    });
  }
  let expected_capacity = usize::try_from(expected_length).map_err(|_| NeteaseArtifactReadError::LengthOutOfRange {
    uri: uri.clone(),
    actual: expected_length,
  })?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|source| NeteaseArtifactReadError::Allocation {
    uri: uri.clone(),
    expected: expected_length,
    source,
  })?;
  let mut reader = store.open_artifact(uri.clone()).await.map_err(|source| NeteaseArtifactReadError::Open {
    uri: uri.clone(),
    source,
  })?;
  let mut actual_length = 0_u64;
  while let Some(chunk) = reader.next().await {
    let chunk = chunk.map_err(|source| NeteaseArtifactReadError::Stream {
      uri: uri.clone(),
      source,
    })?;
    actual_length = actual_length.checked_add(chunk.len() as u64).ok_or_else(|| NeteaseArtifactReadError::PayloadTooLarge {
      uri: uri.clone(),
      limit: NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
      actual: u64::MAX,
    })?;
    if actual_length > NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      return Err(NeteaseArtifactReadError::PayloadTooLarge {
        uri: uri.clone(),
        limit: NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual: actual_length,
      });
    }
    if actual_length > expected_length {
      return Err(NeteaseArtifactReadError::LengthMismatch {
        uri: uri.clone(),
        expected: expected_length,
        actual: actual_length,
      });
    }
    bytes.extend_from_slice(&chunk);
  }
  if actual_length != expected_length {
    return Err(NeteaseArtifactReadError::LengthMismatch {
      uri: uri.clone(),
      expected: expected_length,
      actual: actual_length,
    });
  }
  let actual_digest = Sha256Digest::new(Sha256::digest(&bytes).into());
  if actual_digest != metadata.sha256() {
    return Err(NeteaseArtifactReadError::DigestMismatch {
      uri: uri.clone(),
      expected: metadata.sha256(),
      actual: actual_digest,
    });
  }
  Ok(bytes)
}

#[derive(Debug, thiserror::Error)]
pub enum NeteaseArtifactReadError {
  #[error("NetEase snapshot authority {snapshot_authority} does not match store authority {store_authority}")]
  SnapshotAuthorityMismatch {
    snapshot_authority: AuthorityId,
    store_authority: AuthorityId,
  },
  #[error("NetEase artifact URI belongs to run {artifact_run_id}, not snapshot run {snapshot_run_id}")]
  WrongOwner {
    snapshot_run_id: RunId,
    artifact_run_id: RunId,
  },
  #[error("NetEase artifact URI is not committed in the supplied snapshot: {uri}")]
  DanglingUri { uri: ArtifactUri },
  #[error("NetEase artifact {uri} has purpose {actual}, expected {expected}")]
  WrongPurpose {
    uri: ArtifactUri,
    expected: &'static str,
    actual: String,
  },
  #[error("NetEase artifact {uri} has content type {actual}, expected application/json")]
  WrongContentType { uri: ArtifactUri, actual: String },
  #[error("NetEase artifact {uri} is {actual} bytes, exceeding the {limit}-byte structured-artifact limit")]
  PayloadTooLarge {
    uri: ArtifactUri,
    limit: u64,
    actual: u64,
  },
  #[error("NetEase artifact {uri} byte length {actual} cannot be represented by this process")]
  LengthOutOfRange { uri: ArtifactUri, actual: u64 },
  #[error("failed to reserve {expected} bytes for NetEase artifact {uri}: {source}")]
  Allocation {
    uri: ArtifactUri,
    expected: u64,
    #[source]
    source: TryReserveError,
  },
  #[error("failed to open NetEase artifact {uri}: {source}")]
  Open {
    uri: ArtifactUri,
    #[source]
    source: ReadError,
  },
  #[error("failed to stream NetEase artifact {uri}: {source}")]
  Stream {
    uri: ArtifactUri,
    #[source]
    source: ArtifactReadError,
  },
  #[error("NetEase artifact {uri} length mismatch: expected {expected}, read {actual}")]
  LengthMismatch {
    uri: ArtifactUri,
    expected: u64,
    actual: u64,
  },
  #[error("NetEase artifact {uri} digest mismatch: expected {expected}, read {actual}")]
  DigestMismatch {
    uri: ArtifactUri,
    expected: Sha256Digest,
    actual: Sha256Digest,
  },
  #[error("NetEase artifact {uri} is not the expected JSON type: {message}")]
  MalformedJson { uri: ArtifactUri, message: String },
}

impl NeteaseArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let code = match self {
      Self::SnapshotAuthorityMismatch { .. } => "auv.netease.artifact.snapshot_authority_mismatch",
      Self::WrongOwner { .. } => "auv.netease.artifact.wrong_owner",
      Self::DanglingUri { .. } => "auv.netease.artifact.dangling_uri",
      Self::WrongPurpose { .. } => "auv.netease.artifact.wrong_purpose",
      Self::WrongContentType { .. } => "auv.netease.artifact.wrong_content_type",
      Self::PayloadTooLarge { .. } => NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::LengthOutOfRange { .. } => "auv.netease.artifact.length_out_of_range",
      Self::Allocation { .. } => "auv.netease.artifact.allocation_failed",
      Self::Open { .. } => "auv.netease.artifact.open_failed",
      Self::Stream { .. } => "auv.netease.artifact.stream_failed",
      Self::LengthMismatch { .. } => "auv.netease.artifact.length_mismatch",
      Self::DigestMismatch { .. } => "auv.netease.artifact.digest_mismatch",
      Self::MalformedJson { .. } => "auv.netease.artifact.malformed_json",
    };
    ErrorCode::parse(code).expect("static NetEase artifact error code is valid")
  }
}

async fn publish_json<T: Serialize>(purpose: &'static str, value: &T) -> Result<Option<ArtifactMetadata>, NeteaseArtifactPublishError> {
  let (body, digest) = serialize_json_exact(purpose, value)?;
  let length = ByteLength::new(body.len() as u64).map_err(|error| NeteaseArtifactPublishError::InvalidContract {
    purpose,
    message: error.to_string(),
  })?;
  let artifact = NewArtifact::new(
    auv_tracing::ArtifactPurpose::parse(purpose).map_err(|error| NeteaseArtifactPublishError::InvalidContract {
      purpose,
      message: error.to_string(),
    })?,
    ContentType::parse("application/json").map_err(|error| NeteaseArtifactPublishError::InvalidContract {
      purpose,
      message: error.to_string(),
    })?,
    length,
    digest,
    Attributes::empty(),
    AsyncCursor::new(body),
  );
  auv_tracing::emit_artifact!(artifact).await.map_err(|source| NeteaseArtifactPublishError::Publication { purpose, source })
}

fn serialize_json_exact<T: Serialize>(purpose: &'static str, value: &T) -> Result<(Vec<u8>, Sha256Digest), NeteaseArtifactPublishError> {
  let mut measurement = JsonMeasurement::default();
  if let Err(source) = serde_json::to_writer(&mut measurement, value) {
    if let Some(actual) = measurement.exceeded_at {
      return Err(NeteaseArtifactPublishError::PayloadTooLarge {
        purpose,
        limit: NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
        actual,
      });
    }
    return Err(NeteaseArtifactPublishError::Serialize { purpose, source });
  }
  let measured_length = usize::try_from(measurement.length).map_err(|_| NeteaseArtifactPublishError::InvalidContract {
    purpose,
    message: "measured JSON length does not fit usize".to_string(),
  })?;
  let measured_digest = Sha256Digest::new(measurement.hasher.finalize().into());
  let mut output = ExactJsonBuffer::new(purpose, measured_length)?;
  serde_json::to_writer(&mut output, value).map_err(|source| NeteaseArtifactPublishError::Serialize { purpose, source })?;
  let body = output.finish(measured_digest).ok_or(NeteaseArtifactPublishError::NondeterministicSerialization { purpose })?;
  Ok((body, measured_digest))
}

#[derive(Default)]
struct JsonMeasurement {
  length: u64,
  hasher: Sha256,
  exceeded_at: Option<u64>,
}

impl Write for JsonMeasurement {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let length = self.length.checked_add(buffer.len() as u64).ok_or_else(|| std::io::Error::other("NetEase JSON length overflow"))?;
    if length > NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
      self.exceeded_at = Some(length);
      return Err(std::io::Error::other(NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE));
    }
    self.length = length;
    self.hasher.update(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

struct ExactJsonBuffer {
  bytes: Vec<u8>,
  measured_length: usize,
  actual_length: usize,
  hasher: Sha256,
}

impl ExactJsonBuffer {
  fn new(purpose: &'static str, measured_length: usize) -> Result<Self, NeteaseArtifactPublishError> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(measured_length).map_err(|source| NeteaseArtifactPublishError::Allocation { purpose, source })?;
    Ok(Self {
      bytes,
      measured_length,
      actual_length: 0,
      hasher: Sha256::new(),
    })
  }

  fn finish(self, measured_digest: Sha256Digest) -> Option<Vec<u8>> {
    if self.actual_length != self.measured_length || Sha256Digest::new(self.hasher.finalize().into()) != measured_digest {
      return None;
    }
    Some(self.bytes)
  }
}

impl Write for ExactJsonBuffer {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    self.actual_length =
      self.actual_length.checked_add(buffer.len()).ok_or_else(|| std::io::Error::other("NetEase JSON length overflow"))?;
    self.hasher.update(buffer);
    let remaining = self.measured_length.saturating_sub(self.bytes.len());
    self.bytes.extend_from_slice(&buffer[..buffer.len().min(remaining)]);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}
