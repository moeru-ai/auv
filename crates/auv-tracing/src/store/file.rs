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
#[cfg(windows)]
use cap_fs_ext::{OpenOptionsExt, OpenOptionsMaybeDirExt};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use fs2::FileExt;
use futures_core::Stream;
use futures_util::AsyncReadExt;
use futures_util::task::AtomicWaker;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use windows_sys::Win32::Foundation::GENERIC_WRITE;
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::{
  ArtifactBody, ArtifactReadError, ArtifactReader, ArtifactWriteError, BoxFuture, CommitError, ReadError, RunCommitPage, RunStore,
  RunSubscription, StoreArtifactRequest, SubscriptionError,
};
use crate::history::IncrementalReducer;
use crate::{
  ArtifactMetadata, ArtifactPublished, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode,
  IdempotencyKey, PageLimit, RunCommit, RunCommitRequest, RunFact, RunId, RunMutation, RunRevision, RunSnapshot, Sha256Digest, SpanId,
  Timestamp,
};

const AUTHORITY_VERSION: u32 = 1;
const MAX_AUTHORITY_FILE_BYTES: u64 = 4 * 1024;
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
  cache: Mutex<HashMap<RunId, Arc<Mutex<RunIndex>>>>,
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

struct RunIndex {
  commits: Vec<RunCommit>,
  reducer: IncrementalReducer,
  idempotency: HashMap<IdempotencyKey, StoredRequest>,
  verified_bytes: u64,
  log_identity: Option<LogIdentity>,
  verified_hasher: Sha256,
  verified_digest: [u8; 32],
  observed_signature: Option<LogSignature>,
  has_partial_tail: bool,
}

struct ParsedTail {
  commits: Vec<RunCommit>,
  verified_bytes: u64,
  has_partial_tail: bool,
  signature: LogSignature,
  verified_hasher: Sha256,
  verified_digest: [u8; 32],
}

struct LogStorage {
  verified_bytes: u64,
  log_identity: LogIdentity,
  verified_hasher: Sha256,
  verified_digest: [u8; 32],
  observed_signature: LogSignature,
}

#[derive(Clone, PartialEq, Eq)]
struct LogIdentity {
  #[cfg(unix)]
  device: u64,
  #[cfg(unix)]
  inode: u64,
  #[cfg(windows)]
  handle: Arc<same_file::Handle>,
  #[cfg(not(any(unix, windows)))]
  created: Option<SystemTime>,
}

#[derive(Clone, PartialEq, Eq)]
struct LogSignature {
  identity: LogIdentity,
  length: u64,
  modified: Option<SystemTime>,
  #[cfg(unix)]
  changed_seconds: i64,
  #[cfg(unix)]
  changed_nanoseconds: i64,
  #[cfg(windows)]
  created_ticks: u64,
  #[cfg(not(any(unix, windows)))]
  created: Option<SystemTime>,
}

enum RefreshError {
  Io(io::Error),
  Integrity,
}

enum AppendError {
  Unavailable,
  Integrity,
  Unknown,
}

