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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::association::{FrameObservation, associate_adjacent_frames};
  use crate::producer::produce_frames_from_fixture_dir;
  use crate::reader::load_scan_frames_from_dir;

  #[test]
  fn build_coverage_view_records_last_seen_frame() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir = std::env::temp_dir().join(format!("auv-scan-coverage-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let associations = associate_adjacent_frames(
      &[FrameObservation {
        observation_id: "o0".into(),
        label: "widget".into(),
      }],
      &[FrameObservation {
        observation_id: "o1".into(),
        label: "widget".into(),
      }],
    );
    let view = build_coverage_view(&bundle, &associations);
    assert_eq!(view.entries.len(), 1);
    assert_eq!(view.entries[0].last_seen_frame_id, "frame-0002");
    assert_eq!(view.entries[0].observation_count, 2);
    assert!(view.open_uncertainty_codes().is_empty());
    assert!(view.negative_evidence().is_empty());
    assert_eq!(view.status(), &CoverageStatus::Complete);
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_coverage_view_records_no_new_observation_negative_evidence() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir = std::env::temp_dir().join(format!("auv-scan-coverage-negative-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let view = build_coverage_view(&bundle, &[]);
    assert!(view.entries.is_empty());
    assert_eq!(view.negative_evidence().len(), 1);
    assert_eq!(view.negative_evidence()[0].code, "no_new_observation");
    assert!(matches!(view.status(), CoverageStatus::Incomplete { .. }));
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_coverage_view_records_ambiguous_association_uncertainty() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir = std::env::temp_dir().join(format!("auv-scan-coverage-ambiguous-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let associations = associate_adjacent_frames(
      &[
        FrameObservation {
          observation_id: "o0-a1".into(),
          label: "dup".into(),
        },
        FrameObservation {
          observation_id: "o0-a2".into(),
          label: "dup".into(),
        },
      ],
      &[FrameObservation {
        observation_id: "o1-a".into(),
        label: "dup".into(),
      }],
    );
    let view = build_coverage_view(&bundle, &associations);
    assert!(view.entries.is_empty());
    assert_eq!(view.open_uncertainty_codes(), ["ambiguous_association"]);
    assert!(matches!(view.status(), CoverageStatus::Incomplete { .. }));
    let _ = std::fs::remove_dir_all(&out_dir);
  }
}
