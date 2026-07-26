//! Coverage fixture evaluator.
//!
//! ## Cross-fixture layout (D4)
//!
//! ```text
//! tests/testdata/scan/
//!   coverage/coverage_stable_v0/manifest.json   ← `--fixture-dir`
//!   temporal/two_frame_v0/                        ← `manifest.frame_fixture` target
//! ```
//!
//! Scan fixtures root = `coverage_fixture_dir.parent().parent()` (requires `.../scan/coverage/<scenario>/`).
//!
//! Producer chain: fixture decode → `build_coverage_view`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use super::{ScanProducerError, load_multi_frame_fixture};
use crate::association::{FrameObservation, associate_adjacent_frames};
use crate::coverage::{CoverageView, build_coverage_view};
use crate::reader::ScanFrameBundle;

const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Deserialize)]
struct ObservationFixture {
  observation_id: String,
  label: String,
}

#[derive(Debug, Deserialize)]
struct CoverageFixture {
  #[serde(rename = "scenario")]
  _scenario: String,
  frame_fixture: String,
  observations_by_frame: Vec<Vec<ObservationFixture>>,
}

#[derive(Debug, Error)]
pub enum CoverageProducerError {
  #[error("coverage fixture manifest missing: {path}")]
  MissingManifest { path: String },
  #[error("coverage fixture manifest invalid: {0}")]
  InvalidManifest(String),
  #[error("observations_by_frame length {observation_frames} does not match bundle frame count {bundle_frames}")]
  InvalidObservationShape {
    observation_frames: usize,
    bundle_frames: usize,
  },
  #[error("frame fixture not found at resolved path (frame_fixture={frame_fixture}, resolved_path={resolved_path})")]
  InvalidFixtureLayout {
    frame_fixture: String,
    resolved_path: String,
  },
  #[error(transparent)]
  FrameProducer(ScanProducerError),
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

fn observations_from_fixture(raw: &[Vec<ObservationFixture>]) -> Vec<Vec<FrameObservation>> {
  raw
    .iter()
    .map(|frame| {
      frame
        .iter()
        .map(|obs| FrameObservation {
          observation_id: obs.observation_id.clone(),
          label: obs.label.clone(),
        })
        .collect()
    })
    .collect()
}

fn resolve_frame_fixture_dir(coverage_fixture_dir: &Path, frame_fixture: &str) -> Result<PathBuf, CoverageProducerError> {
  let Some(scan_fixtures_root) = coverage_fixture_dir.parent().and_then(|parent| parent.parent()) else {
    return Err(CoverageProducerError::InvalidFixtureLayout {
      frame_fixture: frame_fixture.to_string(),
      resolved_path: coverage_fixture_dir.display().to_string(),
    });
  };
  let frame_fixture_dir = scan_fixtures_root.join(frame_fixture);
  if !frame_fixture_dir.is_dir() {
    return Err(CoverageProducerError::InvalidFixtureLayout {
      frame_fixture: frame_fixture.to_string(),
      resolved_path: frame_fixture_dir.display().to_string(),
    });
  }
  Ok(frame_fixture_dir)
}

fn load_coverage_fixture(coverage_fixture_dir: &Path) -> Result<CoverageFixture, CoverageProducerError> {
  let manifest_path = coverage_fixture_dir.join(MANIFEST_FILE);
  if !manifest_path.is_file() {
    return Err(CoverageProducerError::MissingManifest {
      path: manifest_path.display().to_string(),
    });
  }
  let text = fs::read_to_string(&manifest_path)?;
  serde_json::from_str(&text).map_err(|error| CoverageProducerError::InvalidManifest(error.to_string()))
}

/// Build a coverage value from a hermetic fixture without creating an artifact
/// store or exposing local output paths.
pub fn build_coverage_fixture(coverage_fixture_dir: &Path) -> Result<CoverageView, CoverageProducerError> {
  let fixture = load_coverage_fixture(coverage_fixture_dir)?;
  let frame_fixture_dir = resolve_frame_fixture_dir(coverage_fixture_dir, &fixture.frame_fixture)?;

  let bundle = ScanFrameBundle {
    frames: load_multi_frame_fixture(&frame_fixture_dir)
      .map_err(CoverageProducerError::FrameProducer)?
      .into_iter()
      .map(|(frame, _)| frame)
      .collect(),
  };

  let observations_by_frame = observations_from_fixture(&fixture.observations_by_frame);
  if observations_by_frame.len() != bundle.frames.len() {
    return Err(CoverageProducerError::InvalidObservationShape {
      observation_frames: observations_by_frame.len(),
      bundle_frames: bundle.frames.len(),
    });
  }

  let associations = if bundle.frames.len() < 2 {
    Vec::new()
  } else {
    let last = bundle.frames.len() - 1;
    associate_adjacent_frames(&observations_by_frame[last - 1], &observations_by_frame[last])
  };

  Ok(build_coverage_view(&bundle, &associations))
}
