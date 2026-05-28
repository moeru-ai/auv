// File: src/driver/macos/support/selector.rs
use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;

use super::{app_contains_window, looks_like_bundle_identifier, window_area};
use crate::capture::types::DisplayDescriptor;
use crate::types::{
  AppSelector, AuvResult, ObservedRect, ObservedWindow, ObservedWindowSnapshot, ResolvedAppRef,
  WindowCandidate, WindowSelection,
};

pub fn parse_app_selector(raw: &str) -> AuvResult<AppSelector> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return Err("app selector cannot be empty".to_string());
  }

  Ok(AppSelector {
    raw: trimmed.to_string(),
    bundle_id: if looks_like_bundle_identifier(trimmed) {
      Some(trimmed.to_string())
    } else {
      None
    },
    app_name_hint: if looks_like_bundle_identifier(trimmed) {
      None
    } else {
      Some(trimmed.to_string())
    },
  })
}

pub fn resolve_app_ref(
  snapshot: &ObservedWindowSnapshot,
  selector: &AppSelector,
) -> AuvResult<ResolvedAppRef> {
  if let Some(bundle_id) = selector.bundle_id.as_deref() {
    let exact_bundle_matches = snapshot
      .windows
      .iter()
      .filter(|window| window.owner_bundle_id.eq_ignore_ascii_case(bundle_id))
      .collect::<Vec<_>>();
    if !exact_bundle_matches.is_empty() {
      return Ok(build_resolved_app_ref(
        selector,
        Some(bundle_id.to_string()),
        &exact_bundle_matches,
        "bundle-id-exact",
      ));
    }

    if !snapshot.frontmost_app_bundle_id.is_empty()
      && snapshot
        .frontmost_app_bundle_id
        .eq_ignore_ascii_case(bundle_id)
      && !snapshot.frontmost_app_name.trim().is_empty()
    {
      let frontmost_name_matches = snapshot
        .windows
        .iter()
        .filter(|window| window.app_name == snapshot.frontmost_app_name)
        .collect::<Vec<_>>();
      if !frontmost_name_matches.is_empty() {
        return Ok(build_resolved_app_ref(
          selector,
          Some(bundle_id.to_string()),
          &frontmost_name_matches,
          "frontmost-bundle-fallback",
        ));
      }
    }
  }

  if let Some(app_name_hint) = selector.app_name_hint.as_deref() {
    let exact_name_matches = snapshot
      .windows
      .iter()
      .filter(|window| window.app_name.eq_ignore_ascii_case(app_name_hint))
      .collect::<Vec<_>>();
    if !exact_name_matches.is_empty() {
      return Ok(build_resolved_app_ref(
        selector,
        first_non_empty_bundle_id(&exact_name_matches),
        &exact_name_matches,
        "app-name-exact",
      ));
    }

    let heuristic_name_matches = snapshot
      .windows
      .iter()
      .filter(|window| app_contains_window(app_name_hint, &window.app_name))
      .collect::<Vec<_>>();
    if !heuristic_name_matches.is_empty() {
      return Ok(build_resolved_app_ref(
        selector,
        first_non_empty_bundle_id(&heuristic_name_matches),
        &heuristic_name_matches,
        "app-name-heuristic",
      ));
    }
  }

  Err(format!(
    "could not resolve a visible app reference for selector {:?}",
    selector.raw
  ))
}

