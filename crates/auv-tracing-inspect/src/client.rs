use std::time::Duration;

use auv_tracing::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, ByteLength, CommitError,
  CommitResult, ContentType, ErrorCode, IdempotencyKey, NonEmptyVec, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest,
  RunFact, RunId, RunRevision, RunSnapshot, RunStore, RunSubscription, Sha256Digest, StoreArtifactRequest, SubscriptionError,
};
use base64::Engine;
use bytes::Bytes;
use futures_util::stream;
use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, EXPECT};
use reqwest::{Response, StatusCode};
use sha2::{Digest, Sha256};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::io::ReaderStream;
use url::Url;

use crate::protocol::{
  ARTIFACT_RESOLVE_MEDIA_TYPE, ARTIFACT_UPLOAD_MEDIA_TYPE, ArtifactApiError, ArtifactUploadDraft, ArtifactUploadDraftRequest,
  AuthorityResponse, RUN_MEDIA_TYPE, ResolveArtifactsRequest, ResolveArtifactsResponse, ResolvedArtifact, RunApiError, RunCommitBody,
  RunStreamGap, decode_strict,
};

const MAX_PROTOCOL_JSON_BYTES: usize = 32 * 1024 * 1024;
const MAX_SSE_FRAME_BYTES: usize = MAX_PROTOCOL_JSON_BYTES + 64 * 1024;

/// A complete remote implementation of the canonical [`RunStore`] authority port.
#[derive(Clone)]
pub struct InspectRunStore {
  authority_id: AuthorityId,
  base_url: Url,
  client: reqwest::Client,
}

impl InspectRunStore {
  /// Fetches and caches the remote authority identity before returning a usable store.
  pub async fn connect(base_url: Url) -> Result<Self, ConnectError> {
    let base_url = normalize_base_url(base_url)?;
    let client = reqwest::Client::new();
    let response =
      client.get(endpoint(&base_url, "v1/authority")).header(ACCEPT, RUN_MEDIA_TYPE).send().await.map_err(ConnectError::transport)?;
    if response.status() != StatusCode::OK || !has_media_type(&response, RUN_MEDIA_TYPE) {
      return Err(ConnectError::new("Inspect authority returned an invalid identity response"));
    }
    let bytes = bounded_response_bytes(response).await.map_err(ConnectError::new)?;
    let identity = decode_strict::<AuthorityResponse>(&bytes).map_err(|error| ConnectError::new(error.to_string()))?;
    Ok(Self {
      authority_id: identity.authority_id,
      base_url,
      client,
    })
  }

  /// Resolves canonical artifact URIs without adding a resolver method to [`RunStore`].
  pub async fn resolve_artifacts(&self, uris: Vec<ArtifactUri>) -> Result<Vec<ResolvedArtifact>, ReadError> {
    let requested = ResolveArtifactsRequest::new(self.authority_id, uris)
      .map_err(|_| ReadError::InvalidReference(code("auv.inspect.resolve_batch_invalid")))?;
    let response = self
      .client
      .post(endpoint(&self.base_url, "v1/resources/artifacts/resolve"))
      .header(CONTENT_TYPE, ARTIFACT_RESOLVE_MEDIA_TYPE)
      .header(ACCEPT, ARTIFACT_RESOLVE_MEDIA_TYPE)
      .json(&requested)
      .send()
      .await
      .map_err(|_| ReadError::Unavailable(code("auv.inspect.transport_unavailable")))?;
    if response.status() != StatusCode::OK {
      return Err(read_artifact_failure(response, ARTIFACT_RESOLVE_MEDIA_TYPE).await);
    }
    if !has_media_type(&response, ARTIFACT_RESOLVE_MEDIA_TYPE) {
      return Err(ReadError::Integrity(code("auv.inspect.resolve_media_type_invalid")));
    }
    let bytes = bounded_response_bytes(response).await.map_err(|_| ReadError::Integrity(code("auv.inspect.resolve_response_invalid")))?;
    let response =
      decode_strict::<ResolveArtifactsResponse>(&bytes).map_err(|_| ReadError::Integrity(code("auv.inspect.resolve_response_invalid")))?;
    if response.results().len() != requested.uris().len() {
      return Err(ReadError::Integrity(code("auv.inspect.resolve_result_count_mismatch")));
    }
    for (requested, resolved) in requested.uris().iter().zip(response.results()) {
      let resolved_uri = match resolved {
        ResolvedArtifact::Available {
          uri, content_url, ..
        } => {
          if !valid_content_url(content_url) {
            return Err(ReadError::Integrity(code("auv.inspect.resolve_content_url_invalid")));
          }
          uri
        }
        ResolvedArtifact::NotFound { uri } => uri,
      };
      if resolved_uri != requested {
        return Err(ReadError::Integrity(code("auv.inspect.resolve_result_order_mismatch")));
      }
    }
    Ok(response.into_results())
  }

