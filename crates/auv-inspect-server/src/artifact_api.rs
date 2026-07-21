use std::collections::{BTreeSet, HashMap};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{
  ArtifactMetadata, ArtifactUri, ArtifactWriteError, AuthorityId, CommitResult, ErrorCode, IdempotencyKey, ReadError, RunCommit, RunFact,
  RunId, StoreArtifactRequest, Timestamp, artifact_identity_conflict_error_code,
};
use auv_tracing_inspect::protocol::{
  ARTIFACT_IDENTITY_CONFLICT_ERROR, ARTIFACT_RESOLVE_MEDIA_TYPE, ARTIFACT_UPLOAD_ADMISSION_BUSY, ARTIFACT_UPLOAD_ADMISSION_HEADER,
  ARTIFACT_UPLOAD_ADMISSION_LEASE_SECONDS, ARTIFACT_UPLOAD_ADMISSION_REQUIRED_ERROR, ARTIFACT_UPLOAD_MEDIA_TYPE, AUTHORITY_ID_HEADER,
  ArtifactApiError, ArtifactUploadAdmissionId, ArtifactUploadDraft, ArtifactUploadDraftRequest, ArtifactUploadId,
  IDEMPOTENCY_MISMATCH_ERROR, RUN_MEDIA_TYPE, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact,
  decode_artifact_upload_draft_request, decode_strict,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, Request, State};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use base64::Engine;
use futures_util::{Stream, TryStreamExt};
use serde::Serialize;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::io::StreamReader;
use url::Url;

use crate::server::InspectServerState;

const MAX_ARTIFACT_JSON_BYTES: usize = 32 * 1024 * 1024;
const V1_MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const DRAFT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
const ADMISSION_LEASE: Duration = Duration::from_secs(ARTIFACT_UPLOAD_ADMISSION_LEASE_SECONDS);
const MAX_ACTIVE_DRAFTS_PER_RUN: usize = 4_096;
const MAX_ACTIVE_DRAFTS: usize = 16_384;
const MAX_PUBLISHED_RESPONSES: usize = 4_096;
const MAX_EXPIRED_TOMBSTONES: usize = 4_096;

pub(crate) struct ArtifactApiState {
  drafts: Mutex<DraftIndexes>,
  clock: Clock,
  max_artifact_bytes: u64,
  max_active_drafts_per_run: usize,
  max_active_drafts: usize,
}

impl ArtifactApiState {
  pub(crate) fn new() -> Self {
    Self {
      drafts: Mutex::new(DraftIndexes::default()),
      clock: Clock::new(),
      max_artifact_bytes: V1_MAX_ARTIFACT_BYTES,
      max_active_drafts_per_run: MAX_ACTIVE_DRAFTS_PER_RUN,
      max_active_drafts: MAX_ACTIVE_DRAFTS,
    }
  }

  #[cfg(test)]
  fn with_max_artifact_bytes(max_artifact_bytes: u64) -> Self {
    Self {
      drafts: Mutex::new(DraftIndexes::default()),
      clock: Clock::new(),
      max_artifact_bytes,
      max_active_drafts_per_run: MAX_ACTIVE_DRAFTS_PER_RUN,
      max_active_drafts: MAX_ACTIVE_DRAFTS,
    }
  }

  #[cfg(test)]
  fn with_limits(max_artifact_bytes: u64, max_active_drafts_per_run: usize, max_active_drafts: usize) -> Self {
    Self {
      drafts: Mutex::new(DraftIndexes::default()),
      clock: Clock::new(),
      max_artifact_bytes,
      max_active_drafts_per_run,
      max_active_drafts,
    }
  }

  fn prune(&self) {
    let mut indexes = self.drafts.lock().expect("artifact draft index lock");
    indexes.prune_due(self.clock.monotonic_now());
  }

  pub(crate) fn reserves(&self, run_id: RunId, key: IdempotencyKey) -> bool {
    let mut indexes = self.drafts.lock().expect("artifact draft index lock");
    indexes.prune_due(self.clock.monotonic_now());
    indexes.reserves(run_id, key)
  }
}

#[derive(Clone)]
struct Clock {
  wall_anchor: SystemTime,
  monotonic_anchor: tokio::time::Instant,
}

impl Clock {
  fn new() -> Self {
    Self {
      wall_anchor: SystemTime::now(),
      monotonic_anchor: tokio::time::Instant::now(),
    }
  }

  fn monotonic_now(&self) -> tokio::time::Instant {
    tokio::time::Instant::now()
  }

  fn deadline_and_timestamp(&self, duration: Duration) -> (tokio::time::Instant, Timestamp) {
    let now = self.monotonic_now();
    let deadline = now + duration;
    let elapsed = deadline.saturating_duration_since(self.monotonic_anchor);
    let wall = self.wall_anchor.checked_add(elapsed).unwrap_or(self.wall_anchor);
    let since_epoch = wall.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX);
    let timestamp = Timestamp::new(seconds, since_epoch.subsec_nanos()).expect("current artifact draft timestamps satisfy the run contract");
    (deadline, timestamp)
  }
}

#[derive(Default)]
struct DraftIndexes {
  by_upload: HashMap<(RunId, ArtifactUploadId), DraftRecord>,
  by_key: HashMap<(RunId, IdempotencyKey), ArtifactUploadId>,
  by_uri: HashMap<ArtifactUri, ArtifactUploadId>,
  active_by_run: HashMap<RunId, usize>,
  active_count: usize,
  published_count: usize,
  tombstone_count: usize,
  published_order: BTreeSet<DraftQueueEntry>,
  tombstone_order: BTreeSet<DraftQueueEntry>,
  deadlines: BTreeSet<DraftDeadline>,
  next_generation: u64,
  next_queue_sequence: u64,
  next_deadline_sequence: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DraftQueueEntry {
  sequence: u64,
  run_id: RunId,
  upload_id: ArtifactUploadId,
  generation: u64,
}

#[derive(Clone)]
struct DraftRecord {
  draft: ArtifactUploadDraft,
  run_id: RunId,
  key: IdempotencyKey,
  generation: u64,
  deadline_sequence: u64,
  state: DraftState,
}

#[derive(Clone)]
enum DraftState {
  Pending(PendingDraft),
  PublishedCache {
    deadline: tokio::time::Instant,
    order: u64,
  },
  Expired {
    prune_at: tokio::time::Instant,
    order: u64,
  },
}

struct DraftDeadline {
  at: tokio::time::Instant,
  sequence: u64,
  run_id: RunId,
  upload_id: ArtifactUploadId,
  generation: u64,
}

impl PartialEq for DraftDeadline {
  fn eq(&self, other: &Self) -> bool {
    self.at == other.at && self.sequence == other.sequence
  }
}

impl Eq for DraftDeadline {}

impl PartialOrd for DraftDeadline {
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for DraftDeadline {
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    self.at.cmp(&other.at).then_with(|| self.sequence.cmp(&other.sequence))
  }
}

#[derive(Clone)]
struct PendingDraft {
  authority_id: AuthorityId,
  deadline: tokio::time::Instant,
  metadata: Box<ArtifactUploadDraftRequest>,
  admission: AdmissionState,
}

#[derive(Clone, Copy)]
enum AdmissionState {
  Admitted {
    id: ArtifactUploadAdmissionId,
    lease_deadline: tokio::time::Instant,
  },
  Released(ArtifactUploadAdmissionId),
  Uploading(ArtifactUploadAdmissionId),
  // TODO(inspect-indeterminate-retention-v1): publication-unknown reservations
  // remain authoritative in memory because removing one could admit a duplicate;
  // prune them only after an owner accepts a durable reservation/retention contract.
  Indeterminate(ArtifactUploadAdmissionId),
}

impl AdmissionState {
  fn id(self) -> ArtifactUploadAdmissionId {
    match self {
      Self::Admitted { id, .. } | Self::Released(id) | Self::Uploading(id) | Self::Indeterminate(id) => id,
    }
  }
}

impl DraftIndexes {
  fn allocate_generation(&mut self) -> u64 {
    self.next_generation = self.next_generation.checked_add(1).expect("artifact draft generation counter exhausted");
    self.next_generation
  }

  fn allocate_queue_sequence(&mut self) -> u64 {
    self.next_queue_sequence = self.next_queue_sequence.checked_add(1).expect("artifact draft queue sequence exhausted");
    self.next_queue_sequence
  }

  fn schedule(&mut self, at: tokio::time::Instant, run_id: RunId, upload_id: ArtifactUploadId, generation: u64) -> u64 {
    self.next_deadline_sequence = self.next_deadline_sequence.checked_add(1).expect("artifact draft deadline counter exhausted");
    self.deadlines.insert(DraftDeadline {
      at,
      sequence: self.next_deadline_sequence,
      run_id,
      upload_id,
      generation,
    });
    self.next_deadline_sequence
  }

  fn prune_due(&mut self, now: tokio::time::Instant) {
    while self.deadlines.first().is_some_and(|entry| entry.at <= now) {
      let deadline = self.deadlines.pop_first().expect("peeked artifact draft deadline exists");
      let locator = (deadline.run_id, deadline.upload_id);
      let mut expire = false;
      let mut remove = false;
      if let Some(record) = self.by_upload.get_mut(&locator)
        && record.generation == deadline.generation
      {
        match &mut record.state {
          DraftState::Pending(pending)
            if pending.deadline == deadline.at
              && matches!(pending.admission, AdmissionState::Admitted { .. } | AdmissionState::Released(_)) =>
          {
            expire = true;
          }
          DraftState::PublishedCache {
            deadline: cache_deadline,
            ..
          } if *cache_deadline == deadline.at => {
            remove = true;
          }
          DraftState::Expired { prune_at, .. } if *prune_at == deadline.at => {
            remove = true;
          }
          _ => {}
        }
      }
      if expire {
        self.expire(deadline.run_id, deadline.upload_id);
      } else if remove {
        self.remove(deadline.run_id, deadline.upload_id);
      }
    }
  }

