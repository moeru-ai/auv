use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt;
use futures_core::Stream;
use futures_util::AsyncReadExt;
use futures_util::task::AtomicWaker;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactWriteError, BoxFuture, CommitError, ReadError, RunCommitPage, RunStore,
  RunSubscription, StoreArtifactRequest, SubscriptionError,
};
use crate::{
  ArtifactMetadata, ArtifactPublished, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode,
  IdempotencyKey, PageLimit, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunSnapshot, Sha256Digest, SpanId,
  Timestamp, reduce_commits,
};

const AUTHORITY_VERSION: u32 = 1;
const FRAME_MAGIC: [u8; 8] = *b"AUVRCMT\0";
const FRAME_VERSION: u16 = 1;
const FRAME_HEADER_BYTES: usize = FRAME_MAGIC.len() + size_of::<u16>() + size_of::<u64>() + 32;
const MAX_FRAME_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const ARTIFACT_CHUNK_BYTES: usize = 64 * 1024;

/// Crash-durable local authority with private commit logs and content-addressed blobs.
#[derive(Clone)]
pub struct FileRunStore {
  inner: Arc<FileAuthority>,
}

struct FileAuthority {
  root: Dir,
  authority_id: AuthorityId,
  cache: Mutex<HashMap<RunId, Arc<RunIndex>>>,
  subscriptions: Mutex<SubscriptionRegistry>,
  watcher: Mutex<Option<JoinHandle<()>>>,
  stopping: AtomicBool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityFile {
  version: u32,
  authority_id: AuthorityId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct RequestFingerprint([u8; 32]);

#[derive(Clone)]
struct StoredRequest {
  fingerprint: RequestFingerprint,
  commit: RunCommit,
}

#[derive(Clone)]
struct RunIndex {
  commits: Vec<RunCommit>,
  snapshot: Option<RunSnapshot>,
  idempotency: HashMap<IdempotencyKey, StoredRequest>,
  verified_bytes: u64,
  observed_bytes: u64,
  log_identity: Option<LogIdentity>,
  log_change: Option<LogChange>,
}

struct ParsedLog {
  commits: Vec<RunCommit>,
  verified_bytes: u64,
  has_partial_tail: bool,
  observed_bytes: u64,
  log_identity: Option<LogIdentity>,
  log_change: Option<LogChange>,
}

#[derive(Clone, Copy)]
struct LogStorage {
  verified_bytes: u64,
  observed_bytes: u64,
  log_identity: LogIdentity,
  log_change: LogChange,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LogIdentity {
  #[cfg(unix)]
  device: u64,
  #[cfg(unix)]
  inode: u64,
  #[cfg(windows)]
  volume: Option<u32>,
  #[cfg(windows)]
  index: Option<u64>,
  #[cfg(not(any(unix, windows)))]
  created: Option<SystemTime>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LogChange {
  modified: Option<SystemTime>,
  #[cfg(unix)]
  changed_seconds: i64,
  #[cfg(unix)]
  changed_nanoseconds: i64,
}

enum RefreshError {
  Io(io::Error),
  Integrity,
}

enum AppendError {
  Unavailable,
  Unknown,
}

struct RunLock {
  file: File,
  directory: Dir,
}

struct TemporaryFile {
  directory: Dir,
  name: PathBuf,
  published: bool,
}

#[derive(Default)]
struct SubscriptionRegistry {
  next_token: u64,
  entries: HashMap<u64, SubscriptionWatch>,
}

struct SubscriptionWatch {
  run_id: RunId,
  waker: Weak<AtomicWaker>,
  observed: FileSignal,
}

#[derive(Clone, PartialEq, Eq)]
struct FileSignal {
  length: Option<u64>,
  modified: Option<SystemTime>,
  is_regular: bool,
}

struct FileSubscription {
  authority: Arc<FileAuthority>,
  run_id: RunId,
  cursor: RunRevision,
  token: u64,
  waker: Arc<AtomicWaker>,
  terminal: bool,
}

struct FileArtifactReader {
  file: File,
  remaining: u64,
  expected_sha256: Sha256Digest,
  hasher: Sha256,
  terminal: bool,
}

impl FileRunStore {
  /// Opens or initializes one stable authority below an administrator-selected root.
  ///
  /// The caller owns the root and its existing ancestors while this call opens
  /// that trusted capability. All derived store paths are subsequently resolved
  /// relative to it with no-follow opens.
  pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
    let root = prepare_root(root.as_ref())?;
    ensure_directory(&root, "runs")?;
    let blobs = ensure_directory(&root, "blobs")?;
    ensure_directory(&blobs, "sha256")?;
    let tmp = ensure_directory(&root, "tmp")?;
    let authority_id = initialize_authority(&root, &tmp)?;
    Ok(Self {
      inner: Arc::new(FileAuthority {
        root,
        authority_id,
        cache: Mutex::new(HashMap::new()),
        subscriptions: Mutex::new(SubscriptionRegistry::default()),
        watcher: Mutex::new(None),
        stopping: AtomicBool::new(false),
      }),
    })
  }

  fn validate_authority(&self, received: AuthorityId) -> Result<(), (AuthorityId, AuthorityId)> {
    if received == self.inner.authority_id {
      Ok(())
    } else {
      Err((self.inner.authority_id, received))
    }
  }

  fn commit_ordinary(&self, request: RunCommitRequest) -> Result<RunCommit, CommitError> {
    self.validate_authority(request.authority_id()).map_err(|(expected, received)| CommitError::AuthorityMismatch { expected, received })?;
    let fingerprint = ordinary_fingerprint(&request).map_err(|_| CommitError::Rejected(rejected()))?;
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let lock = self.inner.acquire_run_lock(run_id).map_err(map_commit_io)?;
    let index = self.inner.refresh_locked(run_id, &lock, true).map_err(map_commit_refresh)?;
    if let Some(stored) = index.idempotency.get(&key) {
      return if stored.fingerprint == fingerprint {
        Ok(stored.commit.clone())
      } else {
        Err(CommitError::IdempotencyMismatch)
      };
    }

    let revision = next_revision(index.latest_revision()).map_err(CommitError::Rejected)?;
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
    let storage = append_commit(&lock, &commit).map_err(|error| match error {
      AppendError::Unavailable => CommitError::Unavailable(unavailable()),
      AppendError::Unknown => CommitError::CommitUnknown(unavailable()),
    })?;
    let next_index = index.with_commit(commit.clone(), storage).map_err(|_| CommitError::Rejected(rejected()))?;
    self.inner.install_index(run_id, next_index);
    self.inner.wake_run(run_id);
    Ok(commit)
  }

  async fn write_artifact_inner(&self, request: StoreArtifactRequest, body: ArtifactBody) -> Result<RunCommit, ArtifactWriteError> {
    self
      .validate_authority(request.authority_id())
      .map_err(|(expected, received)| ArtifactWriteError::AuthorityMismatch { expected, received })?;
    let fingerprint = artifact_fingerprint(&request).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
    let run_id = request.run_id();
    let key = request.idempotency_key();
    let uri = ArtifactUri::from_ids(run_id, request.artifact_id());
    {
      let lock = self.inner.acquire_run_lock(run_id).map_err(map_artifact_io)?;
      let index = self.inner.refresh_locked(run_id, &lock, true).map_err(map_artifact_refresh)?;
      if let Some(committed) = precheck_artifact(&index, key, fingerprint, &uri)? {
        return Ok(committed);
      }
    }

    let (mut temporary, mut file) = self.inner.create_temporary_file("artifact").map_err(map_artifact_io)?;
    stream_artifact(body, &mut file, request.expected_byte_length().get(), request.expected_sha256()).await?;
    file.sync_data().map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    drop(file);

    let lock = self.inner.acquire_run_lock(run_id).map_err(map_artifact_io)?;
    let index = self.inner.refresh_locked(run_id, &lock, true).map_err(map_artifact_refresh)?;
    if let Some(committed) = precheck_artifact(&index, key, fingerprint, &uri)? {
      return Ok(committed);
    }

    let revision = next_revision(index.latest_revision()).map_err(ArtifactWriteError::Rejected)?;
    let metadata = ArtifactMetadata::new(
      uri,
      request.purpose().clone(),
      request.content_type().clone(),
      request.expected_byte_length(),
      request.expected_sha256(),
      request.attributes().clone(),
    );
    let publication = ArtifactPublished::new(request.span_id(), metadata);
    let commit = RunCommit::new(self.inner.authority_id, run_id, revision, key, now(), vec![RunFact::ArtifactPublished(publication)])
      .map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
    let blob_directory = self.inner.blob_directory(request.expected_sha256()).map_err(map_artifact_io)?;
    publish_blob(&mut temporary, &blob_directory, request.expected_sha256(), request.expected_byte_length())?;

    let storage = append_commit(&lock, &commit).map_err(|error| match error {
      AppendError::Unavailable => ArtifactWriteError::Unavailable(unavailable()),
      AppendError::Unknown => ArtifactWriteError::PublicationUnknown(unavailable()),
    })?;
    let next_index = index.with_commit(commit.clone(), storage).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
    self.inner.install_index(run_id, next_index);
    self.inner.wake_run(run_id);
    Ok(commit)
  }
}

impl RunStore for FileRunStore {
  fn authority_id(&self) -> AuthorityId {
    self.inner.authority_id
  }

  fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<RunCommit, CommitError>> {
    Box::pin(async move { self.commit_ordinary(request) })
  }

  fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<RunCommit, ArtifactWriteError>> {
    Box::pin(self.write_artifact_inner(request, body))
  }

  fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
    Box::pin(async move {
      let index = self.inner.refresh(run_id, false).map_err(map_read_refresh)?;
      Ok(index.idempotency.get(&key).map(|stored| stored.commit.clone()))
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    Box::pin(async move {
      let index = self.inner.refresh(run_id, false).map_err(map_read_refresh)?;
      Ok(index.snapshot.clone())
    })
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async move {
      let index = self.inner.refresh(run_id, false).map_err(map_read_refresh)?;
      build_page(&index, after, limit)
    })
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async move {
      let index = self.inner.refresh(run_id, false).map_err(map_read_refresh)?;
      let latest = index.latest_revision();
      if after > latest {
        return Err(ReadError::CursorAhead {
          requested_after: after,
          latest,
        });
      }
      let waker = Arc::new(AtomicWaker::new());
      let token = self.inner.register_subscription(run_id, &waker)?;
      if let Err(error) = self.inner.ensure_watcher() {
        self.inner.remove_subscription(token);
        return Err(ReadError::Unavailable(error));
      }
      Ok(Box::pin(FileSubscription {
        authority: Arc::clone(&self.inner),
        run_id,
        cursor: after,
        token,
        waker,
        terminal: false,
      }) as RunSubscription)
    })
  }

  fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
    Box::pin(async move {
      let run_id = uri.run_id();
      let index = self.inner.refresh(run_id, false).map_err(map_read_refresh)?;
      let metadata = index
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.artifacts().get(&uri))
        .map(|publication| publication.metadata().clone())
        .ok_or(ReadError::NotFound)?;
      let directory = self.inner.blob_directory(metadata.sha256()).map_err(|_| ReadError::Integrity(integrity()))?;
      let file = open_private_file(&directory, metadata.sha256().to_string(), true, false, false, false, false).map_err(|error| {
        if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) {
          ReadError::Integrity(integrity())
        } else {
          ReadError::Unavailable(unavailable())
        }
      })?;
      let stored_length = file.metadata().map_err(|_| ReadError::Unavailable(unavailable()))?.len();
      if stored_length != metadata.byte_length().get() {
        return Err(ReadError::Integrity(integrity()));
      }
      Ok(Box::pin(FileArtifactReader {
        file,
        remaining: stored_length,
        expected_sha256: metadata.sha256(),
        hasher: Sha256::new(),
        terminal: false,
      }) as ArtifactReader)
    })
  }
}

