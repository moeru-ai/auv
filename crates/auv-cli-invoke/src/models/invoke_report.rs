use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use comfy_table::{Cell, Table, presets::NOTHING};

use super::InvokeOutputOptions;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReport {
  pub fields: Vec<InvokeReportField>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub tables: Vec<InvokeReportTable>,
  #[serde(default, skip)]
  pub wide_tables: Vec<InvokeReportTable>,
  pub sections: Vec<InvokeReportSection>,
}

impl InvokeReport {
  pub fn new(fields: Vec<InvokeReportField>, sections: Vec<InvokeReportSection>) -> Self {
    Self {
      fields,
      tables: Vec::new(),
      wide_tables: Vec::new(),
      sections,
    }
  }

  pub(crate) fn write_human<W: Write>(&self, writer: &mut W, options: InvokeOutputOptions, color: bool) -> Result<(), String> {
    write_field_rows(writer, &self.fields, color)?;

    for table in self.human_tables(options) {
      writeln!(writer).map_err(write_error)?;
      table.write_human(writer)?;
    }

    for section in &self.sections {
      writeln!(writer).map_err(write_error)?;
      writeln!(writer, "  {}", section.title).map_err(write_error)?;
      write_field_rows(writer, &section.fields, color)?;
    }

    Ok(())
  }

  fn human_tables(&self, options: InvokeOutputOptions) -> &[InvokeReportTable] {
    if options.wide && !self.wide_tables.is_empty() {
      &self.wide_tables
    } else {
      &self.tables
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportField {
  pub label: String,
  pub value: String,
}

impl InvokeReportField {
  pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
    Self {
      label: label.into(),
      value: value.into(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportSection {
  pub title: String,
  pub fields: Vec<InvokeReportField>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportTable {
  pub columns: Vec<String>,
  pub rows: Vec<InvokeReportTableRow>,
  #[serde(default, skip)]
  pub display_max_chars: Vec<Option<usize>>,
}

impl InvokeReportTable {
  pub fn new(columns: Vec<String>, rows: Vec<InvokeReportTableRow>) -> Self {
    Self {
      columns,
      rows,
      display_max_chars: Vec::new(),
    }
  }

  pub fn with_display_max_chars(mut self, display_max_chars: Vec<Option<usize>>) -> Self {
    self.display_max_chars = display_max_chars;
    self
  }

  pub fn from_columns(columns: &[&str], rows: Vec<InvokeReportTableRow>) -> Self {
    Self::new(columns.iter().map(|column| (*column).to_string()).collect(), rows)
  }

  pub fn from_columns_with_display_max_chars(
    columns: &[&str],
    rows: Vec<InvokeReportTableRow>,
    display_max_chars: Vec<Option<usize>>,
  ) -> Self {
    Self::from_columns(columns, rows).with_display_max_chars(display_max_chars)
  }

  pub(crate) fn write_human<W: Write>(&self, writer: &mut W) -> Result<(), String> {
    let mut rendered = Table::new();
    rendered.load_preset(NOTHING);
    rendered.set_header(self.columns.iter().map(Cell::new));
    for row in &self.rows {
      rendered.add_row(row.cells.iter().enumerate().map(|(index, cell)| Cell::new(self.display_cell(index, cell))));
    }
    for line in rendered.to_string().lines() {
      writeln!(writer, "  {}", line.trim()).map_err(write_error)?;
    }
    Ok(())
  }

  fn display_cell(&self, column_index: usize, value: &str) -> String {
    match self.display_max_chars.get(column_index).copied().flatten() {
      Some(max_chars) => truncate(value, max_chars),
      None => value.to_string(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportTableRow {
  pub cells: Vec<String>,
}

impl InvokeReportTableRow {
  pub fn new(cells: Vec<String>) -> Self {
    Self { cells }
  }

  pub fn from_cells(cells: impl IntoIterator<Item = String>) -> Self {
    Self::new(cells.into_iter().collect())
  }
}

pub(crate) trait InvokeReportValue {
  fn report_value(&self) -> String;
}

impl InvokeReportValue for auv_driver::Point {
  fn report_value(&self) -> String {
    format!("{:.0},{:.0}", self.x, self.y)
  }
}

impl InvokeReportValue for auv_driver::Rect {
  fn report_value(&self) -> String {
    format!("{:.0},{:.0} {:.0}x{:.0}", self.origin.x, self.origin.y, self.size.width, self.size.height)
  }
}

pub(crate) trait InvokeSignalValue {
  fn signal_value(&self) -> String;
}

impl InvokeSignalValue for auv_driver::Rect {
  fn signal_value(&self) -> String {
    format!("x={:.0},y={:.0},width={:.0},height={:.0}", self.origin.x, self.origin.y, self.size.width, self.size.height)
  }
}

pub(crate) trait OptionalReportText<'a> {
  fn report_or(self, fallback: &'a str) -> &'a str;
}

impl<'a> OptionalReportText<'a> for Option<&'a str> {
  fn report_or(self, fallback: &'a str) -> &'a str {
    self.filter(|value| !value.trim().is_empty()).unwrap_or(fallback)
  }
}

pub(crate) trait InvokeReportLabels {
  fn report_labels(&self) -> String;
}

impl InvokeReportLabels for Vec<&'static str> {
  fn report_labels(&self) -> String {
    self.join(",")
  }
}

pub(super) fn write_detail_section<W: Write>(writer: &mut W, title: &str, rows: &[String], color: bool) -> Result<(), String> {
  writeln!(writer).map_err(write_error)?;
  writeln!(writer, "  {}", label(title, color)).map_err(write_error)?;
  for row in rows {
    writeln!(writer, "    {row}").map_err(write_error)?;
  }
  Ok(())
}

pub(super) fn write_field_rows<W: Write>(writer: &mut W, fields: &[InvokeReportField], color: bool) -> Result<(), String> {
  for field in fields {
    writeln!(writer, "  {}: {}", label(&field.label, color), field.value).map_err(write_error)?;
  }
  Ok(())
}

fn label(value: &str, color: bool) -> String {
  if color {
    let style: Style = AnsiColor::BrightBlack.on_default();
    format!("{style}{value}{style:#}")
  } else {
    value.to_string()
  }
}

fn truncate(value: &str, max_chars: usize) -> String {
  if value.chars().count() <= max_chars {
    return value.to_string();
  }
  if max_chars <= 3 {
    return ".".repeat(max_chars);
  }
  let mut truncated = value.chars().take(max_chars - 3).collect::<String>();
  truncated.push_str("...");
  truncated
}

fn write_error(error: io::Error) -> String {
  format!("failed to write invoke output: {error}")
}