  async fn recover_commit(&self, request: &RunCommitRequest, unknown: ErrorCode) -> Result<CommitResult, CommitError> {
    match self.lookup_commit(request.run_id(), request.idempotency_key()).await {
      Ok(Some(commit)) if ordinary_commit_matches(&commit, request) => Ok(CommitResult::Replayed(commit)),
      Ok(Some(_)) | Ok(None) | Err(_) => Err(CommitError::CommitUnknown(unknown)),
    }
  }

  async fn recover_artifact(&self, request: &StoreArtifactRequest, unknown: ErrorCode) -> Result<CommitResult, ArtifactWriteError> {
    match self.lookup_commit(request.run_id(), request.idempotency_key()).await {
      Ok(Some(commit)) if artifact_commit_matches(&commit, request) => Ok(CommitResult::Replayed(commit)),
      Ok(Some(_)) | Ok(None) | Err(_) => Err(ArtifactWriteError::PublicationUnknown(unknown)),
    }
  }
}

impl RunStore for InspectRunStore {
  fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    Box::pin(async move {
      if request.authority_id() != self.authority_id {
        return Err(CommitError::AuthorityMismatch {
          expected: self.authority_id,
          received: request.authority_id(),
        });
      }
      let body = RunCommitBody {
        authority_id: request.authority_id(),
        mutations: NonEmptyVec::new(request.mutations().to_vec()).expect("validated commit request is non-empty"),
      };
      let unknown = code("auv.inspect.commit_transport_unknown");
      let response = match self
        .client
        .post(endpoint(&self.base_url, &format!("v1/runs/{}/commits", request.run_id())))
        .header(CONTENT_TYPE, RUN_MEDIA_TYPE)
        .header(ACCEPT, RUN_MEDIA_TYPE)
        .header("Idempotency-Key", request.idempotency_key().to_string())
        .json(&body)
        .send()
        .await
      {
        Ok(response) => response,
        Err(_) => return self.recover_commit(&request, unknown).await,
      };
      match response.status() {
        StatusCode::CREATED | StatusCode::OK => {
          let appended = response.status() == StatusCode::CREATED;
          if !has_media_type(&response, RUN_MEDIA_TYPE) {
            return self.recover_commit(&request, unknown).await;
          }
          let bytes = match bounded_response_bytes(response).await {
            Ok(bytes) => bytes,
            Err(_) => return self.recover_commit(&request, unknown).await,
          };
          let commit = match decode_strict::<RunCommit>(&bytes) {
            Ok(commit) if ordinary_commit_matches(&commit, &request) => commit,
            _ => return self.recover_commit(&request, unknown).await,
          };
          Ok(if appended {
            CommitResult::Appended(commit)
          } else {
            CommitResult::Replayed(commit)
          })
        }
        _ => Err(commit_failure(response).await),
      }
    })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    Box::pin(async move {
      if request.authority_id() != self.authority_id {
        return Err(ArtifactWriteError::AuthorityMismatch {
          expected: self.authority_id,
          received: request.authority_id(),
        });
      }
      let draft_request = ArtifactUploadDraftRequest::new(
        request.authority_id(),
        request.artifact_id(),
        request.span_id(),
        request.purpose().clone(),
        request.content_type().clone(),
        request.expected_byte_length(),
        request.expected_sha256(),
        request.attributes().clone(),
      );
      let draft_response = self
        .client
        .post(endpoint(&self.base_url, &format!("v1/runs/{}/artifact-uploads", request.run_id())))
        .header(CONTENT_TYPE, ARTIFACT_UPLOAD_MEDIA_TYPE)
        .header(ACCEPT, ARTIFACT_UPLOAD_MEDIA_TYPE)
        .header("Idempotency-Key", request.idempotency_key().to_string())
        .json(&draft_request)
        .send()
        .await
        .map_err(|_| ArtifactWriteError::Unavailable(code("auv.inspect.draft_transport_unavailable")))?;
      if !matches!(draft_response.status(), StatusCode::CREATED | StatusCode::OK) {
        return Err(write_artifact_failure(draft_response).await);
      }
      if !has_media_type(&draft_response, ARTIFACT_UPLOAD_MEDIA_TYPE) {
        return Err(ArtifactWriteError::Unavailable(code("auv.inspect.draft_media_type_invalid")));
      }
      let draft_bytes = bounded_response_bytes(draft_response)
        .await
        .map_err(|_| ArtifactWriteError::Unavailable(code("auv.inspect.draft_response_invalid")))?;
      let draft = decode_strict::<ArtifactUploadDraft>(&draft_bytes)
        .map_err(|_| ArtifactWriteError::Unavailable(code("auv.inspect.draft_response_invalid")))?;
      if draft.artifact_uri() != &ArtifactUri::from_ids(request.run_id(), request.artifact_id()) {
        return Err(ArtifactWriteError::Unavailable(code("auv.inspect.draft_identity_mismatch")));
      }

      let stream = ReaderStream::new(body.compat());
      let response = self
        .client
        .put(endpoint(&self.base_url, &format!("v1/runs/{}/artifact-uploads/{}/content", request.run_id(), draft.upload_id())))
        .header(CONTENT_TYPE, request.content_type().to_string())
        .header(CONTENT_LENGTH, request.expected_byte_length().get())
        .header("Content-Digest", content_digest(request.expected_sha256()))
        .header(EXPECT, "100-continue")
        .header(ACCEPT, RUN_MEDIA_TYPE)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
      let unknown = code("auv.inspect.publication_transport_unknown");
      let response = match response {
        Ok(response) => response,
        Err(_) => return self.recover_artifact(&request, unknown).await,
      };
      match response.status() {
        StatusCode::CREATED | StatusCode::OK => {
          let appended = response.status() == StatusCode::CREATED;
          if !has_media_type(&response, RUN_MEDIA_TYPE) {
            return self.recover_artifact(&request, unknown).await;
          }
          let bytes = match bounded_response_bytes(response).await {
            Ok(bytes) => bytes,
            Err(_) => return self.recover_artifact(&request, unknown).await,
          };
          let commit = match decode_strict::<RunCommit>(&bytes) {
            Ok(commit) if artifact_commit_matches(&commit, &request) => commit,
            _ => return self.recover_artifact(&request, unknown).await,
          };
          Ok(if appended {
            CommitResult::Appended(commit)
          } else {
            CommitResult::Replayed(commit)
          })
        }
        _ => match write_artifact_failure(response).await {
          ArtifactWriteError::PublicationUnknown(code) => self.recover_artifact(&request, code).await,
          error => Err(error),
        },
      }
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    Box::pin(async move {
      let response = self
        .client
        .get(endpoint(&self.base_url, &format!("v1/runs/{run_id}/commits/by-idempotency-key/{key}")))
        .header(ACCEPT, RUN_MEDIA_TYPE)
        .send()
        .await
        .map_err(|_| ReadError::Unavailable(code("auv.inspect.transport_unavailable")))?;
      if response.status() == StatusCode::NOT_FOUND {
        ensure_run_error(response, RunApiError::NotFound).await?;
        return Ok(None);
      }
      if response.status() != StatusCode::OK {
        return Err(read_failure(response).await);
      }
      let commit = decode_run_success::<RunCommit>(response).await?;
      if commit.authority_id() != self.authority_id || commit.run_id() != run_id || commit.idempotency_key() != key {
        return Err(ReadError::Integrity(code("auv.inspect.lookup_identity_mismatch")));
      }
      Ok(Some(commit))
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    Box::pin(async move {
      let response = self
        .client
        .get(endpoint(&self.base_url, &format!("v1/runs/{run_id}/snapshot")))
        .header(ACCEPT, RUN_MEDIA_TYPE)
        .send()
        .await
        .map_err(|_| ReadError::Unavailable(code("auv.inspect.transport_unavailable")))?;
      if response.status() == StatusCode::NOT_FOUND {
        ensure_run_error(response, RunApiError::NotFound).await?;
        return Ok(None);
      }
      if response.status() != StatusCode::OK {
        return Err(read_failure(response).await);
      }
      if !has_media_type(&response, RUN_MEDIA_TYPE) {
        return Err(ReadError::Integrity(code("auv.inspect.snapshot_media_type_invalid")));
      }
      // `load_snapshot` is the deliberate full-materialization API. Unlike
      // pages and individual SSE events, a valid snapshot has no 32 MiB cap.
      let bytes = response.bytes().await.map_err(|_| ReadError::Unavailable(code("auv.inspect.snapshot_transport_unavailable")))?;
      let snapshot = decode_strict::<RunSnapshot>(&bytes).map_err(|_| ReadError::Integrity(code("auv.inspect.snapshot_invalid")))?;
      if snapshot.authority_id() != self.authority_id || snapshot.run_id() != run_id {
        return Err(ReadError::Integrity(code("auv.inspect.snapshot_identity_mismatch")));
      }
      Ok(Some(snapshot))
    })
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async move {
      let mut url = endpoint(&self.base_url, &format!("v1/runs/{run_id}/commits"));
      url.query_pairs_mut().append_pair("after_revision", &after.get().to_string()).append_pair("limit", &limit.get().to_string());
      let response = self
        .client
        .get(url)
        .header(ACCEPT, RUN_MEDIA_TYPE)
        .send()
        .await
        .map_err(|_| ReadError::Unavailable(code("auv.inspect.transport_unavailable")))?;
      if response.status() != StatusCode::OK {
        return Err(read_failure(response).await);
      }
      let page = decode_run_success::<RunCommitPage>(response).await?;
      if page.commits().iter().any(|commit| commit.authority_id() != self.authority_id || commit.run_id() != run_id)
        || page.commits().first().is_some_and(|commit| commit.revision().get() != after.get() + 1)
        || (page.commits().is_empty() && page.last_revision() != after)
      {
        return Err(ReadError::Integrity(code("auv.inspect.commit_page_identity_mismatch")));
      }
      Ok(page)
    })
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async move {
      let response = open_sse(self, run_id, after, None).await?;
      let state = SseState {
        store: self.clone(),
        run_id,
        accepted: after,
        response: Some(response),
        buffer: Vec::new(),
        closed: false,
      };
      Ok(Box::pin(stream::unfold(state, next_sse_item)) as RunSubscription)
    })
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    Box::pin(async move {
      let response = self
        .client
        .get(endpoint(&self.base_url, &format!("v1/runs/{}/artifacts/{}", uri.run_id(), uri.artifact_id())))
        .send()
        .await
        .map_err(|_| ReadError::Unavailable(code("auv.inspect.artifact_transport_unavailable")))?;
      if response.status() != StatusCode::OK {
        return Err(read_artifact_failure(response, ARTIFACT_UPLOAD_MEDIA_TYPE).await);
      }
      let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| ContentType::parse(value).ok())
        .ok_or_else(|| ReadError::Integrity(code("auv.inspect.artifact_content_type_invalid")))?;
      let expected_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .and_then(|value| ByteLength::new(value).ok())
        .ok_or_else(|| ReadError::Integrity(code("auv.inspect.artifact_content_length_invalid")))?;
      let expected_sha256 = response
        .headers()
        .get("Content-Digest")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_content_digest(value).ok())
        .ok_or_else(|| ReadError::Integrity(code("auv.inspect.artifact_content_digest_invalid")))?;
      let _ = content_type;
      let state = ArtifactStreamState {
        response,
        expected_length,
        expected_sha256,
        observed_length: 0,
        hasher: Sha256::new(),
        done: false,
      };
      Ok(Box::pin(stream::unfold(state, next_artifact_chunk)) as ArtifactReader)
    })
  }
}

