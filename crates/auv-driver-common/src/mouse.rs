use std::time::Duration;

use crate::{DriverError, DriverResult, Point};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseStart {
  Current,
  Screen(Point),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseCubicBezierSegment {
  pub control_1: Point,
  pub control_2: Point,
  pub end: Point,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MouseCurve {
  pub start: Point,
  pub segments: Vec<MouseCubicBezierSegment>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseCurveMapping {
  pub width: f64,
  pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseMotionOptions {
  pub duration: Duration,
  pub sample_rate_hz: u32,
  /// Positive screen-space arc-length approximation tolerance for curves.
  pub curve_tolerance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MoveMouseRequest {
  /// Zero selects the shared default logical mouse.
  pub mouse: u64,
  /// Omission continues a held route, otherwise selects foreground delivery.
  pub target: Option<crate::InputTarget>,
  pub start: MouseStart,
  pub curve: MouseCurve,
  pub mapping: MouseCurveMapping,
  pub options: MouseMotionOptions,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseMotionSample {
  pub point: Point,
  pub elapsed: Duration,
}

impl MoveMouseRequest {
  pub fn direct(point: Point) -> Self {
    Self {
      mouse: 0,
      target: None,
      start: MouseStart::Screen(point),
      curve: MouseCurve {
        start: Point::new(0.0, 0.0),
        segments: Vec::new(),
      },
      mapping: MouseCurveMapping {
        width: 1.0,
        height: 1.0,
      },
      options: MouseMotionOptions {
        duration: Duration::ZERO,
        sample_rate_hz: 0,
        curve_tolerance: 0.0,
      },
    }
  }

  /// Validates geometry once; time samples are evaluated on demand.
  pub fn samples(&self, resolved_start: Point) -> DriverResult<MouseSamples> {
    validate_point(resolved_start, "resolved mouse start")?;
    validate_point(self.curve.start, "curve start")?;
    if !self.mapping.width.is_finite() || !self.mapping.height.is_finite() || self.mapping.width <= 0.0 || self.mapping.height <= 0.0 {
      return Err(invalid("mouse mapping width and height must be finite and positive"));
    }
    if self.curve.segments.is_empty() {
      return Ok(MouseSamples {
        points: vec![resolved_start],
        distances: vec![0.0],
        intervals: 0,
        options: self.options,
      });
    }
    if !self.options.curve_tolerance.is_finite() || self.options.curve_tolerance <= 0.0 {
      return Err(invalid("mouse curve_tolerance must be finite and positive"));
    }
    if self.options.sample_rate_hz == 0 && !self.options.duration.is_zero() {
      return Err(invalid("timed mouse movement requires a positive sample_rate_hz"));
    }
    let intervals = (self.options.duration.as_nanos() * u128::from(self.options.sample_rate_hz)).div_ceil(1_000_000_000).max(1);
    let intervals = u64::try_from(intervals)
      .ok()
      .filter(|value| *value < u64::MAX)
      .ok_or_else(|| invalid("mouse sample count exceeds the protocol integer range"))?;
    let mut points = vec![resolved_start];
    let mut from = resolved_start;
    for segment in &self.curve.segments {
      let control_1 = mapped(segment.control_1, self.curve.start, resolved_start, self.mapping);
      let control_2 = mapped(segment.control_2, self.curve.start, resolved_start, self.mapping);
      let end = mapped(segment.end, self.curve.start, resolved_start, self.mapping);
      for point in [control_1, control_2, end] {
        validate_point(point, "mapped curve coordinate")?;
      }
      flatten([from, control_1, control_2, end], self.options.curve_tolerance, &mut points)?;
      from = end;
    }
    let mut distances = Vec::new();
    distances.try_reserve(points.len()).map_err(|_| invalid("cannot allocate mouse curve distances"))?;
    distances.push(0.0);
    for pair in points.windows(2) {
      let distance = distances.last().unwrap() + (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
      if !distance.is_finite() {
        return Err(invalid("mouse curve length is not finite"));
      }
      distances.push(distance);
    }
    Ok(MouseSamples {
      points,
      distances,
      intervals,
      options: self.options,
    })
  }
}

/// Geometry is retained once; memory does not grow with duration or sample rate.
pub struct MouseSamples {
  points: Vec<Point>,
  distances: Vec<f64>,
  intervals: u64,
  options: MouseMotionOptions,
}
impl MouseSamples {
  pub fn len(&self) -> u64 {
    self.intervals + 1
  }
  pub fn is_empty(&self) -> bool {
    false
  }
  pub fn at(&self, index: u64) -> MouseMotionSample {
    assert!(index < self.len());
    let elapsed = if index == self.intervals {
      self.options.duration
    } else if self.options.sample_rate_hz == 0 {
      Duration::ZERO
    } else {
      Duration::from_secs(index / u64::from(self.options.sample_rate_hz))
        + Duration::from_nanos(
          (u128::from(index % u64::from(self.options.sample_rate_hz)) * 1_000_000_000 / u128::from(self.options.sample_rate_hz)) as u64,
        )
    };
    let elapsed = if self.intervals == 0 {
      Duration::ZERO
    } else {
      elapsed
    };
    let ratio = if self.options.duration.is_zero() {
      if index == self.intervals { 1.0 } else { 0.0 }
    } else {
      elapsed.as_secs_f64() / self.options.duration.as_secs_f64()
    };
    MouseMotionSample {
      point: if index == self.intervals {
        *self.points.last().unwrap()
      } else {
        interpolate_polyline(&self.points, &self.distances, self.distances.last().unwrap() * ratio)
      },
      elapsed,
    }
  }
  /// Skip overdue samples arithmetically, without walking or allocating them.
  pub fn latest_due(&self, next: u64, elapsed: Duration) -> u64 {
    if elapsed >= self.options.duration {
      return self.intervals;
    }
    let due = elapsed.as_nanos() * u128::from(self.options.sample_rate_hz) / 1_000_000_000;
    (due.min(u128::from(self.intervals)) as u64).max(next)
  }
}

// De Casteljau subdivision bounds arc-length error by control-polygon excess.
// The caller selects accuracy in screen units, rather than a fixed step count.
fn flatten(curve: [Point; 4], tolerance: f64, points: &mut Vec<Point>) -> DriverResult<()> {
  let mut pending = vec![(curve, tolerance)];
  while let Some(([a, b, c, d], tolerance)) = pending.pop() {
    let polygon = (b.x - a.x).hypot(b.y - a.y) + (c.x - b.x).hypot(c.y - b.y) + (d.x - c.x).hypot(d.y - c.y);
    let chord = (d.x - a.x).hypot(d.y - a.y);
    if !polygon.is_finite() {
      return Err(invalid("mouse curve length is not finite"));
    }
    if polygon - chord <= tolerance {
      points.try_reserve(1).map_err(|_| invalid("cannot allocate mouse curve geometry"))?;
      points.push(d);
      continue;
    }
    let ab = midpoint(a, b);
    let bc = midpoint(b, c);
    let cd = midpoint(c, d);
    let abc = midpoint(ab, bc);
    let bcd = midpoint(bc, cd);
    let mid = midpoint(abc, bcd);
    let left = [a, ab, abc, mid];
    let right = [mid, bcd, cd, d];
    if left == [a, b, c, d] || right == [a, b, c, d] || tolerance / 2.0 == 0.0 {
      return Err(invalid("mouse curve tolerance is below coordinate precision"));
    }
    pending.try_reserve(2).map_err(|_| invalid("cannot allocate mouse curve subdivision"))?;
    pending.push((right, tolerance / 2.0));
    pending.push((left, tolerance / 2.0));
  }
  Ok(())
}
fn midpoint(a: Point, b: Point) -> Point {
  Point::new(a.x / 2.0 + b.x / 2.0, a.y / 2.0 + b.y / 2.0)
}

fn mapped(point: Point, origin: Point, start: Point, mapping: MouseCurveMapping) -> Point {
  Point::new(start.x + (point.x - origin.x) * mapping.width, start.y + (point.y - origin.y) * mapping.height)
}

fn interpolate_polyline(points: &[Point], distances: &[f64], target: f64) -> Point {
  if target <= 0.0 {
    return points[0];
  }
  let index = distances.partition_point(|distance| *distance < target).min(distances.len() - 1);
  if index == 0 {
    return points[0];
  }
  let span = distances[index] - distances[index - 1];
  if span <= 0.0 {
    return points[index];
  }
  let ratio = (target - distances[index - 1]) / span;
  Point::new(
    points[index - 1].x + (points[index].x - points[index - 1].x) * ratio,
    points[index - 1].y + (points[index].y - points[index - 1].y) * ratio,
  )
}

fn validate_point(point: Point, label: &str) -> DriverResult<()> {
  if point.x.is_finite() && point.y.is_finite() {
    Ok(())
  } else {
    Err(invalid(format!("{label} must contain finite coordinates")))
  }
}

fn invalid(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn maps_normalized_curve_relative_to_resolved_start() {
    let request = MoveMouseRequest {
      mouse: 0,
      target: None,
      start: MouseStart::Current,
      curve: MouseCurve {
        start: Point::new(0.25, 0.25),
        segments: vec![MouseCubicBezierSegment {
          control_1: Point::new(0.5, 0.25),
          control_2: Point::new(0.75, 0.5),
          end: Point::new(1.0, 1.0),
        }],
      },
      mapping: MouseCurveMapping {
        width: 800.0,
        height: 400.0,
      },
      options: MouseMotionOptions {
        duration: Duration::from_secs(1),
        sample_rate_hz: 10,
        curve_tolerance: 0.1,
      },
    };
    let samples = request.samples(Point::new(100.0, 200.0)).unwrap();
    assert_eq!(samples.at(0).point, Point::new(100.0, 200.0));
    assert_eq!(samples.at(samples.len() - 1).point, Point::new(700.0, 500.0));
    assert_eq!(samples.at(samples.len() - 1).elapsed, Duration::from_secs(1));
  }

  #[test]
  fn rejects_non_finite_curve_coordinates_before_delivery() {
    let mut request = MoveMouseRequest::direct(Point::new(1.0, 2.0));
    // Keep other options valid so the error must come from the NaN coordinate.
    request.options.curve_tolerance = 0.01;
    request.curve.segments.push(MouseCubicBezierSegment {
      control_1: Point::new(f64::NAN, 0.0),
      control_2: Point::new(0.0, 0.0),
      end: Point::new(1.0, 1.0),
    });
    assert!(matches!(request.samples(Point::new(1.0, 2.0)), Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn sample_indices_exceed_u32_without_materializing_the_schedule() {
    let mut request = MoveMouseRequest::direct(Point::new(0.0, 0.0));
    request.curve.segments = vec![MouseCubicBezierSegment {
      control_1: Point::new(1.0, 0.0),
      control_2: Point::new(2.0, 0.0),
      end: Point::new(3.0, 0.0),
    }];
    request.options = MouseMotionOptions {
      duration: Duration::from_secs(86_400),
      sample_rate_hz: 1_000_000,
      curve_tolerance: 0.01,
    };
    let samples = request.samples(Point::new(0.0, 0.0)).unwrap();
    assert_eq!(samples.len(), 86_400_000_001);
    assert_eq!(samples.points.len(), 2);
    assert_eq!(samples.latest_due(0, Duration::from_secs(43_200)), 43_200_000_000);
    assert_eq!(samples.at(samples.len() - 1).point, Point::new(3.0, 0.0));
    assert_eq!(samples.at(samples.len() - 1).elapsed, request.options.duration);
  }
}