fn precheck_artifact(
  index: &RunIndex,
  key: IdempotencyKey,
  fingerprint: RequestFingerprint,
  uri: &ArtifactUri,
) -> Result<Option<RunCommit>, ArtifactWriteError> {
  if let Some(stored) = index.idempotency.get(&key) {
    return if stored.fingerprint == fingerprint {
      Ok(Some(stored.commit.clone()))
    } else {
      Err(ArtifactWriteError::IdempotencyMismatch)
    };
  }
  if index.snapshot.as_ref().is_some_and(|snapshot| snapshot.artifacts().contains_key(uri)) {
    return Err(ArtifactWriteError::Rejected(rejected()));
  }
  Ok(None)
}

impl FileAuthority {
  fn acquire_run_lock(&self, run_id: RunId) -> io::Result<RunLock> {
    let runs = ensure_directory(&self.root, "runs")?;
    let directory = ensure_directory(&runs, run_id.to_string())?;
    let file = open_private_file(&directory, "commit.lock", true, true, true, false, false)?;
    FileExt::lock_exclusive(&file)?;
    Ok(RunLock { file, directory })
  }

  fn refresh(&self, run_id: RunId, repair: bool) -> Result<Arc<RunIndex>, RefreshError> {
    let lock = self.acquire_run_lock(run_id).map_err(RefreshError::Io)?;
    self.refresh_locked(run_id, &lock, repair)
  }

