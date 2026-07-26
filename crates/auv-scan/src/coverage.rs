//! Coverage evaluator and canonical typed result.

use crate::association::AssociationResult;
use crate::reader::ScanFrameBundle;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageEntry {
  pub track_id: String,
  pub last_seen_frame_id: String,
  pub observation_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NegativeEvidence {
  pub code: String,
  pub after_frame_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoverageStatus {
  Complete,
  Incomplete {
    reason: String,
    open_uncertainty_codes: Vec<String>,
    negative_evidence: Vec<NegativeEvidence>,
  },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageView {
  pub entries: Vec<CoverageEntry>,
  status: CoverageStatus,
}

impl CoverageView {
  pub fn complete(entries: Vec<CoverageEntry>) -> Self {
    Self {
      entries,
      status: CoverageStatus::Complete,
    }
  }

  pub fn incomplete(
    entries: Vec<CoverageEntry>,
    reason: impl Into<String>,
    open_uncertainty_codes: Vec<String>,
    negative_evidence: Vec<NegativeEvidence>,
  ) -> Self {
    Self {
      entries,
      status: CoverageStatus::Incomplete {
        reason: reason.into(),
        open_uncertainty_codes,
        negative_evidence,
      },
    }
  }

  pub fn status(&self) -> &CoverageStatus {
    &self.status
  }

  pub fn open_uncertainty_codes(&self) -> &[String] {
    match &self.status {
      CoverageStatus::Complete => &[],
      CoverageStatus::Incomplete {
        open_uncertainty_codes,
        ..
      } => open_uncertainty_codes,
    }
  }

  pub fn negative_evidence(&self) -> &[NegativeEvidence] {
    match &self.status {
      CoverageStatus::Complete => &[],
      CoverageStatus::Incomplete {
        negative_evidence, ..
      } => negative_evidence,
    }
  }
}

/// Build an in-memory coverage view from a frame bundle and association results.
pub fn build_coverage_view(bundle: &ScanFrameBundle, associations: &[AssociationResult]) -> CoverageView {
  build_coverage_view_for_sequence(bundle.frames.len(), bundle.frames.last().map(|frame| frame.frame_id.as_str()), associations)
}

pub(crate) fn build_coverage_view_for_sequence(
  frame_count: usize,
  last_frame_id: Option<&str>,
  associations: &[AssociationResult],
) -> CoverageView {
  let mut entries = Vec::new();
  let mut open_uncertainty_codes = Vec::new();
  let mut negative_evidence = Vec::new();
  let last_frame_id = last_frame_id.unwrap_or_default();

  for association in associations {
    match association {
      AssociationResult::Linked { track_id, .. } => {
        entries.push(CoverageEntry {
          track_id: track_id.clone(),
          last_seen_frame_id: last_frame_id.to_string(),
          observation_count: 2,
        });
      }
      AssociationResult::NewTrack { track_id, .. } => {
        entries.push(CoverageEntry {
          track_id: track_id.clone(),
          last_seen_frame_id: last_frame_id.to_string(),
          observation_count: 1,
        });
      }
      AssociationResult::AmbiguousAssociation { diagnostic, .. } => {
        open_uncertainty_codes.push(diagnostic.code.clone());
      }
    }
  }

  if frame_count >= 2 && associations.is_empty() {
    negative_evidence.push(NegativeEvidence {
      code: "no_new_observation".into(),
      after_frame_id: last_frame_id.to_string(),
    });
  }

  if open_uncertainty_codes.is_empty() && negative_evidence.is_empty() {
    CoverageView::complete(entries)
  } else {
    CoverageView::incomplete(entries, "open uncertainties or negative evidence remain", open_uncertainty_codes, negative_evidence)
  }
}
