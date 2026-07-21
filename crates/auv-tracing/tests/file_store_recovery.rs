#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactWriteError, Attributes, ByteLength, CommitError, ContentType, EventId, EventName, EventOccurred,
  EventSchema, FileRunStore, IdempotencyKey, JsonPayload, PageLimit, ReadError, RunCommitRequest, RunId, RunMutation, RunRevision, RunStore,
  Sha256Digest, SpanId, StoreArtifactRequest, Timestamp,
};
use futures_io::AsyncRead;
use futures_util::StreamExt;
use futures_util::io::Cursor;
use sha2::{Digest, Sha256};

#[test]
fn truncated_final_frame_is_ignored_then_repaired_before_append_but_prior_corruption_fails() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  commit_event(&store, run_id, "first");
  commit_event(&store, run_id, "second");
  let log = commit_log(root.path(), run_id);
  let complete_length = fs::metadata(&log).unwrap().len();
  OpenOptions::new().write(true).open(&log).unwrap().set_len(complete_length - 3).unwrap();

  let reopened = FileRunStore::open(root.path()).unwrap();
  let recovered = futures_executor::block_on(reopened.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(recovered.through_revision().get(), 1);
  assert_eq!(recovered.events().len(), 1);
  assert_eq!(fs::metadata(&log).unwrap().len(), complete_length - 3, "a point read repaired the partial tail");

  let repaired = commit_event(&reopened, run_id, "replacement second");
  assert_eq!(repaired.revision().get(), 2);
  let snapshot = futures_executor::block_on(reopened.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.events().len(), 2);

  let mut file = OpenOptions::new().write(true).open(&log).unwrap();
  file.seek(SeekFrom::Start(0)).unwrap();
  file.write_all(b"X").unwrap();
  file.sync_data().unwrap();
  assert!(matches!(futures_executor::block_on(reopened.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn unsupported_complete_frame_version_is_integrity_failure() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  commit_event(&store, run_id, "versioned");
  let log = commit_log(root.path(), run_id);
  let mut file = OpenOptions::new().write(true).open(log).unwrap();
  file.seek(SeekFrom::Start(8)).unwrap();
  file.write_all(&2_u16.to_be_bytes()).unwrap();
  file.sync_data().unwrap();

  assert!(matches!(futures_executor::block_on(store.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn two_open_store_instances_refresh_indexes_under_the_run_lock() {
  let root = tempfile::tempdir().unwrap();
  let first = FileRunStore::open(root.path()).unwrap();
  let second = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let one = commit_event(&first, run_id, "one");
  let two = commit_event(&second, run_id, "two");
  assert_eq!(one.revision().get(), 1);
  assert_eq!(two.revision().get(), 2);
  let snapshot = futures_executor::block_on(first.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.events().len(), 2);
}

#[test]
fn already_open_point_reads_refresh_from_the_verified_log() {
  let root = tempfile::tempdir().unwrap();
  let first = FileRunStore::open(root.path()).unwrap();
  let second = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let key = IdempotencyKey::new();
  let request = event_request(&second, run_id, key, "external");
  let committed = futures_executor::block_on(second.commit(request)).unwrap();

  assert_eq!(futures_executor::block_on(first.lookup_commit(run_id, key)).unwrap(), Some(committed.clone()));
  assert_eq!(futures_executor::block_on(first.load_snapshot(run_id)).unwrap().unwrap().through_revision().get(), 1);
  let page = futures_executor::block_on(first.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits(), &[committed]);
}

#[test]
fn slow_artifact_body_does_not_block_an_ordinary_same_run_commit() {
  let root = tempfile::tempdir().unwrap();
  let store = Arc::new(FileRunStore::open(root.path()).unwrap());
  let run_id = RunId::new();
  let bytes = b"blocked artifact body".to_vec();
  let gate = Arc::new(BodyGate::default());
  let artifact_request = artifact_request(&store, run_id, ArtifactId::new(), IdempotencyKey::new(), &bytes);
  let artifact_store = Arc::clone(&store);
  let artifact_gate = Arc::clone(&gate);
  let artifact = thread::spawn(move || {
    let body = GatedBody {
      bytes: Cursor::new(bytes),
      gate: artifact_gate,
      announced: false,
    };
    futures_executor::block_on(artifact_store.write_artifact(artifact_request, Box::pin(body)))
  });
  assert!(gate.wait_until_polled(Duration::from_secs(5)), "artifact body was never polled");

  let (sent, received) = mpsc::sync_channel(1);
  let commit_store = Arc::clone(&store);
  let commit = thread::spawn(move || {
    let result = futures_executor::block_on(commit_store.commit(event_request(&commit_store, run_id, IdempotencyKey::new(), "ordinary")));
    sent.send(result).unwrap();
  });
  let ordinary = received.recv_timeout(Duration::from_secs(2));

  gate.release();
  let artifact_result = artifact.join().unwrap();
  commit.join().unwrap();
  let ordinary = ordinary.expect("ordinary commit remained blocked on the artifact stream").unwrap();
  assert_eq!(ordinary.revision().get(), 1);
  assert_eq!(artifact_result.unwrap().revision().get(), 2);
}

#[test]
fn open_durably_creates_each_missing_root_component() {
  let parent = tempfile::tempdir().unwrap();
  let root = parent.path().join("one").join("two").join("store");
  let authority_id = FileRunStore::open(&root).unwrap().authority_id();

  assert!(root.is_dir());
  assert_eq!(FileRunStore::open(&root).unwrap().authority_id(), authority_id);
}

#[test]
fn oversized_sparse_frame_is_integrity_without_trusting_declared_length() {
  const FRAME_HEADER_BYTES: u64 = 8 + 2 + 8 + 32;
  const OVERSIZED_PAYLOAD: u64 = 64 * 1024 * 1024;

  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let run_directory = root.path().join("runs").join(run_id.to_string());
  fs::create_dir(&run_directory).unwrap();
  let mut log = OpenOptions::new().write(true).create_new(true).open(run_directory.join("commits.log")).unwrap();
  log.write_all(b"AUVRCMT\0").unwrap();
  log.write_all(&1_u16.to_be_bytes()).unwrap();
  log.write_all(&OVERSIZED_PAYLOAD.to_be_bytes()).unwrap();
  log.write_all(&[0_u8; 32]).unwrap();
  log.set_len(FRAME_HEADER_BYTES + OVERSIZED_PAYLOAD).unwrap();
  log.sync_data().unwrap();

  assert!(matches!(futures_executor::block_on(store.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn stale_cached_writer_consumes_only_the_new_verified_tail() {
  let root = tempfile::tempdir().unwrap();
  let first = FileRunStore::open(root.path()).unwrap();
  let second = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&first, run_id, "one").revision().get(), 1);
  assert_eq!(futures_executor::block_on(first.load_snapshot(run_id)).unwrap().unwrap().events().len(), 1);
  assert_eq!(commit_event(&second, run_id, "two").revision().get(), 2);

  assert_eq!(commit_event(&first, run_id, "three").revision().get(), 3);
  assert_eq!(futures_executor::block_on(second.load_snapshot(run_id)).unwrap().unwrap().events().len(), 3);
}

#[test]
fn rejected_duplicate_event_does_not_grow_log_or_poison_the_next_revision() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let event_id = EventId::new();
  let first =
    futures_executor::block_on(store.commit(event_request_with_id(&store, run_id, IdempotencyKey::new(), event_id, "first"))).unwrap();
  assert_eq!(first.revision().get(), 1);
  let log = commit_log(root.path(), run_id);
  let valid_length = fs::metadata(&log).unwrap().len();

  let duplicate =
    futures_executor::block_on(store.commit(event_request_with_id(&store, run_id, IdempotencyKey::new(), event_id, "duplicate")));
  assert!(matches!(duplicate, Err(CommitError::Rejected(_))));
  assert_eq!(fs::metadata(&log).unwrap().len(), valid_length);

  assert_eq!(commit_event(&store, run_id, "second").revision().get(), 2);
}

#[test]
fn rejected_unknown_span_artifact_publishes_neither_frame_nor_blob() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&store, run_id, "first").revision().get(), 1);
  let log = commit_log(root.path(), run_id);
  let valid_length = fs::metadata(&log).unwrap().len();
  let bytes = b"must not be published".to_vec();
  let sha256 = digest(&bytes);
  let request = artifact_request_with_span(&store, run_id, ArtifactId::new(), IdempotencyKey::new(), Some(SpanId::new()), &bytes);

  let rejected = futures_executor::block_on(store.write_artifact(request, Box::pin(Cursor::new(bytes))));
  assert!(matches!(rejected, Err(ArtifactWriteError::Rejected(_))));
  assert_eq!(fs::metadata(&log).unwrap().len(), valid_length);
  assert!(!blob_path(root.path(), sha256).exists());
  assert_eq!(commit_event(&store, run_id, "second").revision().get(), 2);
}

#[test]
fn cached_prefix_mutation_is_integrity_even_when_a_valid_tail_was_appended() {
  let root = tempfile::tempdir().unwrap();
  let first = FileRunStore::open(root.path()).unwrap();
  let second = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&first, run_id, "one").revision().get(), 1);
  assert_eq!(commit_event(&second, run_id, "two").revision().get(), 2);
  let log = commit_log(root.path(), run_id);
  let mut file = OpenOptions::new().write(true).open(log).unwrap();
  file.write_all(b"X").unwrap();
  file.sync_data().unwrap();

  assert!(matches!(futures_executor::block_on(first.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn cached_log_shrink_is_integrity_instead_of_accepted_history_rollback() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&store, run_id, "one").revision().get(), 1);
  let log = commit_log(root.path(), run_id);
  let first_length = fs::metadata(&log).unwrap().len();
  assert_eq!(commit_event(&store, run_id, "two").revision().get(), 2);
  OpenOptions::new().write(true).open(&log).unwrap().set_len(first_length).unwrap();

  assert!(matches!(futures_executor::block_on(store.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn cached_log_inode_replacement_is_integrity_even_with_identical_bytes() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&store, run_id, "one").revision().get(), 1);
  let log = commit_log(root.path(), run_id);
  replace_file(&log);

  assert!(matches!(futures_executor::block_on(store.load_snapshot(run_id)), Err(ReadError::Integrity(_))));
}

#[test]
fn replaced_partial_tail_is_not_repaired_or_appended_through_a_new_inode() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  assert_eq!(commit_event(&store, run_id, "one").revision().get(), 1);
  let log = commit_log(root.path(), run_id);
  let mut file = OpenOptions::new().append(true).open(&log).unwrap();
  file.write_all(b"AUV").unwrap();
  file.sync_data().unwrap();
  drop(file);
  assert_eq!(futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap().through_revision().get(), 1);
  replace_file(&log);
  let partial_length = fs::metadata(&log).unwrap().len();

  assert!(matches!(
    futures_executor::block_on(store.commit(event_request(&store, run_id, IdempotencyKey::new(), "two"))),
    Err(CommitError::Unavailable(_))
  ));
  assert_eq!(fs::metadata(&log).unwrap().len(), partial_length);
}

#[test]
fn artifact_revalidates_the_bound_log_after_streaming() {
  let root = tempfile::tempdir().unwrap();
  let store = Arc::new(FileRunStore::open(root.path()).unwrap());
  let run_id = RunId::new();
  assert_eq!(commit_event(&store, run_id, "one").revision().get(), 1);
  let bytes = b"replacement race body".to_vec();
  let sha256 = digest(&bytes);
  let gate = Arc::new(BodyGate::default());
  let request = artifact_request(&store, run_id, ArtifactId::new(), IdempotencyKey::new(), &bytes);
  let artifact_store = Arc::clone(&store);
  let artifact_gate = Arc::clone(&gate);
  let artifact = thread::spawn(move || {
    let body = GatedBody {
      bytes: Cursor::new(bytes),
      gate: artifact_gate,
      announced: false,
    };
    futures_executor::block_on(artifact_store.write_artifact(request, Box::pin(body)))
  });
  assert!(gate.wait_until_polled(Duration::from_secs(5)), "artifact body was never polled");
  replace_file(&commit_log(root.path(), run_id));
  gate.release();

  assert!(matches!(artifact.join().unwrap(), Err(ArtifactWriteError::Integrity(_))));
  assert!(!blob_path(root.path(), sha256).exists());
}

#[test]
fn concurrent_open_creates_one_stable_nested_root() {
  let parent = tempfile::tempdir().unwrap();
  let root = parent.path().join("raced").join("nested").join("store");
  let barrier = Arc::new(std::sync::Barrier::new(3));
  let open = |root: PathBuf, barrier: Arc<std::sync::Barrier>| {
    thread::spawn(move || {
      barrier.wait();
      FileRunStore::open(root).unwrap().authority_id()
    })
  };
  let first = open(root.clone(), Arc::clone(&barrier));
  let second = open(root.clone(), Arc::clone(&barrier));
  barrier.wait();

  assert_eq!(first.join().unwrap(), second.join().unwrap());
  assert!(root.is_dir());
}

#[cfg(unix)]
#[test]
fn artifact_publication_rejects_a_symlink_escape_below_root() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let bytes = b"must remain inside the store".to_vec();
  let digest = digest(&bytes);
  let prefix = &digest.to_string()[..2];
  let prefix_path = root.path().join("blobs").join("sha256").join(prefix);
  symlink(outside.path(), &prefix_path).unwrap();
  let request = artifact_request(&store, run_id, artifact_id, IdempotencyKey::new(), &bytes);

  assert!(matches!(
    futures_executor::block_on(store.write_artifact(request, Box::pin(Cursor::new(bytes)))),
    Err(ArtifactWriteError::Rejected(_))
  ));
  assert!(!outside.path().join(digest.to_string()).exists());
  assert_eq!(futures_executor::block_on(store.load_snapshot(run_id)).unwrap(), None);
}

#[cfg(unix)]
#[test]
fn commit_rejects_a_symlinked_run_directory() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  symlink(outside.path(), root.path().join("runs").join(run_id.to_string())).unwrap();

  assert!(matches!(
    futures_executor::block_on(store.commit(event_request(&store, run_id, IdempotencyKey::new(), "escape"))),
    Err(CommitError::Rejected(_))
  ));
  assert!(!outside.path().join("commits.log").exists());
}

#[cfg(unix)]
#[test]
fn open_rejects_a_symlinked_private_directory() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  symlink(outside.path(), root.path().join("tmp")).unwrap();
  assert!(FileRunStore::open(root.path()).is_err());
}

#[cfg(unix)]
#[test]
fn already_open_store_rejects_a_replaced_private_parent() {
  use std::os::unix::fs::symlink;

  let root = tempfile::tempdir().unwrap();
  let outside = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  fs::rename(root.path().join("runs"), root.path().join("detached-runs")).unwrap();
  symlink(outside.path(), root.path().join("runs")).unwrap();
  let run_id = RunId::new();

  assert!(matches!(
    futures_executor::block_on(store.commit(event_request(&store, run_id, IdempotencyKey::new(), "escape"))),
    Err(CommitError::Rejected(_))
  ));
  assert!(!outside.path().join(run_id.to_string()).exists());
}

#[test]
fn artifact_reader_reports_digest_corruption() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let bytes = b"integrity protected body".to_vec();
  let request = artifact_request(&store, run_id, artifact_id, IdempotencyKey::new(), &bytes);
  let commit = futures_executor::block_on(store.write_artifact(request, Box::pin(Cursor::new(bytes.clone())))).unwrap();
  let metadata = commit
    .facts()
    .iter()
    .find_map(|fact| match fact {
      auv_tracing::RunFact::ArtifactPublished(publication) => Some(publication.metadata()),
      _ => None,
    })
    .unwrap();
  let blob = blob_path(root.path(), metadata.sha256());
  let mut corrupted = bytes;
  corrupted[0] ^= 0xff;
  fs::write(&blob, corrupted).unwrap();
  let mut reader = futures_executor::block_on(store.open_artifact(metadata.uri().clone())).unwrap();
  let error = futures_executor::block_on(async {
    loop {
      match reader.next().await {
        Some(Ok(_)) => {}
        Some(Err(error)) => break error,
        None => panic!("corrupted artifact stream ended without an integrity error"),
      }
    }
  });
  assert!(matches!(error, auv_tracing::ArtifactReadError::Integrity(_)));
}