  fn refresh_locked(&self, run_id: RunId, lock: &RunLock, repair: bool) -> Result<Arc<RunIndex>, RefreshError> {
    let cached = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&run_id).cloned();
    let mut parsed = read_log(&lock.directory, cached.as_deref())?;
    if repair && parsed.has_partial_tail {
      let file = open_private_file(&lock.directory, "commits.log", false, true, false, false, false).map_err(RefreshError::Io)?;
      file.set_len(parsed.verified_bytes).map_err(RefreshError::Io)?;
      file.sync_data().map_err(RefreshError::Io)?;
      let metadata = file.metadata().map_err(RefreshError::Io)?;
      parsed.observed_bytes = metadata.len();
      parsed.log_identity = Some(LogIdentity::from_metadata(&metadata));
      parsed.log_change = Some(LogChange::from_metadata(&metadata));
    }
    let index = Arc::new(RunIndex::build(
      self.authority_id,
      run_id,
      parsed.commits,
      parsed.verified_bytes,
      parsed.observed_bytes,
      parsed.log_identity,
      parsed.log_change,
    )?);
    self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(run_id, Arc::clone(&index));
    Ok(index)
  }

  fn install_index(&self, run_id: RunId, index: RunIndex) {
    self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(run_id, Arc::new(index));
  }

  fn blob_directory(&self, digest: Sha256Digest) -> io::Result<Dir> {
    let blobs = ensure_directory(&self.root, "blobs")?;
    let sha256 = ensure_directory(&blobs, "sha256")?;
    let digest = digest.to_string();
    ensure_directory(&sha256, &digest[..2])
  }

  fn create_temporary_file(&self, prefix: &str) -> io::Result<(TemporaryFile, File)> {
    let tmp = ensure_directory(&self.root, "tmp")?;
    create_temporary_file(&tmp, prefix)
  }

  fn register_subscription(&self, run_id: RunId, waker: &Arc<AtomicWaker>) -> Result<u64, ReadError> {
    let mut registry = self.subscriptions.lock().map_err(|_| ReadError::Unavailable(unavailable()))?;
    registry.next_token = registry.next_token.checked_add(1).ok_or_else(|| ReadError::Unavailable(unavailable()))?;
    let token = registry.next_token;
    registry.entries.insert(
      token,
      SubscriptionWatch {
        run_id,
        waker: Arc::downgrade(waker),
        observed: self.file_signal(run_id),
      },
    );
    Ok(token)
  }

  fn remove_subscription(&self, token: u64) {
    if let Ok(mut registry) = self.subscriptions.lock() {
      registry.entries.remove(&token);
    }
  }

  fn wake_run(&self, run_id: RunId) {
    let wakers = match self.subscriptions.lock() {
      Ok(registry) => {
        registry.entries.values().filter(|entry| entry.run_id == run_id).filter_map(|entry| entry.waker.upgrade()).collect::<Vec<_>>()
      }
      Err(_) => return,
    };
    for waker in wakers {
      waker.wake();
    }
  }

  fn ensure_watcher(self: &Arc<Self>) -> Result<(), ErrorCode> {
    let mut watcher = self.watcher.lock().map_err(|_| unavailable())?;
    if watcher.as_ref().is_some_and(|handle| !handle.is_finished()) {
      return Ok(());
    }
    if let Some(handle) = watcher.take() {
      let _ = handle.join();
    }
    let authority = Arc::downgrade(self);
    let handle = thread::Builder::new()
      .name("auv-file-store-watch".to_owned())
      .spawn(move || watch_subscriptions(authority))
      .map_err(|_| unavailable())?;
    *watcher = Some(handle);
    Ok(())
  }

  fn file_signal(&self, run_id: RunId) -> FileSignal {
    let Ok(runs) = self.root.open_dir_nofollow("runs") else {
      return FileSignal::missing();
    };
    let Ok(directory) = runs.open_dir_nofollow(run_id.to_string()) else {
      return FileSignal::missing();
    };
    file_signal(&directory, "commits.log")
  }
}