pub fn build_window_candidates(
  snapshot: &ObservedWindowSnapshot,
  resolved_app: &ResolvedAppRef,
  displays: &[DisplayDescriptor],
) -> AuvResult<Vec<WindowCandidate>> {
  let mut windows = snapshot
    .windows
    .iter()
    .filter(|window| window_matches_resolved_app(window, resolved_app))
    .filter(|window| is_substantial_window(window))
    .collect::<Vec<_>>();

  windows.sort_by_key(|window| {
    std::cmp::Reverse((
      if window.layer == 0 { 1 } else { 0 },
      if !window.title.trim().is_empty() {
        1
      } else {
        0
      },
      window_area(window),
    ))
  });

  Ok(
    windows
      .into_iter()
      .enumerate()
      .map(|(candidate_index, window)| {
        let display = containing_display(&window.bounds, displays);
        let is_main_candidate = candidate_index == 0;
        WindowCandidate {
          candidate_index,
          window_ref: window.to_window_ref(),
          native_window_id: Some(window.window_number.to_string()),
          display_ref: display.map(|display| display.display_ref.clone()),
          native_display_id: display.map(|display| display.native_display_id.clone()),
          is_main_candidate,
          is_fully_contained_in_display: display.is_some(),
          area: window_area(window),
          selection_reason: if is_main_candidate {
            "largest-visible-normal-window".to_string()
          } else {
            "visible-app-window".to_string()
          },
        }
      })
      .collect(),
  )
}

pub fn resolve_window_candidate(
  snapshot: &ObservedWindowSnapshot,
  resolved_app: &ResolvedAppRef,
  displays: &[DisplayDescriptor],
  selection: &WindowSelection,
) -> AuvResult<WindowCandidate> {
  let candidates = build_window_candidates(snapshot, resolved_app, displays)?;
  if selection.has_selector() {
    let filtered = filter_window_candidates(&candidates, selection);
    if filtered.len() == 1 {
      return Ok(filtered[0].clone());
    }
    if filtered.len() > 1 {
      let selector_detail = if selection.title.is_some() {
        "title"
      } else if selection.window_ref.is_some() {
        "window_ref"
      } else if selection.native_window_id.is_some() {
        "native_window_id"
      } else {
        "selector"
      };
      return Err(format!(
        "multiple window candidates matched {selector_detail}; inspect `debug.listWindows` and provide --window_ref or --native_window_id"
      ));
    }
    return Err(
      "no window candidate matched the explicit selector; inspect `debug.listWindows` and refresh --window_ref, --native_window_id, or --title"
        .to_string(),
    );
  }

  candidates
    .into_iter()
    .find(|candidate| candidate.is_fully_contained_in_display)
    .ok_or_else(|| {
      "could not resolve a fully contained visible window; inspect `debug.listWindows`".to_string()
    })
}

pub fn resolve_window_candidate_for_input(
  snapshot: &ObservedWindowSnapshot,
  resolved_app: &ResolvedAppRef,
  displays: &[DisplayDescriptor],
  selection: &WindowSelection,
) -> AuvResult<WindowCandidate> {
  let candidates = build_window_candidates(snapshot, resolved_app, displays)?;
  if selection.has_selector() {
    let filtered = filter_window_candidates(&candidates, selection);
    if filtered.len() == 1 {
      return Ok(filtered[0].clone());
    }
    if filtered.len() > 1 {
      let selector_detail = if selection.title.is_some() {
        "title"
      } else if selection.window_ref.is_some() {
        "window_ref"
      } else if selection.native_window_id.is_some() {
        "native_window_id"
      } else {
        "selector"
      };
      return Err(format!(
        "multiple window candidates matched {selector_detail}; inspect `debug.listWindows` and provide --window_ref or --native_window_id"
      ));
    }
    return Err(
      "no window candidate matched the explicit selector; inspect `debug.listWindows` and refresh --window_ref, --native_window_id, or --title"
        .to_string(),
    );
  }

  candidates.into_iter().next().ok_or_else(|| {
    "could not resolve a visible window for input; inspect `debug.listWindows`".to_string()
  })
}

pub fn retry_window_capture_operation<T, F>(mut operation: F) -> AuvResult<T>
where
  F: FnMut() -> AuvResult<T>,
{
  let retry_delays = [150_u64, 300_u64];
  for (attempt_index, delay_ms) in retry_delays.iter().copied().enumerate() {
    match operation() {
      Ok(value) => return Ok(value),
      Err(error) if is_retryable_window_capture_error(&error) => {
        let _ = attempt_index;
        thread::sleep(Duration::from_millis(delay_ms));
      }
      Err(error) => return Err(error),
    }
  }
  operation()
}

