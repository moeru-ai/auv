use std::io;

use anstream::{AutoStream, ColorChoice};

use crate::{InvokeOutputOptions, InvokeResult, RunStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeCliOutcome {
  pub exit_code: i32,
}

impl InvokeCliOutcome {
  pub fn from_status(status: RunStatus) -> Self {
    Self {
      exit_code: if status == RunStatus::Failed { 1 } else { 0 },
    }
  }
}

pub fn render_invoke_result(result: &InvokeResult, options: InvokeOutputOptions) -> Result<InvokeCliOutcome, String> {
  if options.json {
    let mut stdout = io::stdout().lock();
    result.write_json(&mut stdout, options)?;
  } else {
    let stdout = io::stdout();
    let mut stream = AutoStream::new(stdout.lock(), ColorChoice::Auto);
    result.write_human(&mut stream, options, true)?;
  }
  Ok(InvokeCliOutcome::from_status(result.status.clone()))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_tracing::{ArtifactMetadata, ArtifactPurpose, ArtifactUri, Attributes, ByteLength, ContentType, RunId, Sha256Digest};
  use serde_json::Value;

  use crate::{
    InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow, InvokeResult,
    RunStatus,
  };

  fn fixture_result(status: RunStatus) -> InvokeResult {
    let mut signals = BTreeMap::new();
    signals.insert("result_dir".to_string(), "/tmp/auv-result".to_string());
    signals.insert("operatorSummary".to_string(), "raw operator text".to_string());
    signals.insert("selected_target".to_string(), "Safari address field".to_string());

    InvokeResult {
      run_id: "run_fixture".to_string(),
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
        tables: vec![
          InvokeReportTable::new(
            vec!["REF".to_string(), "APP".to_string()],
            vec![InvokeReportTableRow {
              cells: vec![
                "fixture_0".to_string(),
                "Fixture Application With A Long Display Name".to_string(),
              ],
            }],
          )
          .with_display_max_chars(vec![None, Some(16)]),
        ],
        wide_tables: vec![InvokeReportTable::new(
          vec!["REF".to_string(), "APP".to_string(), "PID".to_string()],
          vec![InvokeReportTableRow {
            cells: vec![
              "fixture_0".to_string(),
              "Fixture Application With A Long Display Name".to_string(),
              "1234".to_string(),
            ],
          }],
        )],
        sections: vec![InvokeReportSection {
          title: "fixture_0".to_string(),
          fields: vec![InvokeReportField {
            label: "Role".to_string(),
            value: "primary".to_string(),
          }],
        }],
      }),
      canonical_artifacts: vec![ArtifactMetadata::new(
        ArtifactUri::from_ids(RunId::new(), auv_tracing::ArtifactId::new()),
        ArtifactPurpose::parse("auv.test.screenshot").expect("purpose"),
        ContentType::parse("image/png").expect("content type"),
        ByteLength::new(1).expect("length"),
        Sha256Digest::new([0; 32]),
        Attributes::empty(),
      )],
      artifact_failures: Vec::new(),
      failure_message: None,
    }
  }

  #[test]
  fn default_success_omits_operator_summary_raw_signals_artifacts_notes_and_limits() {
    let output = fixture_result(RunStatus::Completed).render_to_string(Default::default()).expect("render should succeed");

    assert!(output.contains("OK. Run: run_fixture"));
    assert!(output.contains("fixture.observe - Observe fixture"));
    assert!(output.contains("Result: observed"));
    assert!(output.contains("REF"));
    assert!(output.contains("fixture_0"));
    assert!(output.contains("Fixture Appli..."));
    assert!(!output.contains("Fixture Application With A Long Display Name"));
    assert!(output.contains("fixture_0"));
    assert!(!output.contains("PID"));
    assert!(!output.contains("1234"));
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

    let output = result.render_to_string(Default::default()).expect("render should succeed");

    assert!(output.contains("ERROR. Run: run_fixture"));
    assert!(output.contains("fixture failed"));
    assert!(output.contains("Inspect: auv inspect run_fixture"));
  }

  #[test]
  fn failed_output_without_a_store_omits_the_inspect_hint() {
    let mut result = fixture_result(RunStatus::Failed);
    result.failure_message = Some("fixture failed".to_string());

    let output = result
      .render_to_string(InvokeOutputOptions {
        inspect_hint: false,
        ..InvokeOutputOptions::default()
      })
      .expect("render should succeed");

    assert!(!output.contains("auv inspect run_fixture"));
  }

  #[test]
  fn detail_includes_notes_limits_verification_artifacts_and_selected_signals() {
    let output = fixture_result(RunStatus::Completed)
      .render_to_string(InvokeOutputOptions {
        json: false,
        detail: true,
        wide: false,
        inspect_hint: true,
      })
      .expect("render should succeed");

    assert!(output.contains("Notes"));
    assert!(output.contains("note for detail"));
    assert!(output.contains("Known limits"));
    assert!(output.contains("limit for detail"));
    assert!(output.contains("Verification"));
    assert!(output.contains("activation_only"));
    assert!(output.contains("Artifacts"));
    assert!(output.contains("auv.test.screenshot"));
    assert!(output.contains("Signals"));
    assert!(output.contains("selected_target: Safari address field"));
  }

  #[test]
  fn wide_output_renders_wide_report_table() {
    let output = fixture_result(RunStatus::Completed)
      .render_to_string(InvokeOutputOptions {
        json: false,
        detail: false,
        wide: true,
        inspect_hint: true,
      })
      .expect("render should succeed");

    assert!(output.contains("PID"));
    assert!(output.contains("1234"));
  }

  #[test]
  fn json_output_parses_and_contains_no_ansi() {
    let output = fixture_result(RunStatus::Completed)
      .render_to_string(InvokeOutputOptions {
        json: true,
        detail: false,
        wide: false,
        inspect_hint: true,
      })
      .expect("render should succeed");

    assert!(!output.contains("\u{1b}["));
    let value: Value = serde_json::from_str(&output).expect("json should parse");
    assert_eq!(value["run_id"], "run_fixture");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["command_id"], "fixture.observe");
    assert_eq!(value["summary"], "fixture observed");
    assert!(value.get("report").is_some());
    assert_eq!(value["report"]["tables"][0]["rows"][0]["cells"][1], "Fixture Application With A Long Display Name");
    assert!(value["report"].get("wide_tables").is_none());
    assert!(value["report"]["tables"][0].get("display_max_chars").is_none());
    assert!(value.get("artifacts").is_some());
    assert!(value.get("signals").is_none());
  }
}
