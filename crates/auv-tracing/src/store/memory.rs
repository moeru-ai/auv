use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::AsyncReadExt;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
  ArtifactBody, ArtifactReader, ArtifactWriteError, BoxFuture, CommitError, CommitResult, ReadError, RunCommitPage, RunStore,
  RunSubscription, StoreArtifactRequest, SubscriptionError,
};
use crate::history::IncrementalReducer;
use crate::{
  ArtifactMetadata, ArtifactPublished, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode,
  IdempotencyKey, PageLimit, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunSnapshot, Sha256Digest, SpanId,
  Timestamp,
};

const ARTIFACT_CHUNK_BYTES: usize = 64 * 1024;

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
  subscription_waiters: HashMap<RunId, HashMap<u64, Waker>>,
  pending_artifacts: HashMap<(RunId, IdempotencyKey), PendingArtifact>,
  pending_artifact_uris: HashMap<ArtifactUri, PendingArtifactOwner>,
  next_token: u64,
}

struct MemoryRun {
  commits: VecDeque<Arc<RunCommit>>,
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
  fingerprint: RequestFingerprint,
  commit: Arc<RunCommit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RequestFingerprint([u8; 32]);

#[derive(Clone)]
struct StoredArtifact {
  bytes: Bytes,
  byte_length: ByteLength,
  sha256: Sha256Digest,
}

struct PendingArtifact {
  fingerprint: RequestFingerprint,
  uri: ArtifactUri,
  reservation_id: u64,
  waiters: HashMap<u64, Waker>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PendingArtifactOwner {
  run_id: RunId,
  key: IdempotencyKey,
  reservation_id: u64,
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

  fn commit_ordinary(&self, request: RunCommitRequest) -> Result<CommitResult, CommitError> {
    self.validate_authority(request.authority_id()).map_err(|(expected, received)| CommitError::AuthorityMismatch { expected, received })?;
    let fingerprint = commit_fingerprint(&request).map_err(CommitError::Rejected)?;
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let mut state = self.lock_state().map_err(CommitError::Unavailable)?;

    if let Some(stored) = state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)) {
      if stored.fingerprint != fingerprint {
        return Err(CommitError::IdempotencyMismatch);
      }
      let commit = Arc::clone(&stored.commit);
      let result = CommitResult::Replayed(commit.as_ref().clone());
      drop(state);
      return Ok(result);
    }
    if state.pending_artifacts.contains_key(&(run_id, key)) {
      return Err(CommitError::IdempotencyMismatch);
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
    let shared = Arc::new(commit.clone());

    if let Some(run) = state.runs.get_mut(&run_id) {
      run.reducer.apply(&commit).map_err(|_| CommitError::Rejected(rejected()))?;
      run.commits.push_back(Arc::clone(&shared));
      run.idempotency.insert(
        key,
        StoredRequest {
          fingerprint,
          commit: shared,
        },
      );
      prune_history(run, self.inner.history_limit);
    } else {
      let mut run = MemoryRun::new(self.inner.authority_id, run_id);
      run.reducer.apply(&commit).map_err(|_| CommitError::Rejected(rejected()))?;
      run.commits.push_back(Arc::clone(&shared));
      run.idempotency.insert(
        key,
        StoredRequest {
          fingerprint,
          commit: shared,
        },
      );
      state.runs.insert(run_id, run);
    }
    let waiters = take_subscription_waiters(&mut state, run_id);
    let result = CommitResult::Appended(commit);
    drop(state);
    wake(waiters);
    Ok(result)
  }

