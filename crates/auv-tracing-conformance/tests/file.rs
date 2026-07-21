#![forbid(unsafe_code)]

use std::fs;
use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::pin::Pin;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

use auv_tracing::{ArtifactId, AuthorityId, EventId, FileRunStore, IdempotencyKey, PageLimit, RunFact, RunId, RunRevision, RunStore};
use auv_tracing_conformance::{artifact_request, assert_store_contract};
use futures_timer::Delay;
use futures_util::future::{Either, select};
use futures_util::task::noop_waker;
use futures_util::{StreamExt, pin_mut};
use serde::Deserialize;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn file_store_satisfies_authority_contract() {
  futures_executor::block_on(assert_store_contract(|| {
    let root = tempfile::tempdir().unwrap().keep();
    Arc::new(FileRunStore::open(root).unwrap())
  }));
}

#[test]
fn concurrent_first_open_chooses_one_authority() {
  let root = tempfile::tempdir().unwrap();
  let sync = tempfile::tempdir().unwrap();
  let ready_one = sync.path().join("ready-one");
  let ready_two = sync.path().join("ready-two");
  let go = sync.path().join("go");
  let mut first = spawn_authority(root.path(), &ready_one, &go);
  let mut second = spawn_authority(root.path(), &ready_two, &go);
  wait_for_ready(&[&ready_one, &ready_two], &mut [&mut first, &mut second]);
  release(&go);

  let first = first.finish();
  let second = second.finish();
  let ChildResult::Authority {
    authority_id: first,
  } = first
  else {
    panic!("authority child returned {first:?}");
  };
  let ChildResult::Authority {
    authority_id: second,
  } = second
  else {
    panic!("authority child returned {second:?}");
  };
  assert_eq!(first, second);
  assert!(!first.as_uuid().is_nil());

  let authority: AuthorityFile = serde_json::from_slice(&fs::read(root.path().join("authority.json")).unwrap()).unwrap();
  assert_eq!(authority.version, 1);
  assert_eq!(authority.authority_id, first);
}

#[test]
fn concurrent_process_commits_allocate_contiguous_revisions() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let ready_one = sync.path().join("ready-one");
  let ready_two = sync.path().join("ready-two");
  let go = sync.path().join("go");
  let mut first = spawn_commit(root.path(), run_id, EventId::new(), IdempotencyKey::new(), "one", &ready_one, &go);
  let mut second = spawn_commit(root.path(), run_id, EventId::new(), IdempotencyKey::new(), "two", &ready_two, &go);
  wait_for_ready(&[&ready_one, &ready_two], &mut [&mut first, &mut second]);
  release(&go);

  let mut revisions = [
    commit_revision(first.finish()),
    commit_revision(second.finish()),
  ];
  revisions.sort_unstable();
  assert_eq!(revisions, [1, 2]);
  let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.through_revision().get(), 2);
  assert_eq!(snapshot.events().len(), 2);
}

#[test]
fn concurrent_equal_idempotency_replay_appends_once() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let event_id = EventId::new();
  let key = IdempotencyKey::new();
  let ready_one = sync.path().join("ready-one");
  let ready_two = sync.path().join("ready-two");
  let go = sync.path().join("go");
  let mut first = spawn_commit(root.path(), run_id, event_id, key, "same", &ready_one, &go);
  let mut second = spawn_commit(root.path(), run_id, event_id, key, "same", &ready_two, &go);
  wait_for_ready(&[&ready_one, &ready_two], &mut [&mut first, &mut second]);
  release(&go);

  let results = [first.finish(), second.finish()];
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::CommitAppended { .. })).count(), 1);
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::CommitReplayed { .. })).count(), 1);
  assert_eq!(results.map(commit_revision), [1, 1]);
  let page = futures_executor::block_on(store.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits().len(), 1);
}

