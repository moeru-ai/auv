//! Structured decision logging system enforcing the ZERO silent errors redline.

use crate::models::{DecisionAction, DecisionCategory, DecisionLog, ReasonCode};
use chrono::Utc;
use std::sync::{Arc, Mutex};

/// Thread-safe structured decision logger.
#[derive(Debug, Clone, Default)]
pub struct DecisionLogger {
  logs: Arc<Mutex<Vec<DecisionLog>>>,
}

impl DecisionLogger {
  pub fn new() -> Self {
    Self {
      logs: Arc::new(Mutex::new(Vec::new())),
    }
  }

  /// Records an automated decision with its mandatory reason code.
  #[allow(clippy::too_many_arguments)]
  pub fn log(
    &self,
    category: DecisionCategory,
    decision: DecisionAction,
    reason_code: ReasonCode,
    task_name: impl Into<String>,
    operation_id: Option<String>,
    message: impl Into<String>,
    details: serde_json::Value,
  ) -> DecisionLog {
    let entry = DecisionLog {
      timestamp: Utc::now().to_rfc3339(),
      category,
      decision,
      reason_code,
      operation_id,
      task_name: task_name.into(),
      message: message.into(),
      details,
    };

    if let Ok(mut lock) = self.logs.lock() {
      lock.push(entry.clone());
    }

    entry
  }

  /// Returns a snapshot of all logged decisions.
  pub fn entries(&self) -> Vec<DecisionLog> {
    self.logs.lock().map(|l| l.clone()).unwrap_or_default()
  }

  /// Finds entries matching a specific reason code.
  pub fn find_by_reason(&self, code: ReasonCode) -> Vec<DecisionLog> {
    self.entries().into_iter().filter(|e| e.reason_code == code).collect()
  }

  /// Clears all recorded entries.
  pub fn clear(&self) {
    if let Ok(mut lock) = self.logs.lock() {
      lock.clear();
    }
  }
}
