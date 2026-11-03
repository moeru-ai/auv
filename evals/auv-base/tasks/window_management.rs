//! The window management scenario the macOS and Windows desktop evaluations share: `set_frame`,
//! minimize, maximize and focus on a throwaway window the evaluation owns.
//!
//! The oracle is a fresh `WindowApi::get` after each step, not the mutation result. See
//! `docs/ai/references/driver/2026-11-02-window-management-api-v1-design.md`.

use anyhow::{Result, anyhow, bail, ensure};
use auv_driver_common::{
  DriverError, DriverResult, Point, Rect, Size, Window, WindowMutationOptions, WindowMutationResult, WindowStateTarget,
};

/// AppKit and DWM round frames to whole pixels at a fractional scale; the evidence runs saw 1 px.
const FRAME_TOLERANCE: f64 = 2.0;

/// The platform `WindowApi` calls the scenario uses, so it runs unchanged on both drivers.
pub trait WindowManagement {
  fn get(&self, id: &str) -> DriverResult<Window>;
  fn set_frame(&self, window: &Window, origin: Option<Point>, size: Option<Size>) -> DriverResult<WindowMutationResult>;
  fn set_state(&self, window: &Window, state: WindowStateTarget) -> DriverResult<WindowMutationResult>;
  fn focus(&self, window: &Window) -> DriverResult<WindowMutationResult>;
}

macro_rules! window_management {
  ($api:ty) => {
    impl WindowManagement for $api {
      fn get(&self, id: &str) -> DriverResult<Window> {
        <$api>::get(self, id)
      }

      fn set_frame(&self, window: &Window, origin: Option<Point>, size: Option<Size>) -> DriverResult<WindowMutationResult> {
        <$api>::set_frame(self, window, origin, size, WindowMutationOptions::default())
      }

      fn set_state(&self, window: &Window, state: WindowStateTarget) -> DriverResult<WindowMutationResult> {
        <$api>::set_state(self, window, state, WindowMutationOptions::default())
      }

      fn focus(&self, window: &Window) -> DriverResult<WindowMutationResult> {
        <$api>::focus(self, window, WindowMutationOptions::default())
      }
    }
  };
}

#[cfg(target_os = "macos")]
window_management!(auv_driver_macos::WindowApi<'_>);
#[cfg(windows)]
window_management!(auv_driver_windows::WindowApi<'_>);

/// Runs the scenario on the window `id` and returns at the first failed check. Leaves the window
/// normal at a frame near where it started; the caller closes it.
pub fn run(api: &impl WindowManagement, id: &str) -> Result<()> {
  let mut window = driver("get", api.get(id))?;
  if window.state.is_minimized == Some(true) || window.state.is_maximized == Some(true) {
    driver("set_state(Normal) before the scenario", api.set_state(&window, WindowStateTarget::Normal))?;
    window = driver("get", api.get(id))?;
  }
  eprintln!("start: {} state {:?}", frame(&window.frame), window.state);

  let target = Rect::new(window.frame.origin.x + 40.0, window.frame.origin.y + 30.0, 520.0, 360.0);
  let result = driver("set_frame", api.set_frame(&window, Some(target.origin), Some(target.size)))?;
  let window = driver("get", api.get(id))?;
  eprintln!("set_frame: target {} got {} path {:?}", frame(&target), frame(&window.frame), result.selected_path);
  ensure!(
    close(&window.frame, &target),
    "set_frame landed at {}, not within {FRAME_TOLERANCE} px of {}",
    frame(&window.frame),
    frame(&target)
  );
  let restore = window.frame;

  driver("set_state(Minimized)", api.set_state(&window, WindowStateTarget::Minimized))?;
  let minimized = driver("get", api.get(id))?;
  eprintln!("minimized: {} state {:?}", frame(&minimized.frame), minimized.state);
  ensure!(minimized.state.is_minimized == Some(true), "the window does not read as minimized: {:?}", minimized.state);
  match api.set_frame(&minimized, Some(Point::new(restore.origin.x + 10.0, restore.origin.y)), None) {
    Err(DriverError::FailedPrecondition { message, .. }) => eprintln!("set_frame while minimized: FailedPrecondition {message:?}"),
    Err(error) => bail!("set_frame while minimized failed with {error}, not FailedPrecondition"),
    Ok(result) => bail!("set_frame while minimized succeeded: {}", frame(&result.after_frame.unwrap_or_default())),
  }
  driver("set_state(Normal) from minimized", api.set_state(&minimized, WindowStateTarget::Normal))?;
  let normal = driver("get", api.get(id))?;
  eprintln!("normal: {} state {:?}", frame(&normal.frame), normal.state);
  ensure!(normal.state.is_minimized != Some(true), "the window still reads as minimized");
  ensure!(close(&normal.frame, &restore), "unminimize restored {}, not {}", frame(&normal.frame), frame(&restore));

  driver("set_state(Maximized)", api.set_state(&normal, WindowStateTarget::Maximized))?;
  let maximized = driver("get", api.get(id))?;
  eprintln!("maximized: {} state {:?}", frame(&maximized.frame), maximized.state);
  ensure!(maximized.state.is_maximized == Some(true), "the window does not read as maximized: {:?}", maximized.state);
  ensure!(!close(&maximized.frame, &restore), "maximize left the frame at {}", frame(&maximized.frame));
  driver("set_state(Normal) from maximized", api.set_state(&maximized, WindowStateTarget::Normal))?;
  let normal = driver("get", api.get(id))?;
  eprintln!("normal: {} state {:?}", frame(&normal.frame), normal.state);
  ensure!(normal.state.is_maximized != Some(true), "the window still reads as maximized");
  ensure!(close(&normal.frame, &restore), "unmaximize restored {}, not {}", frame(&normal.frame), frame(&restore));

  let result = driver("focus", api.focus(&normal))?;
  let focused = driver("get", api.get(id))?;
  eprintln!("focus: state {:?} focus disturbance {:?}", focused.state, result.focus_disturbance);
  ensure!(focused.state.is_focused == Some(true), "the window does not read as focused: {:?}", focused.state);
  Ok(())
}

fn driver<T>(step: &str, result: DriverResult<T>) -> Result<T> {
  result.map_err(|error| anyhow!("{step}: {error}"))
}

fn close(a: &Rect, b: &Rect) -> bool {
  [
    (a.origin.x, b.origin.x),
    (a.origin.y, b.origin.y),
    (a.size.width, b.size.width),
    (a.size.height, b.size.height),
  ]
  .into_iter()
  .all(|(a, b)| (a - b).abs() <= FRAME_TOLERANCE)
}

fn frame(rect: &Rect) -> String {
  format!("{},{} {}x{}", rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
}