struct ArtifactStreamState {
  response: Response,
  expected_length: ByteLength,
  expected_sha256: Sha256Digest,
  observed_length: u64,
  hasher: Sha256,
  done: bool,
}

async fn next_artifact_chunk(mut state: ArtifactStreamState) -> Option<(Result<Bytes, ArtifactReadError>, ArtifactStreamState)> {
  if state.done {
    return None;
  }
  match state.response.chunk().await {
    Ok(Some(bytes)) => {
      let Some(observed_length) = state.observed_length.checked_add(bytes.len() as u64) else {
        state.done = true;
        return Some((Err(ArtifactReadError::Integrity(code("auv.inspect.artifact_length_mismatch"))), state));
      };
      if observed_length > state.expected_length.get() {
        state.done = true;
        return Some((Err(ArtifactReadError::Integrity(code("auv.inspect.artifact_length_mismatch"))), state));
      }
      state.observed_length = observed_length;
      state.hasher.update(&bytes);
      Some((Ok(bytes), state))
    }
    Ok(None) => {
      state.done = true;
      let digest = Sha256Digest::new(state.hasher.clone().finalize().into());
      if state.observed_length != state.expected_length.get() || digest != state.expected_sha256 {
        Some((Err(ArtifactReadError::Integrity(code("auv.inspect.artifact_integrity_mismatch"))), state))
      } else {
        None
      }
    }
    Err(_) => {
      state.done = true;
      Some((Err(ArtifactReadError::Unavailable(code("auv.inspect.artifact_stream_unavailable"))), state))
    }
  }
}