#[test]
fn concurrent_equal_artifact_writers_report_one_append_and_one_replay() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let body = sync.path().join("body");
  fs::write(&body, b"equal concurrent artifact").unwrap();
  let ready_one = sync.path().join("ready-one");
  let ready_two = sync.path().join("ready-two");
  let go = sync.path().join("go");
  let mut first = spawn_artifact(root.path(), run_id, artifact_id, key, &body, &ready_one, &go);
  let mut second = spawn_artifact(root.path(), run_id, artifact_id, key, &body, &ready_two, &go);
  wait_for_ready(&[&ready_one, &ready_two], &mut [&mut first, &mut second]);
  release(&go);

  let results = [first.finish(), second.finish()];
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::ArtifactAppended { .. })).count(), 1);
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::ArtifactReplayed { .. })).count(), 1);
  assert_eq!(results.map(artifact_revision), [1, 1]);
  let page = futures_executor::block_on(store.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits().len(), 1);
  assert_eq!(blob_count(root.path()), 1);
}

// ROOT CAUSE:
//
// If an equal artifact commit became visible while another writer staged a
// body that later failed, the loser returned the body error without refreshing
// the run index. The fix rechecks under the run lock before settling that error.
#[test]
fn concurrent_equal_artifact_staging_failure_rechecks_and_replays() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let body = sync.path().join("body");
  let bytes = b"winner artifact body";
  fs::write(&body, bytes).unwrap();
  let ready = sync.path().join("ready");
  let go = sync.path().join("go");
  let body_polled = sync.path().join("body-polled");
  let fail = sync.path().join("fail");
  let mut loser = spawn_failing_artifact(root.path(), run_id, artifact_id, key, &body, &ready, &go, &body_polled, &fail);
  wait_for_ready(&[&ready], &mut [&mut loser]);
  release(&go);
  wait_for_ready(&[&body_polled], &mut [&mut loser]);

  // The request was not committed at the loser's initial check, so its body is
  // polled outside the run lock before the winner establishes the replay.
  let winner = futures_executor::block_on(store.write_artifact(
    artifact_request(store.authority_id(), run_id, key, artifact_id, bytes),
    Box::pin(futures_util::io::Cursor::new(bytes)),
  ))
  .unwrap();
  assert!(matches!(winner, auv_tracing::CommitResult::Appended(_)));
  release(&fail);

  assert!(matches!(loser.finish(), ChildResult::ArtifactReplayed { revision: 1 }));
  let page = futures_executor::block_on(store.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits().len(), 1);
}

#[test]
fn concurrent_artifact_id_conflict_keeps_one_blob_and_one_fact() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let first_body = sync.path().join("first.body");
  let second_body = sync.path().join("second.body");
  fs::write(&first_body, b"first artifact body").unwrap();
  fs::write(&second_body, b"different artifact body").unwrap();
  let ready_one = sync.path().join("ready-one");
  let ready_two = sync.path().join("ready-two");
  let go = sync.path().join("go");
  let mut first = spawn_artifact(root.path(), run_id, artifact_id, IdempotencyKey::new(), &first_body, &ready_one, &go);
  let mut second = spawn_artifact(root.path(), run_id, artifact_id, IdempotencyKey::new(), &second_body, &ready_two, &go);
  wait_for_ready(&[&ready_one, &ready_two], &mut [&mut first, &mut second]);
  release(&go);

  let results = [first.finish(), second.finish()];
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::ArtifactAppended { .. })).count(), 1);
  assert_eq!(results.iter().filter(|result| matches!(result, ChildResult::ArtifactConflict)).count(), 1);
  let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.artifacts().len(), 1);
  let publication = snapshot.artifacts().values().next().unwrap();
  let bytes = collect_artifact(futures_executor::block_on(store.open_artifact(publication.metadata().uri().clone())).unwrap());
  assert!(bytes == b"first artifact body" || bytes == b"different artifact body");
  assert_eq!(publication.metadata().byte_length().get(), bytes.len() as u64);
  assert_eq!(blob_count(root.path()), 1);
  let page = futures_executor::block_on(store.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits().len(), 1);
  assert!(matches!(page.commits()[0].facts(), [RunFact::ArtifactPublished(_)]));
}