  fn reserves(&self, run_id: RunId, key: IdempotencyKey) -> bool {
    self.by_key.contains_key(&(run_id, key))
  }

  fn existing(&self, run_id: RunId, key: IdempotencyKey, uri: &ArtifactUri) -> Option<ArtifactUploadId> {
    self.by_key.get(&(run_id, key)).copied().or_else(|| self.by_uri.get(uri).copied())
  }

  fn record_mut(&mut self, run_id: RunId, upload_id: ArtifactUploadId, now: tokio::time::Instant) -> Option<&mut DraftRecord> {
    let record = self.by_upload.get_mut(&(run_id, upload_id))?;
    if let DraftState::Pending(pending) = &mut record.state
      && matches!(pending.admission, AdmissionState::Admitted { lease_deadline, .. } if lease_deadline <= now)
    {
      pending.admission = AdmissionState::Released(pending.admission.id());
    }
    Some(record)
  }

  fn insert(&mut self, mut record: DraftRecord, per_run_capacity: usize, global_capacity: usize) -> Result<(), ()> {
    if self.active_by_run.get(&record.run_id).copied().unwrap_or(0) >= per_run_capacity || self.active_count >= global_capacity {
      return Err(());
    }
    let upload_id = record.draft.upload_id();
    let run_id = record.run_id;
    let generation = record.generation;
    let DraftState::Pending(pending) = &record.state else {
      unreachable!("new artifact draft records are pending")
    };
    let deadline = pending.deadline;
    match pending.admission {
      AdmissionState::Admitted { .. } => {}
      _ => unreachable!("new artifact draft records own an admission"),
    }
    record.deadline_sequence = self.schedule(deadline, run_id, upload_id, generation);
    self.by_key.insert((record.run_id, record.key), upload_id);
    self.by_uri.insert(record.draft.artifact_uri().clone(), upload_id);
    self.by_upload.insert((run_id, upload_id), record);
    *self.active_by_run.entry(run_id).or_default() += 1;
    self.active_count += 1;
    Ok(())
  }

  fn remove(&mut self, run_id: RunId, upload_id: ArtifactUploadId) {
    let Some(record) = self.by_upload.remove(&(run_id, upload_id)) else {
      return;
    };
    let deadline = match &record.state {
      DraftState::Pending(pending) => pending.deadline,
      DraftState::PublishedCache { deadline, .. } => *deadline,
      DraftState::Expired { prune_at, .. } => *prune_at,
    };
    self.deadlines.remove(&DraftDeadline {
      at: deadline,
      sequence: record.deadline_sequence,
      run_id,
      upload_id,
      generation: record.generation,
    });
    match &record.state {
      DraftState::Pending(_) => self.decrement_active(run_id),
      DraftState::PublishedCache { order, .. } => {
        self.published_count -= 1;
        self.published_order.remove(&DraftQueueEntry {
          sequence: *order,
          run_id,
          upload_id,
          generation: record.generation,
        });
      }
      DraftState::Expired { order, .. } => {
        self.tombstone_count -= 1;
        self.tombstone_order.remove(&DraftQueueEntry {
          sequence: *order,
          run_id,
          upload_id,
          generation: record.generation,
        });
      }
    }
    if self.by_key.get(&(record.run_id, record.key)) == Some(&upload_id) {
      self.by_key.remove(&(record.run_id, record.key));
    }
    if self.by_uri.get(record.draft.artifact_uri()) == Some(&upload_id) {
      self.by_uri.remove(record.draft.artifact_uri());
    }
  }

  fn expire(&mut self, run_id: RunId, upload_id: ArtifactUploadId) {
    let Some(record) = self.by_upload.get(&(run_id, upload_id)) else {
      return;
    };
    let DraftState::Pending(pending) = &record.state else {
      return;
    };
    let pending_deadline = pending.deadline;
    let prune_at = pending_deadline.checked_add(DRAFT_LIFETIME).unwrap_or(pending_deadline);
    let run_id = record.run_id;
    let key = record.key;
    let uri = record.draft.artifact_uri().clone();
    let generation = record.generation;
    let deadline_sequence = record.deadline_sequence;
    let order = self.allocate_queue_sequence();
    self.deadlines.remove(&DraftDeadline {
      at: pending_deadline,
      sequence: deadline_sequence,
      run_id,
      upload_id,
      generation,
    });
    if self.by_key.get(&(run_id, key)) == Some(&upload_id) {
      self.by_key.remove(&(run_id, key));
    }
    if self.by_uri.get(&uri) == Some(&upload_id) {
      self.by_uri.remove(&uri);
    }
    if let Some(record) = self.by_upload.get_mut(&(run_id, upload_id)) {
      record.state = DraftState::Expired { prune_at, order };
    }
    self.decrement_active(run_id);
    self.tombstone_count += 1;
    self.tombstone_order.insert(DraftQueueEntry {
      sequence: order,
      run_id,
      upload_id,
      generation,
    });
    let deadline_sequence = self.schedule(prune_at, run_id, upload_id, generation);
    if let Some(record) = self.by_upload.get_mut(&(run_id, upload_id)) {
      record.deadline_sequence = deadline_sequence;
    }
    self.evict_oldest_tombstones();
  }

  fn cache_published(
    &mut self,
    run_id: RunId,
    upload_id: ArtifactUploadId,
    admission: ArtifactUploadAdmissionId,
    now: tokio::time::Instant,
  ) {
    let locator = (run_id, upload_id);
    let Some(record) = self.by_upload.get_mut(&locator) else {
      return;
    };
    let DraftState::Pending(pending) = &record.state else {
      return;
    };
    if !pending.admission.id().matches(admission) {
      return;
    }
    if pending.deadline <= now {
      self.remove(run_id, upload_id);
      return;
    }
    let deadline = pending.deadline;
    let generation = record.generation;
    let order = self.allocate_queue_sequence();
    let record = self.by_upload.get_mut(&locator).expect("published draft remains indexed");
    record.state = DraftState::PublishedCache { deadline, order };
    self.decrement_active(run_id);
    self.published_count += 1;
    self.published_order.insert(DraftQueueEntry {
      sequence: order,
      run_id,
      upload_id,
      generation,
    });
    self.evict_oldest_published();
  }

  fn decrement_active(&mut self, run_id: RunId) {
    let count = self.active_by_run.get_mut(&run_id).expect("pending artifact draft has an active run count");
    *count = count.checked_sub(1).expect("artifact draft active run count underflow");
    self.active_count = self.active_count.checked_sub(1).expect("artifact draft global active count underflow");
    if *count == 0 {
      self.active_by_run.remove(&run_id);
    }
  }

  fn evict_oldest_published(&mut self) {
    while self.published_count > MAX_PUBLISHED_RESPONSES {
      let Some(oldest) = self.published_order.first().copied() else {
        break;
      };
      let is_current = matches!(
        self.by_upload.get(&(oldest.run_id, oldest.upload_id)),
        Some(DraftRecord {
          generation,
          state: DraftState::PublishedCache { .. },
          ..
        }) if *generation == oldest.generation
      );
      if is_current {
        self.remove(oldest.run_id, oldest.upload_id);
      }
    }
  }