struct SseState {
  store: InspectRunStore,
  run_id: RunId,
  accepted: RunRevision,
  response: Option<Response>,
  buffer: Vec<u8>,
  closed: bool,
}

async fn next_sse_item(mut state: SseState) -> Option<(Result<RunCommit, SubscriptionError>, SseState)> {
  loop {
    if state.closed {
      return None;
    }
    if let Some(frame) = take_sse_frame(&mut state.buffer) {
      match parse_sse_frame(&frame, state.store.authority_id, state.run_id, state.accepted) {
        Ok(SseFrameResult::Commit(commit)) => {
          state.accepted = commit.revision();
          return Some((Ok(commit), state));
        }
        Ok(SseFrameResult::Gap(gap)) => {
          state.closed = true;
          return Some((
            Err(SubscriptionError::Gap {
              requested_after: gap.requested_after,
              earliest_available: gap.earliest_available,
            }),
            state,
          ));
        }
        Ok(SseFrameResult::Store(error)) => {
          state.closed = true;
          return Some((Err(SubscriptionError::Store(error)), state));
        }
        Ok(SseFrameResult::Ignored) => continue,
        Err(error) => {
          state.closed = true;
          return Some((Err(SubscriptionError::Store(error)), state));
        }
      }
    }
    if state.buffer.len() > MAX_SSE_FRAME_BYTES {
      state.closed = true;
      return Some((Err(SubscriptionError::Store(ReadError::Integrity(code("auv.inspect.sse_event_too_large")))), state));
    }
    if state.response.is_none() {
      tokio::time::sleep(Duration::from_millis(10)).await;
      match open_sse(&state.store, state.run_id, state.accepted, Some(state.accepted)).await {
        Ok(response) => state.response = Some(response),
        Err(error) => {
          state.closed = true;
          return Some((Err(SubscriptionError::Store(error)), state));
        }
      }
    }
    let response = state.response.as_mut().expect("SSE response is installed");
    match response.chunk().await {
      Ok(Some(bytes)) => state.buffer.extend_from_slice(&bytes),
      Ok(None) | Err(_) => {
        state.response = None;
        state.buffer.clear();
      }
    }
  }
}

