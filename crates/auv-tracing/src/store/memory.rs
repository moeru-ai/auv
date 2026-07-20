use std::collections::{HashMap, VecDeque};
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::AsyncReadExt;
use sha2::{Digest, Sha256};

use super::{
  ArtifactBody, ArtifactReader, ArtifactWriteError, BoxFuture, CommitError, ReadError, RunCommitPage, RunStore, RunSubscription,
  StoreArtifactRequest, SubscriptionError,
};
use crate::history::IncrementalReducer;
use crate::{
  ArtifactMetadata, ArtifactPublished, ArtifactUri, AuthorityId, ErrorCode, IdempotencyKey, PageLimit, RunCommit, RunCommitRequest, RunFact,
  RunId, RunMutation, RunRevision, RunSnapshot, Sha256Digest, Timestamp,
};

const ARTIFACT_CHUNK_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// Complete in-process run authority with unbounded history by default.
#[derive(Clone)]
pub struct MemoryRunStore {
  inner: Arc<MemoryAuthority>,
}

struct MemoryAuthority {
  authority_id: AuthorityId,
  history_limit: Option<usize>,
  state: Mutex<MemoryState>,
}

#[derive(Default)]
struct MemoryState {
  runs: HashMap<RunId, MemoryRun>,
  blobs: HashMap<ArtifactUri, StoredArtifact>,
  waiters: HashMap<RunId, Vec<Waker>>,
}

struct MemoryRun {
  commits: VecDeque<RunCommit>,
  reducer: IncrementalReducer,
  idempotency: HashMap<IdempotencyKey, StoredRequest>,
}

impl MemoryRun {
  fn new(authority_id: AuthorityId, run_id: RunId) -> Self {
    Self {
      commits: VecDeque::new(),
      reducer: IncrementalReducer::new(authority_id, run_id),
      idempotency: HashMap::new(),
    }
  }

  fn latest_revision(&self) -> RunRevision {
    self.reducer.snapshot().through_revision()
  }
}

struct StoredRequest {
  request: StoredRequestKind,
  commit: RunCommit,
}

#[derive(PartialEq)]
enum StoredRequestKind {
  Commit(RunCommitRequest),
  Artifact(StoreArtifactRequest),
}

#[derive(Clone)]
struct StoredArtifact {
  bytes: Arc<[u8]>,
  byte_length: crate::ByteLength,
  sha256: Sha256Digest,
}

impl MemoryRunStore {
  /// Creates an in-process authority that retains all committed history.
  pub fn new(authority_id: AuthorityId) -> Self {
    Self::build(authority_id, None)
  }

  /// Creates an authority that retains at most `history_limit` readable commits per run.
  pub fn with_history_limit(authority_id: AuthorityId, history_limit: NonZeroUsize) -> Self {
    Self::build(authority_id, Some(history_limit.get()))
  }

  fn build(authority_id: AuthorityId, history_limit: Option<usize>) -> Self {
    Self {
      inner: Arc::new(MemoryAuthority {
        authority_id,
        history_limit,
        state: Mutex::new(MemoryState::default()),
      }),
    }
  }

