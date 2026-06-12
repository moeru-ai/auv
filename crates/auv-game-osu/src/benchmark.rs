use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rosu_map::Beatmap;
use rosu_map::section::hit_objects::HitObjectKind;
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use auv_driver::capture::Capture;
#[cfg(target_os = "macos")]
use auv_driver::{
  App, Click, ClickOptions, Driver, InputActionResult, InputPolicy, WindowClickStrategy,
  WindowPoint, WindowSelector,
};
#[cfg(target_os = "macos")]
use auv_driver_macos::MacosDriver;

pub type OsuResult<T> = Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
  DryRun,
  TypedDispatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkInputs {
  pub beatmap_path: PathBuf,
  pub output_dir: PathBuf,
  pub lead_in_ms: u64,
  pub run_mode: RunMode,
  pub target_app: Option<String>,
  pub dispatch_limit: Option<usize>,
  pub capture_verify: bool,
  pub post_capture_offsets_ms: Vec<u64>,
}

impl BenchmarkInputs {
  pub fn new(beatmap_path: PathBuf, output_dir: PathBuf) -> Self {
    Self {
      beatmap_path,
      output_dir,
      lead_in_ms: 25,
      run_mode: RunMode::DryRun,
      target_app: None,
      dispatch_limit: None,
      capture_verify: false,
      post_capture_offsets_ms: default_post_capture_offsets_ms(),
    }
  }

