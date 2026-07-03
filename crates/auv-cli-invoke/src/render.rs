use std::collections::BTreeMap;
use std::io::{self, Write};

use anstream::{AutoStream, ColorChoice};
use anstyle::{AnsiColor, Style};
use serde::Serialize;

use crate::{InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeResult, RunStatus};

pub fn render_invoke_result(
  result: &InvokeResult,
  options: InvokeOutputOptions,
) -> Result<(), String> {
  if options.json {
    let mut stdout = io::stdout().lock();
    write_json(&mut stdout, result, options)
  } else {
    let stdout = io::stdout();
    let mut stream = AutoStream::new(stdout.lock(), ColorChoice::Auto);
    write_human(&mut stream, result, options, true)
  }
}

pub fn write_rendered<W: Write>(
  writer: &mut W,
  result: &InvokeResult,
  options: InvokeOutputOptions,
) -> Result<(), String> {
  if options.json {
    write_json(writer, result, options)
  } else {
    write_human(writer, result, options, false)
  }
}

pub fn render_to_string(
  result: &InvokeResult,
  options: InvokeOutputOptions,
) -> Result<String, String> {
  let mut bytes = Vec::new();
  write_rendered(&mut bytes, result, options)?;
  String::from_utf8(bytes).map_err(|error| format!("renderer emitted invalid UTF-8: {error}"))
}

fn write_json<W: Write>(
  writer: &mut W,
  result: &InvokeResult,
  options: InvokeOutputOptions,
) -> Result<(), String> {
  let output = InvokeJsonOutput::from_result(result, options);
  serde_json::to_writer_pretty(&mut *writer, &output)
    .map_err(|error| format!("failed to serialize invoke output: {error}"))?;
  writeln!(writer).map_err(|error| format!("failed to write invoke output: {error}"))
}

fn write_human<W: Write>(
  writer: &mut W,
  result: &InvokeResult,
  options: InvokeOutputOptions,
  color: bool,
) -> Result<(), String> {
  writeln!(
    writer,
    "{}. {}: {}",
    terminal_status(&result.status),
    label("Run", color),
    result.run_id
  )
  .map_err(write_error)?;
  writeln!(writer).map_err(write_error)?;
  writeln!(
    writer,
    "● {} - {}",
    result.command_id, result.command_summary
  )
  .map_err(write_error)?;

  if let Some(failure) = result.failure_message.as_deref() {
    write_field_rows(writer, &[field("Failure", failure)], color)?;
  }

  match result.report.as_ref() {
    Some(report) => write_report(writer, report, color)?,
    None => write_field_rows(writer, &[field("Output", &result.output_summary)], color)?,
  }

  if result.status == RunStatus::Failed {
    writeln!(writer).map_err(write_error)?;
    write_field_rows(
      writer,
      &[field("Inspect", &format!("auv inspect {}", result.run_id))],
      color,
    )?;
  }

  if options.detail {
    write_detail(writer, result, color)?;
  }

  Ok(())
}

fn write_report<W: Write>(
  writer: &mut W,
  report: &InvokeReport,
  color: bool,
) -> Result<(), String> {
  write_field_rows(writer, &report.fields, color)?;

  for section in &report.sections {
    writeln!(writer).map_err(write_error)?;
    writeln!(writer, "  {}", section.title).map_err(write_error)?;
    write_field_rows(writer, &section.fields, color)?;
  }

  Ok(())
}