impl Drop for FileAuthority {
  fn drop(&mut self) {
    self.stopping.store(true, Ordering::Release);
    let handle = match self.watcher.get_mut() {
      Ok(slot) => slot.take(),
      Err(poisoned) => poisoned.into_inner().take(),
    };
    if let Some(handle) = handle
      && handle.thread().id() != thread::current().id()
    {
      let _ = handle.join();
    }
  }
}

impl RunIndex {
  fn build(
    authority_id: AuthorityId,
    run_id: RunId,
    commits: Vec<RunCommit>,
    verified_bytes: u64,
    observed_bytes: u64,
    log_identity: Option<LogIdentity>,
    log_change: Option<LogChange>,
  ) -> Result<Self, RefreshError> {
    let mut idempotency = HashMap::new();
    for commit in &commits {
      if commit.authority_id() != authority_id || commit.run_id() != run_id {
        return Err(RefreshError::Integrity);
      }
      let fingerprint = stored_fingerprint(commit).map_err(|_| RefreshError::Integrity)?;
      if idempotency
        .insert(
          commit.idempotency_key(),
          StoredRequest {
            fingerprint,
            commit: commit.clone(),
          },
        )
        .is_some()
      {
        return Err(RefreshError::Integrity);
      }
    }
    let snapshot = if commits.is_empty() {
      None
    } else {
      Some(reduce_commits(&commits).map_err(|_| RefreshError::Integrity)?)
    };
    Ok(Self {
      commits,
      snapshot,
      idempotency,
      verified_bytes,
      observed_bytes,
      log_identity,
      log_change,
    })
  }

  fn latest_revision(&self) -> RunRevision {
    self.snapshot.as_ref().map(RunSnapshot::through_revision).unwrap_or_else(zero_revision)
  }

  fn with_commit(&self, commit: RunCommit, storage: LogStorage) -> Result<Self, RefreshError> {
    let mut commits = self.commits.clone();
    commits.push(commit.clone());
    Self::build(
      commit.authority_id(),
      commit.run_id(),
      commits,
      storage.verified_bytes,
      storage.observed_bytes,
      Some(storage.log_identity),
      Some(storage.log_change),
    )
  }
}

impl LogIdentity {
  fn from_metadata(metadata: &fs::Metadata) -> Self {
    #[cfg(unix)]
    {
      use std::os::unix::fs::MetadataExt;
      Self {
        device: metadata.dev(),
        inode: metadata.ino(),
      }
    }
    #[cfg(windows)]
    {
      use std::os::windows::fs::MetadataExt;
      Self {
        volume: metadata.volume_serial_number(),
        index: metadata.file_index(),
      }
    }
    #[cfg(not(any(unix, windows)))]
    {
      Self {
        created: metadata.created().ok(),
      }
    }
  }
}

impl LogChange {
  fn from_metadata(metadata: &fs::Metadata) -> Self {
    #[cfg(unix)]
    {
      use std::os::unix::fs::MetadataExt;
      Self {
        modified: metadata.modified().ok(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
      }
    }
    #[cfg(not(unix))]
    {
      Self {
        modified: metadata.modified().ok(),
      }
    }
  }
}

impl Drop for RunLock {
  fn drop(&mut self) {
    let _ = FileExt::unlock(&self.file);
  }
}

impl Drop for TemporaryFile {
  fn drop(&mut self) {
    if !self.published {
      let _ = self.directory.remove_file(&self.name);
    }
  }
}

impl Stream for FileSubscription {
  type Item = Result<RunCommit, SubscriptionError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.terminal {
      return Poll::Ready(None);
    }
    self.waker.register(cx.waker());
    let index = match self.authority.refresh(self.run_id, false) {
      Ok(index) => index,
      Err(error) => {
        self.terminal = true;
        return Poll::Ready(Some(Err(SubscriptionError::Store(map_read_refresh(error)))));
      }
    };
    if self.cursor < index.latest_revision() {
      let expected = self.cursor.get() + 1;
      let next = index.commits.iter().find(|commit| commit.revision().get() == expected).cloned();
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
    Poll::Pending
  }
}

impl Drop for FileSubscription {
  fn drop(&mut self) {
    self.authority.remove_subscription(self.token);
  }
}

impl Stream for FileArtifactReader {
  type Item = Result<Bytes, ArtifactReadError>;

  fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.terminal {
      return Poll::Ready(None);
    }
    if self.remaining == 0 {
      self.terminal = true;
      let digest = Sha256Digest::new(self.hasher.clone().finalize().into());
      return if digest == self.expected_sha256 {
        Poll::Ready(None)
      } else {
        Poll::Ready(Some(Err(ArtifactReadError::Integrity(integrity()))))
      };
    }

    let read_length = usize::try_from(self.remaining.min(ARTIFACT_CHUNK_BYTES as u64)).expect("artifact chunk length fits usize");
    let mut bytes = vec![0_u8; read_length];
    match self.file.read(&mut bytes) {
      Ok(0) => {
        self.terminal = true;
        Poll::Ready(Some(Err(ArtifactReadError::Integrity(integrity()))))
      }
      Ok(count) => {
        bytes.truncate(count);
        self.remaining -= count as u64;
        self.hasher.update(&bytes);
        Poll::Ready(Some(Ok(Bytes::from(bytes))))
      }
      Err(_) => {
        self.terminal = true;
        Poll::Ready(Some(Err(ArtifactReadError::Unavailable(unavailable()))))
      }
    }
  }
}

