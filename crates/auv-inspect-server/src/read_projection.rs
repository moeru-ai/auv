//! Disposable Inspect read projection from canonical run state.

use std::sync::Arc;

use auv_inspect_model::InspectDocument;
use auv_tracing::{BoxFuture, ErrorCode, RunSnapshot, RunStore};

/// HTTP-facing failure category for a named Inspect run extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectRunExtensionErrorCategory {
  /// The extension rejected a caller-supplied typed reference.
  InvalidReference,
  /// The extension authority denied the read.
  Forbidden,
  /// The extension's backing authority is temporarily unavailable.
  Unavailable,
  /// Canonical extension data failed an integrity contract.
  Integrity,
}

/// Safe typed failure returned by an Inspect run extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectRunExtensionError {
  category: InspectRunExtensionErrorCategory,
  code: ErrorCode,
}

impl InspectRunExtensionError {
  /// Creates a failure from one safe category and stable code.
  pub fn new(category: InspectRunExtensionErrorCategory, code: ErrorCode) -> Self {
    Self { category, code }
  }

  /// Returns the route-level failure category.
  pub fn category(&self) -> InspectRunExtensionErrorCategory {
    self.category
  }

  /// Returns the safe stable code; extension-internal messages are not carried.
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Projects named product data from one canonical Inspect authority snapshot.
pub trait InspectRunExtension: Send + Sync + 'static {
  /// Returns JSON for a recognized extension name, or `None` when unknown.
  fn project_json<'a>(
    &'a self,
    extension: &'a str,
    store: &'a Arc<dyn RunStore>,
    snapshot: &'a RunSnapshot,
  ) -> BoxFuture<'a, Result<Option<serde_json::Value>, InspectRunExtensionError>>;
}

pub(crate) struct DefaultInspectRunExtension;

impl InspectRunExtension for DefaultInspectRunExtension {
  fn project_json<'a>(
    &'a self,
    _extension: &'a str,
    _store: &'a Arc<dyn RunStore>,
    _snapshot: &'a RunSnapshot,
  ) -> BoxFuture<'a, Result<Option<serde_json::Value>, InspectRunExtensionError>> {
    Box::pin(async { Ok(None) })
  }
}

/// Builds viewer-facing data without introducing a second read authority.
pub fn project_snapshot(snapshot: &RunSnapshot) -> InspectDocument {
  InspectDocument::from(snapshot)
}
