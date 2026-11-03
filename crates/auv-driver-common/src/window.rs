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
  /// Minimized, maximized, focus, stacking and desktop facts. A `None` field
  /// means the platform cannot tell, never "false".
  #[serde(default)]
  pub state: WindowState,
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

/// Opt-outs for a window listing. The default lists every window a user could
/// switch to, minimized windows and windows on other desktops included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowListFilter {
  pub exclude_minimized: bool,
  /// Drops windows known to be on another desktop. A window whose desktop is
  /// unknown (`state.desktop` is `None`) is kept.
  pub exclude_other_desktops: bool,
}

impl WindowListFilter {
  /// Whether `window` stays in a listing with this filter.
  pub fn keeps(&self, window: &Window) -> bool {
    let minimized = window.state.is_minimized == Some(true);
    let on_other_desktop = window.state.desktop.as_ref().is_some_and(|desktop| !desktop.is_current);
    !(self.exclude_minimized && minimized || self.exclude_other_desktops && on_other_desktop)
  }
}

/// The application's main window among `candidates`: the windows that already
/// match a selector's application and title. `None` only when there is none.
///
/// One narrowing cascade shared by every platform resolver and by callers that
/// need "the app's main window" (`application.activate`). Each preference keeps only
/// the candidates that pass it, but is skipped when none would pass, so a
/// single minimized window or a window on another desktop is still chosen.
/// See "Main window selection" in
/// `docs/ai/references/driver/2026-11-02-window-management-api-v1-design.md`.
pub fn select_main_window<'a>(candidates: impl IntoIterator<Item = &'a Window>) -> Option<&'a Window> {
  let preferences: [fn(&Window) -> bool; 5] = [
    |window| window.state.desktop.as_ref().is_none_or(|desktop| desktop.is_current),
    |window| window.state.is_minimized != Some(true),
    |window| window.state.is_focused == Some(true),
    |window| window.is_main,
    |window| window.title.as_deref().is_some_and(|title| !title.trim().is_empty()),
  ];
  let mut candidates = candidates.into_iter().collect::<Vec<_>>();
  for prefers in preferences {
    if candidates.len() <= 1 {
      break;
    }
    if candidates.iter().any(|window| prefers(window)) {
      candidates.retain(|window| prefers(window));
    }
  }
  // Frontmost first (a window without a stacking position goes last), then
  // the largest frame; `min_by` keeps the first of equal windows, which is
  // listing order.
  candidates.into_iter().min_by(|a, b| {
    let stack = |window: &Window| (window.state.stack_index.is_none(), window.state.stack_index);
    let area = |window: &Window| window.frame.size.width * window.frame.size.height;
    stack(a).cmp(&stack(b)).then_with(|| area(b).total_cmp(&area(a)))
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
  /// The window changed but the app settled it at another frame than the one
  /// requested (minimum or maximum size, grid snapping).
  #[serde(default)]
  pub adjusted_by_app: bool,
  pub focus_disturbance: DisturbanceLevel,
  pub mouse_disturbance: DisturbanceLevel,
}

impl WindowMutationResult {
  /// Returns the first failed mutation attempt's diagnostic.
  pub fn fallback_reason(&self) -> Option<&str> {
    self.attempts.iter().find(|attempt| !attempt.succeeded).and_then(|attempt| attempt.message.as_deref())
  }
}

/// What a platform knows about a window beyond its frame. See
/// `docs/ai/references/driver/2026-11-02-window-management-api-v1-design.md`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
  pub is_minimized: Option<bool>,
  pub is_maximized: Option<bool>,
  pub is_fullscreen: Option<bool>,
  pub is_visible: Option<bool>,
  /// Has keyboard focus. Distinct from `Window::is_main`, which keeps its
  /// selector meaning.
  pub is_focused: Option<bool>,
  pub is_always_on_top: Option<bool>,
  /// 0 is the frontmost window of the listing that carries it. Valid only
  /// inside that listing or event; it is not an identity.
  pub stack_index: Option<u32>,
  pub desktop: Option<WindowDesktop>,
}

/// The virtual desktop a window is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowDesktop {
  /// Opaque platform id, such as a Windows virtual desktop GUID. Callers
  /// compare it and never parse it.
  pub id: String,
  pub is_current: bool,
}

/// The state `SetWindowState` moves a window to.
// TODO(window-fullscreen): no `Fullscreen` target. macOS full screen moves the
// window to its own Space with an asynchronous animation, and Win32 has no
// generic full screen for another app's window. `WindowState::is_fullscreen`
// stays on the read side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStateTarget {
  /// Neither minimized nor maximized.
  Normal,
  Minimized,
  Maximized,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMutationKind {
  /// Moves and/or resizes a `Normal` window. A missing origin keeps the
  /// position; a missing size keeps the size.
  SetFrame {
    origin: Option<Point>,
    size: Option<Size>,
  },
  SetState {
    state: WindowStateTarget,
  },
}

#[cfg(test)]
#[path = "window_test.rs"]
mod tests;
