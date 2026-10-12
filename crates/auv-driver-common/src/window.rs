use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
  error::{DriverError, DriverResult},
  geometry::{CoordinateSpace, Point, Rect, Size},
  input::DisturbanceLevel,
  selector::{AppSelector, TextMatcher},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowRef {
  pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
  pub reference: WindowRef,
  pub title: Option<String>,
  pub app_name: Option<String>,
  /// Platform application id: a bundle id on macOS, a desktop application id
  /// on Linux (AT-SPI AccessibleId, or the GTK id / WM_CLASS class on X11).
  /// Windows does not report one yet.
  pub application_id: Option<String>,
  pub process_id: Option<u32>,
  pub frame: Rect,
  pub coordinate_space: CoordinateSpace,
  pub is_main: bool,
  pub is_visible: bool,
}

/// The window with `id` from an observation, or `NotFound` naming `window:<id>`.
///
/// Window targets are stable references, not selectors: callers that hold a
/// window ID look it up here instead of re-resolving by app or title.
pub fn find_window(windows: impl IntoIterator<Item = Window>, id: &str) -> DriverResult<Window> {
  windows.into_iter().find(|window| window.reference.id == id).ok_or_else(|| DriverError::NotFound {
    target: format!("window:{id}"),
  })
}

/// A `NotFound` for an application selector that lists the applications
/// owning visible windows, so a caller that guessed an id (`com.google.Chrome`
/// on Linux or Windows) can retry without another `window.list` round trip.
// NOTICE(app-not-found-hint): `DriverError::NotFound` has only a target, so the
// hint rides in it. Move it to a structured recovery field if NotFound gains one.
pub fn application_not_found(selector: &AppSelector, windows: &[Window]) -> DriverError {
  let requested = [&selector.id, &selector.name]
    .into_iter()
    .flatten()
    .map(|matcher| match matcher {
      TextMatcher::Exact(text) | TextMatcher::Contains(text) => text.as_str(),
    })
    .collect::<Vec<_>>()
    .join(" ");
  let visible =
    windows.iter().filter(|window| window.is_visible).filter_map(|window| window.application_id.as_deref().or(window.app_name.as_deref()));
  DriverError::NotFound {
    target: format!("application {requested:?} ({})", visible_applications_hint(visible)),
  }
}

/// `visible applications: a, b, …` for application ids, sorted and deduplicated
/// ignoring case, or `no visible application windows`. Long lists are cut so
/// an error stays readable.
pub fn visible_applications_hint<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
  const MAX_LISTED: usize = 20;
  let mut ids = ids.into_iter().filter(|id| !id.trim().is_empty()).collect::<Vec<_>>();
  ids.sort_unstable_by_key(|id| id.to_lowercase());
  ids.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
  match ids.len() {
    0 => "no visible application windows".to_string(),
    count if count > MAX_LISTED => format!("visible applications: {}, … ({} more)", ids[..MAX_LISTED].join(", "), count - MAX_LISTED),
    _ => format!("visible applications: {}", ids.join(", ")),
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObservedWindows {
  pub windows: Vec<Window>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMutationOptions {
  pub policy: WindowMutationPolicy,
  pub strategy: WindowMutationStrategy,
  pub settle: Duration,
  pub verification: WindowMutationVerification,
}

impl Default for WindowMutationOptions {
  fn default() -> Self {
    Self {
      policy: WindowMutationPolicy::NativePreferred,
      strategy: WindowMutationStrategy::default(),
      settle: Duration::from_millis(100),
      verification: WindowMutationVerification::default(),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPolicy {
  NativeOnly,
  #[default]
  NativePreferred,
  ForegroundPreferred,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationStrategy {
  pub candidates: Vec<WindowMutationCandidate>,
}

impl Default for WindowMutationStrategy {
  fn default() -> Self {
    Self {
      candidates: vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ],
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationCandidate {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationVerification {
  FrameTolerance { points: f64 },
  BestEffortState,
}

impl Default for WindowMutationVerification {
  fn default() -> Self {
    Self::FrameTolerance { points: 2.0 }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationPath {
  AxWindowAttribute,
  AxWindowAction,
  PlatformNative,
  ForegroundSystemEvents,
  Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowMutationAttempt {
  pub path: WindowMutationPath,
  pub succeeded: bool,
  pub message: Option<String>,
}

impl WindowMutationAttempt {
  pub fn success(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: true,
      message: Some(message.into()),
    }
  }

  pub fn failure(path: WindowMutationPath, message: impl Into<String>) -> Self {
    Self {
      path,
      succeeded: false,
      message: Some(message.into()),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMutationResult {
  pub selected_path: WindowMutationPath,
  pub attempts: Vec<WindowMutationAttempt>,
  pub before_frame: Option<Rect>,
  pub after_frame: Option<Rect>,
  pub before_state: Option<WindowState>,
  pub after_state: Option<WindowState>,
  pub focus_disturbance: DisturbanceLevel,
  pub mouse_disturbance: DisturbanceLevel,
}

impl WindowMutationResult {
  /// Returns the first failed mutation attempt's diagnostic.
  pub fn fallback_reason(&self) -> Option<&str> {
    self.attempts.iter().find(|attempt| !attempt.succeeded).and_then(|attempt| attempt.message.as_deref())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_visible: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationKind {
  MoveTo { point: Point },
  Resize { size: Size },
  SetFrame { frame: Rect },
  Minimize,
  Restore,
  Zoom,
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
