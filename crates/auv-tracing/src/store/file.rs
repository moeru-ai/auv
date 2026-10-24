use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::{read_artifact_body, store_error};
use crate::{ArtifactBody, ArtifactMetadata, ArtifactRequest, BoxFuture, StoreError, TraceRecord, TracingStore};

const RECORDS_FILE: &str = "records.jsonl";
const ARTIFACTS_DIRECTORY: &str = "artifacts";

/// The `records.jsonl` envelope version this store writes and reads.
///
/// Version 2 added `occurred_at` to artifact records.
// TODO(trace-envelope-v1-read): version 1 stores (no artifact time) are
// rejected line by line. Add a read adapter only if an owner needs replays of
// stores written before 2026-10-21; they also lack command spans.
pub const ENVELOPE_VERSION: u32 = 2;

/// A simple append-only file destination for trace records and artifact bytes.
pub struct FileTracingStore {
  root: PathBuf,
  records: Mutex<BufWriter<File>>,
}

impl FileTracingStore {
  /// Opens or creates a store rooted at `root`.
  pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
    let root = root.as_ref().to_path_buf();
    fs::create_dir_all(root.join(ARTIFACTS_DIRECTORY)).map_err(|_| store_error("auv.tracing.store.open_failed"))?;
    let file = OpenOptions::new()
      .create(true)
      .append(true)
      .open(root.join(RECORDS_FILE))
      .map_err(|_| store_error("auv.tracing.store.open_failed"))?;
    Ok(Self {
      root,
      records: Mutex::new(BufWriter::new(file)),
    })
  }

  /// Resolves durable artifact metadata to this file store's physical body path.
  pub fn artifact_path(&self, metadata: &ArtifactMetadata) -> PathBuf {
    Self::artifact_path_in(&self.root, metadata)
  }

  /// Resolves artifact metadata read from the store at `root` to its body path.
  pub fn artifact_path_in(root: &Path, metadata: &ArtifactMetadata) -> PathBuf {
    artifact_path_for(root, metadata.uri().run_id().to_string(), metadata.uri().artifact_id().to_string(), metadata.file_extension())
  }

  /// Iterates the records of the store at `root` in write order.
  ///
  /// Write order is not time order across producers: an artifact record is
  /// written after its body, and separate processes keep separate stores. Sort
  /// by the records' own timestamps when order matters.
  ///
  /// This reads the durable file layout; it is not part of the write-only
  /// [`TracingStore`] port.
  pub fn read_records(root: &Path) -> Result<RecordReader, StoreError> {
    let file = File::open(root.join(RECORDS_FILE)).map_err(|_| store_error("auv.tracing.store.read_open_failed"))?;
    Ok(RecordReader {
      lines: BufReader::new(file).lines(),
      line: 0,
    })
  }

  fn request_artifact_path(&self, request: &ArtifactRequest) -> PathBuf {
    artifact_path_for(&self.root, request.run_id().to_string(), request.artifact_id().to_string(), request.file_extension())
  }
}

fn artifact_path_for(root: &Path, run_id: String, artifact_id: String, file_extension: Option<&str>) -> PathBuf {
  let mut path = root.join(ARTIFACTS_DIRECTORY).join(run_id).join(artifact_id);
  if let Some(extension) = file_extension {
    path.set_extension(extension);
  }
  path
}

/// One record read back from a file store, with its 1-based line number.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredRecord {
  pub line: u64,
  pub record: TraceRecord,
}

/// A line of `records.jsonl` that could not be read.
///
/// A crash can leave the last line torn, so readers may choose to skip a
/// failure on the final line and fail on any other.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("trace record line {line}: {code}")]
pub struct RecordReadError {
  pub line: u64,
  pub code: crate::ErrorCode,
}

/// Iterator over a file store's records; see [`FileTracingStore::read_records`].
pub struct RecordReader {
  lines: std::io::Lines<BufReader<File>>,
  line: u64,
}

impl Iterator for RecordReader {
  type Item = Result<StoredRecord, RecordReadError>;

  fn next(&mut self) -> Option<Self::Item> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
      version: u32,
      record: Box<serde_json::value::RawValue>,
    }
    let text = self.lines.next()?;
    self.line += 1;
    let line = self.line;
    let failure = |code: &'static str| RecordReadError {
      line,
      code: crate::ErrorCode::new(code),
    };
    Some((|| {
      let text = text.map_err(|_| failure("auv.tracing.store.read_failed"))?;
      let envelope: Envelope = serde_json::from_str(&text).map_err(|_| failure("auv.tracing.store.envelope_invalid"))?;
      if envelope.version != ENVELOPE_VERSION {
        return Err(failure("auv.tracing.store.unsupported_envelope_version"));
      }
      let record = serde_json::from_str(envelope.record.get()).map_err(|_| failure("auv.tracing.store.record_invalid"))?;
      Ok(StoredRecord { line, record })
    })())
  }
}

impl TracingStore for FileTracingStore {
  fn write(&self, record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async move {
      #[derive(Serialize)]
      struct Envelope<'a> {
        version: u32,
        record: &'a TraceRecord,
      }
      let mut line = serde_json::to_vec(&Envelope {
        version: ENVELOPE_VERSION,
        record: &record,
      })
      .map_err(|_| store_error("auv.tracing.store.record_encode_failed"))?;
      // NOTICE(trace-store-shared-append): several processes may append to one
      // store (`auv mcp` and `auv invoke` share `<project>/.auv/store`). Each
      // line goes to the buffer as one write, so the buffer only ever writes
      // whole lines, and an O_APPEND write of a regular file is not split by
      // another process's append. A record and its newline written separately
      // let another line land between them once a record outgrew the buffer.
      line.push(b'\n');
      let mut writer = self.records.lock().map_err(|_| store_error("auv.tracing.store.poisoned"))?;
      writer.write_all(&line).map_err(|_| store_error("auv.tracing.store.record_write_failed"))
    })
  }

  fn write_artifact(&self, request: ArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    Box::pin(async move {
      let bytes = read_artifact_body(&request, body).await?;
      let metadata = request.metadata();
      let path = self.request_artifact_path(&request);
      let parent = path.parent().expect("artifact path has a run directory");
      fs::create_dir_all(parent).map_err(|_| store_error("auv.tracing.store.artifact_write_failed"))?;
      fs::write(path, bytes).map_err(|_| store_error("auv.tracing.store.artifact_write_failed"))?;
      Ok(metadata)
    })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async move {
      self
        .records
        .lock()
        .map_err(|_| store_error("auv.tracing.store.poisoned"))?
        .flush()
        .map_err(|_| store_error("auv.tracing.store.flush_failed"))
    })
  }
}