fn prepare_root(root: &Path) -> io::Result<Dir> {
  let absolute = std::path::absolute(root)?;
  let mut existing = absolute.as_path();
  let mut missing = Vec::<OsString>::new();
  let mut directory = loop {
    match fs::symlink_metadata(existing) {
      Ok(metadata) => {
        validate_directory_metadata(existing, &metadata)?;
        break Dir::open_ambient_dir(existing, ambient_authority())?;
      }
      Err(error) if error.kind() == io::ErrorKind::NotFound => {
        let name = existing.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "store root has no existing ancestor"))?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "store root has no existing ancestor"))?;
      }
      Err(error) => return Err(error),
    }
  };

  // The existing ancestor above is the administrator-selected trusted boundary.
  // Every component at or below the requested root is created and opened through
  // directory capabilities, and each newly durable name is synced in its parent.
  for name in missing.iter().rev() {
    directory = ensure_directory(&directory, name)?;
  }
  Ok(directory)
}

fn ensure_directory(parent: &Dir, name: impl AsRef<Path>) -> io::Result<Dir> {
  let name = name.as_ref();
  match parent.open_dir_nofollow(name) {
    Ok(directory) => Ok(directory),
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      match parent.create_dir(name) {
        Ok(()) => sync_directory(parent)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
      }
      parent.open_dir_nofollow(name).map_err(|error| private_path_error(parent, name, error))
    }
    Err(error) => Err(private_path_error(parent, name, error)),
  }
}

fn validate_directory_metadata(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("private store directory is not a real directory: {}", path.display())))
  } else {
    Ok(())
  }
}

#[allow(clippy::too_many_arguments)]
fn open_private_file(
  directory: &Dir,
  name: impl AsRef<Path>,
  read: bool,
  write: bool,
  create: bool,
  create_new: bool,
  append: bool,
) -> io::Result<File> {
  let name = name.as_ref();
  let mut options = OpenOptions::new();
  options.read(read).write(write).create(create).create_new(create_new).append(append);
  options.follow(FollowSymlinks::No);
  let file = directory.open_with(name, &options).map_err(|error| private_path_error(directory, name, error))?;
  if !file.metadata()?.is_file() {
    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "private store path is not a regular file"));
  }
  if create || create_new {
    sync_directory(directory)?;
  }
  Ok(file.into_std())
}

fn private_path_error(directory: &Dir, name: &Path, error: io::Error) -> io::Error {
  if directory.symlink_metadata(name).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
    io::Error::new(io::ErrorKind::PermissionDenied, "private store path is a symlink")
  } else {
    error
  }
}

fn initialize_authority(root: &Dir, tmp: &Dir) -> io::Result<AuthorityId> {
  let lock = open_private_file(root, "authority.lock", true, true, true, false, false)?;
  FileExt::lock_exclusive(&lock)?;
  let result = match read_authority(root)? {
    Some(authority_id) => Ok(authority_id),
    None => {
      let authority_id = AuthorityId::new();
      write_authority(root, tmp, authority_id)?;
      read_authority(root)?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "authority file vanished"))
    }
  };
  let _ = FileExt::unlock(&lock);
  result
}

fn read_authority(root: &Dir) -> io::Result<Option<AuthorityId>> {
  let mut file = match open_private_file(root, "authority.json", true, false, false, false, false) {
    Ok(file) => file,
    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error),
  };
  let mut bytes = Vec::new();
  file.read_to_end(&mut bytes)?;
  let authority: AuthorityFile = serde_json::from_slice(&bytes).map_err(invalid_data)?;
  if authority.version != AUTHORITY_VERSION {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported authority file version"));
  }
  Ok(Some(authority.authority_id))
}

fn write_authority(root: &Dir, tmp: &Dir, authority_id: AuthorityId) -> io::Result<()> {
  let (mut temporary, mut file) = create_temporary_file(tmp, "authority")?;
  let bytes = serde_json::to_vec(&AuthorityFile {
    version: AUTHORITY_VERSION,
    authority_id,
  })
  .map_err(invalid_data)?;
  file.write_all(&bytes)?;
  file.sync_data()?;
  drop(file);
  temporary.directory.rename(&temporary.name, root, "authority.json")?;
  temporary.published = true;
  sync_directory(root)?;
  sync_directory(tmp)?;
  Ok(())
}

