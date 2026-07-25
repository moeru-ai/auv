use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;

use super::{read_artifact_body, store_error};
use crate::{ArtifactBody, ArtifactMetadata, ArtifactRequest, BoxFuture, StoreError, TraceRecord, TracingStore};

/// A simple append-only file destination for trace records and artifact bytes.
pub struct FileTracingStore {
  root: PathBuf,
  records: Mutex<BufWriter<File>>,
}

impl FileTracingStore {
  /// Opens or creates a store rooted at `root`.
  pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
    let root = root.as_ref().to_path_buf();
    fs::create_dir_all(root.join("artifacts")).map_err(|_| store_error("auv.tracing.store.open_failed"))?;
    let file = OpenOptions::new()
      .create(true)
      .append(true)
      .open(root.join("records.jsonl"))
      .map_err(|_| store_error("auv.tracing.store.open_failed"))?;
    Ok(Self {
      root,
      records: Mutex::new(BufWriter::new(file)),
    })
  }

  fn artifact_path(&self, request: &ArtifactRequest) -> PathBuf {
    self.root.join("artifacts").join(request.run_id().to_string()).join(request.artifact_id().to_string())
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
      let encoded = serde_json::to_vec(&Envelope {
        version: 1,
        record: &record,
      })
      .map_err(|_| store_error("auv.tracing.store.record_encode_failed"))?;
      let mut writer = self.records.lock().map_err(|_| store_error("auv.tracing.store.poisoned"))?;
      writer.write_all(&encoded).and_then(|_| writer.write_all(b"\n")).map_err(|_| store_error("auv.tracing.store.record_write_failed"))
    })
  }

  fn write_artifact(&self, request: ArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    Box::pin(async move {
      let bytes = read_artifact_body(&request, body).await?;
      let metadata = request.metadata();
      let path = self.artifact_path(&request);
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
