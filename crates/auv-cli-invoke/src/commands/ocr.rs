use crate::{InvokeReport, InvokeReportField, InvokeReportTable, InvokeReportTableRow, InvokeReportValue};

pub(super) fn match_report(matches: &[auv_driver::OcrMatch], selected_index: Option<usize>) -> InvokeReport {
  let has_selection = selected_index.is_some();
  let columns = if has_selection {
    &["SEL", "IDX", "TEXT", "POINT", "BOUNDS"][..]
  } else {
    &["IDX", "TEXT", "POINT", "BOUNDS"][..]
  };
  let wide_columns = if has_selection {
    &["SEL", "IDX", "TEXT", "POINT", "BOUNDS", "CONF"][..]
  } else {
    &["IDX", "TEXT", "POINT", "BOUNDS", "CONF"][..]
  };
  let display_max_chars = if has_selection {
    vec![None, None, Some(48), None, None]
  } else {
    vec![None, Some(48), None, None]
  };
  let wide_display_max_chars = if has_selection {
    vec![None, None, Some(48), None, None, None]
  } else {
    vec![None, Some(48), None, None, None]
  };

  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} text match(es)", matches.len()),
    )],
    tables: vec![InvokeReportTable::from_columns_with_display_max_chars(
      columns,
      match_rows(matches, selected_index, false),
      display_max_chars,
    )],
    wide_tables: vec![InvokeReportTable::from_columns_with_display_max_chars(
      wide_columns,
      match_rows(matches, selected_index, true),
      wide_display_max_chars,
    )],
    sections: Vec::new(),
  }
}

fn match_rows(matches: &[auv_driver::OcrMatch], selected_index: Option<usize>, wide: bool) -> Vec<crate::InvokeReportTableRow> {
  matches
    .iter()
    .enumerate()
    .map(|(index, matched)| {
      let mut cells = Vec::new();
      if let Some(selected_index) = selected_index {
        cells.push(if index == selected_index { "*" } else { "" }.to_string());
      }
      cells.extend([
        index.to_string(),
        matched.text.clone(),
        matched.action_point().report_value(),
        matched.bounds.report_value(),
      ]);
      if wide {
        cells.push(format!("{:.3}", matched.confidence));
      }
      InvokeReportTableRow::from_cells(cells)
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use auv_driver::{OcrMatch, Rect};

  use super::*;

  #[test]
  fn match_report_uses_default_table_and_wide_confidence_column() {
    let matches = vec![
      OcrMatch {
        text: "Play".to_string(),
        confidence: 0.92,
        bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
      },
      OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.81,
        bounds: Rect::new(50.0, 60.0, 70.0, 80.0),
      },
    ];

    let report = match_report(&matches, None);

    assert_eq!(report.fields[0].value, "2 text match(es)");
    assert_eq!(report.tables[0].columns, ["IDX", "TEXT", "POINT", "BOUNDS"]);
    assert_eq!(report.tables[0].display_max_chars, [None, Some(48), None, None]);
    assert_eq!(report.tables[0].rows[0].cells, ["0", "Play", "25,40", "10,20 30x40"]);
    assert_eq!(report.wide_tables[0].columns, ["IDX", "TEXT", "POINT", "BOUNDS", "CONF"]);
    assert_eq!(report.wide_tables[0].display_max_chars, [None, Some(48), None, None, None]);
    assert_eq!(report.wide_tables[0].rows[0].cells[4], "0.920");
  }

  #[test]
  fn match_report_marks_selected_match_for_click_results() {
    let matches = vec![OcrMatch {
      text: "Play".to_string(),
      confidence: 0.92,
      bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
    }];

    let report = match_report(&matches, Some(0));

    assert_eq!(report.tables[0].columns, ["SEL", "IDX", "TEXT", "POINT", "BOUNDS"]);
    assert_eq!(report.tables[0].rows[0].cells[0], "*");
    assert_eq!(report.tables[0].display_max_chars, [None, None, Some(48), None, None]);
  }

  #[test]
  fn match_report_preserves_full_text_for_machine_output() {
    let text = "A very long OCR match that should remain complete in the report data before rendering".to_string();
    let matches = vec![OcrMatch {
      text: text.clone(),
      confidence: 0.92,
      bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
    }];

    let report = match_report(&matches, None);

    assert_eq!(report.tables[0].rows[0].cells[1], text);
  }
}
