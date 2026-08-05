use std::sync::mpsc;
use std::time::Duration;

use auv::client::runner::MouseMotionEvent;
use auv::client::{Client, RunOptions, RunnerOptions};
use auv::resource::DeviceSelector;
use auv_driver::{MouseCubicBezierSegment, MouseCurve, MouseCurveMapping, MouseMotionOptions, MouseMotionPlan, MouseStart, Point};
use clap::Parser;
use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};

const WIDTH: usize = 820;
const HEIGHT: usize = 540;
const MARGIN: usize = 30;
const CANVAS_WIDTH: usize = WIDTH - MARGIN * 2;
const CANVAS_HEIGHT: usize = HEIGHT - 100;

#[derive(Parser)]
#[command(about = "Draw a normalized vector curve and execute it as remote AUV mouse motion")]
struct Args {
  /// Remote Device ID prefix or exact Device name.
  #[arg(long = "device-id", alias = "device", env = "AUV_LINUX_DEVICE_ID")]
  device_id: Option<String>,
  /// Logical screen displacement represented by the full canvas width.
  #[arg(long, default_value_t = 800.0)]
  map_width: f64,
  /// Logical screen displacement represented by the full canvas height.
  #[arg(long, default_value_t = 450.0)]
  map_height: f64,
  /// Motion duration in milliseconds.
  #[arg(long, default_value_t = 900)]
  duration_ms: u64,
  #[arg(long, default_value_t = 120)]
  sample_rate_hz: u32,
  /// Explicit logical screen start X. Requires --start-y; otherwise uses the current pointer.
  #[arg(long, requires = "start_y")]
  start_x: Option<f64>,
  /// Explicit logical screen start Y. Requires --start-x; otherwise uses the current pointer.
  #[arg(long, requires = "start_x")]
  start_y: Option<f64>,
  /// Start with a small S-shaped example curve.
  #[arg(long)]
  demo_curve: bool,
  /// Execute the example curve once when the window opens.
  #[arg(long, requires = "demo_curve")]
  auto_run: bool,
}

enum WorkerMessage {
  Execute(MouseMotionPlan),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = Args::parse();
  let selector = args.device_id.as_deref().map(DeviceSelector::parse).transpose()?;
  let mapping_label = match (args.start_x, args.start_y) {
    (Some(x), Some(y)) => format!("origin={x:.0},{y:.0}; canvas={:.0}×{:.0}", args.map_width, args.map_height),
    _ => format!("origin=current cursor; canvas={:.0}×{:.0}", args.map_width, args.map_height),
  };
  let (work_tx, work_rx) = mpsc::channel();
  let (status_tx, status_rx) = mpsc::channel();
  std::thread::spawn(move || worker(selector, work_rx, status_tx));

  let mut window = Window::new("AUV mouse curve — drag to draw, Enter to run, C to clear", WIDTH, HEIGHT, WindowOptions::default())?;
  window.set_position(80, 80);
  window.set_target_fps(60);
  let mut buffer = vec![0x00151a22; WIDTH * HEIGHT];
  let mut points = if args.demo_curve {
    demo_curve()
  } else {
    Vec::new()
  };
  let mut drawing = false;
  let mut status = "ready".to_string();
  if args.auto_run {
    work_tx.send(WorkerMessage::Execute(plan_from_points(&args, &points)?))?;
    status = "submitted".to_string();
  }

  while window.is_open() && !window.is_key_down(Key::Escape) {
    while let Ok(value) = status_rx.try_recv() {
      status = value;
    }
    if window.get_mouse_down(MouseButton::Left) {
      if let Some((x, y)) = window.get_mouse_pos(MouseMode::Clamp)
        && x >= MARGIN as f32
        && x <= (MARGIN + CANVAS_WIDTH) as f32
        && y >= MARGIN as f32
        && y <= (MARGIN + CANVAS_HEIGHT) as f32
      {
        let point = Point::new((x as f64 - MARGIN as f64) / CANVAS_WIDTH as f64, (y as f64 - MARGIN as f64) / CANVAS_HEIGHT as f64);
        if !drawing {
          points.clear();
          drawing = true;
        }
        if points.last().is_none_or(|last| (last.x - point.x).hypot(last.y - point.y) > 0.004) {
          points.push(point);
        }
      }
    } else {
      drawing = false;
    }
    if window.is_key_pressed(Key::C, minifb::KeyRepeat::No) {
      points.clear();
      status = "cleared".to_string();
    }
    if window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) {
      match plan_from_points(&args, &points) {
        Ok(plan) => {
          work_tx.send(WorkerMessage::Execute(plan))?;
          status = "submitted".to_string();
        }
        Err(message) => status = message,
      }
    }