enum SseFrameResult {
  Commit(RunCommit),
  Gap(RunStreamGap),
  Store(ReadError),
  Ignored,
}

fn parse_sse_frame(bytes: &[u8], authority_id: AuthorityId, run_id: RunId, accepted: RunRevision) -> Result<SseFrameResult, ReadError> {
  let text = std::str::from_utf8(bytes).map_err(|_| ReadError::Integrity(code("auv.inspect.sse_event_invalid")))?;
  let mut id = None;
  let mut event = "message";
  let mut data = Vec::new();
  for line in text.lines() {
    let line = line.strip_suffix('\r').unwrap_or(line);
    if line.starts_with(':') || line.is_empty() {
      continue;
    }
    let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)));
    match field {
      "id" => {
        if value.contains('\0') {
          return Err(ReadError::Integrity(code("auv.inspect.sse_id_invalid")));
        }
        id = Some(value);
      }
      "event" => event = value,
      "data" => data.push(value),
      _ => {}
    }
  }
  let data = data.join("\n");
  if data.len() > MAX_PROTOCOL_JSON_BYTES {
    return Err(ReadError::Integrity(code("auv.inspect.sse_event_too_large")));
  }
  match event {
    "commit" => {
      let revision = id
        .and_then(|id| id.parse::<u64>().ok())
        .and_then(|revision| RunRevision::new(revision).ok())
        .ok_or_else(|| ReadError::Integrity(code("auv.inspect.sse_id_invalid")))?;
      let commit = decode_strict::<RunCommit>(data.as_bytes()).map_err(|_| ReadError::Integrity(code("auv.inspect.sse_commit_invalid")))?;
      let expected = accepted
        .get()
        .checked_add(1)
        .and_then(|revision| RunRevision::new(revision).ok())
        .ok_or_else(|| ReadError::Integrity(code("auv.inspect.sse_revision_invalid")))?;
      if revision != commit.revision() || revision != expected || commit.authority_id() != authority_id || commit.run_id() != run_id {
        return Err(ReadError::Integrity(code("auv.inspect.sse_commit_identity_mismatch")));
      }
      Ok(SseFrameResult::Commit(commit))
    }
    "gap" => {
      let gap = decode_strict::<RunStreamGap>(data.as_bytes()).map_err(|_| ReadError::Integrity(code("auv.inspect.sse_gap_invalid")))?;
      if gap.requested_after != accepted {
        return Err(ReadError::Integrity(code("auv.inspect.sse_gap_cursor_mismatch")));
      }
      Ok(SseFrameResult::Gap(gap))
    }
    "error" => {
      let error = decode_strict::<RunApiError>(data.as_bytes()).map_err(|_| ReadError::Integrity(code("auv.inspect.sse_error_invalid")))?;
      Ok(SseFrameResult::Store(map_read_error(error)))
    }
    _ => Ok(SseFrameResult::Ignored),
  }
}

