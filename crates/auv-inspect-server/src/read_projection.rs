//! Disposable Inspect read projection from canonical run state.

use std::sync::Arc;

use auv_inspect_model::InspectDocument;
use auv_tracing::{BoxFuture, RunSnapshot, RunStore};

use crate::InspectResult;

/// Projects named product data from one canonical Inspect authority snapshot.
pub trait InspectRunExtension: Send + Sync + 'static {
  /// Returns JSON for a recognized extension name, or `None` when unknown.
  fn project_json<'a>(
    &'a self,
    extension: &'a str,
    store: &'a Arc<dyn RunStore>,
    snapshot: &'a RunSnapshot,
  ) -> BoxFuture<'a, InspectResult<Option<serde_json::Value>>>;
}

pub(crate) struct DefaultInspectRunExtension;

impl InspectRunExtension for DefaultInspectRunExtension {
  fn project_json<'a>(
    &'a self,
    _extension: &'a str,
    _store: &'a Arc<dyn RunStore>,
    _snapshot: &'a RunSnapshot,
  ) -> BoxFuture<'a, InspectResult<Option<serde_json::Value>>> {
    Box::pin(async { Ok(None) })
  }
}

/// Builds viewer-facing data without introducing a second read authority.
pub fn project_snapshot(snapshot: &RunSnapshot) -> InspectDocument {
  InspectDocument::from(snapshot)
}
