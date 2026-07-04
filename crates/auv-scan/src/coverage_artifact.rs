//! Bounded crate-local coverage ledger wire (`scan-coverage-v0`).
//!
//! NOTICE(s8a-artifact-boundary): directory-level artifact beside scan-frame-*.json;
//! not run-level; not runtime-staged by S8a; not scene_state durable product.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::coverage::{CompletenessClaim, CoverageEntry, CoverageView, NegativeEvidence};

pub const SCAN_COVERAGE_SCHEMA_VERSION: &str = "scan-coverage-v0";
pub const SCAN_COVERAGE_ARTIFACT_FILE_NAME: &str = "scan-coverage.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanCoverageWire {
  pub schema_version: String,
  pub entries: Vec<CoverageEntryWire>,
  pub open_uncertainty_codes: Vec<String>,
  pub negative_evidence: Vec<NegativeEvidenceWire>,
  pub completeness: CompletenessWire,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageEntryWire {
  pub track_id: String,
  pub last_seen_frame_id: String,
  pub observation_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NegativeEvidenceWire {
  pub code: String,
  pub after_frame_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CompletenessWire {
  Complete,
  Incomplete { reason: String },
}

#[derive(Debug, Error)]
pub enum CoverageArtifactError {
  #[error("schema_version mismatch: expected {SCAN_COVERAGE_SCHEMA_VERSION}, found {found}")]
  SchemaMismatch { found: String },
  #[error("missing required field: {0}")]
  MissingField(&'static str),
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

/// Project an in-memory [`CoverageView`] into durable wire. Does not recompute coverage.
pub fn coverage_view_to_wire(view: &CoverageView) -> ScanCoverageWire {
  ScanCoverageWire {
    schema_version: SCAN_COVERAGE_SCHEMA_VERSION.to_string(),
    entries: view
      .entries
      .iter()
      .map(|entry| CoverageEntryWire {
        track_id: entry.track_id.clone(),
        last_seen_frame_id: entry.last_seen_frame_id.clone(),
        observation_count: entry.observation_count,
      })
      .collect(),
    open_uncertainty_codes: view.open_uncertainty_codes.clone(),
    negative_evidence: view
      .negative_evidence
      .iter()
      .map(|evidence| NegativeEvidenceWire {
        code: evidence.code.clone(),
        after_frame_id: evidence.after_frame_id.clone(),
      })
      .collect(),
    completeness: completeness_to_wire(&view.completeness),
  }
}

fn completeness_to_wire(claim: &CompletenessClaim) -> CompletenessWire {
  match claim {
    CompletenessClaim::Complete => CompletenessWire::Complete,
    CompletenessClaim::Incomplete { reason } => CompletenessWire::Incomplete {
      reason: reason.clone(),
    },
  }
}

fn completeness_from_wire(wire: &CompletenessWire) -> CompletenessClaim {
  match wire {
    CompletenessWire::Complete => CompletenessClaim::Complete,
    CompletenessWire::Incomplete { reason } => CompletenessClaim::Incomplete {
      reason: reason.clone(),
    },
  }
}

/// Hydrate an in-memory [`CoverageView`] from durable wire. Does not recompute coverage.
pub(crate) fn coverage_wire_to_view(wire: &ScanCoverageWire) -> CoverageView {
  CoverageView {
    entries: wire
      .entries
      .iter()
      .map(|entry| CoverageEntry {
        track_id: entry.track_id.clone(),
        last_seen_frame_id: entry.last_seen_frame_id.clone(),
        observation_count: entry.observation_count,
      })
      .collect(),
    open_uncertainty_codes: wire.open_uncertainty_codes.clone(),
    negative_evidence: wire
      .negative_evidence
      .iter()
      .map(|evidence| NegativeEvidence {
        code: evidence.code.clone(),
        after_frame_id: evidence.after_frame_id.clone(),
      })
      .collect(),
    completeness: completeness_from_wire(&wire.completeness),
  }
}

/// Read `scan-coverage.json` from a scan frame directory.
#[cfg(test)]
pub(crate) fn read_coverage_artifact_from_scan_dir(
  dir: &Path,
) -> Result<ScanCoverageWire, CoverageArtifactError> {
  read_coverage_artifact(&dir.join(SCAN_COVERAGE_ARTIFACT_FILE_NAME))
}

pub fn write_coverage_artifact(
  dir: &Path,
  coverage: &ScanCoverageWire,
) -> Result<PathBuf, CoverageArtifactError> {
  if coverage.schema_version != SCAN_COVERAGE_SCHEMA_VERSION {
    return Err(CoverageArtifactError::SchemaMismatch {
      found: coverage.schema_version.clone(),
    });
  }
  fs::create_dir_all(dir)?;
  let path = dir.join(SCAN_COVERAGE_ARTIFACT_FILE_NAME);
  let json = serde_json::to_string_pretty(coverage)?;
  let mut file = fs::File::create(&path)?;
  file.write_all(json.as_bytes())?;
  file.write_all(b"\n")?;
  Ok(path)
}

pub fn read_coverage_artifact(path: &Path) -> Result<ScanCoverageWire, CoverageArtifactError> {
  let bytes = fs::read(path)?;
  let value: serde_json::Value = serde_json::from_slice(&bytes)?;
  let Some(schema_version) = value.get("schema_version") else {
    return Err(CoverageArtifactError::MissingField("schema_version"));
  };
  let Some(schema_version) = schema_version.as_str() else {
    return Err(CoverageArtifactError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  };
  if schema_version != SCAN_COVERAGE_SCHEMA_VERSION {
    return Err(CoverageArtifactError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  }
  serde_json::from_value(value).map_err(CoverageArtifactError::from)
}

#[cfg(test)]
mod tests {
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::process;
  use std::sync::atomic::{AtomicU64, Ordering};

  use crate::association::{FrameObservation, associate_adjacent_frames};
  use crate::coverage::build_coverage_view;
  use crate::producer::produce_frames_from_fixture_dir;
  use crate::reader::load_scan_frames_from_dir;

  use super::*;

  static BUNDLE_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

  fn two_frame_bundle() -> crate::reader::ScanFrameBundle {
    let fixture_dir =
      PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
    let seq = BUNDLE_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = env::temp_dir().join(format!("auv-scan-coverage-wire-{}-{}", process::id(), seq));
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    load_scan_frames_from_dir(&out_dir).expect("load")
  }

  fn stable_coverage_view() -> CoverageView {
    let bundle = two_frame_bundle();
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
    build_coverage_view(&bundle, &associations)
  }

  fn no_observation_coverage_view() -> CoverageView {
    let bundle = two_frame_bundle();
    build_coverage_view(&bundle, &[])
  }

  fn ambiguous_coverage_view() -> CoverageView {
    let bundle = two_frame_bundle();
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
    build_coverage_view(&bundle, &associations)
  }

  fn golden_path(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/coverage")
      .join(scenario)
      .join("golden")
      .join(SCAN_COVERAGE_ARTIFACT_FILE_NAME)
  }

  static ARTIFACT_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

  #[test]
  fn coverage_wire_to_view_roundtrip() {
    for view in [
      stable_coverage_view(),
      no_observation_coverage_view(),
      ambiguous_coverage_view(),
    ] {
      let wire = coverage_view_to_wire(&view);
      let roundtrip = coverage_wire_to_view(&wire);
      assert_eq!(roundtrip, view);
    }
  }

  #[test]
  fn read_coverage_artifact_from_scan_dir_roundtrip() {
    let wire = coverage_view_to_wire(&stable_coverage_view());
    let seq = ARTIFACT_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = env::temp_dir().join(format!(
      "auv-scan-coverage-scan-dir-{}-{}",
      process::id(),
      seq
    ));
    let _ = fs::remove_dir_all(&out_dir);
    write_coverage_artifact(&out_dir, &wire).expect("write");
    let read_back = read_coverage_artifact_from_scan_dir(&out_dir).expect("read dir");
    assert_eq!(read_back, wire);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn write_read_coverage_artifact_roundtrip() {
    let wire = coverage_view_to_wire(&stable_coverage_view());
    let seq = ARTIFACT_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = env::temp_dir().join(format!(
      "auv-scan-coverage-roundtrip-{}-{}",
      process::id(),
      seq
    ));
    let _ = fs::remove_dir_all(&out_dir);
    let written = write_coverage_artifact(&out_dir, &wire).expect("write");
    let read_back = read_coverage_artifact(&written).expect("read");
    assert_eq!(read_back, wire);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn read_coverage_artifact_rejects_unknown_schema_version() {
    let path = env::temp_dir().join(format!("auv-scan-coverage-bad-schema-{}", process::id()));
    fs::write(
      &path,
      r#"{"schema_version":"scan-coverage-v99","entries":[],"open_uncertainty_codes":[],"negative_evidence":[],"completeness":{"status":"complete"}}"#,
    )
    .expect("write");
    let err = read_coverage_artifact(&path).expect_err("schema");
    assert!(matches!(err, CoverageArtifactError::SchemaMismatch { .. }));
    let _ = fs::remove_file(&path);
  }

  #[test]
  fn read_coverage_artifact_rejects_missing_schema_version() {
    let path = env::temp_dir().join(format!(
      "auv-scan-coverage-missing-schema-{}",
      process::id()
    ));
    fs::write(
      &path,
      r#"{"entries":[],"open_uncertainty_codes":[],"negative_evidence":[],"completeness":{"status":"complete"}}"#,
    )
    .expect("write");
    let err = read_coverage_artifact(&path).expect_err("missing");
    assert!(matches!(
      err,
      CoverageArtifactError::MissingField("schema_version")
    ));
    let _ = fs::remove_file(&path);
  }

  #[test]
  fn coverage_view_to_wire_matches_golden_stable() {
    let wire = coverage_view_to_wire(&stable_coverage_view());
    let golden = read_coverage_artifact(&golden_path("coverage_stable_v0")).expect("golden");
    assert_eq!(wire, golden);
  }

  #[test]
  fn coverage_view_to_wire_matches_golden_no_observation() {
    let wire = coverage_view_to_wire(&no_observation_coverage_view());
    let golden =
      read_coverage_artifact(&golden_path("coverage_no_observation_v0")).expect("golden");
    assert_eq!(wire, golden);
  }

  #[test]
  fn coverage_view_to_wire_matches_golden_ambiguous() {
    let wire = coverage_view_to_wire(&ambiguous_coverage_view());
    let golden = read_coverage_artifact(&golden_path("coverage_ambiguous_v0")).expect("golden");
    assert_eq!(wire, golden);
  }

  /// Regenerates committed golden fixtures from the fixed pipeline. Run with `--ignored`.
  #[test]
  #[ignore = "golden regeneration only"]
  fn coverage_golden_regenerate_fixtures() {
    let scenarios = [
      ("coverage_stable_v0", stable_coverage_view()),
      ("coverage_no_observation_v0", no_observation_coverage_view()),
      ("coverage_ambiguous_v0", ambiguous_coverage_view()),
    ];
    for (scenario, view) in scenarios {
      let wire = coverage_view_to_wire(&view);
      let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/scan/coverage")
        .join(scenario)
        .join("golden");
      fs::create_dir_all(&dir).expect("mkdir");
      write_coverage_artifact(&dir, &wire).expect("write golden");
    }
  }
}
