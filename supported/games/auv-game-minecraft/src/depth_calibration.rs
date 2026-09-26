//! Multi-anchor affine depth calibration.
//!
//! Monocular depth estimation models (such as MiDaS and Depth Anything) produce relative
//! disparity or affine-invariant depth maps rather than absolute metric depth.
//! Single-point linear scale calibration (scale = d_true / d_pred) is vulnerable to non-linear
//! scale/shift drift across viewpoints.
//!
//! `AffineDepthCalibrator` fits a 2-parameter affine model:
//!     d* = s * d_pred + t
//! using ordinary least squares (OLS) over a sliding window of ground-truth raycast anchors.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AffineDepthCalibrator {
  anchors: Vec<(f32, f32)>, // (predicted, true_depth_m)
  max_anchors: usize,
}

impl AffineDepthCalibrator {
  pub fn new(max_anchors: usize) -> Self {
    Self {
      anchors: Vec::with_capacity(max_anchors),
      max_anchors: max_anchors.max(2),
    }
  }

  /// Add a new calibration anchor: (predicted depth value, ground truth metric depth in meters).
  pub fn add_anchor(&mut self, predicted: f32, true_depth_m: f32) {
    if !predicted.is_finite() || !true_depth_m.is_finite() || predicted <= 0.0 || true_depth_m <= 0.0 {
      return;
    }
    if self.anchors.len() >= self.max_anchors {
      self.anchors.remove(0);
    }
    self.anchors.push((predicted, true_depth_m));
  }

  pub fn len(&self) -> usize {
    self.anchors.len()
  }

  pub fn is_empty(&self) -> bool {
    self.anchors.is_empty()
  }

  /// Fit affine parameters (s, t) such that true_depth ≈ s * predicted + t.
  /// Returns None if anchor count < 2 or if predicted values have zero variance.
  pub fn fit(&self) -> Option<(f32, f32)> {
    if self.anchors.len() < 2 {
      return None;
    }

    let n = self.anchors.len() as f64;
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for &(x, y) in &self.anchors {
      sum_x += x as f64;
      sum_y += y as f64;
    }
    let mean_x = sum_x / n;
    let mean_y = sum_y / n;

    let mut var_x = 0.0;
    let mut cov_xy = 0.0;
    for &(x, y) in &self.anchors {
      let dx = x as f64 - mean_x;
      let dy = y as f64 - mean_y;
      var_x += dx * dx;
      cov_xy += dx * dy;
    }

    if var_x < 1e-8 {
      return None;
    }

    let s = cov_xy / var_x;
    let t = mean_y - s * mean_x;

    Some((s as f32, t as f32))
  }

  /// Returns the root-mean-square error (RMSE) in meters of the fitted model over anchors.
  /// Returns None if fit() is None.
  pub fn fit_rmse_m(&self) -> Option<f32> {
    let (s, t) = self.fit()?;
    let n = self.anchors.len() as f64;
    let mut sum_sq_err = 0.0;
    for &(x, y) in &self.anchors {
      let pred = s as f64 * x as f64 + t as f64;
      let err = pred - y as f64;
      sum_sq_err += err * err;
    }
    Some((sum_sq_err / n).sqrt() as f32)
  }

  /// Calibrate a model-predicted depth value into physical metric depth (meters).
  /// Returns None if calibration cannot be fitted or if the result is non-positive.
  pub fn calibrate(&self, predicted: f32) -> Option<f32> {
    let (s, t) = self.fit()?;
    let calibrated = s * predicted + t;
    if calibrated.is_finite() && calibrated > 0.0 {
      Some(calibrated)
    } else {
      None
    }
  }
}

impl Default for AffineDepthCalibrator {
  fn default() -> Self {
    Self::new(20)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_fit_recovers_synthetic_affine_parameters() {
    let mut calibrator = AffineDepthCalibrator::new(20);
    // Ground truth: d_true = 0.5 * d_pred + 1.0
    // Generate anchors with slight noise
    let test_points = [(2.0, 2.0), (4.0, 3.0), (6.0, 4.0), (8.0, 5.0), (10.0, 6.0)];
    for &(pred, true_m) in &test_points {
      calibrator.add_anchor(pred, true_m);
    }

    let (s, t) = calibrator.fit().expect("fit successful");
    assert!((s - 0.5).abs() < 0.02, "expected s ~ 0.5, got {}", s);
    assert!((t - 1.0).abs() < 0.05, "expected t ~ 1.0, got {}", t);

    let rmse = calibrator.fit_rmse_m().expect("rmse calculated");
    assert!(rmse < 0.01, "expected small rmse, got {}", rmse);

    // Test prediction calibration
    let calibrated = calibrator.calibrate(5.0).expect("calibrated 5.0");
    // 0.5 * 5.0 + 1.0 = 3.5
    assert!((calibrated - 3.5).abs() < 0.05);
  }

  #[test]
  fn test_insufficient_anchors_returns_none() {
    let mut calibrator = AffineDepthCalibrator::new(20);
    assert_eq!(calibrator.fit(), None);
    assert_eq!(calibrator.calibrate(5.0), None);

    calibrator.add_anchor(2.0, 3.0);
    assert_eq!(calibrator.fit(), None);
    assert_eq!(calibrator.calibrate(5.0), None);
    assert_eq!(calibrator.fit_rmse_m(), None);
  }

  #[test]
  fn test_sliding_window_evicts_old_anchors() {
    let mut calibrator = AffineDepthCalibrator::new(3);
    calibrator.add_anchor(1.0, 1.0);
    calibrator.add_anchor(2.0, 2.0);
    calibrator.add_anchor(3.0, 3.0);
    assert_eq!(calibrator.len(), 3);

    calibrator.add_anchor(4.0, 4.0);
    assert_eq!(calibrator.len(), 3);
    // Oldest (1.0, 1.0) should have been evicted; anchors are (2,2), (3,3), (4,4)
    let (s, t) = calibrator.fit().unwrap();
    assert!((s - 1.0).abs() < 1e-4);
    assert!(t.abs() < 1e-4);
  }

  #[test]
  fn test_zero_variance_returns_none() {
    let mut calibrator = AffineDepthCalibrator::new(5);
    calibrator.add_anchor(3.0, 1.0);
    calibrator.add_anchor(3.0, 5.0);
    assert_eq!(calibrator.fit(), None);
  }
}