  fn evict_oldest_tombstones(&mut self) {
    while self.tombstone_count > MAX_EXPIRED_TOMBSTONES {
      let Some(oldest) = self.tombstone_order.first().copied() else {
        break;
      };
      let is_current = matches!(
        self.by_upload.get(&(oldest.run_id, oldest.upload_id)),
        Some(DraftRecord {
          generation,
          state: DraftState::Expired { .. },
          ..
        }) if *generation == oldest.generation
      );
      if is_current {
        self.remove(oldest.run_id, oldest.upload_id);
      }
    }
  }
}

pub(crate) fn routes() -> Router<Arc<InspectServerState>> {
  Router::new()
    .route("/v1/runs/{run_id}/artifact-uploads", post(create_draft))
    .route("/v1/runs/{run_id}/artifact-uploads/{upload_id}/content", put(upload_content))
    .route("/v1/runs/{run_id}/artifacts/{artifact_id}", get(read_artifact))
    .route("/v1/resources/artifacts/resolve", post(resolve_artifacts))
}

async fn create_draft(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<String>, PathRejection>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ArtifactFailure> {
  state.artifacts.prune();
  require_media_type(&headers, ARTIFACT_UPLOAD_MEDIA_TYPE)?;
  let Path(run_id) = path.map_err(|_| ArtifactFailure::invalid_reference())?;
  let run_id = run_id.parse::<RunId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let key = idempotency_key(&headers)?;
  let admission = upload_admission(&headers)?;
  let bytes = to_bytes(request.into_body(), MAX_ARTIFACT_JSON_BYTES).await.map_err(ArtifactFailure::from_body)?;
  let body = decode_artifact_upload_draft_request(&bytes).map_err(|error| {
    if error.is_payload_too_large() {
      ArtifactFailure::artifact_too_large()
    } else {
      ArtifactFailure::invalid_reference()
    }
  })?;
  if body.byte_length().get() > state.artifacts.max_artifact_bytes {
    return Err(ArtifactFailure::artifact_too_large());
  }
  let expected_authority = state.store.authority_id();
  if body.authority_id() != expected_authority {
    return Err(ArtifactFailure::authority_mismatch(expected_authority, body.authority_id()));
  }
  let uri = ArtifactUri::from_ids(run_id, body.artifact_id());
  let _mutation = state.mutation_arbitrator.acquire(run_id).await;

  if let Some(existing) = existing_draft(&state, run_id, key, &uri) {
    return resolve_existing_draft(&state, existing, run_id, key, admission, &body).await;
  }

  match state.store.lookup_commit(run_id, key).await.map_err(ArtifactFailure::from_read)? {
    Some(commit) if artifact_commit_matches(&commit, &body, run_id, key) => {
      let draft = published_draft(&commit, uri);
      return Ok(draft_response(StatusCode::OK, &draft, DraftAdmission::Busy));
    }
    Some(_) => return Err(ArtifactFailure::idempotency_mismatch()),
    None => {}
  }

  if let Some(snapshot) = state.store.load_snapshot(run_id).await.map_err(ArtifactFailure::from_read)? {
    if snapshot.artifacts().contains_key(&uri) {
      return Err(ArtifactFailure::artifact_identity_conflict());
    }
    if body.span_id().is_some_and(|span_id| !snapshot.spans().contains_key(&span_id)) {
      return Err(ArtifactFailure::not_found());
    }
  } else if body.span_id().is_some() {
    return Err(ArtifactFailure::not_found());
  }

  let upload_id = ArtifactUploadId::from_idempotency_key(key);
  let (deadline, expires_at) = state.artifacts.clock.deadline_and_timestamp(DRAFT_LIFETIME);
  let draft = ArtifactUploadDraft::new(upload_id, uri.clone(), expires_at);
  let mut record = DraftRecord {
    draft: draft.clone(),
    run_id,
    key,
    generation: 0,
    deadline_sequence: 0,
    state: DraftState::Pending(PendingDraft {
      authority_id: expected_authority,
      deadline,
      metadata: Box::new(body.clone()),
      admission: AdmissionState::Admitted {
        id: admission,
        lease_deadline: state.artifacts.clock.monotonic_now() + ADMISSION_LEASE,
      },
    }),
  };
  let raced = {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    let now = state.artifacts.clock.monotonic_now();
    indexes.prune_due(now);
    let mut existing = indexes.existing(run_id, key, &uri);
    if existing.is_none() {
      let replace_expired = matches!(
        indexes.by_upload.get(&(run_id, upload_id)),
        Some(DraftRecord {
          run_id: existing_run,
          key: existing_key,
          state: DraftState::Expired { .. },
          ..
        }) if *existing_run == run_id && *existing_key == key
      );
      if replace_expired {
        indexes.remove(run_id, upload_id);
      } else if indexes.by_upload.contains_key(&(run_id, upload_id)) {
        existing = Some(upload_id);
      }
    }
    if existing.is_none() {
      record.generation = indexes.allocate_generation();
      indexes
        .insert(record, state.artifacts.max_active_drafts_per_run, state.artifacts.max_active_drafts)
        .map_err(|_| ArtifactFailure::capacity())?;
    }
    existing
  };
  if let Some(existing) = raced {
    return resolve_existing_draft(&state, existing, run_id, key, admission, &body).await;
  }
  Ok(draft_response(StatusCode::CREATED, &draft, DraftAdmission::Granted(admission)))
}

fn existing_draft(state: &InspectServerState, run_id: RunId, key: IdempotencyKey, uri: &ArtifactUri) -> Option<ArtifactUploadId> {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  indexes.prune_due(state.artifacts.clock.monotonic_now());
  indexes.existing(run_id, key, uri)
}

async fn resolve_existing_draft(
  state: &InspectServerState,
  upload_id: ArtifactUploadId,
  run_id: RunId,
  key: IdempotencyKey,
  admission: ArtifactUploadAdmissionId,
  request: &ArtifactUploadDraftRequest,
) -> Result<Response, ArtifactFailure> {
  let resolution = {
    let now = state.artifacts.clock.monotonic_now();
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune_due(now);
    resolve_cached_draft(&mut indexes, upload_id, run_id, key, admission, request, now)?
  };
  match resolution {
    DraftResolution::Immediate(response) => Ok(response),
    DraftResolution::Lookup(reason) => reconcile_draft_lookup(state, run_id, key, upload_id, request, reason).await,
  }
}

enum DraftResolution {
  Immediate(Response),
  Lookup(DraftLookupReason),
}

#[derive(Clone, Copy)]
enum DraftLookupReason {
  Missing,
  PublishedCache { generation: u64 },
  Indeterminate { generation: u64 },
}

fn resolve_cached_draft(
  indexes: &mut DraftIndexes,
  upload_id: ArtifactUploadId,
  run_id: RunId,
  key: IdempotencyKey,
  admission: ArtifactUploadAdmissionId,
  request: &ArtifactUploadDraftRequest,
  now: tokio::time::Instant,
) -> Result<DraftResolution, ArtifactFailure> {
  let Some(existing) = indexes.record_mut(run_id, upload_id, now) else {
    return Ok(DraftResolution::Lookup(DraftLookupReason::Missing));
  };
  if existing.key != key {
    return Err(ArtifactFailure::artifact_identity_conflict());
  }
  if existing.draft.artifact_uri() != &ArtifactUri::from_ids(run_id, request.artifact_id()) {
    return Err(ArtifactFailure::idempotency_mismatch());
  }
  let draft = existing.draft.clone();
  let generation = existing.generation;
  let response = match &mut existing.state {
    DraftState::Pending(pending) => {
      if pending.metadata.as_ref() != request {
        return Err(ArtifactFailure::idempotency_mismatch());
      }
      if matches!(pending.admission, AdmissionState::Indeterminate(_)) {
        return Ok(DraftResolution::Lookup(DraftLookupReason::Indeterminate { generation }));
      }
      let response_admission = match pending.admission {
        AdmissionState::Admitted { id, .. } if id.matches(admission) => {
          pending.admission = AdmissionState::Admitted {
            id,
            lease_deadline: now + ADMISSION_LEASE,
          };
          DraftAdmission::Granted(admission)
        }
        AdmissionState::Released(previous) if !previous.matches(admission) => {
          let lease_deadline = now + ADMISSION_LEASE;
          pending.admission = AdmissionState::Admitted {
            id: admission,
            lease_deadline,
          };
          DraftAdmission::Granted(admission)
        }
        current if current.id().matches(admission) && !matches!(current, AdmissionState::Released(_)) => DraftAdmission::Granted(admission),
        _ => DraftAdmission::Busy,
      };
      draft_response(StatusCode::OK, &draft, response_admission)
    }
    DraftState::PublishedCache { .. } => return Ok(DraftResolution::Lookup(DraftLookupReason::PublishedCache { generation })),
    DraftState::Expired { .. } => return Err(ArtifactFailure::gone()),
  };
  Ok(DraftResolution::Immediate(response))
}

// NOTICE(inspect-draft-expiry-reconstruction-v1): RunStore commits do not retain
// the original draft expiry. Exact replay therefore lasts only while the bounded
// live response cache survives; store-only reconstruction uses committed_at.
fn published_draft(commit: &RunCommit, uri: ArtifactUri) -> ArtifactUploadDraft {
  ArtifactUploadDraft::new(ArtifactUploadId::from_idempotency_key(commit.idempotency_key()), uri, commit.committed_at())
}

async fn reconcile_draft_lookup(
  state: &InspectServerState,
  run_id: RunId,
  key: IdempotencyKey,
  upload_id: ArtifactUploadId,
  request: &ArtifactUploadDraftRequest,
  reason: DraftLookupReason,
) -> Result<Response, ArtifactFailure> {
  let lookup = state.store.lookup_commit(run_id, key).await;
  match lookup {
    Ok(Some(commit)) if artifact_commit_matches(&commit, request, run_id, key) => {
      let draft = {
        let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
        if let DraftLookupReason::Indeterminate { generation } = reason {
          let admission = indexes.by_upload.get(&(run_id, upload_id)).and_then(|record| {
            (record.generation == generation).then_some(&record.state).and_then(|state| match state {
              DraftState::Pending(PendingDraft {
                admission: AdmissionState::Indeterminate(admission),
                ..
              }) => Some(*admission),
              _ => None,
            })
          });
          if let Some(admission) = admission {
            indexes.cache_published(run_id, upload_id, admission, state.artifacts.clock.monotonic_now());
          }
        }
        indexes
          .by_upload
          .get(&(run_id, upload_id))
          .filter(|record| {
            matches!(reason, DraftLookupReason::Missing)
              || match reason {
                DraftLookupReason::PublishedCache { generation } | DraftLookupReason::Indeterminate { generation } => {
                  record.generation == generation
                }
                DraftLookupReason::Missing => true,
              }
          })
          .and_then(|record| matches!(record.state, DraftState::PublishedCache { .. }).then(|| record.draft.clone()))
      }
      .unwrap_or_else(|| published_draft(&commit, ArtifactUri::from_ids(run_id, request.artifact_id())));
      Ok(draft_response(StatusCode::OK, &draft, DraftAdmission::Busy))
    }
    Ok(Some(_)) => {
      clear_reconciled_draft(state, run_id, upload_id, reason);
      Err(ArtifactFailure::idempotency_mismatch())
    }
    Ok(None) if matches!(reason, DraftLookupReason::Indeterminate { .. }) => {
      Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")))
    }
    Ok(None) => Err(ArtifactFailure::not_found()),
    Err(_) if matches!(reason, DraftLookupReason::Indeterminate { .. }) => {
      Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")))
    }
    Err(error) => Err(ArtifactFailure::from_read(error)),
  }
}

fn clear_reconciled_draft(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, reason: DraftLookupReason) {
  let generation = match reason {
    DraftLookupReason::Missing => return,
    DraftLookupReason::PublishedCache { generation } | DraftLookupReason::Indeterminate { generation } => generation,
  };
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  if indexes.by_upload.get(&(run_id, upload_id)).is_some_and(|record| record.generation == generation) {
    indexes.remove(run_id, upload_id);
  }
}

#[derive(Clone)]
struct ActiveUpload {
  authority_id: AuthorityId,
  run_id: RunId,
  key: IdempotencyKey,
  deadline: tokio::time::Instant,
  metadata: Box<ArtifactUploadDraftRequest>,
}

enum UploadAction {
  Start(ActiveUpload),
  Indeterminate(ActiveUpload),
  RecoverPublished,
}

async fn upload_content(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ArtifactFailure> {
  state.artifacts.prune();
  let Path((run_id, upload_id)) = path.map_err(|_| ArtifactFailure::invalid_reference())?;
  let run_id = run_id.parse::<RunId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let upload_id = upload_id.parse::<ArtifactUploadId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let admission = upload_admission(&headers)?;
  let controls = match parse_upload_control_headers(&headers) {
    Ok(controls) => controls,
    Err(error) => {
      release_unstarted_admission(&state, run_id, upload_id, admission);
      return Err(error);
    }
  };
  let expected_authority = state.store.authority_id();
  let action = {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune_due(state.artifacts.clock.monotonic_now());
    match indexes.record_mut(run_id, upload_id, state.artifacts.clock.monotonic_now()) {
      None => UploadAction::RecoverPublished,
      Some(record) => {
        if record.run_id != run_id {
          return Err(ArtifactFailure::not_found());
        }
        match &mut record.state {
          DraftState::Expired { .. } => return Err(ArtifactFailure::gone()),
          DraftState::PublishedCache { .. } => UploadAction::RecoverPublished,
          DraftState::Pending(pending) => {
            if pending.authority_id != expected_authority {
              return Err(ArtifactFailure::authority_mismatch(expected_authority, pending.authority_id));
            }
            if !pending.admission.id().matches(admission) {
              return Err(ArtifactFailure::artifact_identity_conflict());
            }
            if let Err(error) = controls.validate(&pending.metadata) {
              if matches!(pending.admission, AdmissionState::Admitted { .. }) {
                pending.admission = AdmissionState::Released(admission);
              }
              return Err(error);
            }
            let active = ActiveUpload {
              authority_id: pending.authority_id,
              run_id: record.run_id,
              key: record.key,
              deadline: pending.deadline,
              metadata: pending.metadata.clone(),
            };
            match pending.admission {
              AdmissionState::Admitted { .. } => {
                pending.admission = AdmissionState::Uploading(admission);
                UploadAction::Start(active)
              }
              AdmissionState::Indeterminate(_) => UploadAction::Indeterminate(active),
              AdmissionState::Released(_) | AdmissionState::Uploading(_) => {
                return Err(ArtifactFailure::artifact_identity_conflict());
              }
            }
          }
        }
      }
    }
  };

  match action {
    UploadAction::RecoverPublished => return recover_published_upload(&state, run_id, upload_id, &controls).await,
    UploadAction::Indeterminate(active) => return indeterminate_replay(&state, upload_id, admission, &active).await,
    UploadAction::Start(active) => publish_upload(state, upload_id, admission, active, request).await,
  }
}

async fn publish_upload(
  state: Arc<InspectServerState>,
  upload_id: ArtifactUploadId,
  admission: ArtifactUploadAdmissionId,
  active: ActiveUpload,
  request: Request,
) -> Result<Response, ArtifactFailure> {
  let upload_reset = UploadResetGuard {
    state: state.clone(),
    run_id: active.run_id,
    upload_id,
    admission,
    body_complete: Arc::new(AtomicBool::new(false)),
  };
  let store_request = StoreArtifactRequest::new(
    active.authority_id,
    active.run_id,
    active.key,
    active.metadata.artifact_id(),
    active.metadata.span_id(),
    active.metadata.purpose().clone(),
    active.metadata.content_type().clone(),
    active.metadata.byte_length(),
    active.metadata.sha256(),
    active.metadata.attributes().clone(),
  );
  let mut body = Box::pin(request.into_body().into_data_stream());
  let body_complete = upload_reset.body_complete.clone();
  let limit_exceeded = Arc::new(AtomicBool::new(false));
  let stream_limit_exceeded = limit_exceeded.clone();
  let max_artifact_bytes = state.artifacts.max_artifact_bytes;
  let mut observed_bytes = 0_u64;
  let stream = futures_util::stream::poll_fn(move |context| match body.as_mut().poll_next(context) {
    Poll::Ready(Some(Ok(bytes))) => {
      let Some(next_observed) = observed_bytes.checked_add(bytes.len() as u64) else {
        stream_limit_exceeded.store(true, Ordering::Release);
        return Poll::Ready(Some(Err(io::Error::other("artifact body exceeds the configured limit"))));
      };
      if next_observed > max_artifact_bytes {
        stream_limit_exceeded.store(true, Ordering::Release);
        return Poll::Ready(Some(Err(io::Error::other("artifact body exceeds the configured limit"))));
      }
      observed_bytes = next_observed;
      Poll::Ready(Some(Ok(bytes)))
    }
    Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(io::Error::other(error)))),
    Poll::Ready(None) => {
      body_complete.store(true, Ordering::Release);
      Poll::Ready(None)
    }
    Poll::Pending => Poll::Pending,
  });
  let reader = StreamReader::new(stream).compat();
  let mut write = state.store.write_artifact(store_request, Box::pin(reader));
  let result = tokio::select! {
    biased;
    result = &mut write => Some(result),
    _ = tokio::time::sleep_until(active.deadline) => None,
  };
  let Some(result) = result else {
    if upload_reset.body_complete.load(Ordering::Acquire) {
      mark_indeterminate(&state, active.run_id, upload_id, admission);
      return Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")));
    }
    expire_upload(&state, active.run_id, upload_id, admission);
    return Err(ArtifactFailure::gone());
  };
  if limit_exceeded.load(Ordering::Acquire) {
    release_upload(&state, active.run_id, upload_id, admission);
    return Err(ArtifactFailure::artifact_too_large());
  }
  match result {
    Ok(CommitResult::Appended(commit)) => {
      cache_published_draft(&state, active.run_id, upload_id, admission);
      Ok(run_json(StatusCode::CREATED, &commit))
    }
    Ok(CommitResult::Replayed(commit)) => {
      cache_published_draft(&state, active.run_id, upload_id, admission);
      Ok(run_json(StatusCode::OK, &commit))
    }
    Err(ArtifactWriteError::PublicationUnknown(_)) => {
      mark_indeterminate(&state, active.run_id, upload_id, admission);
      match state.store.lookup_commit(active.run_id, active.key).await {
        Ok(Some(commit)) if artifact_commit_matches(&commit, &active.metadata, active.run_id, active.key) => {
          cache_published_draft(&state, active.run_id, upload_id, admission);
          Ok(run_json(StatusCode::OK, &commit))
        }
        Ok(Some(_)) => {
          clear_upload_reservation(&state, active.run_id, upload_id, admission);
          Err(ArtifactFailure::idempotency_mismatch())
        }
        Ok(None) | Err(_) => Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown"))),
      }
    }
    Err(ArtifactWriteError::Rejected(code)) if code == artifact_identity_conflict_error_code() => {
      clear_upload_reservation(&state, active.run_id, upload_id, admission);
      Err(ArtifactFailure::artifact_identity_conflict())
    }
    Err(error) => {
      release_upload(&state, active.run_id, upload_id, admission);
      Err(ArtifactFailure::from_write(error))
    }
  }
}

async fn indeterminate_replay(
  state: &InspectServerState,
  upload_id: ArtifactUploadId,
  admission: ArtifactUploadAdmissionId,
  active: &ActiveUpload,
) -> Result<Response, ArtifactFailure> {
  match state.store.lookup_commit(active.run_id, active.key).await {
    Ok(Some(commit)) if artifact_commit_matches(&commit, &active.metadata, active.run_id, active.key) => {
      cache_published_draft(state, active.run_id, upload_id, admission);
      Ok(run_json(StatusCode::OK, &commit))
    }
    Ok(Some(_)) => {
      clear_upload_reservation(state, active.run_id, upload_id, admission);
      Err(ArtifactFailure::idempotency_mismatch())
    }
    Ok(None) | Err(_) => Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown"))),
  }
}

async fn recover_published_upload(
  state: &InspectServerState,
  run_id: RunId,
  upload_id: ArtifactUploadId,
  controls: &UploadControlHeaders,
) -> Result<Response, ArtifactFailure> {
  let authority_id = state.store.authority_id();
  let key = upload_id.to_idempotency_key();
  let Some(commit) = state.store.lookup_commit(run_id, key).await.map_err(ArtifactFailure::from_read)? else {
    return Err(ArtifactFailure::not_found());
  };
  if commit.authority_id() != authority_id || commit.run_id() != run_id || commit.idempotency_key() != key {
    return Err(ArtifactFailure::from_read(ReadError::Integrity(error_code("auv.inspect.published_commit_identity_invalid"))));
  }
  let Some(metadata) = published_artifact_metadata(&commit, run_id) else {
    return Err(ArtifactFailure::idempotency_mismatch());
  };
  controls.validate_metadata(metadata)?;
  Ok(run_json(StatusCode::OK, &commit))
}

fn published_artifact_metadata(commit: &RunCommit, run_id: RunId) -> Option<&ArtifactMetadata> {
  let [RunFact::ArtifactPublished(published)] = commit.facts() else {
    return None;
  };
  (published.metadata().uri().run_id() == run_id).then_some(published.metadata())
}

fn cache_published_draft(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, admission: ArtifactUploadAdmissionId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  indexes.cache_published(run_id, upload_id, admission, state.artifacts.clock.monotonic_now());
}

fn release_unstarted_admission(
  state: &InspectServerState,
  run_id: RunId,
  upload_id: ArtifactUploadId,
  admission: ArtifactUploadAdmissionId,
) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let Some(record) = indexes.by_upload.get_mut(&(run_id, upload_id)) else {
    return;
  };
  if let DraftState::Pending(pending) = &mut record.state
    && matches!(pending.admission, AdmissionState::Admitted { id: current, .. } if current.matches(admission))
  {
    pending.admission = AdmissionState::Released(admission);
  }
}

