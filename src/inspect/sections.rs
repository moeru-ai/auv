//! Core inspect section implementations (prefix + suffix).

use std::sync::Arc;

use auv_inspect_model::{InspectComposer, InspectError, InspectSection, InspectSectionOutput};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::{inspect_run_core_prefix_body, inspect_run_core_suffix_body};

/// Core prefix: run header through Detector Recognition Lineage.
pub struct CorePrefixSection;

impl InspectSection for CorePrefixSection {
  fn id(&self) -> &'static str {
    "core_prefix"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let text = inspect_run_core_prefix_body(store, run.run.run_id.as_str()).map_err(InspectError::Message)?;
    // Prefer always Some(...) matching legacy (headers are never empty).
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

/// Core suffix: view-parser proof + scene state (after donor sections).
pub struct CoreSuffixSection;

impl InspectSection for CoreSuffixSection {
  fn id(&self) -> &'static str {
    "core_suffix"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let text = inspect_run_core_suffix_body(store, run.run.run_id.as_str()).map_err(InspectError::Message)?;
    // Scene-state formatting may be non-empty even when view-parser proof is absent.
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

pub fn build_core_inspect_composer() -> Result<Arc<InspectComposer>, InspectError> {
  InspectComposer::try_new(vec![Arc::new(CorePrefixSection), Arc::new(CoreSuffixSection)]).map(Arc::new)
}
