use std::collections::BTreeMap;

use crate::arg::ArgSpec;
use auv_tracing_driver::ProducedArtifact;

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
  pub artifacts: Vec<ProducedArtifact>,
  pub known_limits: Vec<String>,
  /// Human-readable boundary claim produced by the handler for this execution.
  ///
  /// This is intentionally not a structured `VerificationResult`: direct
  /// invoke commands such as capture/OCR often need to state "capture-only" or
  /// "recognition-only" without claiming semantic success.
  // TODO(invoke-boundary-claims): promote this event-backed string into a
  // first-class read-side boundary-claim model after the shape in
  // `docs/ai/references/2026-06-18-invoke-direct-command-implementations-handoff.md`
  // is accepted. Do not map capture-only/recognition-only/activation-only
  // claims into semantic `VerificationResult`s.
  pub verification: Option<String>,
}

impl InvokeCommandOutput {
  pub fn new(summary: impl Into<String>) -> Self {
    Self {
      summary: summary.into(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
      artifacts: Vec::new(),
      known_limits: Vec::new(),
      verification: None,
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

#[cfg(test)]
mod tests {
  use super::InvokeCommandOutput;

  #[test]
  fn command_output_defaults_evidence_fields_to_empty() {
    let output = InvokeCommandOutput::new("observed");

    assert!(output.artifacts.is_empty());
    assert!(output.known_limits.is_empty());
    assert!(output.verification.is_none());
  }
}