fn release_upload(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, admission: ArtifactUploadAdmissionId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let Some(record) = indexes.by_upload.get_mut(&(run_id, upload_id)) else {
    return;
  };
  let mut expire = false;
  if let DraftState::Pending(pending) = &mut record.state
    && matches!(pending.admission, AdmissionState::Uploading(current) if current.matches(admission))
  {
    pending.admission = AdmissionState::Released(admission);
    expire = pending.deadline <= state.artifacts.clock.monotonic_now();
  }
  if expire {
    indexes.expire(run_id, upload_id);
  }
}

fn mark_indeterminate(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, admission: ArtifactUploadAdmissionId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let Some(record) = indexes.by_upload.get_mut(&(run_id, upload_id)) else {
    return;
  };
  if let DraftState::Pending(pending) = &mut record.state
    && matches!(pending.admission, AdmissionState::Uploading(current) if current.matches(admission))
  {
    pending.admission = AdmissionState::Indeterminate(admission);
  }
}

fn clear_upload_reservation(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, admission: ArtifactUploadAdmissionId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let remove = matches!(
    indexes.by_upload.get(&(run_id, upload_id)).map(|record| &record.state),
    Some(DraftState::Pending(pending))
      if matches!(pending.admission, AdmissionState::Uploading(current) | AdmissionState::Indeterminate(current) if current.matches(admission))
  );
  if remove {
    indexes.remove(run_id, upload_id);
  }
}

