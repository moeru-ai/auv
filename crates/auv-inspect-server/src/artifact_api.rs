use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_tracing::{
  ArtifactMetadata, ArtifactUri, ArtifactWriteError, AuthorityId, CommitResult, ErrorCode, IdempotencyKey, ReadError, RunCommit, RunFact,
  RunId, StoreArtifactRequest, Timestamp,
};
use auv_tracing_inspect::protocol::{
  ARTIFACT_RESOLVE_MEDIA_TYPE, ARTIFACT_UPLOAD_MEDIA_TYPE, ArtifactApiError, ArtifactUploadDraft, ArtifactUploadDraftRequest,
  ArtifactUploadId, RUN_MEDIA_TYPE, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact, decode_strict,
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
const DRAFT_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) struct ArtifactApiState {
  drafts: Mutex<DraftIndexes>,
  clock: Clock,
}

impl ArtifactApiState {
  pub(crate) fn new() -> Self {
    Self {
      drafts: Mutex::new(DraftIndexes::default()),
      clock: Clock::new(),
    }
  }

  pub(crate) fn reserves(&self, run_id: RunId, key: IdempotencyKey) -> bool {
    let mut indexes = self.drafts.lock().expect("artifact draft index lock");
    indexes.prune(self.clock.monotonic_now());
    indexes.by_key.contains_key(&(run_id, key))
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
  by_upload: HashMap<ArtifactUploadId, DraftRecord>,
  by_key: HashMap<(RunId, IdempotencyKey), ArtifactUploadId>,
  by_uri: HashMap<ArtifactUri, ArtifactUploadId>,
}

#[derive(Clone)]
struct DraftRecord {
  draft: ArtifactUploadDraft,
  run_id: RunId,
  key: IdempotencyKey,
  authority_id: AuthorityId,
  deadline: tokio::time::Instant,
  metadata: Box<ArtifactUploadDraftRequest>,
  status: DraftStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftStatus {
  /// One successful POST response owns permission to start the PUT.
  Admitted,
  /// A definite pre-publication failure released permission for a later POST.
  Released,
  Uploading,
  // TODO(inspect-indeterminate-retention-v1): publication-unknown reservations
  // remain authoritative in memory because removing one could admit a duplicate;
  // prune them only after an owner accepts a durable reservation/retention contract.
  Indeterminate,
  Published,
  Expired,
}

impl DraftIndexes {
  fn prune(&mut self, now: tokio::time::Instant) {
    let unpublished_expired = self
      .by_upload
      .iter()
      .filter_map(|(upload_id, record)| {
        (record.deadline <= now && matches!(record.status, DraftStatus::Admitted | DraftStatus::Released)).then_some((
          *upload_id,
          record.run_id,
          record.key,
          record.draft.artifact_uri().clone(),
        ))
      })
      .collect::<Vec<_>>();
    for (upload_id, run_id, key, uri) in unpublished_expired {
      if self.by_key.get(&(run_id, key)) == Some(&upload_id) {
        self.by_key.remove(&(run_id, key));
      }
      if self.by_uri.get(&uri) == Some(&upload_id) {
        self.by_uri.remove(&uri);
      }
      if let Some(record) = self.by_upload.get_mut(&upload_id) {
        record.status = DraftStatus::Expired;
      }
    }

    let removable = self
      .by_upload
      .iter()
      .filter_map(|(upload_id, record)| match record.status {
        DraftStatus::Published if record.deadline <= now => Some(*upload_id),
        DraftStatus::Expired if record.deadline.checked_add(DRAFT_LIFETIME).is_some_and(|retention_deadline| retention_deadline <= now) => {
          Some(*upload_id)
        }
        _ => None,
      })
      .collect::<Vec<_>>();
    for upload_id in removable {
      self.remove(upload_id);
    }
  }

  fn insert(&mut self, record: DraftRecord) {
    let upload_id = record.draft.upload_id();
    self.by_key.insert((record.run_id, record.key), upload_id);
    self.by_uri.insert(record.draft.artifact_uri().clone(), upload_id);
    self.by_upload.insert(upload_id, record);
  }

