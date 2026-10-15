use std::path::Path;

use anyhow::{Context, Result};
use rosu_map::Beatmap;
use rosu_map::section::hit_objects::{HitObjectKind, SplineType};

use crate::model::{BeatmapModel, CurveType, Difficulty, HitObject, TimingPoint};

pub fn parse_beatmap_file(path: impl AsRef<Path>) -> Result<BeatmapModel> {
  let path = path.as_ref();
  let content = std::fs::read_to_string(path).with_context(|| format!("failed to read beatmap file at {}", path.display()))?;
  parse_beatmap_str(&content)
}

pub fn parse_beatmap_str(content: &str) -> Result<BeatmapModel> {
  let beatmap = Beatmap::from_bytes(content.as_bytes()).map_err(|error| anyhow::anyhow!("rosu-map parse error: {error}"))?;

  let difficulty = Difficulty {
    hp_drain_rate: beatmap.hp_drain_rate,
    circle_size: beatmap.circle_size,
    overall_difficulty: beatmap.overall_difficulty,
    approach_rate: beatmap.approach_rate,
    slider_multiplier: beatmap.slider_multiplier,
    slider_tick_rate: beatmap.slider_tick_rate,
  };

  let mut timing_points = Vec::with_capacity(beatmap.control_points.timing_points.len() + beatmap.control_points.difficulty_points.len());

  for tp in &beatmap.control_points.timing_points {
    timing_points.push(TimingPoint {
      time_ms: tp.time,
      beat_length: tp.beat_len,
      meter: tp.time_signature.numerator.get(),
      uninherited: true,
    });
  }

  for dp in &beatmap.control_points.difficulty_points {
    let beat_len = if dp.slider_velocity.abs() > 1e-6 {
      -100.0 / dp.slider_velocity
    } else {
      -100.0
    };
    timing_points.push(TimingPoint {
      time_ms: dp.time,
      beat_length: beat_len,
      meter: 4,
      uninherited: false,
    });
  }

  // Sort timing points by time
  timing_points.sort_by(|a, b| a.time_ms.partial_cmp(&b.time_ms).unwrap_or(std::cmp::Ordering::Equal));

  // Temporary model to allow timing lookups while calculating slider durations
  let temp_model = BeatmapModel {
    format_version: beatmap.format_version.max(0) as u32,
    title: beatmap.title.clone(),
    artist: beatmap.artist.clone(),
    creator: beatmap.creator.clone(),
    version: beatmap.version.clone(),
    stack_leniency: beatmap.stack_leniency,
    difficulty: difficulty.clone(),
    timing_points: timing_points.clone(),
    hit_objects: Vec::new(),
  };

  let mut hit_objects = Vec::with_capacity(beatmap.hit_objects.len());
  for obj in &beatmap.hit_objects {
    let start_time_ms = obj.start_time;

    match &obj.kind {
      HitObjectKind::Circle(circle) => {
        hit_objects.push(HitObject::Circle {
          x: f64::from(circle.pos.x),
          y: f64::from(circle.pos.y),
          time_ms: start_time_ms,
        });
      }
      HitObjectKind::Slider(slider) => {
        let (beat_length_base, sv_multiplier) = temp_model.timing_at(start_time_ms);
        let repeat_count = slider.span_count() as usize;
        let pixel_length = slider.path.expected_dist().unwrap_or(0.0);

        // Calculate slider duration
        // velocity (px/ms) = (100 * slider_multiplier * sv_multiplier) / beat_length_base
        let velocity_px_per_ms = if slider.velocity > 0.0 {
          slider.velocity
        } else {
          (100.0 * difficulty.slider_multiplier * sv_multiplier) / beat_length_base
        };

        let span_duration_ms = if velocity_px_per_ms > 0.0 {
          pixel_length / velocity_px_per_ms
        } else {
          0.0
        };
        let duration_ms = span_duration_ms * (repeat_count as f64);
        let end_time_ms = start_time_ms + duration_ms;

        let kind = slider.path.control_points().first().and_then(|p| p.path_type).map(|pt| pt.kind);

        let curve_type = match kind {
          Some(SplineType::Linear) => CurveType::Linear,
          Some(SplineType::PerfectCurve) => CurveType::PerfectCircle,
          Some(SplineType::BSpline) => CurveType::Bezier,
          Some(SplineType::Catmull) | None => CurveType::Catmull,
        };

        let control_points: Vec<(f64, f64)> =
          slider.path.control_points().iter().map(|p| (f64::from(slider.pos.x + p.pos.x), f64::from(slider.pos.y + p.pos.y))).collect();

        hit_objects.push(HitObject::Slider {
          x: f64::from(slider.pos.x),
          y: f64::from(slider.pos.y),
          time_ms: start_time_ms,
          curve_type,
          control_points,
          repeat_count,
          pixel_length,
          span_duration_ms,
          duration_ms,
          end_time_ms,
        });
      }
      HitObjectKind::Spinner(spinner) => {
        let duration_ms = spinner.duration;
        let end_time_ms = start_time_ms + duration_ms;
        hit_objects.push(HitObject::Spinner {
          x: f64::from(spinner.pos.x),
          y: f64::from(spinner.pos.y),
          time_ms: start_time_ms,
          end_time_ms,
          duration_ms,
        });
      }
      HitObjectKind::Hold(_) => {
        // Mania hold note, ignored for standard mode
      }
    }
  }

  // Sort hit objects chronologically
  hit_objects.sort_by(|a, b| a.start_time_ms().partial_cmp(&b.start_time_ms()).unwrap_or(std::cmp::Ordering::Equal));

  // Apply StackLeniency offsets so click target = rendered position
  crate::model::apply_stacking(&mut hit_objects, beatmap.stack_leniency, difficulty.circle_size, difficulty.approach_rate);

  Ok(BeatmapModel {
    format_version: beatmap.format_version.max(0) as u32,
    title: beatmap.title,
    artist: beatmap.artist,
    creator: beatmap.creator,
    version: beatmap.version,
    stack_leniency: beatmap.stack_leniency,
    difficulty,
    timing_points,
    hit_objects,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_sample_beatmap() {
    let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/sample-beatmap-osu.osu");
    let model = parse_beatmap_file(fixture_path).expect("must parse sample beatmap");

    // Reconciliation checks
    assert_eq!(model.circle_count(), 6, "circle count mismatch");
    assert_eq!(model.slider_count(), 5, "slider count mismatch");
    assert_eq!(model.spinner_count(), 1, "spinner count mismatch");
    assert_eq!(model.total_objects(), 12, "total objects mismatch");

    assert_eq!(model.first_object_time_ms(), Some(24.0));

    // Slider 1: 729ms, len ~360.0, base beat_len 352.941176, SV 1.0 -> duration 352.94ms
    if let HitObject::Slider {
      time_ms,
      curve_type,
      duration_ms,
      pixel_length,
      repeat_count,
      ..
    } = &model.hit_objects[4]
    {
      assert_eq!(*time_ms, 729.0);
      assert_eq!(*curve_type, CurveType::PerfectCircle);
      assert_eq!(*repeat_count, 1);
      assert!((*pixel_length - 360.0).abs() < 1e-3);
      assert!((*duration_ms - 352.941).abs() < 0.1, "slider 1 duration was {}", duration_ms);
    } else {
      panic!("expected slider at index 4");
    }

    // Slider 2: 1259ms, len ~450.0 -> duration 441.176ms
    if let HitObject::Slider {
      time_ms,
      curve_type,
      duration_ms,
      repeat_count,
      ..
    } = &model.hit_objects[5]
    {
      assert_eq!(*time_ms, 1259.0);
      assert_eq!(*curve_type, CurveType::PerfectCircle);
      assert_eq!(*repeat_count, 1);
      assert!((*duration_ms - 441.176).abs() < 0.1, "slider 2 duration was {}", duration_ms);
    } else {
      panic!("expected slider at index 5");
    }

    // Slider 4: 2406ms, len ~990.0, Bezier -> duration 970.588ms
    if let HitObject::Slider {
      time_ms,
      curve_type,
      duration_ms,
      repeat_count,
      ..
    } = &model.hit_objects[7]
    {
      assert_eq!(*time_ms, 2406.0);
      assert_eq!(*curve_type, CurveType::Bezier);
      assert_eq!(*repeat_count, 1);
      assert!((*duration_ms - 970.588).abs() < 0.1, "slider 4 duration was {}", duration_ms);
    } else {
      panic!("expected slider at index 7");
    }

    // Spinner: 3553ms -> 4259ms, duration 706ms
    if let HitObject::Spinner {
      time_ms,
      end_time_ms,
      duration_ms,
      ..
    } = &model.hit_objects[8]
    {
      assert_eq!(*time_ms, 3553.0);
      assert_eq!(*end_time_ms, 4259.0);
      assert_eq!(*duration_ms, 706.0);
    } else {
      panic!("expected spinner at index 8");
    }
  }

  #[test]
  fn test_red_timing_point_resets_sv_multiplier() {
    let model = BeatmapModel {
      format_version: 14,
      title: "Test".to_string(),
      artist: "Test".to_string(),
      creator: "Test".to_string(),
      version: "Test".to_string(),
      stack_leniency: 0.7,
      difficulty: Difficulty {
        hp_drain_rate: 5.0,
        circle_size: 4.0,
        overall_difficulty: 5.0,
        approach_rate: 5.0,
        slider_multiplier: 1.4,
        slider_tick_rate: 1.0,
      },
      timing_points: vec![
        TimingPoint {
          time_ms: 0.0,
          beat_length: 500.0,
          meter: 4,
          uninherited: true,
        },
        TimingPoint {
          time_ms: 1000.0,
          beat_length: -50.0,
          meter: 4,
          uninherited: false,
        }, // 2.0x SV
        TimingPoint {
          time_ms: 2000.0,
          beat_length: 400.0,
          meter: 4,
          uninherited: true,
        }, // Red point resets SV to 1.0x
      ],
      hit_objects: vec![],
    };

    let (_, sv1) = model.timing_at(500.0);
    assert_eq!(sv1, 1.0, "SV before green point must be 1.0");

    let (_, sv2) = model.timing_at(1500.0);
    assert_eq!(sv2, 2.0, "SV after green point must be 2.0");

    let (_, sv3) = model.timing_at(2500.0);
    assert_eq!(sv3, 1.0, "SV after subsequent red point must be reset to 1.0");
  }

  #[test]
  fn test_green_timing_point_encodes_negative_100_over_sv() {
    let tp1 = TimingPoint {
      time_ms: 100.0,
      beat_length: -50.0,
      meter: 4,
      uninherited: false,
    };
    assert_eq!(tp1.slider_velocity_multiplier(), 2.0);

    let tp2 = TimingPoint {
      time_ms: 200.0,
      beat_length: -200.0,
      meter: 4,
      uninherited: false,
    };
    assert_eq!(tp2.slider_velocity_multiplier(), 0.5);

    let tp3 = TimingPoint {
      time_ms: 300.0,
      beat_length: -100.0,
      meter: 4,
      uninherited: false,
    };
    assert_eq!(tp3.slider_velocity_multiplier(), 1.0);
  }

  #[test]
  fn test_bezier_red_anchors_split_segments() {
    use crate::curve::SliderCurve;
    let curve = SliderCurve::new(CurveType::Bezier, (100.0, 100.0), &[(200.0, 100.0), (200.0, 100.0), (300.0, 200.0)], 250.0);
    let p = curve.position_at_distance(100.0);
    assert!((p.0 - 200.0).abs() < 1e-3);
    assert!((p.1 - 100.0).abs() < 1e-3);
  }

  #[test]
  fn test_perfect_circle_collinear_degenerates_to_linear() {
    use crate::curve::SliderCurve;
    let curve = SliderCurve::new(CurveType::PerfectCircle, (100.0, 100.0), &[(200.0, 200.0), (300.0, 300.0)], 282.84);
    let p_mid = curve.position_at_distance(141.42);
    assert!((p_mid.0 - 200.0).abs() < 1.0);
    assert!((p_mid.1 - 200.0).abs() < 1.0);
  }

  #[test]
  fn test_stack_leniency_applies_rendered_offset() {
    let mut objects = vec![
      HitObject::Circle {
        x: 200.0,
        y: 200.0,
        time_ms: 100.0,
      },
      HitObject::Circle {
        x: 200.0,
        y: 200.0,
        time_ms: 250.0,
      },
    ];
    crate::model::apply_stacking(&mut objects, 0.7, 5.0, 5.0);

    assert_eq!(objects[0].start_pos(), (200.0, 200.0));
    let pos1 = objects[1].start_pos();
    assert!(pos1.0 < 200.0, "stacked object x must be shifted: {}", pos1.0);
    assert!(pos1.1 < 200.0, "stacked object y must be shifted: {}", pos1.1);
  }
}