fn write_detail<W: Write>(
  writer: &mut W,
  result: &InvokeResult,
  color: bool,
) -> Result<(), String> {
  if let Some(backend) = result.backend.as_deref() {
    write_detail_section(writer, "Backend", &[backend.to_string()], color)?;
  }
  if let Some(verification) = result.verification.as_deref() {
    write_detail_section(writer, "Verification", &[verification.to_string()], color)?;
  }
  if !result.notes.is_empty() {
    write_detail_section(writer, "Notes", &result.notes, color)?;
  }
  if !result.known_limits.is_empty() {
    write_detail_section(writer, "Known limits", &result.known_limits, color)?;
  }
  if !result.artifacts.is_empty() {
    let artifacts = result
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

  let signals = selected_signals(&result.signals)
    .into_iter()
    .map(|(key, value)| format!("{key}: {value}"))
    .collect::<Vec<_>>();
  if !signals.is_empty() {
    write_detail_section(writer, "Signals", &signals, color)?;
  }

  Ok(())
}

fn write_detail_section<W: Write>(
  writer: &mut W,
  title: &str,
  rows: &[String],
  color: bool,
) -> Result<(), String> {
  writeln!(writer).map_err(write_error)?;
  writeln!(writer, "  {}", label(title, color)).map_err(write_error)?;
  for row in rows {
    writeln!(writer, "    {row}").map_err(write_error)?;
  }
  Ok(())
}

fn write_field_rows<W: Write>(
  writer: &mut W,
  fields: &[InvokeReportField],
  color: bool,
) -> Result<(), String> {
  for field in fields {
    writeln!(writer, "  {}: {}", label(&field.label, color), field.value).map_err(write_error)?;
  }
  Ok(())
}

fn terminal_status(status: &RunStatus) -> &'static str {
  match status {
    RunStatus::Completed => "OK",
    RunStatus::Failed => "ERROR",
  }
}

fn field(label: &str, value: &str) -> InvokeReportField {
  InvokeReportField {
    label: label.to_string(),
    value: value.to_string(),
  }
}

fn label(value: &str, color: bool) -> String {
  if color {
    let style: Style = AnsiColor::BrightBlack.on_default();
    format!("{style}{value}{style:#}")
  } else {
    value.to_string()
  }
}

fn selected_signals(signals: &BTreeMap<String, String>) -> BTreeMap<String, String> {
  signals
    .iter()
    .filter(|(key, _)| {
      let normalized = key.replace('_', "").to_ascii_lowercase();
      normalized != "operatorsummary"
    })
    .map(|(key, value)| (key.clone(), value.clone()))
    .collect()
}

fn write_error(error: io::Error) -> String {
  format!("failed to write invoke output: {error}")
}

#[derive(Serialize)]
struct InvokeJsonOutput<'a> {
  run_id: &'a str,
  status: &'a str,
  command_id: &'a str,
  summary: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  report: Option<&'a InvokeReport>,
  #[serde(skip_serializing_if = "Option::is_none")]
  failure: Option<&'a str>,
  artifacts: Vec<InvokeJsonArtifact<'a>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  signals: Option<BTreeMap<String, String>>,
}

impl<'a> InvokeJsonOutput<'a> {
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
        .map(|artifact| InvokeJsonArtifact {
          artifact_id: artifact.artifact_id.as_str(),
          role: &artifact.role,
          mime_type: &artifact.mime_type,
          summary: artifact.summary.as_deref(),
        })
        .collect(),
      signals: options.detail.then(|| selected_signals(&result.signals)),
    }
  }
}

