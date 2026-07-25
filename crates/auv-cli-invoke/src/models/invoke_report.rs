use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use comfy_table::{Cell, ColumnConstraint, ContentArrangement, Row, Table, Width, presets::NOTHING};

use super::InvokeOutputOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeReport {
  pub fields: Vec<InvokeReportField>,
  pub tables: Vec<InvokeReportTable>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeReportSection {
  pub title: String,
  pub fields: Vec<InvokeReportField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeReportTable {
  pub columns: Vec<String>,
  pub rows: Vec<InvokeReportTableRow>,
  pub display_max_chars: Vec<Option<usize>>,
}

impl InvokeReportTable {
  pub fn new<C>(columns: impl IntoIterator<Item = C>, rows: Vec<InvokeReportTableRow>) -> Self
  where
    C: AsRef<str>,
  {
    Self {
      columns: columns.into_iter().map(|column| column.as_ref().to_string()).collect(),
      rows,
      display_max_chars: Vec::new(),
    }
  }

  pub fn with_display_max_chars(mut self, display_max_chars: Vec<Option<usize>>) -> Self {
    self.display_max_chars = display_max_chars;
    self
  }

  pub(crate) fn write_human<W: Write>(&self, writer: &mut W) -> Result<(), String> {
    let mut rendered = Table::new();
    rendered.load_preset(NOTHING);
    rendered.set_content_arrangement(ContentArrangement::Dynamic);
    rendered.set_header(self.columns.iter().map(Cell::new));
    rendered.set_constraints(
      self
        .display_max_chars
        .iter()
        .map(|limit| ColumnConstraint::UpperBoundary(Width::Fixed(limit.and_then(|value| u16::try_from(value).ok()).unwrap_or(u16::MAX)))),
    );
    for row in &self.rows {
      let mut rendered_row = Row::from(row.cells.iter().map(Cell::new).collect::<Vec<_>>());
      rendered_row.max_height(1);
      rendered.add_row(rendered_row);
    }
    for line in rendered.to_string().lines() {
      writeln!(writer, "  {}", line.trim()).map_err(write_error)?;
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeReportTableRow {
  pub cells: Vec<String>,
}

impl InvokeReportTableRow {
  pub fn new(cells: impl IntoIterator<Item = String>) -> Self {
    Self {
      cells: cells.into_iter().collect(),
    }
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

impl InvokeReportValue for auv_scan::ScanBounds {
  fn report_value(&self) -> String {
    format!("{},{} {}x{}", self.x, self.y, self.width, self.height)
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

pub(super) fn write_field_rows<W: Write>(writer: &mut W, fields: &[InvokeReportField], color: bool) -> Result<(), String> {
  for field in fields {
    writeln!(writer, "  {}: {}", label(&field.label, color), field.value).map_err(write_error)?;
  }
  Ok(())
}

pub(super) fn label(value: &str, color: bool) -> String {
  if color {
    let style: Style = AnsiColor::BrightBlack.on_default();
    format!("{style}{value}{style:#}")
  } else {
    value.to_string()
  }
}

pub(super) fn write_error(error: io::Error) -> String {
  format!("failed to write invoke output: {error}")
}
