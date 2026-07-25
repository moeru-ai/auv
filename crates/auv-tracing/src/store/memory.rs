use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{read_artifact_body, store_error};
use crate::{ArtifactBody, ArtifactMetadata, ArtifactRequest, ArtifactUri, BoxFuture, StoreError, TraceRecord, TracingStore};

/// In-memory write destination intended for tests and ephemeral embedding.
#[derive(Default)]
pub struct MemoryTracingStore {
  records: Mutex<Vec<TraceRecord>>,
  artifacts: Mutex<BTreeMap<ArtifactUri, Vec<u8>>>,
}

impl MemoryTracingStore {
  pub fn new() -> Self {
    Self::default()
  }

  /// Returns a copy of observed records.
  ///
  /// This concrete test utility is intentionally not part of [`TracingStore`].
  pub fn records(&self) -> Vec<TraceRecord> {
    self.records.lock().unwrap().clone()
  }

  /// Returns a copied body for concrete-store tests.
  pub fn artifact(&self, uri: &ArtifactUri) -> Option<Vec<u8>> {
    self.artifacts.lock().unwrap().get(uri).cloned()
  }
}

impl TracingStore for MemoryTracingStore {
  fn write(&self, record: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async move {
      self.records.lock().map_err(|_| store_error("auv.tracing.store.poisoned"))?.push(record);
      Ok(())
    })
  }

  fn write_artifact(&self, request: ArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    Box::pin(async move {
      let bytes = read_artifact_body(&request, body).await?;
      let metadata = request.metadata();
      self.artifacts.lock().map_err(|_| store_error("auv.tracing.store.poisoned"))?.insert(metadata.uri().clone(), bytes);
      Ok(metadata)
    })
  }

  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }
}
