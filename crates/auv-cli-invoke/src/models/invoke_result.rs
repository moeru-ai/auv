use std::io::Write;

use auv_tracing::RunId;
use serde::Serialize;

use super::{InvokeOutputOptions, InvokeReport, InvokeReportField};
use crate::models::invoke_report::write_field_rows;
use crate::{InvokeCommand, InvokeCommandResult};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvokeStatus {
  Completed,
  Failed,
}

impl InvokeStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
  pub run_id: RunId,
  pub command_id: String,
  pub command_description: String,
  terminal: InvokeTerminal,
}

#[derive(Clone, Debug)]
enum InvokeTerminal {
  Completed {
    result: Option<serde_json::Value>,
    report: Option<InvokeReport>,
  },
  Failed {
    failure: String,
  },
}

impl InvokeResult {
  /// Maps the direct command value into CLI-only presentation state.
  pub fn from_command_result(run_id: RunId, command: &InvokeCommand, result: InvokeCommandResult) -> Self {
    match result {
      Ok(output) => Self {
        run_id,
        command_id: command.id.to_string(),
        command_description: command.description.to_string(),
        terminal: InvokeTerminal::Completed {
          result: output.result().cloned(),
          report: output.report,
        },
      },
      Err(error) => Self {
        run_id,
        command_id: command.id.to_string(),
        command_description: command.description.to_string(),
        terminal: InvokeTerminal::Failed { failure: error },
      },
    }
  }

  pub fn status(&self) -> InvokeStatus {
    match self.terminal {
      InvokeTerminal::Completed { .. } => InvokeStatus::Completed,
      InvokeTerminal::Failed { .. } => InvokeStatus::Failed,
    }
  }

  pub fn report(&self) -> Option<&InvokeReport> {
    match &self.terminal {
      InvokeTerminal::Completed { report, .. } => report.as_ref(),
      InvokeTerminal::Failed { .. } => None,
    }
  }

  pub fn result(&self) -> Option<&serde_json::Value> {
    match &self.terminal {
      InvokeTerminal::Completed { result, .. } => result.as_ref(),
      InvokeTerminal::Failed { .. } => None,
    }
  }

  pub fn failure(&self) -> Option<&str> {
    match &self.terminal {
      InvokeTerminal::Completed { .. } => None,
      InvokeTerminal::Failed { failure } => Some(failure),
    }
  }

  pub(crate) fn write_rendered<W: Write>(&self, writer: &mut W, options: InvokeOutputOptions) -> Result<(), String> {
    if options.json {
      self.write_json(writer, options)
    } else {
      self.write_human(writer, options, false)
    }
  }

  pub(crate) fn write_json<W: Write>(&self, writer: &mut W, options: InvokeOutputOptions) -> Result<(), String> {
    let output = InvokeResultJsonOutput::from_result(self, options);
    serde_json::to_writer_pretty(&mut *writer, &output).map_err(|error| format!("failed to serialize invoke output: {error}"))?;
    writeln!(writer).map_err(|error| format!("failed to write invoke output: {error}"))
  }

  pub(crate) fn write_human<W: Write>(&self, writer: &mut W, options: InvokeOutputOptions, color: bool) -> Result<(), String> {
    writeln!(writer, "{}. {}: {}", self.terminal_status(), label("Run", color), self.run_id).map_err(write_error)?;
    writeln!(writer).map_err(write_error)?;
    writeln!(writer, "● {} - {}", self.command_id, self.command_description).map_err(write_error)?;

    if let Some(failure) = self.failure() {
      write_field_rows(writer, &[InvokeReportField::new("Failure", failure)], color)?;
    }

    if let Some(report) = self.report() {
      report.write_human(writer, options, color)?;
    }

    if self.status() == InvokeStatus::Failed && options.inspect_hint {
      writeln!(writer).map_err(write_error)?;
      write_field_rows(
        writer,
        &[InvokeReportField::new(
          "Inspect",
          format!("auv inspect {}", self.run_id),
        )],
        color,
      )?;
    }

    Ok(())
  }

  pub fn render_to_string(&self, options: InvokeOutputOptions) -> Result<String, String> {
    let mut bytes = Vec::new();
    self.write_rendered(&mut bytes, options)?;
    String::from_utf8(bytes).map_err(|error| format!("renderer emitted invalid UTF-8: {error}"))
  }

  fn terminal_status(&self) -> &'static str {
    match self.status() {
      InvokeStatus::Completed => "OK",
      InvokeStatus::Failed => "ERROR",
    }
  }
}

#[derive(Serialize)]
struct InvokeResultJsonOutput<'a> {
  run_id: &'a RunId,
  status: &'a str,
  command_id: &'a str,
  result: Option<&'a serde_json::Value>,
  #[serde(skip_serializing_if = "Option::is_none")]
  failure: Option<&'a str>,
}

impl<'a> InvokeResultJsonOutput<'a> {
  fn from_result(result: &'a InvokeResult, _options: InvokeOutputOptions) -> Self {
    Self {
      run_id: &result.run_id,
      status: result.status().as_str(),
      command_id: &result.command_id,
      result: result.result(),
      failure: result.failure(),
    }
  }
}

fn label(value: &str, color: bool) -> String {
  if color {
    let style: anstyle::Style = anstyle::AnsiColor::BrightBlack.on_default();
    format!("{style}{value}{style:#}")
  } else {
    value.to_string()
  }
}

fn write_error(error: std::io::Error) -> String {
  format!("failed to write invoke output: {error}")
}

#[cfg(test)]
mod tests {
  use auv_tracing::RunId;

  use crate::{InvokeCommandOutput, default_registry};

  use super::InvokeResult;

  #[test]
  fn direct_command_result_does_not_require_store_readback() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("command");
    let result = InvokeResult::from_command_result(RunId::new(), command, Ok(InvokeCommandOutput::completed()));

    assert_eq!(result.status(), super::InvokeStatus::Completed);
    assert!(result.failure().is_none());
  }
}
