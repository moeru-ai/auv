pub(crate) const PROBE_ACCESSIBILITY_SCRIPT: &str =
  include_str!("scripts/probe_accessibility.swift");
pub(crate) const PROBE_SCREEN_RECORDING_SCRIPT: &str =
  include_str!("scripts/probe_screen_recording.swift");
pub(crate) const ENUMERATE_DISPLAYS_SCRIPT: &str = include_str!("scripts/enumerate_displays.swift");
pub(crate) const OBSERVE_WINDOWS_SCRIPT_TEMPLATE: &str =
  include_str!("scripts/observe_windows.swift");
pub(crate) const OBSERVE_WINDOW_TREE_SCRIPT_TEMPLATE: &str =
  include_str!("scripts/observe_window_tree.swift");
pub(crate) const OCR_FIND_TEXT_SCRIPT_TEMPLATE: &str = include_str!("scripts/ocr_find_text.swift");
pub(crate) const FIND_VISUAL_ROWS_SCRIPT_TEMPLATE: &str =
  include_str!("scripts/find_visual_rows.swift");
pub(crate) const CLICK_POINT_SCRIPT_TEMPLATE: &str = include_str!("scripts/click_point.swift");
pub(crate) const SCROLL_POINT_SCRIPT_TEMPLATE: &str = include_str!("scripts/scroll_point.swift");
pub(crate) const CAPTURE_CLIPBOARD_SCRIPT: &str = include_str!("scripts/capture_clipboard.swift");
pub(crate) const RESTORE_CLIPBOARD_SCRIPT_TEMPLATE: &str =
  include_str!("scripts/restore_clipboard.swift");
pub(crate) const SET_CLIPBOARD_TEXT_SCRIPT_TEMPLATE: &str =
  include_str!("scripts/set_clipboard_text.swift");
pub(crate) const OVERLAY_CURSOR_DAEMON_SCRIPT: &str =
  include_str!("scripts/overlay_cursor_daemon.swift");

pub(crate) const XCRUN_BINARY: &str = "/usr/bin/xcrun";
pub(crate) const OSASCRIPT_BINARY: &str = "/usr/bin/osascript";
