use std::path::{Path, PathBuf};

use comfy_table::{Cell, Table, presets::NOTHING};

/// One column in a derived table schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Column {
  pub header: &'static str,
}

/// Selects which columns are visible and how an empty table is explained.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TableOptions<'a> {
  pub wide: bool,
  pub empty_message: Option<&'a str>,
}

impl<'a> TableOptions<'a> {
  pub const fn wide(mut self, wide: bool) -> Self {
    self.wide = wide;
    self
  }

  pub const fn empty_message(mut self, message: &'a str) -> Self {
    self.empty_message = Some(message);
    self
  }
}

/// Describes one typed row without exposing `comfy_table` to callers.
pub trait TableRow {
  // TODO(cli-table-schema-v1): nested/flattened rows are deferred until an
  // app-owned output needs them; explicit presentation rows keep v0 narrow.
  fn columns(options: TableOptions<'_>) -> Vec<Column>;

  fn cells(&self, options: TableOptions<'_>) -> Vec<String>;
}

/// Convert a common Rust value into stable CLI cell text.
pub trait TableValue {
  fn table_value(&self) -> String;

  fn is_zero(&self) -> bool {
    false
  }
}

impl TableValue for str {
  fn table_value(&self) -> String {
    self.to_string()
  }

  fn is_zero(&self) -> bool {
    self.is_empty()
  }
}

impl TableValue for String {
  fn table_value(&self) -> String {
    self.clone()
  }

  fn is_zero(&self) -> bool {
    self.is_empty()
  }
}

impl<T> TableValue for &T
where
  T: TableValue + ?Sized,
{
  fn table_value(&self) -> String {
    (*self).table_value()
  }

  fn is_zero(&self) -> bool {
    (*self).is_zero()
  }
}

impl<T> TableValue for Option<T>
where
  T: TableValue,
{
  fn table_value(&self) -> String {
    self.as_ref().map(TableValue::table_value).unwrap_or_else(|| "-".to_string())
  }

  fn is_zero(&self) -> bool {
    self.is_none()
  }
}

impl TableValue for bool {
  fn table_value(&self) -> String {
    self.to_string()
  }

  fn is_zero(&self) -> bool {
    !self
  }
}

macro_rules! impl_display_table_value {
  ($($type:ty),+ $(,)?) => {
    $(
      impl TableValue for $type {
        fn table_value(&self) -> String {
          self.to_string()
        }

        fn is_zero(&self) -> bool {
          *self == 0 as $type
        }
      }
    )+
  };
}

impl_display_table_value!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64,);

impl TableValue for char {
  fn table_value(&self) -> String {
    self.to_string()
  }

  fn is_zero(&self) -> bool {
    *self == '\0'
  }
}

impl TableValue for Path {
  fn table_value(&self) -> String {
    self.display().to_string()
  }

  fn is_zero(&self) -> bool {
    self.as_os_str().is_empty()
  }
}

impl TableValue for PathBuf {
  fn table_value(&self) -> String {
    self.display().to_string()
  }

  fn is_zero(&self) -> bool {
    self.as_os_str().is_empty()
  }
}

/// Render rows using AUV's compact, borderless `comfy_table` presentation.
pub fn render<R>(rows: &[R], options: TableOptions<'_>) -> String
where
  R: TableRow,
{
  let mut table = Table::new();
  table.load_preset(NOTHING);
  table.set_header(R::columns(options).into_iter().map(|column| Cell::new(column.header)));
  for row in rows {
    table.add_row(row.cells(options).into_iter().map(Cell::new));
  }

  let mut output = table.to_string().lines().map(str::trim).collect::<Vec<_>>().join("\n");
  if rows.is_empty()
    && let Some(message) = options.empty_message
  {
    output.push('\n');
    output.push_str(message);
  }
  output
}

#[cfg(test)]
#[path = "table_test.rs"]
mod tests;
