use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use auv_tracing::{ArtifactMetadata, ArtifactUri, RunId};
use serde::Serialize;

use super::{InvokeOutputOptions, InvokeReport, InvokeReportField};
use crate::models::invoke_report::{label, write_error, write_field_rows};
use crate::{InvokeCommand, InvokeExecutionResult, InvokeFailure};

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
    artifacts: Vec<InvokeArtifactResult>,
  },
  Failed {
    failure: InvokeFailure,
  },
}

impl InvokeResult {
  /// Maps the direct command value into CLI-only presentation state.
  pub fn from_command_result(run_id: RunId, command: &InvokeCommand, result: InvokeExecutionResult) -> Self {
    match result {
      Ok(output) => Self {
        run_id,
        command_id: command.id.to_string(),
        command_description: command.description.to_string(),
        terminal: InvokeTerminal::Completed {
          result: output.result().cloned(),
          report: output.report.clone(),
          artifacts: output.artifacts().iter().cloned().map(InvokeArtifactResult::new).collect(),
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

  /// Adds file-backed locations supplied by the frontend's concrete store.
  pub fn with_artifact_paths(mut self, paths: impl IntoIterator<Item = (ArtifactUri, PathBuf)>) -> Self {
    let paths = paths.into_iter().collect::<BTreeMap<_, _>>();
    if let InvokeTerminal::Completed { artifacts, .. } = &mut self.terminal {
      for artifact in artifacts {
        artifact.file_path = paths.get(artifact.metadata.uri()).cloned();
      }
    }
    self
  }

  pub fn failure(&self) -> Option<&str> {
    match &self.terminal {
      InvokeTerminal::Completed { .. } => None,
      InvokeTerminal::Failed { failure } => Some(&failure.message),
    }
  }

  pub(crate) fn write_json<W: Write>(&self, writer: &mut W) -> Result<(), String> {
    let output = InvokeResultJsonOutput {
      run_id: &self.run_id,
      status: self.status().as_str(),
      command_id: &self.command_id,
      result: self.result(),
      artifacts: self.artifacts(),
      failure: self.failure(),
      failure_details: match &self.terminal {
        InvokeTerminal::Failed { failure } => Some(failure),
        _ => None,
      },
    };
    serde_json::to_writer_pretty(&mut *writer, &output).map_err(|error| format!("failed to serialize invoke output: {error}"))?;
    writeln!(writer).map_err(|error| format!("failed to write invoke output: {error}"))
  }

  pub(crate) fn write_human<W: Write>(&self, writer: &mut W, options: InvokeOutputOptions, color: bool) -> Result<(), String> {
    let terminal_status = match self.status() {
      InvokeStatus::Completed => "OK",
      InvokeStatus::Failed => "ERROR",
    };
    writeln!(writer, "{}. {}: {}", terminal_status, label("Run", color), self.run_id).map_err(write_error)?;
    writeln!(writer).map_err(write_error)?;
    writeln!(writer, "● {} - {}", self.command_id, self.command_description).map_err(write_error)?;

    if let Some(failure) = self.failure() {
      write_field_rows(writer, &[InvokeReportField::new("Failure", failure)], color)?;
    }

    if let Some(report) = self.report() {
      report.write_human(writer, options, color)?;
    }

    if !self.artifacts().is_empty() {
      writeln!(writer).map_err(write_error)?;
      writeln!(writer, "  Artifacts").map_err(write_error)?;
      for artifact in self.artifacts() {
        let location =
          artifact.file_path.as_ref().map(|path| path.display().to_string()).unwrap_or_else(|| artifact.metadata.uri().to_string());
        write_field_rows(
          writer,
          &[
            InvokeReportField::new(artifact.metadata.purpose().as_str(), location),
            InvokeReportField::new("Artifact URI", artifact.metadata.uri().to_string()),
          ],
          color,
        )?;
      }
    }

    Ok(())
  }

  pub fn render_to_string(&self, options: InvokeOutputOptions) -> Result<String, String> {
    let mut bytes = Vec::new();
    if options.json {
      self.write_json(&mut bytes)?;
    } else {
      self.write_human(&mut bytes, options, false)?;
    }
    String::from_utf8(bytes).map_err(|error| format!("renderer emitted invalid UTF-8: {error}"))
  }

  fn artifacts(&self) -> &[InvokeArtifactResult] {
    match &self.terminal {
      InvokeTerminal::Completed { artifacts, .. } => artifacts,
      InvokeTerminal::Failed { .. } => &[],
    }
  }
}

#[derive(Clone, Debug, Serialize)]
struct InvokeArtifactResult {
  #[serde(flatten)]
  metadata: ArtifactMetadata,
  #[serde(skip_serializing_if = "Option::is_none")]
  file_path: Option<PathBuf>,
}

impl InvokeArtifactResult {
  fn new(metadata: ArtifactMetadata) -> Self {
    Self {
      metadata,
      file_path: None,
    }
  }
}

#[derive(Serialize)]
struct InvokeResultJsonOutput<'a> {
  run_id: &'a RunId,
  status: &'a str,
  command_id: &'a str,
  result: Option<&'a serde_json::Value>,
  #[serde(skip_serializing_if = "<[InvokeArtifactResult]>::is_empty")]
  artifacts: &'a [InvokeArtifactResult],
  #[serde(skip_serializing_if = "Option::is_none")]
  failure: Option<&'a str>,
  #[serde(skip_serializing_if = "Option::is_none")]
  failure_details: Option<&'a InvokeFailure>,
}

#[cfg(test)]
#[path = "invoke_result_test.rs"]
mod tests;
