//! Versioned serialized form of [`CoverageView`](crate::CoverageView).

use serde::{Deserialize, Serialize};

use crate::coverage::CoverageView;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanCoverageArtifact {
  schema: ScanCoverageSchema,
  coverage: CoverageView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum ScanCoverageSchema {
  #[serde(rename = "auv.scan.coverage.v1")]
  V1,
}

impl ScanCoverageArtifact {
  pub fn new(coverage: CoverageView) -> Self {
    Self {
      schema: ScanCoverageSchema::V1,
      coverage,
    }
  }

  pub fn coverage(&self) -> &CoverageView {
    &self.coverage
  }

  pub fn into_coverage(self) -> CoverageView {
    self.coverage
  }
}
