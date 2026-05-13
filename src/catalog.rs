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
      id: "debug.probeCoordinateReadiness",
      summary: "Capture a screenshot and compare its pixels against the observed macOS coordinate space.",
      driver_id: "macos.observe",
      operation: "probe_coordinate_readiness",
    },
    CommandSpec {
      id: "debug.probePermissions",
      summary: "Probe macOS screen recording, accessibility, and automation permissions.",
      driver_id: "macos.observe",
      operation: "probe_permissions",
    },
    CommandSpec {
      id: "debug.fixtureObserve",
      summary: "Emit a deterministic observation result without touching the host UI.",
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
    assert!(catalog.resolve("debug.probeCoordinateReadiness").is_some());
    assert!(catalog.resolve("debug.probePermissions").is_some());
  }

  #[test]
  fn default_catalog_does_not_expose_mutation_commands() {
    let catalog = default_command_catalog();
    assert!(catalog.resolve("debug.click").is_none());
    assert!(catalog.resolve("debug.focusApp").is_none());
  }
}
