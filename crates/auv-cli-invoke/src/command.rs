use std::collections::BTreeMap;
use std::sync::Arc;

use auv_tracing::ArtifactMetadata;

use crate::InvokeReport;
use crate::arg::ArgSpec;

use crate::{ArtifactInstrumentationFailure, ArtifactInstrumentationReceipt};

pub type InvokeCommandFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<InvokeCommandOutput, String>> + Send + 'static>>;
pub type InvokeCommandHandler = fn(InvokeCommandInput) -> InvokeCommandFuture;

/// Cloneable cancellation shared by one frontend dispatch and its typed command.
#[derive(Clone, Debug)]
pub struct InvokeCancellation {
  token: Arc<tokio_util::sync::CancellationToken>,
}

impl InvokeCancellation {
  pub fn new() -> Self {
    Self {
      token: Arc::new(tokio_util::sync::CancellationToken::new()),
    }
  }

  pub fn from_token(token: tokio_util::sync::CancellationToken) -> Self {
    Self {
      token: Arc::new(token),
    }
  }

  pub fn cancel(&self) {
    self.token.cancel();
  }

  pub fn is_cancelled(&self) -> bool {
    self.token.is_cancelled()
  }

  pub fn check(&self) -> Result<(), InvokeCancelled> {
    if self.is_cancelled() {
      Err(InvokeCancelled)
    } else {
      Ok(())
    }
  }

  pub async fn cancelled(&self) {
    self.token.cancelled().await;
  }
}

impl Default for InvokeCancellation {
  fn default() -> Self {
    Self::new()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invoke cancelled")]
pub struct InvokeCancelled;

#[derive(Clone, Debug)]
pub struct InvokeCommandInput {
  pub command_id: String,
  pub target_application_id: Option<String>,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
  pub cancellation: InvokeCancellation,
}

impl InvokeCommandInput {
  pub fn required_input(&self, name: &str) -> Result<&str, String> {
    self
      .inputs
      .get(name)
      .map(String::as_str)
      .filter(|value| !value.trim().is_empty())
      .ok_or_else(|| format!("{} requires --{name}", self.command_id))
  }

  pub fn required_f64(&self, name: &str) -> Result<f64, String> {
    self.required_input(name)?.parse::<f64>().map_err(|error| format!("{} received invalid --{name}: {error}", self.command_id))
  }

  pub fn target_or_input_target(&self) -> Option<&str> {
    self.target_application_id.as_deref().or_else(|| self.inputs.get("target").map(String::as_str)).filter(|value| !value.trim().is_empty())
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InvokeCommandOutput {
  pub summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
  /// Canonical metadata returned directly by successful artifact publications.
  pub artifacts: Vec<ArtifactMetadata>,
  /// Non-authoritative diagnostics from attempted artifact instrumentation.
  pub artifact_failures: Vec<ArtifactInstrumentationFailure>,
  pub known_limits: Vec<String>,
  pub report: Option<InvokeReport>,
  /// CLI presentation failure derived from an otherwise successful typed call.
  pub failure_message: Option<String>,
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
      artifact_failures: Vec::new(),
      known_limits: Vec::new(),
      report: None,
      failure_message: None,
      verification: None,
    }
  }

  pub fn apply_artifact_instrumentation(&mut self, receipt: ArtifactInstrumentationReceipt) {
    let (artifacts, failures) = receipt.into_parts();
    self.artifacts.extend(artifacts);
    self.artifact_failures.extend(failures);
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
  Scan,
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
      Self::Scan => "scan",
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
  pub fn invoke(&self, input: InvokeCommandInput) -> InvokeCommandFuture {
    if let Err(error) = input.cancellation.check() {
      return Box::pin(async move { Err(error.to_string()) });
    }
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
  handler: InvokeCommandHandler,
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
  use super::{InvokeCancellation, InvokeCommandOutput};

  #[test]
  fn invoke_cancellation_is_typed_cloneable_and_observable() {
    let cancellation = InvokeCancellation::new();
    let observer = cancellation.clone();

    assert!(observer.check().is_ok());
    cancellation.cancel();

    let error = observer.check().expect_err("shared cancellation must be observable");
    assert_eq!(error.to_string(), "invoke cancelled");
  }

  #[test]
  fn command_output_defaults_instrumentation_and_report_fields_to_empty() {
    let output = InvokeCommandOutput::new("observed");

    assert!(output.artifacts.is_empty());
    assert!(output.artifact_failures.is_empty());
    assert!(output.known_limits.is_empty());
    assert!(output.report.is_none());
    assert!(output.failure_message.is_none());
    assert!(output.verification.is_none());
  }
}
