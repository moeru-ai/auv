//! Versioned serialized form of [`CoverageView`](crate::CoverageView).

#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::io::Write;
#[cfg(test)]
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
#[cfg(test)]
use thiserror::Error;

use crate::coverage::CoverageView;

#[cfg(test)]
pub const SCAN_COVERAGE_ARTIFACT_FILE_NAME: &str = "scan-coverage.json";

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

#[cfg(test)]
#[derive(Debug, Error)]
pub enum CoverageArtifactError {
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

#[cfg(test)]
pub(crate) fn read_coverage_artifact_from_scan_dir(dir: &Path) -> Result<ScanCoverageArtifact, CoverageArtifactError> {
  read_coverage_artifact(&dir.join(SCAN_COVERAGE_ARTIFACT_FILE_NAME))
}

#[cfg(test)]
pub(crate) fn write_coverage_artifact(dir: &Path, coverage: &ScanCoverageArtifact) -> Result<PathBuf, CoverageArtifactError> {
  fs::create_dir_all(dir)?;
  let path = dir.join(SCAN_COVERAGE_ARTIFACT_FILE_NAME);
  let json = serde_json::to_string_pretty(coverage)?;
  let mut file = fs::File::create(&path)?;
  file.write_all(json.as_bytes())?;
  file.write_all(b"\n")?;
  Ok(path)
}

#[cfg(test)]
pub(crate) fn read_coverage_artifact(path: &Path) -> Result<ScanCoverageArtifact, CoverageArtifactError> {
  let bytes = fs::read(path)?;
  serde_json::from_slice(&bytes).map_err(CoverageArtifactError::from)
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
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
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
  fn coverage_artifact_roundtrip_preserves_the_canonical_value() {
    for view in [
      stable_coverage_view(),
      no_observation_coverage_view(),
      ambiguous_coverage_view(),
    ] {
      let bytes = serde_json::to_vec(&ScanCoverageArtifact::new(view.clone())).expect("serialize");
      let roundtrip = serde_json::from_slice::<ScanCoverageArtifact>(&bytes).expect("deserialize").into_coverage();
      assert_eq!(roundtrip, view);
    }
  }

  #[test]
  fn read_coverage_artifact_from_scan_dir_roundtrip() {
    let artifact = ScanCoverageArtifact::new(stable_coverage_view());
    let seq = ARTIFACT_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = env::temp_dir().join(format!("auv-scan-coverage-scan-dir-{}-{}", process::id(), seq));
    let _ = fs::remove_dir_all(&out_dir);
    write_coverage_artifact(&out_dir, &artifact).expect("write");
    let read_back = read_coverage_artifact_from_scan_dir(&out_dir).expect("read dir");
    assert_eq!(read_back, artifact);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn write_read_coverage_artifact_roundtrip() {
    let artifact = ScanCoverageArtifact::new(stable_coverage_view());
    let seq = ARTIFACT_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let out_dir = env::temp_dir().join(format!("auv-scan-coverage-roundtrip-{}-{}", process::id(), seq));
    let _ = fs::remove_dir_all(&out_dir);
    let written = write_coverage_artifact(&out_dir, &artifact).expect("write");
    let read_back = read_coverage_artifact(&written).expect("read");
    assert_eq!(read_back, artifact);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn read_coverage_artifact_rejects_unknown_schema_version() {
    let path = env::temp_dir().join(format!("auv-scan-coverage-bad-schema-{}", process::id()));
    fs::write(&path, r#"{"schema":"auv.scan.coverage.v99","coverage":{"entries":[],"status":"complete"}}"#).expect("write");
    let err = read_coverage_artifact(&path).expect_err("schema");
    assert!(matches!(err, CoverageArtifactError::Json(_)));
    let _ = fs::remove_file(&path);
  }

  #[test]
  fn read_coverage_artifact_rejects_missing_schema_version() {
    let path = env::temp_dir().join(format!("auv-scan-coverage-missing-schema-{}", process::id()));
    fs::write(&path, r#"{"coverage":{"entries":[],"status":"complete"}}"#).expect("write");
    let err = read_coverage_artifact(&path).expect_err("missing");
    assert!(matches!(err, CoverageArtifactError::Json(_)));
    let _ = fs::remove_file(&path);
  }

  #[test]
  fn coverage_artifact_matches_golden_stable() {
    let artifact = ScanCoverageArtifact::new(stable_coverage_view());
    let golden = read_coverage_artifact(&golden_path("coverage_stable_v0")).expect("golden");
    assert_eq!(artifact, golden);
  }

  #[test]
  fn coverage_artifact_matches_golden_no_observation() {
    let artifact = ScanCoverageArtifact::new(no_observation_coverage_view());
    let golden = read_coverage_artifact(&golden_path("coverage_no_observation_v0")).expect("golden");
    assert_eq!(artifact, golden);
  }

  #[test]
  fn coverage_artifact_matches_golden_ambiguous() {
    let artifact = ScanCoverageArtifact::new(ambiguous_coverage_view());
    let golden = read_coverage_artifact(&golden_path("coverage_ambiguous_v0")).expect("golden");
    assert_eq!(artifact, golden);
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
      let artifact = ScanCoverageArtifact::new(view);
      let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/coverage").join(scenario).join("golden");
      fs::create_dir_all(&dir).expect("mkdir");
      write_coverage_artifact(&dir, &artifact).expect("write golden");
    }
  }
}
