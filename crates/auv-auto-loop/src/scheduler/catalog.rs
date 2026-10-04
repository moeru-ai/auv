//! Storage for active operations, isolated operations, and manual review backlog.
//!
//! Provides durable persistence for operation isolation state:
//! - Auto-isolation records (operation name, reason code, reason description, timestamp) are persisted to disk.
//! - Restarting the catalog reloads persisted isolation records on startup.
//! - Registration of previously isolated operations is rejected to prevent bad operation revival.

use crate::models::{ManualReviewItem, OperationDef, PersistedIsolationRecord};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct OperationCatalog {
  active_operations: HashMap<String, OperationDef>,
  isolated_operations: HashMap<String, OperationDef>,
  isolated_records: HashMap<String, PersistedIsolationRecord>,
  manual_review_queue: Vec<ManualReviewItem>,
  persistence_path: Option<PathBuf>,
}

impl OperationCatalog {
  pub fn new() -> Self {
    let mut catalog = Self::default();
    if let Ok(env_path) = std::env::var("AUV_ISOLATION_CATALOG_PATH") {
      let path = PathBuf::from(env_path);
      let _ = catalog.load_from_path(&path);
      catalog.persistence_path = Some(path);
    }
    catalog
  }

  /// Creates an operation catalog backed by durable persistence at `path`.
  /// Automatically loads existing isolation records on startup.
  pub fn with_persistence(path: impl Into<PathBuf>) -> std::io::Result<Self> {
    let path = path.into();
    let mut catalog = Self::default();
    catalog.load_from_path(&path)?;
    catalog.persistence_path = Some(path);
    Ok(catalog)
  }

  /// Sets the persistence path and loads any existing records from disk.
  pub fn set_persistence_path(&mut self, path: impl Into<PathBuf>) -> std::io::Result<()> {
    let path = path.into();
    self.load_from_path(&path)?;
    self.persistence_path = Some(path);
    Ok(())
  }

  /// Returns the configured persistence path, if any.
  pub fn persistence_path(&self) -> Option<&Path> {
    self.persistence_path.as_deref()
  }

  /// Loads persisted isolation records from a file path.
  /// Supports JSON Lines format (one JSON object per line) and JSON array format.
  pub fn load_from_path(&mut self, path: &Path) -> std::io::Result<usize> {
    if !path.exists() {
      return Ok(0);
    }
    let content = std::fs::read_to_string(path)?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
      return Ok(0);
    }

    let mut count = 0;
    if trimmed.starts_with('[')
      && let Ok(records) = serde_json::from_str::<Vec<PersistedIsolationRecord>>(trimmed)
    {
      for record in records {
        self.apply_persisted_record(record);
        count += 1;
      }
      return Ok(count);
    }

    for line in trimmed.lines() {
      let line = line.trim();
      if line.is_empty() {
        continue;
      }
      if let Ok(record) = serde_json::from_str::<PersistedIsolationRecord>(line) {
        self.apply_persisted_record(record);
        count += 1;
      }
    }

    Ok(count)
  }

  fn apply_persisted_record(&mut self, record: PersistedIsolationRecord) {
    let name = record.operation_name.clone();
    if let Some(op) = self.active_operations.remove(&name) {
      self.isolated_operations.insert(name.clone(), op);
    }
    let review_item = ManualReviewItem {
      id: format!("persisted_{}", name),
      task_name: name.clone(),
      reason_code: record.reason_code,
      reason_description: record.reason_description.clone(),
      source_trajectory: None,
      isolated_operation: self.isolated_operations.get(&name).map(|op| Box::new(op.clone())),
      created_at: record.isolated_at.clone(),
    };
    if !self.manual_review_queue.iter().any(|item| item.task_name == name) {
      self.manual_review_queue.push(review_item);
    }
    self.isolated_records.insert(name, record);
  }

  fn append_record_to_file(path: &Path, record: &PersistedIsolationRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
      && !parent.as_os_str().is_empty()
    {
      std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(record).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    writeln!(file, "{}", line)?;
    file.flush()?;
    Ok(())
  }

  /// Adds a compiled operation to the active pool.
  ///
  /// If the operation has already been isolated, registration is rejected
  /// to prevent reviving a faulty operation.
  pub fn register_active(&mut self, op: OperationDef) -> bool {
    if self.is_isolated(&op.name) {
      self.isolated_operations.insert(op.name.clone(), op);
      return false;
    }
    self.active_operations.insert(op.name.clone(), op);
    true
  }

  /// Retrieves an active operation by exact name.
  pub fn get_active(&self, name: &str) -> Option<&OperationDef> {
    self.active_operations.get(name)
  }

  /// Checks if an operation has been isolated.
  pub fn is_isolated(&self, name: &str) -> bool {
    self.isolated_operations.contains_key(name) || self.isolated_records.contains_key(name)
  }

  /// Retrieves the persisted isolation record by operation name.
  pub fn get_isolated_record(&self, name: &str) -> Option<&PersistedIsolationRecord> {
    self.isolated_records.get(name)
  }

  /// Retrieves an isolated operation definition by name, if registered.
  pub fn get_isolated(&self, name: &str) -> Option<&OperationDef> {
    self.isolated_operations.get(name)
  }

  /// Moves an operation from the active pool to the isolated pool upon repeated failures,
  /// persists the isolation record to disk if configured, and queues an item for human inspection.
  pub fn isolate(&mut self, name: &str, review_item: ManualReviewItem) {
    if let Some(op) = self.active_operations.remove(name) {
      self.isolated_operations.insert(name.to_string(), op);
    }

    let record = PersistedIsolationRecord {
      operation_name: name.to_string(),
      reason_code: review_item.reason_code,
      reason_description: review_item.reason_description.clone(),
      isolated_at: review_item.created_at.clone(),
    };
    self.isolated_records.insert(name.to_string(), record.clone());

    if let Some(ref path) = self.persistence_path {
      let _ = Self::append_record_to_file(path, &record);
    }

    self.manual_review_queue.push(review_item);
  }

  /// Enqueues a rejected trajectory or compilation failure into the manual review queue.
  pub fn enqueue_manual_review(&mut self, item: ManualReviewItem) {
    self.manual_review_queue.push(item);
  }

  pub fn active_operations(&self) -> &HashMap<String, OperationDef> {
    &self.active_operations
  }

  pub fn isolated_operations(&self) -> &HashMap<String, OperationDef> {
    &self.isolated_operations
  }

  pub fn isolated_records(&self) -> &HashMap<String, PersistedIsolationRecord> {
    &self.isolated_records
  }

  pub fn manual_review_queue(&self) -> &[ManualReviewItem] {
    &self.manual_review_queue
  }
}
