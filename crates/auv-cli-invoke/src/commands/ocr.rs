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
    tables: vec![InvokeReportTable::new(columns, match_rows(matches, selected_index, false)).with_display_max_chars(display_max_chars)],
    wide_tables: vec![
      InvokeReportTable::new(wide_columns, match_rows(matches, selected_index, true)).with_display_max_chars(wide_display_max_chars),
    ],
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
      InvokeReportTableRow::new(cells)
    })
    .collect()
}

#[cfg(test)]
#[path = "ocr_test.rs"]
mod tests;