fn expire_upload(state: &InspectServerState, run_id: RunId, upload_id: ArtifactUploadId, admission: ArtifactUploadAdmissionId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let should_expire = matches!(
    indexes.by_upload.get(&(run_id, upload_id)).map(|record| &record.state),
    Some(DraftState::Pending(pending))
      if matches!(pending.admission, AdmissionState::Uploading(current) if current.matches(admission))
  );
  if should_expire {
    indexes.expire(run_id, upload_id);
  }
}

struct UploadResetGuard {
  state: Arc<InspectServerState>,
  run_id: RunId,
  upload_id: ArtifactUploadId,
  admission: ArtifactUploadAdmissionId,
  body_complete: Arc<AtomicBool>,
}

impl Drop for UploadResetGuard {
  fn drop(&mut self) {
    let mut indexes = self.state.artifacts.drafts.lock().expect("artifact draft index lock");
    let Some(record) = indexes.by_upload.get_mut(&(self.run_id, self.upload_id)) else {
      return;
    };
    let DraftState::Pending(pending) = &mut record.state else {
      return;
    };
    if !matches!(pending.admission, AdmissionState::Uploading(current) if current.matches(self.admission)) {
      return;
    }
    let body_complete = self.body_complete.load(Ordering::Acquire);
    pending.admission = if body_complete {
      AdmissionState::Indeterminate(self.admission)
    } else {
      AdmissionState::Released(self.admission)
    };
    let expire = !body_complete && pending.deadline <= self.state.artifacts.clock.monotonic_now();
    if expire {
      indexes.expire(self.run_id, self.upload_id);
    }
  }
}

async fn read_artifact(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ArtifactFailure> {
  state.artifacts.prune();
  let Path((run_id, artifact_id)) = path.map_err(|_| ArtifactFailure::invalid_reference())?;
  let run_id = run_id.parse::<RunId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let artifact_id = artifact_id.parse().map_err(|_| ArtifactFailure::invalid_reference())?;
  let uri = ArtifactUri::from_ids(run_id, artifact_id);
  let snapshot = state.store.load_snapshot(run_id).await.map_err(ArtifactFailure::from_read)?.ok_or_else(ArtifactFailure::not_found)?;
  let metadata = snapshot.artifacts().get(&uri).map(|published| published.metadata().clone()).ok_or_else(ArtifactFailure::not_found)?;
  let reader = state.store.open_artifact(uri).await.map_err(ArtifactFailure::from_read)?;
  let stream = reader.map_err(|error| io::Error::other(error.to_string()));
  let mut response = Response::new(Body::from_stream(stream));
  response
    .headers_mut()
    .insert(CONTENT_TYPE, HeaderValue::from_str(&metadata.content_type().to_string()).expect("validated content type is a header value"));
  response
    .headers_mut()
    .insert(CONTENT_LENGTH, HeaderValue::from_str(&metadata.byte_length().get().to_string()).expect("byte length is a header value"));
  response
    .headers_mut()
    .insert("Content-Digest", HeaderValue::from_str(&content_digest(metadata.sha256())).expect("digest is a header value"));
  Ok(response)
}

async fn resolve_artifacts(
  State(state): State<Arc<InspectServerState>>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ArtifactFailure> {
  resolve_artifacts_inner(state, headers, request).await.map_err(ArtifactFailure::for_resolver)
}

async fn resolve_artifacts_inner(state: Arc<InspectServerState>, headers: HeaderMap, request: Request) -> Result<Response, ArtifactFailure> {
  state.artifacts.prune();
  require_media_type(&headers, ARTIFACT_RESOLVE_MEDIA_TYPE)?;
  let bytes = to_bytes(request.into_body(), MAX_ARTIFACT_JSON_BYTES).await.map_err(ArtifactFailure::from_body)?;
  let request = decode_strict::<ResolveArtifactsRequest>(&bytes).map_err(|_| ArtifactFailure::invalid_reference())?;
  let expected_authority = state.store.authority_id();
  if request.authority_id() != expected_authority {
    return Err(ArtifactFailure::authority_mismatch(expected_authority, request.authority_id()));
  }
  let base_url =
    state.artifact_origin.as_ref().ok_or_else(|| ArtifactFailure::unavailable(error_code("auv.inspect.artifact_origin_unavailable")))?;

  let mut by_run: HashMap<RunId, Vec<ArtifactUri>> = HashMap::new();
  for uri in request.uris() {
    by_run.entry(uri.run_id()).or_default().push(uri.clone());
  }
  let mut metadata_by_uri: HashMap<ArtifactUri, Option<ArtifactMetadata>> = HashMap::new();
  for (run_id, uris) in by_run {
    let snapshot = match state.store.load_snapshot(run_id).await {
      Ok(snapshot) => snapshot,
      Err(ReadError::NotFound) => None,
      Err(error) => return Err(ArtifactFailure::from_read(error)),
    };
    for uri in uris {
      let metadata = snapshot.as_ref().and_then(|snapshot| snapshot.artifacts().get(&uri)).map(|published| published.metadata().clone());
      metadata_by_uri.insert(uri, metadata);
    }
  }

  let results = request
    .uris()
    .iter()
    .map(|uri| match metadata_by_uri.get(uri).and_then(Option::as_ref) {
      Some(metadata) => ResolvedArtifact::Available {
        uri: uri.clone(),
        content_type: metadata.content_type().clone(),
        byte_length: metadata.byte_length(),
        sha256: metadata.sha256(),
        content_url: artifact_content_url(base_url, uri),
      },
      None => ResolvedArtifact::NotFound { uri: uri.clone() },
    })
    .collect();
  Ok(json(StatusCode::OK, ARTIFACT_RESOLVE_MEDIA_TYPE, &ResolveArtifactsResponse::new(results)))
}

fn artifact_content_url(base_url: &Url, uri: &ArtifactUri) -> Url {
  base_url
    .join(&format!("v1/runs/{}/artifacts/{}", uri.run_id(), uri.artifact_id()))
    .expect("validated IDs form an absolute artifact content URL")
}

fn artifact_commit_matches(commit: &RunCommit, request: &ArtifactUploadDraftRequest, run_id: RunId, key: IdempotencyKey) -> bool {
  if commit.authority_id() != request.authority_id()
    || commit.run_id() != run_id
    || commit.idempotency_key() != key
    || commit.facts().len() != 1
  {
    return false;
  }
  let RunFact::ArtifactPublished(published) = &commit.facts()[0] else {
    return false;
  };
  let metadata = published.metadata();
  published.span_id() == request.span_id()
    && metadata.uri() == &ArtifactUri::from_ids(run_id, request.artifact_id())
    && metadata.purpose() == request.purpose()
    && metadata.content_type() == request.content_type()
    && metadata.byte_length() == request.byte_length()
    && metadata.sha256() == request.sha256()
    && metadata.attributes() == request.attributes()
}

fn idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, ArtifactFailure> {
  exactly_one_header(headers, "idempotency-key")?
    .to_str()
    .ok()
    .ok_or_else(ArtifactFailure::invalid_reference)?
    .parse()
    .map_err(|_| ArtifactFailure::invalid_reference())
}

fn upload_admission(headers: &HeaderMap) -> Result<ArtifactUploadAdmissionId, ArtifactFailure> {
  let mut values = headers.get_all(ARTIFACT_UPLOAD_ADMISSION_HEADER).iter();
  let Some(value) = values.next() else {
    return Err(ArtifactFailure::admission_required());
  };
  if values.next().is_some() {
    return Err(ArtifactFailure::invalid_reference());
  }
  value.to_str().ok().ok_or_else(ArtifactFailure::invalid_reference)?.parse().map_err(|_| ArtifactFailure::invalid_reference())
}

fn require_media_type(headers: &HeaderMap, expected: &str) -> Result<(), ArtifactFailure> {
  let mut values = headers.get_all(CONTENT_TYPE).iter();
  let Some(value) = values.next() else {
    return Err(ArtifactFailure::unsupported_media_type());
  };
  if values.next().is_some() {
    return Err(ArtifactFailure::invalid_reference());
  }
  if value.to_str().ok() == Some(expected) {
    Ok(())
  } else {
    Err(ArtifactFailure::unsupported_media_type())
  }
}

struct UploadControlHeaders {
  content_type: String,
  content_digest: auv_tracing::Sha256Digest,
  content_length: Option<u64>,
}

impl UploadControlHeaders {
  fn validate(&self, metadata: &ArtifactUploadDraftRequest) -> Result<(), ArtifactFailure> {
    self.validate_values(metadata.content_type(), metadata.sha256(), metadata.byte_length())
  }

  fn validate_metadata(&self, metadata: &ArtifactMetadata) -> Result<(), ArtifactFailure> {
    self.validate_values(metadata.content_type(), metadata.sha256(), metadata.byte_length())
  }

  fn validate_values(
    &self,
    content_type: &auv_tracing::ContentType,
    sha256: auv_tracing::Sha256Digest,
    byte_length: auv_tracing::ByteLength,
  ) -> Result<(), ArtifactFailure> {
    if self.content_type != content_type.to_string() {
      return Err(ArtifactFailure::unsupported_media_type());
    }
    if self.content_digest != sha256 {
      return Err(ArtifactFailure::integrity(error_code("auv.inspect.content_digest_mismatch")));
    }
    if self.content_length.is_some_and(|length| length != byte_length.get()) {
      return Err(ArtifactFailure::integrity(error_code("auv.inspect.content_length_mismatch")));
    }
    Ok(())
  }
}

fn parse_upload_control_headers(headers: &HeaderMap) -> Result<UploadControlHeaders, ArtifactFailure> {
  let mut media_types = headers.get_all(CONTENT_TYPE).iter();
  let content_type = media_types.next().ok_or_else(ArtifactFailure::unsupported_media_type)?;
  if media_types.next().is_some() {
    return Err(ArtifactFailure::invalid_reference());
  }
  let content_type = content_type.to_str().map_err(|_| ArtifactFailure::invalid_reference())?.to_owned();
  let content_digest = parse_content_digest(headers)?;
  let mut lengths = headers.get_all(CONTENT_LENGTH).iter();
  let content_length = lengths
    .next()
    .map(|value| value.to_str().map_err(|_| ArtifactFailure::invalid_reference())?.parse().map_err(|_| ArtifactFailure::invalid_reference()))
    .transpose()?;
  if lengths.next().is_some() {
    return Err(ArtifactFailure::invalid_reference());
  }
  Ok(UploadControlHeaders {
    content_type,
    content_digest,
    content_length,
  })
}

fn parse_content_digest(headers: &HeaderMap) -> Result<auv_tracing::Sha256Digest, ArtifactFailure> {
  let value = exactly_one_header(headers, "content-digest")?.to_str().ok().ok_or_else(ArtifactFailure::invalid_reference)?;
  let encoded = value.strip_prefix("sha-256=:").and_then(|value| value.strip_suffix(':')).ok_or_else(ArtifactFailure::invalid_reference)?;
  let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ArtifactFailure::invalid_reference())?;
  let bytes: [u8; 32] = bytes.try_into().map_err(|_| ArtifactFailure::invalid_reference())?;
  Ok(auv_tracing::Sha256Digest::new(bytes))
}

fn exactly_one_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a HeaderValue, ArtifactFailure> {
  let mut values = headers.get_all(name).iter();
  let value = values.next().ok_or_else(ArtifactFailure::invalid_reference)?;
  if values.next().is_some() {
    return Err(ArtifactFailure::invalid_reference());
  }
  Ok(value)
}

fn content_digest(digest: auv_tracing::Sha256Digest) -> String {
  format!("sha-256=:{}:", base64::engine::general_purpose::STANDARD.encode(digest.as_bytes()))
}

#[derive(Clone, Copy)]
enum DraftAdmission {
  Granted(ArtifactUploadAdmissionId),
  Busy,
}

fn draft_response(status: StatusCode, draft: &ArtifactUploadDraft, admission: DraftAdmission) -> Response {
  let mut response = json(status, ARTIFACT_UPLOAD_MEDIA_TYPE, draft);
  let value = match admission {
    DraftAdmission::Granted(admission) => HeaderValue::from_str(&admission.to_string()).expect("validated admission ID is a header value"),
    DraftAdmission::Busy => HeaderValue::from_static(ARTIFACT_UPLOAD_ADMISSION_BUSY),
  };
  response.headers_mut().insert(ARTIFACT_UPLOAD_ADMISSION_HEADER, value);
  response
}

fn run_json(status: StatusCode, value: &impl Serialize) -> Response {
  json(status, RUN_MEDIA_TYPE, value)
}

fn json(status: StatusCode, media_type: &'static str, value: &impl Serialize) -> Response {
  let mut response = Response::new(Body::from(serde_json::to_vec(value).expect("validated Inspect value must encode as JSON")));
  *response.status_mut() = status;
  response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(media_type));
  response
}

