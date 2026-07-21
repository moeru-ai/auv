use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
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
  root: PathBuf,
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

struct StoredRequest {
  fingerprint: RequestFingerprint,
  commit: RunCommit,
}

struct RunIndex {
  commits: Vec<RunCommit>,
  snapshot: Option<RunSnapshot>,
  idempotency: HashMap<IdempotencyKey, StoredRequest>,
}

struct ParsedLog {
  commits: Vec<RunCommit>,
  verified_bytes: u64,
  has_partial_tail: bool,
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
  directory: PathBuf,
  log: PathBuf,
}

struct TemporaryFile {
  path: PathBuf,
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
  pub fn open(root: impl AsRef<Path>) -> io::Result<Self> {
    let root = prepare_root(root.as_ref())?;
    ensure_directory(&root.join("runs"))?;
    ensure_directory(&root.join("blobs"))?;
    ensure_directory(&root.join("blobs").join("sha256"))?;
    ensure_directory(&root.join("tmp"))?;
    let authority_id = initialize_authority(&root)?;
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
    let next_index = index.with_commit(commit.clone()).map_err(|_| CommitError::Rejected(rejected()))?;
    append_commit(&lock, &commit).map_err(|error| match error {
      AppendError::Unavailable => CommitError::Unavailable(unavailable()),
      AppendError::Unknown => CommitError::CommitUnknown(unavailable()),
    })?;
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
    let lock = self.inner.acquire_run_lock(run_id).map_err(map_artifact_io)?;
    let index = self.inner.refresh_locked(run_id, &lock, true).map_err(map_artifact_refresh)?;
    if let Some(stored) = index.idempotency.get(&key) {
      return if stored.fingerprint == fingerprint {
        Ok(stored.commit.clone())
      } else {
        Err(ArtifactWriteError::IdempotencyMismatch)
      };
    }
    if index.snapshot.as_ref().is_some_and(|snapshot| snapshot.artifacts().contains_key(&uri)) {
      return Err(ArtifactWriteError::Rejected(rejected()));
    }

    let blob = self.inner.blob_path(request.expected_sha256());
    let blob_directory = blob.parent().expect("derived blob path has a parent");
    ensure_directory(blob_directory).map_err(map_artifact_io)?;
    reject_symlink(&blob).map_err(map_artifact_io)?;

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
    let next_index = index.with_commit(commit.clone()).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;

    let (mut temporary, mut file) = self.inner.create_temporary_file("artifact").map_err(map_artifact_io)?;
    stream_artifact(body, &mut file, request.expected_byte_length().get(), request.expected_sha256()).await?;
    file.sync_data().map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    drop(file);
    publish_blob(&mut temporary, &blob, request.expected_byte_length(), request.expected_sha256())?;

    append_commit(&lock, &commit).map_err(|error| match error {
      AppendError::Unavailable => ArtifactWriteError::Unavailable(unavailable()),
      AppendError::Unknown => ArtifactWriteError::PublicationUnknown(unavailable()),
    })?;
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
      let path = self.inner.blob_path(metadata.sha256());
      let file = open_private_file(&path, true, false, false).map_err(|error| {
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

impl FileAuthority {
  fn acquire_run_lock(&self, run_id: RunId) -> io::Result<RunLock> {
    ensure_directory(&self.root.join("runs"))?;
    let directory = self.root.join("runs").join(run_id.to_string());
    ensure_directory(&directory)?;
    let lock_path = directory.join("commit.lock");
    let file = open_private_file(&lock_path, true, true, true)?;
    FileExt::lock_exclusive(&file)?;
    reject_symlink(&directory)?;
    reject_symlink(&lock_path)?;
    Ok(RunLock {
      file,
      log: directory.join("commits.log"),
      directory,
    })
  }

  fn refresh(&self, run_id: RunId, repair: bool) -> Result<Arc<RunIndex>, RefreshError> {
    let lock = self.acquire_run_lock(run_id).map_err(RefreshError::Io)?;
    self.refresh_locked(run_id, &lock, repair)
  }

  fn refresh_locked(&self, run_id: RunId, lock: &RunLock, repair: bool) -> Result<Arc<RunIndex>, RefreshError> {
    reject_symlink(&lock.log).map_err(RefreshError::Io)?;
    let parsed = read_log(&lock.log)?;
    if repair && parsed.has_partial_tail {
      reject_symlink(&lock.log).map_err(RefreshError::Io)?;
      let file = OpenOptions::new().write(true).open(&lock.log).map_err(RefreshError::Io)?;
      reject_symlink(&lock.log).map_err(RefreshError::Io)?;
      file.set_len(parsed.verified_bytes).map_err(RefreshError::Io)?;
      file.sync_data().map_err(RefreshError::Io)?;
    }
    let index = Arc::new(RunIndex::build(self.authority_id, run_id, parsed.commits)?);
    self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(run_id, Arc::clone(&index));
    Ok(index)
  }

  fn install_index(&self, run_id: RunId, index: RunIndex) {
    self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(run_id, Arc::new(index));
  }

  fn blob_path(&self, digest: Sha256Digest) -> PathBuf {
    let digest = digest.to_string();
    self.root.join("blobs").join("sha256").join(&digest[..2]).join(digest)
  }

  fn create_temporary_file(&self, prefix: &str) -> io::Result<(TemporaryFile, File)> {
    let directory = self.root.join("tmp");
    ensure_directory(&directory)?;
    for _ in 0..8 {
      let name = format!("{prefix}-{}.tmp", uuid::Uuid::now_v7());
      let path = directory.join(name);
      match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => {
          return Ok((
            TemporaryFile {
              path,
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

  fn register_subscription(&self, run_id: RunId, waker: &Arc<AtomicWaker>) -> Result<u64, ReadError> {
    let mut registry = self.subscriptions.lock().map_err(|_| ReadError::Unavailable(unavailable()))?;
    registry.next_token = registry.next_token.checked_add(1).ok_or_else(|| ReadError::Unavailable(unavailable()))?;
    let token = registry.next_token;
    registry.entries.insert(
      token,
      SubscriptionWatch {
        run_id,
        waker: Arc::downgrade(waker),
        observed: file_signal(&self.run_log_path(run_id)),
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

  fn run_log_path(&self, run_id: RunId) -> PathBuf {
    self.root.join("runs").join(run_id.to_string()).join("commits.log")
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
  fn build(authority_id: AuthorityId, run_id: RunId, commits: Vec<RunCommit>) -> Result<Self, RefreshError> {
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
    })
  }

  fn latest_revision(&self) -> RunRevision {
    self.snapshot.as_ref().map(RunSnapshot::through_revision).unwrap_or_else(zero_revision)
  }

  fn with_commit(&self, commit: RunCommit) -> Result<Self, RefreshError> {
    let mut commits = self.commits.clone();
    commits.push(commit.clone());
    Self::build(commit.authority_id(), commit.run_id(), commits)
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
      let _ = fs::remove_file(&self.path);
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

fn prepare_root(root: &Path) -> io::Result<PathBuf> {
  match fs::symlink_metadata(root) {
    Ok(metadata) => validate_directory_metadata(root, &metadata)?,
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      fs::create_dir_all(root)?;
      validate_directory_metadata(root, &fs::symlink_metadata(root)?)?;
    }
    Err(error) => return Err(error),
  }
  root.canonicalize()
}

fn ensure_directory(path: &Path) -> io::Result<()> {
  match fs::symlink_metadata(path) {
    Ok(metadata) => validate_directory_metadata(path, &metadata),
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      match fs::create_dir(path) {
        Ok(()) => {
          if let Some(parent) = path.parent() {
            sync_directory(parent)?;
          }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
      }
      validate_directory_metadata(path, &fs::symlink_metadata(path)?)
    }
    Err(error) => Err(error),
  }
}

fn validate_directory_metadata(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("private store directory is not a real directory: {}", path.display())))
  } else {
    Ok(())
  }
}

fn reject_symlink(path: &Path) -> io::Result<()> {
  // NOTICE(file-store-symlink-toctou): Safe portable `std` has no atomic
  // no-follow open. Checks bracket every open, but a hostile concurrent local
  // filesystem mutation remains possible until a portable capability API exists.
  match fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_symlink() => {
      Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("private store path is a symlink: {}", path.display())))
    }
    Ok(_) => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  }
}

fn open_private_file(path: &Path, read: bool, write: bool, create: bool) -> io::Result<File> {
  reject_symlink(path)?;
  let file = OpenOptions::new().read(read).write(write).create(create).open(path)?;
  reject_symlink(path)?;
  if !file.metadata()?.is_file() {
    return Err(io::Error::new(io::ErrorKind::PermissionDenied, "private store path is not a regular file"));
  }
  if create && let Some(parent) = path.parent() {
    sync_directory(parent)?;
  }
  Ok(file)
}

fn initialize_authority(root: &Path) -> io::Result<AuthorityId> {
  let lock_path = root.join("authority.lock");
  let lock = open_private_file(&lock_path, true, true, true)?;
  FileExt::lock_exclusive(&lock)?;
  let result = match read_authority(&root.join("authority.json"))? {
    Some(authority_id) => Ok(authority_id),
    None => {
      let authority_id = AuthorityId::new();
      write_authority(root, authority_id)?;
      read_authority(&root.join("authority.json"))?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "authority file vanished"))
    }
  };
  let _ = FileExt::unlock(&lock);
  result
}

fn read_authority(path: &Path) -> io::Result<Option<AuthorityId>> {
  let mut file = match open_private_file(path, true, false, false) {
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

fn write_authority(root: &Path, authority_id: AuthorityId) -> io::Result<()> {
  let tmp_directory = root.join("tmp");
  ensure_directory(&tmp_directory)?;
  let temporary = tmp_directory.join(format!("authority-{}.tmp", uuid::Uuid::now_v7()));
  let bytes = serde_json::to_vec(&AuthorityFile {
    version: AUTHORITY_VERSION,
    authority_id,
  })
  .map_err(invalid_data)?;
  let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
  file.write_all(&bytes)?;
  file.sync_data()?;
  drop(file);
  fs::rename(&temporary, root.join("authority.json"))?;
  sync_directory(root)?;
  sync_directory(&tmp_directory)?;
  Ok(())
}

fn read_log(path: &Path) -> Result<ParsedLog, RefreshError> {
  let mut file = match open_private_file(path, true, false, false) {
    Ok(file) => file,
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      return Ok(ParsedLog {
        commits: Vec::new(),
        verified_bytes: 0,
        has_partial_tail: false,
      });
    }
    Err(error) => return Err(RefreshError::Io(error)),
  };
  if !file.metadata().map_err(RefreshError::Io)?.is_file() {
    return Err(RefreshError::Integrity);
  }
  let mut bytes = Vec::new();
  file.read_to_end(&mut bytes).map_err(RefreshError::Io)?;
  parse_log(&bytes)
}

fn parse_log(bytes: &[u8]) -> Result<ParsedLog, RefreshError> {
  let mut commits = Vec::new();
  let mut offset = 0_usize;
  while offset < bytes.len() {
    if bytes.len() - offset < FRAME_HEADER_BYTES {
      return Ok(ParsedLog {
        commits,
        verified_bytes: offset as u64,
        has_partial_tail: true,
      });
    }
    let frame = &bytes[offset..];
    if frame[..FRAME_MAGIC.len()] != FRAME_MAGIC {
      return Err(RefreshError::Integrity);
    }
    let version_start = FRAME_MAGIC.len();
    let version = u16::from_be_bytes(frame[version_start..version_start + 2].try_into().expect("fixed frame version bytes"));
    if version != FRAME_VERSION {
      return Err(RefreshError::Integrity);
    }
    let length_start = version_start + 2;
    let payload_length = u64::from_be_bytes(frame[length_start..length_start + 8].try_into().expect("fixed frame length bytes"));
    let payload_length = usize::try_from(payload_length).map_err(|_| RefreshError::Integrity)?;
    if payload_length == 0 || payload_length > MAX_FRAME_PAYLOAD_BYTES {
      return Err(RefreshError::Integrity);
    }
    let frame_length = FRAME_HEADER_BYTES.checked_add(payload_length).ok_or(RefreshError::Integrity)?;
    if frame.len() < frame_length {
      return Ok(ParsedLog {
        commits,
        verified_bytes: offset as u64,
        has_partial_tail: true,
      });
    }
    let digest_start = length_start + 8;
    let payload = &frame[FRAME_HEADER_BYTES..frame_length];
    let expected: [u8; 32] = frame[digest_start..digest_start + 32].try_into().expect("fixed frame digest bytes");
    if Sha256::digest(payload).as_slice() != expected {
      return Err(RefreshError::Integrity);
    }
    let commit = serde_json::from_slice(payload).map_err(|_| RefreshError::Integrity)?;
    commits.push(commit);
    offset = offset.checked_add(frame_length).ok_or(RefreshError::Integrity)?;
  }
  Ok(ParsedLog {
    commits,
    verified_bytes: offset as u64,
    has_partial_tail: false,
  })
}

fn append_commit(lock: &RunLock, commit: &RunCommit) -> Result<(), AppendError> {
  reject_symlink(&lock.log).map_err(|_| AppendError::Unavailable)?;
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
  let mut file = OpenOptions::new().append(true).create(true).open(&lock.log).map_err(|_| AppendError::Unavailable)?;
  reject_symlink(&lock.log).map_err(|_| AppendError::Unavailable)?;
  if !file.metadata().map_err(|_| AppendError::Unavailable)?.is_file() {
    return Err(AppendError::Unavailable);
  }
  file.write_all(&frame).map_err(|_| AppendError::Unknown)?;
  file.sync_data().map_err(|_| AppendError::Unknown)?;
  sync_directory(&lock.directory).map_err(|_| AppendError::Unknown)?;
  Ok(())
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
  blob: &Path,
  expected_length: ByteLength,
  expected_sha256: Sha256Digest,
) -> Result<(), ArtifactWriteError> {
  reject_symlink(blob).map_err(map_artifact_io)?;
  if blob.exists() {
    verify_blob(blob, expected_length, expected_sha256).map_err(|error| match error {
      RefreshError::Io(_) => ArtifactWriteError::Unavailable(unavailable()),
      RefreshError::Integrity => ArtifactWriteError::Integrity(integrity()),
    })?;
    fs::remove_file(&temporary.path).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    temporary.published = true;
  } else {
    match fs::rename(&temporary.path, blob) {
      Ok(()) => temporary.published = true,
      Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
        verify_blob(blob, expected_length, expected_sha256).map_err(|error| match error {
          RefreshError::Io(_) => ArtifactWriteError::Unavailable(unavailable()),
          RefreshError::Integrity => ArtifactWriteError::Integrity(integrity()),
        })?;
        fs::remove_file(&temporary.path).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
        temporary.published = true;
      }
      Err(_) => return Err(ArtifactWriteError::Unavailable(unavailable())),
    }
  }
  sync_directory(blob.parent().expect("derived blob path has a parent")).map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
  sync_directory(temporary.path.parent().expect("derived temporary path has a parent"))
    .map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
  // TODO(file-store-blob-gc): Crash-dangling blobs remain by contract; add
  // collection only with an owner-approved retention and liveness policy.
  Ok(())
}

fn verify_blob(path: &Path, expected_length: ByteLength, expected_sha256: Sha256Digest) -> Result<(), RefreshError> {
  let mut file = open_private_file(path, true, false, false).map_err(RefreshError::Io)?;
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
      let current = file_signal(&authority.run_log_path(entry.run_id));
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

fn file_signal(path: &Path) -> FileSignal {
  match fs::symlink_metadata(path) {
    Ok(metadata) => FileSignal {
      length: Some(metadata.len()),
      modified: metadata.modified().ok(),
      is_regular: metadata.is_file() && !metadata.file_type().is_symlink(),
    },
    Err(_) => FileSignal {
      length: None,
      modified: None,
      is_regular: false,
    },
  }
}

fn sync_directory(path: &Path) -> io::Result<()> {
  File::open(path)?.sync_all()
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
