// File: src/catalog.rs
//! Built-in command catalog.
//!
//! The catalog is the *registry* of command IDs -> driver operations, including
//! disturbance gates (`DisturbanceClass`) used by skill planning/validation.
//! It does not execute anything: execution lives in `runtime` + drivers.
//!
//! Note: some entries (e.g. `music.search.results` -> `music.result.play`) are
//! intentionally "typed consumer" paths that demonstrate consuming
//! `OperationResult`/`CandidateRef` evidence instead of only dumping artifacts.

use crate::model::{CommandNamespace, CommandSpec, DisturbanceClass};

const DISPLAY_LIST: &str = "display.list";
const DISPLAY_CAPTURE: &str = "display.capture";
const DISPLAY_CAPTURE_REGION: &str = "display.captureRegion";
const DISPLAY_IDENTIFY_POINT: &str = "display.identifyPoint";
const DISPLAY_PROJECT_SCREENSHOT_POINT: &str = "display.projectScreenshotPoint";
const DISPLAY_PROBE_COORDINATE_READINESS: &str = "display.probeCoordinateReadiness";
const SCREEN_FIND_TEXT: &str = "screen.findText";
const SCREEN_WAIT_FOR_TEXT: &str = "screen.waitForText";
const SCREEN_FIND_ROWS: &str = "screen.findRows";
const SCREEN_WAIT_FOR_ROWS: &str = "screen.waitForRows";
const SCREEN_FIND_IMAGE_TEXT: &str = "screen.findImageText";
const SCREEN_CLICK_TEXT: &str = "screen.clickText";
const SCREEN_CLICK_ROW: &str = "screen.clickRow";
const WINDOW_LIST: &str = "window.list";
const WINDOW_CAPTURE: &str = "window.capture";
const WINDOW_CAPTURE_AX_TREE: &str = "window.captureAxTree";
const WINDOW_FIND_TEXT: &str = "window.findText";
const WINDOW_WAIT_FOR_TEXT: &str = "window.waitForText";
const WINDOW_FIND_ROWS: &str = "window.findRows";
const WINDOW_WAIT_FOR_ROWS: &str = "window.waitForRows";
const WINDOW_OBSERVE_REGION: &str = "window.observeRegion";
const WINDOW_FIND_ICON_MATCH: &str = "window.findIconMatch";
const WINDOW_SCROLL_REGION: &str = "window.scrollRegion";
const WINDOW_CLICK_TEXT: &str = "window.clickText";
const WINDOW_CLICK_ROW: &str = "window.clickRow";
const WINDOW_VERIFY_TEXT: &str = "window.verifyText";
const INPUT_FOCUS_TEXT: &str = "input.focusText";
const INPUT_PRESS_BUTTON: &str = "input.pressButton";
const INPUT_AX_PRESS_BUTTON: &str = "input.axPressButton";
const INPUT_AX_FOCUS_TEXT: &str = "input.axFocusText";
const INPUT_AX_CLICK_WINDOW_TEXT: &str = "input.axClickWindowText";
const INPUT_SMART_PRESS: &str = "input.smartPress";
const INPUT_TYPE_TEXT: &str = "input.typeText";
const INPUT_PASTE_TEXT: &str = "input.pasteText";
const INPUT_KEY: &str = "input.key";
const INPUT_CLICK_POINT: &str = "input.clickPoint";
const INPUT_CLICK_WINDOW_POINT: &str = "input.clickWindowPoint";
const INPUT_TEACH_CLICK: &str = "input.teachClick";
const INPUT_SCROLL_POINT: &str = "input.scrollPoint";
const INPUT_OVERLAY_CLICK_POINT: &str = "input.overlayClickPoint";
const APP_ACTIVATE: &str = "app.activate";
const APP_PROBE_PERMISSIONS: &str = "app.probePermissions";
const OVERLAY_SHOW_CURSOR: &str = "overlay.showCursor";
const OVERLAY_SHOW_DUAL_CURSOR: &str = "overlay.showDualCursor";
const OVERLAY_APPLY_CURSOR_BATCH: &str = "overlay.applyCursorBatch";
const OVERLAY_SET_CURSOR: &str = "overlay.setCursor";
const OVERLAY_MOVE_CURSOR: &str = "overlay.moveCursor";
const OVERLAY_MOVE_CURSOR_BY_ID: &str = "overlay.moveCursorById";
const OVERLAY_FLASH_CURSOR: &str = "overlay.flashCursor";
const OVERLAY_FLASH_CURSOR_BY_ID: &str = "overlay.flashCursorById";
const OVERLAY_HIDE_CURSOR_ID: &str = "overlay.hideCursorId";
const OVERLAY_HIDE_CURSOR: &str = "overlay.hideCursor";
const OVERLAY_SHUTDOWN: &str = "overlay.shutdown";
const FIXTURE_OBSERVE: &str = "fixture.observe";
const MEDIA_CONTROL_NOW_PLAYING: &str = "mediaControl.nowPlaying";

pub fn render_invoke_help(command_id: Option<&str>) -> Result<String, String> {
  let catalog = invoke_discovery_catalog();
  match command_id {
    None => Ok(render_invoke_help_index(&catalog)),
    Some(command_id) => render_invoke_command_help(&catalog, command_id),
  }
}

