use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
  /// Pure synthetic/offline model simulation without external hardware or capture
  SyntheticPredicted,
  /// Geometric derivation from layout rules without empirical screen sampling
  LayoutDerived,
  /// Static reconciliation against known map reference metadata
  StaticReferenceChecked,
  /// Verified empirical measurement from hardware or real screen capture
  EmpiricallyMeasured,
}

impl std::fmt::Display for EvidenceLevel {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::SyntheticPredicted => write!(f, "SYNTHETIC_PREDICTED"),
      Self::LayoutDerived => write!(f, "LAYOUT_DERIVED"),
      Self::StaticReferenceChecked => write!(f, "STATIC_REFERENCE_CHECKED"),
      Self::EmpiricallyMeasured => write!(f, "EMPIRICALLY_MEASURED"),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveType {
  Linear,
  PerfectCircle,
  Bezier,
  Catmull,
}

impl CurveType {
  pub fn from_char(c: char) -> Option<Self> {
    match c {
      'L' | 'l' => Some(Self::Linear),
      'P' | 'p' => Some(Self::PerfectCircle),
      'B' | 'b' => Some(Self::Bezier),
      'C' | 'c' => Some(Self::Catmull),
      _ => None,
    }
  }

  pub fn as_char(&self) -> char {
    match self {
      Self::Linear => 'L',
      Self::PerfectCircle => 'P',
      Self::Bezier => 'B',
      Self::Catmull => 'C',
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Difficulty {
  pub hp_drain_rate: f32,
  pub circle_size: f32,
  pub overall_difficulty: f32,
  pub approach_rate: f32,
  pub slider_multiplier: f64,
  pub slider_tick_rate: f64,
}

impl Difficulty {
  /// Hit window for 300 judgement in ms (+/-).
  pub fn hit_window_300(&self) -> f64 {
    80.0 - 6.0 * f64::from(self.overall_difficulty)
  }

  /// Hit window for 100 judgement in ms (+/-).
  pub fn hit_window_100(&self) -> f64 {
    140.0 - 8.0 * f64::from(self.overall_difficulty)
  }

  /// Hit window for 50 judgement in ms (+/-).
  pub fn hit_window_50(&self) -> f64 {
    200.0 - 10.0 * f64::from(self.overall_difficulty)
  }

  /// Hit object circle radius in playfield pixels (512x384 space).
  pub fn circle_radius_px(&self) -> f64 {
    54.4 - 4.48 * f64::from(self.circle_size)
  }

  /// Approach time in ms from appearance to hit time.
  pub fn approach_time_ms(&self) -> f64 {
    let ar = f64::from(self.approach_rate);
    if ar < 5.0 {
      1200.0 + 600.0 * (5.0 - ar) / 5.0
    } else if (ar - 5.0).abs() < 1e-4 {
      1200.0
    } else {
      1200.0 - 750.0 * (ar - 5.0) / 5.0
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingPoint {
  pub time_ms: f64,
  pub beat_length: f64,
  pub meter: u32,
  pub uninherited: bool,
}

impl TimingPoint {
  pub fn bpm(&self) -> f64 {
    if self.uninherited && self.beat_length > 0.0 {
      60_000.0 / self.beat_length
    } else {
      0.0
    }
  }

  pub fn slider_velocity_multiplier(&self) -> f64 {
    if !self.uninherited && self.beat_length < 0.0 {
      (-100.0 / self.beat_length).clamp(0.1, 10.0)
    } else {
      1.0
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitObject {
  Circle {
    x: f64,
    y: f64,
    time_ms: f64,
  },
  Slider {
    x: f64,
    y: f64,
    time_ms: f64,
    curve_type: CurveType,
    control_points: Vec<(f64, f64)>,
    repeat_count: usize,
    pixel_length: f64,
    span_duration_ms: f64,
    duration_ms: f64,
    end_time_ms: f64,
  },
  Spinner {
    x: f64,
    y: f64,
    time_ms: f64,
    end_time_ms: f64,
    duration_ms: f64,
  },
}

impl HitObject {
  pub fn start_time_ms(&self) -> f64 {
    match self {
      Self::Circle { time_ms, .. } => *time_ms,
      Self::Slider { time_ms, .. } => *time_ms,
      Self::Spinner { time_ms, .. } => *time_ms,
    }
  }

  pub fn end_time_ms(&self) -> f64 {
    match self {
      Self::Circle { time_ms, .. } => *time_ms,
      Self::Slider { end_time_ms, .. } => *end_time_ms,
      Self::Spinner { end_time_ms, .. } => *end_time_ms,
    }
  }

  pub fn start_pos(&self) -> (f64, f64) {
    match self {
      Self::Circle { x, y, .. } => (*x, *y),
      Self::Slider { x, y, .. } => (*x, *y),
      Self::Spinner { x, y, .. } => (*x, *y),
    }
  }

  pub fn kind_str(&self) -> &'static str {
    match self {
      Self::Circle { .. } => "circle",
      Self::Slider { .. } => "slider",
      Self::Spinner { .. } => "spinner",
    }
  }

  pub fn apply_offset(&mut self, dx: f64, dy: f64) {
    match self {
      Self::Circle { x, y, .. } => {
        *x += dx;
        *y += dy;
      }
      Self::Slider {
        x,
        y,
        control_points,
        ..
      } => {
        *x += dx;
        *y += dy;
        for cp in control_points.iter_mut() {
          cp.0 += dx;
          cp.1 += dy;
        }
      }
      Self::Spinner { x, y, .. } => {
        *x += dx;
        *y += dy;
      }
    }
  }
}

pub fn apply_stacking(hit_objects: &mut [HitObject], stack_leniency: f32, circle_size: f32, approach_rate: f32) {
  if stack_leniency <= 0.0 || hit_objects.is_empty() {
    return;
  }
  let ar = f64::from(approach_rate);
  let approach_time_ms = if ar < 5.0 {
    1200.0 + 600.0 * (5.0 - ar) / 5.0
  } else {
    1200.0 - 750.0 * (ar - 5.0) / 5.0
  };
  let stack_threshold_ms = f64::from(stack_leniency) * approach_time_ms;
  let stack_scale = (1.0 - 0.7 * (f64::from(circle_size) - 5.0) / 5.0) / 2.0;
  let stack_offset_step = -stack_scale * 6.4;

  let mut stack_heights = vec![0usize; hit_objects.len()];

  for i in (0..hit_objects.len()).rev() {
    let pos_i = hit_objects[i].start_pos();
    let end_time_i = hit_objects[i].end_time_ms();

    for j in (i + 1)..hit_objects.len() {
      let time_j = hit_objects[j].start_time_ms();
      if time_j - end_time_i > stack_threshold_ms {
        break;
      }
      let pos_j = hit_objects[j].start_pos();
      if (pos_i.0 - pos_j.0).hypot(pos_i.1 - pos_j.1) < 3.0 {
        stack_heights[j] = stack_heights[i] + 1;
      }
    }
  }

  for (obj, &height) in hit_objects.iter_mut().zip(stack_heights.iter()) {
    if height > 0 {
      let offset = (height as f64) * stack_offset_step;
      obj.apply_offset(offset, offset);
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatmapModel {
  pub format_version: u32,
  pub title: String,
  pub artist: String,
  pub creator: String,
  pub version: String,
  pub stack_leniency: f32,
  pub difficulty: Difficulty,
  pub timing_points: Vec<TimingPoint>,
  pub hit_objects: Vec<HitObject>,
}

impl BeatmapModel {
  pub fn circle_count(&self) -> usize {
    self.hit_objects.iter().filter(|o| matches!(o, HitObject::Circle { .. })).count()
  }

  pub fn slider_count(&self) -> usize {
    self.hit_objects.iter().filter(|o| matches!(o, HitObject::Slider { .. })).count()
  }

  pub fn spinner_count(&self) -> usize {
    self.hit_objects.iter().filter(|o| matches!(o, HitObject::Spinner { .. })).count()
  }

  pub fn total_objects(&self) -> usize {
    self.hit_objects.len()
  }

  pub fn first_object_time_ms(&self) -> Option<f64> {
    self.hit_objects.first().map(HitObject::start_time_ms)
  }

  pub fn last_object_time_ms(&self) -> Option<f64> {
    self.hit_objects.last().map(HitObject::end_time_ms)
  }

  /// Resolves the active base beat length and slider velocity multiplier at a given time.
  pub fn timing_at(&self, time_ms: f64) -> (f64, f64) {
    let uninherited_points: Vec<&TimingPoint> = self.timing_points.iter().filter(|tp| tp.uninherited).collect();
    let base_tp = uninherited_points.iter().rfind(|tp| tp.time_ms <= time_ms).copied().or_else(|| uninherited_points.first().copied());

    let beat_length_base = base_tp.map(|tp| tp.beat_length).unwrap_or(500.0);
    let base_time = base_tp.map(|tp| tp.time_ms).unwrap_or(0.0);

    let inherited_tp = self.timing_points.iter().rfind(|tp| !tp.uninherited && tp.time_ms >= base_time && tp.time_ms <= time_ms);

    let sv_mult = inherited_tp.map(TimingPoint::slider_velocity_multiplier).unwrap_or(1.0);
    (beat_length_base, sv_mult)
  }
}