fn read_log(directory: &Dir, cached: Option<&RunIndex>) -> Result<ParsedLog, RefreshError> {
  let mut file = match open_private_file(directory, "commits.log", true, false, false, false, false) {
    Ok(file) => file,
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      if cached.is_some_and(|index| index.verified_bytes != 0) {
        return Err(RefreshError::Integrity);
      }
      return Ok(ParsedLog {
        commits: Vec::new(),
        verified_bytes: 0,
        has_partial_tail: false,
        observed_bytes: 0,
        log_identity: None,
        log_change: None,
      });
    }
    Err(error) => return Err(RefreshError::Io(error)),
  };
  let metadata = file.metadata().map_err(RefreshError::Io)?;
  let log_identity = LogIdentity::from_metadata(&metadata);
  let log_change = LogChange::from_metadata(&metadata);
  let total_bytes = metadata.len();
  let (mut commits, mut offset) = match cached {
    Some(index)
      if index.log_identity == Some(log_identity)
        && (total_bytes > index.observed_bytes || (total_bytes == index.observed_bytes && index.log_change == Some(log_change))) =>
    {
      if total_bytes < index.verified_bytes {
        return Err(RefreshError::Integrity);
      }
      (index.commits.clone(), index.verified_bytes)
    }
    _ => (Vec::new(), 0),
  };
  file.seek(SeekFrom::Start(offset)).map_err(RefreshError::Io)?;

  while offset < total_bytes {
    let remaining = total_bytes.checked_sub(offset).ok_or(RefreshError::Integrity)?;
    if remaining < FRAME_HEADER_BYTES as u64 {
      return Ok(ParsedLog {
        commits,
        verified_bytes: offset,
        has_partial_tail: true,
        observed_bytes: total_bytes,
        log_identity: Some(log_identity),
        log_change: Some(log_change),
      });
    }
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    file.read_exact(&mut header).map_err(RefreshError::Io)?;
    if header[..FRAME_MAGIC.len()] != FRAME_MAGIC {
      return Err(RefreshError::Integrity);
    }
    let version_start = FRAME_MAGIC.len();
    let version = u16::from_be_bytes(header[version_start..version_start + 2].try_into().expect("fixed frame version bytes"));
    if version != FRAME_VERSION {
      return Err(RefreshError::Integrity);
    }
    let length_start = version_start + 2;
    let payload_length = u64::from_be_bytes(header[length_start..length_start + 8].try_into().expect("fixed frame length bytes"));
    let payload_length = usize::try_from(payload_length).map_err(|_| RefreshError::Integrity)?;
    if payload_length == 0 || payload_length > MAX_FRAME_PAYLOAD_BYTES {
      return Err(RefreshError::Integrity);
    }
    let frame_length = FRAME_HEADER_BYTES.checked_add(payload_length).ok_or(RefreshError::Integrity)? as u64;
    if remaining < frame_length {
      return Ok(ParsedLog {
        commits,
        verified_bytes: offset,
        has_partial_tail: true,
        observed_bytes: total_bytes,
        log_identity: Some(log_identity),
        log_change: Some(log_change),
      });
    }
    let digest_start = length_start + 8;
    let expected: [u8; 32] = header[digest_start..digest_start + 32].try_into().expect("fixed frame digest bytes");
    let mut payload = vec![0_u8; payload_length];
    file.read_exact(&mut payload).map_err(RefreshError::Io)?;
    if Sha256::digest(&payload).as_slice() != expected {
      return Err(RefreshError::Integrity);
    }
    let commit = serde_json::from_slice(&payload).map_err(|_| RefreshError::Integrity)?;
    commits.push(commit);
    offset = offset.checked_add(frame_length).ok_or(RefreshError::Integrity)?;
  }
  Ok(ParsedLog {
    commits,
    verified_bytes: offset,
    has_partial_tail: false,
    observed_bytes: total_bytes,
    log_identity: Some(log_identity),
    log_change: Some(log_change),
  })
}

fn append_commit(lock: &RunLock, commit: &RunCommit) -> Result<LogStorage, AppendError> {
  let payload = serde_json::to_vec(commit).map_err(|_| AppendError::Unavailable)?;
  if payload.is_empty() || payload.len() > MAX_FRAME_PAYLOAD_BYTES {
    return Err(AppendError::Unavailable);
  }
  let mut frame = Vec::with_capacity(FRAME_HEADER_BYTES + payload.len());
  frame.extend_from_slice(&FRAME_MAGIC);
  frame.extend_from_slice(&FRAME_VERSION.to_be_bytes());
  frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
  frame.extend_from_slice(&Sha256::digest(&payload));
  frame.extend_from_slice(&payload);
  let mut file = open_private_file(&lock.directory, "commits.log", false, false, true, false, true).map_err(|_| AppendError::Unavailable)?;
  file.write_all(&frame).map_err(|_| AppendError::Unknown)?;
  file.sync_data().map_err(|_| AppendError::Unknown)?;
  sync_directory(&lock.directory).map_err(|_| AppendError::Unknown)?;
  let metadata = file.metadata().map_err(|_| AppendError::Unknown)?;
  Ok(LogStorage {
    verified_bytes: metadata.len(),
    observed_bytes: metadata.len(),
    log_identity: LogIdentity::from_metadata(&metadata),
    log_change: LogChange::from_metadata(&metadata),
  })
}

async fn stream_artifact(
  mut body: ArtifactBody,
  file: &mut File,
  expected_length: u64,
  expected_sha256: Sha256Digest,
) -> Result<(), ArtifactWriteError> {
  let mut hasher = Sha256::new();
  let mut total = 0_u64;
  let mut chunk = [0_u8; ARTIFACT_CHUNK_BYTES];
  loop {
    let remaining_with_probe = expected_length.saturating_sub(total).saturating_add(1);
    let read_length = usize::try_from(remaining_with_probe.min(ARTIFACT_CHUNK_BYTES as u64)).expect("artifact chunk length fits usize");
    let count = body.read(&mut chunk[..read_length]).await.map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    if count == 0 {
      break;
    }
    total = total.checked_add(count as u64).ok_or_else(|| ArtifactWriteError::Integrity(integrity()))?;
    if total > expected_length {
      return Err(ArtifactWriteError::Integrity(integrity()));
    }
    hasher.update(&chunk[..count]);
    file.write_all(&chunk[..count]).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
  }
  if total != expected_length || Sha256Digest::new(hasher.finalize().into()) != expected_sha256 {
    return Err(ArtifactWriteError::Integrity(integrity()));
  }
  Ok(())
}

