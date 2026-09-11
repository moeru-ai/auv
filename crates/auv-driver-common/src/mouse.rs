use std::time::Duration;

use crate::{DriverError, DriverResult, Point};

const FLATTEN_STEPS_PER_SEGMENT: usize = 24;
pub const MOUSE_MOTION_MAX_SEGMENTS: usize = 4096;
const MAX_SAMPLES: usize = 60 * 240;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseMotionOptions {
  pub duration: Duration,
  pub sample_rate_hz: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MouseMotionPlan {
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

impl MouseMotionPlan {
  pub fn direct(point: Point) -> Self {
    Self {
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
        sample_rate_hz: 120,
      },
    }
  }

  /// Validates and samples the plan after the caller resolves `MouseStart`.
  pub fn samples(&self, resolved_start: Point) -> DriverResult<Vec<MouseMotionSample>> {
    validate_point(resolved_start, "resolved mouse start")?;
    validate_point(self.curve.start, "curve start")?;
    if !self.mapping.width.is_finite() || !self.mapping.height.is_finite() || self.mapping.width <= 0.0 || self.mapping.height <= 0.0 {
      return Err(invalid("mouse mapping width and height must be finite and positive"));
    }
    if !(1..=240).contains(&self.options.sample_rate_hz) {
      return Err(invalid("mouse sample_rate_hz must be in 1..=240"));
    }
    if self.options.duration > Duration::from_secs(60) {
      return Err(invalid("mouse duration must not exceed 60 seconds"));
    }
    if self.curve.segments.len() > MOUSE_MOTION_MAX_SEGMENTS {
      return Err(invalid("mouse curve has too many segments"));
    }
    if self.curve.segments.is_empty() {
      return Ok(vec![MouseMotionSample {
        point: resolved_start,
        elapsed: Duration::ZERO,
      }]);
    }

    let mut polyline = vec![mapped(
      self.curve.start,
      self.curve.start,
      resolved_start,
      self.mapping,
    )];
    let mut from = self.curve.start;
    for segment in &self.curve.segments {
      validate_point(segment.control_1, "Bezier control point")?;
      validate_point(segment.control_2, "Bezier control point")?;
      validate_point(segment.end, "Bezier end point")?;
      for step in 1..=FLATTEN_STEPS_PER_SEGMENT {
        let t = step as f64 / FLATTEN_STEPS_PER_SEGMENT as f64;
        polyline.push(mapped(cubic(from, *segment, t), self.curve.start, resolved_start, self.mapping));
      }
      from = segment.end;
    }
    for point in &polyline {
      validate_point(*point, "mapped mouse curve point")?;
    }

    let mut cumulative = Vec::with_capacity(polyline.len());
    cumulative.push(0.0);
    for pair in polyline.windows(2) {
      let distance = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
      cumulative.push(cumulative.last().copied().unwrap_or(0.0) + distance);
    }
    let total_distance = *cumulative.last().unwrap_or(&0.0);
    let sample_count =
      ((self.options.duration.as_secs_f64() * f64::from(self.options.sample_rate_hz)).ceil() as usize).clamp(1, MAX_SAMPLES);
    let mut samples = Vec::with_capacity(sample_count + 1);
    for index in 0..=sample_count {
      let ratio = index as f64 / sample_count as f64;
      let point = interpolate_polyline(&polyline, &cumulative, total_distance * ratio);
      samples.push(MouseMotionSample {
        point,
        elapsed: self.options.duration.mul_f64(ratio),
      });
    }
    Ok(samples)
  }
}

fn mapped(point: Point, origin: Point, start: Point, mapping: MouseCurveMapping) -> Point {
  Point::new(start.x + (point.x - origin.x) * mapping.width, start.y + (point.y - origin.y) * mapping.height)
}

fn cubic(start: Point, segment: MouseCubicBezierSegment, t: f64) -> Point {
  let u = 1.0 - t;
  Point::new(
    u.powi(3) * start.x + 3.0 * u.powi(2) * t * segment.control_1.x + 3.0 * u * t.powi(2) * segment.control_2.x + t.powi(3) * segment.end.x,
    u.powi(3) * start.y + 3.0 * u.powi(2) * t * segment.control_1.y + 3.0 * u * t.powi(2) * segment.control_2.y + t.powi(3) * segment.end.y,
  )
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
  if span <= f64::EPSILON {
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
    let plan = MouseMotionPlan {
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
      },
    };
    let samples = plan.samples(Point::new(100.0, 200.0)).unwrap();
    assert_eq!(samples.first().unwrap().point, Point::new(100.0, 200.0));
    assert_eq!(samples.last().unwrap().point, Point::new(700.0, 500.0));
    assert_eq!(samples.last().unwrap().elapsed, Duration::from_secs(1));
  }

  #[test]
  fn rejects_non_finite_curve_coordinates_before_delivery() {
    let mut plan = MouseMotionPlan::direct(Point::new(1.0, 2.0));
    plan.curve.segments.push(MouseCubicBezierSegment {
      control_1: Point::new(f64::NAN, 0.0),
      control_2: Point::new(0.0, 0.0),
      end: Point::new(1.0, 1.0),
    });
    assert!(matches!(plan.samples(Point::new(1.0, 2.0)), Err(DriverError::InvalidInput { .. })));
  }

  #[test]
  fn maximum_duration_and_rate_keep_the_requested_sample_rate() {
    let mut plan = MouseMotionPlan::direct(Point::new(1.0, 2.0));
    plan.curve.segments.push(MouseCubicBezierSegment {
      control_1: Point::new(0.25, 0.0),
      control_2: Point::new(0.75, 1.0),
      end: Point::new(1.0, 1.0),
    });
    plan.options = MouseMotionOptions {
      duration: Duration::from_secs(60),
      sample_rate_hz: 240,
    };

    let samples = plan.samples(Point::new(1.0, 2.0)).expect("maximum valid timing");
    assert_eq!(samples.len(), 14_401);
    assert_eq!(samples.last().expect("final sample").elapsed, Duration::from_secs(60));
  }
}
