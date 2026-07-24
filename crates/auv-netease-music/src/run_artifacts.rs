//! NetEase run-scoped artifact publication and typed reads.

use auv_driver::InputActionResult;
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ArtifactWriteError, Attributes, ByteLength, ContentType, ErrorCode, JsonArtifactError,
  JsonArtifactReadError, ReadArtifactError, RunSnapshot, RunStore, ValidationError, read_artifact_bytes,
};
use auv_view::ViewBounds;
use auv_view::memory::{MemoryReadConfig, MemoryReadOutcome, StaleReason, ViewMemory};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::commands::playlist::PlaylistSelectResult;
use crate::scroll::policies::detection_motion::MotionEvidence;
use crate::{Inputs, PlaylistSidebarScan, SidebarScanStopReason, SidebarViewportObservation};

pub const PLAYLIST_SIDEBAR_SCAN_PURPOSE: &str = "auv.netease.playlist_sidebar_scan";
pub const VIEW_MEMORY_PURPOSE: &str = "auv.netease.view_memory";
pub const PLAYLIST_SELECT_RESULT_PURPOSE: &str = "auv.netease.playlist_select_result";

/// NetEase structured artifacts contain OCR/view records, not bulk media.
/// Four MiB leaves ample room above the bounded 12-scroll playlist fixtures
/// while keeping producer and reader allocation independent of the 512 MiB
/// whole-artifact ceiling.
pub const NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;
pub const NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE: &str = "auv.netease.structured_artifact.payload_too_large";

#[derive(Serialize)]
struct ArtifactPreparationFailed {
  purpose: &'static str,
  error: String,
}

impl auv_tracing::EventPayload for ArtifactPreparationFailed {
  const NAME: &'static str = "auv.netease.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarObserved {
  observation_index: usize,
  viewport_fingerprint: String,
}

impl auv_tracing::EventPayload for SidebarObserved {
  const NAME: &'static str = "auv.netease.sidebar.observed";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarScrolled {
  from_observation: usize,
  to_observation: usize,
  requested_delta: f64,
  settle_ms: u64,
  delivery_path: Option<String>,
  motion: Option<MotionEvidence>,
}

impl auv_tracing::EventPayload for SidebarScrolled {
  const NAME: &'static str = "auv.netease.sidebar.scrolled";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarScanStopped {
  reason: SidebarScanStopReason,
}