#[derive(Serialize)]
struct InvokeJsonArtifact<'a> {
  artifact_id: &'a str,
  role: &'a str,
  mime_type: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  summary: Option<&'a str>,
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_tracing_driver::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EventId, SpanId,
  };
  use serde_json::Value;

  use crate::{
    InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeReportSection, InvokeResult,
    RunStatus,
  };

  fn fixture_result(status: RunStatus) -> InvokeResult {
    let mut signals = BTreeMap::new();
    signals.insert("result_dir".to_string(), "/tmp/auv-result".to_string());
    signals.insert(
      "operatorSummary".to_string(),
      "raw operator text".to_string(),
    );
    signals.insert(
      "selected_target".to_string(),
      "Safari address field".to_string(),
    );

    InvokeResult {
      run_id: "run_fixture".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      command_id: "fixture.observe".to_string(),
      command_summary: "Observe fixture".to_string(),
      status,
      output_summary: "fixture observed".to_string(),
      backend: Some("macos".to_string()),
      signals,
      notes: vec!["note for detail".to_string()],
      known_limits: vec!["limit for detail".to_string()],
      verification: Some("activation_only".to_string()),
      report: Some(InvokeReport {
        fields: vec![InvokeReportField {
          label: "Result".to_string(),
          value: "observed".to_string(),
        }],
        sections: vec![InvokeReportSection {
          title: "fixture_0".to_string(),
          fields: vec![InvokeReportField {
            label: "Role".to_string(),
            value: "primary".to_string(),
          }],
        }],
      }),
      artifacts: vec![ArtifactRecordV1Alpha1 {
        api_version: ARTIFACT_API_VERSION.to_string(),
        artifact_id: ArtifactId::new("artifact_fixture"),
        span_id: SpanId::new("0000000000000001"),
        event_id: Some(EventId::new("event_fixture")),
        role: "screenshot".to_string(),
        mime_type: "image/png".to_string(),
        path: "artifacts/artifact_fixture.png".to_string(),
        sha256: None,
        attributes: BTreeMap::new(),
        summary: Some("fixture screenshot".to_string()),
      }],
      artifact_paths: vec!["/tmp/auv/artifact_fixture.png".into()],
      failure_message: None,
    }
  }

  #[test]
  fn default_success_omits_operator_summary_raw_signals_artifacts_notes_and_limits() {
    let output = super::render_to_string(&fixture_result(RunStatus::Completed), Default::default())
      .expect("render should succeed");

    assert!(output.contains("OK. Run: run_fixture"));
    assert!(output.contains("fixture.observe - Observe fixture"));
    assert!(output.contains("Result: observed"));
    assert!(output.contains("fixture_0"));
    assert!(!output.contains("operatorSummary"));
    assert!(!output.contains("raw operator text"));
    assert!(!output.contains("Signals"));
    assert!(!output.contains("artifact_fixture"));
    assert!(!output.contains("note for detail"));
    assert!(!output.contains("limit for detail"));
  }

  #[test]
  fn failed_output_renders_error_failure_message_and_inspect_hint() {
    let mut result = fixture_result(RunStatus::Failed);
    result.failure_message = Some("fixture failed".to_string());

    let output =
      super::render_to_string(&result, Default::default()).expect("render should succeed");

    assert!(output.contains("ERROR. Run: run_fixture"));
    assert!(output.contains("fixture failed"));
    assert!(output.contains("Inspect: auv inspect run_fixture"));
  }

  #[test]
  fn detail_includes_notes_limits_verification_artifacts_and_selected_signals() {
    let output = super::render_to_string(
      &fixture_result(RunStatus::Completed),
      InvokeOutputOptions {
        json: false,
        detail: true,
      },
    )
    .expect("render should succeed");

    assert!(output.contains("Notes"));
    assert!(output.contains("note for detail"));
    assert!(output.contains("Known limits"));
    assert!(output.contains("limit for detail"));
    assert!(output.contains("Verification"));
    assert!(output.contains("activation_only"));
    assert!(output.contains("Artifacts"));
    assert!(output.contains("artifact_fixture"));
    assert!(output.contains("Signals"));
    assert!(output.contains("selected_target: Safari address field"));
  }

  #[test]
  fn json_output_parses_and_contains_no_ansi() {
    let output = super::render_to_string(
      &fixture_result(RunStatus::Completed),
      InvokeOutputOptions {
        json: true,
        detail: false,
      },
    )
    .expect("render should succeed");

    assert!(!output.contains("\u{1b}["));
    let value: Value = serde_json::from_str(&output).expect("json should parse");
    assert_eq!(value["run_id"], "run_fixture");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["command_id"], "fixture.observe");
    assert_eq!(value["summary"], "fixture observed");
    assert!(value.get("report").is_some());
    assert!(value.get("artifacts").is_some());
    assert!(value.get("signals").is_none());
  }
}
