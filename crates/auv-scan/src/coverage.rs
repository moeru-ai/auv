//! Optional coverage analysis view derived from frames and association (no durable wire v0).

use crate::association::AssociationResult;
use crate::reader::ScanFrameBundle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageEntry {
  pub track_id: String,
  pub last_seen_frame_id: String,
  pub observation_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegativeEvidence {
  pub code: String,
  pub after_frame_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletenessClaim {
  Complete,
  Incomplete { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageView {
  pub entries: Vec<CoverageEntry>,
  pub open_uncertainty_codes: Vec<String>,
  pub negative_evidence: Vec<NegativeEvidence>,
  pub completeness: CompletenessClaim,
}

/// Build an in-memory coverage view from a frame bundle and association results.
pub fn build_coverage_view(
  bundle: &ScanFrameBundle,
  associations: &[AssociationResult],
) -> CoverageView {
  let mut entries = Vec::new();
  let mut open_uncertainty_codes = Vec::new();
  let mut negative_evidence = Vec::new();
  let last_frame_id = bundle
    .frames
    .last()
    .map(|f| f.frame_id.as_str())
    .unwrap_or_default();

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

  if bundle.frames.len() >= 2 && associations.is_empty() {
    negative_evidence.push(NegativeEvidence {
      code: "no_new_observation".into(),
      after_frame_id: last_frame_id.to_string(),
    });
  }

  let completeness = if open_uncertainty_codes.is_empty() && negative_evidence.is_empty() {
    CompletenessClaim::Complete
  } else {
    CompletenessClaim::Incomplete {
      reason: "open uncertainties or negative evidence remain".into(),
    }
  };

  CoverageView {
    entries,
    open_uncertainty_codes,
    negative_evidence,
    completeness,
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
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-coverage-view-{}", std::process::id()));
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
    assert!(view.open_uncertainty_codes.is_empty());
    assert!(view.negative_evidence.is_empty());
    assert_eq!(view.completeness, CompletenessClaim::Complete);
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_coverage_view_records_no_new_observation_negative_evidence() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir =
      std::env::temp_dir().join(format!("auv-scan-coverage-negative-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let view = build_coverage_view(&bundle, &[]);
    assert!(view.entries.is_empty());
    assert_eq!(view.negative_evidence.len(), 1);
    assert_eq!(view.negative_evidence[0].code, "no_new_observation");
    assert!(matches!(
      view.completeness,
      CompletenessClaim::Incomplete { .. }
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_coverage_view_records_ambiguous_association_uncertainty() {
    let fixture_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-coverage-ambiguous-{}",
      std::process::id()
    ));
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
    assert_eq!(view.open_uncertainty_codes, vec!["ambiguous_association"]);
    assert!(matches!(
      view.completeness,
      CompletenessClaim::Incomplete { .. }
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
  }
}