fn commit_event(store: &FileRunStore, run_id: RunId, value: &str) -> auv_tracing::RunCommit {
  futures_executor::block_on(store.commit(event_request(store, run_id, IdempotencyKey::new(), value))).unwrap()
}

fn event_request(store: &FileRunStore, run_id: RunId, key: IdempotencyKey, value: &str) -> RunCommitRequest {
  event_request_with_id(store, run_id, key, EventId::new(), value)
}

fn event_request_with_id(store: &FileRunStore, run_id: RunId, key: IdempotencyKey, event_id: EventId, value: &str) -> RunCommitRequest {
  let schema = EventSchema::new(EventName::parse("auv.test.file").unwrap(), 1).unwrap();
  let payload = JsonPayload::encode(&serde_json::json!({ "value": value })).unwrap();
  let event = EventOccurred::new(event_id, None, Timestamp::new(1, 0).unwrap(), schema, payload);
  RunCommitRequest::new(store.authority_id(), run_id, key, vec![RunMutation::EmitEvent(event)]).unwrap()
}

fn artifact_request(
  store: &FileRunStore,
  run_id: RunId,
  artifact_id: ArtifactId,
  key: IdempotencyKey,
  bytes: &[u8],
) -> StoreArtifactRequest {
  artifact_request_with_span(store, run_id, artifact_id, key, None, bytes)
}