fn take_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
  let lf = buffer.windows(2).position(|window| window == b"\n\n").map(|position| (position, 2));
  let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n").map(|position| (position, 4));
  let (position, delimiter) = match (lf, crlf) {
    (Some(left), Some(right)) => left.min(right),
    (Some(found), None) | (None, Some(found)) => found,
    (None, None) => return None,
  };
  let frame = buffer[..position].to_vec();
  buffer.drain(..position + delimiter);
  Some(frame)
}

async fn open_sse(
  store: &InspectRunStore,
  run_id: RunId,
  after: RunRevision,
  last_event_id: Option<RunRevision>,
) -> Result<Response, ReadError> {
  let mut url = endpoint(&store.base_url, &format!("v1/runs/{run_id}/commits/stream"));
  url.query_pairs_mut().append_pair("after_revision", &after.get().to_string());
  let mut request = store.client.get(url).header(ACCEPT, "text/event-stream");
  if let Some(last_event_id) = last_event_id {
    request = request.header("Last-Event-ID", last_event_id.get().to_string());
  }
  let response = request.send().await.map_err(|_| ReadError::Unavailable(code("auv.inspect.sse_transport_unavailable")))?;
  if response.status() != StatusCode::OK {
    return Err(read_failure(response).await);
  }
  if !has_media_type(&response, "text/event-stream") {
    return Err(ReadError::Integrity(code("auv.inspect.sse_media_type_invalid")));
  }
  Ok(response)
}

async fn decode_run_success<T: serde::de::DeserializeOwned>(response: Response) -> Result<T, ReadError> {
  if !has_media_type(&response, RUN_MEDIA_TYPE) {
    return Err(ReadError::Integrity(code("auv.inspect.run_media_type_invalid")));
  }
  let bytes = bounded_response_bytes(response).await.map_err(|_| ReadError::Integrity(code("auv.inspect.run_response_invalid")))?;
  decode_strict(&bytes).map_err(|_| ReadError::Integrity(code("auv.inspect.run_response_invalid")))
}

async fn ensure_run_error(response: Response, expected: RunApiError) -> Result<(), ReadError> {
  if !has_media_type(&response, RUN_MEDIA_TYPE) {
    return Err(ReadError::Integrity(code("auv.inspect.run_error_media_type_invalid")));
  }
  let bytes = bounded_response_bytes(response).await.map_err(|_| ReadError::Integrity(code("auv.inspect.run_error_invalid")))?;
  let received = decode_strict::<RunApiError>(&bytes).map_err(|_| ReadError::Integrity(code("auv.inspect.run_error_invalid")))?;
  if received != expected {
    return Err(ReadError::Integrity(code("auv.inspect.run_error_status_mismatch")));
  }
  Ok(())
}

async fn commit_failure(response: Response) -> CommitError {
  let status = response.status();
  match decode_run_api_error(response).await {
    Ok(error) if status_matches_run_error(status, &error) => map_commit_error(error),
    _ => CommitError::CommitUnknown(code("auv.inspect.commit_error_invalid")),
  }
}

async fn read_failure(response: Response) -> ReadError {
  let status = response.status();
  match decode_run_api_error(response).await {
    Ok(error) if status_matches_run_error(status, &error) => map_read_error(error),
    _ => ReadError::Integrity(code("auv.inspect.run_error_invalid")),
  }
}

async fn decode_run_api_error(response: Response) -> Result<RunApiError, ()> {
  if !has_media_type(&response, RUN_MEDIA_TYPE) {
    return Err(());
  }
  let bytes = bounded_response_bytes(response).await.map_err(|_| ())?;
  decode_strict(&bytes).map_err(|_| ())
}