struct ArtifactFailure {
  status: StatusCode,
  body: ArtifactApiError,
  media_type: &'static str,
  authority_id: Option<AuthorityId>,
}

impl ArtifactFailure {
  fn new(status: StatusCode, code: &str) -> Self {
    Self {
      status,
      body: ArtifactApiError {
        error: error_code(code),
      },
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      authority_id: None,
    }
  }

  fn invalid_reference() -> Self {
    Self::new(StatusCode::BAD_REQUEST, "auv.inspect.invalid_reference")
  }

  fn unsupported_media_type() -> Self {
    Self::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "auv.inspect.unsupported_media_type")
  }

  fn admission_required() -> Self {
    Self::new(StatusCode::PRECONDITION_REQUIRED, ARTIFACT_UPLOAD_ADMISSION_REQUIRED_ERROR)
  }

  fn authority_mismatch(expected: AuthorityId, _received: AuthorityId) -> Self {
    let mut failure = Self::new(StatusCode::CONFLICT, "auv.inspect.authority_mismatch");
    failure.authority_id = Some(expected);
    failure
  }

  fn idempotency_mismatch() -> Self {
    Self::new(StatusCode::CONFLICT, IDEMPOTENCY_MISMATCH_ERROR)
  }

  fn artifact_identity_conflict() -> Self {
    Self::new(StatusCode::CONFLICT, ARTIFACT_IDENTITY_CONFLICT_ERROR)
  }

  fn not_found() -> Self {
    Self::new(StatusCode::NOT_FOUND, "auv.inspect.not_found")
  }

  fn gone() -> Self {
    Self::new(StatusCode::GONE, "auv.inspect.upload_expired")
  }

  fn artifact_too_large() -> Self {
    Self::new(StatusCode::PAYLOAD_TOO_LARGE, "auv.inspect.artifact_too_large")
  }

  fn integrity(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::UNPROCESSABLE_ENTITY,
      body: ArtifactApiError { error: code },
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      authority_id: None,
    }
  }

  fn rejected(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      body: ArtifactApiError { error: code },
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      authority_id: None,
    }
  }

  fn unavailable(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::SERVICE_UNAVAILABLE,
      body: ArtifactApiError { error: code },
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      authority_id: None,
    }
  }

  fn capacity() -> Self {
    Self::unavailable(error_code("auv.inspect.upload_capacity_exhausted"))
  }

  fn for_resolver(mut self) -> Self {
    self.media_type = ARTIFACT_RESOLVE_MEDIA_TYPE;
    self
  }

  fn from_body(error: axum::Error) -> Self {
    if std::error::Error::source(&error).is_some_and(|source| source.is::<http_body_util::LengthLimitError>()) {
      Self::new(StatusCode::PAYLOAD_TOO_LARGE, "auv.inspect.artifact_json_too_large")
    } else {
      Self::invalid_reference()
    }
  }

  fn from_write(error: ArtifactWriteError) -> Self {
    match error {
      ArtifactWriteError::AuthorityMismatch { expected, received } => Self::authority_mismatch(expected, received),
      ArtifactWriteError::IdempotencyMismatch => Self::idempotency_mismatch(),
      ArtifactWriteError::Rejected(code) => Self::rejected(code),
      ArtifactWriteError::Integrity(code) => Self::integrity(code),
      ArtifactWriteError::Unavailable(code) => Self::unavailable(code),
      ArtifactWriteError::PublicationUnknown(_) => Self::unavailable(error_code("auv.inspect.publication_unknown")),
    }
  }

  fn from_read(error: ReadError) -> Self {
    match error {
      ReadError::NotFound => Self::not_found(),
      ReadError::Forbidden => Self::new(StatusCode::FORBIDDEN, "auv.inspect.forbidden"),
      ReadError::InvalidReference(code) => Self {
        status: StatusCode::BAD_REQUEST,
        body: ArtifactApiError { error: code },
        media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
        authority_id: None,
      },
      ReadError::HistoryGap { .. } => Self::new(StatusCode::GONE, "auv.inspect.history_gap"),
      ReadError::CursorAhead { .. } => Self::new(StatusCode::CONFLICT, "auv.inspect.cursor_ahead"),
      ReadError::Unavailable(code) => Self::unavailable(code),
      ReadError::Integrity(code) => Self {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: ArtifactApiError { error: code },
        media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
        authority_id: None,
      },
    }
  }
}

impl IntoResponse for ArtifactFailure {
  fn into_response(self) -> Response {
    let mut response = json(self.status, self.media_type, &self.body);
    if let Some(authority_id) = self.authority_id {
      response
        .headers_mut()
        .insert(AUTHORITY_ID_HEADER, HeaderValue::from_str(&authority_id.to_string()).expect("validated authority ID is a header value"));
    }
    response
  }
}

fn error_code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("static Inspect artifact error code is valid")
}

#[cfg(test)]
mod tests {
  use auv_tracing::{MemoryRunStore, RunStore};
  use axum::body::Bytes;
  use futures_util::io::Cursor;
  use serde_json::json;
  use tower::ServiceExt;

  use super::*;
  use crate::server::RunMutationArbitrator;

  const AUTHORITY: &str = "019f8b1e-4b2d-7a00-8f00-0000000000aa";
  const RUN: &str = "019f8b1e-4b2d-7a00-8f00-000000000001";
  const ARTIFACT: &str = "019f8b1e-4b2d-7a00-8f00-000000000002";
  const KEY: &str = "019f8b1e-4b2d-7a00-8f00-000000000006";
  const ADMISSION: &str = "019f8b1e-4b2d-7a00-8f00-000000000008";
  const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
  const ABC_CONTENT_DIGEST: &str = "sha-256=:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=:";

  fn test_app(store: MemoryRunStore, artifacts: ArtifactApiState) -> (Router, Arc<InspectServerState>) {
    let state = Arc::new(InspectServerState {
      store: Arc::new(store.clone()),
      artifacts,
      artifact_origin: None,
      mutation_arbitrator: RunMutationArbitrator::new(),
    });
    (routes().with_state(state.clone()), state)
  }

