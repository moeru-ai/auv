#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process;
use std::str::FromStr;
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

use auv_tracing::{ArtifactWriteError, CommitResult, FileRunStore, RunStore, SpanId};
use auv_tracing_conformance::{artifact_request_with_span, event_request};
use futures_io::AsyncRead;
use futures_util::io::Cursor;
use serde::Serialize;

const GATE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
enum ChildResult {
  Authority {
    authority_id: auv_tracing::AuthorityId,
  },
  CommitAppended {
    revision: u64,
  },
  CommitReplayed {
    revision: u64,
  },
  ArtifactAppended {
    revision: u64,
  },
  ArtifactReplayed {
    revision: u64,
  },
  ArtifactConflict,
}

fn main() {
  let mut args = env::args().skip(1);
  let command = args.next().expect("missing child protocol command");
  let result = match command.as_str() {
    "authority" => {
      let root = path_arg(&mut args, "root");
      let ready = path_arg(&mut args, "ready-file");
      let go = path_arg(&mut args, "go-file");
      finish_args(args);
      signal_ready(&ready);
      wait_for_go(&go);
      let store = FileRunStore::open(root).expect("authority child failed to open store");
      ChildResult::Authority {
        authority_id: store.authority_id(),
      }
    }
    "commit-event" => {
      let root = path_arg(&mut args, "root");
      let run_id = parse_arg(&mut args, "run-id");
      let event_id = parse_arg(&mut args, "event-id");
      let key = parse_arg(&mut args, "idempotency-key");
      let value = args.next().expect("missing value");
      let ready = path_arg(&mut args, "ready-file");
      let go = path_arg(&mut args, "go-file");
      finish_args(args);
      let store = FileRunStore::open(root).expect("commit child failed to open store");
      signal_ready(&ready);
      wait_for_go(&go);
      let request = event_request(store.authority_id(), run_id, event_id, key, value);
      match futures_executor::block_on(store.commit(request)).expect("commit child failed to append event") {
        CommitResult::Appended(commit) => ChildResult::CommitAppended {
          revision: commit.revision().get(),
        },
        CommitResult::Replayed(commit) => ChildResult::CommitReplayed {
          revision: commit.revision().get(),
        },
      }
    }
    "write-artifact" => {
      let root = path_arg(&mut args, "root");
      let run_id = parse_arg(&mut args, "run-id");
      let span_id = match args.next().expect("missing span-id-or-none") {
        value if value == "none" => None,
        value => Some(value.parse::<SpanId>().unwrap_or_else(|error| panic!("invalid span-id: {error:?}"))),
      };
      let artifact_id = parse_arg(&mut args, "artifact-id");
      let key = parse_arg(&mut args, "idempotency-key");
      let body_file = path_arg(&mut args, "body-file");
      let ready = path_arg(&mut args, "ready-file");
      let go = path_arg(&mut args, "go-file");
      finish_args(args);
      let store = FileRunStore::open(root).expect("artifact child failed to open store");
      let bytes = fs::read(body_file).expect("artifact child failed to read body fixture");
      let request = artifact_request_with_span(store.authority_id(), run_id, key, artifact_id, span_id, &bytes);
      signal_ready(&ready);
      wait_for_go(&go);
      match futures_executor::block_on(store.write_artifact(request, Box::pin(Cursor::new(bytes)))) {
        Ok(result) => artifact_result(result),
        Err(ArtifactWriteError::Rejected(_)) => ChildResult::ArtifactConflict,
        Err(error) => panic!("artifact child failed with unexpected error: {error:?}"),
      }
    }
    "write-artifact-fail-after-poll" => {
      let root = path_arg(&mut args, "root");
      let run_id = parse_arg(&mut args, "run-id");
      let artifact_id = parse_arg(&mut args, "artifact-id");
      let key = parse_arg(&mut args, "idempotency-key");
      let body_file = path_arg(&mut args, "body-file");
      let ready = path_arg(&mut args, "ready-file");
      let go = path_arg(&mut args, "go-file");
      let body_polled = path_arg(&mut args, "body-polled-file");
      let fail = path_arg(&mut args, "fail-file");
      finish_args(args);
      let store = FileRunStore::open(root).expect("artifact child failed to open store");
      let bytes = fs::read(body_file).expect("artifact child failed to read body fixture");
      let request = artifact_request_with_span(store.authority_id(), run_id, key, artifact_id, None, &bytes);
      signal_ready(&ready);
      wait_for_go(&go);
      let body = GatedFailureBody {
        body_polled,
        fail,
        announced: false,
      };
      match futures_executor::block_on(store.write_artifact(request, Box::pin(body))) {
        Ok(result) => artifact_result(result),
        Err(error) => panic!("artifact child failed with unexpected error: {error:?}"),
      }
    }
    _ => panic!("unknown child protocol command: {command}"),
  };

  println!("{}", serde_json::to_string(&result).expect("child result is serializable"));
}

fn artifact_result(result: CommitResult) -> ChildResult {
  match result {
    CommitResult::Appended(commit) => ChildResult::ArtifactAppended {
      revision: commit.revision().get(),
    },
    CommitResult::Replayed(commit) => ChildResult::ArtifactReplayed {
      revision: commit.revision().get(),
    },
  }
}

struct GatedFailureBody {
  body_polled: PathBuf,
  fail: PathBuf,
  announced: bool,
}

impl AsyncRead for GatedFailureBody {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, _buffer: &mut [u8]) -> Poll<io::Result<usize>> {
    if !self.announced {
      signal_ready(&self.body_polled);
      self.announced = true;
    }
    if self.fail.exists() {
      return Poll::Ready(Err(io::Error::new(io::ErrorKind::ConnectionReset, "gated artifact body failure")));
    }
    thread::sleep(Duration::from_millis(1));
    context.waker().wake_by_ref();
    Poll::Pending
  }
}

fn path_arg(args: &mut impl Iterator<Item = String>, name: &str) -> PathBuf {
  PathBuf::from(args.next().unwrap_or_else(|| panic!("missing {name}")))
}

fn parse_arg<T>(args: &mut impl Iterator<Item = String>, name: &str) -> T
where
  T: FromStr,
  T::Err: std::fmt::Debug,
{
  args.next().unwrap_or_else(|| panic!("missing {name}")).parse().unwrap_or_else(|error| panic!("invalid {name}: {error:?}"))
}

fn finish_args(mut args: impl Iterator<Item = String>) {
  assert!(args.next().is_none(), "unexpected child protocol arguments");
}

fn signal_ready(path: &Path) {
  let temporary = path.with_extension(format!("{}.tmp", process::id()));
  fs::write(&temporary, []).expect("failed to write child ready marker");
  fs::rename(temporary, path).expect("failed to publish child ready marker");
}

fn wait_for_go(path: &Path) {
  let deadline = Instant::now() + GATE_TIMEOUT;
  while !path.exists() {
    assert!(Instant::now() < deadline, "timed out waiting for parent go marker");
    thread::sleep(Duration::from_millis(10));
  }
}