  fn remove(&mut self, upload_id: ArtifactUploadId) {
    let Some(record) = self.by_upload.remove(&upload_id) else {
      return;
    };
    if self.by_key.get(&(record.run_id, record.key)) == Some(&upload_id) {
      self.by_key.remove(&(record.run_id, record.key));
    }
    if self.by_uri.get(record.draft.artifact_uri()) == Some(&upload_id) {
      self.by_uri.remove(record.draft.artifact_uri());
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
  require_media_type(&headers, ARTIFACT_UPLOAD_MEDIA_TYPE)?;
  let Path(run_id) = path.map_err(|_| ArtifactFailure::invalid_reference())?;
  let run_id = run_id.parse::<RunId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let key = idempotency_key(&headers)?;
  let bytes = to_bytes(request.into_body(), MAX_ARTIFACT_JSON_BYTES).await.map_err(ArtifactFailure::from_body)?;
  let body = decode_strict::<ArtifactUploadDraftRequest>(&bytes).map_err(|_| ArtifactFailure::invalid_reference())?;
  let expected_authority = state.store.authority_id();
  if body.authority_id() != expected_authority {
    return Err(ArtifactFailure::authority_mismatch(expected_authority, body.authority_id()));
  }
  let uri = ArtifactUri::from_ids(run_id, body.artifact_id());
  let _mutation = state.mutation_arbitrator.acquire(run_id).await;

  if let Some(existing) = existing_draft(&state, run_id, key, &uri) {
    return resolve_existing_draft(&state, existing, key, &body).await;
  }

  match state.store.lookup_commit(run_id, key).await.map_err(ArtifactFailure::from_read)? {
    Some(commit) if artifact_commit_matches(&commit, &body, run_id, key) => {
      let draft = install_published_draft(&state, run_id, key, expected_authority, uri, body.clone())?;
      return Ok(artifact_json(StatusCode::OK, &draft));
    }
    Some(_) => return Err(ArtifactFailure::conflict()),
    None => {}
  }

  if let Some(snapshot) = state.store.load_snapshot(run_id).await.map_err(ArtifactFailure::from_read)? {
    if snapshot.artifacts().contains_key(&uri) {
      return Err(ArtifactFailure::conflict());
    }
    if body.span_id().is_some_and(|span_id| !snapshot.spans().contains_key(&span_id)) {
      return Err(ArtifactFailure::not_found());
    }
  } else if body.span_id().is_some() {
    return Err(ArtifactFailure::not_found());
  }

  let upload_id = ArtifactUploadId::new();
  let (deadline, expires_at) = state.artifacts.clock.deadline_and_timestamp(DRAFT_LIFETIME);
  let draft = ArtifactUploadDraft::new(upload_id, uri.clone(), expires_at);
  let record = DraftRecord {
    draft: draft.clone(),
    run_id,
    key,
    authority_id: expected_authority,
    deadline,
    metadata: Box::new(body.clone()),
    status: DraftStatus::Admitted,
  };
  let raced = {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune(state.artifacts.clock.monotonic_now());
    let existing = indexes.by_key.get(&(run_id, key)).or_else(|| indexes.by_uri.get(&uri)).copied();
    if existing.is_none() {
      indexes.insert(record);
    }
    existing
  };
  if let Some(existing) = raced {
    return resolve_existing_draft(&state, existing, key, &body).await;
  }
  Ok(artifact_json(StatusCode::CREATED, &draft))
}

fn existing_draft(state: &InspectServerState, run_id: RunId, key: IdempotencyKey, uri: &ArtifactUri) -> Option<ArtifactUploadId> {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  indexes.prune(state.artifacts.clock.monotonic_now());
  indexes.by_key.get(&(run_id, key)).or_else(|| indexes.by_uri.get(uri)).copied()
}

async fn resolve_existing_draft(
  state: &InspectServerState,
  upload_id: ArtifactUploadId,
  key: IdempotencyKey,
  request: &ArtifactUploadDraftRequest,
) -> Result<Response, ArtifactFailure> {
  let published = {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune(state.artifacts.clock.monotonic_now());
    let existing = indexes.by_upload.get_mut(&upload_id).ok_or_else(ArtifactFailure::not_found)?;
    if existing.run_id != existing.draft.artifact_uri().run_id() || existing.key != key || existing.metadata.as_ref() != request {
      return Err(ArtifactFailure::conflict());
    }
    match existing.status {
      DraftStatus::Released => {
        existing.status = DraftStatus::Admitted;
        return Ok(artifact_json(StatusCode::OK, &existing.draft));
      }
      DraftStatus::Admitted | DraftStatus::Uploading | DraftStatus::Indeterminate => {
        return Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")));
      }
      DraftStatus::Published => existing.clone(),
      DraftStatus::Expired => return Err(ArtifactFailure::gone()),
    }
  };
  match state.store.lookup_commit(published.run_id, published.key).await.map_err(ArtifactFailure::from_read)? {
    Some(commit) if artifact_commit_matches(&commit, request, published.run_id, published.key) => {
      Ok(artifact_json(StatusCode::OK, &published.draft))
    }
    Some(_) => Err(ArtifactFailure::conflict()),
    None => Err(ArtifactFailure::unavailable(error_code("auv.inspect.published_commit_missing"))),
  }
}

fn install_published_draft(
  state: &InspectServerState,
  run_id: RunId,
  key: IdempotencyKey,
  authority_id: AuthorityId,
  uri: ArtifactUri,
  metadata: ArtifactUploadDraftRequest,
) -> Result<ArtifactUploadDraft, ArtifactFailure> {
  let (deadline, expires_at) = state.artifacts.clock.deadline_and_timestamp(DRAFT_LIFETIME);
  let draft = ArtifactUploadDraft::new(ArtifactUploadId::new(), uri, expires_at);
  let record = DraftRecord {
    draft: draft.clone(),
    run_id,
    key,
    authority_id,
    deadline,
    metadata: Box::new(metadata),
    status: DraftStatus::Published,
  };
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  indexes.prune(state.artifacts.clock.monotonic_now());
  if indexes.by_key.contains_key(&(run_id, key)) || indexes.by_uri.contains_key(record.draft.artifact_uri()) {
    return Err(ArtifactFailure::conflict());
  }
  indexes.insert(record);
  Ok(draft)
}

async fn upload_content(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
  headers: HeaderMap,
  request: Request,
) -> Result<Response, ArtifactFailure> {
  let Path((run_id, upload_id)) = path.map_err(|_| ArtifactFailure::invalid_reference())?;
  let run_id = run_id.parse::<RunId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let upload_id = upload_id.parse::<ArtifactUploadId>().map_err(|_| ArtifactFailure::invalid_reference())?;
  let record = {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune(state.artifacts.clock.monotonic_now());
    indexes.by_upload.get(&upload_id).cloned().ok_or_else(ArtifactFailure::not_found)?
  };
  if record.run_id != run_id {
    return Err(ArtifactFailure::not_found());
  }
  let expected_authority = state.store.authority_id();
  if record.authority_id != expected_authority {
    return Err(ArtifactFailure::authority_mismatch(expected_authority, record.authority_id));
  }
  let controls = match parse_upload_control_headers(&headers) {
    Ok(controls) => controls,
    Err(error) => {
      release_unstarted_admission(&state, upload_id);
      return Err(error);
    }
  };
  if let Err(error) = controls.validate(&record.metadata) {
    release_unstarted_admission(&state, upload_id);
    return Err(error);
  }
  match record.status {
    DraftStatus::Published => return published_replay(&state, &record).await,
    DraftStatus::Expired => return Err(ArtifactFailure::gone()),
    DraftStatus::Uploading | DraftStatus::Released => return Err(ArtifactFailure::conflict()),
    DraftStatus::Indeterminate => return indeterminate_replay(&state, &record).await,
    DraftStatus::Admitted => {}
  }
  {
    let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
    indexes.prune(state.artifacts.clock.monotonic_now());
    let current = indexes.by_upload.get_mut(&upload_id).ok_or_else(ArtifactFailure::not_found)?;
    match current.status {
      DraftStatus::Admitted => current.status = DraftStatus::Uploading,
      DraftStatus::Expired => return Err(ArtifactFailure::gone()),
      _ => return Err(ArtifactFailure::conflict()),
    }
  }
  if state.artifacts.clock.monotonic_now() >= record.deadline {
    expire_upload(&state, upload_id);
    return Err(ArtifactFailure::gone());
  }
  let upload_reset = UploadResetGuard {
    state: state.clone(),
    upload_id,
    body_complete: Arc::new(AtomicBool::new(false)),
  };

  let store_request = StoreArtifactRequest::new(
    record.authority_id,
    record.run_id,
    record.key,
    record.metadata.artifact_id(),
    record.metadata.span_id(),
    record.metadata.purpose().clone(),
    record.metadata.content_type().clone(),
    record.metadata.byte_length(),
    record.metadata.sha256(),
    record.metadata.attributes().clone(),
  );
  let mut body = Box::pin(request.into_body().into_data_stream());
  let body_complete = upload_reset.body_complete.clone();
  let stream = futures_util::stream::poll_fn(move |context| match body.as_mut().poll_next(context) {
    Poll::Ready(None) => {
      body_complete.store(true, Ordering::Release);
      Poll::Ready(None)
    }
    polled => polled,
  })
  .map_err(io::Error::other);
  let reader = StreamReader::new(stream).compat();
  let result = tokio::time::timeout_at(record.deadline, state.store.write_artifact(store_request, Box::pin(reader))).await;
  let result = match result {
    Ok(result) if state.artifacts.clock.monotonic_now() < record.deadline => result,
    Ok(_) | Err(_) => {
      if upload_reset.body_complete.load(Ordering::Acquire) {
        mark_indeterminate(&state, upload_id);
        return Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")));
      }
      expire_upload(&state, upload_id);
      return Err(ArtifactFailure::gone());
    }
  };
  match result {
    Ok(CommitResult::Appended(commit)) => {
      mark_published(&state, upload_id);
      Ok(run_json(StatusCode::CREATED, &commit))
    }
    Ok(CommitResult::Replayed(commit)) => {
      mark_published(&state, upload_id);
      Ok(run_json(StatusCode::OK, &commit))
    }
    Err(ArtifactWriteError::PublicationUnknown(_code)) => match state.store.lookup_commit(record.run_id, record.key).await {
      Ok(Some(commit)) if artifact_commit_matches(&commit, &record.metadata, record.run_id, record.key) => {
        mark_published(&state, upload_id);
        Ok(run_json(StatusCode::OK, &commit))
      }
      Ok(Some(_)) => {
        mark_indeterminate(&state, upload_id);
        Err(ArtifactFailure::conflict())
      }
      Ok(None) | Err(_) => {
        mark_indeterminate(&state, upload_id);
        Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown")))
      }
    },
    Err(error) => {
      release_upload(&state, upload_id);
      Err(ArtifactFailure::from_write(error))
    }
  }
}

async fn published_replay(state: &InspectServerState, record: &DraftRecord) -> Result<Response, ArtifactFailure> {
  match state.store.lookup_commit(record.run_id, record.key).await.map_err(ArtifactFailure::from_read)? {
    Some(commit) if artifact_commit_matches(&commit, &record.metadata, record.run_id, record.key) => Ok(run_json(StatusCode::OK, &commit)),
    Some(_) => Err(ArtifactFailure::conflict()),
    None => Err(ArtifactFailure::unavailable(error_code("auv.inspect.published_commit_missing"))),
  }
}

async fn indeterminate_replay(state: &InspectServerState, record: &DraftRecord) -> Result<Response, ArtifactFailure> {
  match state.store.lookup_commit(record.run_id, record.key).await {
    Ok(Some(commit)) if artifact_commit_matches(&commit, &record.metadata, record.run_id, record.key) => {
      mark_published(state, record.draft.upload_id());
      Ok(run_json(StatusCode::OK, &commit))
    }
    Ok(Some(_)) => Err(ArtifactFailure::conflict()),
    Ok(None) | Err(_) => Err(ArtifactFailure::unavailable(error_code("auv.inspect.publication_unknown"))),
  }
}

fn mark_published(state: &InspectServerState, upload_id: ArtifactUploadId) {
  if let Some(record) = state.artifacts.drafts.lock().expect("artifact draft index lock").by_upload.get_mut(&upload_id) {
    record.status = DraftStatus::Published;
  }
}

fn release_unstarted_admission(state: &InspectServerState, upload_id: ArtifactUploadId) {
  if let Some(record) = state.artifacts.drafts.lock().expect("artifact draft index lock").by_upload.get_mut(&upload_id)
    && record.status == DraftStatus::Admitted
  {
    record.status = DraftStatus::Released;
  }
}

fn release_upload(state: &InspectServerState, upload_id: ArtifactUploadId) {
  if let Some(record) = state.artifacts.drafts.lock().expect("artifact draft index lock").by_upload.get_mut(&upload_id)
    && record.status == DraftStatus::Uploading
  {
    record.status = DraftStatus::Released;
  }
}

fn mark_indeterminate(state: &InspectServerState, upload_id: ArtifactUploadId) {
  if let Some(record) = state.artifacts.drafts.lock().expect("artifact draft index lock").by_upload.get_mut(&upload_id) {
    record.status = DraftStatus::Indeterminate;
  }
}

fn expire_upload(state: &InspectServerState, upload_id: ArtifactUploadId) {
  let mut indexes = state.artifacts.drafts.lock().expect("artifact draft index lock");
  let Some(record) = indexes.by_upload.get(&upload_id).cloned() else {
    return;
  };
  if indexes.by_key.get(&(record.run_id, record.key)) == Some(&upload_id) {
    indexes.by_key.remove(&(record.run_id, record.key));
  }
  if indexes.by_uri.get(record.draft.artifact_uri()) == Some(&upload_id) {
    indexes.by_uri.remove(record.draft.artifact_uri());
  }
  if let Some(record) = indexes.by_upload.get_mut(&upload_id) {
    record.status = DraftStatus::Expired;
  }
}

struct UploadResetGuard {
  state: Arc<InspectServerState>,
  upload_id: ArtifactUploadId,
  body_complete: Arc<AtomicBool>,
}

impl Drop for UploadResetGuard {
  fn drop(&mut self) {
    let mut indexes = self.state.artifacts.drafts.lock().expect("artifact draft index lock");
    let Some(record) = indexes.by_upload.get_mut(&self.upload_id) else {
      return;
    };
    if record.status != DraftStatus::Uploading {
      return;
    }
    record.status = if self.body_complete.load(Ordering::Acquire) {
      DraftStatus::Indeterminate
    } else {
      DraftStatus::Released
    };
  }
}

async fn read_artifact(
  State(state): State<Arc<InspectServerState>>,
  path: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ArtifactFailure> {
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
    if self.content_type != metadata.content_type().to_string() {
      return Err(ArtifactFailure::unsupported_media_type());
    }
    if self.content_digest != metadata.sha256() {
      return Err(ArtifactFailure::integrity(error_code("auv.inspect.content_digest_mismatch")));
    }
    if self.content_length.is_some_and(|length| length != metadata.byte_length().get()) {
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

fn artifact_json(status: StatusCode, value: &impl Serialize) -> Response {
  json(status, ARTIFACT_UPLOAD_MEDIA_TYPE, value)
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
}

impl ArtifactFailure {
  fn new(status: StatusCode, code: &str) -> Self {
    Self {
      status,
      body: ArtifactApiError::new(error_code(code)),
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
    }
  }

  fn invalid_reference() -> Self {
    Self::new(StatusCode::BAD_REQUEST, "auv.inspect.invalid_reference")
  }

  fn unsupported_media_type() -> Self {
    Self::new(StatusCode::UNSUPPORTED_MEDIA_TYPE, "auv.inspect.unsupported_media_type")
  }

  fn authority_mismatch(expected: AuthorityId, received: AuthorityId) -> Self {
    Self {
      status: StatusCode::CONFLICT,
      body: ArtifactApiError::authority_mismatch(error_code("auv.inspect.authority_mismatch"), expected, received),
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
    }
  }

  fn conflict() -> Self {
    Self::new(StatusCode::CONFLICT, "auv.inspect.idempotency_or_artifact_conflict")
  }

  fn not_found() -> Self {
    Self::new(StatusCode::NOT_FOUND, "auv.inspect.not_found")
  }

  fn gone() -> Self {
    Self::new(StatusCode::GONE, "auv.inspect.upload_expired")
  }

  fn integrity(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::UNPROCESSABLE_ENTITY,
      body: ArtifactApiError::new(code),
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
    }
  }

  fn rejected(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::BAD_REQUEST,
      body: ArtifactApiError::new(code),
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
    }
  }

  fn unavailable(code: ErrorCode) -> Self {
    Self {
      status: StatusCode::SERVICE_UNAVAILABLE,
      body: ArtifactApiError::new(code),
      media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
    }
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
      ArtifactWriteError::IdempotencyMismatch => Self::conflict(),
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
        body: ArtifactApiError::new(code),
        media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      },
      ReadError::HistoryGap { .. } => Self::new(StatusCode::GONE, "auv.inspect.history_gap"),
      ReadError::CursorAhead { .. } => Self::new(StatusCode::CONFLICT, "auv.inspect.cursor_ahead"),
      ReadError::Unavailable(code) => Self::unavailable(code),
      ReadError::Integrity(code) => Self {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: ArtifactApiError::new(code),
        media_type: ARTIFACT_UPLOAD_MEDIA_TYPE,
      },
    }
  }
}

impl IntoResponse for ArtifactFailure {
  fn into_response(self) -> Response {
    json(self.status, self.media_type, &self.body)
  }
}

fn error_code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("static Inspect artifact error code is valid")
}