pub fn is_retryable_window_capture_error(error: &str) -> bool {
  error.contains("could not resolve a fully contained visible window")
    || error.contains("refreshed window is not fully contained by one display")
    // Window enumeration → screenshot races: macOS reassigns window IDs while
    // the candidate is being captured, so the ID we selected vanishes between
    // the two enumerations. Always transient, always worth retrying.
    || error.contains("disappeared before capture")
}

pub fn window_capture_readiness_diagnostic(
  candidate: &WindowCandidate,
  displays: &[DisplayDescriptor],
) -> String {
  let display_bounds = displays
    .iter()
    .map(|display| {
      format!(
        "{}={:.3},{:.3},{:.3},{:.3}",
        display.display_ref,
        display.global_logical_bounds.x,
        display.global_logical_bounds.y,
        display.global_logical_bounds.width,
        display.global_logical_bounds.height
      )
    })
    .collect::<Vec<_>>()
    .join("; ");
  format!(
    "selected window window_{} is not fully contained by one display; windowBounds={}; displayBounds=[{}]; inspect `debug.listWindows` or choose a fully visible window",
    candidate.window_ref.window_number,
    render_observed_rect_compact(&candidate.window_ref.bounds),
    display_bounds
  )
}

fn build_resolved_app_ref(
  selector: &AppSelector,
  resolved_bundle_id: Option<String>,
  windows: &[&ObservedWindow],
  match_strategy: &str,
) -> ResolvedAppRef {
  let resolved_app_name = windows
    .iter()
    .max_by_key(|window| {
      (
        if !window.title.trim().is_empty() {
          1
        } else {
          0
        },
        window_area(window),
      )
    })
    .map(|window| window.app_name.clone())
    .or_else(|| selector.app_name_hint.clone())
    .unwrap_or_else(|| selector.raw.clone());

  let owner_pids = windows
    .iter()
    .map(|window| window.owner_pid)
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>();

  ResolvedAppRef {
    selector: selector.clone(),
    resolved_bundle_id,
    resolved_app_name,
    owner_pids,
    match_strategy: match_strategy.to_string(),
  }
}

fn first_non_empty_bundle_id(windows: &[&ObservedWindow]) -> Option<String> {
  windows.iter().find_map(|window| {
    (!window.owner_bundle_id.trim().is_empty()).then(|| window.owner_bundle_id.clone())
  })
}

pub fn filter_windows_for_app<'a>(
  windows: &'a [ObservedWindow],
  resolved_app: &ResolvedAppRef,
) -> Vec<&'a ObservedWindow> {
  windows
    .iter()
    .filter(|window| window_matches_resolved_app(window, resolved_app))
    .collect()
}

fn window_matches_resolved_app(window: &ObservedWindow, resolved_app: &ResolvedAppRef) -> bool {
  if let Some(bundle_id) = resolved_app.resolved_bundle_id.as_deref()
    && !window.owner_bundle_id.trim().is_empty()
  {
    return window.owner_bundle_id.eq_ignore_ascii_case(bundle_id);
  }

  if resolved_app.owner_pids.contains(&window.owner_pid) {
    return true;
  }

  window
    .app_name
    .eq_ignore_ascii_case(&resolved_app.resolved_app_name)
}

fn is_substantial_window(window: &ObservedWindow) -> bool {
  window.bounds.width >= 160 && window.bounds.height >= 120
}

fn filter_window_candidates<'a>(
  candidates: &'a [WindowCandidate],
  selection: &WindowSelection,
) -> Vec<&'a WindowCandidate> {
  candidates
    .iter()
    .filter(|candidate| {
      selection.window_ref.as_ref().is_none_or(|expected| {
        expected == &candidate.window_ref.window_number.to_string()
          || expected == &format!("window_{}", candidate.window_ref.window_number)
      })
    })
    .filter(|candidate| {
      selection
        .native_window_id
        .as_ref()
        .is_none_or(|expected| candidate.native_window_id.as_ref() == Some(expected))
    })
    .filter(|candidate| {
      selection
        .title
        .as_ref()
        .is_none_or(|expected| candidate.window_ref.title == expected.as_str())
    })
    .collect()
}

