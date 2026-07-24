use std::io;

use anstream::{AutoStream, ColorChoice};

use crate::{InvokeOutputOptions, InvokeResult, InvokeStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeCliOutcome {
  pub exit_code: i32,
}

impl InvokeCliOutcome {
  pub fn from_status(status: InvokeStatus) -> Self {
    Self {
      exit_code: if status == InvokeStatus::Failed { 1 } else { 0 },
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
  Ok(InvokeCliOutcome::from_status(result.status()))
}

#[cfg(test)]
mod tests {
  use std::str::FromStr;

  use auv_tracing::RunId;
  use serde::Serialize;
  use serde_json::Value;

  use crate::{
    InvokeOutputOptions, InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow, InvokeResult,
    InvokeStatus,
  };

  fn fixture_result(status: InvokeStatus) -> InvokeResult {
    #[derive(Serialize)]
    struct FixtureResult {
      observed: bool,
      candidate_count: u32,
    }

    let report = InvokeReport {
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
    };
    let registry = crate::default_registry();
    let command = registry.resolve("scan.frame").expect("scan command");
    let result = match status {
      InvokeStatus::Completed => {
        let mut output = crate::InvokeCommandOutput::from_result(&FixtureResult {
          observed: true,
          candidate_count: 3,
        })
        .expect("fixture result should serialize");
        output.report = Some(report);
        Ok(output)
      }
      InvokeStatus::Failed => Err("fixture failed".to_string()),
    };
    InvokeResult::from_command_result(fixture_run_id(), command, result)
  }

  fn fixture_run_id() -> RunId {
    RunId::from_str("019f8b1e-4b2d-7a00-8f00-0000000000aa").expect("fixture run id")
  }

  #[test]
  fn default_success_omits_notes_and_limits() {
    let output = fixture_result(InvokeStatus::Completed).render_to_string(Default::default()).expect("render should succeed");

    assert!(output.contains("OK. Run: 019f8b1e-4b2d-7a00-8f00-0000000000aa"));
    assert!(output.contains("scan.frame - Produce a single scan-frame-v0"));
    assert!(output.contains("Result: observed"));
    assert!(output.contains("REF"));
    assert!(output.contains("fixture_0"));
    assert!(output.contains("Fixture Appli..."));
    assert!(!output.contains("Fixture Application With A Long Display Name"));
    assert!(output.contains("fixture_0"));
    assert!(!output.contains("PID"));
    assert!(!output.contains("1234"));
    assert!(!output.contains("note for detail"));
    assert!(!output.contains("limit for detail"));
  }

  #[test]
  fn failed_output_renders_error_failure_message_and_inspect_hint() {
    let result = fixture_result(InvokeStatus::Failed);

    let output = result.render_to_string(Default::default()).expect("render should succeed");

    assert!(output.contains("ERROR. Run: 019f8b1e-4b2d-7a00-8f00-0000000000aa"));
    assert!(output.contains("fixture failed"));
    assert!(output.contains("Inspect: auv inspect 019f8b1e-4b2d-7a00-8f00-0000000000aa"));
  }

  #[test]
  fn failed_output_without_a_store_omits_the_inspect_hint() {
    let result = fixture_result(InvokeStatus::Failed);

    let output = result
      .render_to_string(InvokeOutputOptions {
        inspect_hint: false,
        ..InvokeOutputOptions::default()
      })
      .expect("render should succeed");

    assert!(!output.contains("auv inspect 019f8b1e-4b2d-7a00-8f00-0000000000aa"));
  }

  #[test]
  fn detail_renders_domain_report_sections_without_a_generic_metadata_bag() {
    let output = fixture_result(InvokeStatus::Completed)
      .render_to_string(InvokeOutputOptions {
        json: false,
        detail: true,
        wide: false,
        inspect_hint: true,
      })
      .expect("render should succeed");

    assert!(output.contains("Role"));
    assert!(output.contains("primary"));
    assert!(!output.contains("Known limits"));
    assert!(!output.contains("Verification"));
  }

  #[test]
  fn wide_output_renders_wide_report_table() {
    let output = fixture_result(InvokeStatus::Completed)
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
    let output = fixture_result(InvokeStatus::Completed)
      .render_to_string(InvokeOutputOptions {
        json: true,
        detail: false,
        wide: false,
        inspect_hint: true,
      })
      .expect("render should succeed");

    assert!(!output.contains("\u{1b}["));
    let value: Value = serde_json::from_str(&output).expect("json should parse");
    assert_eq!(value["run_id"], "019f8b1e-4b2d-7a00-8f00-0000000000aa");
    assert_eq!(value["status"], "completed");
    assert_eq!(value["command_id"], "scan.frame");
    assert!(value.get("summary").is_none());
    assert!(value.get("report").is_none());
    assert_eq!(value["result"]["observed"], true);
    assert_eq!(value["result"]["candidate_count"], 3);
    assert!(value.get("artifacts").is_none());
  }
}