  fn commit_ordinary(&self, request: RunCommitRequest) -> Result<RunCommit, CommitError> {
    self.validate_authority(request.authority_id()).map_err(|(expected, received)| CommitError::AuthorityMismatch { expected, received })?;

    let run_id = request.run_id();
    let key = request.idempotency_key();
    let mut state = self.lock_state().map_err(CommitError::Unavailable)?;
    if let Some(stored) = state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)) {
      return if stored.request == StoredRequestKind::Commit(request) {
        Ok(stored.commit.clone())
      } else {
        Err(CommitError::IdempotencyMismatch)
      };
    }

    let latest = state.runs.get(&run_id).map(MemoryRun::latest_revision).unwrap_or_else(zero_revision);
    let revision = next_revision(latest).map_err(CommitError::Rejected)?;
    let facts = request
      .mutations()
      .iter()
      .cloned()
      .map(|mutation| match mutation {
        RunMutation::StartSpan(started) => RunFact::SpanStarted(started),
        RunMutation::EndSpan(ended) => RunFact::SpanEnded(ended),
        RunMutation::EmitEvent(event) => RunFact::EventOccurred(event),
      })
      .collect();
    let commit =
      RunCommit::new(self.inner.authority_id, run_id, revision, key, now(), facts).map_err(|_| CommitError::Rejected(rejected()))?;
    let stored = StoredRequest {
      request: StoredRequestKind::Commit(request),
      commit: commit.clone(),
    };
    if let Some(run) = state.runs.get_mut(&run_id) {
      run.reducer.apply(&commit).map_err(|_| CommitError::Rejected(rejected()))?;
      run.commits.push_back(commit.clone());
      run.idempotency.insert(key, stored);
      prune_history(run, self.inner.history_limit);
    } else {
      let mut run = MemoryRun::new(self.inner.authority_id, run_id);
      run.reducer.apply(&commit).map_err(|_| CommitError::Rejected(rejected()))?;
      run.commits.push_back(commit.clone());
      run.idempotency.insert(key, stored);
      state.runs.insert(run_id, run);
    }
    let waiters = state.waiters.remove(&run_id).unwrap_or_default();
    drop(state);
    wake(waiters);
    Ok(commit)
  }

  fn preflight_artifact(&self, request: &StoreArtifactRequest) -> Result<Option<RunCommit>, ArtifactWriteError> {
    self
      .validate_authority(request.authority_id())
      .map_err(|(expected, received)| ArtifactWriteError::AuthorityMismatch { expected, received })?;
    let state = self.lock_state().map_err(ArtifactWriteError::Unavailable)?;
    if let Some(stored) = state.runs.get(&request.run_id()).and_then(|run| run.idempotency.get(&request.idempotency_key())) {
      return if stored.request == StoredRequestKind::Artifact(request.clone()) {
        Ok(Some(stored.commit.clone()))
      } else {
        Err(ArtifactWriteError::IdempotencyMismatch)
      };
    }

    let uri = ArtifactUri::from_ids(request.run_id(), request.artifact_id());
    if state.runs.get(&request.run_id()).is_some_and(|run| run.reducer.snapshot().artifacts().contains_key(&uri))
      || state.blobs.contains_key(&uri)
    {
      return Err(ArtifactWriteError::Rejected(rejected()));
    }

    if let Some(run) = state.runs.get(&request.run_id()) {
      artifact_candidate(self.inner.authority_id, request, &run.reducer).map_err(ArtifactWriteError::Rejected)?;
    } else {
      let reducer = IncrementalReducer::new(self.inner.authority_id, request.run_id());
      artifact_candidate(self.inner.authority_id, request, &reducer).map_err(ArtifactWriteError::Rejected)?;
    }
    Ok(None)
  }

  fn publish_artifact(&self, request: StoreArtifactRequest, bytes: Arc<[u8]>) -> Result<RunCommit, ArtifactWriteError> {
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let uri = ArtifactUri::from_ids(run_id, request.artifact_id());
    let mut state = self.lock_state().map_err(ArtifactWriteError::Unavailable)?;
    if let Some(stored) = state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)) {
      return if stored.request == StoredRequestKind::Artifact(request) {
        Ok(stored.commit.clone())
      } else {
        Err(ArtifactWriteError::IdempotencyMismatch)
      };
    }
    if state.runs.get(&run_id).is_some_and(|run| run.reducer.snapshot().artifacts().contains_key(&uri)) || state.blobs.contains_key(&uri) {
      return Err(ArtifactWriteError::Rejected(rejected()));
    }

    let commit = if let Some(run) = state.runs.get(&run_id) {
      artifact_candidate(self.inner.authority_id, &request, &run.reducer).map_err(ArtifactWriteError::Rejected)?
    } else {
      let reducer = IncrementalReducer::new(self.inner.authority_id, run_id);
      artifact_candidate(self.inner.authority_id, &request, &reducer).map_err(ArtifactWriteError::Rejected)?
    };
    let stored_artifact = StoredArtifact {
      bytes,
      byte_length: request.expected_byte_length(),
      sha256: request.expected_sha256(),
    };

    let stored = StoredRequest {
      request: StoredRequestKind::Artifact(request),
      commit: commit.clone(),
    };
    if let Some(run) = state.runs.get_mut(&run_id) {
      run.reducer.apply(&commit).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
      run.commits.push_back(commit.clone());
      run.idempotency.insert(key, stored);
      prune_history(run, self.inner.history_limit);
    } else {
      let mut run = MemoryRun::new(self.inner.authority_id, run_id);
      run.reducer.apply(&commit).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
      run.commits.push_back(commit.clone());
      run.idempotency.insert(key, stored);
      state.runs.insert(run_id, run);
    }
    state.blobs.insert(uri, stored_artifact);
    let waiters = state.waiters.remove(&run_id).unwrap_or_default();
    drop(state);
    wake(waiters);
    Ok(commit)
  }

  fn validate_authority(&self, received: AuthorityId) -> Result<(), (AuthorityId, AuthorityId)> {
    let expected = self.inner.authority_id;
    if received == expected {
      Ok(())
    } else {
      Err((expected, received))
    }
  }

  fn lock_state(&self) -> Result<MutexGuard<'_, MemoryState>, ErrorCode> {
    self.inner.state.lock().map_err(|_| unavailable())
  }
}

