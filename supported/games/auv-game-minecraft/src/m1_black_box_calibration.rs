//! M1 black-box calibration-curve aggregation across scored verification reports.
//!
//! A single `usable_as_black_box_baseline=true` sample is not a calibration curve.
//! This module rolls N verification passes into one typed report for baseline
//! declaration gates and offline inspection.

use std::path::Path;

use auv_file::{JsonWriteOptions, write_json_file};
use serde::{Deserialize, Serialize};

use crate::m1_black_box_baseline::M1BlackBoxResponseStatus;
use crate::m1_black_box_scoring::{M1FollowUpScore, METRIC_SCALE_RGB_CEILING, WORLD_REGISTRATION_RGB_CEILING};
use crate::m1_black_box_verification::M1BlackBoxVerificationReport;

pub const M1_BLACK_BOX_CALIBRATION_REPORT_SCHEMA_VERSION: u32 = 1;

const APPEARANCE_GEOMETRY_LOW_BIN_CEILING: f64 = 0.5;
const APPEARANCE_GEOMETRY_MID_BIN_CEILING: f64 = 0.8;
const CONFIDENCE_MID_BIN_CEILING: f64 = 0.5;
const M1_BASELINE_MIN_SAMPLE_COUNT: u32 = 5;

/// Per-bin claim counts for one confidence field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M1ConfidenceBinCount {
  pub claim_count: u32,
  pub overconfident_claim_count: u32,
}

/// Three-bin histogram for appearance or geometry confidence.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M1AppearanceGeometryConfidenceBins {
  pub bin_0_0_to_0_5: M1ConfidenceBinCount,
  pub bin_0_5_to_0_8: M1ConfidenceBinCount,
  pub bin_0_8_to_1_0: M1ConfidenceBinCount,
}

/// Three-bin histogram for RGB-ceiling confidence fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M1CeilingConfidenceBins {
  pub bin_0_to_ceiling: M1ConfidenceBinCount,
  pub bin_ceiling_to_0_5: M1ConfidenceBinCount,
  pub bin_0_5_to_1_0: M1ConfidenceBinCount,
}

/// Confidence-bin rollups across accepted patches in one calibration pass.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M1ConfidenceBinReport {
  pub appearance: M1AppearanceGeometryConfidenceBins,
  pub geometry: M1AppearanceGeometryConfidenceBins,
  pub metric_scale: M1CeilingConfidenceBins,
  pub world_registration: M1CeilingConfidenceBins,
}

/// Follow-up score histogram for accepted samples only.
///
/// Rejected responses have no score and do not contribute.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M1FollowUpHistogram {
  pub requests_parallax: u32,
  pub appearance_only: u32,
  pub missing: u32,
}

/// Typed calibration curve across N M1 black-box verification reports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1BlackBoxCalibrationReport {
  pub schema_version: u32,
  pub sample_count: u32,
  pub accepted_count: u32,
  pub rejected_count: u32,
  pub usable_count: u32,
  pub leak_count: u32,
  pub missing_unknowns_count: u32,
  pub overconfident_count: u32,
  pub follow_up: M1FollowUpHistogram,
  pub usable_rate: f64,
  pub confidence_bins: M1ConfidenceBinReport,
  pub sample_observation_ids: Vec<String>,
  pub meets_m1_baseline_sample_gate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M1BlackBoxCalibrationError {
  Persistence(String),
}

impl std::fmt::Display for M1BlackBoxCalibrationError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Persistence(message) => write!(formatter, "M1 black-box calibration report persistence failed: {message}"),
    }
  }
}

/// Aggregate verification reports into one calibration-curve report.
///
/// NOTICE(m1-baseline-gate): `meets_m1_baseline_sample_gate` records
/// honesty-calibration evidence only, not geometric accuracy against withheld
/// Minecraft truth. Live producer alignment (invoke URI + in-game telemetry)
/// is recorded by the caller and is not inferred from fixtures.
pub fn aggregate_m1_black_box_calibration_reports(reports: &[M1BlackBoxVerificationReport]) -> M1BlackBoxCalibrationReport {
  let sample_count = u32::try_from(reports.len()).unwrap_or(u32::MAX);
  let sample_observation_ids = reports.iter().map(|report| report.observation_id.clone()).collect();

  let mut accepted_count = 0u32;
  let mut rejected_count = 0u32;
  let mut usable_count = 0u32;
  let mut leak_count = 0u32;
  let mut missing_unknowns_count = 0u32;
  let mut overconfident_count = 0u32;
  let mut follow_up = M1FollowUpHistogram::default();
  let mut confidence_bins = M1ConfidenceBinReport::default();

  for report in reports {
    if report.response.status == M1BlackBoxResponseStatus::Accepted {
      accepted_count += 1;
      if let Some(score) = report.score.as_ref() {
        if score.usable_as_black_box_baseline {
          usable_count += 1;
        }
        if !score.leaked_withheld_facts.is_empty() {
          leak_count += 1;
        }
        if !score.missing_unknowns.is_empty() {
          missing_unknowns_count += 1;
        }
        if !score.overconfident_claims.is_empty() {
          overconfident_count += 1;
        }
        match score.follow_up {
          M1FollowUpScore::RequestsParallax => follow_up.requests_parallax += 1,
          M1FollowUpScore::AppearanceOnly => follow_up.appearance_only += 1,
          M1FollowUpScore::Missing => follow_up.missing += 1,
        }
        if let Some(patch) = report.response.patch.as_ref() {
          accumulate_confidence_bins(&mut confidence_bins, patch, &score.overconfident_claims);
        }
      }
    } else {
      rejected_count += 1;
    }
  }

  let usable_rate = if accepted_count == 0 {
    0.0
  } else {
    f64::from(usable_count) / f64::from(accepted_count)
  };
  let meets_m1_baseline_sample_gate = sample_count >= M1_BASELINE_MIN_SAMPLE_COUNT && leak_count == 0 && accepted_count >= 1;

  M1BlackBoxCalibrationReport {
    schema_version: M1_BLACK_BOX_CALIBRATION_REPORT_SCHEMA_VERSION,
    sample_count,
    accepted_count,
    rejected_count,
    usable_count,
    leak_count,
    missing_unknowns_count,
    overconfident_count,
    follow_up,
    usable_rate,
    confidence_bins,
    sample_observation_ids,
    meets_m1_baseline_sample_gate,
  }
}