fn artifact_request_with_span(
  store: &FileRunStore,
  run_id: RunId,
  artifact_id: ArtifactId,
  key: IdempotencyKey,
  span_id: Option<SpanId>,
  bytes: &[u8],
) -> StoreArtifactRequest {
  StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    key,
    artifact_id,
    span_id,
    ArtifactPurpose::parse("auv.test.file").unwrap(),
    ContentType::parse("application/octet-stream").unwrap(),
    ByteLength::new(bytes.len() as u64).unwrap(),
    digest(bytes),
    Attributes::empty(),
  )
}

fn digest(bytes: &[u8]) -> Sha256Digest {
  Sha256Digest::new(Sha256::digest(bytes).into())
}

fn revision(value: u64) -> RunRevision {
  RunRevision::new(value).unwrap()
}

fn commit_log(root: &Path, run_id: RunId) -> PathBuf {
  root.join("runs").join(run_id.to_string()).join("commits.log")
}

fn blob_path(root: &Path, digest: Sha256Digest) -> PathBuf {
  let digest = digest.to_string();
  root.join("blobs").join("sha256").join(&digest[..2]).join(digest)
}

fn replace_file(path: &Path) {
  let replacement = path.with_extension("replacement");
  fs::write(&replacement, fs::read(path).unwrap()).unwrap();
  OpenOptions::new().read(true).open(&replacement).unwrap().sync_data().unwrap();
  fs::remove_file(path).unwrap();
  fs::rename(replacement, path).unwrap();
}

