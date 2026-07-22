use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use auv_tracing_driver::trace::{ArtifactRecordV1Alpha1, SpanId};
use serde::Serialize;

use super::{InvokeOutputOptions, InvokeReport, InvokeReportField};
use crate::models::invoke_report::{write_detail_section, write_field_rows};
use crate::{InvokeCommand, InvokeCommandResult};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
  Completed,
  Failed,
}

impl RunStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
  pub run_id: String,
  pub producer_span_id: SpanId,
  pub command_id: String,
  pub command_summary: String,
  pub status: RunStatus,
  pub output_summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
  pub known_limits: Vec<String>,
  pub verification: Option<String>,
  pub report: Option<InvokeReport>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}

impl InvokeResult {
  /// Maps the direct command value into CLI-only presentation state.
  pub fn from_command_result(run_id: impl Into<String>, command: &InvokeCommand, result: InvokeCommandResult) -> Self {
    let run_id = run_id.into();
    // NOTICE(run-recording-v1): This legacy CLI field is not canonical run
    // truth. Task 22 removes it with the remaining recorded adapter surface.
    let producer_span_id = SpanId::new(format!("frontend-root:{run_id}"));
    match result {
      Ok(output) => Self {
        run_id,
        producer_span_id,
        command_id: command.id.to_string(),
        command_summary: command.summary.to_string(),
        status: RunStatus::Completed,
        output_summary: output.summary,
        backend: output.backend,
        signals: output.signals,
        notes: output.notes,
        known_limits: output.known_limits,
        verification: output.verification,
        report: output.report,
        artifacts: Vec::new(),
        artifact_paths: Vec::new(),
        failure_message: None,
      },
      Err(error) => Self {
        run_id,
        producer_span_id,
        command_id: command.id.to_string(),
        command_summary: command.summary.to_string(),
        status: RunStatus::Failed,
        output_summary: error.clone(),
        backend: None,
        signals: BTreeMap::new(),
        notes: Vec::new(),
        known_limits: Vec::new(),
        verification: None,
        report: None,
        artifacts: Vec::new(),
        artifact_paths: Vec::new(),
        failure_message: Some(error),
      },
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
    writeln!(writer, "● {} - {}", self.command_id, self.command_summary).map_err(write_error)?;

    if let Some(failure) = self.failure_message.as_deref() {
      write_field_rows(writer, &[InvokeReportField::new("Failure", failure)], color)?;
    }

    match self.report.as_ref() {
      Some(report) => report.write_human(writer, options, color)?,
      None => write_field_rows(writer, &[InvokeReportField::new("Output", &self.output_summary)], color)?,
    }

    if self.status == RunStatus::Failed {
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

    if options.detail {
      self.write_human_detail(writer, color)?;
    }

    Ok(())
  }

  pub fn render_to_string(&self, options: InvokeOutputOptions) -> Result<String, String> {
    let mut bytes = Vec::new();
    self.write_rendered(&mut bytes, options)?;
    String::from_utf8(bytes).map_err(|error| format!("renderer emitted invalid UTF-8: {error}"))
  }

  fn write_human_detail<W: Write>(&self, writer: &mut W, color: bool) -> Result<(), String> {
    if let Some(backend) = self.backend.as_deref() {
      write_detail_section(writer, "Backend", &[backend.to_string()], color)?;
    }
    if let Some(verification) = self.verification.as_deref() {
      write_detail_section(writer, "Verification", &[verification.to_string()], color)?;
    }
    if !self.notes.is_empty() {
      write_detail_section(writer, "Notes", &self.notes, color)?;
    }
    if !self.known_limits.is_empty() {
      write_detail_section(writer, "Known limits", &self.known_limits, color)?;
    }
    if !self.artifacts.is_empty() {
      let artifacts = self
        .artifacts
        .iter()
        .map(|artifact| {
          let mut line = format!("{} ({})", artifact.artifact_id, artifact.role);
          if let Some(summary) = artifact.summary.as_deref() {
            line.push_str(": ");
            line.push_str(summary);
          }
          line
        })
        .collect::<Vec<_>>();
      write_detail_section(writer, "Artifacts", &artifacts, color)?;
    }

    let signals = selected_detail_signals(&self.signals).into_iter().map(|(key, value)| format!("{key}: {value}")).collect::<Vec<_>>();
    if !signals.is_empty() {
      write_detail_section(writer, "Signals", &signals, color)?;
    }

    Ok(())
  }

  fn terminal_status(&self) -> &'static str {
    match self.status {
      RunStatus::Completed => "OK",
      RunStatus::Failed => "ERROR",
    }
  }
}

#[derive(Serialize)]
struct InvokeResultJsonOutput<'a> {
  run_id: &'a str,
  status: &'a str,
  command_id: &'a str,
  summary: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  report: Option<&'a InvokeReport>,
  #[serde(skip_serializing_if = "Option::is_none")]
  failure: Option<&'a str>,
  artifacts: Vec<InvokeResultJsonArtifact<'a>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  signals: Option<BTreeMap<String, String>>,
}

impl<'a> InvokeResultJsonOutput<'a> {
  fn from_result(result: &'a InvokeResult, options: InvokeOutputOptions) -> Self {
    Self {
      run_id: &result.run_id,
      status: result.status.as_str(),
      command_id: &result.command_id,
      summary: &result.output_summary,
      report: result.report.as_ref(),
      failure: result.failure_message.as_deref(),
      artifacts: result
        .artifacts
        .iter()
        .map(|artifact| InvokeResultJsonArtifact {
          artifact_id: artifact.artifact_id.as_str(),
          role: &artifact.role,
          mime_type: &artifact.mime_type,
          summary: artifact.summary.as_deref(),
        })
        .collect(),
      signals: options.detail.then(|| selected_detail_signals(&result.signals)),
    }
  }
}

#[derive(Serialize)]
struct InvokeResultJsonArtifact<'a> {
  artifact_id: &'a str,
  role: &'a str,
  mime_type: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  summary: Option<&'a str>,
}

fn selected_detail_signals(signals: &BTreeMap<String, String>) -> BTreeMap<String, String> {
  signals
    .iter()
    .filter(|(key, _)| {
      let normalized = key.replace('_', "").to_ascii_lowercase();
      normalized != "operatorsummary"
    })
    .map(|(key, value)| (key.clone(), value.clone()))
    .collect()
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
