use crate::model::CurveType;

#[derive(Clone, Debug)]
pub struct SliderCurve {
  polyline: Vec<(f64, f64)>,
  cumulative_lengths: Vec<f64>,
  total_length: f64,
}

impl SliderCurve {
  pub fn new(curve_type: CurveType, start_pos: (f64, f64), control_points: &[(f64, f64)], expected_length: f64) -> Self {
    let mut all_points = Vec::with_capacity(control_points.len() + 1);
    if control_points.is_empty() || (control_points[0].0 - start_pos.0).hypot(control_points[0].1 - start_pos.1) > 1e-4 {
      all_points.push(start_pos);
    }
    all_points.extend_from_slice(control_points);

    let polyline = match curve_type {
      CurveType::Linear => build_linear_polyline(&all_points),
      CurveType::PerfectCircle => {
        if all_points.len() == 3 {
          build_perfect_circle_polyline(all_points[0], all_points[1], all_points[2], expected_length)
        } else {
          build_linear_polyline(&all_points)
        }
      }
      CurveType::Bezier => build_bezier_polyline(&all_points),
      CurveType::Catmull => build_catmull_polyline(&all_points),
    };

    let mut cumulative_lengths = Vec::with_capacity(polyline.len());
    let mut total = 0.0;
    cumulative_lengths.push(0.0);

    for window in polyline.windows(2) {
      let dx = window[1].0 - window[0].0;
      let dy = window[1].1 - window[0].1;
      let dist = dx.hypot(dy);
      total += dist;
      cumulative_lengths.push(total);
    }

    Self {
      polyline,
      cumulative_lengths,
      total_length: expected_length.max(total),
    }
  }

  pub fn total_length(&self) -> f64 {
    self.total_length
  }

  /// Evaluates the curve position at an arc distance `d` (clamped to `[0, total_length]`).
  pub fn position_at_distance(&self, d: f64) -> (f64, f64) {
    if self.polyline.is_empty() {
      return (0.0, 0.0);
    }
    if self.polyline.len() == 1 || d <= 0.0 {
      return self.polyline[0];
    }

    let clamped_d = d.min(*self.cumulative_lengths.last().unwrap_or(&0.0));

    // Binary search for segment
    let idx = match self.cumulative_lengths.binary_search_by(|val| val.partial_cmp(&clamped_d).unwrap()) {
      Ok(i) => return self.polyline[i],
      Err(i) => i.saturating_sub(1),
    };

    if idx >= self.polyline.len() - 1 {
      return *self.polyline.last().unwrap();
    }

    let d0 = self.cumulative_lengths[idx];
    let d1 = self.cumulative_lengths[idx + 1];
    let seg_len = d1 - d0;
    if seg_len <= 1e-7 {
      return self.polyline[idx];
    }

    let t = (clamped_d - d0) / seg_len;
    let p0 = self.polyline[idx];
    let p1 = self.polyline[idx + 1];

    (p0.0 + t * (p1.0 - p0.0), p0.1 + t * (p1.1 - p0.1))
  }

  /// Evaluates the slider ball position at normalized progress [0.0, 1.0] across repeat slides.
  pub fn position_at_slide_progress(&self, progress: f64, repeat_count: usize, pixel_length: f64) -> (f64, f64) {
    let repeats = (repeat_count.max(1)) as f64;
    let full_progress = (progress * repeats).clamp(0.0, repeats);
    let slide_idx = (full_progress.floor() as usize).min(repeat_count.saturating_sub(1));
    let mut slide_p = full_progress - (slide_idx as f64);

    if slide_idx % 2 == 1 {
      // Reverse slide
      slide_p = 1.0 - slide_p;
    }

    let d = slide_p * pixel_length;
    self.position_at_distance(d)
  }
}

fn build_linear_polyline(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
  points.to_vec()
}

fn build_perfect_circle_polyline(p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), expected_length: f64) -> Vec<(f64, f64)> {
  let d = 2.0 * (p1.0 * (p2.1 - p3.1) + p2.0 * (p3.1 - p1.1) + p3.0 * (p1.1 - p2.1));
  if d.abs() < 1e-4 {
    // Degenerate/collinear fallback to linear
    return vec![p1, p2, p3];
  }

  let p1_sq = p1.0 * p1.0 + p1.1 * p1.1;
  let p2_sq = p2.0 * p2.0 + p2.1 * p2.1;
  let p3_sq = p3.0 * p3.0 + p3.1 * p3.1;

  let cx = (p1_sq * (p2.1 - p3.1) + p2_sq * (p3.1 - p1.1) + p3_sq * (p1.1 - p2.1)) / d;
  let cy = (p1_sq * (p3.0 - p2.0) + p2_sq * (p1.0 - p3.0) + p3_sq * (p2.0 - p1.0)) / d;
  let r = (p1.0 - cx).hypot(p1.1 - cy);

  if r <= 1e-4 {
    return vec![p1, p2, p3];
  }

  let theta1 = (p1.1 - cy).atan2(p1.0 - cx);
  let theta2 = (p2.1 - cy).atan2(p2.0 - cx);
  let theta3 = (p3.1 - cy).atan2(p3.0 - cx);

  let mut d_theta2 = theta2 - theta1;
  let mut d_theta3 = theta3 - theta1;

  while d_theta2 < 0.0 {
    d_theta2 += 2.0 * std::f64::consts::PI;
  }
  while d_theta3 < 0.0 {
    d_theta3 += 2.0 * std::f64::consts::PI;
  }

  let clockwise = d_theta2 > d_theta3;
  let total_arc = expected_length / r;

  let segments = ((expected_length / 4.0).ceil() as usize).clamp(16, 256);
  let mut points = Vec::with_capacity(segments + 1);

  for i in 0..=segments {
    let t = (i as f64) / (segments as f64);
    let angle = if clockwise {
      theta1 - t * total_arc
    } else {
      theta1 + t * total_arc
    };
    points.push((cx + r * angle.cos(), cy + r * angle.sin()));
  }

  points
}