fn containing_display<'a>(
  bounds: &ObservedRect,
  displays: &'a [DisplayDescriptor],
) -> Option<&'a DisplayDescriptor> {
  displays.iter().find(|display| {
    let display_bounds = &display.global_logical_bounds;
    bounds.x as f64 >= display_bounds.x
      && bounds.y as f64 >= display_bounds.y
      && (bounds.x + bounds.width) as f64 <= display_bounds.x + display_bounds.width
      && (bounds.y + bounds.height) as f64 <= display_bounds.y + display_bounds.height
  })
}

fn render_observed_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

#[cfg(test)]
mod tests {
  use crate::capture::types::{CaptureBackend, DisplayDescriptor, Rect, Scale2D, Size};
  use crate::types::{
    AppSelector, ObservedRect, ObservedWindow, ObservedWindowSnapshot, ResolvedAppRef,
    WindowSelection,
  };

  use super::{resolve_window_candidate, resolve_window_candidate_for_input};

  #[test]
  fn input_window_candidate_allows_partially_visible_default_window() {
    let displays = sample_displays();
    let snapshot = sample_snapshot_with_partial_main_window();
    let resolved = sample_resolved_app();

    let candidate = resolve_window_candidate_for_input(
      &snapshot,
      &resolved,
      &displays,
      &WindowSelection::default(),
    )
    .expect("scroll/input candidate should resolve");

    assert_eq!(candidate.window_ref.window_number, 42);
    assert!(!candidate.is_fully_contained_in_display);
    assert_eq!(candidate.selection_reason, "largest-visible-normal-window");
  }

  #[test]
  fn capture_window_candidate_still_requires_fully_contained_default_window() {
    let displays = sample_displays();
    let snapshot = sample_snapshot_with_partial_main_window();
    let resolved = sample_resolved_app();

    let error =
      resolve_window_candidate(&snapshot, &resolved, &displays, &WindowSelection::default())
        .expect_err("capture candidate should reject partial windows");

    assert!(error.contains("fully contained visible window"));
  }

  fn sample_resolved_app() -> ResolvedAppRef {
    ResolvedAppRef {
      selector: AppSelector {
        raw: "com.example.music".to_string(),
        bundle_id: Some("com.example.music".to_string()),
        app_name_hint: None,
      },
      resolved_bundle_id: Some("com.example.music".to_string()),
      resolved_app_name: "ExampleMusic".to_string(),
      owner_pids: vec![10],
      match_strategy: "bundle-id-exact".to_string(),
    }
  }

  fn sample_snapshot_with_partial_main_window() -> ObservedWindowSnapshot {
    ObservedWindowSnapshot {
      frontmost_app_name: "ExampleMusic".to_string(),
      frontmost_app_bundle_id: "com.example.music".to_string(),
      frontmost_window_title: "Main".to_string(),
      observed_at: "test".to_string(),
      windows: vec![ObservedWindow {
        window_number: 42,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 10,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Main".to_string(),
        bounds: ObservedRect {
          x: 1500,
          y: 50,
          width: 1200,
          height: 800,
        },
      }],
    }
  }

  fn sample_displays() -> Vec<DisplayDescriptor> {
    vec![DisplayDescriptor {
      display_ref: "display_1".to_string(),
      is_main: true,
      is_builtin: true,
      global_logical_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
      },
      visible_logical_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
      },
      physical_pixel_size: Size {
        width: 3024.0,
        height: 1964.0,
      },
      scale_factor: 2.0,
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      native_display_id: "1".to_string(),
      capture_backend: CaptureBackend::XcapMacos,
    }]
  }
}