impl auv_tracing::EventPayload for SidebarScanStopped {
  const NAME: &'static str = "auv.netease.sidebar.scan_stopped";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum PlaylistSelectInputDelivered {
  SeekSidebarTop {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SeekTargetPage {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SeekBottomPadding {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectPlaylist {
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectPlaylistForegroundRetry {
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for PlaylistSelectInputDelivered {
  const NAME: &'static str = "auv.netease.playlist_select.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum PlaylistTargetResolved {
  ViewMemory { bounds: ViewBounds },
  RescanReplay { attempt: usize, bounds: ViewBounds },
  BottomPadding { attempt: usize, bounds: ViewBounds },
}

impl auv_tracing::EventPayload for PlaylistTargetResolved {
  const NAME: &'static str = "auv.netease.playlist_select.target_resolved";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum PlaylistPlayInputDelivered {
  PlayAll {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAllForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for PlaylistPlayInputDelivered {
  const NAME: &'static str = "auv.netease.playlist_play.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum DailyRecommendedInputDelivered {
  SeekSidebarTop {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectSidebarRecommend {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  OpenDailyRecommendedCard {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  OpenDailyRecommendedTitleForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAll {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAllForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for DailyRecommendedInputDelivered {
  const NAME: &'static str = "auv.netease.daily_recommended.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
pub(crate) struct DailyRecommendedPlayAllChecked {
  pub visible: bool,
}

impl auv_tracing::EventPayload for DailyRecommendedPlayAllChecked {
  const NAME: &'static str = "auv.netease.daily_recommended.play_all_checked";
  const VERSION: u32 = 1;
}

pub(crate) fn emit_sidebar_scan_events(
  observations: &[SidebarViewportObservation],
  scroll_amount: f64,
  scroll_settle_ms: u64,
  stop_reason: Option<SidebarScanStopReason>,
) {
  for (index, observation) in observations.iter().enumerate() {
    auv_tracing::emit_event!(SidebarObserved {
      observation_index: observation.observation_index,
      viewport_fingerprint: observation.viewport_fingerprint.clone(),
    });
    if let Some(next) = observations.get(index + 1) {
      auv_tracing::emit_event!(SidebarScrolled {
        from_observation: observation.observation_index,
        to_observation: next.observation_index,
        requested_delta: -scroll_amount,
        settle_ms: scroll_settle_ms,
        delivery_path: next.incoming_scroll_delivery_path.clone(),
        motion: next.scroll_motion.clone(),
      });
    }
  }
  if let Some(reason) = stop_reason {
    auv_tracing::emit_event!(SidebarScanStopped { reason });
  }
}

pub(crate) fn emit_json<T: Serialize>(purpose: &'static str, value: &T) {
  let result = ArtifactPurpose::parse(purpose).map_err(|error| error.to_string()).and_then(|purpose| {
    auv_tracing::emit_json_artifact(
      purpose,
      Attributes::empty(),
      ByteLength::new(NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static NetEase JSON limit is valid"),
      value,
    )
    .map(drop)
    .map_err(|error| error.to_string())
  });
  if let Err(error) = result {
    preparation_failed(purpose, error);
  }
}

pub(crate) fn emit_png(purpose: &'static str, image: &image::RgbaImage) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  let result = encode_png(image).and_then(|body| {
    let purpose_value = ArtifactPurpose::parse(purpose).map_err(|error| error.to_string())?;
    let content_type = ContentType::parse("image/png").map_err(|error| error.to_string())?;
    let artifact =
      auv_tracing::NewArtifact::from_bytes(purpose_value, content_type, Attributes::empty(), body).map_err(|error| error.to_string())?;
    drop(auv_tracing::emit_artifact!(artifact));
    Ok(())
  });
  if let Err(error) = result {
    preparation_failed(purpose, error);
  }
}

pub(crate) fn preparation_failed(purpose: &'static str, error: impl Into<String>) {
  auv_tracing::emit_event!(ArtifactPreparationFailed {
    purpose,
    error: error.into()
  });
}

pub(crate) fn spawn_artifact_task(operation: impl FnOnce() + Send + 'static) -> Option<std::thread::JoinHandle<()>> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let context = auv_tracing::Context::current();
  Some(std::thread::spawn(move || context.in_scope(operation)))
}

fn encode_png(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
  use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

  let mut body = Vec::new();
  body.try_reserve_exact(image.as_raw().len()).map_err(|error| format!("allocate PNG buffer failed: {error}"))?;
  PngEncoder::new(&mut body)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
    .map_err(|error| format!("encode PNG artifact failed: {error}"))?;
  Ok(body)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistArtifactPublication {
  pub scan_uri: ArtifactUri,
  pub memory: Option<ViewMemory>,
}

/// Caller-read canonical playlist data whose optional memory has been checked
/// against the scan's authority, run, app, scope, and source artifact.
#[derive(Clone, Debug)]
pub struct CanonicalPlaylistArtifacts {
  state: CanonicalPlaylistArtifactState,
  read_limits: Vec<String>,
}

#[derive(Clone, Debug)]
enum CanonicalPlaylistArtifactState {
  Unavailable,
  Available {
    scan: PlaylistSidebarScan,
    memory: Option<ViewMemory>,
  },
}

impl CanonicalPlaylistArtifacts {
  fn from_scan(scan: PlaylistSidebarScan) -> Self {
    Self {
      state: CanonicalPlaylistArtifactState::Available { scan, memory: None },
      read_limits: Vec::new(),
    }
  }

  pub fn scan(&self) -> Option<&PlaylistSidebarScan> {
    match &self.state {
      CanonicalPlaylistArtifactState::Unavailable => None,
      CanonicalPlaylistArtifactState::Available { scan, .. } => Some(scan),
    }
  }

  pub fn memory(&self) -> Option<&ViewMemory> {
    match &self.state {
      CanonicalPlaylistArtifactState::Unavailable => None,
      CanonicalPlaylistArtifactState::Available { memory, .. } => memory.as_ref(),
    }
  }

  pub fn read_limits(&self) -> &[String] {
    &self.read_limits
  }

  pub(crate) fn unavailable(read_limits: Vec<String>) -> Self {
    Self {
      state: CanonicalPlaylistArtifactState::Unavailable,
      read_limits,
    }
  }

  fn attach_memory(&mut self, memory: ViewMemory) {
    let CanonicalPlaylistArtifactState::Available { memory: slot, .. } = &mut self.state else {
      unreachable!("memory can only be attached to a canonical scan")
    };
    *slot = Some(memory);
  }

  fn push_read_limit(&mut self, limit: String) {
    self.read_limits.push(limit);
  }
}

#[derive(Debug, thiserror::Error)]
pub enum CanonicalArtifactReferenceError {
  #[error("canonical playlist scan app {actual:?} does not match requested app {expected:?}")]
  ScanAppMismatch {
    expected: String,
    actual: Option<String>,
  },
  #[error("canonical view-memory app {actual:?} does not match requested app {expected:?}")]
  MemoryAppMismatch { expected: String, actual: String },
  #[error("canonical view-memory scope {actual:?} does not match playlist sidebar scope {expected:?}")]
  MemoryScopeMismatch { expected: String, actual: String },
  #[error("canonical view-memory region {actual:?} does not match playlist sidebar scope {expected:?}")]
  MemoryRegionMismatch { expected: String, actual: String },
  #[error("canonical view-memory ID {actual:?} does not match the requested app and playlist sidebar scope {expected:?}")]
  MemoryIdMismatch { expected: String, actual: String },
  #[error("canonical view-memory is stale ({reason:?})")]
  StaleMemory { reason: StaleReason },
}

#[derive(Debug, thiserror::Error)]
pub enum NeteaseArtifactPublishError {
  #[error("invalid NetEase artifact contract for {purpose}: {source}")]
  InvalidContract {
    purpose: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("failed to construct NetEase artifact {purpose}: {source}")]
  Json {
    purpose: &'static str,
    #[source]
    source: JsonArtifactError,
  },
  #[error("failed to publish {purpose}: {source}")]
  Publication {
    purpose: &'static str,
    #[source]
    source: ArtifactWriteError,
  },
}

/// Publishes the scan and its optional view memory into the caller's current
/// run. The domain scan has already completed; publication errors never cause
/// the scan to execute again.
pub async fn persist_playlist_ls_artifacts(
  scan: &PlaylistSidebarScan,
  inputs: &Inputs,
  memory_enabled: bool,
) -> Result<Option<PlaylistArtifactPublication>, NeteaseArtifactPublishError> {
  let Some(scan_metadata) = publish_json(PLAYLIST_SIDEBAR_SCAN_PURPOSE, scan).await? else {
    return Ok(None);
  };
  let scan_uri = scan_metadata.uri().clone();
  let memory = if memory_enabled {
    crate::view_memory::try_build_writable_memory(inputs, scan, &scan_uri)
  } else {
    None
  };
  if let Some(memory) = &memory {
    let Some(_metadata) = publish_json(VIEW_MEMORY_PURPOSE, memory).await? else {
      return Ok(None);
    };
  }
  Ok(Some(PlaylistArtifactPublication { scan_uri, memory }))
}

/// Admits a playlist sidebar scan as optional tracing evidence without waiting
/// for or returning a storage receipt.
pub fn emit_playlist_sidebar_scan(scan: &PlaylistSidebarScan) {
  crate::run_artifacts::emit_json(PLAYLIST_SIDEBAR_SCAN_PURPOSE, scan);
}

/// Admits a playlist selection result as optional tracing evidence without
/// making storage part of the selection result.
pub fn emit_playlist_select_result(result: &PlaylistSelectResult) {
  crate::run_artifacts::emit_json(PLAYLIST_SELECT_RESULT_PURPOSE, result);
}

/// Publishes the exact existing playlist-select result without changing the
/// domain value or coupling its lifetime to instrumentation.
pub async fn persist_playlist_select_proof(result: &PlaylistSelectResult) -> Result<Option<ArtifactMetadata>, NeteaseArtifactPublishError> {
  publish_json(PLAYLIST_SELECT_RESULT_PURPOSE, result).await
}

pub async fn read_playlist_sidebar_scan(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<PlaylistSidebarScan, NeteaseArtifactReadError> {
  let bytes = read_json_bytes(store, snapshot, uri, PLAYLIST_SIDEBAR_SCAN_PURPOSE).await?;
  let json = std::str::from_utf8(&bytes).map_err(|source| NeteaseArtifactReadError::InvalidUtf8 {
    uri: uri.clone(),
    source,
  })?;
  crate::decode_playlist_sidebar_scan_json(json).map_err(|source| NeteaseArtifactReadError::MalformedPlaylistScan {
    uri: uri.clone(),
    source,
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

/// Reads canonical playlist inputs from one caller-owned run snapshot.
/// Invalid optional memory is omitted and reported as a read limit so it
/// cannot drive reacquisition while the independently valid scan remains usable.
pub async fn read_canonical_playlist_artifacts(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  scan_uri: &ArtifactUri,
  expected_app_id: &str,
  memory_enabled: bool,
) -> Result<CanonicalPlaylistArtifacts, NeteaseArtifactReadError> {
  let scan = read_playlist_sidebar_scan(store, snapshot, scan_uri).await?;
  validate_scan_reference(&scan, expected_app_id).map_err(|source| NeteaseArtifactReadError::InvalidReference { source })?;
  let mut artifacts = CanonicalPlaylistArtifacts::from_scan(scan);
  if !memory_enabled {
    return Ok(artifacts);
  }

  for (memory_uri, published) in snapshot.artifacts() {
    if published.metadata().purpose().as_str() != VIEW_MEMORY_PURPOSE {
      continue;
    }
    let memory = match read_view_memory(store, snapshot, memory_uri).await {
      Ok(memory) => memory,
      Err(error) => {
        artifacts.push_read_limit(format!("canonical view-memory artifact {memory_uri} read failed: {error}"));
        continue;
      }
    };
    if memory.source_scan_uri != *scan_uri {
      continue;
    }
    match validate_memory_reference(memory, expected_app_id) {
      Ok(memory) => {
        artifacts.attach_memory(memory);
        break;
      }
      Err(error) => artifacts.push_read_limit(error.to_string()),
    }
  }
  Ok(artifacts)
}

fn validate_scan_reference(scan: &PlaylistSidebarScan, expected_app_id: &str) -> Result<(), CanonicalArtifactReferenceError> {
  let actual = scan.app().app_id.clone();
  if actual.as_deref() != Some(expected_app_id) {
    return Err(CanonicalArtifactReferenceError::ScanAppMismatch {
      expected: expected_app_id.to_string(),
      actual,
    });
  }
  Ok(())
}

fn validate_memory_reference(memory: ViewMemory, expected_app_id: &str) -> Result<ViewMemory, CanonicalArtifactReferenceError> {
  if memory.app_bundle_id != expected_app_id {
    return Err(CanonicalArtifactReferenceError::MemoryAppMismatch {
      expected: expected_app_id.to_string(),
      actual: memory.app_bundle_id,
    });
  }
  let expected_scope = crate::view_memory::PLAYLIST_SIDEBAR_SCOPE_ID;
  if memory.scope_id != expected_scope {
    return Err(CanonicalArtifactReferenceError::MemoryScopeMismatch {
      expected: expected_scope.to_string(),
      actual: memory.scope_id,
    });
  }
  if memory.scope_snapshot.region_id != expected_scope {
    return Err(CanonicalArtifactReferenceError::MemoryRegionMismatch {
      expected: expected_scope.to_string(),
      actual: memory.scope_snapshot.region_id,
    });
  }
  let expected_memory_id = auv_view::memory::build_memory_id(expected_app_id, expected_scope);
  if memory.memory_id != expected_memory_id {
    return Err(CanonicalArtifactReferenceError::MemoryIdMismatch {
      expected: expected_memory_id,
      actual: memory.memory_id,
    });
  }

  let config = MemoryReadConfig {
    now_millis: crate::view_memory::system_time_millis(),
    ..MemoryReadConfig::default()
  };
  match auv_view::memory::read_memory(memory, &config, None) {
    MemoryReadOutcome::Accepted(memory) => Ok(memory),
    MemoryReadOutcome::Rejected { reason } => Err(CanonicalArtifactReferenceError::StaleMemory { reason }),
  }
}

async fn read_json<T: DeserializeOwned>(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  purpose: &'static str,
) -> Result<T, NeteaseArtifactReadError> {
  let expected_purpose = expected_artifact_purpose(purpose)?;
  auv_tracing::read_json_artifact(
    store,
    snapshot,
    uri,
    &expected_purpose,
    ByteLength::new(NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static NetEase JSON limit is valid"),
  )
  .await
  .map_err(|error| match error {
    JsonArtifactReadError::Artifact(source) => NeteaseArtifactReadError::Read { source },
    JsonArtifactReadError::Decode { source, .. } => NeteaseArtifactReadError::MalformedJson {
      uri: uri.clone(),
      source,
    },
  })
}

async fn read_json_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
) -> Result<Vec<u8>, NeteaseArtifactReadError> {
  let expected_purpose = expected_artifact_purpose(expected_purpose)?;
  let expected_content_type = ContentType::parse("application/json").expect("static JSON content type is valid");
  read_artifact_bytes(
    store,
    snapshot,
    uri,
    &expected_purpose,
    &expected_content_type,
    ByteLength::new(NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static NetEase JSON limit is valid"),
  )
  .await
  .map_err(Into::into)
}

fn expected_artifact_purpose(value: &'static str) -> Result<ArtifactPurpose, NeteaseArtifactReadError> {
  ArtifactPurpose::parse(value).map_err(|source| NeteaseArtifactReadError::InvalidExpectedPurpose { value, source })
}

#[derive(Debug, thiserror::Error)]
pub enum NeteaseArtifactReadError {
  #[error("invalid expected NetEase artifact purpose {value:?}: {source}")]
  InvalidExpectedPurpose {
    value: &'static str,
    #[source]
    source: ValidationError,
  },
  #[error("failed to read NetEase artifact: {source}")]
  Read {
    #[from]
    source: ReadArtifactError,
  },
  #[error("NetEase artifact {uri} is not valid UTF-8: {source}")]
  InvalidUtf8 {
    uri: ArtifactUri,
    #[source]
    source: std::str::Utf8Error,
  },
  #[error("NetEase playlist scan artifact {uri} is invalid: {source}")]
  MalformedPlaylistScan {
    uri: ArtifactUri,
    #[source]
    source: crate::PlaylistSidebarScanDecodeError,
  },
  #[error("NetEase artifact {uri} is not the expected JSON type: {source}")]
  MalformedJson {
    uri: ArtifactUri,
    #[source]
    source: serde_json::Error,
  },
  #[error("invalid canonical NetEase artifact reference: {source}")]
  InvalidReference {
    #[source]
    source: CanonicalArtifactReferenceError,
  },
}

impl NeteaseArtifactReadError {
  pub fn code(&self) -> ErrorCode {
    let code = match self {
      Self::InvalidExpectedPurpose { .. } => "auv.netease.artifact.invalid_reader_contract",
      Self::Read {
        source: ReadArtifactError::PayloadTooLarge { .. },
      } => NETEASE_STRUCTURED_ARTIFACT_PAYLOAD_TOO_LARGE_CODE,
      Self::Read {
        source: ReadArtifactError::SnapshotAuthorityMismatch { .. },
      } => "auv.netease.artifact.snapshot_authority_mismatch",
      Self::Read {
        source: ReadArtifactError::WrongRun { .. },
      } => "auv.netease.artifact.wrong_owner",
      Self::Read {
        source: ReadArtifactError::NotCommitted { .. },
      } => "auv.netease.artifact.dangling_uri",
      Self::Read {
        source: ReadArtifactError::WrongPurpose { .. },
      } => "auv.netease.artifact.wrong_purpose",
      Self::Read {
        source: ReadArtifactError::WrongContentType { .. },
      } => "auv.netease.artifact.wrong_content_type",
      Self::Read {
        source: ReadArtifactError::LengthOutOfRange { .. },
      } => "auv.netease.artifact.length_out_of_range",
      Self::Read {
        source: ReadArtifactError::Allocation { .. },
      } => "auv.netease.artifact.allocation_failed",
      Self::Read {
        source: ReadArtifactError::Open { .. },
      } => "auv.netease.artifact.open_failed",
      Self::Read {
        source: ReadArtifactError::Stream { .. },
      } => "auv.netease.artifact.stream_failed",
      Self::Read {
        source: ReadArtifactError::LengthMismatch { .. },
      } => "auv.netease.artifact.length_mismatch",
      Self::Read {
        source: ReadArtifactError::DigestMismatch { .. },
      } => "auv.netease.artifact.digest_mismatch",
      Self::InvalidUtf8 { .. } | Self::MalformedPlaylistScan { .. } | Self::MalformedJson { .. } => "auv.netease.artifact.malformed_json",
      Self::InvalidReference { .. } => "auv.netease.artifact.invalid_reference",
    };
    ErrorCode::parse(code).expect("static NetEase artifact error code is valid")
  }
}

async fn publish_json<T: Serialize>(purpose: &'static str, value: &T) -> Result<Option<ArtifactMetadata>, NeteaseArtifactPublishError> {
  publish_json_with_attributes(purpose, value, Attributes::empty()).await
}

async fn publish_json_with_attributes<T: Serialize>(
  purpose: &'static str,
  value: &T,
  attributes: Attributes,
) -> Result<Option<ArtifactMetadata>, NeteaseArtifactPublishError> {
  // Contexts without artifact authority must not validate or allocate bytes.
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return Ok(None);
  }
  let purpose_value =
    auv_tracing::ArtifactPurpose::parse(purpose).map_err(|source| NeteaseArtifactPublishError::InvalidContract { purpose, source })?;
  let emission = auv_tracing::emit_json_artifact(
    purpose_value,
    attributes,
    ByteLength::new(NETEASE_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static NetEase JSON limit is valid"),
    value,
  )
  .map_err(|source| NeteaseArtifactPublishError::Json { purpose, source })?;
  emission.await.map_err(|source| NeteaseArtifactPublishError::Publication { purpose, source })
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use auv_driver::{InputActionResult, InputDeliveryPath};
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};
  use auv_view::ViewBounds;

  use super::*;
  use crate::run_artifacts::{PlaylistSelectInputDelivered, PlaylistTargetResolved};

  #[test]
  fn invalid_reader_contract_retains_validation_error_source() {
    let error = expected_artifact_purpose("not_namespaced").expect_err("invalid purpose must retain its validation error");

    match &error {
      NeteaseArtifactReadError::InvalidExpectedPurpose { value, source } => {
        assert_eq!(*value, "not_namespaced");
        assert_eq!(source.to_string(), "namespaced name requires at least two segments");
      }
      other => panic!("expected typed validation error, got {other:?}"),
    }
    assert!(std::error::Error::source(&error).is_some());
  }

  #[test]
  fn invalid_publish_contract_retains_validation_error_source() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch should build");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let publication = root.in_scope(|| publish_json("not_namespaced", &"payload"));

    let error = futures_executor::block_on(root.instrument(publication)).expect_err("invalid purpose must fail publication");

    match &error {
      NeteaseArtifactPublishError::InvalidContract { purpose, source } => {
        assert_eq!(*purpose, "not_namespaced");
        assert_eq!(source.to_string(), "namespaced name requires at least two segments");
      }
      other => panic!("expected typed contract validation error, got {other:?}"),
    }
    let source = std::error::Error::source(&error).expect("validation error source");
    assert!(source.downcast_ref::<ValidationError>().is_some());
  }

  #[test]
  fn publish_json_length_error_retains_value_and_typed_source() {
    let error = NeteaseArtifactPublishError::Json {
      purpose: PLAYLIST_SELECT_RESULT_PURPOSE,
      source: JsonArtifactError::LengthOutOfRange {
        actual: u128::from(u16::MAX),
      },
    };

    match &error {
      NeteaseArtifactPublishError::Json {
        purpose,
        source: JsonArtifactError::LengthOutOfRange { actual },
      } => {
        assert_eq!(*purpose, PLAYLIST_SELECT_RESULT_PURPOSE);
        assert_eq!(*actual, u128::from(u16::MAX));
      }
      other => panic!("expected typed JSON length error, got {other:?}"),
    }
    let source = std::error::Error::source(&error).expect("JSON construction error source");
    assert!(source.downcast_ref::<JsonArtifactError>().is_some());
  }

  #[test]
  fn app_actions_are_recorded_as_typed_events_instead_of_result_steps() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch should build");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    root.in_scope(|| {
      auv_tracing::emit_event!(PlaylistTargetResolved::ViewMemory {
        bounds: ViewBounds::new(12.0, 24.0, 160.0, 32.0),
      });
      auv_tracing::emit_event!(PlaylistSelectInputDelivered::SelectPlaylist {
        bounds: ViewBounds::new(12.0, 24.0, 160.0, 32.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
      });
    });
    futures_executor::block_on(dispatch.flush()).expect("flush action events");

    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("load run").expect("run snapshot");
    let names = snapshot.events().iter().map(|event| event.schema().name().as_str()).collect::<Vec<_>>();
    assert_eq!(
      names,
      vec![
        "auv.netease.playlist_select.target_resolved",
        "auv.netease.playlist_select.input_delivered",
      ]
    );
    let payload: serde_json::Value = serde_json::from_str(snapshot.events()[1].payload().get()).expect("typed delivery payload");
    assert_eq!(payload["action"], "select_playlist");
    assert_eq!(payload["delivery"]["selected_path"], "window_targeted_mouse");
    assert!(payload.get("name").is_none());
    assert!(payload.get("steps").is_none());
    assert!(payload.get("delivery_path").is_none());
  }
}
