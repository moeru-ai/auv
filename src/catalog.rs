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
      namespace: OBSERVE,
      summary: "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_display",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.captureRegion",
      namespace: OBSERVE,
      summary: "Capture one display-contained region and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_region",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.captureWindow",
      namespace: OBSERVE,
      summary: "Capture one single-display window and emit a coordinate contract. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "capture_window",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.listDisplays",
      namespace: OBSERVE,
      summary: "List connected displays using the normalized AUV coordinate contract.",
      driver_id: "macos.desktop",
      operation: "list_displays",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.projectScreenshotPoint",
      namespace: OBSERVE,
      summary: "Project main-display screenshot pixels back into AUV global logical coordinates.",
      driver_id: "macos.desktop",
      operation: "project_screenshot_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.identifyPoint",
      namespace: OBSERVE,
      summary: "Resolve a logical desktop point against the current macOS display layout.",
      driver_id: "macos.desktop",
      operation: "identify_point",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.probeCoordinateReadiness",
      namespace: OBSERVE,
      summary: "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
      driver_id: "macos.desktop",
      operation: "probe_coordinate_readiness",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findScreenText",
      namespace: OBSERVE,
      summary: "Capture a screenshot and locate OCR text anchors in screenshot pixel space. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.waitForScreenText",
      namespace: OBSERVE,
      summary: "Poll live-desktop OCR until a target text anchor appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_text",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.findScreenRows",
      namespace: OBSERVE,
      summary: "Detect visible OCR row bands inside a constrained screen region without depending on one exact anchor string. If activate_target_before_capture is true, the target app is foregrounded first.",
      driver_id: "macos.desktop",
      operation: "find_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.waitForScreenRows",
      namespace: OBSERVE,
      summary: "Poll live-desktop OCR row detection until at least a target number of visible rows appears or the timeout expires. If activate_target_before_capture is true, the target app is foregrounded before each capture attempt.",
      driver_id: "macos.desktop",
      operation: "wait_for_screen_rows",
      disturbance_classes: NONE_OR_FOREGROUND,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.findImageText",
      namespace: OBSERVE,
      summary: "Locate OCR text anchors inside an existing image artifact without touching the live desktop.",
      driver_id: "macos.desktop",
      operation: "find_image_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findWindowText",
      namespace: OBSERVE,
      summary: "Capture a resolved window and locate OCR text anchors in window pixel space.",
      driver_id: "macos.desktop",
      operation: "find_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.waitForWindowText",
      namespace: OBSERVE,
      summary: "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
      driver_id: "macos.desktop",
      operation: "wait_for_window_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findWindowRows",
      namespace: OBSERVE,
      summary: "Detect visible OCR row bands inside a resolved window.",
      driver_id: "macos.desktop",
      operation: "find_window_rows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.waitForWindowRows",
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
      id: "debug.observeWindowRegion",
      namespace: OBSERVE,
      summary: "Observe OCR row-like content inside a resolved macOS window region without scrolling.",
      driver_id: "macos.desktop",
      operation: "observe_window_region",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.findIconMatch",
      namespace: OBSERVE,
      summary: "Match a template image against a resolved macOS window screenshot using NCC and emit a RecognitionResult artifact.",
      driver_id: "macos.desktop",
      operation: "find_icon_match",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.scrollWindowRegion",
      namespace: ACTION,
      summary: "Scroll at the center of a resolved macOS window region and record scroll evidence.",
      driver_id: "macos.desktop",
      operation: "scroll_window_region",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "verify.musicNowPlaying",
      namespace: VERIFY,
      summary: "Verify the current now-playing title from the observed AX tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_now_playing_title",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "verify.axText",
      namespace: VERIFY,
      summary: "Verify that a text-bearing AX node exists in the observed tree without relying on screenshot OCR.",
      driver_id: "macos.desktop",
      operation: "verify_ax_text",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.listWindows",
      namespace: OBSERVE,
      summary: "List visible macOS window candidates using the normalized AUV window selector model.",
      driver_id: "macos.desktop",
      operation: "list_windows",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.captureAxTree",
      namespace: OBSERVE,
      summary: "Capture an AX tree snapshot for a target macOS app window.",
      driver_id: "macos.desktop",
      operation: "capture_ax_tree",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.probePermissions",
      namespace: OBSERVE,
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.desktop",
      operation: "probe_permissions",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.activateApp",
      namespace: ACTION,
      summary: "Bring a target macOS app to the foreground before a foreground-dependent step.",
      driver_id: "macos.desktop",
      operation: "activate_app",
      disturbance_classes: FOREGROUND_ONLY,
      max_disturbance: DisturbanceClass::ForegroundApp,
    },
    CommandSpec {
      id: "debug.focusTextInput",
      namespace: ACTION,
      summary: "Focus a target macOS text input through AX, either by legacy --query text or by a promoted --candidate JSON payload carrying the typed search-entry contract candidate.",
      driver_id: "macos.desktop",
      operation: "focus_text_input",
      disturbance_classes: FOCUS_POINTER_ENTRY,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.pressButton",
      namespace: ACTION,
      summary: "Press a known macOS button-like control by query through AX.",
      driver_id: "macos.desktop",
      operation: "press_button",
      disturbance_classes: PRESS_BUTTON_DISTURBANCE,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.axPressButton",
      namespace: ACTION,
      summary: "Press a control by query via AXUIElementPerformAction; does not warp the real cursor (cursorDisturbance=none). Pass --overlay true to draw a visual AUV cursor over the target during the press for the dual-cursor effect. Falls back with an error when the AX target has no matching action; use debug.pressButton for non-AX-pressable targets.",
      driver_id: "macos.desktop",
      operation: "ax_press_button",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.axFocusTextInput",
      namespace: ACTION,
      summary: "Focus a text input by query or promoted --candidate JSON via AXUIElementSetAttributeValue(kAXFocusedAttribute); does not warp the real cursor (cursorDisturbance=none, focusMechanism=ax-attribute). Pass --overlay true for the dual-cursor visual (auv replay cursor animates to the target while the real cursor stays put). Errors when the target does not accept programmatic focus; use debug.focusTextInput if pointer warp is acceptable.",
      driver_id: "macos.desktop",
      operation: "ax_focus_text_input",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.axClickWindowText",
      namespace: ACTION,
      summary: "Find visible text in a window via Vision OCR, resolve the AX node at that point, then press it via AXUIElementPerformAction (cursorDisturbance=none). Pass --overlay true for the dual-cursor visual. Errors with a hint to debug.clickWindowText when the OCR anchor maps to a canvas-rendered or non-AX-pressable region.",
      driver_id: "macos.desktop",
      operation: "ax_click_window_text",
      disturbance_classes: CAPTURE_AX_TREE_DISTURBANCE,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.smartPress",
      namespace: ACTION,
      summary: "ActionResolver v0 debug press: try OCR-to-AX press first; if it fails and --allow_pointer_fallback is not false, fall back to pointer click. Records actionResolver.* signals plus the selected method, fallback reason, and disturbance metadata.",
      driver_id: "macos.desktop",
      operation: "smart_press",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.typeText",
      namespace: ACTION,
      summary: "Type text into the active macOS control through System Events.",
      driver_id: "macos.desktop",
      operation: "type_text",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.pasteTextPreserveClipboard",
      namespace: ACTION,
      summary: "Paste text into the active macOS control through the clipboard, then restore the prior clipboard snapshot.",
      driver_id: "macos.desktop",
      operation: "paste_text_preserve_clipboard",
      disturbance_classes: FOREGROUND_KEYBOARD_CLIPBOARD,
      max_disturbance: DisturbanceClass::Clipboard,
    },
    CommandSpec {
      id: "debug.pressKey",
      namespace: ACTION,
      summary: "Press a keyboard key or shortcut in the active macOS app through System Events.",
      driver_id: "macos.desktop",
      operation: "press_key",
      disturbance_classes: FOREGROUND_KEYBOARD,
      max_disturbance: DisturbanceClass::Keyboard,
    },
    CommandSpec {
      id: "debug.clickPoint",
      namespace: ACTION,
      summary: "Click a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowPoint",
      namespace: ACTION,
      summary: "Click a point relative to a target macOS window and record the resolved global point, either from legacy --relative_x/--relative_y inputs or from a promoted --candidate JSON payload carrying the typed window-action contract candidate.",
      driver_id: "macos.desktop",
      operation: "click_window_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.teachClick",
      namespace: ACTION,
      summary: "Capture a target window before and after a human-taught click, recording global and window-local click coordinates for automation debugging.",
      driver_id: "macos.desktop",
      operation: "teach_click",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickScreenText",
      namespace: ACTION,
      summary: "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickScreenRow",
      namespace: ACTION,
      summary: "Detect visible OCR row bands inside a constrained screen region and click a chosen row-derived point. If activate_target_before_capture is true, the target app is foregrounded before capture.",
      driver_id: "macos.desktop",
      operation: "click_screen_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowText",
      namespace: ACTION,
      summary: "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_text",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.clickWindowRow",
      namespace: ACTION,
      summary: "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
      driver_id: "macos.desktop",
      operation: "click_window_row",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.scrollPoint",
      namespace: ACTION,
      summary: "Scroll at a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.desktop",
      operation: "scroll_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.overlayClickPoint",
      namespace: ACTION,
      summary: "Move the visual AUV cursor to a target point, click, flash the click-state cursor, then hide overlay. Legacy visualization command path; the real cursor visibly warps to the click target and back (cursorDisturbance=warp-visible).",
      driver_id: "macos.desktop",
      operation: "overlay_click_point",
      disturbance_classes: POINTER_WITH_FOREGROUND,
      max_disturbance: DisturbanceClass::Pointer,
    },
    CommandSpec {
      id: "debug.overlayShowCursor",
      namespace: OVERLAY,
      summary: "Show a visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_show_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayShowDualCursor",
      namespace: OVERLAY,
      summary: "Show visual-only dual cursor overlays: AUV at a target point and You at the current hardware cursor.",
      driver_id: "macos.desktop",
      operation: "overlay_show_dual_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayApplyCursorBatch",
      namespace: OVERLAY,
      summary: "Apply a JSON batch of visual-only overlay cursor operations in one process.",
      driver_id: "macos.desktop",
      operation: "overlay_apply_cursor_batch",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlaySetCursor",
      namespace: OVERLAY,
      summary: "Show or update one visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_set_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayMoveCursor",
      namespace: OVERLAY,
      summary: "Animate the visual-only AUV cursor from the current hardware cursor toward a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayMoveCursorById",
      namespace: OVERLAY,
      summary: "Animate one visual-only overlay cursor by cursor_id, reusing its previous position when available.",
      driver_id: "macos.desktop",
      operation: "overlay_move_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayFlashCursor",
      namespace: OVERLAY,
      summary: "Flash the AUV click-state cursor sprite at a target point.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayFlashCursorById",
      namespace: OVERLAY,
      summary: "Flash the AUV click-state cursor sprite for one overlay cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_flash_cursor_by_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayHideCursorId",
      namespace: OVERLAY,
      summary: "Hide one visual-only overlay cursor by cursor_id.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor_id",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayHideCursor",
      namespace: OVERLAY,
      summary: "Hide the visual-only AUV cursor label overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_hide_cursor",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.overlayShutdown",
      namespace: OVERLAY,
      summary: "Shut down the visual-only AUV cursor overlay inside the current process.",
      driver_id: "macos.desktop",
      operation: "overlay_shutdown",
      disturbance_classes: NONE,
      max_disturbance: DisturbanceClass::None,
    },
    CommandSpec {
      id: "debug.fixtureObserve",
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
    assert!(catalog.resolve("verify.musicNowPlaying").is_some());
    assert!(catalog.resolve("verify.axText").is_some());
    // Old debug.* names must not resolve — they were renamed into verify.*.
    assert!(catalog.resolve("debug.verifyNowPlayingTitle").is_none());
    assert!(catalog.resolve("debug.verifyAxText").is_none());
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
    assert!(catalog.resolve("debug.teachClick").is_some());
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

  #[test]
  fn default_catalog_tags_known_commands_with_expected_namespace() {
    let catalog = default_command_catalog();
    let observe_cases = [
      "debug.captureDisplay",
      "debug.captureWindow",
      "debug.findScreenText",
      "debug.waitForScreenText",
      "debug.observeWindowRegion",
      "debug.listWindows",
      "debug.captureAxTree",
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
      "debug.clickPoint",
      "debug.typeText",
      "debug.smartPress",
      "debug.pressButton",
      "debug.scrollPoint",
      "debug.scrollWindowRegion",
      "debug.overlayClickPoint",
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

    let verify_cases = ["verify.musicNowPlaying", "verify.axText"];
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

    let overlay_cases = [
      "debug.overlayShowCursor",
      "debug.overlayMoveCursor",
      "debug.overlayShutdown",
    ];
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

    let domain_cases = ["music.search.results", "music.result.play"];
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