struct RunLock {
  file: File,
  directory: Dir,
  log: Option<File>,
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
    let mut lock = self.inner.acquire_run_lock(run_id).map_err(map_commit_io)?;
    let cached = self.inner.cached_index(run_id);
    let mut index = cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    self.inner.refresh_locked(&mut lock, true, &mut index).map_err(map_commit_refresh)?;
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
    index.validate_commit(&commit).map_err(|_| CommitError::Rejected(rejected()))?;
    let storage = append_commit(&mut lock, &commit, &index).map_err(|error| match error {
      AppendError::Unavailable => CommitError::Unavailable(unavailable()),
      AppendError::Integrity => CommitError::Unavailable(integrity()),
      AppendError::Unknown => CommitError::CommitUnknown(unavailable()),
    })?;
    index.install_commit(commit.clone(), fingerprint, storage).map_err(|_| CommitError::CommitUnknown(integrity()))?;
    drop(index);
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
      let checked =
        self.inner.with_index(run_id, true, |index| precheck_artifact(index, key, fingerprint, &uri)).map_err(map_artifact_refresh)?;
      if let Some(committed) = checked? {
        return Ok(committed);
      }
    }

    let (mut temporary, mut file) = self.inner.create_temporary_file("artifact").map_err(map_artifact_io)?;
    stream_artifact(body, &mut file, request.expected_byte_length().get(), request.expected_sha256()).await?;
    file.sync_data().map_err(|_| ArtifactWriteError::Unavailable(unavailable()))?;
    drop(file);

    let mut lock = self.inner.acquire_run_lock(run_id).map_err(map_artifact_io)?;
    let cached = self.inner.cached_index(run_id);
    let mut index = cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    self.inner.refresh_locked(&mut lock, true, &mut index).map_err(map_artifact_refresh)?;
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
    index.validate_commit(&commit).map_err(|_| ArtifactWriteError::Rejected(rejected()))?;
    let blob_directory = self.inner.blob_directory(request.expected_sha256()).map_err(map_artifact_io)?;
    publish_blob(&mut temporary, &blob_directory, request.expected_sha256(), request.expected_byte_length())?;

    let storage = append_commit(&mut lock, &commit, &index).map_err(|error| match error {
      AppendError::Unavailable => ArtifactWriteError::Unavailable(unavailable()),
      AppendError::Integrity => ArtifactWriteError::Integrity(integrity()),
      AppendError::Unknown => ArtifactWriteError::PublicationUnknown(unavailable()),
    })?;
    index.install_commit(commit.clone(), fingerprint, storage).map_err(|_| ArtifactWriteError::PublicationUnknown(integrity()))?;
    drop(index);
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
      self.inner.with_index(run_id, false, |index| index.idempotency.get(&key).map(|stored| stored.commit.clone())).map_err(map_read_refresh)
    })
  }

  fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
    Box::pin(async move {
      self
        .inner
        .with_index(run_id, false, |index| (!index.commits.is_empty()).then(|| index.reducer.snapshot().clone()))
        .map_err(map_read_refresh)
    })
  }

  fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
    Box::pin(async move { self.inner.with_index(run_id, false, |index| build_page(index, after, limit)).map_err(map_read_refresh)? })
  }

  fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
    Box::pin(async move {
      let latest = self.inner.with_index(run_id, false, RunIndex::latest_revision).map_err(map_read_refresh)?;
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
      let metadata = self
        .inner
        .with_index(run_id, false, |index| index.reducer.snapshot().artifacts().get(&uri).map(|publication| publication.metadata().clone()))
        .map_err(map_read_refresh)?
        .ok_or(ReadError::NotFound)?;
      let directory = self.inner.blob_directory(metadata.sha256()).map_err(|_| ReadError::Integrity(integrity()))?;
      let file = open_private_file(&directory, metadata.sha256().to_string(), true, false, false, false).map_err(|error| {
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
  if index.reducer.snapshot().artifacts().contains_key(uri) {
    return Err(ArtifactWriteError::Rejected(rejected()));
  }
  Ok(None)
}

impl FileAuthority {
  fn acquire_run_lock(&self, run_id: RunId) -> io::Result<RunLock> {
    let runs = ensure_directory(&self.root, "runs")?;
    let directory = ensure_directory(&runs, run_id.to_string())?;
    let file = open_private_file(&directory, "commit.lock", true, true, true, false)?;
    FileExt::lock_exclusive(&file)?;
    Ok(RunLock {
      file,
      directory,
      log: None,
    })
  }

  fn with_index<T>(&self, run_id: RunId, repair: bool, read: impl FnOnce(&RunIndex) -> T) -> Result<T, RefreshError> {
    let mut lock = self.acquire_run_lock(run_id).map_err(RefreshError::Io)?;
    let cached = self.cached_index(run_id);
    let mut index = cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    self.refresh_locked(&mut lock, repair, &mut index)?;
    Ok(read(&index))
  }

  fn cached_index(&self, run_id: RunId) -> Arc<Mutex<RunIndex>> {
    let mut cache = self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(cache.entry(run_id).or_insert_with(|| Arc::new(Mutex::new(RunIndex::new(self.authority_id, run_id)))))
  }

  fn refresh_locked(&self, lock: &mut RunLock, repair: bool, index: &mut RunIndex) -> Result<(), RefreshError> {
    if lock.log.is_none() {
      match open_private_file(&lock.directory, "commits.log", true, repair, false, false) {
        Ok(file) => lock.log = Some(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(RefreshError::Io(error)),
      }
    }
    let parsed = read_log_tail(lock.log.as_mut(), index)?;
    if let Some(parsed) = parsed {
      index.apply_tail(parsed)?;
    }
    if repair && index.has_partial_tail {
      verify_log_binding(lock)?;
      {
        let file = lock.log.as_mut().ok_or(RefreshError::Integrity)?;
        file.set_len(index.verified_bytes).map_err(RefreshError::Io)?;
        file.sync_data().map_err(RefreshError::Io)?;
      }
      verify_log_binding(lock)?;
      let signature = LogSignature::from_file(lock.log.as_ref().ok_or(RefreshError::Integrity)?).map_err(RefreshError::Io)?;
      if signature.length != index.verified_bytes || index.log_identity.as_ref() != Some(&signature.identity) {
        return Err(RefreshError::Integrity);
      }
      index.observed_signature = Some(signature);
      index.has_partial_tail = false;
    }
    Ok(())
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
  fn new(authority_id: AuthorityId, run_id: RunId) -> Self {
    let verified_hasher = Sha256::new();
    let verified_digest = verified_hasher.clone().finalize().into();
    Self {
      commits: Vec::new(),
      reducer: IncrementalReducer::new(authority_id, run_id),
      idempotency: HashMap::new(),
      verified_bytes: 0,
      log_identity: None,
      verified_hasher,
      verified_digest,
      observed_signature: None,
      has_partial_tail: false,
    }
  }

  fn latest_revision(&self) -> RunRevision {
    self.reducer.snapshot().through_revision()
  }

  fn validate_commit(&self, commit: &RunCommit) -> Result<(), RefreshError> {
    self.reducer.validate(commit).map_err(|_| RefreshError::Integrity)
  }

  fn apply_tail(&mut self, parsed: ParsedTail) -> Result<(), RefreshError> {
    for commit in parsed.commits {
      let fingerprint = stored_fingerprint(&commit).map_err(|_| RefreshError::Integrity)?;
      if self.idempotency.contains_key(&commit.idempotency_key()) {
        return Err(RefreshError::Integrity);
      }
      self.reducer.apply(&commit).map_err(|_| RefreshError::Integrity)?;
      #[cfg(test)]
      tests::record_reducer_apply();
      self.idempotency.insert(
        commit.idempotency_key(),
        StoredRequest {
          fingerprint,
          commit: commit.clone(),
        },
      );
      self.commits.push(commit);
    }
    self.verified_bytes = parsed.verified_bytes;
    self.log_identity = Some(parsed.signature.identity.clone());
    self.verified_hasher = parsed.verified_hasher;
    self.verified_digest = parsed.verified_digest;
    self.observed_signature = Some(parsed.signature);
    self.has_partial_tail = parsed.has_partial_tail;
    Ok(())
  }

  fn install_commit(&mut self, commit: RunCommit, fingerprint: RequestFingerprint, storage: LogStorage) -> Result<(), RefreshError> {
    if self.idempotency.contains_key(&commit.idempotency_key()) {
      return Err(RefreshError::Integrity);
    }
    self.reducer.apply(&commit).map_err(|_| RefreshError::Integrity)?;
    #[cfg(test)]
    tests::record_reducer_apply();
    self.idempotency.insert(
      commit.idempotency_key(),
      StoredRequest {
        fingerprint,
        commit: commit.clone(),
      },
    );
    self.commits.push(commit);
    self.verified_bytes = storage.verified_bytes;
    self.log_identity = Some(storage.log_identity);
    self.verified_hasher = storage.verified_hasher;
    self.verified_digest = storage.verified_digest;
    self.observed_signature = Some(storage.observed_signature);
    self.has_partial_tail = false;
    Ok(())
  }
}

impl LogIdentity {
  fn from_file(file: &File) -> io::Result<Self> {
    #[cfg(unix)]
    {
      use std::os::unix::fs::MetadataExt;
      let metadata = file.metadata()?;
      Ok(Self {
        device: metadata.dev(),
        inode: metadata.ino(),
      })
    }
    #[cfg(windows)]
    {
      Ok(Self {
        handle: Arc::new(same_file::Handle::from_file(file.try_clone()?)?),
      })
    }
    #[cfg(not(any(unix, windows)))]
    {
      Ok(Self {
        created: file.metadata()?.created().ok(),
      })
    }
  }
}

impl LogSignature {
  fn from_file(file: &File) -> io::Result<Self> {
    let metadata = file.metadata()?;
    Ok(Self {
      identity: LogIdentity::from_file(file)?,
      length: metadata.len(),
      modified: metadata.modified().ok(),
      #[cfg(unix)]
      changed_seconds: {
        use std::os::unix::fs::MetadataExt;
        metadata.ctime()
      },
      #[cfg(unix)]
      changed_nanoseconds: {
        use std::os::unix::fs::MetadataExt;
        metadata.ctime_nsec()
      },
      #[cfg(windows)]
      created_ticks: {
        use std::os::windows::fs::MetadataExt;
        metadata.creation_time()
      },
      #[cfg(not(any(unix, windows)))]
      created: metadata.created().ok(),
    })
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
    let expected = self.cursor.get() + 1;
    let state = match self.authority.with_index(self.run_id, false, |index| {
      (index.latest_revision(), index.commits.iter().find(|commit| commit.revision().get() == expected).cloned())
    }) {
      Ok(state) => state,
      Err(error) => {
        self.terminal = true;
        return Poll::Ready(Some(Err(SubscriptionError::Store(map_read_refresh(error)))));
      }
    };
    if self.cursor < state.0 {
      return match state.1 {
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
  match fs::symlink_metadata(&absolute) {
    Ok(metadata) => {
      validate_directory_metadata(&absolute, &metadata)?;
      return open_durable_configured_root(&absolute);
    }
    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
    Err(error) => return Err(error),
  }

  let mut existing = absolute.as_path();
  let mut missing = Vec::<OsString>::new();
  let mut directory = loop {
    let name = existing.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "store root has no existing ancestor"))?;
    missing.push(name.to_owned());
    existing = existing.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "store root has no existing ancestor"))?;
    match fs::metadata(existing) {
      Ok(metadata) if metadata.is_dir() => break open_trusted_ancestor(existing)?,
      Ok(_) => return Err(io::Error::new(io::ErrorKind::NotADirectory, "trusted store ancestor is not a directory")),
      Err(error) if error.kind() == io::ErrorKind::NotFound => {}
      Err(error) => return Err(error),
    }
  };

  // The administrator-selected path above a missing configured root is trusted
  // and may contain symlinks. Every missing component at or below the configured
  // root re-enters the no-follow capability boundary before it is made durable.
  for name in missing.iter().rev() {
    directory = ensure_durable_directory(&directory, name)?;
  }
  Ok(directory)
}

fn open_durable_configured_root(root: &Path) -> io::Result<Dir> {
  if let (Some(parent), Some(name)) = (root.parent(), root.file_name()) {
    let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
    ensure_durable_directory(&parent, name)
  } else {
    Dir::open_ambient_dir(root, ambient_authority())
  }
}

fn open_trusted_ancestor(path: &Path) -> io::Result<Dir> {
  if let Some(parent) = path.parent() {
    let parent = Dir::open_ambient_dir(parent, ambient_authority())?;
    sync_directory(&parent)?;
  }
  Dir::open_ambient_dir(path, ambient_authority())
}

fn ensure_durable_directory(parent: &Dir, name: impl AsRef<Path>) -> io::Result<Dir> {
  let (directory, _) = open_or_create_directory(parent, name.as_ref())?;
  // A concurrent creator may have won before either our initial open or our
  // create attempt. Sync unconditionally so every requested root component's
  // parent entry is durable before `FileRunStore::open` returns.
  sync_directory(parent)?;
  Ok(directory)
}

fn ensure_directory(parent: &Dir, name: impl AsRef<Path>) -> io::Result<Dir> {
  let (directory, created) = open_or_create_directory(parent, name.as_ref())?;
  if created {
    sync_directory(parent)?;
  }
  Ok(directory)
}

fn open_or_create_directory(parent: &Dir, name: &Path) -> io::Result<(Dir, bool)> {
  match parent.open_dir_nofollow(name) {
    Ok(directory) => Ok((directory, false)),
    Err(error) if error.kind() == io::ErrorKind::NotFound => {
      let created = match parent.create_dir(name) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error),
      };
      parent.open_dir_nofollow(name).map(|directory| (directory, created)).map_err(|error| private_path_error(parent, name, error))
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

fn open_private_file(directory: &Dir, name: impl AsRef<Path>, read: bool, write: bool, create: bool, create_new: bool) -> io::Result<File> {
  let name = name.as_ref();
  let mut options = OpenOptions::new();
  options.read(read).write(write).create(create).create_new(create_new);
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
  let lock = open_private_file(root, "authority.lock", true, true, true, false)?;
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
  let mut file = match open_private_file(root, "authority.json", true, false, false, false) {
    Ok(file) => file,
    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error),
  };
  let bytes = read_authority_bytes(&mut file)?;
  let authority: AuthorityFile = serde_json::from_slice(&bytes).map_err(invalid_data)?;
  if authority.version != AUTHORITY_VERSION {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "unsupported authority file version"));
  }
  Ok(Some(authority.authority_id))
}

fn read_authority_bytes(file: &mut File) -> io::Result<Vec<u8>> {
  if file.metadata()?.len() > MAX_AUTHORITY_FILE_BYTES {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "authority file exceeds the fixed size limit"));
  }
  let mut bytes = Vec::with_capacity(MAX_AUTHORITY_FILE_BYTES as usize);
  (&mut *file).take(MAX_AUTHORITY_FILE_BYTES + 1).read_to_end(&mut bytes)?;
  if bytes.len() as u64 > MAX_AUTHORITY_FILE_BYTES {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "authority file exceeds the fixed size limit"));
  }
  Ok(bytes)
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

