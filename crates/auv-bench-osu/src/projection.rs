// Source: ported from supported/games/auv-game-osu/src/projection.rs
//
// NOTICE(auv-bench-osu-spike):
// auv-bench-osu is a standalone spike carrier for actuator fidelity (Phase 0)
// and improv plan synthesis (S1). Post-S2, if this player capability graduates,
// it folds back into `supported/games/auv-game-osu` rather than growing a
// second long-term maintained beatmap/gameplay codebase in the workspace.

use serde::{Deserialize, Serialize};

pub const PLAYFIELD_WIDTH: f64 = 512.0;
pub const PLAYFIELD_HEIGHT: f64 = 384.0;
pub const VIRTUAL_CANVAS_WIDTH: f64 = 640.0;
pub const VIRTUAL_CANVAS_HEIGHT: f64 = 480.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionDerivationMethod {
  LayoutRule,
  EmpiricalCalibration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionStrategy {
  /// Candidate A (Old lane): scale = min(W/512, H/384), centered directly
  CandidateADirect512x384,
  /// Candidate B (Design review claim): 640x480 virtual canvas rule — playfield height = 0.8 x window height
  CandidateBVirtualCanvas640x480,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionBounds {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayfieldProjection {
  pub scale_x: f64,
  pub scale_y: f64,
  pub offset_x: f64,
  pub offset_y: f64,
  pub match_radius_px: f32,
  pub strategy: ProjectionStrategy,
  pub derivation_method: ProjectionDerivationMethod,
}

impl PlayfieldProjection {
  /// Computes playfield projection using the verified winner Candidate B (640x480 virtual canvas rule, 0.8x height).
  pub fn for_capture(capture_width: f64, capture_height: f64, circle_size: f32) -> Result<Self, String> {
    Self::for_capture_with_strategy(capture_width, capture_height, circle_size, ProjectionStrategy::CandidateBVirtualCanvas640x480)
  }

  /// Computes playfield projection from window / capture dimensions and circle size using a selectable strategy.
  pub fn for_capture_with_strategy(
    capture_width: f64,
    capture_height: f64,
    circle_size: f32,
    strategy: ProjectionStrategy,
  ) -> Result<Self, String> {
    if !(capture_width.is_finite() && capture_height.is_finite()) || capture_width <= 0.0 || capture_height <= 0.0 {
      return Err(format!("capture size must have positive finite size, got {}x{}", capture_width, capture_height));
    }

    match strategy {
      ProjectionStrategy::CandidateADirect512x384 => {
        let scale = f64::min(capture_width / PLAYFIELD_WIDTH, capture_height / PLAYFIELD_HEIGHT);
        if !scale.is_finite() || scale <= 0.0 {
          return Err(format!("failed to derive finite playfield scale from capture {}x{}", capture_width, capture_height));
        }

        let playfield_width = PLAYFIELD_WIDTH * scale;
        let playfield_height = PLAYFIELD_HEIGHT * scale;
        let offset_x = (capture_width - playfield_width) / 2.0;
        let offset_y = (capture_height - playfield_height) / 2.0;
        let circle_radius_px = circle_radius_playfield(circle_size) * scale;

        positive_projection_f32("playfield projection", "scale", scale)?;
        let match_radius_px = positive_projection_f32("playfield projection", "match radius", circle_radius_px)?;

        Ok(Self {
          scale_x: scale,
          scale_y: scale,
          offset_x,
          offset_y,
          match_radius_px,
          strategy,
          derivation_method: ProjectionDerivationMethod::LayoutRule,
        })
      }
      ProjectionStrategy::CandidateBVirtualCanvas640x480 => {
        let canvas_scale = f64::min(capture_width / VIRTUAL_CANVAS_WIDTH, capture_height / VIRTUAL_CANVAS_HEIGHT);
        if !canvas_scale.is_finite() || canvas_scale <= 0.0 {
          return Err(format!("failed to derive finite canvas scale from capture {}x{}", capture_width, capture_height));
        }

        let canvas_width = VIRTUAL_CANVAS_WIDTH * canvas_scale;
        let canvas_height = VIRTUAL_CANVAS_HEIGHT * canvas_scale;
        let canvas_offset_x = (capture_width - canvas_width) / 2.0;
        let canvas_offset_y = (capture_height - canvas_height) / 2.0;

        // In 640x480 canvas, playfield 512x384 has (640-512)/2 = 64px horizontal margin and (480-384)/2 = 48px vertical margin
        let offset_x = canvas_offset_x + ((VIRTUAL_CANVAS_WIDTH - PLAYFIELD_WIDTH) / 2.0) * canvas_scale;
        let offset_y = canvas_offset_y + ((VIRTUAL_CANVAS_HEIGHT - PLAYFIELD_HEIGHT) / 2.0) * canvas_scale;
        let circle_radius_px = circle_radius_playfield(circle_size) * canvas_scale;

        positive_projection_f32("playfield projection", "scale", canvas_scale)?;
        let match_radius_px = positive_projection_f32("playfield projection", "match radius", circle_radius_px)?;

        Ok(Self {
          scale_x: canvas_scale,
          scale_y: canvas_scale,
          offset_x,
          offset_y,
          match_radius_px,
          strategy,
          derivation_method: ProjectionDerivationMethod::LayoutRule,
        })
      }
    }
  }

  /// Maps playfield coordinate (osu! pixels [0..512, 0..384]) to screen/window pixels.
  pub fn to_window_point(&self, x: f64, y: f64) -> (f64, f64) {
    (x * self.scale_x + self.offset_x, y * self.scale_y + self.offset_y)
  }

  /// Inverse mapping: window/screen pixel to playfield coordinate.
  pub fn to_playfield_point(&self, wx: f64, wy: f64) -> (f64, f64) {
    ((wx - self.offset_x) / self.scale_x, (wy - self.offset_y) / self.scale_y)
  }
}

use crate::model::EvidenceLevel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStatus {
  /// Pure geometric derivation from layout rules without empirical screen sampling
  LayoutDerived,
  /// Empirically verified against external screen pixel coordinates
  Calibrated,
  /// Uncalibrated or missing required empirical measurement data
  Uncalibrated,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationPoint {
  pub object_index: usize,
  pub playfield_pos: (f64, f64),
  pub projected_screen_pos: (f64, f64),
  pub measured_screen_pos: (f64, f64),
  pub pixel_error: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionCalibrationReport {
  pub strategy: ProjectionStrategy,
  pub derivation_method: ProjectionDerivationMethod,
  pub calibration_status: CalibrationStatus,
  pub evidence_level: EvidenceLevel,
  pub capture_dimensions: (f64, f64),
  pub scale: f64,
  pub offsets: (f64, f64),
  pub points: Vec<CalibrationPoint>,
  pub max_error_px: f64,
  pub mean_error_px: f64,
  pub passed: bool,
  pub disclosure: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DualCandidateGateReport {
  pub candidate_a: ProjectionCalibrationReport,
  pub candidate_b: ProjectionCalibrationReport,
  pub gate_max_allowed_error_px: f64,
  pub winner: Option<ProjectionStrategy>,
  pub falsified: Option<ProjectionStrategy>,
  pub gate_passed: bool,
  pub evidence_level: EvidenceLevel,
  pub disclosure: String,
}

pub type EmpiricalSample = (usize, (f64, f64), (f64, f64));

/// Evaluates empirical calibration against known object pixel coordinates from actual capture frames.
pub fn evaluate_empirical_calibration(
  projection: &PlayfieldProjection,
  capture_width: f64,
  capture_height: f64,
  samples: &[EmpiricalSample], // (object_index, playfield_pos, measured_screen_pos)
  max_allowed_error_px: f64,
) -> ProjectionCalibrationReport {
  let mut points = Vec::with_capacity(samples.len());
  let mut total_error = 0.0;
  let mut max_error = 0.0f64;

  for &(idx, pf_pos, measured_pos) in samples {
    let proj_pos = projection.to_window_point(pf_pos.0, pf_pos.1);
    let dx = proj_pos.0 - measured_pos.0;
    let dy = proj_pos.1 - measured_pos.1;
    let err = dx.hypot(dy);

    total_error += err;
    if err > max_error {
      max_error = err;
    }

    points.push(CalibrationPoint {
      object_index: idx,
      playfield_pos: pf_pos,
      projected_screen_pos: proj_pos,
      measured_screen_pos: measured_pos,
      pixel_error: err,
    });
  }

  let mean_error = if !samples.is_empty() {
    total_error / (samples.len() as f64)
  } else {
    0.0
  };

  let passed = !samples.is_empty() && max_error <= max_allowed_error_px;

  let (calibration_status, evidence_level, disclosure) = if samples.is_empty() {
    (
      CalibrationStatus::Uncalibrated,
      EvidenceLevel::LayoutDerived,
      "No empirical samples provided; projection is uncalibrated against real screen pixels.".to_string(),
    )
  } else if passed {
    (
      CalibrationStatus::Calibrated,
      EvidenceLevel::EmpiricallyMeasured,
      "Empirically calibrated against external screen measurement points.".to_string(),
    )
  } else {
    (
      CalibrationStatus::Uncalibrated,
      EvidenceLevel::EmpiricallyMeasured,
      "Calibration failed: maximum pixel discrepancy exceeded threshold.".to_string(),
    )
  };

  ProjectionCalibrationReport {
    strategy: projection.strategy,
    derivation_method: ProjectionDerivationMethod::EmpiricalCalibration,
    calibration_status,
    evidence_level,
    capture_dimensions: (capture_width, capture_height),
    scale: projection.scale_x,
    offsets: (projection.offset_x, projection.offset_y),
    points,
    max_error_px: max_error,
    mean_error_px: mean_error,
    passed,
    disclosure,
  }
}

/// Evaluates a pure theoretical layout self-check without empirical screen sampling.
pub fn evaluate_layout_self_check(projection: &PlayfieldProjection, capture_width: f64, capture_height: f64) -> ProjectionCalibrationReport {
  ProjectionCalibrationReport {
    strategy: projection.strategy,
    derivation_method: ProjectionDerivationMethod::LayoutRule,
    calibration_status: CalibrationStatus::LayoutDerived,
    evidence_level: EvidenceLevel::LayoutDerived,
    capture_dimensions: (capture_width, capture_height),
    scale: projection.scale_x,
    offsets: (projection.offset_x, projection.offset_y),
    points: Vec::new(),
    max_error_px: 0.0,
    mean_error_px: 0.0,
    passed: true,
    disclosure: "Theoretical layout self-consistency check only; uncalibrated against real screen pixels.".to_string(),
  }
}

/// Runs the projection gate comparing Candidate A and Candidate B against layout/empirical measurements.
pub fn run_projection_dual_candidate_gate(
  capture_width: f64,
  capture_height: f64,
  circle_size: f32,
  samples: &[EmpiricalSample],
  gate_error_threshold_px: f64,
) -> Result<DualCandidateGateReport, String> {
  let proj_a =
    PlayfieldProjection::for_capture_with_strategy(capture_width, capture_height, circle_size, ProjectionStrategy::CandidateADirect512x384)?;
  let proj_b = PlayfieldProjection::for_capture_with_strategy(
    capture_width,
    capture_height,
    circle_size,
    ProjectionStrategy::CandidateBVirtualCanvas640x480,
  )?;

  let calib_a = evaluate_empirical_calibration(&proj_a, capture_width, capture_height, samples, gate_error_threshold_px);
  let calib_b = evaluate_empirical_calibration(&proj_b, capture_width, capture_height, samples, gate_error_threshold_px);

  let (winner, falsified, gate_passed) = if calib_a.passed && !calib_b.passed {
    (Some(ProjectionStrategy::CandidateADirect512x384), Some(ProjectionStrategy::CandidateBVirtualCanvas640x480), true)
  } else if calib_b.passed && !calib_a.passed {
    (Some(ProjectionStrategy::CandidateBVirtualCanvas640x480), Some(ProjectionStrategy::CandidateADirect512x384), true)
  } else if calib_a.passed && calib_b.passed {
    if calib_a.max_error_px <= calib_b.max_error_px {
      (Some(ProjectionStrategy::CandidateADirect512x384), Some(ProjectionStrategy::CandidateBVirtualCanvas640x480), true)
    } else {
      (Some(ProjectionStrategy::CandidateBVirtualCanvas640x480), Some(ProjectionStrategy::CandidateADirect512x384), true)
    }
  } else {
    (None, None, false)
  };

  Ok(DualCandidateGateReport {
    candidate_a: calib_a,
    candidate_b: calib_b,
    gate_max_allowed_error_px: gate_error_threshold_px,
    winner,
    falsified,
    gate_passed,
    evidence_level: EvidenceLevel::LayoutDerived,
    disclosure: "Layout hypothesis competition between Candidate A and Candidate B. Uncalibrated against real screen pixels.".to_string(),
  })
}

fn finite_projection_f32(subject: &str, field: &str, value: f64) -> Result<f32, String> {
  if !value.is_finite() {
    return Err(format!("{subject} {field} must be finite"));
  }
  let converted = value as f32;
  if !converted.is_finite() {
    return Err(format!("{subject} {field} must be representable as a finite f32"));
  }
  Ok(converted)
}

fn positive_projection_f32(subject: &str, field: &str, value: f64) -> Result<f32, String> {
  if value <= 0.0 {
    return Err(format!("{subject} {field} must be positive"));
  }
  let converted = finite_projection_f32(subject, field, value)?;
  if converted <= 0.0 {
    return Err(format!("{subject} {field} must remain positive when represented as f32"));
  }
  Ok(converted)
}

pub fn circle_radius_playfield(circle_size: f32) -> f64 {
  54.4 - 4.48 * f64::from(circle_size)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_candidate_a_maps_center_for_matching_aspect_ratio() {
    let projection =
      PlayfieldProjection::for_capture_with_strategy(1024.0, 768.0, 4.0, ProjectionStrategy::CandidateADirect512x384).expect("projection");

    let (x, y) = projection.to_window_point(256.0, 192.0);
    assert_eq!(x, 512.0);
    assert_eq!(y, 384.0);
    assert!((projection.match_radius_px - 72.96).abs() < 0.01);
  }

  #[test]
  fn test_candidate_a_letterboxes_wider_windows() {
    let projection =
      PlayfieldProjection::for_capture_with_strategy(1280.0, 720.0, 4.0, ProjectionStrategy::CandidateADirect512x384).expect("projection");

    let (left, top) = projection.to_window_point(0.0, 0.0);
    let (right, bottom) = projection.to_window_point(512.0, 384.0);

    assert_eq!(left, 160.0);
    assert_eq!(top, 0.0);
    assert_eq!(right, 1120.0);
    assert_eq!(bottom, 720.0);
  }

  #[test]
  fn test_candidate_b_virtual_canvas_80_percent_height() {
    let projection = PlayfieldProjection::for_capture_with_strategy(1280.0, 720.0, 4.0, ProjectionStrategy::CandidateBVirtualCanvas640x480)
      .expect("projection");

    // 720 / 480 = 1.5 scale. Playfield height = 384 * 1.5 = 576. 576 / 720 = 0.8 (80% height!)
    assert_eq!(projection.scale_x, 1.5);
    assert_eq!(projection.scale_y, 1.5);

    let (left, top) = projection.to_window_point(0.0, 0.0);
    let (right, bottom) = projection.to_window_point(512.0, 384.0);

    assert_eq!(left, 256.0); // 160 + 64*1.5
    assert_eq!(top, 72.0); // 0 + 48*1.5
    assert_eq!(right, 1024.0); // 256 + 512*1.5
    assert_eq!(bottom, 648.0); // 72 + 384*1.5 (height = 576)
  }

  #[test]
  fn test_for_capture_defaults_to_candidate_b() {
    let proj = PlayfieldProjection::for_capture(1280.0, 720.0, 4.0).expect("projection");
    assert_eq!(proj.strategy, ProjectionStrategy::CandidateBVirtualCanvas640x480);
    assert_eq!(proj.scale_x, 1.5);
    assert_eq!(proj.offset_x, 256.0);
    assert_eq!(proj.offset_y, 72.0);
  }

  #[test]
  fn test_dual_candidate_gate_decides_winner() {
    // Ground truth matching Candidate B (1280x720 window)
    let samples = vec![
      (0, (0.0, 0.0), (256.0, 72.0)),
      (1, (256.0, 192.0), (640.0, 360.0)),
      (2, (512.0, 384.0), (1024.0, 648.0)),
    ];
    let report = run_projection_dual_candidate_gate(1280.0, 720.0, 4.0, &samples, 2.0).expect("gate run");

    assert!(report.gate_passed);
    assert_eq!(report.winner, Some(ProjectionStrategy::CandidateBVirtualCanvas640x480));
    assert_eq!(report.falsified, Some(ProjectionStrategy::CandidateADirect512x384));
    assert!(report.candidate_a.max_error_px > 50.0, "Candidate A should fail");
    assert_eq!(report.candidate_b.max_error_px, 0.0, "Candidate B should pass with 0 error");
  }
}