  fn begin_artifact(&self, request: &StoreArtifactRequest, fingerprint: RequestFingerprint) -> Result<ArtifactAttempt, ArtifactWriteError> {
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let uri = ArtifactUri::from_ids(run_id, request.artifact_id());
    let mut state = self.lock_state().map_err(ArtifactWriteError::Unavailable)?;

    if let Some(stored) = state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)) {
      if stored.fingerprint != fingerprint {
        return Err(ArtifactWriteError::IdempotencyMismatch);
      }
      return Ok(ArtifactAttempt::Replay(CommitResult::Replayed(stored.commit.as_ref().clone())));
    }
    if let Some(pending) = state.pending_artifacts.get(&(run_id, key)) {
      if pending.fingerprint != fingerprint {
        return Err(ArtifactWriteError::IdempotencyMismatch);
      }
      let reservation_id = pending.reservation_id;
      let waiter_id = allocate_token(&mut state).map_err(ArtifactWriteError::Unavailable)?;
      return Ok(ArtifactAttempt::Wait(ArtifactReservationWait {
        authority: Arc::clone(&self.inner),
        run_id,
        key,
        reservation_id,
        waiter_id,
        armed: true,
      }));
    }
    if state.pending_artifact_uris.contains_key(&uri)
      || state.runs.get(&run_id).is_some_and(|run| run.reducer.snapshot().artifacts().contains_key(&uri))
      || state.blobs.contains_key(&uri)
    {
      return Err(ArtifactWriteError::Rejected(rejected()));
    }

    if let Some(run) = state.runs.get(&run_id) {
      artifact_candidate(self.inner.authority_id, request, &run.reducer).map_err(ArtifactWriteError::Rejected)?;
    } else {
      let reducer = IncrementalReducer::new(self.inner.authority_id, run_id);
      artifact_candidate(self.inner.authority_id, request, &reducer).map_err(ArtifactWriteError::Rejected)?;
    }