fn read_log_tail(file: Option<&mut File>, index: &RunIndex) -> Result<Option<ParsedTail>, RefreshError> {
  let Some(file) = file else {
    if index.log_identity.is_some() || !index.commits.is_empty() || index.verified_bytes != 0 {
      return Err(RefreshError::Integrity);
    }
    return Ok(None);
  };
  let signature = LogSignature::from_file(file).map_err(RefreshError::Io)?;
  let mut hasher = match index.log_identity.as_ref() {
    Some(identity) => {
      if identity != &signature.identity || signature.length < index.verified_bytes {
        return Err(RefreshError::Integrity);
      }
      if index.observed_signature.as_ref() == Some(&signature) {
        return Ok(None);
      }
      hash_verified_prefix(file, index.verified_bytes, index.verified_digest)?
    }
    None if index.commits.is_empty() && index.verified_bytes == 0 => Sha256::new(),
    None => return Err(RefreshError::Integrity),
  };
  parse_log_tail(file, index.verified_bytes, signature, &mut hasher).map(Some)
}

fn parse_log_tail(file: &mut File, mut offset: u64, signature: LogSignature, hasher: &mut Sha256) -> Result<ParsedTail, RefreshError> {
  file.seek(SeekFrom::Start(offset)).map_err(RefreshError::Io)?;
  let mut commits = Vec::new();
  while offset < signature.length {
    let remaining = signature.length.checked_sub(offset).ok_or(RefreshError::Integrity)?;
    if remaining < FRAME_HEADER_BYTES as u64 {
      return Ok(parsed_tail(commits, offset, true, signature, hasher.clone()));
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
      return Ok(parsed_tail(commits, offset, true, signature, hasher.clone()));
    }
    let digest_start = length_start + 8;
    let expected: [u8; 32] = header[digest_start..digest_start + 32].try_into().expect("fixed frame digest bytes");
    let mut payload = vec![0_u8; payload_length];
    file.read_exact(&mut payload).map_err(RefreshError::Io)?;
    if Sha256::digest(&payload).as_slice() != expected {
      return Err(RefreshError::Integrity);
    }
    let commit = serde_json::from_slice(&payload).map_err(|_| RefreshError::Integrity)?;
    hasher.update(header);
    hasher.update(&payload);
    commits.push(commit);
    #[cfg(test)]
    tests::record_parsed_commit();
    offset = offset.checked_add(frame_length).ok_or(RefreshError::Integrity)?;
  }
  Ok(parsed_tail(commits, offset, false, signature, hasher.clone()))
}