#[derive(Default)]
struct BodyGate {
  released: AtomicBool,
  state: Mutex<BodyGateState>,
  started: Condvar,
}

#[derive(Default)]
struct BodyGateState {
  polled: bool,
  waker: Option<Waker>,
}

impl BodyGate {
  fn wait_until_polled(&self, timeout: Duration) -> bool {
    let state = self.state.lock().unwrap();
    self.started.wait_timeout_while(state, timeout, |state| !state.polled).unwrap().0.polled
  }

  fn release(&self) {
    self.released.store(true, Ordering::Release);
    if let Some(waker) = self.state.lock().unwrap().waker.take() {
      waker.wake();
    }
  }
}

struct GatedBody {
  bytes: Cursor<Vec<u8>>,
  gate: Arc<BodyGate>,
  announced: bool,
}

impl AsyncRead for GatedBody {
  fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    if !self.announced {
      self.announced = true;
      let mut state = self.gate.state.lock().unwrap();
      state.polled = true;
      self.gate.started.notify_all();
    }
    let mut state = self.gate.state.lock().unwrap();
    if !self.gate.released.load(Ordering::Acquire) {
      state.waker = Some(cx.waker().clone());
      return Poll::Pending;
    }
    drop(state);
    Pin::new(&mut self.bytes).poll_read(cx, buffer)
  }
}