    draw(&mut buffer, &points);
    window.set_title(&format!("AUV mouse curve — {mapping_label} — {status} — drag / Enter / C / Esc"));
    window.update_with_buffer(&buffer, WIDTH, HEIGHT)?;
  }
  Ok(())
}

fn demo_curve() -> Vec<Point> {
  vec![
    Point::new(0.1, 0.5),
    Point::new(0.3, 0.2),
    Point::new(0.5, 0.5),
    Point::new(0.7, 0.8),
    Point::new(0.9, 0.5),
  ]
}

fn plan_from_points(args: &Args, points: &[Point]) -> Result<MouseMotionPlan, String> {
  if points.len() < 2 {
    return Err("draw at least two points".to_string());
  }
  if !args.map_width.is_finite() || !args.map_height.is_finite() || args.map_width <= 0.0 || args.map_height <= 0.0 {
    return Err("mapping dimensions must be finite and positive".to_string());
  }
  let start = match (args.start_x, args.start_y) {
    (Some(x), Some(y)) if x.is_finite() && y.is_finite() => MouseStart::Screen(Point::new(x, y)),
    (None, None) => MouseStart::Current,
    _ => return Err("explicit start must contain finite X and Y".to_string()),
  };
  let mut segments = Vec::with_capacity(points.len() - 1);
  for index in 0..points.len() - 1 {
    let p0 = if index == 0 {
      points[index]
    } else {
      points[index - 1]
    };
    let p1 = points[index];
    let p2 = points[index + 1];
    let p3 = points.get(index + 2).copied().unwrap_or(p2);
    segments.push(MouseCubicBezierSegment {
      control_1: Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0),
      control_2: Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0),
      end: p2,
    });
  }
  Ok(MouseMotionPlan {
    start,
    curve: MouseCurve {
      start: points[0],
      segments,
    },
    mapping: MouseCurveMapping {
      width: args.map_width,
      height: args.map_height,
    },
    options: MouseMotionOptions {
      duration: Duration::from_millis(args.duration_ms),
      sample_rate_hz: args.sample_rate_hz,
    },
  })
}

fn worker(selector: Option<DeviceSelector>, receiver: mpsc::Receiver<WorkerMessage>, status: mpsc::Sender<String>) {
  let runtime = match tokio::runtime::Runtime::new() {
    Ok(runtime) => runtime,
    Err(error) => {
      let _ = status.send(format!("runtime error: {error}"));
      return;
    }
  };
  runtime.block_on(async move {
    let client = match Client::from_env_or_local().await {
      Ok(client) => client,
      Err(error) => {
        let _ = status.send(format!("connect error: {error}"));
        return;
      }
    };
    while let Ok(WorkerMessage::Execute(plan)) = receiver.recv() {
      let mut run_options = RunOptions::default();
      if let Some(selector) = selector.clone() {
        run_options.device = selector;
      }
      let execution = match client.runner_with(run_options, RunnerOptions::default()).await {
        Ok(execution) => execution,
        Err(error) => {
          let _ = status.send(format!("route error: {error}"));
          continue;
        }
      };
      let operation = async {
        let mut stream = execution.input().move_mouse(plan).await?;
        while let Some(event) = stream.next().await? {
          match event {
            MouseMotionEvent::Started {
              planned_sample_count,
              ..
            } => {
              let _ = status.send(format!("running up to {planned_sample_count} samples"));
            }
            MouseMotionEvent::Progress { sample_index, .. } if sample_index % 8 == 0 => {
              let _ = status.send(format!("sample {sample_index}"));
            }
            MouseMotionEvent::Completed { point, .. } => {
              let _ = status.send(format!("completed at {:.1},{:.1}", point.x, point.y));
            }
            MouseMotionEvent::Progress { .. } | MouseMotionEvent::Accepted { .. } | MouseMotionEvent::Cancelled => {}
          }
        }
        Ok::<_, auv::client::runner::CapabilityError>(())
      }
      .await;
      let outcome = if operation.is_ok() {
        auv::runs::RunOutcome::Succeeded
      } else {
        auv::runs::RunOutcome::Failed
      };
      let cleanup = execution.finish(outcome).await;
      if let Err(error) = operation {
        let _ = status.send(format!("motion error: {error}"));
      }
      if let Err(error) = cleanup {
        let _ = status.send(format!("run cleanup error: {error}"));
      }
    }
  });
}