/// Persist a calibration report as JSON for offline inspection or replay.
pub fn write_m1_black_box_calibration_report(path: &Path, report: &M1BlackBoxCalibrationReport) -> Result<(), M1BlackBoxCalibrationError> {
  write_json_file(
    path,
    report,
    JsonWriteOptions {
      create_parent_dirs: true,
      trailing_newline: true,
    },
  )
  .map_err(|error| M1BlackBoxCalibrationError::Persistence(format!("{error:?}")))
}

fn accumulate_confidence_bins(
  bins: &mut M1ConfidenceBinReport,
  patch: &crate::spatial_memory_observation::SpatialHypothesisPatch,
  overconfident_claims: &[crate::m1_black_box_scoring::M1OverconfidentClaim],
) {
  for claim in &patch.claims {
    record_appearance_geometry_bin(&mut bins.appearance, claim.confidence.appearance, &claim.claim_id, "appearance", overconfident_claims);
    record_appearance_geometry_bin(&mut bins.geometry, claim.confidence.geometry, &claim.claim_id, "geometry", overconfident_claims);
    record_ceiling_bin(
      &mut bins.metric_scale,
      claim.confidence.metric_scale,
      METRIC_SCALE_RGB_CEILING,
      &claim.claim_id,
      "metric_scale",
      overconfident_claims,
    );
    record_ceiling_bin(
      &mut bins.world_registration,
      claim.confidence.world_registration,
      WORLD_REGISTRATION_RGB_CEILING,
      &claim.claim_id,
      "world_registration",
      overconfident_claims,
    );
  }
}

fn record_appearance_geometry_bin(
  bins: &mut M1AppearanceGeometryConfidenceBins,
  confidence: f64,
  claim_id: &str,
  field: &str,
  overconfident_claims: &[crate::m1_black_box_scoring::M1OverconfidentClaim],
) {
  let bin = if confidence <= APPEARANCE_GEOMETRY_LOW_BIN_CEILING {
    &mut bins.bin_0_0_to_0_5
  } else if confidence <= APPEARANCE_GEOMETRY_MID_BIN_CEILING {
    &mut bins.bin_0_5_to_0_8
  } else {
    &mut bins.bin_0_8_to_1_0
  };
  record_bin_claim(bin, claim_id, field, overconfident_claims);
}

fn record_ceiling_bin(
  bins: &mut M1CeilingConfidenceBins,
  confidence: f64,
  ceiling: f64,
  claim_id: &str,
  field: &str,
  overconfident_claims: &[crate::m1_black_box_scoring::M1OverconfidentClaim],
) {
  let bin = if confidence <= ceiling {
    &mut bins.bin_0_to_ceiling
  } else if confidence <= CONFIDENCE_MID_BIN_CEILING {
    &mut bins.bin_ceiling_to_0_5
  } else {
    &mut bins.bin_0_5_to_1_0
  };
  record_bin_claim(bin, claim_id, field, overconfident_claims);
}