fn parsed_tail(
  commits: Vec<RunCommit>,
  verified_bytes: u64,
  has_partial_tail: bool,
  signature: LogSignature,
  verified_hasher: Sha256,
) -> ParsedTail {
  let verified_digest = verified_hasher.clone().finalize().into();
  ParsedTail {
    commits,
    verified_bytes,
    has_partial_tail,
    signature,
    verified_hasher,
    verified_digest,
  }
}

fn hash_verified_prefix(file: &mut File, length: u64, expected_digest: [u8; 32]) -> Result<Sha256, RefreshError> {
  file.seek(SeekFrom::Start(0)).map_err(RefreshError::Io)?;
  #[cfg(test)]
  tests::record_full_prefix_bytes(length);
  let mut remaining = length;
  let mut hasher = Sha256::new();
  let mut buffer = [0_u8; ARTIFACT_CHUNK_BYTES];
  while remaining != 0 {
    let count = usize::try_from(remaining.min(buffer.len() as u64)).expect("bounded digest read fits usize");
    file.read_exact(&mut buffer[..count]).map_err(RefreshError::Io)?;
    hasher.update(&buffer[..count]);
    remaining -= count as u64;
  }
  if <[u8; 32]>::from(hasher.clone().finalize()) != expected_digest {
    return Err(RefreshError::Integrity);
  }
  Ok(hasher)
}