    let reservation_id = allocate_token(&mut state).map_err(ArtifactWriteError::Unavailable)?;
    let owner = PendingArtifactOwner {
      run_id,
      key,
      reservation_id,
    };
    state.pending_artifact_uris.insert(uri.clone(), owner);
    state.pending_artifacts.insert(
      (run_id, key),
      PendingArtifact {
        fingerprint,
        uri,
        reservation_id,
        waiters: HashMap::new(),
      },
    );
    Ok(ArtifactAttempt::Owner(ArtifactReservation {
      authority: Arc::clone(&self.inner),
      run_id,
      key,
      reservation_id,
      armed: true,
    }))
  }

  fn publish_artifact(
    &self,
    request: &StoreArtifactRequest,
    fingerprint: RequestFingerprint,
    bytes: Bytes,
    reservation: &mut ArtifactReservation,
  ) -> Result<CommitResult, ArtifactWriteError> {
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let uri = ArtifactUri::from_ids(run_id, request.artifact_id());
    let mut state = self.lock_state().map_err(ArtifactWriteError::Unavailable)?;
    let owns_reservation = state.pending_artifacts.get(&(run_id, key)).is_some_and(|pending| {
      pending.reservation_id == reservation.reservation_id && pending.fingerprint == fingerprint && pending.uri == uri
    });
    if !owns_reservation {
      return Err(ArtifactWriteError::Unavailable(unavailable()));
    }

    let commit = if let Some(run) = state.runs.get(&run_id) {
      artifact_candidate(self.inner.authority_id, request, &run.reducer).map_err(ArtifactWriteError::Rejected)?
    } else {
      let reducer = IncrementalReducer::new(self.inner.authority_id, run_id);
      artifact_candidate(self.inner.authority_id, request, &reducer).map_err(ArtifactWriteError::Rejected)?
    };
    let shared = Arc::new(commit.clone());
    let stored_artifact = StoredArtifact {
      bytes,
      byte_length: request.expected_byte_length(),
      sha256: request.expected_sha256(),
    };

    if let Some(run) = state.runs.get_mut(&run_id) {
      run.reducer.apply(&commit).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
      run.commits.push_back(Arc::clone(&shared));
      run.idempotency.insert(
        key,
        StoredRequest {
          fingerprint,
          commit: shared,
        },
      );
      prune_history(run, self.inner.history_limit);
    } else {
      let mut run = MemoryRun::new(self.inner.authority_id, run_id);
      run.reducer.apply(&commit).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
      run.commits.push_back(Arc::clone(&shared));
      run.idempotency.insert(
        key,
        StoredRequest {
          fingerprint,
          commit: shared,
        },
      );
      state.runs.insert(run_id, run);
    }
    state.blobs.insert(uri, stored_artifact);
    let artifact_waiters = release_artifact_reservation(&mut state, run_id, key, reservation.reservation_id);
    let subscription_waiters = take_subscription_waiters(&mut state, run_id);
    reservation.armed = false;
    let result = CommitResult::Appended(commit);
    drop(state);
    wake(artifact_waiters);
    wake(subscription_waiters);
    Ok(result)
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

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
    Box::pin(async move { self.commit_ordinary(request) })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
    Box::pin(async move {
      self
        .validate_authority(request.authority_id())
        .map_err(|(expected, received)| ArtifactWriteError::AuthorityMismatch { expected, received })?;
      let fingerprint = artifact_fingerprint(&request).map_err(ArtifactWriteError::Rejected)?;
      let mut body = Some(body);
      loop {
        match self.begin_artifact(&request, fingerprint)? {
          ArtifactAttempt::Replay(result) => return Ok(result),
          ArtifactAttempt::Wait(wait) => wait.await.map_err(ArtifactWriteError::Unavailable)?,
          ArtifactAttempt::Owner(mut reservation) => {
            let body = body.take().expect("an artifact body is consumed only by its reservation owner");
            let bytes = read_artifact(body, request.expected_byte_length().get(), request.expected_sha256()).await?;
            return self.publish_artifact(&request, fingerprint, bytes, &mut reservation);
          }
        }
      }
    })
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    Box::pin(async move {
      let commit = {
        let state = self.lock_state().map_err(ReadError::Unavailable)?;
        state.runs.get(&run_id).and_then(|run| run.idempotency.get(&key)).map(|stored| Arc::clone(&stored.commit))
      };
      Ok(commit.map(|commit| commit.as_ref().clone()))
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    Box::pin(async move {
      let snapshot = {
        let state = self.lock_state().map_err(ReadError::Unavailable)?;
        state.runs.get(&run_id).map(|run| run.reducer.shared_snapshot())
      };
      Ok(snapshot.map(|snapshot| snapshot.as_ref().clone()))
    })
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async move {
      let source = {
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
        capture_page(run, after, limit)?
      };
      build_page(source, after)
    })
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async move {
      let mut state = self.lock_state().map_err(ReadError::Unavailable)?;
      let latest = state.runs.get(&run_id).map(MemoryRun::latest_revision).unwrap_or_else(zero_revision);
      if after > latest {
        return Err(ReadError::CursorAhead {
          requested_after: after,
          latest,
        });
      }
      let waiter_id = allocate_token(&mut state).map_err(ReadError::Unavailable)?;
      drop(state);
      Ok(Box::pin(MemorySubscription {
        authority: Arc::clone(&self.inner),
        run_id,
        cursor: after,
        waiter_id,
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
          let chunk = bytes.slice(offset..end);
          Some((Ok(chunk), (bytes, end)))
        }
      });
      Ok(Box::pin(stream) as ArtifactReader)
    })
  }
}

enum ArtifactAttempt {
  Replay(CommitResult),
  Wait(ArtifactReservationWait),
  Owner(ArtifactReservation),
}

struct ArtifactReservation {
  authority: Arc<MemoryAuthority>,
  run_id: RunId,
  key: IdempotencyKey,
  reservation_id: u64,
  armed: bool,
}

impl Drop for ArtifactReservation {
  fn drop(&mut self) {
    if !self.armed {
      return;
    }
    let waiters = match self.authority.state.lock() {
      Ok(mut state) => release_artifact_reservation(&mut state, self.run_id, self.key, self.reservation_id),
      Err(_) => Vec::new(),
    };
    wake(waiters);
  }
}

struct ArtifactReservationWait {
  authority: Arc<MemoryAuthority>,
  run_id: RunId,
  key: IdempotencyKey,
  reservation_id: u64,
  waiter_id: u64,
  armed: bool,
}

impl Future for ArtifactReservationWait {
  type Output = Result<(), ErrorCode>;

  fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let authority = Arc::clone(&self.authority);
    let mut state = match authority.state.lock() {
      Ok(state) => state,
      Err(_) => {
        self.armed = false;
        return Poll::Ready(Err(unavailable()));
      }
    };
    let key = (self.run_id, self.key);
    let still_pending = state.pending_artifacts.get(&key).is_some_and(|pending| pending.reservation_id == self.reservation_id);
    if !still_pending {
      drop(state);
      self.armed = false;
      return Poll::Ready(Ok(()));
    }
    state.pending_artifacts.get_mut(&key).expect("reservation was checked above").waiters.insert(self.waiter_id, cx.waker().clone());
    Poll::Pending
  }
}

impl Drop for ArtifactReservationWait {
  fn drop(&mut self) {
    if !self.armed {
      return;
    }
    if let Ok(mut state) = self.authority.state.lock()
      && let Some(pending) = state.pending_artifacts.get_mut(&(self.run_id, self.key))
      && pending.reservation_id == self.reservation_id
    {
      pending.waiters.remove(&self.waiter_id);
    }
  }
}

struct MemorySubscription {
  authority: Arc<MemoryAuthority>,
  run_id: RunId,
  cursor: RunRevision,
  waiter_id: u64,
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
      if let Some(earliest) = run.commits.front().map(|commit| commit.revision())
        && self.cursor.get().saturating_add(1) < earliest.get()
      {
        let requested_after = self.cursor;
        remove_subscription_waiter(&mut state, self.run_id, self.waiter_id);
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
        remove_subscription_waiter(&mut state, self.run_id, self.waiter_id);
        drop(state);
        return match next {
          Some(commit) => {
            self.cursor = commit.revision();
            Poll::Ready(Some(Ok(commit.as_ref().clone())))
          }
          None => {
            self.terminal = true;
            Poll::Ready(Some(Err(SubscriptionError::Store(ReadError::Integrity(integrity())))))
          }
        };
      }
    }

    state.subscription_waiters.entry(self.run_id).or_default().insert(self.waiter_id, cx.waker().clone());
    Poll::Pending
  }
}

impl Drop for MemorySubscription {
  fn drop(&mut self) {
    if let Ok(mut state) = self.authority.state.lock() {
      remove_subscription_waiter(&mut state, self.run_id, self.waiter_id);
    }
  }
}

