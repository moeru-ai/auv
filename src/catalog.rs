use crate::model::{CommandSpec, DisturbanceClass};

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

pub fn default_command_catalog() -> CommandCatalog {
  let commands = vec![
    CommandSpec {
      id: "debug.captureDisplay",
      summary: "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_display",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.captureRegion",
      summary: "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_region",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.captureWindow",
      summary: "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_window",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.listDisplays",
      summary: "List connected displays using the normalized AUV coordinate contract.",
      driver_id: "macos.desktop",
      operation: "list_displays",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.projectScreenshotPoint",
      summary: "Project main-display screenshot pixels back into AUV global logical coordinates.",
      driver_id: "macos.desktop",
      operation: "project_screenshot_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.identifyPoint",
      summary: "Resolve a logical desktop point against the current macOS display layout.",
      driver_id: "macos.desktop",
      operation: "identify_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.probeCoordinateReadiness",
      summary: "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
      driver_id: "macos.desktop",
      operation: "probe_coordinate_readiness",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findScreenText",
      summary: "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.waitForScreenText",
      summary: "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.findScreenRows",
      summary: "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.waitForScreenRows",
      summary: "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.findImageText",
      summary: "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
      driver_id: "macos.desktop",
      operation: "find_image_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findWindowText",
      summary: "Capture a resolved window and locate OCR text anchors in window pixel space.",
      driver_id: "macos.desktop",
      operation: "find_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.waitForWindowText",
      summary: "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
      driver_id: "macos.desktop",
      operation: "wait_for_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findWindowRows",
      summary: "Detect visible OCR row bands inside a resolved window.",
      driver_id: "macos.desktop",
      operation: "find_window_rows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.waitForWindowRows",
      summary: "Poll resolved-window row detection until enough rows appear or the timeout expires.",
      driver_id: "macos.desktop",
      operation: "wait_for_window_rows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    // REVIEW: Public names are explicit window-scoped variants for the first
    // implementation. Revisit before introducing screen or generic region scans.
    CommandSpec {
      id: "debug.observeWindowRegion",
      summary: "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
      driver_id: "macos.desktop",
      operation: "observe_window_region",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.scrollWindowRegion",
      summary: "Scroll at the center of a resolved macOS window region and record scroll evidence.",
      driver_id: "macos.desktop",
      operation: "scroll_window_region",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.verifyNowPlayingTitle",
      summary: "Verify the current now-playing title from the observed AX tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_now_playing_title",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.verifyAxText",
      summary: "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_ax_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.listWindows",
      summary: "List visible macOS window candidates using the normalized AUV window selector model.",
      driver_id: "macos.desktop",
      operation: "list_windows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.captureAxTree",
      summary: "Capture an AX tree snapshot for a target macOS app window.",
      driver_id: "macos.desktop",
      operation: "capture_ax_tree",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.probePermissions",
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.desktop",
      operation: "probe_permissions",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.activateApp",
      summary: "Bring a target macOS app to the foreground before a foreground-dependent step.",
      driver_id: "macos.desktop",
      operation: "activate_app",
      disturbance_classes: FOREGROUND_ONLY,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.focusTextInput",
      summary: "Focus a target macOS text input by query through AX.",
      driver_id: "macos.desktop",
      operation: "focus_text_input",
      disturbance_classes: FOCUS_POINTER_ENTRY,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.pressButton",
      summary: "Press a known macOS button-like control by query through AX.",
      driver_id: "macos.desktop",
      operation: "press_button",
      disturbance_classes: PRESS_BUTTON_DISTURBANCE,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.axPressButton",
      summary: "Press a control by query via AXUIElementPerformAction; does not warp the real cursor (cursorDisturbance=none). Pass --overlay true to draw a visual AUV cursor over the target during the press for the dual-cursor effect. Falls back with an error when the AX target has no matching action; use debug.pressButton for non-AX-pressable targets.",
      driver_id: "macos.desktop",
      operation: "ax_press_button",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.axFocusTextInput",
      summary: "Focus a text input by query via AXUIElementSetAttributeValue(kAXFocusedAttribute); does not warp the real cursor (cursorDisturbance=none, focusMechanism=ax-attribute). Pass --overlay true for the dual-cursor visual (auv replay cursor animates to the target while the real cursor stays put). Errors when the target does not accept programmatic focus; use debug.focusTextInput if pointer warp is acceptable.",
      driver_id: "macos.desktop",
      operation: "ax_focus_text_input",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.axClickWindowText",
      summary: "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction (cursorDisturbance=none). Pass --overlay true for the dual-cursor visual. Errors with a hint to debug.clickWindowText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
      driver_id: "macos.desktop",
      operation: "ax_click_window_text",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.smartPress",
      summary: "Try OCR-to-AX press first; if it fails and --allow_pointer_fallback is not false, fall back to pointer click. Overlay defaults on for this debug command so the target is visible before either strategy acts.",
      driver_id: "macos.desktop",
      operation: "smart_press",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.typeText",
      summary: "Type text into the active macOS control through System Events.",
      driver_id: "macos.desktop",
      operation: "type_text",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.pasteTextPreserveClipboard",
      summary: "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
      driver_id: "macos.desktop",
      operation: "paste_text_preserve_clipboard",
      disturbance_classes: FOREGROUND_KEYBOARD_CLIPBOARD,
      max_disturbance: DisturbanceClass::Clipboard,
    },
    CommandSpec {
      id: "debug.pressKey",
      summary: "Press a keyboard key or shortcut in the active macOS app through System Events.",
      driver_id: "macos.desktop",
      operation: "press_key",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.clickPoint",
      summary: "Click a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowPoint",
      summary: "Click a point relative to a target macOS window and record the resolved global point.",
      driver_id: "macos.desktop",
      operation: "click_window_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickScreenText",
      summary: "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickScreenRow",
      summary: "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowText",
      summary: "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowRow",
      summary: "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.scrollPoint",
      summary: "Scroll at a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "scroll_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.overlayClickPoint",
      summary: "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. Experimental debug-only path; the real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
      driver_id: "macos.desktop",
      operation: "overlay_click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.overlayShowCursor",
      summary: "Show an experimental visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_show_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayShowDualCursor",
      summary: "Show experimental visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
      driver_id: "macos.desktop",
      operation: "overlay_show_dual_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayApplyCursorBatch",
      summary: "Apply a JSON batch of experimental visual-only overlay cursor operations in one process.",
      driver_id: "macos.desktop",
      operation: "overlay_apply_cursor_batch",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlaySetCursor",
      summary: "Show or update one experimental visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_set_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayMoveCursor",
      summary: "Animate the experimental visual-only AUV cursor from the current hardware cursor toward a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayMoveCursorById",
      summary: "Animate one experimental visual-only overlay cursor by cursor_id, reusing its previous position when available.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayFlashCursor",
      summary: "Flash the experimental AUV click-state cursor sprite at a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayFlashCursorById",
      summary: "Flash the experimental AUV click-state cursor sprite for one overlay cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayHideCursorId",
      summary: "Hide one experimental visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayHideCursor",
      summary: "Hide the experimental visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayShutdown",
      summary: "Shut down the experimental visual-only AUV cursor overlay daemon inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_shutdown",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.fixtureObserve",
      summary: "Emit a deterministic observation result without touching the real UI.",
      driver_id: "fixture.observe",
      operation: "observe_fixture_scene",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.validate.candidate.liveness",
      summary: "Resolve a candidate (source_run_id + source_artifact_id + candidate_local_id) from a stored OperationResult artifact and verify its liveness preconditions (window_ref + anchor_recheck).",
      driver_id: "macos.desktop",
      operation: "music_validate_candidate_liveness",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.search.results",
      summary: "Detect visible search-result rows in a resolved window and produce a typed OperationResult candidate-set artifact.",
      driver_id: "macos.desktop",
      operation: "music_search_results",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "music.result.play",
      summary: "Consume a music.search.results candidate (source_run_id + source_artifact_id + candidate_local_id), re-check liveness, activate the resolved row, press play, and emit a typed VerificationResult.",
      driver_id: "macos.desktop",
      operation: "music_result_play",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
  ];

  CommandCatalog::new(commands)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn command_catalog_resolves_existing_command() {
    let catalog = CommandCatalog::new(vec![CommandSpec {
      id: "test.cmd",
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
        summary: "sum1",
        driver_id: "d1",
        operation: "op1",
        disturbance_classes: NONE,
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "cmd2",
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
    assert!(catalog.resolve("debug.captureDisplay").is_some());
    assert!(catalog.resolve("debug.captureWindow").is_some());
    assert!(catalog.resolve("debug.captureRegion").is_some());
    let removed_capture_command = ["debug", &["capture", "Screen"].join("")].join(".");
    assert!(catalog.resolve(&removed_capture_command).is_none());
    assert!(catalog.resolve("debug.listDisplays").is_some());
    let removed_display_probe_command = ["debug", &["probe", "Displays"].join("")].join(".");
    assert!(catalog.resolve(&removed_display_probe_command).is_none());
    assert!(catalog.resolve("debug.projectScreenshotPoint").is_some());
    assert!(catalog.resolve("debug.identifyPoint").is_some());
    assert!(catalog.resolve("debug.probeCoordinateReadiness").is_some());
    assert!(catalog.resolve("debug.findScreenText").is_some());
    assert!(catalog.resolve("debug.verifyNowPlayingTitle").is_some());
    assert!(catalog.resolve("debug.verifyAxText").is_some());
    assert!(catalog.resolve("debug.listWindows").is_some());
    assert!(catalog.resolve("debug.captureAxTree").is_some());
    assert!(catalog.resolve("debug.probePermissions").is_some());
    assert!(catalog.resolve("debug.focusTextInput").is_some());
    assert!(catalog.resolve("debug.pressButton").is_some());
    assert!(catalog.resolve("debug.axPressButton").is_some());
    assert!(catalog.resolve("debug.axFocusTextInput").is_some());
    assert!(catalog.resolve("debug.axClickWindowText").is_some());
    assert!(catalog.resolve("debug.smartPress").is_some());
    assert!(catalog.resolve("debug.typeText").is_some());
    assert!(
      catalog
        .resolve("debug.pasteTextPreserveClipboard")
        .is_some()
    );
    assert!(catalog.resolve("debug.pressKey").is_some());
    assert!(catalog.resolve("debug.clickPoint").is_some());
    assert!(catalog.resolve("debug.clickWindowPoint").is_some());
    assert!(catalog.resolve("debug.clickScreenText").is_some());
    assert!(catalog.resolve("debug.scrollPoint").is_some());
    assert!(catalog.resolve("debug.overlayClickPoint").is_some());
    assert!(catalog.resolve("debug.overlayShowCursor").is_some());
    assert!(catalog.resolve("debug.overlayShowDualCursor").is_some());
    assert!(catalog.resolve("debug.overlayApplyCursorBatch").is_some());
    assert!(catalog.resolve("debug.overlaySetCursor").is_some());
    assert!(catalog.resolve("debug.overlayMoveCursor").is_some());
    assert!(catalog.resolve("debug.overlayMoveCursorById").is_some());
    assert!(catalog.resolve("debug.overlayFlashCursor").is_some());
    assert!(catalog.resolve("debug.overlayFlashCursorById").is_some());
    assert!(catalog.resolve("debug.overlayHideCursorId").is_some());
    assert!(catalog.resolve("debug.overlayHideCursor").is_some());
    assert!(catalog.resolve("debug.overlayShutdown").is_some());
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
    assert!(catalog.resolve("debug.listDisplays").is_some());
    assert!(catalog.resolve("debug.listWindows").is_some());
    assert!(catalog.resolve("debug.observeWindows").is_none());
  }

  #[test]
  fn command_catalog_resolves_window_ocr_commands() {
    let catalog = default_command_catalog();
    for command_id in [
      "debug.findWindowText",
      "debug.waitForWindowText",
      "debug.clickWindowText",
      "debug.findWindowRows",
      "debug.waitForWindowRows",
      "debug.clickWindowRow",
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
    assert!(catalog.resolve("debug.observeWindowRegion").is_some());
    assert!(catalog.resolve("debug.scrollWindowRegion").is_some());
    assert!(catalog.resolve("debug.scanWindowRegion").is_none());
  }

  #[test]
  fn command_catalog_renames_window_tree_to_ax_tree() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve("debug.captureAxTree").is_some());
    let removed_ax_tree_command = ["debug", &["observe", "AxTree"].join("")].join(".");
    assert!(catalog.resolve(&removed_ax_tree_command).is_none());
    assert!(catalog.resolve("debug.observeWindowTree").is_none());
  }

  #[test]
  fn default_catalog_uses_macos_desktop_driver_id() {
    let catalog = default_command_catalog();

    for command in catalog
      .all()
      .iter()
      .filter(|command| command.id.starts_with("debug.") && command.driver_id != "fixture.observe")
    {
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
      .resolve("debug.captureAxTree")
      .expect("debug.captureAxTree should exist");

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
}