fn render_invoke_help_index(catalog: &InvokeDiscoveryCatalog) -> String {
  let mut sections = Vec::new();
  for (title, commands) in [
    (
      "DISPLAY",
      collect_commands(
        catalog,
        &[
          DISPLAY_LIST,
          DISPLAY_CAPTURE,
          DISPLAY_CAPTURE_REGION,
          DISPLAY_IDENTIFY_POINT,
          DISPLAY_PROJECT_SCREENSHOT_POINT,
          DISPLAY_PROBE_COORDINATE_READINESS,
        ],
      ),
    ),
    (
      "SCREEN",
      collect_commands(
        catalog,
        &[
          SCREEN_FIND_TEXT,
          SCREEN_WAIT_FOR_TEXT,
          SCREEN_FIND_ROWS,
          SCREEN_WAIT_FOR_ROWS,
          SCREEN_FIND_IMAGE_TEXT,
          SCREEN_CLICK_TEXT,
          SCREEN_CLICK_ROW,
        ],
      ),
    ),
    (
      "WINDOW",
      collect_commands(
        catalog,
        &[
          WINDOW_LIST,
          WINDOW_CAPTURE,
          WINDOW_CAPTURE_AX_TREE,
          WINDOW_FIND_TEXT,
          WINDOW_WAIT_FOR_TEXT,
          WINDOW_FIND_ROWS,
          WINDOW_WAIT_FOR_ROWS,
          WINDOW_OBSERVE_REGION,
          WINDOW_FIND_ICON_MATCH,
          WINDOW_SCROLL_REGION,
          WINDOW_CLICK_TEXT,
          WINDOW_CLICK_ROW,
          WINDOW_VERIFY_TEXT,
        ],
      ),
    ),
    (
      "INPUT",
      collect_commands(
        catalog,
        &[
          INPUT_FOCUS_TEXT,
          INPUT_PRESS_BUTTON,
          INPUT_AX_PRESS_BUTTON,
          INPUT_AX_FOCUS_TEXT,
          INPUT_AX_CLICK_WINDOW_TEXT,
          INPUT_SMART_PRESS,
          INPUT_TYPE_TEXT,
          INPUT_PASTE_TEXT,
          INPUT_KEY,
          INPUT_CLICK_POINT,
          INPUT_CLICK_WINDOW_POINT,
          INPUT_TEACH_CLICK,
          INPUT_SCROLL_POINT,
          INPUT_OVERLAY_CLICK_POINT,
        ],
      ),
    ),
    (
      "APP",
      collect_commands(catalog, &[APP_ACTIVATE, APP_PROBE_PERMISSIONS]),
    ),
    (
      "OVERLAY",
      collect_commands(
        catalog,
        &[
          OVERLAY_SHOW_CURSOR,
          OVERLAY_SHOW_DUAL_CURSOR,
          OVERLAY_APPLY_CURSOR_BATCH,
          OVERLAY_SET_CURSOR,
          OVERLAY_MOVE_CURSOR,
          OVERLAY_MOVE_CURSOR_BY_ID,
          OVERLAY_FLASH_CURSOR,
          OVERLAY_FLASH_CURSOR_BY_ID,
          OVERLAY_HIDE_CURSOR_ID,
          OVERLAY_HIDE_CURSOR,
          OVERLAY_SHUTDOWN,
        ],
      ),
    ),
    (
      "MEDIA CONTROL",
      collect_commands(catalog, &[MEDIA_CONTROL_NOW_PLAYING]),
    ),
  ] {
    if commands.is_empty() {
      continue;
    }
    sections.push(format!("{title}\n{}", commands.join("\n")));
  }

  let section_text = if sections.is_empty() {
    String::new()
  } else {
    format!("\n\n{}", sections.join("\n\n"))
  };

  format!(
    "USAGE\n  auv-cli invoke <command> [options]{section_text}\n\nUse `auv-cli invoke <command> --help` for command-specific options.\n"
  )
}

fn render_invoke_command_help(
  catalog: &InvokeDiscoveryCatalog,
  command_id: &str,
) -> Result<String, String> {
  let command = catalog.resolve(command_id).ok_or_else(|| {
    format!("unknown command {command_id}; use `invoke --help` to inspect available entries")
  })?;

  Ok(format!(
    "COMMAND\n  {}\n\nSUMMARY\n  {}\n\nBACKEND\n  {}.{}\n\nDISTURBANCE\n  {} (max: {})\n",
    command.id,
    command.summary,
    command.driver_id,
    command.operation,
    command
      .disturbance_classes
      .iter()
      .map(|class| class.as_str())
      .collect::<Vec<_>>()
      .join(", "),
    command.max_disturbance.as_str()
  ))
}

fn collect_commands(catalog: &InvokeDiscoveryCatalog, command_ids: &[&str]) -> Vec<String> {
  command_ids
    .iter()
    .filter_map(|command_id| catalog.resolve(command_id))
    .map(|command| format!("  {}\n    {}", command.id, command.summary))
    .collect()
}

fn command_by_id(command_id: &str) -> CommandSpec {
  default_command_catalog()
    .resolve(command_id)
    .unwrap_or_else(|| panic!("missing command in discovery catalog: {command_id}"))
    .clone()
}

const OBSERVE: CommandNamespace = CommandNamespace::Observe;
const ACTION: CommandNamespace = CommandNamespace::Action;
const VERIFY: CommandNamespace = CommandNamespace::Verify;
const OVERLAY: CommandNamespace = CommandNamespace::Overlay;
const DOMAIN: CommandNamespace = CommandNamespace::Domain;
#[cfg(test)]
const TEST: CommandNamespace = CommandNamespace::Test;

const NONE: &[DisturbanceClass] = &[DisturbanceClass::None];
const NONE_OR_FOREGROUND: &[DisturbanceClass] =
  &[DisturbanceClass::None, DisturbanceClass::ForegroundApp];
const FOREGROUND_KEYBOARD: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Keyboard];
const FOREGROUND_KEYBOARD_CLIPBOARD: &[DisturbanceClass] = &[
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Clipboard,
];
const FOREGROUND_ONLY: &[DisturbanceClass] = &[DisturbanceClass::ForegroundApp];
const FOCUS_POINTER_ENTRY: &[DisturbanceClass] = &[
  DisturbanceClass::Focus,
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Pointer,
];
const POINTER_WITH_FOREGROUND: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Pointer];
const PRESS_BUTTON_DISTURBANCE: &[DisturbanceClass] = &[
  DisturbanceClass::ForegroundApp,
  DisturbanceClass::Keyboard,
  DisturbanceClass::Pointer,
];
const CAPTURE_AX_TREE_DISTURBANCE: &[DisturbanceClass] =
  &[DisturbanceClass::ForegroundApp, DisturbanceClass::Keyboard];