fn verify_log_binding(lock: &RunLock) -> Result<LogIdentity, RefreshError> {
  let bound = lock.log.as_ref().ok_or(RefreshError::Integrity)?;
  let bound_identity = LogIdentity::from_file(bound).map_err(RefreshError::Io)?;
  let current = open_private_file(&lock.directory, "commits.log", true, false, false, false).map_err(|error| {
    if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) {
      RefreshError::Integrity
    } else {
      RefreshError::Io(error)
    }
  })?;
  if LogIdentity::from_file(&current).map_err(RefreshError::Io)? != bound_identity {
    return Err(RefreshError::Integrity);
  }
  Ok(bound_identity)
}

fn prove_appended_frame(file: &mut File, expected_bytes: u64, frame: &[u8]) -> Result<LogSignature, RefreshError> {
  let frame_length = u64::try_from(frame.len()).map_err(|_| RefreshError::Integrity)?;
  let expected_end = expected_bytes.checked_add(frame_length).ok_or(RefreshError::Integrity)?;
  let metadata = file.metadata().map_err(RefreshError::Io)?;
  if metadata.len() != expected_end {
    return Err(RefreshError::Integrity);
  }

  file.seek(SeekFrom::Start(expected_bytes)).map_err(RefreshError::Io)?;
  let mut buffer = [0_u8; ARTIFACT_CHUNK_BYTES];
  let mut frame_offset = 0_usize;
  while frame_offset < frame.len() {
    let count = (frame.len() - frame_offset).min(buffer.len());
    file.read_exact(&mut buffer[..count]).map_err(RefreshError::Io)?;
    if buffer[..count] != frame[frame_offset..frame_offset + count] {
      return Err(RefreshError::Integrity);
    }
    frame_offset += count;
  }
  let mut extra = [0_u8; 1];
  if file.read(&mut extra).map_err(RefreshError::Io)? != 0 {
    return Err(RefreshError::Integrity);
  }
  let signature = LogSignature::from_file(file).map_err(RefreshError::Io)?;
  if signature.length != expected_end {
    return Err(RefreshError::Integrity);
  }
  Ok(signature)
}

