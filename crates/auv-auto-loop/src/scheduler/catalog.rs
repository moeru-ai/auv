//! Storage for active operations, isolated operations, and manual review backlog.
//!
//! Provides durable persistence for operation isolation state:
//! - Auto-isolation records (operation name, reason code, reason description, timestamp, execution mode) are persisted to disk.
//! - Restarting the catalog reloads persisted isolation records on startup.
//! - Registration of previously isolated operations is rejected to prevent bad operation revival.
//! - Enforces fail-closed schema validation: operations without `execution_mode` or with outdated schema_version are rejected.

use crate::compiler::compile_gate;
use crate::models::{ManualReviewItem, OPERATION_SCHEMA_VERSION, OperationDef, PersistedIsolationRecord, ReasonCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Typed failure returned when an operation fails catalog admission or validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdmissionFailure {
  pub operation_key: String,
  pub location: String,
  pub reason_code: ReasonCode,
  pub message: String,
}

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
    let execution_mode = record.execution_mode.or_else(|| self.isolated_operations.get(&name).map(|op| op.execution_mode));
    let review_item = ManualReviewItem {
      id: format!("persisted_{}", name),
      task_name: name.clone(),
      reason_code: record.reason_code,
      reason_description: record.reason_description.clone(),
      execution_mode,
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
  /// Enforces fail-closed schema validation:
  /// - `op.schema_version` must match `OPERATION_SCHEMA_VERSION`.
  /// - If the operation has already been isolated, registration is rejected
  ///   to prevent reviving a faulty operation.
  pub fn register_active(&mut self, op: OperationDef) -> Result<bool, AdmissionFailure> {
    if op.schema_version != OPERATION_SCHEMA_VERSION {
      return Err(AdmissionFailure {
        operation_key: op.name.clone(),
        location: "register_active".to_string(),
        reason_code: ReasonCode::RejectSchemaVersionMismatch,
        message: format!(
          "Operation '{}' rejected: schema_version '{}' does not match current '{}'",
          op.name, op.schema_version, OPERATION_SCHEMA_VERSION
        ),
      });
    }

    if self.is_isolated(&op.name) {
      self.isolated_operations.insert(op.name.clone(), op);
      return Ok(false);
    }
    self.active_operations.insert(op.name.clone(), op);
    Ok(true)
  }

  /// Admits an operation from a raw JSON string into the catalog fail-closed.
  pub fn admit_operation_json(&mut self, raw_json: &str, location: &str) -> Result<OperationDef, AdmissionFailure> {
    let op = validate_and_parse_operation(raw_json, location)?;
    let registered = self.register_active(op.clone())?;
    if !registered {
      return Err(AdmissionFailure {
        operation_key: op.name.clone(),
        location: location.to_string(),
        reason_code: ReasonCode::AutoIsolatedConsecutiveFailures,
        message: format!("Operation '{}' is isolated; cannot admit into active catalog", op.name),
      });
    }
    Ok(op)
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
  pub fn isolate(&mut self, name: &str, mut review_item: ManualReviewItem) {
    let op = self.active_operations.remove(name);
    let execution_mode = review_item
      .execution_mode
      .or_else(|| op.as_ref().map(|o| o.execution_mode))
      .or_else(|| self.isolated_operations.get(name).map(|o| o.execution_mode));
    review_item.execution_mode = execution_mode;

    if let Some(ref o) = op {
      self.isolated_operations.insert(name.to_string(), o.clone());
    }

    let record = PersistedIsolationRecord {
      operation_name: name.to_string(),
      reason_code: review_item.reason_code,
      reason_description: review_item.reason_description.clone(),
      execution_mode,
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

/// Parses and validates an operation definition JSON string fail-closed.
///
/// Enforces:
/// - Missing `execution_mode` field is rejected with `REJECT_MODE_UNDECLARED`.
/// - Outdated or mismatched `schema_version` is rejected with `REJECT_SCHEMA_VERSION_MISMATCH`.
/// - Incompatible mode vs gates (e.g. `unverified-step` + Fast) is rejected with `REJECT_MODE_CONFLICT`.
pub fn validate_and_parse_operation(raw_json: &str, location: &str) -> Result<OperationDef, AdmissionFailure> {
  let val: serde_json::Value = serde_json::from_str(raw_json).map_err(|e| AdmissionFailure {
    operation_key: "unknown".to_string(),
    location: location.to_string(),
    reason_code: ReasonCode::RejectModeUndeclared,
    message: format!("Failed to parse operation JSON at {}: {}", location, e),
  })?;

  let op_name = val.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

  // 1. Fail-closed: execution_mode MUST be present
  if val.get("execution_mode").is_none() || val.get("execution_mode").and_then(|v| v.as_str()).is_none() {
    return Err(AdmissionFailure {
      operation_key: op_name.clone(),
      location: location.to_string(),
      reason_code: ReasonCode::RejectModeUndeclared,
      message: format!("Operation '{}' rejected at {}: missing required field 'execution_mode'", op_name, location),
    });
  }

  // 2. Fail-closed: schema_version check
  let schema = val.get("schema_version").and_then(|v| v.as_str()).unwrap_or("");
  if schema != OPERATION_SCHEMA_VERSION {
    return Err(AdmissionFailure {
      operation_key: op_name.clone(),
      location: location.to_string(),
      reason_code: ReasonCode::RejectSchemaVersionMismatch,
      message: format!(
        "Operation '{}' rejected at {}: schema_version '{}' does not match current '{}'",
        op_name, location, schema, OPERATION_SCHEMA_VERSION
      ),
    });
  }

  // 3. Deserialize into typed OperationDef
  let op_def: OperationDef = serde_json::from_value(val).map_err(|e| AdmissionFailure {
    operation_key: op_name.clone(),
    location: location.to_string(),
    reason_code: ReasonCode::RejectModeUndeclared,
    message: format!("Operation '{}' deserialization failed at {}: {}", op_name, location, e),
  })?;

  // 4. Validate execution mode gates
  let mode_eval = compile_gate::evaluate_execution_mode_gate(op_def.execution_mode, &op_def.steps, &op_def.tags);
  if !mode_eval.passed {
    return Err(AdmissionFailure {
      operation_key: op_def.name.clone(),
      location: location.to_string(),
      reason_code: mode_eval.reason_code,
      message: mode_eval.message,
    });
  }

  Ok(op_def)
}
