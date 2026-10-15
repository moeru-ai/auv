use std::collections::HashSet;
use std::f64::consts::PI;

use serde::{Deserialize, Serialize};

use crate::curve::SliderCurve;
use crate::model::{BeatmapModel, HitObject};
use crate::synthesizer::{ActionPlan, CursorTrajectoryPoint, KeyCode, KeyEventType, ScheduledKeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Judgement {
  Hit300,
  Hit100,
  Hit50,
  Miss,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectJudgement {
  pub object_index: usize,
  pub kind: String,
  pub scheduled_time_ms: f64,
  pub actual_hit_time_ms: Option<f64>,
  pub timing_error_ms: Option<f64>,
  pub spatial_error_px: Option<f64>,
  pub judgement: Judgement,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimulationReport {
  pub map_name: String,
  pub total_objects: usize,
  pub count_300: usize,
  pub count_100: usize,
  pub count_50: usize,
  pub count_miss: usize,
  pub accuracy: f64,
  pub mean_absolute_timing_error_ms: f64,
  pub max_timing_error_ms: f64,
  pub judgements: Vec<ObjectJudgement>,
}

pub fn simulate_plan_offline(model: &BeatmapModel, plan: &ActionPlan) -> SimulationReport {
  let hit_window_300 = model.difficulty.hit_window_300();
  let hit_window_100 = model.difficulty.hit_window_100();
  let hit_window_50 = model.difficulty.hit_window_50();
  let hit_radius = model.difficulty.circle_radius_px();

  let mut judgements = Vec::with_capacity(model.hit_objects.len());
  let mut total_timing_err = 0.0;
  let mut hit_count = 0;
  let mut max_timing_err = 0.0f64;
  let mut consumed_keys: HashSet<usize> = HashSet::new();

  for (idx, obj) in model.hit_objects.iter().enumerate() {
    let target_time = obj.start_time_ms();
    let target_pos = obj.start_pos();

    // Find the closest unconsumed key down event within the outer hit window (hit_window_50)
    let candidate_key = plan
      .key_schedule
      .iter()
      .enumerate()
      .filter(|(k_idx, e)| e.event_type == KeyEventType::Down && !consumed_keys.contains(k_idx))
      .filter(|(_, e)| (e.time_ms - target_time).abs() <= hit_window_50)
      .min_by(|(_, a), (_, b)| {
        let diff_a = (a.time_ms - target_time).abs();
        let diff_b = (b.time_ms - target_time).abs();
        diff_a.partial_cmp(&diff_b).unwrap_or(std::cmp::Ordering::Equal)
      });

    let mut object_judgement = Judgement::Miss;
    let mut actual_hit_time = None;
    let mut timing_error = None;
    let mut spatial_error = None;

    if let Some((key_idx, key_event)) = candidate_key {
      // Consume key event so it cannot be matched by any subsequent object
      consumed_keys.insert(key_idx);

      let dt = (key_event.time_ms - target_time).abs();
      let err = key_event.time_ms - target_time;
      actual_hit_time = Some(key_event.time_ms);
      timing_error = Some(err);

      // Trajectory sampling: must exist, no silent fallback to target_pos
      let cursor_pos_opt = sample_trajectory_at(&plan.trajectory, key_event.time_ms);

      match obj {
        HitObject::Circle { .. } => {
          if let Some(cursor_pos) = cursor_pos_opt {
            let dist = (cursor_pos.0 - target_pos.0).hypot(cursor_pos.1 - target_pos.1);
            spatial_error = Some(dist);

            if dist <= hit_radius {
              if dt <= hit_window_300 {
                object_judgement = Judgement::Hit300;
              } else if dt <= hit_window_100 {
                object_judgement = Judgement::Hit100;
              } else {
                object_judgement = Judgement::Hit50;
              }
            }
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
          if let Some(cursor_pos) = cursor_pos_opt {
            let dist = (cursor_pos.0 - target_pos.0).hypot(cursor_pos.1 - target_pos.1);
            spatial_error = Some(dist);

            if dist <= hit_radius {
              let head_score = if dt <= hit_window_300 {
                Judgement::Hit300
              } else if dt <= hit_window_100 {
                Judgement::Hit100
              } else {
                Judgement::Hit50
              };

              let curve = SliderCurve::new(*curve_type, (*x, *y), control_points, *pixel_length);
              let follow_radius = hit_radius * 2.4;

              // Sample tracking checkpoints across slider duration
              let mut checkpoints = Vec::new();
              let step_ms = 20.0f64;
              let mut cp_t = *time_ms + step_ms;
              while cp_t < *end_time_ms {
                checkpoints.push(cp_t);
                cp_t += step_ms;
              }
              checkpoints.push(*end_time_ms);

              let mut passed = 0;
              for &t in &checkpoints {
                let key_held = is_key_held_at(&plan.key_schedule, key_event.key, key_event.time_ms, t);
                let progress = if *duration_ms > 0.0 {
                  ((t - *time_ms) / *duration_ms).clamp(0.0, 1.0)
                } else {
                  1.0
                };
                let ball_pos = curve.position_at_slide_progress(progress, *repeat_count, *pixel_length);
                let cursor_at_cp = sample_trajectory_at(&plan.trajectory, t);
                let on_slider = match cursor_at_cp {
                  Some((cx, cy)) => (cx - ball_pos.0).hypot(cy - ball_pos.1) <= follow_radius,
                  None => false,
                };

                if key_held && on_slider {
                  passed += 1;
                }
              }

              let tracking_ratio = (passed as f64) / (checkpoints.len() as f64);
              if tracking_ratio >= 0.999 {
                object_judgement = head_score;
              } else if tracking_ratio >= 0.75 {
                object_judgement = match head_score {
                  Judgement::Hit300 => Judgement::Hit100,
                  other => other,
                };
              } else if tracking_ratio >= 0.50 {
                object_judgement = Judgement::Hit50;
              } else {
                object_judgement = Judgement::Miss;
              }
            }
          }
        }
        HitObject::Spinner {
          x,
          y,
          time_ms,
          end_time_ms,
          duration_ms,
        } => {
          let key_held = is_key_held_at(&plan.key_schedule, key_event.key, key_event.time_ms, *end_time_ms - 5.0);

          let center = (*x, *y);
          let mut total_rotation = 0.0f64;
          let mut prev_angle: Option<f64> = None;

          let mut t = *time_ms;
          let step = 8.0f64;
          while t <= *end_time_ms {
            if let Some((cx, cy)) = sample_trajectory_at(&plan.trajectory, t) {
              let dx = cx - center.0;
              let dy = cy - center.1;
              let angle = dy.atan2(dx);
              if let Some(prev) = prev_angle {
                let mut d = angle - prev;
                while d > PI {
                  d -= 2.0 * PI;
                }
                while d < -PI {
                  d += 2.0 * PI;
                }
                total_rotation += d.abs();
              }
              prev_angle = Some(angle);
            }
            t += step;
          }

          let rotations = total_rotation / (2.0 * PI);
          let required_rotations = (*duration_ms / 1000.0) * 3.0;

          if key_held && rotations >= required_rotations {
            object_judgement = Judgement::Hit300;
          } else if key_held && rotations >= 0.5 * required_rotations {
            object_judgement = Judgement::Hit100;
          } else if key_held && rotations > 0.1 * required_rotations {
            object_judgement = Judgement::Hit50;
          } else {
            object_judgement = Judgement::Miss;
          }
        }
      }

      if object_judgement != Judgement::Miss {
        total_timing_err += err.abs();
        hit_count += 1;
        if err.abs() > max_timing_err {
          max_timing_err = err.abs();
        }
      }
    }

    judgements.push(ObjectJudgement {
      object_index: idx,
      kind: obj.kind_str().to_string(),
      scheduled_time_ms: target_time,
      actual_hit_time_ms: actual_hit_time,
      timing_error_ms: timing_error,
      spatial_error_px: spatial_error,
      judgement: object_judgement,
    });
  }

  let count_300 = judgements.iter().filter(|j| j.judgement == Judgement::Hit300).count();
  let count_100 = judgements.iter().filter(|j| j.judgement == Judgement::Hit100).count();
  let count_50 = judgements.iter().filter(|j| j.judgement == Judgement::Hit50).count();
  let count_miss = judgements.iter().filter(|j| j.judgement == Judgement::Miss).count();

  let total_scored = (count_300 + count_100 + count_50 + count_miss) as f64;
  let accuracy = if total_scored > 0.0 {
    (300.0 * (count_300 as f64) + 100.0 * (count_100 as f64) + 50.0 * (count_50 as f64)) / (300.0 * total_scored)
  } else {
    0.0
  };

  let mean_timing_err = if hit_count > 0 {
    total_timing_err / (hit_count as f64)
  } else {
    0.0
  };

  SimulationReport {
    map_name: format!("{} - {} [{}]", model.artist, model.title, model.version),
    total_objects: model.total_objects(),
    count_300,
    count_100,
    count_50,
    count_miss,
    accuracy,
    mean_absolute_timing_error_ms: mean_timing_err,
    max_timing_error_ms: max_timing_err,
    judgements,
  }
}

fn is_key_held_at(schedule: &[ScheduledKeyEvent], key: KeyCode, down_time: f64, check_time: f64) -> bool {
  let has_released =
    schedule.iter().any(|e| e.key == key && e.event_type == KeyEventType::Up && e.time_ms >= down_time && e.time_ms < check_time - 1e-4);
  !has_released
}

pub fn sample_trajectory_at(trajectory: &[CursorTrajectoryPoint], time_ms: f64) -> Option<(f64, f64)> {
  if trajectory.is_empty() {
    return None;
  }
  let first_t = trajectory[0].time_ms;
  let last_t = trajectory.last().unwrap().time_ms;

  if time_ms < first_t - 50.0 || time_ms > last_t + 50.0 {
    return None;
  }
  if time_ms <= first_t {
    return Some((trajectory[0].x, trajectory[0].y));
  }
  if time_ms >= last_t {
    let p = trajectory.last().unwrap();
    return Some((p.x, p.y));
  }

  let idx = match trajectory.binary_search_by(|p| p.time_ms.partial_cmp(&time_ms).unwrap_or(std::cmp::Ordering::Equal)) {
    Ok(i) => return Some((trajectory[i].x, trajectory[i].y)),
    Err(i) => i.saturating_sub(1),
  };

  if idx >= trajectory.len() - 1 {
    let p = trajectory.last().unwrap();
    return Some((p.x, p.y));
  }

  let p0 = &trajectory[idx];
  let p1 = &trajectory[idx + 1];
  let dt = p1.time_ms - p0.time_ms;
  if dt <= 0.0 {
    return Some((p0.x, p0.y));
  }
  let t = (time_ms - p0.time_ms) / dt;
  Some((p0.x + t * (p1.x - p0.x), p0.y + t * (p1.y - p0.y)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parser::parse_beatmap_file;
  use crate::synthesizer::synthesize_plan;

  #[test]
  fn test_simulator_on_synthetic_plan_achieves_perfect_score() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let plan = synthesize_plan(&model);

    // Plan itself must be strictly valid
    plan.validate_plan().expect("synthesized plan must be valid");

    let report = simulate_plan_offline(&model, &plan);

    assert_eq!(report.total_objects, 12);
    assert_eq!(report.count_300, 12, "all 12 objects must hit 300 on synthetic plan");
    assert_eq!(report.count_100, 0);
    assert_eq!(report.count_50, 0);
    assert_eq!(report.count_miss, 0);
    assert!((report.accuracy - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_simulator_on_tear_rain_fixture_achieves_perfect_score() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/351190-cYsmix-Tear-Rain-Normal.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse real tear rain beatmap");
    let plan = synthesize_plan(&model);

    plan.validate_plan().expect("tear rain plan must be valid");

    let report = simulate_plan_offline(&model, &plan);

    assert_eq!(report.total_objects, 295);
    assert_eq!(report.count_300, 295, "all 295 objects must hit 300 on synthetic plan");
    assert_eq!(report.count_miss, 0);
    assert!((report.accuracy - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_negative_missing_trajectory_fails_all_objects() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Delete all mouse trajectory points
    plan.trajectory.clear();

    let report = simulate_plan_offline(&model, &plan);
    assert_eq!(report.count_300, 0, "no 300s allowed without mouse movement");
    assert_eq!(report.count_miss, model.total_objects(), "all objects must miss without trajectory");
    assert_eq!(report.accuracy, 0.0);
  }

  #[test]
  fn test_negative_missing_keydowns_causes_misses() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Remove the first 2 KeyDown events
    let mut removed = 0;
    plan.key_schedule.retain(|e| {
      if e.event_type == KeyEventType::Down && removed < 2 {
        removed += 1;
        false
      } else {
        true
      }
    });

    let report = simulate_plan_offline(&model, &plan);
    assert_eq!(report.count_miss, 2, "2 removed keydowns must result in exactly 2 misses");
    assert_eq!(report.count_300, 10);
    assert!(report.accuracy < 1.0);
  }

  #[test]
  fn test_negative_single_keydown_cannot_score_multiple_objects() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Keep only 1 KeyDown in the entire schedule
    let first_down = plan.key_schedule.iter().find(|e| e.event_type == KeyEventType::Down).cloned().unwrap();
    plan.key_schedule.retain(|e| e.event_type != KeyEventType::Down);
    plan.key_schedule.push(first_down);
    plan.key_schedule.sort_by(|a, b| a.time_ms.partial_cmp(&b.time_ms).unwrap());

    let report = simulate_plan_offline(&model, &plan);
    assert_eq!(report.count_300, 1, "single keydown can score at most 1 object");
    assert_eq!(report.count_miss, 11);
  }

  #[test]
  fn test_negative_slider_off_track_loses_tracking() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Slider 1 is at 729ms - 1081ms. Move cursor far away during its body:
    for pt in &mut plan.trajectory {
      if pt.time_ms > 750.0 && pt.time_ms < 1050.0 {
        pt.x += 1000.0;
        pt.y += 1000.0;
      }
    }

    let report = simulate_plan_offline(&model, &plan);
    assert!(report.count_300 < 12, "slider deviating off track cannot score 300");
    assert!(report.count_miss > 0 || report.count_100 > 0 || report.count_50 > 0);
  }

  #[test]
  fn test_negative_slider_premature_release_loses_tracking() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Release the key prematurely right after head hit (729ms -> release at 740ms, slider ends at ~1081ms)
    for e in &mut plan.key_schedule {
      if e.event_type == KeyEventType::Up && e.time_ms > 729.0 && e.time_ms < 1150.0 {
        e.time_ms = 735.0;
      }
    }

    let report = simulate_plan_offline(&model, &plan);
    assert!(report.count_300 < 12, "slider released prematurely cannot score 300");
  }

  #[test]
  fn test_negative_spinner_no_rotation_misses() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Spinner is at index 8 (3553ms -> 4259ms). Keep cursor completely still:
    for pt in &mut plan.trajectory {
      if pt.time_ms >= 3550.0 && pt.time_ms <= 4260.0 {
        pt.x = 256.0;
        pt.y = 192.0;
      }
    }

    let report = simulate_plan_offline(&model, &plan);
    let spinner_j = report.judgements.iter().find(|j| j.kind == "spinner").unwrap();
    assert_eq!(spinner_j.judgement, Judgement::Miss, "spinner without rotation must miss");
  }

  #[test]
  fn test_negative_non_monotonic_trajectory_fails_validation() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Swap two trajectory timestamps to create a non-monotonic ordering
    if plan.trajectory.len() > 10 {
      plan.trajectory[5].time_ms = plan.trajectory[6].time_ms + 10.0;
    }

    let val = plan.validate_plan();
    assert!(val.is_err(), "non-monotonic trajectory must fail validation");
  }

  #[test]
  fn test_negative_100ms_latency_degrades_score() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("parse sample beatmap");
    let mut plan = synthesize_plan(&model);

    // Artificially inject 100ms latency to all key presses
    for e in &mut plan.key_schedule {
      e.time_ms += 100.0;
    }

    let report = simulate_plan_offline(&model, &plan);
    assert_eq!(report.count_300, 1, "only spinner without strict spatial constraint can hit");
    assert_eq!(report.count_miss, 11, "100ms key delay causes all circles and sliders to miss");
    assert!(report.accuracy < 0.15, "accuracy degrades to ~8%: got {}", report.accuracy);
  }
}
