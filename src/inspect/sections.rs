//! Core inspect section implementations (prefix + suffix).

use std::sync::Arc;

use auv_inspect_model::legacy::{InspectComposer, InspectError, InspectSection, InspectSectionOutput};
use auv_tracing::{RunSnapshot, RunStore};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::{inspect_run_core_prefix_body_with_observations, inspect_run_core_suffix_body};
use crate::contract::ObservationSnapshot;
use crate::run_read::{ScrollScanReadError, read_scroll_scan};
use crate::scroll_scan::SCROLL_SCAN_PURPOSE;

/// Core prefix: run header through Input Actions / Verifications / Observations /
/// Detector Recognition Lineage.
pub struct CorePrefixSection;

struct CorePrefixSectionV1 {
  canonical_scroll_scan_observations: Vec<ObservationSnapshot>,
}

impl CorePrefixSectionV1 {
  async fn v1(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<Self, ScrollScanReadError> {
    let mut observations = Vec::new();
    for (uri, published) in snapshot.artifacts() {
      if published.metadata().purpose().as_str() != SCROLL_SCAN_PURPOSE {
        continue;
      }
      observations.extend(read_scroll_scan(store, snapshot, uri).await?.snapshots);
    }
    Ok(Self {
      canonical_scroll_scan_observations: observations,
    })
  }
}

impl InspectSection for CorePrefixSection {
  fn id(&self) -> &'static str {
    "core_prefix"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    collect_core_prefix(store, run, None, self.id())
  }
}

impl InspectSection for CorePrefixSectionV1 {
  fn id(&self) -> &'static str {
    "core_prefix"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    collect_core_prefix(store, run, Some(&self.canonical_scroll_scan_observations), self.id())
  }
}

fn collect_core_prefix(
  store: &LocalStore,
  run: &CanonicalRun,
  canonical_scroll_scan_observations: Option<&[ObservationSnapshot]>,
  id: &'static str,
) -> Result<Option<InspectSectionOutput>, InspectError> {
  let text = inspect_run_core_prefix_body_with_observations(store, run.run.run_id.as_str(), canonical_scroll_scan_observations)
    .map_err(InspectError::Message)?;
  // Prefer always Some(...) matching legacy (headers are never empty).
  Ok(Some(InspectSectionOutput {
    id,
    text,
    json: None,
  }))
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

pub(crate) async fn build_core_inspect_composer_v1(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Arc<InspectComposer>, ScrollScanReadError> {
  let prefix = CorePrefixSectionV1::v1(store, snapshot).await?;
  Ok(Arc::new(
    InspectComposer::try_new(vec![Arc::new(prefix), Arc::new(CoreSuffixSection)])
      .expect("the static core V1 inspect section IDs are unique"),
  ))
}
