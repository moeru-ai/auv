use crate::{InvokeReport, InvokeReportField, InvokeReportTable, InvokeReportTableRow};

pub(super) fn match_report(matches: &[auv_driver_macos::OcrMatch], selected_index: Option<usize>) -> InvokeReport {
  let has_selection = selected_index.is_some();
  InvokeReport {
    fields: vec![report_field(
      "Result",
      format!("{} text match(es)", matches.len()),
    )],
    tables: vec![
      InvokeReportTable::new(columns(has_selection, false), rows(matches, selected_index, false))
        .with_display_max_chars(display_max_chars(has_selection, false)),
    ],
    wide_tables: vec![
      InvokeReportTable::new(columns(has_selection, true), rows(matches, selected_index, true))
        .with_display_max_chars(display_max_chars(has_selection, true)),
    ],
    sections: Vec::new(),
  }
}

fn columns(has_selection: bool, wide: bool) -> Vec<String> {
  let mut columns = Vec::new();
  if has_selection {
    columns.push("SEL".to_string());
  }
  columns.extend(["IDX", "TEXT", "POINT", "BOUNDS"].map(str::to_string));
  if wide {
    columns.push("CONF".to_string());
  }
  columns
}

fn display_max_chars(has_selection: bool, wide: bool) -> Vec<Option<usize>> {
  let mut max_chars = Vec::new();
  if has_selection {
    max_chars.push(None);
  }
  max_chars.extend([None, Some(48), None, None]);
  if wide {
    max_chars.push(None);
  }
  max_chars
}

fn rows(matches: &[auv_driver_macos::OcrMatch], selected_index: Option<usize>, wide: bool) -> Vec<InvokeReportTableRow> {
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
        format_point(matched.action_point()),
        format_rect(matched.bounds),
      ]);
      if wide {
        cells.push(format!("{:.3}", matched.confidence));
      }
      InvokeReportTableRow { cells }
    })
    .collect()
}

fn report_field(label: &str, value: impl Into<String>) -> InvokeReportField {
  InvokeReportField {
    label: label.to_string(),
    value: value.into(),
  }
}

fn format_point(point: auv_driver::Point) -> String {
  format!("{:.0},{:.0}", point.x, point.y)
}

fn format_rect(rect: auv_driver::Rect) -> String {
  format!("{:.0},{:.0} {:.0}x{:.0}", rect.origin.x, rect.origin.y, rect.size.width, rect.size.height)
}

#[cfg(test)]
mod tests {
  use auv_driver::Rect;
  use auv_driver_macos::OcrMatch;

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