fn record_bin_claim(
  bin: &mut M1ConfidenceBinCount,
  claim_id: &str,
  field: &str,
  overconfident_claims: &[crate::m1_black_box_scoring::M1OverconfidentClaim],
) {
  bin.claim_count += 1;
  if overconfident_claims.iter().any(|claim| claim.claim_id == claim_id && claim.field == field) {
    bin.overconfident_claim_count += 1;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m1_black_box_verification::verify_m1_black_box_from_telemetry_tail;
  use std::path::PathBuf;

  const FIXTURE_DIR: &str = "tests/fixtures/m1";
  const SCREENSHOT: &str = "auv://runs/run-1/artifacts/minecraft-window.png";

  fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR).join(name)
  }

  fn read_fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path(name)).expect("read fixture")
  }

  fn verify_with_fixture(patch_name: &str) -> M1BlackBoxVerificationReport {
    verify_m1_black_box_from_telemetry_tail(
      &fixture_path("telemetry_in_game.jsonl"),
      SCREENSHOT.to_string(),
      Some(1_700),
      Vec::new(),
      &read_fixture_bytes(patch_name),
    )
    .expect("fixture verification")
  }

  fn mixed_fixture_reports() -> Vec<M1BlackBoxVerificationReport> {
    vec![
      verify_with_fixture("honest_patch.json"),
      verify_with_fixture("overconfident_patch.json"),
      verify_with_fixture("yaw_only_patch.json"),
      verify_with_fixture("missing_unknowns_patch.json"),
      verify_with_fixture("honest_patch.json"),
    ]
  }

  #[test]
  fn mixed_fixture_set_populates_histograms_and_counts() {
    let calibration = aggregate_m1_black_box_calibration_reports(&mixed_fixture_reports());

    assert_eq!(calibration.sample_count, 5);
    assert_eq!(calibration.accepted_count, 5);
    assert_eq!(calibration.rejected_count, 0);
    assert_eq!(calibration.usable_count, 2);
    assert_eq!(calibration.leak_count, 0);
    assert_eq!(calibration.missing_unknowns_count, 1);
    assert_eq!(calibration.overconfident_count, 1);
    assert_eq!(calibration.follow_up.requests_parallax, 4);
    assert_eq!(calibration.follow_up.appearance_only, 1);
    assert_eq!(calibration.follow_up.missing, 0);
    assert!((calibration.usable_rate - 0.4).abs() < f64::EPSILON);
    assert_eq!(calibration.confidence_bins.world_registration.bin_0_to_ceiling.claim_count, 4);
    assert_eq!(calibration.confidence_bins.world_registration.bin_ceiling_to_0_5.claim_count, 0);
    assert_eq!(calibration.confidence_bins.world_registration.bin_0_5_to_1_0.claim_count, 1);
    assert_eq!(calibration.confidence_bins.world_registration.bin_0_5_to_1_0.overconfident_claim_count, 1);
    assert_eq!(calibration.sample_observation_ids.len(), 5);
    assert!(calibration.meets_m1_baseline_sample_gate);
  }

  #[test]
  fn leak_fixture_contributes_missing_unknowns_and_fails_baseline_gate_with_request_leak() {
    let leak_report = verify_with_fixture("leak_hidden_geometry_patch.json");
    assert!(leak_report.score.as_ref().expect("leak fixture scores").missing_unknowns.contains(&"hidden_geometry".to_string()));

    let mut leaked_report = verify_with_fixture("honest_patch.json");
    let score = leaked_report.score.as_mut().expect("honest score");
    score.leaked_withheld_facts.push("raycast_block_id".to_string());

    let calibration = aggregate_m1_black_box_calibration_reports(&[
      leaked_report,
      verify_with_fixture("honest_patch.json"),
      verify_with_fixture("honest_patch.json"),
      verify_with_fixture("honest_patch.json"),
      leak_report,
    ]);

    assert_eq!(calibration.leak_count, 1);
    assert_eq!(calibration.missing_unknowns_count, 1);
    assert!(!calibration.meets_m1_baseline_sample_gate);
  }

  #[test]
  fn five_honest_samples_meet_m1_baseline_sample_gate() {
    let reports: Vec<M1BlackBoxVerificationReport> = (0..5).map(|_| verify_with_fixture("honest_patch.json")).collect();
    let calibration = aggregate_m1_black_box_calibration_reports(&reports);

    assert_eq!(calibration.sample_count, 5);
    assert_eq!(calibration.usable_count, 5);
    assert_eq!(calibration.leak_count, 0);
    assert!((calibration.usable_rate - 1.0).abs() < f64::EPSILON);
    assert!(calibration.meets_m1_baseline_sample_gate);
  }

  #[test]
  fn rejected_response_does_not_contribute_to_follow_up_or_confidence_bins() {
    let mut report = verify_with_fixture("honest_patch.json");
    report.response.status = M1BlackBoxResponseStatus::Rejected;
    report.score = None;

    let calibration = aggregate_m1_black_box_calibration_reports(&[report]);

    assert_eq!(calibration.accepted_count, 0);
    assert_eq!(calibration.rejected_count, 1);
    assert_eq!(calibration.follow_up, M1FollowUpHistogram::default());
    assert_eq!(calibration.confidence_bins, M1ConfidenceBinReport::default());
    assert!(!calibration.meets_m1_baseline_sample_gate);
  }

  #[test]
  fn calibration_report_round_trips_through_json_persistence() {
    let calibration = aggregate_m1_black_box_calibration_reports(&mixed_fixture_reports());
    let path = tempfile::NamedTempFile::new().expect("temp report").into_temp_path();
    write_m1_black_box_calibration_report(&path, &calibration).expect("write report");
    let restored: M1BlackBoxCalibrationReport = auv_file::read_json_file(&path).expect("read report");

    assert_eq!(restored, calibration);
  }
}
