use super::ax_tree::capture_ax_tree;
use super::capture::commands::{capture_display, capture_region, capture_window, list_displays};
use super::control::{
  activate_app, ax_click_window_text, ax_press_button, click_point, click_screen_row,
  click_screen_text, click_window_point, click_window_row, click_window_text, find_window_rows,
  find_window_text, focus_text_input, paste_text_preserve_clipboard, press_button, press_key,
  scroll_point, smart_press, type_text, wait_for_window_rows, wait_for_window_text,
};
use super::observe::{
  find_image_text, find_screen_rows, find_screen_text, identify_point, list_windows,
  probe_coordinate_readiness, probe_permissions, project_screenshot_point, verify_ax_text,
  verify_now_playing_title, wait_for_screen_rows, wait_for_screen_text,
};
use super::overlay::{
  overlay_click_point, overlay_hide_cursor, overlay_show_cursor, overlay_shutdown,
};
use super::{
  Driver, DriverCall, DriverDescriptor, DriverResponse, MacOsDesktopDriver, descriptor,
  require_macos,
};
use crate::model::AuvResult;

impl Driver for MacOsDesktopDriver {
  fn descriptor(&self) -> DriverDescriptor {
    descriptor::driver_descriptor()
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    invoke_operation(call)
  }
}

pub(crate) fn invoke_operation(call: &DriverCall) -> AuvResult<DriverResponse> {
  require_macos()?;

  match call.operation.as_str() {
    "capture_display" => capture_display(call),
    "capture_region" => capture_region(call),
    "capture_window" => capture_window(call),
    "probe_coordinate_readiness" => probe_coordinate_readiness(call),
    "list_displays" => list_displays(call),
    "project_screenshot_point" => project_screenshot_point(call),
    "identify_point" => identify_point(call),
    "list_windows" => list_windows(call),
    "capture_ax_tree" => capture_ax_tree(call),
    "find_screen_text" => find_screen_text(call),
    "wait_for_screen_text" => wait_for_screen_text(call),
    "find_screen_rows" => find_screen_rows(call),
    "wait_for_screen_rows" => wait_for_screen_rows(call),
    "find_window_text" => find_window_text(call),
    "wait_for_window_text" => wait_for_window_text(call),
    "find_window_rows" => find_window_rows(call),
    "wait_for_window_rows" => wait_for_window_rows(call),
    "find_image_text" => find_image_text(call),
    "probe_permissions" => probe_permissions(call),
    "verify_ax_text" => verify_ax_text(call),
    "verify_now_playing_title" => verify_now_playing_title(call),
    "activate_app" => activate_app(call),
    "focus_text_input" => focus_text_input(call),
    "press_button" => press_button(call),
    "ax_press_button" => ax_press_button(call),
    "ax_click_window_text" => ax_click_window_text(call),
    "smart_press" => smart_press(call),
    "type_text" => type_text(call),
    "paste_text_preserve_clipboard" => paste_text_preserve_clipboard(call),
    "press_key" => press_key(call),
    "click_point" => click_point(call),
    "click_window_point" => click_window_point(call),
    "click_screen_text" => click_screen_text(call),
    "click_screen_row" => click_screen_row(call),
    "click_window_text" => click_window_text(call),
    "click_window_row" => click_window_row(call),
    "scroll_point" => scroll_point(call),
    "overlay_show_cursor" => overlay_show_cursor(call),
    "overlay_hide_cursor" => overlay_hide_cursor(call),
    "overlay_shutdown" => overlay_shutdown(call),
    "overlay_click_point" => overlay_click_point(call),
    other => Err(format!(
      "driver macos.desktop does not support operation {}",
      other
    )),
  }
}
