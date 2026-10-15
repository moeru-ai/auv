use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::curve::SliderCurve;
use crate::model::{BeatmapModel, HitObject};

// Amended constants per Amendment package v1
pub const ARRIVAL_LEAD_MS: f64 = 12.0;
pub const POST_HIT_DWELL_MS: f64 = 8.0;
pub const STREAM_ALT_THRESHOLD_MS: f64 = 125.0;
pub const SLIDER_HOLD_TAIL_PAD_MS: f64 = 10.0;
pub const SPINNER_RADIUS_OSUPX: f64 = 50.0;
pub const SPINNER_RPS: f64 = 7.5;
pub const STRAIGHT_LINE_THRESHOLD_MS: f64 = 80.0;
pub const TRAJECTORY_SAMPLE_INTERVAL_MS: f64 = 4.0; // 250 Hz

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
  K1,
  K2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyEventType {
  Down,
  Up,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduledKeyEvent {
  pub time_ms: f64,
  pub key: KeyCode,
  pub event_type: KeyEventType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorTrajectoryPoint {
  pub time_ms: f64,
  pub x: f64,
  pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanHeader {
  pub map_title: String,
  pub map_artist: String,
  pub map_version: String,
  pub total_objects: usize,
  pub arrival_lead_ms: f64,
  pub post_hit_dwell_ms: f64,
  pub stream_alt_threshold_ms: f64,
  pub slider_hold_tail_pad_ms: f64,
  pub spinner_radius_osupx: f64,
  pub spinner_rps: f64,
  pub straight_line_threshold_ms: f64,
  pub trajectory_sample_interval_ms: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlanValidationError {
  EmptyTrajectory,
  NonFiniteTrajectoryCoordinate {
    index: usize,
    time_ms: f64,
    x: f64,
    y: f64,
  },
  NonMonotonicTrajectory {
    index: usize,
    prev_ms: f64,
    curr_ms: f64,
  },
  NonFiniteKeyTimestamp {
    index: usize,
    time_ms: f64,
  },
  NonMonotonicKeySchedule {
    index: usize,
    prev_ms: f64,
    curr_ms: f64,
  },
  InvalidKeyStateTransition {
    key: KeyCode,
    time_ms: f64,
    event_type: KeyEventType,
  },
}

impl std::fmt::Display for PlanValidationError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::EmptyTrajectory => write!(f, "trajectory is empty"),
      Self::NonFiniteTrajectoryCoordinate {
        index,
        time_ms,
        x,
        y,
      } => {
        write!(f, "trajectory coordinate at index {index} is non-finite (t={time_ms}, x={x}, y={y})")
      }
      Self::NonMonotonicTrajectory {
        index,
        prev_ms,
        curr_ms,
      } => {
        write!(f, "trajectory timestamp at index {index} is non-monotonic ({prev_ms} > {curr_ms})")
      }
      Self::NonFiniteKeyTimestamp { index, time_ms } => {
        write!(f, "key schedule timestamp at index {index} is non-finite ({time_ms})")
      }
      Self::NonMonotonicKeySchedule {
        index,
        prev_ms,
        curr_ms,
      } => {
        write!(f, "key schedule timestamp at index {index} is non-monotonic ({prev_ms} > {curr_ms})")
      }
      Self::InvalidKeyStateTransition {
        key,
        time_ms,
        event_type,
      } => {
        write!(f, "invalid key state transition: {key:?} at {time_ms} ms received {event_type:?} while already in that state")
      }
    }
  }
}

impl std::error::Error for PlanValidationError {}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionPlan {
  pub header: PlanHeader,
  pub trajectory: Vec<CursorTrajectoryPoint>,
  pub key_schedule: Vec<ScheduledKeyEvent>,
}

impl ActionPlan {
  pub fn validate_plan(&self) -> Result<(), PlanValidationError> {
    if self.trajectory.is_empty() {
      return Err(PlanValidationError::EmptyTrajectory);
    }

    for (i, p) in self.trajectory.iter().enumerate() {
      if !p.time_ms.is_finite() || !p.x.is_finite() || !p.y.is_finite() {
        return Err(PlanValidationError::NonFiniteTrajectoryCoordinate {
          index: i,
          time_ms: p.time_ms,
          x: p.x,
          y: p.y,
        });
      }
      if i > 0 && p.time_ms < self.trajectory[i - 1].time_ms {
        return Err(PlanValidationError::NonMonotonicTrajectory {
          index: i,
          prev_ms: self.trajectory[i - 1].time_ms,
          curr_ms: p.time_ms,
        });
      }
    }

    let mut k1_down = false;
    let mut k2_down = false;

    for (i, e) in self.key_schedule.iter().enumerate() {
      if !e.time_ms.is_finite() {
        return Err(PlanValidationError::NonFiniteKeyTimestamp {
          index: i,
          time_ms: e.time_ms,
        });
      }
      if i > 0 && e.time_ms < self.key_schedule[i - 1].time_ms {
        return Err(PlanValidationError::NonMonotonicKeySchedule {
          index: i,
          prev_ms: self.key_schedule[i - 1].time_ms,
          curr_ms: e.time_ms,
        });
      }

      let is_down = match e.key {
        KeyCode::K1 => &mut k1_down,
        KeyCode::K2 => &mut k2_down,
      };

      match e.event_type {
        KeyEventType::Down => {
          if *is_down {
            return Err(PlanValidationError::InvalidKeyStateTransition {
              key: e.key,
              time_ms: e.time_ms,
              event_type: KeyEventType::Down,
            });
          }
          *is_down = true;
        }
        KeyEventType::Up => {
          if !*is_down {
            return Err(PlanValidationError::InvalidKeyStateTransition {
              key: e.key,
              time_ms: e.time_ms,
              event_type: KeyEventType::Up,
            });
          }
          *is_down = false;
        }
      }
    }

    Ok(())
  }
}

pub fn synthesize_plan(beatmap: &BeatmapModel) -> ActionPlan {
  let header = PlanHeader {
    map_title: beatmap.title.clone(),
    map_artist: beatmap.artist.clone(),
    map_version: beatmap.version.clone(),
    total_objects: beatmap.total_objects(),
    arrival_lead_ms: ARRIVAL_LEAD_MS,
    post_hit_dwell_ms: POST_HIT_DWELL_MS,
    stream_alt_threshold_ms: STREAM_ALT_THRESHOLD_MS,
    slider_hold_tail_pad_ms: SLIDER_HOLD_TAIL_PAD_MS,
    spinner_radius_osupx: SPINNER_RADIUS_OSUPX,
    spinner_rps: SPINNER_RPS,
    straight_line_threshold_ms: STRAIGHT_LINE_THRESHOLD_MS,
    trajectory_sample_interval_ms: TRAJECTORY_SAMPLE_INTERVAL_MS,
  };

  let key_schedule = synthesize_key_schedule(&beatmap.hit_objects);
  let trajectory = synthesize_trajectory(&beatmap.hit_objects);

  ActionPlan {
    header,
    trajectory,
    key_schedule,
  }
}

fn synthesize_key_schedule(objects: &[HitObject]) -> Vec<ScheduledKeyEvent> {
  if objects.is_empty() {
    return Vec::new();
  }

  let mut events = Vec::new();
  let mut last_hit_time: Option<f64> = None;
  let mut active_key = KeyCode::K1;
  let mut prev_was_slider = false;
  let mut slider_key = KeyCode::K1;
  let mut k1_held_until = f64::NEG_INFINITY;
  let mut k2_held_until = f64::NEG_INFINITY;

  for (i, obj) in objects.iter().enumerate() {
    let hit_time = obj.start_time_ms();
    let k1_avail = hit_time >= k1_held_until;
    let k2_avail = hit_time >= k2_held_until;

    // Forced key alternation rules:
    // 1. Forced key switch for the first note after a slider hold
    // 2. Key occupancy constraint (cannot press already held key)
    // 3. Consecutive notes closer than STREAM_ALT_THRESHOLD_MS alternate K1/K2
    let key = if prev_was_slider {
      let alt_key = match slider_key {
        KeyCode::K1 => KeyCode::K2,
        KeyCode::K2 => KeyCode::K1,
      };
      let alt_avail = match alt_key {
        KeyCode::K1 => k1_avail,
        KeyCode::K2 => k2_avail,
      };
      if alt_avail {
        alt_key
      } else if k1_avail {
        KeyCode::K1
      } else {
        KeyCode::K2
      }
    } else if k1_avail && !k2_avail {
      KeyCode::K1
    } else if !k1_avail && k2_avail {
      KeyCode::K2
    } else if let Some(last_time) = last_hit_time {
      if (hit_time - last_time) <= STREAM_ALT_THRESHOLD_MS {
        match active_key {
          KeyCode::K1 => KeyCode::K2,
          KeyCode::K2 => KeyCode::K1,
        }
      } else {
        KeyCode::K1
      }
    } else {
      KeyCode::K1
    };

    active_key = key;
    last_hit_time = Some(hit_time);

    match obj {
      HitObject::Circle { .. } => {
        prev_was_slider = false;
        let raw_release = hit_time + POST_HIT_DWELL_MS;
        let release_time = if let Some(next_obj) = objects.get(i + 1) {
          let next_hit = next_obj.start_time_ms();
          if raw_release >= next_hit {
            (next_hit - 1.0).max(hit_time + 1.0)
          } else {
            raw_release
          }
        } else {
          raw_release
        };

        events.push(ScheduledKeyEvent {
          time_ms: hit_time,
          key,
          event_type: KeyEventType::Down,
        });
        events.push(ScheduledKeyEvent {
          time_ms: release_time,
          key,
          event_type: KeyEventType::Up,
        });

        match key {
          KeyCode::K1 => k1_held_until = release_time,
          KeyCode::K2 => k2_held_until = release_time,
        }
      }
      HitObject::Slider { duration_ms, .. } => {
        prev_was_slider = true;
        slider_key = key;
        let release_time = hit_time + duration_ms + SLIDER_HOLD_TAIL_PAD_MS;

        events.push(ScheduledKeyEvent {
          time_ms: hit_time,
          key,
          event_type: KeyEventType::Down,
        });
        events.push(ScheduledKeyEvent {
          time_ms: release_time,
          key,
          event_type: KeyEventType::Up,
        });

        match key {
          KeyCode::K1 => k1_held_until = release_time,
          KeyCode::K2 => k2_held_until = release_time,
        }
      }
      HitObject::Spinner { end_time_ms, .. } => {
        prev_was_slider = false;
        let release_time = *end_time_ms;

        events.push(ScheduledKeyEvent {
          time_ms: hit_time,
          key,
          event_type: KeyEventType::Down,
        });
        events.push(ScheduledKeyEvent {
          time_ms: release_time,
          key,
          event_type: KeyEventType::Up,
        });

        match key {
          KeyCode::K1 => k1_held_until = release_time,
          KeyCode::K2 => k2_held_until = release_time,
        }
      }
    }
  }

  events.sort_by(|a, b| {
    a.time_ms.partial_cmp(&b.time_ms).unwrap_or(std::cmp::Ordering::Equal).then_with(|| match (a.event_type, b.event_type) {
      (KeyEventType::Up, KeyEventType::Down) => std::cmp::Ordering::Less,
      (KeyEventType::Down, KeyEventType::Up) => std::cmp::Ordering::Greater,
      _ => std::cmp::Ordering::Equal,
    })
  });

  events
}

fn synthesize_trajectory(objects: &[HitObject]) -> Vec<CursorTrajectoryPoint> {
  if objects.is_empty() {
    return Vec::new();
  }

  let mut trajectory = Vec::new();

  // First object start point (arrive ARRIVAL_LEAD_MS early)
  let first_obj = &objects[0];
  let first_pos = first_obj.start_pos();
  let first_arrival_time = (first_obj.start_time_ms() - ARRIVAL_LEAD_MS).max(0.0);

  let mut current_time = first_arrival_time;
  let mut current_pos = first_pos;

  trajectory.push(CursorTrajectoryPoint {
    time_ms: current_time,
    x: current_pos.0,
    y: current_pos.1,
  });

  for (i, obj) in objects.iter().enumerate() {
    let target_pos = obj.start_pos();
    let hit_time = obj.start_time_ms();

    // 1. Move to object start pos
    let target_arrival_time = if hit_time - ARRIVAL_LEAD_MS > current_time {
      hit_time - ARRIVAL_LEAD_MS
    } else {
      hit_time.max(current_time)
    };

    if target_arrival_time > current_time {
      let duration = target_arrival_time - current_time;
      let steps = (duration / TRAJECTORY_SAMPLE_INTERVAL_MS).ceil().max(1.0) as usize;

      for step in 1..=steps {
        let t = (step as f64) / (steps as f64);
        let sample_time = current_time + t * duration;
        let x = current_pos.0 + t * (target_pos.0 - current_pos.0);
        let y = current_pos.1 + t * (target_pos.1 - current_pos.1);
        trajectory.push(CursorTrajectoryPoint {
          time_ms: sample_time,
          x,
          y,
        });
      }
      current_time = target_arrival_time;
      current_pos = target_pos;
    } else if (current_pos.0 - target_pos.0).hypot(current_pos.1 - target_pos.1) > 1e-4 {
      trajectory.push(CursorTrajectoryPoint {
        time_ms: current_time,
        x: target_pos.0,
        y: target_pos.1,
      });
      current_pos = target_pos;
    }

    // 2. Handle object-specific trajectory
    match obj {
      HitObject::Circle { x, y, time_ms } => {
        let next_start = objects.get(i + 1).map(|o| o.start_time_ms());
        let raw_dwell = *time_ms + POST_HIT_DWELL_MS;
        let dwell_time = match next_start {
          Some(next_t) => raw_dwell.min(next_t - 1.0).max(*time_ms),
          None => raw_dwell,
        };

        if dwell_time > current_time {
          current_time = dwell_time;
          current_pos = (*x, *y);
          trajectory.push(CursorTrajectoryPoint {
            time_ms: current_time,
            x: current_pos.0,
            y: current_pos.1,
          });
        }
      }
      HitObject::Slider {
        x,
        y,
        time_ms,
        curve_type,
        control_points,
        repeat_count,
        pixel_length,
        duration_ms,
        end_time_ms,
        ..
      } => {
        let curve = SliderCurve::new(*curve_type, (*x, *y), control_points, *pixel_length);
        let steps = (duration_ms / TRAJECTORY_SAMPLE_INTERVAL_MS).ceil().max(1.0) as usize;

        for step in 1..=steps {
          let elapsed = (step as f64) * TRAJECTORY_SAMPLE_INTERVAL_MS;
          let sample_time = (time_ms + elapsed).min(*end_time_ms);
          let progress = if *duration_ms > 0.0 {
            (elapsed / duration_ms).clamp(0.0, 1.0)
          } else {
            1.0
          };

          let pos = curve.position_at_slide_progress(progress, *repeat_count, *pixel_length);
          trajectory.push(CursorTrajectoryPoint {
            time_ms: sample_time,
            x: pos.0,
            y: pos.1,
          });
          current_pos = pos;
        }
        current_time = *end_time_ms;
      }
      HitObject::Spinner {
        x,
        y,
        time_ms,
        end_time_ms,
        duration_ms,
      } => {
        let steps = (duration_ms / TRAJECTORY_SAMPLE_INTERVAL_MS).ceil().max(1.0) as usize;
        let rev_speed = 2.0 * PI * SPINNER_RPS;

        for step in 1..=steps {
          let elapsed = (step as f64) * TRAJECTORY_SAMPLE_INTERVAL_MS;
          let sample_time = (time_ms + elapsed).min(*end_time_ms);
          let theta = (elapsed / 1000.0) * rev_speed;

          let sx = x + SPINNER_RADIUS_OSUPX * theta.cos();
          let sy = y + SPINNER_RADIUS_OSUPX * theta.sin();

          trajectory.push(CursorTrajectoryPoint {
            time_ms: sample_time,
            x: sx,
            y: sy,
          });
          current_pos = (sx, sy);
        }
        current_time = *end_time_ms;
      }
    }
  }

  trajectory
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parser::parse_beatmap_file;

  #[test]
  fn test_synthesize_sample_plan_amended_constants() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("must parse sample beatmap");
    let plan = synthesize_plan(&model);

    // Verify amended header constants
    assert_eq!(plan.header.total_objects, 12);
    assert_eq!(plan.header.arrival_lead_ms, 12.0);
    assert_eq!(plan.header.post_hit_dwell_ms, 8.0);
    assert_eq!(plan.header.stream_alt_threshold_ms, 125.0);
    assert_eq!(plan.header.slider_hold_tail_pad_ms, 10.0);
    assert_eq!(plan.header.spinner_radius_osupx, 50.0);
    assert_eq!(plan.header.spinner_rps, 7.5);
    assert_eq!(plan.header.straight_line_threshold_ms, 80.0);

    // Verify key schedule
    assert_eq!(plan.key_schedule.len(), 24);
    assert_eq!(plan.key_schedule[0].key, KeyCode::K1);
    assert_eq!(plan.key_schedule[0].time_ms, 24.0);
    assert_eq!(plan.key_schedule[1].time_ms, 32.0); // 24 + 8 (POST_HIT_DWELL_MS)
  }

  #[test]
  fn test_forced_key_switch_after_slider_hold() {
    // Slider followed immediately by a circle
    let objects = vec![
      HitObject::Slider {
        x: 100.0,
        y: 100.0,
        time_ms: 100.0,
        curve_type: crate::model::CurveType::Linear,
        control_points: vec![(200.0, 100.0)],
        repeat_count: 1,
        pixel_length: 100.0,
        span_duration_ms: 200.0,
        duration_ms: 200.0,
        end_time_ms: 300.0,
      },
      HitObject::Circle {
        x: 250.0,
        y: 100.0,
        time_ms: 500.0, // gap > stream threshold, BUT must force switch because prev was slider!
      },
    ];

    let schedule = synthesize_key_schedule(&objects);
    assert_eq!(schedule[0].key, KeyCode::K1, "slider should use K1");
    // Next note after slider must force switch to K2!
    assert_eq!(schedule[2].key, KeyCode::K2, "first note after slider must switch to K2");
  }
}
