#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactWriteError, Attributes, ByteLength, CommitError, ContentType, EventId, EventName, EventOccurred,
  EventSchema, FileRunStore, IdempotencyKey, JsonPayload, PageLimit, ReadError, RunCommitRequest, RunId, RunMutation, RunRevision, RunStore,
  Sha256Digest, StoreArtifactRequest, Timestamp,
};
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
  let schema = EventSchema::new(EventName::parse("auv.test.file").unwrap(), 1).unwrap();
  let payload = JsonPayload::encode(&serde_json::json!({ "value": value })).unwrap();
  let event = EventOccurred::new(EventId::new(), None, Timestamp::new(1, 0).unwrap(), schema, payload);
  RunCommitRequest::new(store.authority_id(), run_id, key, vec![RunMutation::EmitEvent(event)]).unwrap()
}

fn artifact_request(
  store: &FileRunStore,
  run_id: RunId,
  artifact_id: ArtifactId,
  key: IdempotencyKey,
  bytes: &[u8],
) -> StoreArtifactRequest {
  StoreArtifactRequest::new(
    store.authority_id(),
    run_id,
    key,
    artifact_id,
    None,
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