fn publish_blob(
  temporary: &mut TemporaryFile,
  blob_directory: &Dir,
  expected_sha256: Sha256Digest,
  expected_length: ByteLength,
) -> Result<(), ArtifactWriteError> {
  let blob_name = expected_sha256.to_string();
  match verify_blob(blob_directory, Path::new(&blob_name), expected_length, expected_sha256) {
    Ok(()) => {
      temporary.directory.remove_file(&temporary.name).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
      temporary.published = true;
    }
    Err(RefreshError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
      match temporary.directory.rename(&temporary.name, blob_directory, &blob_name) {
        Ok(()) => temporary.published = true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
          verify_blob(blob_directory, Path::new(&blob_name), expected_length, expected_sha256).map_err(|error| match error {
            RefreshError::Io(_) => ArtifactWriteError::Unavailable(unavailable()),
            RefreshError::Integrity => ArtifactWriteError::Integrity(integrity()),
          })?;
          temporary.directory.remove_file(&temporary.name).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
          temporary.published = true;
        }
        Err(_) => return Err(ArtifactWriteError::Unavailable(unavailable())),
      }
    }
    Err(error) => {
      return Err(match error {
        RefreshError::Io(_) => ArtifactWriteError::Unavailable(unavailable()),
        RefreshError::Integrity => ArtifactWriteError::Integrity(integrity()),
      });
    }
  }
  sync_directory(blob_directory).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
  sync_directory(&temporary.directory).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
  // TODO(file-store-blob-gc): Crash-dangling blobs remain by contract; add
  // collection only with an owner-approved retention and liveness policy.
  Ok(())
}

fn verify_blob(directory: &Dir, name: &Path, expected_length: ByteLength, expected_sha256: Sha256Digest) -> Result<(), RefreshError> {
  let mut file = open_private_file(directory, name, true, false, false, false, false).map_err(RefreshError::Io)?;
  if file.metadata().map_err(RefreshError::Io)?.len() != expected_length.get() {
    return Err(RefreshError::Integrity);
  }
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; ARTIFACT_CHUNK_BYTES];
  loop {
    let count = file.read(&mut buffer).map_err(RefreshError::Io)?;
    if count == 0 {
      break;
    }
    hasher.update(&buffer[..count]);
  }
  if Sha256Digest::new(hasher.finalize().into()) != expected_sha256 {
    return Err(RefreshError::Integrity);
  }
  Ok(())
}

fn create_temporary_file(directory: &Dir, prefix: &str) -> io::Result<(TemporaryFile, File)> {
  for _ in 0..8 {
    let name = PathBuf::from(format!("{prefix}-{}.tmp", uuid::Uuid::now_v7()));
    match open_private_file(directory, &name, false, true, false, true, false) {
      Ok(file) => {
        return Ok((
          TemporaryFile {
            directory: directory.try_clone()?,
            name,
            published: false,
          },
          file,
        ));
      }
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
      Err(error) => return Err(error),
    }
  }
  Err(io::Error::new(io::ErrorKind::AlreadyExists, "could not allocate a private temporary file"))
}

fn stored_fingerprint(commit: &RunCommit) -> Result<RequestFingerprint, ()> {
  if let [RunFact::ArtifactPublished(publication)] = commit.facts() {
    return artifact_publication_fingerprint(commit, publication);
  }
  if commit.facts().iter().any(|fact| matches!(fact, RunFact::ArtifactPublished(_))) {
    return Err(());
  }
  let mutations = commit
    .facts()
    .iter()
    .cloned()
    .map(|fact| match fact {
      RunFact::SpanStarted(started) => Ok(RunMutation::StartSpan(started)),
      RunFact::SpanEnded(ended) => Ok(RunMutation::EndSpan(ended)),
      RunFact::EventOccurred(event) => Ok(RunMutation::EmitEvent(event)),
      RunFact::ArtifactPublished(_) => Err(()),
    })
    .collect::<Result<Vec<_>, _>>()?;
  let request = RunCommitRequest::new(commit.authority_id(), commit.run_id(), commit.idempotency_key(), mutations).map_err(|_| ())?;
  ordinary_fingerprint(&request)
}

fn ordinary_fingerprint(request: &RunCommitRequest) -> Result<RequestFingerprint, ()> {
  fingerprint(b"auv.file.commit-request.v1", request)
}

fn artifact_fingerprint(request: &StoreArtifactRequest) -> Result<RequestFingerprint, ()> {
  artifact_fingerprint_fields(
    request.authority_id(),
    request.run_id(),
    request.idempotency_key(),
    request.artifact_id(),
    request.span_id(),
    request.purpose(),
    request.content_type(),
    request.expected_byte_length(),
    request.expected_sha256(),
    request.attributes(),
  )
}

fn artifact_publication_fingerprint(commit: &RunCommit, publication: &ArtifactPublished) -> Result<RequestFingerprint, ()> {
  let metadata = publication.metadata();
  artifact_fingerprint_fields(
    commit.authority_id(),
    commit.run_id(),
    commit.idempotency_key(),
    metadata.uri().artifact_id(),
    publication.span_id(),
    metadata.purpose(),
    metadata.content_type(),
    metadata.byte_length(),
    metadata.sha256(),
    metadata.attributes(),
  )
}

#[allow(clippy::too_many_arguments)]
fn artifact_fingerprint_fields(
  authority_id: AuthorityId,
  run_id: RunId,
  idempotency_key: IdempotencyKey,
  artifact_id: crate::ArtifactId,
  span_id: Option<SpanId>,
  purpose: &ArtifactPurpose,
  content_type: &ContentType,
  expected_byte_length: ByteLength,
  expected_sha256: Sha256Digest,
  attributes: &Attributes,
) -> Result<RequestFingerprint, ()> {
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
    b"auv.file.artifact-request.v1",
    &Wire {
      authority_id,
      run_id,
      idempotency_key,
      artifact_id,
      span_id,
      purpose,
      content_type,
      expected_byte_length,
      expected_sha256,
      attributes,
    },
  )
}

