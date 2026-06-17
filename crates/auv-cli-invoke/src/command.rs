use std::collections::BTreeMap;

use crate::arg::ArgSpec;

type InvokeCommandHandler = fn(InvokeCommandInput<'_>) -> InvokeCommandResult;

#[derive(Clone, Copy, Debug)]
pub struct InvokeCommandInput<'a> {
  pub command_id: &'a str,
  pub target_application_id: Option<&'a str>,
  pub inputs: &'a BTreeMap<String, String>,
  pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeCommandOutput {
  pub summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
}

impl InvokeCommandOutput {
  pub fn new(summary: impl Into<String>) -> Self {
    Self {
      summary: summary.into(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
    }
  }
}

pub type InvokeCommandResult = Result<InvokeCommandOutput, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvokeNamespace {
  Display,
  Screen,
  Window,
  Input,
  App,
  Overlay,
  MediaControl,
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
  handler: InvokeCommandHandler,
}

impl InvokeCommand {
  pub fn invoke(&self, input: InvokeCommandInput<'_>) -> InvokeCommandResult {
    (self.handler)(input)
  }
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
  handler: fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
) -> InvokeCommand {
  InvokeCommand {
    id,
    namespace,
    summary,
    args,
    handler,
  }
}
