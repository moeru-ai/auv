#[cfg(target_os = "macos")]
use std::cell::RefCell;

use auv_driver_overlay_common::layers::{Cursor, Outline, Status};

#[cfg(target_os = "macos")]
use super::binding::ffi::{NativeActionResponse, NativeOverlayController, make_overlay_controller};
use crate::AuvResult;

#[cfg(target_os = "macos")]
thread_local! {
  // TODO(overlay-renderer-lifecycle): explicit renderer disposal is deferred
  // because the current driver exposes show/remove rather than renderer
  // ownership; add RAII disposal when a session owns the native controller.
  static OVERLAY_CONTROLLER: RefCell<Option<NativeOverlayController>> = const { RefCell::new(None) };
}

/// Validates typed appearance before passing plain values across the Swift bridge.
#[cfg(target_os = "macos")]
fn native_shadow(shadow: Option<auv_driver_overlay_common::style::Shadow>) -> AuvResult<super::binding::ffi::NativeCursorShadow> {
  if let Some(shadow) = shadow {
    shadow.validate()?;
  }
  let enabled = shadow.is_some();
  let shadow = shadow.unwrap_or(auv_driver_overlay_common::style::Shadow {
    color: auv_driver_overlay_common::style::Color::CLEAR,
    blur_radius: 0.0,
    offset_x: 0.0,
    offset_y: 0.0,
  });
  Ok(super::binding::ffi::NativeCursorShadow {
    enabled,
    red: shadow.color.red,
    green: shadow.color.green,
    blue: shadow.color.blue,
    alpha: shadow.color.alpha,
    blur_radius: shadow.blur_radius,
    offset_x: shadow.offset_x,
    offset_y: shadow.offset_y,
  })
}

#[cfg(target_os = "macos")]
pub(crate) fn move_cursor(id: &str, cursor: &Cursor, variant: &str, duration_ms: u64) -> AuvResult<()> {
  let point = cursor.point().point();
  let style = cursor.style();
  let shadow = native_shadow(style.shadow)?;
  with_controller("move_overlay_cursor", |controller| {
    controller.move_overlay_cursor(
      id.to_string(),
      point.x,
      point.y,
      visible_cursor_label(cursor).to_string(),
      variant.to_string(),
      duration_ms,
      style.label_foreground.red,
      style.label_foreground.green,
      style.label_foreground.blue,
      style.label_foreground.alpha,
      style.label_background.red,
      style.label_background.green,
      style.label_background.blue,
      style.label_background.alpha,
      style.label_padding.top,
      style.label_padding.right,
      style.label_padding.bottom,
      style.label_padding.left,
      style.label_corner_radius,
      style.sprite_size,
      style.label_gap,
      shadow,
    )
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn move_cursor(_id: &str, _cursor: &Cursor, _variant: &str, _duration_ms: u64) -> AuvResult<()> {
  unsupported()
}

#[cfg(target_os = "macos")]
pub(crate) fn move_svg_cursor(id: &str, cursor: &Cursor, svg: &str, duration_ms: u64) -> AuvResult<()> {
  let point = cursor.point().point();
  let style = cursor.style();
  let shadow = native_shadow(style.shadow)?;
  with_controller("move_overlay_cursor_svg", |controller| {
    controller.move_overlay_cursor_svg(
      id.to_string(),
      point.x,
      point.y,
      visible_cursor_label(cursor).to_string(),
      svg.to_string(),
      duration_ms,
      style.label_foreground.red,
      style.label_foreground.green,
      style.label_foreground.blue,
      style.label_foreground.alpha,
      style.label_background.red,
      style.label_background.green,
      style.label_background.blue,
      style.label_background.alpha,
      style.label_padding.top,
      style.label_padding.right,
      style.label_padding.bottom,
      style.label_padding.left,
      style.label_corner_radius,
      style.sprite_size,
      style.label_gap,
      shadow,
    )
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn move_svg_cursor(_id: &str, _cursor: &Cursor, _svg: &str, _duration_ms: u64) -> AuvResult<()> {
  unsupported()
}

#[cfg(target_os = "macos")]
pub(crate) fn show_outline(id: &str, outline: &Outline, duration_ms: u64) -> AuvResult<()> {
  let rect = outline.rect();
  let style = outline.style();
  let stroke = style.stroke;
  with_controller("show_overlay_outline", |controller| {
    controller.show_overlay_outline(
      id.to_string(),
      rect.origin.x - style.padding.left,
      rect.origin.y - style.padding.top,
      rect.size.width + style.padding.left + style.padding.right,
      rect.size.height + style.padding.top + style.padding.bottom,
      visible_outline_label(outline).to_string(),
      stroke.color.red,
      stroke.color.green,
      stroke.color.blue,
      stroke.color.alpha,
      stroke.width,
      style.corner_radius,
      duration_ms,
    )
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_outline(_id: &str, _outline: &Outline, _duration_ms: u64) -> AuvResult<()> {
  unsupported()
}

#[cfg(target_os = "macos")]
pub(crate) fn show_status(id: &str, status: &Status, duration_ms: u64) -> AuvResult<()> {
  let point = status.point().point();
  let style = status.style();
  with_controller("show_overlay_status", |controller| {
    controller.show_overlay_status(
      id.to_string(),
      point.x,
      point.y,
      status.text().to_string(),
      style.foreground.red,
      style.foreground.green,
      style.foreground.blue,
      style.foreground.alpha,
      style.background.red,
      style.background.green,
      style.background.blue,
      style.background.alpha,
      style.padding.top,
      style.padding.right,
      style.padding.bottom,
      style.padding.left,
      style.corner_radius,
      duration_ms,
    )
  })
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn show_status(_id: &str, _status: &Status, _duration_ms: u64) -> AuvResult<()> {
  unsupported()
}

#[cfg(target_os = "macos")]
pub(crate) fn pump_events(duration_ms: u64) -> AuvResult<()> {
  action_result("pump_overlay_events", super::binding::ffi::pump_overlay_events(duration_ms))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn pump_events(_duration_ms: u64) -> AuvResult<()> {
  unsupported()
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_all() -> AuvResult<()> {
  with_controller("hide_overlay_cursor", |controller| controller.hide_overlay_cursor())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn hide_all() -> AuvResult<()> {
  unsupported()
}

fn visible_cursor_label(cursor: &Cursor) -> &str {
  if cursor.label_visible() {
    cursor.label().unwrap_or_default()
  } else {
    ""
  }
}

fn visible_outline_label(outline: &Outline) -> &str {
  if outline.label_visible() {
    outline.label().unwrap_or_default()
  } else {
    ""
  }
}

#[cfg(target_os = "macos")]
fn with_controller(operation: &str, action: impl FnOnce(&NativeOverlayController) -> NativeActionResponse) -> AuvResult<()> {
  OVERLAY_CONTROLLER.with(|cell| {
    if cell.borrow().is_none() {
      *cell.borrow_mut() = Some(make_overlay_controller());
    }
    let controller = cell.borrow();
    let controller = controller.as_ref().ok_or_else(|| "failed to initialize native overlay controller".to_string())?;
    action_result(operation, action(controller))
  })
}

#[cfg(target_os = "macos")]
fn action_result(operation: &str, response: NativeActionResponse) -> AuvResult<()> {
  crate::error::native_result(operation, response.ok.then_some(()), response.error_message, response.recovery_hint)
}

#[cfg(not(target_os = "macos"))]
fn unsupported<T>() -> AuvResult<T> {
  Err("macOS native overlay is unsupported on this target".to_string())
}