impl RunStore for MemoryRunStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    Box::pin(async move { self.commit_ordinary(request) })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>> {
    Box::pin(async move {
      if let Some(commit) = self.preflight_artifact(&request)? {
        return Ok(commit);
      }
      let bytes = read_artifact(body, request.expected_byte_length().get(), request.expected_sha256()).await?;
      self.publish_artifact(request, bytes)
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    Box::pin(async move {
      let state = self.lock_state().map_err(ReadError::Unavailable)?;
      Ok(state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)).map(|stored| stored.commit.clone()))
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    Box::pin(async move {
      let state = self.lock_state().map_err(ReadError::Unavailable)?;
      Ok(state.runs.get(&run_id).map(|run| run.reducer.snapshot().clone()))
    })
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async move {
      let state = self.lock_state().map_err(ReadError::Unavailable)?;
      let Some(run) = state.runs.get(&run_id) else {
        if after.get() > 0 {
          return Err(ReadError::CursorAhead {
            requested_after: after,
            latest: zero_revision(),
          });
        }
        return RunCommitPage::new(Vec::new(), after, false).map_err(|_| ReadError::Integrity(integrity()));
      };
      page_after(run, after, limit)
    })
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async move {
      let state = self.lock_state().map_err(ReadError::Unavailable)?;
      let latest = state.runs.get(&run_id).map(MemoryRun::latest_revision).unwrap_or_else(zero_revision);
      if after > latest {
        return Err(ReadError::CursorAhead {
          requested_after: after,
          latest,
        });
      }
      drop(state);
      Ok(Box::pin(MemorySubscription {
        authority: Arc::clone(&self.inner),
        run_id,
        cursor: after,
        terminal: false,
      }) as RunSubscription)
    })
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    Box::pin(async move {
      let artifact = {
        let state = self.lock_state().map_err(ReadError::Unavailable)?;
        state.blobs.get(&uri).cloned().ok_or(ReadError::NotFound)?
      };
      verify_artifact(&artifact).map_err(ReadError::Integrity)?;
      let stream = futures_util::stream::unfold((artifact.bytes, 0_usize), |(bytes, offset)| async move {
        if offset == bytes.len() {
          None
        } else {
          let end = bytes.len().min(offset + ARTIFACT_CHUNK_BYTES);
          let chunk = Bytes::copy_from_slice(&bytes[offset..end]);
          Some((Ok(chunk), (bytes, end)))
        }
      });
      Ok(Box::pin(stream) as ArtifactReader)
    })
  }
}

struct MemorySubscription {
  authority: Arc<MemoryAuthority>,
  run_id: RunId,
  cursor: RunRevision,
  terminal: bool,
}