fn pre_append_error(error: RefreshError) -> AppendError {
  match error {
    RefreshError::Io(_) => AppendError::Unavailable,
    RefreshError::Integrity => AppendError::Integrity,
  }
}

fn post_append_error(_error: RefreshError) -> AppendError {
  AppendError::Unknown
}

fn append_commit(lock: &mut RunLock, commit: &RunCommit, index: &RunIndex) -> Result<LogStorage, AppendError> {
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
  let frame_length = u64::try_from(frame.len()).map_err(|_| AppendError::Unavailable)?;
  if lock.log.is_none() {
    if index.verified_bytes != 0 || index.observed_signature.is_some() {
      return Err(AppendError::Integrity);
    }
    lock.log = Some(open_private_file(&lock.directory, "commits.log", true, true, false, true).map_err(|error| {
      if error.kind() == io::ErrorKind::AlreadyExists {
        AppendError::Integrity
      } else {
        AppendError::Unavailable
      }
    })?);
  }
  let log_identity = verify_log_binding(lock).map_err(pre_append_error)?;
  let current_signature = LogSignature::from_file(lock.log.as_ref().ok_or(AppendError::Integrity)?).map_err(|_| AppendError::Unavailable)?;
  if current_signature.length != index.verified_bytes
    || index.observed_signature.as_ref().is_some_and(|expected| expected != &current_signature)
    || index.log_identity.as_ref().is_some_and(|expected| expected != &log_identity)
  {
    return Err(AppendError::Integrity);
  }
  // The selected root is administrator-controlled and legitimate writers honor
  // `commit.lock`. A changed signature is revalidated during refresh; the normal
  // local append can therefore carry its rolling digest forward without reading history.
  verify_log_binding(lock).map_err(pre_append_error)?;
  {
    let file = lock.log.as_mut().ok_or(AppendError::Integrity)?;
    file.seek(SeekFrom::Start(index.verified_bytes)).map_err(|_| AppendError::Unavailable)?;
    file.write_all(&frame).map_err(|_| AppendError::Unknown)?;
    file.sync_data().map_err(|_| AppendError::Unknown)?;
  }
  sync_directory(&lock.directory).map_err(|_| AppendError::Unknown)?;
  verify_log_binding(lock).map_err(post_append_error)?;
  let observed_signature =
    prove_appended_frame(lock.log.as_mut().ok_or(AppendError::Unknown)?, index.verified_bytes, &frame).map_err(post_append_error)?;
  verify_log_binding(lock).map_err(post_append_error)?;
  if observed_signature.identity != log_identity {
    return Err(AppendError::Unknown);
  }
  let verified_bytes = index.verified_bytes.checked_add(frame_length).ok_or(AppendError::Unknown)?;
  let mut verified_hasher = index.verified_hasher.clone();
  verified_hasher.update(&frame);
  let verified_digest = verified_hasher.clone().finalize().into();
  Ok(LogStorage {
    verified_bytes,
    log_identity: observed_signature.identity.clone(),
    verified_hasher,
    verified_digest,
    observed_signature,
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
  let mut file = open_private_file(directory, name, true, false, false, false).map_err(RefreshError::Io)?;
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
    match open_private_file(directory, &name, false, true, false, true) {
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

#[cfg(not(windows))]
fn sync_directory(directory: &Dir) -> io::Result<()> {
  directory.try_clone()?.into_std_file().sync_all()
}

#[cfg(windows)]
fn sync_directory(directory: &Dir) -> io::Result<()> {
  let mut options = OpenOptions::new();
  options.access_mode(GENERIC_WRITE).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).maybe_dir(true).follow(FollowSymlinks::No);
  let handle = directory.open_with(".", &options)?;
  if !handle.metadata()?.is_dir() {
    return Err(io::Error::new(io::ErrorKind::InvalidData, "directory durability handle is not a directory"));
  }
  handle.into_std().sync_all()
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

#[cfg(test)]
mod tests {
  use std::cell::Cell;

  use super::*;

  #[derive(Clone, Copy, Default, Debug)]
  pub(super) struct WorkCounters {
    full_prefix_bytes: u64,
    parsed_commits: u64,
    reducer_applies: u64,
  }

  thread_local! {
    static WORK_COUNTERS: Cell<WorkCounters> = Cell::new(WorkCounters::default());
  }

  pub(super) fn record_full_prefix_bytes(bytes: u64) {
    WORK_COUNTERS.set(WorkCounters {
      full_prefix_bytes: WORK_COUNTERS.get().full_prefix_bytes + bytes,
      ..WORK_COUNTERS.get()
    });
  }

  pub(super) fn record_parsed_commit() {
    WORK_COUNTERS.set(WorkCounters {
      parsed_commits: WORK_COUNTERS.get().parsed_commits + 1,
      ..WORK_COUNTERS.get()
    });
  }

  pub(super) fn record_reducer_apply() {
    WORK_COUNTERS.set(WorkCounters {
      reducer_applies: WORK_COUNTERS.get().reducer_applies + 1,
      ..WORK_COUNTERS.get()
    });
  }

  fn reset_work_counters() {
    WORK_COUNTERS.set(WorkCounters::default());
  }

  fn work_counters() -> WorkCounters {
    WORK_COUNTERS.get()
  }

  #[test]
  fn local_sequential_commits_and_unchanged_reads_do_only_incremental_work() {
    let root = tempfile::tempdir().unwrap();
    let store = FileRunStore::open(root.path()).unwrap();
    let run_id = RunId::new();
    for ordinal in 0..8 {
      commit_test_event(&store, run_id, ordinal);
    }

    reset_work_counters();
    for ordinal in 8..72 {
      commit_test_event(&store, run_id, ordinal);
      let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
      assert_eq!(snapshot.events().len(), ordinal as usize + 1);
    }

    let work = work_counters();
    assert_eq!(work.full_prefix_bytes, 0, "normal local writes rehashed prior history");
    assert_eq!(work.parsed_commits, 0, "normal local writes reparsed prior frames");
    assert_eq!(work.reducer_applies, 64, "normal local writes replayed reducer history");
  }

  #[test]
  fn external_tail_applies_only_new_frames_then_unchanged_read_is_constant_work() {
    let root = tempfile::tempdir().unwrap();
    let first = FileRunStore::open(root.path()).unwrap();
    let second = FileRunStore::open(root.path()).unwrap();
    let run_id = RunId::new();
    commit_test_event(&first, run_id, 0);
    commit_test_event(&second, run_id, 1);

    reset_work_counters();
    let snapshot = futures_executor::block_on(first.load_snapshot(run_id)).unwrap().unwrap();
    assert_eq!(snapshot.events().len(), 2);
    let external = work_counters();
    assert!(external.full_prefix_bytes > 0, "external change skipped the prior-prefix integrity proof");
    assert_eq!(external.parsed_commits, 1, "external refresh parsed more than its appended tail");
    assert_eq!(external.reducer_applies, 1, "external refresh replayed more than its appended tail");

    reset_work_counters();
    futures_executor::block_on(first.lookup_commit(run_id, IdempotencyKey::new())).unwrap();
    let unchanged = work_counters();
    assert_eq!(unchanged.full_prefix_bytes, 0);
    assert_eq!(unchanged.parsed_commits, 0);
    assert_eq!(unchanged.reducer_applies, 0);
  }

  #[test]
  fn authority_reader_rejects_sparse_input_above_the_fixed_bound() {
    let mut file = tempfile::tempfile().unwrap();
    file.set_len(4097).unwrap();
    let error = read_authority_bytes(&mut file).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }

  #[test]
  fn post_write_proof_rejects_wrong_end_length_as_unknown() {
    let prefix = b"verified prefix";
    let frame = b"exact frame";
    let mut file = proof_file(&[prefix.as_slice(), frame.as_slice(), b"extra"].concat());

    let error = prove_appended_frame(&mut file, prefix.len() as u64, frame).err().unwrap();
    assert!(matches!(post_append_error(error), AppendError::Unknown));
  }

  #[test]
  fn suspicious_refresh_rejects_changed_old_prefix_as_integrity() {
    let prefix = b"verified prefix";
    let frame = b"exact frame";
    let mut changed = prefix.to_vec();
    changed[0] ^= 0xff;
    let mut file = proof_file(&[changed.as_slice(), frame.as_slice()].concat());

    let error = hash_verified_prefix(&mut file, prefix.len() as u64, Sha256::digest(prefix).into()).err().unwrap();
    assert!(matches!(error, RefreshError::Integrity));
  }

  #[test]
  fn post_write_proof_rejects_changed_frame_as_unknown() {
    let prefix = b"verified prefix";
    let frame = b"exact frame";
    let mut changed = frame.to_vec();
    changed[0] ^= 0xff;
    let mut file = proof_file(&[prefix.as_slice(), changed.as_slice()].concat());

    let error = prove_appended_frame(&mut file, prefix.len() as u64, frame).err().unwrap();
    assert!(matches!(post_append_error(error), AppendError::Unknown));
  }

  fn proof_file(bytes: &[u8]) -> File {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.sync_data().unwrap();
    file
  }

  fn commit_test_event(store: &FileRunStore, run_id: RunId, ordinal: u64) {
    let schema = crate::EventSchema::new(crate::EventName::parse("auv.test.file.work").unwrap(), 1).unwrap();
    let payload = crate::JsonPayload::encode(&serde_json::json!({ "ordinal": ordinal })).unwrap();
    let event = crate::EventOccurred::new(crate::EventId::new(), None, Timestamp::new(1, 0).unwrap(), schema, payload);
    let request = RunCommitRequest::new(store.authority_id(), run_id, IdempotencyKey::new(), vec![RunMutation::EmitEvent(event)]).unwrap();
    futures_executor::block_on(store.commit(request)).unwrap();
  }
}
