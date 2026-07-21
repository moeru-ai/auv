//! Disposable Inspect read projection from canonical run state.

use auv_inspect_model::InspectDocument;
use auv_tracing::RunSnapshot;

/// Builds viewer-facing data without introducing a second read authority.
pub fn project_snapshot(snapshot: &RunSnapshot) -> InspectDocument {
  InspectDocument::from(snapshot)
}