fn draw(buffer: &mut [u32], points: &[Point]) {
  buffer.fill(0x00151a22);
  for y in MARGIN..=MARGIN + CANVAS_HEIGHT {
    for x in MARGIN..=MARGIN + CANVAS_WIDTH {
      if y == MARGIN || y == MARGIN + CANVAS_HEIGHT || x == MARGIN || x == MARGIN + CANVAS_WIDTH {
        buffer[y * WIDTH + x] = 0x006b7280;
      }
    }
  }
  for pair in points.windows(2) {
    line(
      buffer,
      MARGIN as i32 + (pair[0].x * CANVAS_WIDTH as f64) as i32,
      MARGIN as i32 + (pair[0].y * CANVAS_HEIGHT as f64) as i32,
      MARGIN as i32 + (pair[1].x * CANVAS_WIDTH as f64) as i32,
      MARGIN as i32 + (pair[1].y * CANVAS_HEIGHT as f64) as i32,
      0x0038bdf8,
    );
  }
}

fn line(buffer: &mut [u32], mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u32) {
  let dx = (x1 - x0).abs();
  let sx = if x0 < x1 { 1 } else { -1 };
  let dy = -(y1 - y0).abs();
  let sy = if y0 < y1 { 1 } else { -1 };
  let mut error = dx + dy;
  loop {
    if x0 >= 0 && y0 >= 0 && (x0 as usize) < WIDTH && (y0 as usize) < HEIGHT {
      buffer[y0 as usize * WIDTH + x0 as usize] = color;
    }
    if x0 == x1 && y0 == y1 {
      break;
    }
    let doubled = 2 * error;
    if doubled >= dy {
      error += dy;
      x0 += sx;
    }
    if doubled <= dx {
      error += dx;
      y0 += sy;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn args() -> Args {
    Args {
      device_id: None,
      map_width: 800.0,
      map_height: 450.0,
      duration_ms: 900,
      sample_rate_hz: 120,
      start_x: None,
      start_y: None,
      demo_curve: false,
      auto_run: false,
    }
  }

  #[test]
  fn drawn_curve_uses_current_cursor_and_normalized_mapping() {
    let plan = plan_from_points(&args(), &[Point::new(0.1, 0.2), Point::new(0.6, 0.7)]).unwrap();
    assert_eq!(plan.start, MouseStart::Current);
    assert_eq!(plan.curve.start, Point::new(0.1, 0.2));
    assert_eq!(plan.curve.segments.last().unwrap().end, Point::new(0.6, 0.7));
    assert_eq!(
      plan.mapping,
      MouseCurveMapping {
        width: 800.0,
        height: 450.0
      }
    );
  }

  #[test]
  fn explicit_start_is_independent_of_canvas_coordinates() {
    let mut args = args();
    args.start_x = Some(1200.0);
    args.start_y = Some(700.0);
    let plan = plan_from_points(&args, &[Point::new(0.4, 0.4), Point::new(0.5, 0.5)]).unwrap();
    assert_eq!(plan.start, MouseStart::Screen(Point::new(1200.0, 700.0)));
    let samples = plan.samples(Point::new(1200.0, 700.0)).unwrap();
    assert_eq!(samples.first().unwrap().point, Point::new(1200.0, 700.0));
    assert_eq!(samples.last().unwrap().point, Point::new(1280.0, 745.0));
  }

  #[test]
  fn demo_curve_stays_inside_normalized_canvas() {
    let points = demo_curve();
    assert!(points.iter().all(|point| (0.0..=1.0).contains(&point.x) && (0.0..=1.0).contains(&point.y)));
  }
}
