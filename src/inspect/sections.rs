//! Core inspect section implementations (prefix + suffix).

use std::sync::Arc;

use auv_inspect_model::legacy::{InspectComposer, InspectError, InspectSection, InspectSectionOutput};
use auv_tracing::{RunSnapshot, RunStore};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::{inspect_run_core_prefix_body, inspect_run_core_suffix_body};
use crate::contract::ObservationSnapshot;
use crate::run_read::{ScrollScanReadError, read_scroll_scan};
use crate::scroll_scan::SCROLL_SCAN_PURPOSE;

/// Core prefix: run header through Input Actions / Verifications / Observations /
/// Detector Recognition Lineage.
pub struct CorePrefixSection;

impl CorePrefixSection {
  /// Reads the canonical V1 scroll-scan observations for this root section.
  pub async fn read_scroll_scan_observations(
    &self,
    store: &dyn RunStore,
    snapshot: &RunSnapshot,
  ) -> Result<Vec<ObservationSnapshot>, ScrollScanReadError> {
    let mut observations = Vec::new();
    for (uri, published) in snapshot.artifacts() {
      if published.metadata().purpose().as_str() != SCROLL_SCAN_PURPOSE {
        continue;
      }
      observations.extend(read_scroll_scan(store, snapshot, uri).await?.snapshots);
    }
    Ok(observations)
  }
}

impl InspectSection for CorePrefixSection {
  fn id(&self) -> &'static str {
    "core_prefix"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    // TODO(run-contract-task-22): This legacy composer receives only
    // LocalStore/CanonicalRun. Replace it with the V1 section method above when
    // Task 22 migrates the remaining root inspect adapters.
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
