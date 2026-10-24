//! Run-recording vocabulary for one invoke command execution.
//!
//! Every frontend that runs a registered command (CLI, MCP, library) gets the
//! same records from [`InvokeCommand::invoke`](crate::InvokeCommand::invoke)
//! and [`runner::invoke`](crate::runner::invoke):
//!
//! ```text
//! span   auv.invoke.command          command_id, execution, dry_run
//! event  auv.invoke.command_started  target and protocol inputs
//! ...    artifacts and Runner RPC spans emitted by the command
//! event  auv.invoke.command_finished status, direct result or failure
//! ```
//!
//! A span that ends without `command_finished` was dropped before it
//! completed, for example by MCP request cancellation.

use std::collections::BTreeMap;
use std::future::Future;

use auv_tracing::{AttributeValue, Attributes, EventPayload, SpanSpec};
use serde::Serialize;

pub const COMMAND_SPAN: &str = "auv.invoke.command";
pub const COMMAND_STARTED: &str = "auv.invoke.command_started";
pub const COMMAND_FINISHED: &str = "auv.invoke.command_finished";

/// Event payloads are bounded to 64 KiB by `auv-tracing`; an oversized payload
/// is dropped whole, so these bounds keep the records themselves.
///
/// NOTICE(invoke-trace-bounds): one input value and one direct result are cut
/// independently. Typed text longer than this is recorded as a prefix with
/// its full length, so one long input cannot push the whole event past the
/// limit and lose it.
const MAX_INPUT_VALUE_BYTES: usize = 4 * 1024;
const MAX_RESULT_BYTES: usize = 32 * 1024;

/// Where the command's driver work ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
  /// The frontend process called its own local driver.
  Local,
  /// The frontend called a daemon-owned Runner over gRPC; the Runner records
  /// its RPC spans as children of this command span.
  Runner,
}

impl Execution {
  fn as_str(self) -> &'static str {
    match self {
      Self::Local => "local",
      Self::Runner => "runner",
    }
  }
}

struct CommandSpan<'a> {
  command_id: &'a str,
  execution: Execution,
  dry_run: bool,
}

impl SpanSpec for CommandSpan<'_> {
  const NAME: &'static str = COMMAND_SPAN;

  fn attributes(&self) -> Attributes {
    Attributes::from_iter([
      ("command_id".to_string(), AttributeValue::string(self.command_id)),
      ("execution".to_string(), AttributeValue::string(self.execution.as_str())),
      ("dry_run".to_string(), AttributeValue::boolean(self.dry_run)),
    ])
  }
}

// NOTICE(invoke-trace-text): inputs are recorded verbatim up to the bound,
// including text for `input.typeText` and `input.pasteText`. AUV cannot tell a
// password field from any other target here.
// TODO(input-text-redaction): add a per-call opt-out or a redaction policy when
// an owner names the callers that type secrets through invoke.
#[derive(Serialize)]
struct CommandStarted<'a> {
  command_id: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  target: Option<String>,
  inputs: BTreeMap<&'a str, BoundedText<'a>>,
  dry_run: bool,
}

impl EventPayload for CommandStarted<'_> {
  const NAME: &'static str = COMMAND_STARTED;
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(untagged)]
enum BoundedText<'a> {
  Whole(&'a str),
  Cut { prefix: &'a str, byte_length: usize },
}

impl<'a> BoundedText<'a> {
  fn new(value: &'a str) -> Self {
    if value.len() <= MAX_INPUT_VALUE_BYTES {
      return Self::Whole(value);
    }
    let mut end = MAX_INPUT_VALUE_BYTES;
    while !value.is_char_boundary(end) {
      end -= 1;
    }
    Self::Cut {
      prefix: &value[..end],
      byte_length: value.len(),
    }
  }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CommandStatus {
  Succeeded,
  Failed,
}

#[derive(Serialize)]
struct CommandFinished<'a> {
  command_id: &'a str,
  status: CommandStatus,
  /// The direct result, when it fits; see [`MAX_RESULT_BYTES`].
  #[serde(skip_serializing_if = "Option::is_none")]
  result: Option<&'a serde_json::Value>,
  #[serde(skip_serializing_if = "Option::is_none")]
  result_byte_length: Option<usize>,
  #[serde(skip_serializing_if = "Option::is_none")]
  failure: Option<&'a crate::InvokeFailure>,
}

impl EventPayload for CommandFinished<'_> {
  const NAME: &'static str = COMMAND_FINISHED;
  const VERSION: u32 = 1;
}

/// Runs `execute` inside one command span and records its start and outcome.
///
/// Call it where the current context is the frontend's Run root, so the span
/// becomes a child of that Run; without a recording context it only runs.
pub(crate) fn record_command<F>(
  input: crate::InvokeCommandInput,
  execution: Execution,
  execute: impl FnOnce(crate::InvokeCommandInput) -> F,
) -> impl Future<Output = crate::InvokeExecutionResult> + Send + 'static
where
  F: Future<Output = crate::InvokeExecutionResult> + Send + 'static,
{
  let command_id = input.command_id.clone();
  let span = auv_tracing::start_span(CommandSpan {
    command_id: &command_id,
    execution,
    dry_run: input.dry_run,
  });
  if span.is_enabled() {
    span.in_scope(|| {
      auv_tracing::emit_event!(CommandStarted {
        command_id: &command_id,
        target: input.target.as_ref().map(target_text),
        inputs: input.inputs.iter().map(|(key, value)| (key.as_str(), BoundedText::new(value))).collect(),
        dry_run: input.dry_run,
      });
    });
  }
  // Built after the span opens, so a command that emits while it is being
  // constructed still lands inside the span.
  let execute = span.in_scope(|| execute(input));
  span.instrument(async move {
    let result = execute.await;
    record_outcome(&command_id, &result);
    result
  })
}

fn record_outcome(command_id: &str, result: &crate::InvokeExecutionResult) {
  if !auv_tracing::Context::current().is_enabled() {
    return;
  }
  let event = match result {
    Ok(output) => {
      let encoded_length = output.result().map(|value| serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len()));
      let fits = encoded_length.is_none_or(|length| length <= MAX_RESULT_BYTES);
      CommandFinished {
        command_id,
        status: CommandStatus::Succeeded,
        result: output.result().filter(|_| fits),
        result_byte_length: encoded_length.filter(|_| !fits),
        failure: None,
      }
    }
    Err(failure) => CommandFinished {
      command_id,
      status: CommandStatus::Failed,
      result: None,
      result_byte_length: None,
      failure: Some(failure),
    },
  };
  auv_tracing::emit_event!(event);
}

fn target_text(target: &crate::ExecutionTarget) -> String {
  match target {
    crate::ExecutionTarget::Application { id } => format!("app:{id}"),
    crate::ExecutionTarget::Window { id } => format!("window:{id}"),
    crate::ExecutionTarget::Display { id } => format!("display:{id}"),
  }
}

#[cfg(test)]
#[path = "trace_test.rs"]
mod tests;
