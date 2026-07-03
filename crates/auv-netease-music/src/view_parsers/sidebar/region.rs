use crate::*;
use auv_driver::Size;
use auv_driver::geometry::{Point, WindowPoint};
#[cfg(target_os = "macos")]
use auv_driver::window::Window;

pub(crate) fn detect_sidebar_region(
  manual: Option<RatioRect>,
  window_size: Size,
  recognition: &TextRecognition,
) -> Result<ViewRegionRecord, ParserDiagnostic> {
  if let Some(region) = manual {
    return Ok(sidebar_region_record(ratio_to_window_bounds(
      region,
      window_size,
    )));
  }

  let left_limit = window_size.width * 0.38;
  let left_regions = recognition
    .regions
    .iter()
    .filter(|region| region.bounds.origin.x < left_limit)
    .collect::<Vec<_>>();
  let mut markers = left_regions
    .iter()
    .filter(|region| is_sidebar_marker(region.text.trim()))
    .map(|region| {
      (
        region.bounds.origin.x + region.bounds.size.width,
        region.bounds.origin.y,
        region.text.trim(),
      )
    })
    .collect::<Vec<_>>();

  if markers.is_empty() {
    return Err(ParserDiagnostic {
      code: "sidebar_region_not_found".to_string(),
      message: "sidebar markers could not be identified on the left side; refusing to infer sidebar bounds from unanchored list rows".to_string(),
      node_id: None,
    });
  }

  markers.sort_by(|left, right| {
    left
      .0
      .partial_cmp(&right.0)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  let max_x = markers
    .last()
    .map(|marker| marker.0)
    .unwrap_or_default()
    .max(window_size.width * 0.18)
    .min(window_size.width * 0.42);
  // Floor `window_size.height` at 0 before using it as the clamp upper
  // bound. `f64::clamp(0.0, h)` panics on `min > max` when `h < 0`, the
  // same shape fixed in `playlist_sidebar_bottom` for this module.
  let usable_height = window_size.height.max(0.0);
  let y_marker = markers
    .iter()
    .filter(|marker| is_playlist_section_marker(marker.2))
    .map(|marker| marker.1)
    .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    .unwrap_or(0.0)
    .clamp(0.0, usable_height);
  let bottom = playlist_sidebar_bottom(window_size);
  let y = expand_sidebar_playlist_body_top(y_marker, window_size);

  Ok(sidebar_region_record(ViewBounds::new(
    0.0,
    y,
    max_x + 48.0,
    bottom - y,
  )))
}

fn min_playlist_sidebar_body_height(window_size: Size) -> f64 {
  // NOTICE: Heuristic minimum playlist sidebar body height for logged-in layouts.
  // This is not a stable contract; tune against live SIGNOFF probes when the client
  // layout shifts.
  // REVIEW(netease-sidebar-min-body-height): revisit ratio/floor after owner live
  // re-probe on the default 1057×752 window.
  let usable_height = window_size.height.max(0.0);
  let ratio = 0.38 * usable_height;
  let floor = 240.0;
  let fallback_y = fallback_playlist_sidebar_region(window_size)
    .bounds
    .map(|bounds| bounds.y)
    .unwrap_or(0.0);
  let bottom = playlist_sidebar_bottom(window_size);
  let cap = (bottom - fallback_y).max(0.0);
  ratio.max(floor).min(cap)
}

fn expand_sidebar_playlist_body_top(y_marker: f64, window_size: Size) -> f64 {
  let bottom = playlist_sidebar_bottom(window_size);
  let fallback_y = fallback_playlist_sidebar_region(window_size)
    .bounds
    .map(|bounds| bounds.y)
    .unwrap_or(0.0);
  let min_body = min_playlist_sidebar_body_height(window_size);

  if bottom - y_marker < min_body {
    (bottom - min_body).max(fallback_y)
  } else {
    y_marker
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefaultScreenRestoreReason {
  SongDetailScreen,
  MissingSidebarRegion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DefaultScreenRestore {
  pub(crate) reason: DefaultScreenRestoreReason,
  pub(crate) point: Point,
}

pub(crate) fn detect_default_screen_restore(
  recognition: &TextRecognition,
  window_size: Size,
) -> Option<DefaultScreenRestore> {
  let screen = screen::classify_screen(recognition, window_size);
  let point = match screen.state() {
    screen::ScreenState::PlayingSongDetail => screen.restore_point()?,
    _ => return None,
  };

  Some(DefaultScreenRestore {
    reason: DefaultScreenRestoreReason::SongDetailScreen,
    point,
  })
}

pub(crate) fn song_detail_restore_point(_window_size: Size) -> Point {
  // NOTICE: This is a learned window-local logical point for the song-detail
  // back affordance. The older heuristic point `(40, 48)` landed left and below
  // the actual clickable target in the live macOS client.
  Point::new(82.602, 16.336)
}

#[cfg(target_os = "macos")]
pub(crate) fn click_default_screen_restore(
  session: &MacosDriverSession,
  window: &Window,
  point: Point,
) -> Result<(), String> {
  let lease = session
    .window()
    .prepare_for_input(
      window,
      PrepareForInputOptions {
        activation: ActivationPolicy::Foreground {
          settle: std::time::Duration::ZERO,
        },
        preserve_frontmost: false,
        install_focus_guard: false,
        settle: std::time::Duration::ZERO,
      },
    )
    .map_err(|error| format!("foreground preparation failed: {error}"))?;
  let global_x = window.frame.origin.x + point.x;
  let global_y = window.frame.origin.y + point.y;
  // NOTICE: Route this restore through the foreground global HID path. Some
  // app-rendered affordances do not reliably react to typed/window-targeted
  // clicks; `click_point` carries the mouse-move + settle behavior that makes
  // this class of click observable to those controls.
  let click_result = auv_driver_macos::native::pointer::click_point(global_x, global_y, 0, 1, 80);
  let restore_result = session.window().restore_input(lease);
  click_result.map_err(|error| format!("foreground restore click failed: {error}"))?;
  restore_result.map_err(|error| format!("foreground restore cleanup failed: {error}"))?;
  Ok(())
}

pub(crate) fn playlist_sidebar_bottom(window_size: Size) -> f64 {
  // Guard against non-positive / NaN heights before clamping. `f64::clamp`
  // panics on `min > max` (and on NaN bounds), and `(h - 82).clamp(0, h)`
  // becomes `clamp(_, 0, -h)` when `h < 0`. Treat such windows as having
  // no usable sidebar area.
  let height = window_size.height.max(0.0);
  (height - 82.0).clamp(0.0, height)
}

pub(crate) fn broad_sidebar_probe_bounds(window_size: Size) -> ViewBounds {
  // Floor window width at 0 before computing the probe. The
  // `(.max(280)).min(width * 0.42)` shape silently yields a negative probe
  // width when the input width is negative, which corrupts downstream
  // ViewBounds. Same family as the y-clamp fix in `detect_sidebar_region`.
  let width = window_size.width.max(0.0);
  let probe_width = (width * 0.24).max(280.0).min(width * 0.42);
  ViewBounds::new(0.0, 0.0, probe_width, playlist_sidebar_bottom(window_size))
}

pub(crate) fn sidebar_scroll_anchor(bounds: ViewBounds) -> WindowPoint {
  WindowPoint::new(
    bounds.x + bounds.width * 0.5,
    bounds.y + bounds.height * 0.75,
  )
}

pub(crate) fn fallback_playlist_sidebar_region(window_size: Size) -> ViewRegionRecord {
  // NOTICE(netease-sidebar-fallback): if OCR misses section headers, avoid the
  // full left rail because it can target library/navigation rows such as
  // "我喜欢的音乐". Start near the observed playlist section band instead;
  // replace this with AX/sidebar-scrollbar evidence when that preflight
  // contract is approved.
  // Floor window dimensions at 0 before computing fallback bounds. The
  // `(.max(constant)).min(dim * ratio)` shape silently emits negative y /
  // width values when the underlying dimension is negative.
  let usable_height = window_size.height.max(0.0);
  let usable_width = window_size.width.max(0.0);
  let y = (usable_height * 0.30).max(220.0).min(usable_height * 0.55);
  let width = (usable_width * 0.24).max(280.0).min(usable_width * 0.42);
  sidebar_region_record(ViewBounds::new(
    0.0,
    y,
    width,
    playlist_sidebar_bottom(window_size) - y,
  ))
}

pub(crate) fn sidebar_region_record(bounds: ViewBounds) -> ViewRegionRecord {
  ViewRegionRecord {
    id: None,
    name: Some("playlist_sidebar".to_string()),
    bounds: Some(bounds),
    coordinate_space: Some("window".to_string()),
  }
}

pub(crate) fn ratio_to_window_bounds(region: RatioRect, window_size: Size) -> ViewBounds {
  ViewBounds::new(
    region.x * window_size.width,
    region.y * window_size.height,
    region.width * window_size.width,
    region.height * window_size.height,
  )
}

pub(crate) fn is_sidebar_marker(label: &str) -> bool {
  SidebarSectionKind::from_label(label).is_known()
    || matches!(label, "推荐" | "发现音乐" | "最近播放")
}

pub(crate) fn is_playlist_section_marker(label: &str) -> bool {
  SidebarSectionKind::from_label(label).is_playlist_collection()
}

pub(crate) fn detect_blocking_modal(recognition: &TextRecognition) -> Option<ParserDiagnostic> {
  let has_cancel = recognition.best_contains("取消").is_some();
  let has_dialog_action =
    recognition.best_contains("打开").is_some() || recognition.best_contains("存储").is_some();

  (has_cancel && has_dialog_action).then(|| ParserDiagnostic {
    code: "blocking_modal_dialog".to_string(),
    message: "blocking open or save dialog markers were detected".to_string(),
    node_id: None,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::Size;

  #[test]
  fn playlist_sidebar_bottom_subtracts_panel_height_when_window_has_room() {
    assert_eq!(playlist_sidebar_bottom(Size::new(800.0, 600.0)), 518.0);
  }

  #[test]
  fn playlist_sidebar_bottom_clamps_to_zero_when_window_smaller_than_panel() {
    assert_eq!(playlist_sidebar_bottom(Size::new(800.0, 40.0)), 0.0);
  }

  #[test]
  fn playlist_sidebar_bottom_returns_zero_when_window_height_is_zero() {
    assert_eq!(playlist_sidebar_bottom(Size::new(800.0, 0.0)), 0.0);
  }

  #[test]
  fn playlist_sidebar_bottom_returns_zero_when_window_height_is_negative() {
    assert_eq!(playlist_sidebar_bottom(Size::new(800.0, -10.0)), 0.0);
  }
}