impl Stream for MemorySubscription {
  type Item = Result<RunCommit, SubscriptionError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.terminal {
      return Poll::Ready(None);
    }
    let authority = Arc::clone(&self.authority);
    let mut state = match authority.state.lock() {
      Ok(state) => state,
      Err(_) => {
        self.terminal = true;
        return Poll::Ready(Some(Err(SubscriptionError::Store(ReadError::Unavailable(unavailable())))));
      }
    };

    if let Some(run) = state.runs.get(&self.run_id) {
      if let Some(earliest) = run.commits.front().map(RunCommit::revision)
        && self.cursor.get().saturating_add(1) < earliest.get()
      {
        let requested_after = self.cursor;
        drop(state);
        self.terminal = true;
        return Poll::Ready(Some(Err(SubscriptionError::Gap {
          requested_after,
          earliest_available: earliest,
        })));
      }

      if self.cursor < run.latest_revision() {
        let expected = self.cursor.get() + 1;
        let next = run.commits.iter().find(|commit| commit.revision().get() == expected).cloned();
        drop(state);
        return match next {
          Some(commit) => {
            self.cursor = commit.revision();
            Poll::Ready(Some(Ok(commit)))
          }
          None => {
            self.terminal = true;
            Poll::Ready(Some(Err(SubscriptionError::Store(ReadError::Integrity(integrity())))))
          }
        };
      }
    }

    let waiters = state.waiters.entry(self.run_id).or_default();
    if !waiters.iter().any(|waker| waker.will_wake(cx.waker())) {
      waiters.push(cx.waker().clone());
    }
    Poll::Pending
  }
}

async fn read_artifact(
  mut body: ArtifactBody,
  expected_length: u64,
  expected_sha256: Sha256Digest,
) -> Result<Arc<[u8]>, ArtifactWriteError> {
  let mut bytes = Vec::new();
  let mut hasher = Sha256::new();
  let mut total = 0_u64;
  let mut chunk = [0_u8; ARTIFACT_CHUNK_BYTES];
  loop {
    let remaining_with_probe = expected_length.saturating_sub(total).saturating_add(1);
    let read_limit = usize::try_from(remaining_with_probe.min(ARTIFACT_CHUNK_BYTES as u64)).expect("artifact chunk size fits usize");
    let count = body.read(&mut chunk[..read_limit]).await.map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    if count == 0 {
      break;
    }
    total = total.checked_add(count as u64).ok_or_else(|| ArtifactWriteError::Integrity(integrity()))?;
    if total > expected_length || total > MAX_ARTIFACT_BYTES {
      return Err(ArtifactWriteError::Integrity(integrity()));
    }
    hasher.update(&chunk[..count]);
    bytes.extend_from_slice(&chunk[..count]);
  }
  if total != expected_length {
    return Err(ArtifactWriteError::Integrity(integrity()));
  }
  let digest = Sha256Digest::new(hasher.finalize().into());
  if digest != expected_sha256 {
    return Err(ArtifactWriteError::Integrity(integrity()));
  }
  Ok(Arc::from(bytes))
}

fn artifact_candidate(
  authority_id: AuthorityId,
  request: &StoreArtifactRequest,
  reducer: &IncrementalReducer,
) -> Result<RunCommit, ErrorCode> {
  let revision = next_revision(reducer.snapshot().through_revision())?;
  let metadata = ArtifactMetadata::new(
    ArtifactUri::from_ids(request.run_id(), request.artifact_id()),
    request.purpose().clone(),
    request.content_type().clone(),
    request.expected_byte_length(),
    request.expected_sha256(),
    request.attributes().clone(),
  );
  let publication = ArtifactPublished::new(request.span_id(), metadata);
  let commit = RunCommit::new(
    authority_id,
    request.run_id(),
    revision,
    request.idempotency_key(),
    now(),
    vec![RunFact::ArtifactPublished(publication)],
  )
  .map_err(|_| rejected())?;
  reducer.validate(&commit).map_err(|_| rejected())?;
  Ok(commit)
}