pub struct CommandCatalog {
  commands: Vec<CommandSpec>,
}

pub struct InvokeDiscoveryCatalog {
  commands: Vec<CommandSpec>,
}

impl CommandCatalog {
  pub fn new(commands: Vec<CommandSpec>) -> Self {
    Self { commands }
  }

  pub fn resolve(&self, command_id: &str) -> Option<&CommandSpec> {
    self
      .commands
      .iter()
      .find(|command| command.id == command_id)
  }

  pub fn all(&self) -> &[CommandSpec] {
    &self.commands
  }
}

impl InvokeDiscoveryCatalog {
  pub fn new(commands: Vec<CommandSpec>) -> Self {
    Self { commands }
  }

  pub fn resolve(&self, command_id: &str) -> Option<&CommandSpec> {
    self
      .commands
      .iter()
      .find(|command| command.id == command_id)
  }

  pub fn all(&self) -> &[CommandSpec] {
    &self.commands
  }
}

pub fn invoke_discovery_catalog() -> InvokeDiscoveryCatalog {
  let commands = vec![
    command_by_id(DISPLAY_LIST),
    command_by_id(DISPLAY_CAPTURE),
    command_by_id(DISPLAY_CAPTURE_REGION),
    command_by_id(DISPLAY_IDENTIFY_POINT),
    command_by_id(DISPLAY_PROJECT_SCREENSHOT_POINT),
    command_by_id(DISPLAY_PROBE_COORDINATE_READINESS),
    command_by_id(SCREEN_FIND_TEXT),
    command_by_id(SCREEN_WAIT_FOR_TEXT),
    command_by_id(SCREEN_FIND_ROWS),
    command_by_id(SCREEN_WAIT_FOR_ROWS),
    command_by_id(SCREEN_FIND_IMAGE_TEXT),
    command_by_id(SCREEN_CLICK_TEXT),
    command_by_id(SCREEN_CLICK_ROW),
    command_by_id(WINDOW_LIST),
    command_by_id(WINDOW_CAPTURE),
    command_by_id(WINDOW_CAPTURE_AX_TREE),
    command_by_id(WINDOW_FIND_TEXT),
    command_by_id(WINDOW_WAIT_FOR_TEXT),
    command_by_id(WINDOW_FIND_ROWS),
    command_by_id(WINDOW_WAIT_FOR_ROWS),
    command_by_id(WINDOW_OBSERVE_REGION),
    command_by_id(WINDOW_FIND_ICON_MATCH),
    command_by_id(WINDOW_SCROLL_REGION),
    command_by_id(WINDOW_CLICK_TEXT),
    command_by_id(WINDOW_CLICK_ROW),
    command_by_id(WINDOW_VERIFY_TEXT),
    command_by_id(INPUT_FOCUS_TEXT),
    command_by_id(INPUT_PRESS_BUTTON),
    command_by_id(INPUT_AX_PRESS_BUTTON),
    command_by_id(INPUT_AX_FOCUS_TEXT),
    command_by_id(INPUT_AX_CLICK_WINDOW_TEXT),
    command_by_id(INPUT_SMART_PRESS),
    command_by_id(INPUT_TYPE_TEXT),
    command_by_id(INPUT_PASTE_TEXT),
    command_by_id(INPUT_KEY),
    command_by_id(INPUT_CLICK_POINT),
    command_by_id(INPUT_CLICK_WINDOW_POINT),
    command_by_id(INPUT_TEACH_CLICK),
    command_by_id(INPUT_SCROLL_POINT),
    command_by_id(INPUT_OVERLAY_CLICK_POINT),
    command_by_id(APP_ACTIVATE),
    command_by_id(APP_PROBE_PERMISSIONS),
    command_by_id(OVERLAY_SHOW_CURSOR),
    command_by_id(OVERLAY_SHOW_DUAL_CURSOR),
    command_by_id(OVERLAY_APPLY_CURSOR_BATCH),
    command_by_id(OVERLAY_SET_CURSOR),
    command_by_id(OVERLAY_MOVE_CURSOR),
    command_by_id(OVERLAY_MOVE_CURSOR_BY_ID),
    command_by_id(OVERLAY_FLASH_CURSOR),
    command_by_id(OVERLAY_FLASH_CURSOR_BY_ID),
    command_by_id(OVERLAY_HIDE_CURSOR_ID),
    command_by_id(OVERLAY_HIDE_CURSOR),
    command_by_id(OVERLAY_SHUTDOWN),
    command_by_id(MEDIA_CONTROL_NOW_PLAYING),
  ];
  InvokeDiscoveryCatalog::new(commands)
}