#[test]
fn point_reads_refresh_after_another_process_writes() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let key = IdempotencyKey::new();
  let body = sync.path().join("body");
  let bytes = b"external process artifact";
  fs::write(&body, bytes).unwrap();
  let ready = sync.path().join("ready");
  let go = sync.path().join("go");
  let mut child = spawn_artifact(root.path(), run_id, artifact_id, key, &body, &ready, &go);
  wait_for_ready(&[&ready], &mut [&mut child]);
  release(&go);
  assert!(matches!(child.finish(), ChildResult::ArtifactAppended { revision: 1 }));

  let lookup = futures_executor::block_on(store.lookup_commit(run_id, key)).unwrap().unwrap();
  assert_eq!(lookup.revision().get(), 1);
  let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).unwrap().unwrap();
  assert_eq!(snapshot.through_revision().get(), 1);
  assert_eq!(snapshot.artifacts().len(), 1);
  let page = futures_executor::block_on(store.commits_after(run_id, revision(0), PageLimit::new(10).unwrap())).unwrap();
  assert_eq!(page.commits(), &[lookup]);
  let reader = futures_executor::block_on(store.open_artifact(auv_tracing::ArtifactUri::from_ids(run_id, artifact_id))).unwrap();
  assert_eq!(collect_artifact(reader), bytes);
}

#[test]
fn subscription_observes_another_instance_commit() {
  let root = tempfile::tempdir().unwrap();
  let store = FileRunStore::open(root.path()).unwrap();
  let sync = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let ready = sync.path().join("ready");
  let go = sync.path().join("go");
  let mut child = spawn_commit(root.path(), run_id, EventId::new(), IdempotencyKey::new(), "external", &ready, &go);
  wait_for_ready(&[&ready], &mut [&mut child]);

  let mut subscription = futures_executor::block_on(store.subscribe(run_id, revision(0))).unwrap();
  let next = subscription.next();
  pin_mut!(next);
  assert_pending(next.as_mut());
  let go_for_thread = go.clone();
  let release_thread = thread::spawn(move || {
    thread::sleep(Duration::from_millis(100));
    release(&go_for_thread);
  });
  let wait_started = Instant::now();
  let received = futures_executor::block_on(async {
    let timeout = Delay::new(PROCESS_TIMEOUT);
    pin_mut!(timeout);
    match select(next, timeout).await {
      Either::Left((Some(Ok(commit)), _)) => commit,
      Either::Left((Some(Err(error)), _)) => panic!("subscription returned an error: {error:?}"),
      Either::Left((None, _)) => panic!("subscription ended before the external commit"),
      Either::Right(_) => panic!("subscription did not observe the external commit within five seconds"),
    }
  });
  assert!(wait_started.elapsed() < Duration::from_secs(2), "subscription was not woken by the 25 ms file watcher");
  release_thread.join().unwrap();
  assert_eq!(received.revision().get(), 1);
  assert_eq!(commit_revision(child.finish()), 1);
}

#[derive(Debug, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum ChildResult {
  Authority { authority_id: AuthorityId },
  CommitAppended { revision: u64 },
  CommitReplayed { revision: u64 },
  ArtifactAppended { revision: u64 },
  ArtifactReplayed { revision: u64 },
  ArtifactConflict,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityFile {
  version: u32,
  authority_id: AuthorityId,
}

struct ChildGuard {
  child: Child,
  complete: bool,
}

impl ChildGuard {
  fn spawn(command: &mut Command) -> Self {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    Self {
      child: command.spawn().expect("failed to spawn file-store child"),
      complete: false,
    }
  }

  fn finish(mut self) -> ChildResult {
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    let status = loop {
      if let Some(status) = self.child.try_wait().expect("failed to query file-store child") {
        break status;
      }
      if Instant::now() >= deadline {
        self.terminate();
        panic!("file-store child did not exit within five seconds");
      }
      thread::sleep(Duration::from_millis(10));
    };
    self.complete = true;
    let mut stdout = String::new();
    self.child.stdout.take().unwrap().read_to_string(&mut stdout).unwrap();
    let mut stderr = String::new();
    self.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert!(status.success(), "file-store child failed: {stderr}");
    serde_json::from_str(stdout.trim()).unwrap_or_else(|error| panic!("invalid child JSON `{stdout}`: {error}"))
  }

  fn terminate(&mut self) {
    if self.complete {
      return;
    }
    let _ = self.child.kill();
    let _ = self.child.wait();
    self.complete = true;
  }
}

impl Drop for ChildGuard {
  fn drop(&mut self) {
    self.terminate();
  }
}