fn build_bezier_polyline(control_points: &[(f64, f64)]) -> Vec<(f64, f64)> {
  if control_points.len() <= 2 {
    return control_points.to_vec();
  }

  let mut result = Vec::new();
  let mut start = 0;

  for i in 1..control_points.len() {
    // Duplicate control point marks the boundary of multi-bezier segments
    if i < control_points.len() - 1 && control_points[i] == control_points[i + 1] {
      let segment = &control_points[start..=i];
      sample_single_bezier(segment, &mut result);
      start = i + 1;
    }
  }

  if start < control_points.len() {
    sample_single_bezier(&control_points[start..], &mut result);
  }

  result.dedup();
  result
}

fn sample_single_bezier(points: &[(f64, f64)], out: &mut Vec<(f64, f64)>) {
  if points.len() <= 1 {
    if let Some(&p) = points.first() {
      out.push(p);
    }
    return;
  }
  if points.len() == 2 {
    out.push(points[0]);
    out.push(points[1]);
    return;
  }

  let approx_len = points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum::<f64>();
  let steps = ((approx_len / 4.0).ceil() as usize).clamp(10, 100);

  for step in 0..=steps {
    let t = (step as f64) / (steps as f64);
    out.push(de_casteljau(points, t));
  }
}

fn de_casteljau(points: &[(f64, f64)], t: f64) -> (f64, f64) {
  let mut current = points.to_vec();
  while current.len() > 1 {
    for i in 0..current.len() - 1 {
      current[i].0 = (1.0 - t) * current[i].0 + t * current[i + 1].0;
      current[i].1 = (1.0 - t) * current[i].1 + t * current[i + 1].1;
    }
    current.pop();
  }
  current[0]
}

fn build_catmull_polyline(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
  if points.len() <= 2 {
    return points.to_vec();
  }

  let mut result = Vec::new();
  for i in 0..points.len() - 1 {
    let p0 = if i == 0 { points[0] } else { points[i - 1] };
    let p1 = points[i];
    let p2 = points[i + 1];
    let p3 = if i + 2 < points.len() {
      points[i + 2]
    } else {
      p2
    };

    for step in 0..=20 {
      let t = (step as f64) / 20.0;
      let t2 = t * t;
      let t3 = t2 * t;

      let x = 0.5
        * ((2.0 * p1.0)
          + (-p0.0 + p2.0) * t
          + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
          + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);
      let y = 0.5
        * ((2.0 * p1.1)
          + (-p0.1 + p2.1) * t
          + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
          + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);

      result.push((x, y));
    }
  }

  result.dedup();
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_slider_curve_with_absolute_control_points_does_not_duplicate_start() {
    // 3 points describing a circular arc: start (100, 100), mid (200, 150), end (300, 100)
    let start_pos = (100.0, 100.0);
    let control_points = vec![(100.0, 100.0), (200.0, 150.0), (300.0, 100.0)];
    let curve = SliderCurve::new(CurveType::PerfectCircle, start_pos, &control_points, 200.0);

    // Initial position at d=0 must be start_pos
    let p0 = curve.position_at_distance(0.0);
    assert!((p0.0 - 100.0).abs() < 1e-3);
    assert!((p0.1 - 100.0).abs() < 1e-3);

    // Arc midpoint must have y > 100.0 (bowing upward toward 150.0)
    let p_mid = curve.position_at_distance(100.0);
    assert!(p_mid.1 > 120.0, "arc midpoint y should be curved up: got {}", p_mid.1);
  }

  #[test]
  fn test_slider_curve_with_stacked_offset_translates_identically() {
    let start_pos1 = (100.0, 100.0);
    let control_points1 = vec![(100.0, 100.0), (200.0, 150.0), (300.0, 100.0)];
    let curve1 = SliderCurve::new(CurveType::PerfectCircle, start_pos1, &control_points1, 200.0);

    // Shift everything by (-10.0, -10.0)
    let dx = -10.0;
    let dy = -10.0;
    let start_pos2 = (100.0 + dx, 100.0 + dy);
    let control_points2 = vec![
      (100.0 + dx, 100.0 + dy),
      (200.0 + dx, 150.0 + dy),
      (300.0 + dx, 100.0 + dy),
    ];
    let curve2 = SliderCurve::new(CurveType::PerfectCircle, start_pos2, &control_points2, 200.0);

    let p1 = curve1.position_at_distance(100.0);
    let p2 = curve2.position_at_distance(100.0);

    assert!((p2.0 - (p1.0 + dx)).abs() < 1e-3);
    assert!((p2.1 - (p1.1 + dy)).abs() < 1e-3);
  }
}