  fn test_draft_request(artifact_id: &str, key: &str, admission: &str) -> Request {
    test_draft_request_for(RUN, artifact_id, key, admission)
  }

  fn test_draft_request_for(run_id: &str, artifact_id: &str, key: &str, admission: &str) -> Request {
    Request::builder()
      .method("POST")
      .uri(format!("/v1/runs/{run_id}/artifact-uploads"))
      .header(CONTENT_TYPE, ARTIFACT_UPLOAD_MEDIA_TYPE)
      .header("Idempotency-Key", key)
      .header(ARTIFACT_UPLOAD_ADMISSION_HEADER, admission)
      .body(Body::from(
        serde_json::to_vec(&json!({
          "authority_id": AUTHORITY,
          "artifact_id": artifact_id,
          "purpose": "display.capture",
          "content_type": "text/plain",
          "byte_length": 3,
          "sha256": ABC_SHA256,
          "attributes": {},
        }))
        .unwrap(),
      ))
      .unwrap()
  }

  async fn create_test_draft(app: &Router) -> ArtifactUploadDraft {
    let draft_request = test_draft_request(ARTIFACT, KEY, ADMISSION);
    let draft = app.clone().oneshot(draft_request).await.unwrap();
    decode_strict::<ArtifactUploadDraft>(&to_bytes(draft.into_body(), 1024 * 1024).await.unwrap()).unwrap()
  }

