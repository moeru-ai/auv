use crate::model::CommandSpec;

pub struct CommandCatalog {
  commands: Vec<CommandSpec>,
}

impl CommandCatalog {
  pub fn new(commands: Vec<CommandSpec>) -> Self {
    Self { commands }
  }

  pub fn resolve(&self, command_id: &str) -> Option<&CommandSpec> {
    self.commands.iter().find(|command| command.id == command_id)
  }

  pub fn all(&self) -> &[CommandSpec] {
    &self.commands
  }
}

pub fn default_command_catalog() -> CommandCatalog {
  CommandCatalog::new(vec![
    CommandSpec {
      id: "debug.captureScreen",
      summary: "Capture one desktop screenshot through the shared runtime path.",
      driver_id: "macos.desktop",
      operation: "capture_screen",
    },
    CommandSpec {
      id: "debug.observeWindows",
      summary: "Observe visible macOS windows and capture a text report artifact.",
      driver_id: "macos.desktop",
      operation: "observe_windows",
    },
    CommandSpec {
      id: "debug.probePermissions",
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.desktop",
      operation: "probe_permissions",
    },
    CommandSpec {
      id: "debug.openApp",
      summary: "Open an application on the local macOS host.",
      driver_id: "macos.desktop",
      operation: "open_app",
    },
    CommandSpec {
      id: "debug.focusApp",
      summary: "Open and activate an application on the local macOS host.",
      driver_id: "macos.desktop",
      operation: "focus_app",
    },
    CommandSpec {
      id: "debug.click",
      summary: "Move the pointer and click on the local macOS desktop.",
      driver_id: "macos.desktop",
      operation: "click",
    },
    CommandSpec {
      id: "debug.typeText",
      summary: "Type text through Quartz keyboard events on the local macOS host.",
      driver_id: "macos.desktop",
      operation: "type_text",
    },
    CommandSpec {
      id: "debug.pressKeys",
      summary: "Press a macOS key combination through Quartz keyboard events.",
      driver_id: "macos.desktop",
      operation: "press_keys",
    },
    CommandSpec {
      id: "debug.scroll",
      summary: "Scroll on the local macOS desktop with optional pointer positioning.",
      driver_id: "macos.desktop",
      operation: "scroll",
    },
    CommandSpec {
      id: "debug.wait",
      summary: "Sleep inside the shared runtime path for a bounded duration.",
      driver_id: "macos.desktop",
      operation: "wait",
    },
    CommandSpec {
      id: "debug.fixtureObserve",
      summary: "Emit a deterministic observation result without touching the real UI.",
      driver_id: "fixture.observe",
      operation: "observe_fixture_scene",
    },
  ])
}