fn page_after(run: &MemoryRun, after: RunRevision, limit: PageLimit) -> Result<RunCommitPage, ReadError> {
  let latest = run.latest_revision();
  if after > latest {
    return Err(ReadError::CursorAhead {
      requested_after: after,
      latest,
    });
  }
  let earliest = run.commits.front().map(RunCommit::revision).expect("a committed run retains at least one revision");
  if after.get().saturating_add(1) < earliest.get() {
    return Err(ReadError::HistoryGap {
      requested_after: after,
      earliest_available: earliest,
    });
  }
  if after == latest {
    return RunCommitPage::new(Vec::new(), after, false).map_err(|_| ReadError::Integrity(integrity()));
  }

  let start = if after < earliest {
    0
  } else {
    usize::try_from(after.get() - earliest.get() + 1).map_err(|_| ReadError::Integrity(integrity()))?
  };
  let max_count = usize::try_from(limit.get().get()).expect("page limit fits usize");
  let mut selected = Vec::new();
  let mut commit_json_bytes = 0_usize;
  for (offset, commit) in run.commits.iter().skip(start).take(max_count).enumerate() {
    let encoded = serde_json::to_vec(commit).map_err(|_| ReadError::Integrity(integrity()))?;
    let candidate_count = selected.len() + 1;
    let has_more = start + offset + 1 < run.commits.len();
    let candidate_bytes = commit_json_bytes + encoded.len();
    if compact_page_len(candidate_bytes, candidate_count, commit.revision(), has_more) > super::MAX_COMMIT_PAGE_JSON_BYTES {
      break;
    }
    commit_json_bytes = candidate_bytes;
    selected.push(commit.clone());
  }
  if selected.is_empty() {
    return Err(ReadError::Integrity(integrity()));
  }
  let last_revision = selected.last().expect("the page made progress").revision();
  let has_more = last_revision < latest;
  RunCommitPage::new(selected, last_revision, has_more).map_err(|_| ReadError::Integrity(integrity()))
}

fn compact_page_len(commit_bytes: usize, commit_count: usize, last_revision: RunRevision, has_more: bool) -> usize {
  let separators = commit_count.saturating_sub(1);
  "{\"commits\":[".len()
    + commit_bytes
    + separators
    + "],\"last_revision\":".len()
    + last_revision.get().to_string().len()
    + ",\"has_more\":".len()
    + if has_more {
      "true".len()
    } else {
      "false".len()
    }
    + "}".len()
}

fn verify_artifact(artifact: &StoredArtifact) -> Result<(), ErrorCode> {
  if u64::try_from(artifact.bytes.len()).ok() != Some(artifact.byte_length.get()) {
    return Err(integrity());
  }
  let digest = Sha256Digest::new(Sha256::digest(artifact.bytes.as_ref()).into());
  if digest != artifact.sha256 {
    return Err(integrity());
  }
  Ok(())
}

fn prune_history(run: &mut MemoryRun, limit: Option<usize>) {
  if let Some(limit) = limit {
    while run.commits.len() > limit {
      run.commits.pop_front();
    }
  }
}

fn wake(waiters: Vec<Waker>) {
  for waker in waiters {
    waker.wake();
  }
}

fn next_revision(current: RunRevision) -> Result<RunRevision, ErrorCode> {
  current.get().checked_add(1).and_then(|revision| RunRevision::new(revision).ok()).ok_or_else(rejected)
}

fn zero_revision() -> RunRevision {
  RunRevision::new(0).expect("revision zero is valid")
}

fn now() -> Timestamp {
  let now = time::OffsetDateTime::now_utc();
  Timestamp::new(now.unix_timestamp(), now.nanosecond()).expect("system time is representable by the run contract")
}

fn rejected() -> ErrorCode {
  ErrorCode::parse("auv.store.rejected").expect("static error code is valid")
}

fn unavailable() -> ErrorCode {
  ErrorCode::parse("auv.store.unavailable").expect("static error code is valid")
}

fn integrity() -> ErrorCode {
  ErrorCode::parse("auv.store.integrity").expect("static error code is valid")
}