async fn read_artifact(mut body: ArtifactBody, expected_length: u64, expected_sha256: Sha256Digest) -> Result<Bytes, ArtifactWriteError> {
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
    if total > expected_length {
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
  Ok(Bytes::from(bytes))
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

struct PageSource {
  commits: Vec<Arc<RunCommit>>,
  latest: RunRevision,
}

fn capture_page(run: &MemoryRun, after: RunRevision, limit: PageLimit) -> Result<PageSource, ReadError> {
  let latest = run.latest_revision();
  if after > latest {
    return Err(ReadError::CursorAhead {
      requested_after: after,
      latest,
    });
  }
  let earliest = run.commits.front().map(|commit| commit.revision()).expect("a committed run retains at least one revision");
  if after.get().saturating_add(1) < earliest.get() {
    return Err(ReadError::HistoryGap {
      requested_after: after,
      earliest_available: earliest,
    });
  }
  if after == latest {
    return Ok(PageSource {
      commits: Vec::new(),
      latest,
    });
  }

  let start = if after < earliest {
    0
  } else {
    usize::try_from(after.get() - earliest.get() + 1).map_err(|_| ReadError::Integrity(integrity()))?
  };
  let max_count = usize::try_from(limit.get().get()).expect("page limit fits usize");
  Ok(PageSource {
    commits: run.commits.iter().skip(start).take(max_count).cloned().collect(),
    latest,
  })
}

fn build_page(source: PageSource, after: RunRevision) -> Result<RunCommitPage, ReadError> {
  if source.commits.is_empty() {
    return RunCommitPage::new(Vec::new(), after, false).map_err(|_| ReadError::Integrity(integrity()));
  }
  let mut selected = Vec::new();
  let mut commit_json_bytes = 0_usize;
  for commit in source.commits {
    let encoded = serde_json::to_vec(commit.as_ref()).map_err(|_| ReadError::Integrity(integrity()))?;
    let candidate_count = selected.len() + 1;
    let has_more = commit.revision() < source.latest;
    let candidate_bytes = commit_json_bytes + encoded.len();
    if compact_page_len(candidate_bytes, candidate_count, commit.revision(), has_more) > super::MAX_COMMIT_PAGE_JSON_BYTES {
      break;
    }
    commit_json_bytes = candidate_bytes;
    selected.push(commit.as_ref().clone());
  }
  if selected.is_empty() {
    return Err(ReadError::Integrity(integrity()));
  }
  let last_revision = selected.last().expect("the page made progress").revision();
  let has_more = last_revision < source.latest;
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

fn commit_fingerprint(request: &RunCommitRequest) -> Result<RequestFingerprint, ErrorCode> {
  fingerprint(b"auv.memory.commit-request.v1", request)
}

fn artifact_fingerprint(request: &StoreArtifactRequest) -> Result<RequestFingerprint, ErrorCode> {
  #[derive(Serialize)]
  struct Wire<'a> {
    authority_id: AuthorityId,
    run_id: RunId,
    idempotency_key: IdempotencyKey,
    artifact_id: crate::ArtifactId,
    span_id: Option<SpanId>,
    purpose: &'a ArtifactPurpose,
    content_type: &'a ContentType,
    expected_byte_length: ByteLength,
    expected_sha256: Sha256Digest,
    attributes: &'a Attributes,
  }

  fingerprint(
    b"auv.memory.artifact-request.v1",
    &Wire {
      authority_id: request.authority_id(),
      run_id: request.run_id(),
      idempotency_key: request.idempotency_key(),
      artifact_id: request.artifact_id(),
      span_id: request.span_id(),
      purpose: request.purpose(),
      content_type: request.content_type(),
      expected_byte_length: request.expected_byte_length(),
      expected_sha256: request.expected_sha256(),
      attributes: request.attributes(),
    },
  )
}

fn fingerprint(domain: &[u8], value: &impl Serialize) -> Result<RequestFingerprint, ErrorCode> {
  let encoded = serde_json::to_vec(value).map_err(|_| rejected())?;
  let mut hasher = Sha256::new();
  hasher.update((domain.len() as u64).to_be_bytes());
  hasher.update(domain);
  hasher.update(encoded);
  Ok(RequestFingerprint(hasher.finalize().into()))
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

fn release_artifact_reservation(state: &mut MemoryState, run_id: RunId, key: IdempotencyKey, reservation_id: u64) -> Vec<Waker> {
  let map_key = (run_id, key);
  if state.pending_artifacts.get(&map_key).is_none_or(|pending| pending.reservation_id != reservation_id) {
    return Vec::new();
  }
  let pending = state.pending_artifacts.remove(&map_key).expect("reservation was checked above");
  let owner = PendingArtifactOwner {
    run_id,
    key,
    reservation_id,
  };
  if state.pending_artifact_uris.get(&pending.uri) == Some(&owner) {
    state.pending_artifact_uris.remove(&pending.uri);
  }
  pending.waiters.into_values().collect()
}

fn take_subscription_waiters(state: &mut MemoryState, run_id: RunId) -> Vec<Waker> {
  state.subscription_waiters.remove(&run_id).map(|waiters| waiters.into_values().collect()).unwrap_or_default()
}

fn remove_subscription_waiter(state: &mut MemoryState, run_id: RunId, waiter_id: u64) {
  let remove_run = if let Some(waiters) = state.subscription_waiters.get_mut(&run_id) {
    waiters.remove(&waiter_id);
    waiters.is_empty()
  } else {
    false
  };
  if remove_run {
    state.subscription_waiters.remove(&run_id);
  }
}

fn allocate_token(state: &mut MemoryState) -> Result<u64, ErrorCode> {
  state.next_token = state.next_token.checked_add(1).ok_or_else(unavailable)?;
  Ok(state.next_token)
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