fn status_matches_run_error(status: StatusCode, error: &RunApiError) -> bool {
  matches!(
    (status, error),
    (StatusCode::NOT_FOUND, RunApiError::NotFound)
      | (StatusCode::FORBIDDEN, RunApiError::Forbidden)
      | (StatusCode::BAD_REQUEST, RunApiError::InvalidReference { .. })
      | (StatusCode::CONFLICT, RunApiError::AuthorityMismatch { .. })
      | (StatusCode::CONFLICT, RunApiError::IdempotencyMismatch)
      | (StatusCode::CONFLICT, RunApiError::CursorAhead { .. })
      | (StatusCode::UNPROCESSABLE_ENTITY, RunApiError::Rejected { .. })
      | (StatusCode::GONE, RunApiError::HistoryGap { .. })
      | (StatusCode::INTERNAL_SERVER_ERROR, RunApiError::Integrity { .. })
      | (StatusCode::SERVICE_UNAVAILABLE, RunApiError::Unavailable { .. })
  )
}

fn map_commit_error(error: RunApiError) -> CommitError {
  match error {
    RunApiError::AuthorityMismatch { expected, received } => CommitError::AuthorityMismatch { expected, received },
    RunApiError::IdempotencyMismatch => CommitError::IdempotencyMismatch,
    RunApiError::Rejected { code } | RunApiError::InvalidReference { code } => CommitError::Rejected(code),
    RunApiError::Unavailable { code } => CommitError::Unavailable(code),
    RunApiError::Integrity { code } => CommitError::CommitUnknown(code),
    RunApiError::NotFound | RunApiError::Forbidden | RunApiError::HistoryGap { .. } | RunApiError::CursorAhead { .. } => {
      CommitError::CommitUnknown(code("auv.inspect.commit_error_unexpected"))
    }
  }
}

fn map_read_error(error: RunApiError) -> ReadError {
  match error {
    RunApiError::NotFound => ReadError::NotFound,
    RunApiError::Forbidden => ReadError::Forbidden,
    RunApiError::InvalidReference { code } | RunApiError::Rejected { code } => ReadError::InvalidReference(code),
    RunApiError::HistoryGap {
      requested_after,
      earliest_available,
    } => ReadError::HistoryGap {
      requested_after,
      earliest_available,
    },
    RunApiError::CursorAhead {
      requested_after,
      latest,
    } => ReadError::CursorAhead {
      requested_after,
      latest,
    },
    RunApiError::Integrity { code } => ReadError::Integrity(code),
    RunApiError::Unavailable { code } => ReadError::Unavailable(code),
    RunApiError::AuthorityMismatch { .. } | RunApiError::IdempotencyMismatch => {
      ReadError::InvalidReference(code("auv.inspect.read_conflict"))
    }
  }
}

async fn write_artifact_failure(response: Response) -> ArtifactWriteError {
  let status = response.status();
  match decode_artifact_error(response, ARTIFACT_UPLOAD_MEDIA_TYPE).await {
    Ok(error) => match status {
      StatusCode::CONFLICT => ArtifactWriteError::IdempotencyMismatch,
      StatusCode::UNPROCESSABLE_ENTITY => ArtifactWriteError::Integrity(error),
      StatusCode::SERVICE_UNAVAILABLE if error == code("auv.inspect.publication_unknown") => ArtifactWriteError::PublicationUnknown(error),
      StatusCode::SERVICE_UNAVAILABLE => ArtifactWriteError::Unavailable(error),
      StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::GONE | StatusCode::PAYLOAD_TOO_LARGE => {
        ArtifactWriteError::Rejected(error)
      }
      _ => ArtifactWriteError::Unavailable(code("auv.inspect.artifact_error_status_invalid")),
    },
    Err(()) => ArtifactWriteError::Unavailable(code("auv.inspect.artifact_error_invalid")),
  }
}

async fn read_artifact_failure(response: Response, expected_media: &str) -> ReadError {
  let status = response.status();
  match decode_artifact_error(response, expected_media).await {
    Ok(error) => match status {
      StatusCode::NOT_FOUND => ReadError::NotFound,
      StatusCode::FORBIDDEN => ReadError::Forbidden,
      StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::GONE => ReadError::InvalidReference(error),
      StatusCode::INTERNAL_SERVER_ERROR => ReadError::Integrity(error),
      StatusCode::SERVICE_UNAVAILABLE => ReadError::Unavailable(error),
      _ => ReadError::Integrity(code("auv.inspect.artifact_error_status_invalid")),
    },
    Err(()) => ReadError::Integrity(code("auv.inspect.artifact_error_invalid")),
  }
}

