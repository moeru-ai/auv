use std::collections::BTreeMap;

use auv_driver::{OperationDisturbance, OperationNamespace, OperationSpec};

use crate::arg::ArgSpec;

pub const NONE: &[OperationDisturbance] = &[OperationDisturbance::None];
pub const NONE_OR_FOREGROUND: &[OperationDisturbance] = &[
  OperationDisturbance::None,
  OperationDisturbance::ForegroundApp,
];
pub(crate) const FOREGROUND_KEYBOARD: &[OperationDisturbance] = &[
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Keyboard,
];
pub(crate) const FOREGROUND_KEYBOARD_CLIPBOARD: &[OperationDisturbance] = &[
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Keyboard,
  OperationDisturbance::Clipboard,
];
pub(crate) const MEDIA_TRANSPORT: &[OperationDisturbance] = &[OperationDisturbance::Keyboard];
pub(crate) const FOREGROUND_ONLY: &[OperationDisturbance] = &[OperationDisturbance::ForegroundApp];
pub(crate) const FOCUS_POINTER_ENTRY: &[OperationDisturbance] = &[
  OperationDisturbance::Focus,
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Keyboard,
  OperationDisturbance::Pointer,
];
pub(crate) const POINTER_WITH_FOREGROUND: &[OperationDisturbance] = &[
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Pointer,
];
pub(crate) const PRESS_BUTTON_DISTURBANCE: &[OperationDisturbance] = &[
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Keyboard,
  OperationDisturbance::Pointer,
];
pub(crate) const CAPTURE_AX_TREE_DISTURBANCE: &[OperationDisturbance] = &[
  OperationDisturbance::ForegroundApp,
  OperationDisturbance::Keyboard,
];

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

#[derive(Clone, Copy, Debug)]
pub struct InvokeContext<'a> {
  pub target_application_id: Option<&'a str>,
  pub target_label: Option<&'a str>,
  pub inputs: &'a BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeDriverDispatch {
  pub command_id: &'static str,
  pub driver_id: &'static str,
  pub operation: &'static str,
  pub target_application_id: Option<String>,
  pub target_label: Option<String>,
  pub inputs: BTreeMap<String, String>,
}

pub type InvokeCommandHandler = fn(InvokeContext<'_>, &InvokeCommand) -> InvokeDriverDispatch;

#[derive(Clone, Debug)]
pub struct InvokeCommand {
  pub operation: OperationSpec,
  pub namespace: InvokeNamespace,
  pub args: &'static [ArgSpec],
  pub artifacts: &'static [&'static str],
  pub signals: &'static [&'static str],
  pub verification: &'static str,
  pub handler: InvokeCommandHandler,
  pub handler_name: &'static str,
}

impl InvokeCommand {
  pub fn dispatch(&self, context: InvokeContext<'_>) -> InvokeDriverDispatch {
    (self.handler)(context, self)
  }

  pub fn with_handler(mut self, handler: InvokeCommandHandler, handler_name: &'static str) -> Self {
    self.handler = handler;
    self.handler_name = handler_name;
    self
  }
}

pub fn default_driver_dispatch(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  InvokeDriverDispatch {
    command_id: command.operation.id,
    driver_id: command.operation.driver_id,
    operation: command.operation.operation,
    target_application_id: context.target_application_id.map(str::to_string),
    target_label: context.target_label.map(str::to_string),
    inputs: context.inputs.clone(),
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
// directly to one part of the public help/operation contract.
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn spec(
  id: &'static str,
  namespace: InvokeNamespace,
  summary: &'static str,
  driver_id: &'static str,
  operation: &'static str,
  disturbance_classes: &'static [OperationDisturbance],
  max_disturbance: OperationDisturbance,
  args: &'static [ArgSpec],
  artifacts: &'static [&'static str],
  signals: &'static [&'static str],
  verification: &'static str,
) -> InvokeCommand {
  spec_with_operation_namespace(
    id,
    namespace,
    operation_namespace_for_invoke(namespace),
    summary,
    driver_id,
    operation,
    disturbance_classes,
    max_disturbance,
    args,
    artifacts,
    signals,
    verification,
  )
}

// Same declaration DSL as `spec`, with an explicit driver operation namespace
// for commands whose invoke group does not imply the operation family.
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn spec_with_operation_namespace(
  id: &'static str,
  namespace: InvokeNamespace,
  operation_namespace: OperationNamespace,
  summary: &'static str,
  driver_id: &'static str,
  operation: &'static str,
  disturbance_classes: &'static [OperationDisturbance],
  max_disturbance: OperationDisturbance,
  args: &'static [ArgSpec],
  artifacts: &'static [&'static str],
  signals: &'static [&'static str],
  verification: &'static str,
) -> InvokeCommand {
  InvokeCommand {
    operation: OperationSpec {
      id,
      summary,
      driver_id,
      operation,
      disturbance_classes,
      max_disturbance,
      namespace: operation_namespace,
    },
    namespace,
    args,
    artifacts,
    signals,
    verification,
    handler: default_driver_dispatch,
    handler_name: "default_driver_dispatch",
  }
}

fn operation_namespace_for_invoke(namespace: InvokeNamespace) -> OperationNamespace {
  match namespace {
    InvokeNamespace::Display
    | InvokeNamespace::Screen
    | InvokeNamespace::Window
    | InvokeNamespace::MediaControl
    | InvokeNamespace::Steam
    | InvokeNamespace::Fixture => OperationNamespace::Observe,
    InvokeNamespace::Input | InvokeNamespace::App => OperationNamespace::Action,
    InvokeNamespace::Overlay => OperationNamespace::Overlay,
  }
}