fn spawn_authority(root: &Path, ready: &Path, go: &Path) -> ChildGuard {
  let mut command = child_command();
  command.arg("authority").arg(root).arg(ready).arg(go);
  ChildGuard::spawn(&mut command)
}

#[allow(clippy::too_many_arguments)]
fn spawn_commit(root: &Path, run_id: RunId, event_id: EventId, key: IdempotencyKey, value: &str, ready: &Path, go: &Path) -> ChildGuard {
  let mut command = child_command();
  command.arg("commit-event").arg(root).arg(run_id.to_string()).arg(event_id.to_string()).arg(key.to_string()).arg(value).arg(ready).arg(go);
  ChildGuard::spawn(&mut command)
}

#[allow(clippy::too_many_arguments)]
fn spawn_artifact(
  root: &Path,
  run_id: RunId,
  artifact_id: ArtifactId,
  key: IdempotencyKey,
  body: &Path,
  ready: &Path,
  go: &Path,
) -> ChildGuard {
  let mut command = child_command();
  command
    .arg("write-artifact")
    .arg(root)
    .arg(run_id.to_string())
    .arg("none")
    .arg(artifact_id.to_string())
    .arg(key.to_string())
    .arg(body)
    .arg(ready)
    .arg(go);
  ChildGuard::spawn(&mut command)
}

#[allow(clippy::too_many_arguments)]
fn spawn_failing_artifact(
  root: &Path,
  run_id: RunId,
  artifact_id: ArtifactId,
  key: IdempotencyKey,
  body: &Path,
  ready: &Path,
  go: &Path,
  body_polled: &Path,
  fail: &Path,
) -> ChildGuard {
  let mut command = child_command();
  command
    .arg("write-artifact-fail-after-poll")
    .arg(root)
    .arg(run_id.to_string())
    .arg(artifact_id.to_string())
    .arg(key.to_string())
    .arg(body)
    .arg(ready)
    .arg(go)
    .arg(body_polled)
    .arg(fail);
  ChildGuard::spawn(&mut command)
}

fn child_command() -> Command {
  Command::new(env!("CARGO_BIN_EXE_file_store_child"))
}

fn wait_for_ready(paths: &[&Path], children: &mut [&mut ChildGuard]) {
  let deadline = Instant::now() + PROCESS_TIMEOUT;
  while !paths.iter().all(|path| path.exists()) {
    if Instant::now() >= deadline {
      for child in children.iter_mut() {
        child.terminate();
      }
      panic!("file-store children did not become ready within five seconds");
    }
    thread::sleep(Duration::from_millis(10));
  }
}

fn release(path: &Path) {
  let temporary = path.with_extension("tmp");
  fs::write(&temporary, []).unwrap();
  fs::rename(temporary, path).unwrap();
}

fn commit_revision(result: ChildResult) -> u64 {
  match result {
    ChildResult::CommitAppended { revision } | ChildResult::CommitReplayed { revision } => revision,
    result => panic!("commit child returned {result:?}"),
  }
}

fn artifact_revision(result: ChildResult) -> u64 {
  match result {
    ChildResult::ArtifactAppended { revision } | ChildResult::ArtifactReplayed { revision } => revision,
    result => panic!("artifact child returned {result:?}"),
  }
}

fn revision(value: u64) -> RunRevision {
  RunRevision::new(value).unwrap()
}

fn collect_artifact(mut reader: auv_tracing::ArtifactReader) -> Vec<u8> {
  futures_executor::block_on(async {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.next().await {
      bytes.extend_from_slice(&chunk.unwrap());
    }
    bytes
  })
}

fn blob_count(root: &Path) -> usize {
  let sha_root = root.join("blobs").join("sha256");
  fs::read_dir(sha_root)
    .unwrap()
    .map(|prefix| {
      fs::read_dir(prefix.unwrap().path())
        .unwrap()
        .filter(|entry| entry.as_ref().is_ok_and(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())))
        .count()
    })
    .sum()
}

fn assert_pending<F: Future>(mut future: Pin<&mut F>) {
  let waker = noop_waker();
  let mut context = Context::from_waker(&waker);
  assert!(matches!(Future::poll(future.as_mut(), &mut context), Poll::Pending));
}