async fn decode_artifact_error(response: Response, media_type: &str) -> Result<ErrorCode, ()> {
  if !has_media_type(&response, media_type) {
    return Err(());
  }
  let bytes = bounded_response_bytes(response).await.map_err(|_| ())?;
  let error = decode_strict::<ArtifactApiError>(&bytes).map_err(|_| ())?;
  Ok(error.error().clone())
}

async fn bounded_response_bytes(mut response: Response) -> Result<Vec<u8>, String> {
  let mut bytes = Vec::new();
  while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
    if bytes.len().saturating_add(chunk.len()) > MAX_PROTOCOL_JSON_BYTES {
      return Err("Inspect JSON response exceeds 32 MiB".to_string());
    }
    bytes.extend_from_slice(&chunk);
  }
  Ok(bytes)
}

fn ordinary_commit_matches(commit: &RunCommit, request: &RunCommitRequest) -> bool {
  commit.authority_id() == request.authority_id()
    && commit.run_id() == request.run_id()
    && commit.idempotency_key() == request.idempotency_key()
    && commit.facts().len() == request.mutations().len()
    && commit.facts().iter().zip(request.mutations()).all(|(fact, mutation)| match (fact, mutation) {
      (RunFact::SpanStarted(left), auv_tracing::RunMutation::StartSpan(right)) => left == right,
      (RunFact::SpanEnded(left), auv_tracing::RunMutation::EndSpan(right)) => left == right,
      (RunFact::EventOccurred(left), auv_tracing::RunMutation::EmitEvent(right)) => left == right,
      _ => false,
    })
}

fn artifact_commit_matches(commit: &RunCommit, request: &StoreArtifactRequest) -> bool {
  if commit.authority_id() != request.authority_id()
    || commit.run_id() != request.run_id()
    || commit.idempotency_key() != request.idempotency_key()
    || commit.facts().len() != 1
  {
    return false;
  }
  let RunFact::ArtifactPublished(published) = &commit.facts()[0] else {
    return false;
  };
  let metadata = published.metadata();
  published.span_id() == request.span_id()
    && metadata.uri() == &ArtifactUri::from_ids(request.run_id(), request.artifact_id())
    && metadata.purpose() == request.purpose()
    && metadata.content_type() == request.content_type()
    && metadata.byte_length() == request.expected_byte_length()
    && metadata.sha256() == request.expected_sha256()
    && metadata.attributes() == request.attributes()
}

fn normalize_base_url(mut url: Url) -> Result<Url, ConnectError> {
  if !matches!(url.scheme(), "http" | "https")
    || !url.username().is_empty()
    || url.password().is_some()
    || url.host_str().is_none()
    || url.query().is_some()
    || url.fragment().is_some()
  {
    return Err(ConnectError::new("Inspect base URL must be an absolute credential-free HTTP(S) URL"));
  }
  if !url.path().ends_with('/') {
    let path = format!("{}/", url.path());
    url.set_path(&path);
  }
  Ok(url)
}

fn endpoint(base_url: &Url, relative: &str) -> Url {
  base_url.join(relative).expect("validated base URL accepts relative Inspect endpoints")
}

fn has_media_type(response: &Response, expected: &str) -> bool {
  response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()) == Some(expected)
}

fn valid_content_url(url: &Url) -> bool {
  matches!(url.scheme(), "http" | "https")
    && url.host_str().is_some()
    && url.username().is_empty()
    && url.password().is_none()
    && url.query().is_none()
    && url.fragment().is_none()
}

fn parse_content_digest(value: &str) -> Result<Sha256Digest, ()> {
  let encoded = value.strip_prefix("sha-256=:").and_then(|value| value.strip_suffix(':')).ok_or(())?;
  let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| ())?;
  Ok(Sha256Digest::new(bytes.try_into().map_err(|_| ())?))
}

fn content_digest(digest: Sha256Digest) -> String {
  format!("sha-256=:{}:", base64::engine::general_purpose::STANDARD.encode(digest.as_bytes()))
}

fn code(value: &str) -> ErrorCode {
  ErrorCode::parse(value).expect("static Inspect client error code is valid")
}

/// Reports that an Inspect authority could not be validated during connection.
#[derive(Debug, thiserror::Error)]
#[error("failed to connect to Inspect authority: {message}")]
pub struct ConnectError {
  message: String,
}

impl ConnectError {
  fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }

  fn transport(error: reqwest::Error) -> Self {
    Self::new(error.to_string())
  }
}