  pub fn typed_dispatch(
    beatmap_path: PathBuf,
    output_dir: PathBuf,
    target_app: impl Into<String>,
  ) -> Self {
    Self {
      beatmap_path,
      output_dir,
      lead_in_ms: 25,
      run_mode: RunMode::TypedDispatch,
      target_app: Some(target_app.into()),
      dispatch_limit: Some(8),
      capture_verify: false,
      post_capture_offsets_ms: default_post_capture_offsets_ms(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
  Circle,
  Slider,
  Spinner,
  Hold,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MapSummary {
  pub beatmap_path: String,
  pub mode: u8,
  pub total_objects: usize,
  pub circle_count: usize,
  pub slider_count: usize,
  pub spinner_count: usize,
  pub hold_count: usize,
  pub first_object_time_ms: Option<u64>,
  pub last_object_time_ms: Option<u64>,
  pub approach_rate: f32,
  pub overall_difficulty: f32,
  pub circle_size: f32,
  pub hp_drain_rate: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScheduledAction {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub x: f32,
  pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DispatchSample {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub actual_dispatch_time_ms: u64,
  pub dispatch_error_ms: i64,
  pub x: f32,
  pub y: f32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub delivery_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureSample {
  pub phase: CapturePhase,
  pub capture_time_ms: u64,
  pub relative_to_dispatch_ms: i64,
  pub file_name: String,
  pub width: u32,
  pub height: u32,
  pub backend: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePhase {
  BeforeDispatch,
  AfterDispatch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureTraceSample {
  pub object_index: usize,
  pub object_kind: ObjectKind,
  pub scheduled_time_ms: u64,
  pub actual_dispatch_time_ms: u64,
  pub dispatch_error_ms: i64,
  pub captures: Vec<CaptureSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerificationSummary {
  pub capture_enabled: bool,
  pub captured_action_count: usize,
  pub missing_frame_count: usize,
  pub max_capture_delay_ms: i64,
  pub suspicious_time_inversion_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LatencyReport {
  pub run_mode: RunMode,
  pub total_actions: usize,
  pub mean_error_ms: f64,
  pub p50_error_ms: i64,
  pub p95_error_ms: i64,
  pub p99_error_ms: i64,
  pub max_error_ms: i64,
  pub jitter_ms: i64,
  pub missed_schedule_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkOutput {
  pub map_summary: MapSummary,
  pub schedule: Vec<ScheduledAction>,
  pub dispatch_trace: Vec<DispatchSample>,
  pub capture_trace: Vec<CaptureTraceSample>,
  pub latency_report: LatencyReport,
  pub verification_summary: Option<VerificationSummary>,
  pub output_dir: PathBuf,
}

pub fn run_benchmark(inputs: &BenchmarkInputs) -> OsuResult<BenchmarkOutput> {
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create osu benchmark output dir {}: {error}",
      inputs.output_dir.display()
    )
  })?;

  let beatmap = Beatmap::from_path(&inputs.beatmap_path).map_err(|error| {
    format!(
      "failed to parse beatmap {}: {error}",
      inputs.beatmap_path.display()
    )
  })?;

  let schedule = build_schedule(&beatmap);
  if schedule.is_empty() {
    return Err(format!(
      "beatmap {} contains no hit objects",
      inputs.beatmap_path.display()
    ));
  }

  let map_summary = build_map_summary(&inputs.beatmap_path, &beatmap, &schedule);
  let (dispatch_trace, capture_trace, verification_summary) = match inputs.run_mode {
    RunMode::DryRun => (run_dry_schedule(&schedule, inputs.lead_in_ms), Vec::new(), None),
    RunMode::TypedDispatch => run_typed_dispatch(schedule.as_slice(), inputs)?,
  };
  let latency_report = build_latency_report(inputs.run_mode.clone(), &dispatch_trace);

  write_json(
    inputs.output_dir.join("parsed_map_summary.json"),
    &map_summary,
  )?;
  write_json(
    inputs.output_dir.join("action_schedule.json"),
    &schedule,
  )?;
  write_json(
    inputs.output_dir.join("dispatch_trace.json"),
    &dispatch_trace,
  )?;
  write_json(
    inputs.output_dir.join("latency_report.json"),
    &latency_report,
  )?;
  if inputs.capture_verify {
    write_json(
      inputs.output_dir.join("capture_trace.json"),
      &capture_trace,
    )?;
    write_json(
      inputs.output_dir.join("verification_summary.json"),
      &verification_summary,
    )?;
  }

  Ok(BenchmarkOutput {
    map_summary,
    schedule,
    dispatch_trace,
    capture_trace,
    latency_report,
    verification_summary,
    output_dir: inputs.output_dir.clone(),
  })
}

fn build_schedule(beatmap: &Beatmap) -> Vec<ScheduledAction> {
  beatmap
    .hit_objects
    .iter()
    .enumerate()
    .map(|(index, object)| {
      let (object_kind, x, y) = scheduled_target(&object.kind);
      ScheduledAction {
        object_index: index,
        object_kind,
        scheduled_time_ms: object.start_time.max(0.0).round() as u64,
        x,
        y,
      }
    })
    .collect()
}

fn build_map_summary(
  beatmap_path: &Path,
  beatmap: &Beatmap,
  schedule: &[ScheduledAction],
) -> MapSummary {
  let mut circle_count = 0usize;
  let mut slider_count = 0usize;
  let mut spinner_count = 0usize;
  let mut hold_count = 0usize;

  for action in schedule {
    match action.object_kind {
      ObjectKind::Circle => circle_count += 1,
      ObjectKind::Slider => slider_count += 1,
      ObjectKind::Spinner => spinner_count += 1,
      ObjectKind::Hold => hold_count += 1,
    }
  }

  MapSummary {
    beatmap_path: beatmap_path.display().to_string(),
    mode: beatmap.mode as u8,
    total_objects: schedule.len(),
    circle_count,
    slider_count,
    spinner_count,
    hold_count,
    first_object_time_ms: schedule.first().map(|action| action.scheduled_time_ms),
    last_object_time_ms: schedule.last().map(|action| action.scheduled_time_ms),
    approach_rate: beatmap.approach_rate,
    overall_difficulty: beatmap.overall_difficulty,
    circle_size: beatmap.circle_size,
    hp_drain_rate: beatmap.hp_drain_rate,
  }
}

fn run_dry_schedule(schedule: &[ScheduledAction], lead_in_ms: u64) -> Vec<DispatchSample> {
  let start = Instant::now() + Duration::from_millis(lead_in_ms);
  let mut trace = Vec::with_capacity(schedule.len());

  for action in schedule {
    wait_until_due(start, action.scheduled_time_ms);
    let actual_dispatch_time_ms = start.elapsed().as_millis() as u64;
    let dispatch_error_ms = actual_dispatch_time_ms as i64 - action.scheduled_time_ms as i64;
    trace.push(DispatchSample {
      object_index: action.object_index,
      object_kind: action.object_kind.clone(),
      scheduled_time_ms: action.scheduled_time_ms,
      actual_dispatch_time_ms,
      dispatch_error_ms,
      x: action.x,
      y: action.y,
      delivery_path: None,
      fallback_reason: None,
    });
  }

  trace
}

#[cfg(target_os = "macos")]
fn run_typed_dispatch(
  schedule: &[ScheduledAction],
  inputs: &BenchmarkInputs,
) -> OsuResult<(
  Vec<DispatchSample>,
  Vec<CaptureTraceSample>,
  Option<VerificationSummary>,
)> {
  let target_app = inputs
    .target_app
    .as_deref()
    .ok_or_else(|| "typed dispatch requires target_app".to_string())?;
  let dispatch_limit = inputs.dispatch_limit.unwrap_or(8).min(schedule.len());
  let driver = MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(WindowSelector::default().owned_by(App::name(target_app.to_string())).title_contains("osu"))
    .map_err(|error| error.to_string())?;
  let start = Instant::now() + Duration::from_millis(inputs.lead_in_ms);
  let mut trace = Vec::with_capacity(dispatch_limit);
  let mut capture_trace = if inputs.capture_verify {
    Vec::with_capacity(dispatch_limit)
  } else {
    Vec::new()
  };

  for action in schedule.iter().take(dispatch_limit) {
    let mut captures = Vec::new();
    if inputs.capture_verify {
      captures.push(capture_sample(
        &session,
        &window,
        &inputs.output_dir,
        action,
        CapturePhase::BeforeDispatch,
        &start,
        0,
      )?);
    }
    wait_until_due(start, action.scheduled_time_ms);
    let window_point = WindowPoint::new(f64::from(action.x), f64::from(action.y));
    let result = session
      .window()
      .click(
        &window,
        window_point,
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click: Click::Single,
          window_strategy: WindowClickStrategy::ChromiumCompatible,
        },
      )
      .map_err(|error| format!("typed dispatch failed at object {}: {error}", action.object_index))?;
    let actual_dispatch_time_ms = start.elapsed().as_millis() as u64;
    let dispatch_error_ms = actual_dispatch_time_ms as i64 - action.scheduled_time_ms as i64;
    trace.push(dispatch_sample_from_result(action, actual_dispatch_time_ms, dispatch_error_ms, result));

    if inputs.capture_verify {
      for offset_ms in &inputs.post_capture_offsets_ms {
        if *offset_ms > 0 {
          thread::sleep(Duration::from_millis(*offset_ms));
        }
        captures.push(capture_sample(
          &session,
          &window,
          &inputs.output_dir,
          action,
          CapturePhase::AfterDispatch,
          &start,
          *offset_ms,
        )?);
      }
      capture_trace.push(CaptureTraceSample {
        object_index: action.object_index,
        object_kind: action.object_kind.clone(),
        scheduled_time_ms: action.scheduled_time_ms,
        actual_dispatch_time_ms,
        dispatch_error_ms,
        captures,
      });
    }
  }

  let verification_summary = inputs
    .capture_verify
    .then(|| build_verification_summary(dispatch_limit, &capture_trace));

  Ok((trace, capture_trace, verification_summary))
}

#[cfg(target_os = "macos")]
fn capture_sample(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  output_dir: &Path,
  action: &ScheduledAction,
  phase: CapturePhase,
  start: &Instant,
  phase_offset_ms: u64,
) -> OsuResult<CaptureSample> {
  let capture = session
    .window()
    .capture(window)
    .map_err(|error| format!("window capture failed at object {}: {error}", action.object_index))?;
  capture_to_sample(output_dir, action, phase, start, phase_offset_ms, capture)
}

#[cfg(target_os = "macos")]
fn capture_to_sample(
  output_dir: &Path,
  action: &ScheduledAction,
  phase: CapturePhase,
  start: &Instant,
  phase_offset_ms: u64,
  capture: Capture,
) -> OsuResult<CaptureSample> {
  let capture_time_ms = start.elapsed().as_millis() as u64;
  let file_name = format!(
    "capture-object-{:04}-{}-{}ms.png",
    action.object_index,
    match phase {
      CapturePhase::BeforeDispatch => "before",
      CapturePhase::AfterDispatch => "after",
    },
    phase_offset_ms
  );
  let file_path = output_dir.join(&file_name);
  capture
    .image
    .save(&file_path)
    .map_err(|error| format!("failed to save {}: {error}", file_path.display()))?;
  Ok(CaptureSample {
    phase,
    capture_time_ms,
    relative_to_dispatch_ms: capture_time_ms as i64 - action.scheduled_time_ms as i64,
    file_name,
    width: capture.image.width(),
    height: capture.image.height(),
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

#[cfg(not(target_os = "macos"))]
fn run_typed_dispatch(
  _schedule: &[ScheduledAction],
  _inputs: &BenchmarkInputs,
) -> OsuResult<(
  Vec<DispatchSample>,
  Vec<CaptureTraceSample>,
  Option<VerificationSummary>,
)> {
  Err("typed dispatch is currently implemented only for macOS".to_string())
}

fn dispatch_sample_from_result(
  action: &ScheduledAction,
  actual_dispatch_time_ms: u64,
  dispatch_error_ms: i64,
  result: InputActionResult,
) -> DispatchSample {
  DispatchSample {
    object_index: action.object_index,
    object_kind: action.object_kind.clone(),
    scheduled_time_ms: action.scheduled_time_ms,
    actual_dispatch_time_ms,
    dispatch_error_ms,
    x: action.x,
    y: action.y,
    delivery_path: Some(format!("{:?}", result.selected_path)),
    fallback_reason: result.fallback_reason,
  }
}

fn wait_until_due(start: Instant, scheduled_time_ms: u64) {
  let due = start + Duration::from_millis(scheduled_time_ms);
  loop {
    let now = Instant::now();
    if now >= due {
      break;
    }
    let remaining = due.saturating_duration_since(now);
    if remaining > Duration::from_millis(2) {
      thread::sleep(Duration::from_millis(1));
    } else {
      std::hint::spin_loop();
    }
  }
}

fn build_latency_report(run_mode: RunMode, dispatch_trace: &[DispatchSample]) -> LatencyReport {
  let total_actions = dispatch_trace.len();
  let mut errors = dispatch_trace
    .iter()
    .map(|sample| sample.dispatch_error_ms)
    .collect::<Vec<_>>();
  errors.sort_unstable();

  let mean_error_ms = if total_actions == 0 {
    0.0
  } else {
    errors.iter().map(|error| *error as f64).sum::<f64>() / total_actions as f64
  };

  let p50_error_ms = percentile(&errors, 0.50);
  let p95_error_ms = percentile(&errors, 0.95);
  let p99_error_ms = percentile(&errors, 0.99);
  let max_error_ms = errors.iter().copied().max().unwrap_or(0);
  let jitter_ms = max_error_ms - errors.iter().copied().min().unwrap_or(0);
  let missed_schedule_count = dispatch_trace
    .iter()
    .filter(|sample| sample.dispatch_error_ms > 0)
    .count();

  LatencyReport {
    run_mode,
    total_actions,
    mean_error_ms,
    p50_error_ms,
    p95_error_ms,
    p99_error_ms,
    max_error_ms,
    jitter_ms,
    missed_schedule_count,
  }
}

fn build_verification_summary(
  expected_actions: usize,
  capture_trace: &[CaptureTraceSample],
) -> VerificationSummary {
  let captured_action_count = capture_trace.iter().filter(|sample| !sample.captures.is_empty()).count();
  let missing_frame_count: usize = capture_trace
    .iter()
    .map(|sample| sample.captures.iter().filter(|capture| capture.width == 0 || capture.height == 0).count())
    .sum();
  let max_capture_delay_ms = capture_trace
    .iter()
    .flat_map(|sample| sample.captures.iter().map(|capture| capture.relative_to_dispatch_ms))
    .max()
    .unwrap_or(0);
  let suspicious_time_inversion_count = capture_trace
    .iter()
    .flat_map(|sample| sample.captures.iter())
    .filter(|capture| matches!(capture.phase, CapturePhase::AfterDispatch) && capture.relative_to_dispatch_ms < 0)
    .count();

  VerificationSummary {
    capture_enabled: true,
    captured_action_count,
    missing_frame_count: missing_frame_count + expected_actions.saturating_sub(captured_action_count),
    max_capture_delay_ms,
    suspicious_time_inversion_count,
  }
}

fn percentile(sorted_errors: &[i64], percentile: f64) -> i64 {
  if sorted_errors.is_empty() {
    return 0;
  }

  let last_index = sorted_errors.len() - 1;
  let index = ((last_index as f64) * percentile).round() as usize;
  sorted_errors[index.min(last_index)]
}

fn scheduled_target(kind: &HitObjectKind) -> (ObjectKind, f32, f32) {
  match kind {
    HitObjectKind::Circle(circle) => (ObjectKind::Circle, circle.pos.x, circle.pos.y),
    HitObjectKind::Slider(slider) => (ObjectKind::Slider, slider.pos.x, slider.pos.y),
    HitObjectKind::Spinner(spinner) => (ObjectKind::Spinner, spinner.pos.x, spinner.pos.y),
    HitObjectKind::Hold(hold) => (ObjectKind::Hold, hold.pos_x, 192.0),
  }
}

fn write_json(path: PathBuf, value: &impl Serialize) -> OsuResult<()> {
  let mut rendered = serde_json::to_string_pretty(value)
    .map_err(|error| format!("failed to encode {}: {error}", path.display()))?;
  rendered.push('\n');
  fs::write(&path, rendered)
    .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn default_post_capture_offsets_ms() -> Vec<u64> {
  vec![16, 48]
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};

  #[test]
  fn latency_report_aggregates_percentiles() {
    let dispatch_trace = vec![
      DispatchSample {
        object_index: 0,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 10,
        actual_dispatch_time_ms: 11,
        dispatch_error_ms: 1,
        x: 1.0,
        y: 2.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 1,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 20,
        actual_dispatch_time_ms: 23,
        dispatch_error_ms: 3,
        x: 3.0,
        y: 4.0,
        delivery_path: None,
        fallback_reason: None,
      },
      DispatchSample {
        object_index: 2,
        object_kind: ObjectKind::Circle,
        scheduled_time_ms: 30,
        actual_dispatch_time_ms: 32,
        dispatch_error_ms: 2,
        x: 5.0,
        y: 6.0,
        delivery_path: None,
        fallback_reason: None,
      },
    ];

    let report = build_latency_report(RunMode::DryRun, &dispatch_trace);
    assert_eq!(report.total_actions, 3);
    assert_eq!(report.p50_error_ms, 2);
    assert_eq!(report.p95_error_ms, 3);
    assert_eq!(report.p99_error_ms, 3);
    assert_eq!(report.max_error_ms, 3);
    assert_eq!(report.jitter_ms, 2);
    assert_eq!(report.missed_schedule_count, 3);
  }

  #[test]
  fn percentile_handles_empty_input() {
    assert_eq!(percentile(&[], 0.95), 0);
  }

  #[test]
  fn scheduled_target_maps_variants() {
    let circle = HitObjectKind::Circle(rosu_map::section::hit_objects::HitObjectCircle {
      pos: rosu_map::util::Pos::new(10.0, 20.0),
      new_combo: false,
      combo_offset: 0,
    });
    let (kind, x, y) = scheduled_target(&circle);
    assert_eq!(kind, ObjectKind::Circle);
    assert_eq!((x, y), (10.0, 20.0));
  }

  #[test]
  fn benchmark_inputs_default_to_dry_run() {
    let inputs = BenchmarkInputs::new(PathBuf::from("map.osu"), PathBuf::from("out"));
    assert_eq!(inputs.run_mode, RunMode::DryRun);
    assert_eq!(inputs.lead_in_ms, 25);
    assert_eq!(inputs.target_app, None);
    assert_eq!(inputs.dispatch_limit, None);
    assert!(!inputs.capture_verify);
    assert_eq!(inputs.post_capture_offsets_ms, vec![16, 48]);
  }

  #[test]
  fn typed_dispatch_inputs_set_defaults() {
    let inputs = BenchmarkInputs::typed_dispatch(
      PathBuf::from("map.osu"),
      PathBuf::from("out"),
      "osu!",
    );
    assert_eq!(inputs.run_mode, RunMode::TypedDispatch);
    assert_eq!(inputs.target_app.as_deref(), Some("osu!"));
    assert_eq!(inputs.dispatch_limit, Some(8));
    assert!(!inputs.capture_verify);
  }

  #[test]
  fn dispatch_sample_captures_input_result_metadata() {
    let action = ScheduledAction {
      object_index: 4,
      object_kind: ObjectKind::Circle,
      scheduled_time_ms: 120,
      x: 256.0,
      y: 192.0,
    };
    let sample = dispatch_sample_from_result(
      &action,
      125,
      5,
      InputActionResult {
        selected_path: InputDeliveryPath::WindowTargetedMouse,
        attempts: vec![],
        fallback_reason: Some("fallback".to_string()),
        mouse_disturbance: auv_driver::DisturbanceLevel::Temporary,
        focus_disturbance: auv_driver::DisturbanceLevel::Foreground,
        clipboard_disturbance: auv_driver::DisturbanceLevel::None,
      },
    );
    assert_eq!(sample.delivery_path.as_deref(), Some("WindowTargetedMouse"));
    assert_eq!(sample.fallback_reason.as_deref(), Some("fallback"));
  }

  #[test]
  fn verification_summary_aggregates_capture_readiness() {
    let summary = build_verification_summary(
      2,
      &[
        CaptureTraceSample {
          object_index: 0,
          object_kind: ObjectKind::Circle,
          scheduled_time_ms: 10,
          actual_dispatch_time_ms: 11,
          dispatch_error_ms: 1,
          captures: vec![
            CaptureSample {
              phase: CapturePhase::BeforeDispatch,
              capture_time_ms: 9,
              relative_to_dispatch_ms: -1,
              file_name: "a.png".to_string(),
              width: 100,
              height: 100,
              backend: "test".to_string(),
              fallback_reason: None,
            },
            CaptureSample {
              phase: CapturePhase::AfterDispatch,
              capture_time_ms: 25,
              relative_to_dispatch_ms: 14,
              file_name: "b.png".to_string(),
              width: 100,
              height: 100,
              backend: "test".to_string(),
              fallback_reason: None,
            },
          ],
        },
        CaptureTraceSample {
          object_index: 1,
          object_kind: ObjectKind::Circle,
          scheduled_time_ms: 20,
          actual_dispatch_time_ms: 20,
          dispatch_error_ms: 0,
          captures: vec![],
        },
      ],
    );
    assert_eq!(summary.captured_action_count, 1);
    assert_eq!(summary.missing_frame_count, 1);
    assert_eq!(summary.max_capture_delay_ms, 14);
    assert_eq!(summary.suspicious_time_inversion_count, 0);
  }
}