fn fingerprint(domain: &[u8], value: &impl Serialize) -> Result<RequestFingerprint, ()> {
  let encoded = serde_json::to_vec(value).map_err(|_| ())?;
  let mut hasher = Sha256::new();
  hasher.update((domain.len() as u64).to_be_bytes());
  hasher.update(domain);
  hasher.update(encoded);
  Ok(RequestFingerprint(hasher.finalize().into()))
}

fn build_page(index: &RunIndex, after: RunRevision, limit: PageLimit) -> Result<RunCommitPage, ReadError> {
  let latest = index.latest_revision();
  if after > latest {
    return Err(ReadError::CursorAhead {
      requested_after: after,
      latest,
    });
  }
  if after == latest {
    return RunCommitPage::new(Vec::new(), after, false).map_err(|_| ReadError::Integrity(integrity()));
  }
  let start = usize::try_from(after.get()).map_err(|_| ReadError::Integrity(integrity()))?;
  let max_count = usize::try_from(limit.get().get()).expect("page limit fits usize");
  let mut selected = Vec::new();
  let mut commit_bytes = 0_usize;
  for commit in index.commits.iter().skip(start).take(max_count) {
    let encoded = serde_json::to_vec(commit).map_err(|_| ReadError::Integrity(integrity()))?;
    let candidate_count = selected.len() + 1;
    let has_more = commit.revision() < latest;
    let candidate_bytes = commit_bytes + encoded.len();
    if compact_page_len(candidate_bytes, candidate_count, commit.revision(), has_more) > super::MAX_COMMIT_PAGE_JSON_BYTES {
      break;
    }
    commit_bytes = candidate_bytes;
    selected.push(commit.clone());
  }
  if selected.is_empty() {
    return Err(ReadError::Integrity(integrity()));
  }
  let last_revision = selected.last().expect("page made progress").revision();
  RunCommitPage::new(selected, last_revision, last_revision < latest).map_err(|_| ReadError::Integrity(integrity()))
}

fn compact_page_len(commit_bytes: usize, commit_count: usize, last_revision: RunRevision, has_more: bool) -> usize {
  "{\"commits\":[".len()
    + commit_bytes
    + commit_count.saturating_sub(1)
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

fn watch_subscriptions(authority: Weak<FileAuthority>) {
  loop {
    // NOTICE(file-store-subscription-poll): stable Rust has no portable filesystem
    // notification API, so external-process commits are detected at a private 25 ms cadence.
    thread::sleep(Duration::from_millis(25));
    let Some(authority) = authority.upgrade() else {
      return;
    };
    if authority.stopping.load(Ordering::Acquire) {
      return;
    }
    let mut wake = Vec::new();
    let mut registry = match authority.subscriptions.lock() {
      Ok(registry) => registry,
      Err(_) => return,
    };
    registry.entries.retain(|_, entry| {
      let Some(waker) = entry.waker.upgrade() else {
        return false;
      };
      let current = authority.file_signal(entry.run_id);
      if current != entry.observed {
        entry.observed = current;
        wake.push(waker);
      }
      true
    });
    let empty = registry.entries.is_empty();
    drop(registry);
    for waker in wake {
      waker.wake();
    }
    if empty {
      let mut watcher = match authority.watcher.lock() {
        Ok(watcher) => watcher,
        Err(_) => return,
      };
      let registry = match authority.subscriptions.lock() {
        Ok(registry) => registry,
        Err(_) => return,
      };
      if registry.entries.is_empty() {
        watcher.take();
        return;
      }
    }
    drop(authority);
  }
}

fn file_signal(directory: &Dir, name: impl AsRef<Path>) -> FileSignal {
  match directory.symlink_metadata(name) {
    Ok(metadata) => FileSignal {
      length: Some(metadata.len()),
      modified: metadata.modified().ok().map(cap_std::time::SystemTime::into_std),
      is_regular: metadata.is_file() && !metadata.file_type().is_symlink(),
    },
    Err(_) => FileSignal::missing(),
  }
}

impl FileSignal {
  fn missing() -> Self {
    Self {
      length: None,
      modified: None,
      is_regular: false,
    }
  }
}

fn sync_directory(directory: &Dir) -> io::Result<()> {
  directory.try_clone()?.into_std_file().sync_all()
}

fn invalid_data(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, error)
}

fn map_commit_io(error: io::Error) -> CommitError {
  if error.kind() == io::ErrorKind::PermissionDenied {
    CommitError::Rejected(rejected())
  } else {
    CommitError::Unavailable(unavailable())
  }
}

fn map_commit_refresh(error: RefreshError) -> CommitError {
  match error {
    RefreshError::Io(error) => map_commit_io(error),
    RefreshError::Integrity => CommitError::Unavailable(integrity()),
  }
}

fn map_artifact_io(error: io::Error) -> ArtifactWriteError {
  if error.kind() == io::ErrorKind::PermissionDenied {
    ArtifactWriteError::Rejected(rejected())
  } else {
    ArtifactWriteError::Unavailable(unavailable())
  }
}

fn map_artifact_refresh(error: RefreshError) -> ArtifactWriteError {
  match error {
    RefreshError::Io(error) => map_artifact_io(error),
    RefreshError::Integrity => ArtifactWriteError::Integrity(integrity()),
  }
}

fn map_read_refresh(error: RefreshError) -> ReadError {
  match error {
    RefreshError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => ReadError::Integrity(integrity()),
    RefreshError::Io(_) => ReadError::Unavailable(unavailable()),
    RefreshError::Integrity => ReadError::Integrity(integrity()),
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
