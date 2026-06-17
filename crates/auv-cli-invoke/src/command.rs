use crate::arg::ArgSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvokeNamespace {
  Display,
  Screen,
  Window,
  Input,
  App,
  Overlay,
  MediaControl,
  Steam,
  Fixture,
}

impl InvokeNamespace {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Display => "display",
      Self::Screen => "screen",
      Self::Window => "window",
      Self::Input => "input",
      Self::App => "app",
      Self::Overlay => "overlay",
      Self::MediaControl => "mediaControl",
      Self::Steam => "steam",
      Self::Fixture => "fixture",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeCommand {
  pub id: &'static str,
  pub namespace: InvokeNamespace,
  pub summary: &'static str,
  pub args: &'static [ArgSpec],
}

#[derive(Clone, Debug)]
pub struct CommandGroup {
  pub name: &'static str,
  pub heading: &'static str,
  pub children: Vec<CommandNode>,
}

impl CommandGroup {
  pub fn new(name: &'static str, heading: &'static str) -> Self {
    Self {
      name,
      heading,
      children: Vec::new(),
    }
  }

  pub fn command(mut self, command: InvokeCommand) -> Self {
    self.children.push(CommandNode::Command(command));
    self
  }

  pub fn group(mut self, group: CommandGroup) -> Self {
    self.children.push(CommandNode::Group(group));
    self
  }
}

#[derive(Clone, Debug)]
pub enum CommandNode {
  Command(InvokeCommand),
  Group(CommandGroup),
}

// The registry files use this as a compact declaration DSL: every field maps
// directly to one part of the public invoke command metadata.
#[doc(hidden)]
pub fn spec(
  id: &'static str,
  namespace: InvokeNamespace,
  summary: &'static str,
  args: &'static [ArgSpec],
) -> InvokeCommand {
  InvokeCommand {
    id,
    namespace,
    summary,
    args,
  }
}