  async fn response_json(response: Response) -> serde_json::Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
  }

  fn pending_record(
    indexes: &mut DraftIndexes,
    run_id: RunId,
    key: IdempotencyKey,
    artifact_id: auv_tracing::ArtifactId,
    admission: ArtifactUploadAdmissionId,
  ) -> DraftRecord {
    let deadline = tokio::time::Instant::now() + DRAFT_LIFETIME;
    DraftRecord {
      draft: ArtifactUploadDraft::new(
        ArtifactUploadId::from_idempotency_key(key),
        ArtifactUri::from_ids(run_id, artifact_id),
        Timestamp::new(1, 0).unwrap(),
      ),
      run_id,
      key,
      generation: indexes.allocate_generation(),
      deadline_sequence: 0,
      state: DraftState::Pending(PendingDraft {
        authority_id: AUTHORITY.parse().unwrap(),
        deadline,
        metadata: Box::new(ArtifactUploadDraftRequest::new(
          AUTHORITY.parse().unwrap(),
          artifact_id,
          None,
          auv_tracing::ArtifactPurpose::parse("display.capture").unwrap(),
          auv_tracing::ContentType::parse("text/plain").unwrap(),
          auv_tracing::ByteLength::new(3).unwrap(),
          ABC_SHA256.parse().unwrap(),
          auv_tracing::Attributes::empty(),
        )),
        admission: AdmissionState::Admitted {
          id: admission,
          lease_deadline: deadline,
        },
      }),
    }
  }

  fn upload_request(draft: &ArtifactUploadDraft, body: Body) -> Request {
    Request::builder()
      .method("PUT")
      .uri(format!("/v1/runs/{RUN}/artifact-uploads/{}/content", draft.upload_id()))
      .header(CONTENT_TYPE, "text/plain")
      .header("Content-Digest", ABC_CONTENT_DIGEST)
      .header(ARTIFACT_UPLOAD_ADMISSION_HEADER, ADMISSION)
      .body(body)
      .unwrap()
  }

  #[tokio::test]
  async fn chunked_body_above_the_deployment_limit_is_413_without_publication() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, _) = test_app(store.clone(), ArtifactApiState::with_max_artifact_bytes(3));
    let draft = create_test_draft(&app).await;
    let body = Body::from_stream(futures_util::stream::iter([
      Ok::<_, io::Error>(Bytes::from_static(b"abc")),
      Ok(Bytes::from_static(b"d")),
    ]));

    let response = app.oneshot(upload_request(&draft, body)).await.unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(store.lookup_commit(RUN.parse().unwrap(), KEY.parse().unwrap()).await.unwrap().is_none());
  }

  #[tokio::test(start_paused = true)]
  async fn successful_publication_caches_only_the_original_response_until_its_deadline() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store, ArtifactApiState::new());
    let draft = create_test_draft(&app).await;

    let response = app.oneshot(upload_request(&draft, Body::from("abc"))).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    {
      let indexes = state.artifacts.drafts.lock().unwrap();
      assert_eq!(indexes.by_upload.len(), 1);
      assert!(matches!(indexes.by_upload.values().next().map(|record| &record.state), Some(DraftState::PublishedCache { .. })));
    }

    tokio::time::advance(DRAFT_LIFETIME).await;
    state.artifacts.prune();
    let indexes = state.artifacts.drafts.lock().unwrap();
    assert!(indexes.by_upload.is_empty());
    assert!(indexes.by_key.is_empty());
    assert!(indexes.by_uri.is_empty());
  }

  #[tokio::test(start_paused = true)]
  async fn unrelated_request_prunes_every_due_draft_entry() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store, ArtifactApiState::new());
    create_test_draft(&app).await;
    tokio::time::advance(DRAFT_LIFETIME + DRAFT_LIFETIME).await;

    let response = app
      .oneshot(Request::builder().uri(format!("/v1/runs/{RUN}/artifacts/019f8b1e-4b2d-7a00-8f00-000000000099")).body(Body::empty()).unwrap())
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let indexes = state.artifacts.drafts.lock().unwrap();
    assert!(indexes.by_upload.is_empty());
    assert!(indexes.by_key.is_empty());
    assert!(indexes.by_uri.is_empty());
    assert!(indexes.deadlines.is_empty());
  }

  #[tokio::test(start_paused = true)]
  async fn abandoned_and_indeterminate_drafts_consume_the_hard_capacity() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store, ArtifactApiState::with_limits(V1_MAX_ARTIFACT_BYTES, 1, 1));
    let draft = create_test_draft(&app).await;

    let abandoned_capacity = app
      .clone()
      .oneshot(test_draft_request(
        "019f8b1e-4b2d-7a00-8f00-000000000003",
        "019f8b1e-4b2d-7a00-8f00-000000000007",
        "019f8b1e-4b2d-7a00-8f00-000000000009",
      ))
      .await
      .unwrap();
    assert_eq!(abandoned_capacity.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response_json(abandoned_capacity).await, json!({"error":"auv.inspect.upload_capacity_exhausted"}));

    {
      let mut indexes = state.artifacts.drafts.lock().unwrap();
      let record = indexes.by_upload.get_mut(&(RUN.parse().unwrap(), draft.upload_id())).unwrap();
      let DraftState::Pending(pending) = &mut record.state else {
        panic!("new draft must be pending");
      };
      pending.admission = AdmissionState::Indeterminate(ADMISSION.parse().unwrap());
    }
    tokio::time::advance(DRAFT_LIFETIME + DRAFT_LIFETIME).await;

    let response = app
      .oneshot(test_draft_request(
        "019f8b1e-4b2d-7a00-8f00-000000000004",
        "019f8b1e-4b2d-7a00-8f00-00000000000b",
        "019f8b1e-4b2d-7a00-8f00-00000000000c",
      ))
      .await
      .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response_json(response).await, json!({"error":"auv.inspect.upload_capacity_exhausted"}));
    let indexes = state.artifacts.drafts.lock().unwrap();
    assert_eq!(indexes.by_upload.len(), 1);
    assert!(matches!(
      indexes.by_upload.values().next().map(|record| &record.state),
      Some(DraftState::Pending(PendingDraft {
        admission: AdmissionState::Indeterminate(_),
        ..
      }))
    ));
  }

  #[tokio::test(start_paused = true)]
  async fn global_active_capacity_rejects_unrelated_runs_until_expiry_releases_every_slot() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store, ArtifactApiState::with_limits(V1_MAX_ARTIFACT_BYTES, 2, 3));
    for _ in 0..3 {
      let run_id = RunId::new().to_string();
      let artifact_id = auv_tracing::ArtifactId::new().to_string();
      let key = IdempotencyKey::new().to_string();
      let admission = ArtifactUploadAdmissionId::new().to_string();
      let response = app.clone().oneshot(test_draft_request_for(&run_id, &artifact_id, &key, &admission)).await.unwrap();
      assert_eq!(response.status(), StatusCode::CREATED);
    }
    assert_eq!(state.artifacts.drafts.lock().unwrap().active_count, 3);

    let blocked = app
      .clone()
      .oneshot(test_draft_request_for(
        &RunId::new().to_string(),
        &auv_tracing::ArtifactId::new().to_string(),
        &IdempotencyKey::new().to_string(),
        &ArtifactUploadAdmissionId::new().to_string(),
      ))
      .await
      .unwrap();
    assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response_json(blocked).await, json!({"error":"auv.inspect.upload_capacity_exhausted"}));

    tokio::time::advance(DRAFT_LIFETIME).await;
    let released = app
      .oneshot(test_draft_request_for(
        &RunId::new().to_string(),
        &auv_tracing::ArtifactId::new().to_string(),
        &IdempotencyKey::new().to_string(),
        &ArtifactUploadAdmissionId::new().to_string(),
      ))
      .await
      .unwrap();
    assert_eq!(released.status(), StatusCode::CREATED);
    assert_eq!(state.artifacts.drafts.lock().unwrap().active_count, 1);
  }

  #[tokio::test(start_paused = true)]
  async fn indeterminate_reservation_remains_bounded_by_global_capacity() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store, ArtifactApiState::with_limits(V1_MAX_ARTIFACT_BYTES, 2, 1));
    let draft = create_test_draft(&app).await;
    {
      let mut indexes = state.artifacts.drafts.lock().unwrap();
      let record = indexes.by_upload.get_mut(&(RUN.parse().unwrap(), draft.upload_id())).unwrap();
      let DraftState::Pending(pending) = &mut record.state else {
        panic!("new draft must be pending");
      };
      pending.admission = AdmissionState::Indeterminate(ADMISSION.parse().unwrap());
    }
    tokio::time::advance(DRAFT_LIFETIME + DRAFT_LIFETIME).await;

    let blocked = app
      .oneshot(test_draft_request_for(
        &RunId::new().to_string(),
        &auv_tracing::ArtifactId::new().to_string(),
        &IdempotencyKey::new().to_string(),
        &ArtifactUploadAdmissionId::new().to_string(),
      ))
      .await
      .unwrap();

    assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(state.artifacts.drafts.lock().unwrap().active_count, 1);
  }

  #[test]
  fn active_capacity_is_counted_per_run() {
    let mut indexes = DraftIndexes::default();
    let first =
      pending_record(&mut indexes, RunId::new(), IdempotencyKey::new(), auv_tracing::ArtifactId::new(), ArtifactUploadAdmissionId::new());
    let second =
      pending_record(&mut indexes, RunId::new(), IdempotencyKey::new(), auv_tracing::ArtifactId::new(), ArtifactUploadAdmissionId::new());

    assert!(indexes.insert(first, 1, 2).is_ok());
    assert!(indexes.insert(second, 1, 2).is_ok());
  }

  #[test]
  fn four_thousand_ninety_six_published_responses_do_not_consume_active_capacity() {
    let mut indexes = DraftIndexes::default();
    let run_id = RunId::new();
    for _ in 0..MAX_PUBLISHED_RESPONSES {
      let admission = ArtifactUploadAdmissionId::new();
      let record = pending_record(&mut indexes, run_id, IdempotencyKey::new(), auv_tracing::ArtifactId::new(), admission);
      let upload_id = record.draft.upload_id();
      assert!(indexes.insert(record, MAX_ACTIVE_DRAFTS_PER_RUN, 1).is_ok());
      indexes.cache_published(run_id, upload_id, admission, tokio::time::Instant::now());
    }

    let next = pending_record(&mut indexes, run_id, IdempotencyKey::new(), auv_tracing::ArtifactId::new(), ArtifactUploadAdmissionId::new());
    assert!(indexes.insert(next, MAX_ACTIVE_DRAFTS_PER_RUN, 1).is_ok());
    assert_eq!(indexes.published_count, MAX_PUBLISHED_RESPONSES);
    assert_eq!(indexes.published_order.len(), MAX_PUBLISHED_RESPONSES);
    assert_eq!(indexes.deadlines.len(), MAX_PUBLISHED_RESPONSES + 1);
    assert_eq!(indexes.active_count, 1);
  }

  #[test]
  fn tombstones_are_bounded_without_consuming_active_capacity() {
    let mut indexes = DraftIndexes::default();
    for _ in 0..=MAX_EXPIRED_TOMBSTONES {
      let run_id = RunId::new();
      let record =
        pending_record(&mut indexes, run_id, IdempotencyKey::new(), auv_tracing::ArtifactId::new(), ArtifactUploadAdmissionId::new());
      let upload_id = record.draft.upload_id();
      assert!(indexes.insert(record, 1, 1).is_ok());
      indexes.expire(run_id, upload_id);
    }

    assert_eq!(
      indexes.by_upload.values().filter(|record| matches!(record.state, DraftState::Expired { .. })).count(),
      MAX_EXPIRED_TOMBSTONES
    );
    assert_eq!(indexes.tombstone_count, MAX_EXPIRED_TOMBSTONES);
    assert_eq!(indexes.tombstone_order.len(), MAX_EXPIRED_TOMBSTONES);
    assert_eq!(indexes.deadlines.len(), MAX_EXPIRED_TOMBSTONES);
    assert_eq!(indexes.active_count, 0);
  }

  #[tokio::test]
  async fn indeterminate_mismatch_clears_the_run_capacity_before_conflict() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store.clone(), ArtifactApiState::with_limits(V1_MAX_ARTIFACT_BYTES, 1, 1));
    let draft = create_test_draft(&app).await;
    {
      let mut indexes = state.artifacts.drafts.lock().unwrap();
      let record = indexes.by_upload.get_mut(&(RUN.parse().unwrap(), draft.upload_id())).unwrap();
      let DraftState::Pending(pending) = &mut record.state else {
        panic!("new draft must be pending");
      };
      pending.admission = AdmissionState::Indeterminate(ADMISSION.parse().unwrap());
    }
    store
      .commit(
        auv_tracing::RunCommitRequest::new(
          AUTHORITY.parse().unwrap(),
          RUN.parse().unwrap(),
          KEY.parse().unwrap(),
          vec![auv_tracing::RunMutation::StartSpan(
            auv_tracing::SpanStarted::new(
              auv_tracing::SpanId::new(),
              None,
              None,
              auv_tracing::SpanName::parse("auv.test.mismatch").unwrap(),
              Timestamp::new(1, 0).unwrap(),
              auv_tracing::Attributes::empty(),
            ),
          )],
        )
        .unwrap(),
      )
      .await
      .unwrap();

    let conflict = app.clone().oneshot(test_draft_request(ARTIFACT, KEY, ADMISSION)).await.unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(state.artifacts.drafts.lock().unwrap().active_count, 0);

    let artifact_id = auv_tracing::ArtifactId::new().to_string();
    let key = IdempotencyKey::new().to_string();
    let admission = ArtifactUploadAdmissionId::new().to_string();
    let next = app.oneshot(test_draft_request(&artifact_id, &key, &admission)).await.unwrap();
    assert_eq!(next.status(), StatusCode::CREATED);
  }

  #[tokio::test]
  async fn indeterminate_matching_lookup_moves_the_response_out_of_active_capacity() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store.clone(), ArtifactApiState::with_limits(V1_MAX_ARTIFACT_BYTES, 1, 1));
    let draft = create_test_draft(&app).await;
    {
      let mut indexes = state.artifacts.drafts.lock().unwrap();
      let record = indexes.by_upload.get_mut(&(RUN.parse().unwrap(), draft.upload_id())).unwrap();
      let DraftState::Pending(pending) = &mut record.state else {
        panic!("new draft must be pending");
      };
      pending.admission = AdmissionState::Indeterminate(ADMISSION.parse().unwrap());
    }
    store
      .write_artifact(
        StoreArtifactRequest::new(
          AUTHORITY.parse().unwrap(),
          RUN.parse().unwrap(),
          KEY.parse().unwrap(),
          ARTIFACT.parse().unwrap(),
          None,
          auv_tracing::ArtifactPurpose::parse("display.capture").unwrap(),
          auv_tracing::ContentType::parse("text/plain").unwrap(),
          auv_tracing::ByteLength::new(3).unwrap(),
          ABC_SHA256.parse().unwrap(),
          auv_tracing::Attributes::empty(),
        ),
        Box::pin(Cursor::new(b"abc".to_vec())),
      )
      .await
      .unwrap();

    let replay = app.clone().oneshot(test_draft_request(ARTIFACT, KEY, ADMISSION)).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(replay.headers().get(ARTIFACT_UPLOAD_ADMISSION_HEADER).unwrap(), ARTIFACT_UPLOAD_ADMISSION_BUSY);
    assert_eq!(state.artifacts.drafts.lock().unwrap().active_count, 0);

    let artifact_id = auv_tracing::ArtifactId::new().to_string();
    let key = IdempotencyKey::new().to_string();
    let admission = ArtifactUploadAdmissionId::new().to_string();
    let next = app.oneshot(test_draft_request(&artifact_id, &key, &admission)).await.unwrap();
    assert_eq!(next.status(), StatusCode::CREATED);
  }

  #[tokio::test]
  async fn disappearing_observed_draft_reconstructs_from_store_under_run_arbitration() {
    let store = MemoryRunStore::new(AUTHORITY.parse().unwrap());
    let (app, state) = test_app(store.clone(), ArtifactApiState::new());
    let draft = create_test_draft(&app).await;
    let run_id: RunId = RUN.parse().unwrap();
    let key: IdempotencyKey = KEY.parse().unwrap();
    let artifact_id: auv_tracing::ArtifactId = ARTIFACT.parse().unwrap();
    let request = ArtifactUploadDraftRequest::new(
      AUTHORITY.parse().unwrap(),
      artifact_id,
      None,
      auv_tracing::ArtifactPurpose::parse("display.capture").unwrap(),
      auv_tracing::ContentType::parse("text/plain").unwrap(),
      auv_tracing::ByteLength::new(3).unwrap(),
      ABC_SHA256.parse().unwrap(),
      auv_tracing::Attributes::empty(),
    );
    store
      .write_artifact(
        StoreArtifactRequest::new(
          AUTHORITY.parse().unwrap(),
          run_id,
          key,
          artifact_id,
          None,
          auv_tracing::ArtifactPurpose::parse("display.capture").unwrap(),
          auv_tracing::ContentType::parse("text/plain").unwrap(),
          auv_tracing::ByteLength::new(3).unwrap(),
          ABC_SHA256.parse().unwrap(),
          auv_tracing::Attributes::empty(),
        ),
        Box::pin(Cursor::new(b"abc".to_vec())),
      )
      .await
      .unwrap();
    let _mutation = state.mutation_arbitrator.acquire(run_id).await;
    assert_eq!(existing_draft(&state, run_id, key, draft.artifact_uri()), Some(draft.upload_id()));

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let remover_barrier = barrier.clone();
    let remover_state = state.clone();
    let upload_id = draft.upload_id();
    let remover = tokio::spawn(async move {
      remover_barrier.wait().await;
      remover_state.artifacts.drafts.lock().unwrap().remove(run_id, upload_id);
    });
    barrier.wait().await;
    remover.await.unwrap();

    let response = match resolve_existing_draft(&state, upload_id, run_id, key, ADMISSION.parse().unwrap(), &request).await {
      Ok(response) => response,
      Err(error) => panic!("draft reconstruction failed with {}", error.status),
    };

    assert_eq!(response.status(), StatusCode::OK);
  }
}
