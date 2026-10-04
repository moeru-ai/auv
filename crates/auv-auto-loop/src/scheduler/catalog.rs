//! Storage for active operations, isolated operations, and manual review backlog.

use crate::models::{ManualReviewItem, OperationDef};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct OperationCatalog {
  active_operations: HashMap<String, OperationDef>,
  isolated_operations: HashMap<String, OperationDef>,
  manual_review_queue: Vec<ManualReviewItem>,
}

impl OperationCatalog {
  pub fn new() -> Self {
    Self::default()
  }

  /// Adds a compiled operation to the active pool.
  pub fn register_active(&mut self, op: OperationDef) {
    self.active_operations.insert(op.name.clone(), op);
  }

  /// Retrieves an active operation by exact name.
  pub fn get_active(&self, name: &str) -> Option<&OperationDef> {
    self.active_operations.get(name)
  }

  /// Checks if an operation has been isolated.
  pub fn is_isolated(&self, name: &str) -> bool {
    self.isolated_operations.contains_key(name)
  }

  /// Moves an operation from the active pool to the isolated pool upon repeated failures,
  /// and queues an item for human inspection.
  pub fn isolate(&mut self, name: &str, review_item: ManualReviewItem) {
    if let Some(op) = self.active_operations.remove(name) {
      self.isolated_operations.insert(name.to_string(), op);
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

  pub fn manual_review_queue(&self) -> &[ManualReviewItem] {
    &self.manual_review_queue
  }
}
