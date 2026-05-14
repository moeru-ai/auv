use crate::model::CommandSpec;

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
      id: "debug.captureScreen",
      summary: "Capture one desktop screenshot through the shared runtime path.",
      driver_id: "macos.observe",
      operation: "capture_screen",
    },
    CommandSpec {
      id: "debug.probeDisplays",
      summary: "Enumerate connected macOS displays and capture a coordinate-space report.",
      driver_id: "macos.observe",
      operation: "probe_displays",
    },
    CommandSpec {
      id: "debug.projectScreenshotPoint",
      summary: "Project main-display screenshot pixels back into AUV global logical coordinates.",
      driver_id: "macos.observe",
      operation: "project_screenshot_point",
    },
    CommandSpec {
      id: "debug.identifyPoint",
      summary: "Resolve a logical desktop point against the current macOS display layout.",
      driver_id: "macos.observe",
      operation: "identify_point",
    },
    CommandSpec {
      id: "debug.probeCoordinateReadiness",
      summary: "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
      driver_id: "macos.observe",
      operation: "probe_coordinate_readiness",
    },
    CommandSpec {
      id: "debug.findScreenText",
      summary: "Capture a screenshot and locate OCR text anchors in screenshot pixel space.",
      driver_id: "macos.observe",
      operation: "find_screen_text",
    },
    CommandSpec {
      id: "debug.observeWindows",
      summary: "Observe visible macOS windows and capture a text report artifact.",
      driver_id: "macos.observe",
      operation: "observe_windows",
    },
    CommandSpec {
      id: "debug.observeWindowTree",
      summary: "Capture an AX tree snapshot for a target macOS app window.",
      driver_id: "macos.observe",
      operation: "observe_window_tree",
    },
    CommandSpec {
      id: "debug.probePermissions",
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.observe",
      operation: "probe_permissions",
    },
    CommandSpec {
      id: "debug.focusTextInput",
      summary: "Focus a target macOS text input by query through AX.",
      driver_id: "macos.observe",
      operation: "focus_text_input",
    },
    CommandSpec {
      id: "debug.pressButton",
      summary: "Press a known macOS button-like control by query through AX.",
      driver_id: "macos.observe",
      operation: "press_button",
    },
    CommandSpec {
      id: "debug.typeText",
      summary: "Type text into the active macOS control through System Events.",
      driver_id: "macos.observe",
      operation: "type_text",
    },
    CommandSpec {
      id: "debug.pressKey",
      summary: "Press a keyboard key or shortcut in the active macOS app through System Events.",
      driver_id: "macos.observe",
      operation: "press_key",
    },
    CommandSpec {
      id: "debug.clickPoint",
      summary: "Click a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.observe",
      operation: "click_point",
    },
    CommandSpec {
      id: "debug.clickWindowPoint",
      summary: "Click a point relative to a target macOS window and record the resolved global point.",
      driver_id: "macos.observe",
      operation: "click_window_point",
    },
    CommandSpec {
      id: "debug.clickScreenText",
      summary: "Capture a screenshot, resolve an OCR text anchor, and click its projected logical point.",
      driver_id: "macos.observe",
      operation: "click_screen_text",
    },
    CommandSpec {
      id: "debug.scrollPoint",
      summary: "Scroll at a macOS global logical point through Quartz and record its display contract.",
      driver_id: "macos.observe",
      operation: "scroll_point",
    },
    CommandSpec {
      id: "debug.fixtureObserve",
      summary: "Emit a deterministic observation result without touching the real UI.",
      driver_id: "fixture.observe",
      operation: "observe_fixture_scene",
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
      },
      CommandSpec {
        id: "cmd2",
        summary: "sum2",
        driver_id: "d2",
        operation: "op2",
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
    assert!(catalog.resolve("debug.captureScreen").is_some());
    assert!(catalog.resolve("debug.probeDisplays").is_some());
    assert!(catalog.resolve("debug.projectScreenshotPoint").is_some());
    assert!(catalog.resolve("debug.identifyPoint").is_some());
    assert!(catalog.resolve("debug.probeCoordinateReadiness").is_some());
    assert!(catalog.resolve("debug.findScreenText").is_some());
    assert!(catalog.resolve("debug.observeWindows").is_some());
    assert!(catalog.resolve("debug.observeWindowTree").is_some());
    assert!(catalog.resolve("debug.probePermissions").is_some());
    assert!(catalog.resolve("debug.focusTextInput").is_some());
    assert!(catalog.resolve("debug.pressButton").is_some());
    assert!(catalog.resolve("debug.typeText").is_some());
    assert!(catalog.resolve("debug.pressKey").is_some());
    assert!(catalog.resolve("debug.clickPoint").is_some());
    assert!(catalog.resolve("debug.clickWindowPoint").is_some());
    assert!(catalog.resolve("debug.clickScreenText").is_some());
    assert!(catalog.resolve("debug.scrollPoint").is_some());
  }

  #[test]
  fn default_catalog_does_not_expose_mutation_commands() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve("debug.click").is_none());
    assert!(catalog.resolve("debug.focusApp").is_none());
  }
}
