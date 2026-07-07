use std::collections::HashSet;

use crate::{CommandGroup, CommandNode, InvokeCommand, commands};

pub struct InvokeRegistry {
  groups: Vec<CommandGroup>,
  commands: Vec<InvokeCommand>,
}

impl InvokeRegistry {
  pub fn from_groups(groups: Vec<CommandGroup>) -> Self {
    let mut commands = Vec::new();
    for group in &groups {
      flatten_group(group, &mut commands);
    }
    assert_unique_command_ids(&commands);
    Self { groups, commands }
  }

  pub fn resolve(&self, command_id: &str) -> Option<&InvokeCommand> {
    self.commands.iter().find(|command| command.id == command_id)
  }

  pub fn all(&self) -> &[InvokeCommand] {
    &self.commands
  }

  pub fn groups(&self) -> &[CommandGroup] {
    &self.groups
  }
}

fn assert_unique_command_ids(commands: &[InvokeCommand]) {
  let mut ids = HashSet::new();
  for command in commands {
    assert!(ids.insert(command.id), "duplicate invoke command id registered: {}", command.id);
  }
}

pub fn default_registry() -> InvokeRegistry {
  InvokeRegistry::from_groups(vec![
    commands::display::group(),
    commands::screen::group(),
    commands::window::group(),
    commands::input::group(),
    commands::app::group(),
    commands::overlay::group(),
    commands::media_control::group(),
    commands::fixture::group(),
    commands::scan::group(),
  ])
}

fn flatten_group(group: &CommandGroup, commands: &mut Vec<InvokeCommand>) {
  for child in &group.children {
    match child {
      CommandNode::Command(command) => commands.push(command.clone()),
      CommandNode::Group(group) => flatten_group(group, commands),
    }
  }
}