pub fn default_command_catalog() -> CommandCatalog {
  let commands = vec![
    CommandSpec {
      id: DISPLAY_CAPTURE,
      namespace: OBSERVE,
      summary: "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_display",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: DISPLAY_CAPTURE_REGION,
      namespace: OBSERVE,
      summary: "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_region",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: WINDOW_CAPTURE,
      namespace: OBSERVE,
      summary: "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_window",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: DISPLAY_LIST,
      namespace: OBSERVE,
      summary: "List connected displays using the normalized AUV coordinate contract.",
      driver_id: "macos.desktop",
      operation: "list_displays",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: DISPLAY_PROJECT_SCREENSHOT_POINT,
      namespace: OBSERVE,
      summary: "Project main-display screenshot pixels back into AUV global logical coordinates.",
      driver_id: "macos.desktop",
      operation: "project_screenshot_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: DISPLAY_IDENTIFY_POINT,
      namespace: OBSERVE,
      summary: "Resolve a logical desktop point against the current macOS display layout.",
      driver_id: "macos.desktop",
      operation: "identify_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: DISPLAY_PROBE_COORDINATE_READINESS,
      namespace: OBSERVE,
      summary: "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
      driver_id: "macos.desktop",
      operation: "probe_coordinate_readiness",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: SCREEN_FIND_TEXT,
      namespace: OBSERVE,
      summary: "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: SCREEN_WAIT_FOR_TEXT,
      namespace: OBSERVE,
      summary: "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: SCREEN_FIND_ROWS,
      namespace: OBSERVE,
      summary: "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: SCREEN_WAIT_FOR_ROWS,
      namespace: OBSERVE,
      summary: "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: SCREEN_FIND_IMAGE_TEXT,
      namespace: OBSERVE,
      summary: "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
      driver_id: "macos.desktop",
      operation: "find_image_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_FIND_TEXT,
      namespace: OBSERVE,
      summary: "Capture a resolved window and locate OCR text anchors in window pixel space.",
      driver_id: "macos.desktop",
      operation: "find_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_WAIT_FOR_TEXT,
      namespace: OBSERVE,
      summary: "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
      driver_id: "macos.desktop",
      operation: "wait_for_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_FIND_ROWS,
      namespace: OBSERVE,
      summary: "Detect visible OCR row bands inside a resolved window.",
      driver_id: "macos.desktop",
      operation: "find_window_rows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_WAIT_FOR_ROWS,
      namespace: OBSERVE,
      summary: "Poll resolved-window row detection until enough rows appear or the timeout expires.",
      driver_id: "macos.desktop",
      operation: "wait_for_window_rows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    // REVIEW: Public names are explicit window-scoped variants for the first
    // implementation. Revisit before introducing screen or generic region scans.
    CommandSpec {
      id: WINDOW_OBSERVE_REGION,
      namespace: OBSERVE,
      summary: "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
      driver_id: "macos.desktop",
      operation: "observe_window_region",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_FIND_ICON_MATCH,
      namespace: OBSERVE,
      summary: "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
      driver_id: "macos.desktop",
      operation: "find_icon_match",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_SCROLL_REGION,
      namespace: ACTION,
      summary: "Scroll at the center of a resolved macOS window region and record scroll evidence.",
      driver_id: "macos.desktop",
      operation: "scroll_window_region",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: MEDIA_CONTROL_NOW_PLAYING,
      namespace: VERIFY,
      summary: "Verify the current now-playing title from the observed AX tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_now_playing_title",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_VERIFY_TEXT,
      namespace: VERIFY,
      summary: "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_ax_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_LIST,
      namespace: OBSERVE,
      summary: "List visible macOS window candidates using the normalized AUV window selector model.",
      driver_id: "macos.desktop",
      operation: "list_windows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: WINDOW_CAPTURE_AX_TREE,
      namespace: OBSERVE,
      summary: "Capture an AX tree snapshot for a target macOS app window.",
      driver_id: "macos.desktop",
      operation: "capture_ax_tree",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: APP_PROBE_PERMISSIONS,
      namespace: OBSERVE,
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.desktop",
      operation: "probe_permissions",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: APP_ACTIVATE,
      namespace: ACTION,
      summary: "Bring a target macOS app to the foreground before a foreground-dependent step.",
      driver_id: "macos.desktop",
      operation: "activate_app",
      disturbance_classes: FOREGROUND_ONLY,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: INPUT_FOCUS_TEXT,
      namespace: ACTION,
      summary: "Focus a target macOS text input through AX, either by legacy --query text or by a promoted --candidate JSON payload carrying the typed search-entry contract candidate.",
      driver_id: "macos.desktop",
      operation: "focus_text_input",
      disturbance_classes: FOCUS_POINTER_ENTRY,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_PRESS_BUTTON,
      namespace: ACTION,
      summary: "Press a known macOS button-like control by query through AX.",
      driver_id: "macos.desktop",
      operation: "press_button",
      disturbance_classes: PRESS_BUTTON_DISTURBANCE,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_AX_PRESS_BUTTON,
      namespace: ACTION,
      summary: "Press a control by query via AXUIElementPerformAction; does not warp the real cursor (cursorDisturbance=none). Pass --overlay true to draw a visual AUV cursor over the target during the press for the dual-cursor effect. Falls back with an error when the AX target has no matching action; use input.pressButton for non-AX-pressable targets.",
      driver_id: "macos.desktop",
      operation: "ax_press_button",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: INPUT_AX_FOCUS_TEXT,
      namespace: ACTION,
      summary: "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute); does not warp the real cursor (cursorDisturbance=none, focusMechanism=ax-attribute). Pass --overlay true for the dual-cursor visual (auv replay cursor animates to the target while the real cursor stays put). Errors when the target does not accept programmatic focus; use input.focusText if pointer warp is acceptable.",
      driver_id: "macos.desktop",
      operation: "ax_focus_text_input",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: INPUT_AX_CLICK_WINDOW_TEXT,
      namespace: ACTION,
      summary: "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction (cursorDisturbance=none). Pass --overlay true for the dual-cursor visual. Errors with a hint to window.clickText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
      driver_id: "macos.desktop",
      operation: "ax_click_window_text",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: INPUT_SMART_PRESS,
      namespace: ACTION,
      summary: "ActionResolver v0 debug press: try OCR-to-AX press first; if it fails and --allow_pointer_fallback is not false, fall back to pointer click. Records actionResolver.* signals plus the selected method, fallback reason, and disturbance metadata.",
      driver_id: "macos.desktop",
      operation: "smart_press",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_TYPE_TEXT,
      namespace: ACTION,
      summary: "Type text into the active macOS control through System Events.",
      driver_id: "macos.desktop",
      operation: "type_text",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: INPUT_PASTE_TEXT,
      namespace: ACTION,
      summary: "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
      driver_id: "macos.desktop",
      operation: "paste_text_preserve_clipboard",
      disturbance_classes: FOREGROUND_KEYBOARD_CLIPBOARD,
      max_disturbance: DisturbanceClass::Clipboard,
    },
    CommandSpec {
      id: INPUT_KEY,
      namespace: ACTION,
      summary: "Press a keyboard key or shortcut in the active macOS app through System Events.",
      driver_id: "macos.desktop",
      operation: "press_key",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: INPUT_CLICK_POINT,
      namespace: ACTION,
      summary: "Click a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_CLICK_WINDOW_POINT,
      namespace: ACTION,
      summary: "Click a point relative to a target macOS window and record the resolved global point, either from legacy --relative_x/--relative_y inputs or from a promoted --candidate JSON payload carrying the typed window-action contract candidate.",
      driver_id: "macos.desktop",
      operation: "click_window_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_TEACH_CLICK,
      namespace: ACTION,
      summary: "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
      driver_id: "macos.desktop",
      operation: "teach_click",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: SCREEN_CLICK_TEXT,
      namespace: ACTION,
      summary: "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: SCREEN_CLICK_ROW,
      namespace: ACTION,
      summary: "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: WINDOW_CLICK_TEXT,
      namespace: ACTION,
      summary: "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: WINDOW_CLICK_ROW,
      namespace: ACTION,
      summary: "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_SCROLL_POINT,
      namespace: ACTION,
      summary: "Scroll at a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "scroll_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: INPUT_OVERLAY_CLICK_POINT,
      namespace: ACTION,
      summary: "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. Legacy visualization command path; the real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
      driver_id: "macos.desktop",
      operation: "overlay_click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: OVERLAY_SHOW_CURSOR,
      namespace: OVERLAY,
      summary: "Show a visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_show_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_SHOW_DUAL_CURSOR,
      namespace: OVERLAY,
      summary: "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
      driver_id: "macos.desktop",
      operation: "overlay_show_dual_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_APPLY_CURSOR_BATCH,
      namespace: OVERLAY,
      summary: "Apply a JSON batch of visual-only overlay cursor operations in one process.",
      driver_id: "macos.desktop",
      operation: "overlay_apply_cursor_batch",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_SET_CURSOR,
      namespace: OVERLAY,
      summary: "Show or update one visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_set_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_MOVE_CURSOR,
      namespace: OVERLAY,
      summary: "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_MOVE_CURSOR_BY_ID,
      namespace: OVERLAY,
      summary: "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_FLASH_CURSOR,
      namespace: OVERLAY,
      summary: "Flash the AUV click-state cursor sprite at a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_FLASH_CURSOR_BY_ID,
      namespace: OVERLAY,
      summary: "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_HIDE_CURSOR_ID,
      namespace: OVERLAY,
      summary: "Hide one visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_HIDE_CURSOR,
      namespace: OVERLAY,
      summary: "Hide the visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: OVERLAY_SHUTDOWN,
      namespace: OVERLAY,
      summary: "Shut down the visual-only AUV cursor overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_shutdown",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: FIXTURE_OBSERVE,
      namespace: OBSERVE,
      summary: "Emit a deterministic observation result without touching the real UI.",
      driver_id: "fixture.observe",
      operation: "observe_fixture_scene",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "recognition.read.ratio",
      namespace: DOMAIN,
      summary: "Resolve a producer-exported recognition handle via --recognition_ref JSON (source_run_id + recognition_id + artifact_role) from run/artifact lineage and assert exactly one current/max numeric reading in the best recognized row, refusing on missing or ambiguous evidence.",
      driver_id: "macos.desktop",
      operation: "recognition_read_ratio",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.validate.candidate.liveness",
      namespace: DOMAIN,
      summary: "Resolve a music.search.results candidate via --candidate_ref JSON (legacy source_run_id + source_artifact_id + candidate_local_id still accepted) and verify its liveness preconditions (window_ref + anchor_recheck).",
      driver_id: "macos.desktop",
      operation: "music_validate_candidate_liveness",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.search.results",
      namespace: DOMAIN,
      summary: "Detect visible search-result rows in a resolved window, produce a typed OperationResult candidate-set artifact, and emit CandidateRef signals (including selected_candidate_ref when --selected_row_index is provided).",
      driver_id: "macos.desktop",
      operation: "music_search_results",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.result.play",
      namespace: DOMAIN,
      summary: "Consume a music.search.results candidate via --candidate_ref JSON (legacy source_run_id + source_artifact_id + candidate_local_id still accepted), re-check liveness, activate the resolved row, press play, and emit a typed VerificationResult.",
      driver_id: "macos.desktop",
      operation: "music_result_play",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "steam.library.list.v0",
      namespace: DOMAIN,
      summary: "List installed Steam library apps through auv-steam local appmanifest grounding and record the result into the shared run/artifact store.",
      driver_id: "fixture.observe",
      operation: "steam_library_list",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
  ];

  CommandCatalog::new(commands)
}

#[cfg(test)]
mod tests {
  use super::*;

  const RENAMED_COMMAND_CASES: &[(&str, &str)] = &[
    ("debug.captureRegion", DISPLAY_CAPTURE_REGION),
    (
      "debug.probeCoordinateReadiness",
      DISPLAY_PROBE_COORDINATE_READINESS,
    ),
    ("debug.findScreenText", SCREEN_FIND_TEXT),
    ("debug.waitForScreenText", SCREEN_WAIT_FOR_TEXT),
    ("debug.findScreenRows", SCREEN_FIND_ROWS),
    ("debug.waitForScreenRows", SCREEN_WAIT_FOR_ROWS),
    ("debug.clickScreenText", SCREEN_CLICK_TEXT),
    ("debug.clickScreenRow", SCREEN_CLICK_ROW),
    ("debug.findImageText", SCREEN_FIND_IMAGE_TEXT),
    ("debug.waitForWindowText", WINDOW_WAIT_FOR_TEXT),
    ("debug.findWindowRows", WINDOW_FIND_ROWS),
    ("debug.waitForWindowRows", WINDOW_WAIT_FOR_ROWS),
    ("debug.observeWindowRegion", WINDOW_OBSERVE_REGION),
    ("debug.findIconMatch", WINDOW_FIND_ICON_MATCH),
    ("debug.scrollWindowRegion", WINDOW_SCROLL_REGION),
    ("debug.clickWindowRow", WINDOW_CLICK_ROW),
    ("verify.axText", WINDOW_VERIFY_TEXT),
    ("debug.focusTextInput", INPUT_FOCUS_TEXT),
    ("debug.pressButton", INPUT_PRESS_BUTTON),
    ("debug.axPressButton", INPUT_AX_PRESS_BUTTON),
    ("debug.axFocusTextInput", INPUT_AX_FOCUS_TEXT),
    ("debug.axClickWindowText", INPUT_AX_CLICK_WINDOW_TEXT),
    ("debug.smartPress", INPUT_SMART_PRESS),
    ("debug.typeText", INPUT_TYPE_TEXT),
    ("debug.pasteTextPreserveClipboard", INPUT_PASTE_TEXT),
    ("debug.pressKey", INPUT_KEY),
    ("debug.clickPoint", INPUT_CLICK_POINT),
    ("debug.clickWindowPoint", INPUT_CLICK_WINDOW_POINT),
    ("debug.teachClick", INPUT_TEACH_CLICK),
    ("debug.scrollPoint", INPUT_SCROLL_POINT),
    ("debug.overlayClickPoint", INPUT_OVERLAY_CLICK_POINT),
    ("debug.activateApp", APP_ACTIVATE),
    ("debug.probePermissions", APP_PROBE_PERMISSIONS),
    ("debug.overlayShowCursor", OVERLAY_SHOW_CURSOR),
    ("debug.overlayShowDualCursor", OVERLAY_SHOW_DUAL_CURSOR),
    ("debug.overlayApplyCursorBatch", OVERLAY_APPLY_CURSOR_BATCH),
    ("debug.overlaySetCursor", OVERLAY_SET_CURSOR),
    ("debug.overlayMoveCursor", OVERLAY_MOVE_CURSOR),
    ("debug.overlayMoveCursorById", OVERLAY_MOVE_CURSOR_BY_ID),
    ("debug.overlayFlashCursor", OVERLAY_FLASH_CURSOR),
    ("debug.overlayFlashCursorById", OVERLAY_FLASH_CURSOR_BY_ID),
    ("debug.overlayHideCursorId", OVERLAY_HIDE_CURSOR_ID),
    ("debug.overlayHideCursor", OVERLAY_HIDE_CURSOR),
    ("debug.overlayShutdown", OVERLAY_SHUTDOWN),
    ("debug.fixtureObserve", FIXTURE_OBSERVE),
  ];

  const SURVIVOR_COMMAND_IDS: &[&str] = &[
    "music.validate.candidate.liveness",
    "music.search.results",
    "music.result.play",
    "recognition.read.ratio",
    "steam.library.list.v0",
    FIXTURE_OBSERVE,
  ];

  fn assert_catalog_resolves(catalog: &CommandCatalog, command_id: &str) {
    assert!(
      catalog.resolve(command_id).is_some(),
      "missing command {command_id}"
    );
  }

  #[test]
  fn default_catalog_renamed_ids_resolve_and_legacy_ids_fail() {
    let catalog = default_command_catalog();

    for (legacy_id, canonical_id) in RENAMED_COMMAND_CASES {
      assert!(
        catalog.resolve(legacy_id).is_none(),
        "legacy id {legacy_id} should not resolve"
      );
      assert_catalog_resolves(&catalog, canonical_id);
    }
  }

  #[test]
  fn default_catalog_has_no_debug_or_verify_prefix_ids() {
    let catalog = default_command_catalog();

    for command in catalog.all() {
      assert!(
        !command.id.starts_with("debug.") && !command.id.starts_with("verify."),
        "production command id should not keep legacy prefix: {}",
        command.id
      );
    }
  }

  #[test]
  fn survivor_commands_still_resolve_but_stay_hidden_from_discovery() {
    let catalog = default_command_catalog();
    let discovery = invoke_discovery_catalog();
    let help_text = render_invoke_help(None).expect("index help should render");

    for command_id in SURVIVOR_COMMAND_IDS {
      assert_catalog_resolves(&catalog, command_id);
      assert!(
        discovery.resolve(command_id).is_none(),
        "survivor {command_id} should stay hidden from discovery"
      );
      assert!(
        !help_text.contains(command_id),
        "survivor {command_id} should stay hidden from help index"
      );
    }
  }

  #[test]
  fn invoke_help_index_uses_canonical_ids_only() {
    let text = render_invoke_help(None).expect("index help should render");

    for command_id in [
      DISPLAY_CAPTURE_REGION,
      SCREEN_FIND_TEXT,
      WINDOW_OBSERVE_REGION,
      INPUT_TYPE_TEXT,
      APP_PROBE_PERMISSIONS,
      OVERLAY_SHOW_CURSOR,
      MEDIA_CONTROL_NOW_PLAYING,
    ] {
      assert!(
        text.contains(command_id),
        "help index should include {command_id}"
      );
    }

    assert!(!text.contains("debug."));
    assert!(!text.contains("verify."));
  }

  #[test]
  fn render_invoke_help_index_includes_expected_sections() {
    let text = render_invoke_help(None).expect("index help should render");
    assert!(text.contains("USAGE"));
    assert!(text.contains("DISPLAY"));
    assert!(text.contains("WINDOW"));
    assert!(text.contains("MEDIA CONTROL"));
    assert!(text.contains(DISPLAY_CAPTURE));
  }

  #[test]
  fn render_invoke_help_for_command_includes_metadata() {
    let text = render_invoke_help(Some(DISPLAY_CAPTURE)).expect("command help should render");
    assert!(text.contains("COMMAND"));
    assert!(text.contains(DISPLAY_CAPTURE));
    assert!(text.contains("BACKEND"));
    assert!(text.contains("macos.desktop.capture_display"));
    assert!(text.contains("DISTURBANCE"));
  }

  #[test]
  fn command_catalog_resolves_existing_command() {
    let catalog = CommandCatalog::new(vec![CommandSpec {
      id: "test.cmd",
      namespace: TEST,
      summary: "Test command",
      driver_id: "test.driver",
      operation: "test_op",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    }]);

    let resolved = catalog.resolve("test.cmd").expect("should resolve");
    assert_eq!(resolved.id, "test.cmd");
    assert_eq!(resolved.operation, "test_op");
  }

  #[test]
  fn command_catalog_returns_none_for_missing_command() {
    let catalog = CommandCatalog::new(vec![]);
    assert!(catalog.resolve("missing").is_none());
  }

  #[test]
  fn command_catalog_returns_all_commands() {
    let commands = vec![
      CommandSpec {
        id: "cmd1",
        namespace: crate::model::CommandNamespace::Test,
        summary: "sum1",
        driver_id: "d1",
        operation: "op1",
        disturbance_classes: NONE,
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "cmd2",
        namespace: crate::model::CommandNamespace::Test,
        summary: "sum2",
        driver_id: "d2",
        operation: "op2",
        disturbance_classes: NONE,
        max_disturbance: DisturbanceClass::None,
      },
    ];
    let catalog = CommandCatalog::new(commands.clone());
    let all = catalog.all();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "cmd1");
    assert_eq!(all[1].id, "cmd2");
  }

  #[test]
  fn default_catalog_always_exposes_observation_commands() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve(DISPLAY_CAPTURE).is_some());
    assert!(catalog.resolve(DISPLAY_CAPTURE_REGION).is_some());
    assert!(catalog.resolve(WINDOW_CAPTURE).is_some());
    let removed_capture_command = ["debug", &["capture", "Screen"].join("")].join(".");
    assert!(catalog.resolve(&removed_capture_command).is_none());
    assert!(catalog.resolve(DISPLAY_LIST).is_some());
    let removed_display_probe_command = ["debug", &["probe", "Displays"].join("")].join(".");
    assert!(catalog.resolve(&removed_display_probe_command).is_none());
    assert!(catalog.resolve(DISPLAY_PROJECT_SCREENSHOT_POINT).is_some());
    assert!(catalog.resolve(DISPLAY_IDENTIFY_POINT).is_some());
    assert!(
      catalog
        .resolve(DISPLAY_PROBE_COORDINATE_READINESS)
        .is_some()
    );
    assert!(catalog.resolve(SCREEN_FIND_TEXT).is_some());
    assert!(catalog.resolve(MEDIA_CONTROL_NOW_PLAYING).is_some());
    assert!(catalog.resolve(WINDOW_VERIFY_TEXT).is_some());
    assert!(catalog.resolve("verify.musicNowPlaying").is_none());
    assert!(catalog.resolve("verify.axText").is_none());
    assert!(catalog.resolve(WINDOW_LIST).is_some());
    assert!(catalog.resolve(WINDOW_CAPTURE_AX_TREE).is_some());
    assert!(catalog.resolve(APP_PROBE_PERMISSIONS).is_some());
    assert!(catalog.resolve(INPUT_FOCUS_TEXT).is_some());
    assert!(catalog.resolve(INPUT_PRESS_BUTTON).is_some());
    assert!(catalog.resolve(INPUT_AX_PRESS_BUTTON).is_some());
    assert!(catalog.resolve(INPUT_AX_FOCUS_TEXT).is_some());
    assert!(catalog.resolve(INPUT_AX_CLICK_WINDOW_TEXT).is_some());
    assert!(catalog.resolve(INPUT_SMART_PRESS).is_some());
    assert!(catalog.resolve(INPUT_TYPE_TEXT).is_some());
    assert!(catalog.resolve(INPUT_PASTE_TEXT).is_some());
    assert!(catalog.resolve(INPUT_KEY).is_some());
    assert!(catalog.resolve(INPUT_CLICK_POINT).is_some());
    assert!(catalog.resolve(INPUT_CLICK_WINDOW_POINT).is_some());
    assert!(catalog.resolve(INPUT_TEACH_CLICK).is_some());
    assert!(catalog.resolve(SCREEN_CLICK_TEXT).is_some());
    assert!(catalog.resolve(INPUT_SCROLL_POINT).is_some());
    assert!(catalog.resolve(INPUT_OVERLAY_CLICK_POINT).is_some());
    assert!(catalog.resolve(OVERLAY_SHOW_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_SHOW_DUAL_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_APPLY_CURSOR_BATCH).is_some());
    assert!(catalog.resolve(OVERLAY_SET_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_MOVE_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_MOVE_CURSOR_BY_ID).is_some());
    assert!(catalog.resolve(OVERLAY_FLASH_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_FLASH_CURSOR_BY_ID).is_some());
    assert!(catalog.resolve(OVERLAY_HIDE_CURSOR_ID).is_some());
    assert!(catalog.resolve(OVERLAY_HIDE_CURSOR).is_some());
    assert!(catalog.resolve(OVERLAY_SHUTDOWN).is_some());
    assert!(catalog.resolve("music.search.results").is_some());
    assert!(
      catalog
        .resolve("music.validate.candidate.liveness")
        .is_some()
    );
    assert!(catalog.resolve("music.result.play").is_some());
  }

  #[test]
  fn command_catalog_resolves_window_listing_commands() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve(DISPLAY_LIST).is_some());
    assert!(catalog.resolve(WINDOW_LIST).is_some());
    assert!(catalog.resolve("debug.observeWindows").is_none());
  }

  #[test]
  fn command_catalog_resolves_window_ocr_commands() {
    let catalog = default_command_catalog();
    for command_id in [
      WINDOW_FIND_TEXT,
      WINDOW_WAIT_FOR_TEXT,
      WINDOW_CLICK_TEXT,
      WINDOW_FIND_ROWS,
      WINDOW_WAIT_FOR_ROWS,
      WINDOW_CLICK_ROW,
    ] {
      assert!(
        catalog.resolve(command_id).is_some(),
        "missing {command_id}"
      );
    }
  }

  #[test]
  fn default_catalog_resolves_window_region_scan_primitives() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve(WINDOW_OBSERVE_REGION).is_some());
    assert!(catalog.resolve(WINDOW_SCROLL_REGION).is_some());
    assert!(catalog.resolve("debug.scanWindowRegion").is_none());
  }

  #[test]
  fn command_catalog_renames_window_tree_to_ax_tree() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve(WINDOW_CAPTURE_AX_TREE).is_some());
    let removed_ax_tree_command = ["debug", &["observe", "AxTree"].join("")].join(".");
    assert!(catalog.resolve(&removed_ax_tree_command).is_none());
    assert!(catalog.resolve("debug.observeWindowTree").is_none());
  }

  #[test]
  fn default_catalog_routes_expected_commands_through_macos_desktop() {
    let catalog = default_command_catalog();

    for command_id in [
      DISPLAY_CAPTURE,
      DISPLAY_CAPTURE_REGION,
      DISPLAY_PROBE_COORDINATE_READINESS,
      SCREEN_FIND_TEXT,
      WINDOW_OBSERVE_REGION,
      INPUT_TYPE_TEXT,
      INPUT_OVERLAY_CLICK_POINT,
      WINDOW_VERIFY_TEXT,
      OVERLAY_SHUTDOWN,
    ] {
      let command = catalog
        .resolve(command_id)
        .unwrap_or_else(|| panic!("{command_id} must resolve"));
      assert_eq!(
        command.driver_id, "macos.desktop",
        "command {} should route through macos.desktop",
        command.id
      );
    }
  }

  #[test]
  fn command_catalog_renames_ax_tree_capture_command() {
    let catalog = default_command_catalog();
    let command = catalog
      .resolve(WINDOW_CAPTURE_AX_TREE)
      .expect("window.captureAxTree should exist");

    assert_eq!(command.operation, "capture_ax_tree");
    let removed_ax_tree_command = ["debug", &["observe", "AxTree"].join("")].join(".");
    assert!(catalog.resolve(&removed_ax_tree_command).is_none());
    assert!(catalog.resolve("debug.observeWindowTree").is_none());
  }

  #[test]
  fn default_catalog_does_not_expose_mutation_commands() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve("debug.click").is_none());
    assert!(catalog.resolve("debug.focusApp").is_none());
  }

  #[test]
  fn default_catalog_tags_known_commands_with_expected_namespace() {
    let catalog = default_command_catalog();
    let observe_cases = [
      DISPLAY_CAPTURE,
      WINDOW_CAPTURE,
      SCREEN_FIND_TEXT,
      SCREEN_WAIT_FOR_TEXT,
      WINDOW_OBSERVE_REGION,
      WINDOW_LIST,
      WINDOW_CAPTURE_AX_TREE,
    ];
    for id in observe_cases {
      let command = catalog
        .resolve(id)
        .unwrap_or_else(|| panic!("{id} must resolve"));
      assert_eq!(
        command.namespace,
        CommandNamespace::Observe,
        "{id} should tag as observe, got {:?}",
        command.namespace
      );
    }

    let action_cases = [
      INPUT_CLICK_POINT,
      INPUT_TYPE_TEXT,
      INPUT_SMART_PRESS,
      INPUT_PRESS_BUTTON,
      INPUT_SCROLL_POINT,
      WINDOW_SCROLL_REGION,
      INPUT_OVERLAY_CLICK_POINT,
    ];
    for id in action_cases {
      let command = catalog
        .resolve(id)
        .unwrap_or_else(|| panic!("{id} must resolve"));
      assert_eq!(
        command.namespace,
        CommandNamespace::Action,
        "{id} should tag as action, got {:?}",
        command.namespace
      );
    }

    let verify_cases = [MEDIA_CONTROL_NOW_PLAYING, WINDOW_VERIFY_TEXT];
    for id in verify_cases {
      let command = catalog
        .resolve(id)
        .unwrap_or_else(|| panic!("{id} must resolve"));
      assert_eq!(
        command.namespace,
        CommandNamespace::Verify,
        "{id} should tag as verify, got {:?}",
        command.namespace
      );
    }

    let overlay_cases = [OVERLAY_SHOW_CURSOR, OVERLAY_MOVE_CURSOR, OVERLAY_SHUTDOWN];
    for id in overlay_cases {
      let command = catalog
        .resolve(id)
        .unwrap_or_else(|| panic!("{id} must resolve"));
      assert_eq!(
        command.namespace,
        CommandNamespace::Overlay,
        "{id} should tag as overlay, got {:?}",
        command.namespace
      );
    }

    let domain_cases = [
      "music.search.results",
      "music.result.play",
      "steam.library.list.v0",
    ];
    for id in domain_cases {
      let command = catalog
        .resolve(id)
        .unwrap_or_else(|| panic!("{id} must resolve"));
      assert_eq!(
        command.namespace,
        CommandNamespace::Domain,
        "{id} should tag as domain, got {:?}",
        command.namespace
      );
    }
  }

  #[test]
  fn default_catalog_never_emits_test_namespace() {
    let catalog = default_command_catalog();
    for command in catalog.all() {
      assert_ne!(
        command.namespace,
        CommandNamespace::Test,
        "production catalog must not carry CommandNamespace::Test (got {})",
        command.id
      );
    }
  }
}
