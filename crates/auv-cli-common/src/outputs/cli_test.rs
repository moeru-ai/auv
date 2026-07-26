use serde::Serialize;

use super::*;
use crate::{TableRow, outputs::formats::table};

#[derive(Serialize)]
struct JsonOutput<'a> {
  command: &'a str,
}

#[derive(TableRow)]
struct SummaryRow<'a> {
  command: &'a str,
}

struct CommandResult;

impl CliOutput for CommandResult {
  fn to_json(&self) -> impl Serialize {
    JsonOutput { command: "demo.ls" }
  }

  fn to_table_print(&self, options: TableOptions<'_>) -> String {
    table::render(&[SummaryRow { command: "demo.ls" }], options)
  }

  fn human_details(&self, _options: TableOptions<'_>) -> Option<String> {
    Some("known_limits:\n  (none)".to_string())
  }
}

#[test]
fn routes_table_human_and_json_without_owning_io() {
  let result = CommandResult;

  assert_eq!(render(&result, OutputFormat::Table, TableOptions::default()).unwrap(), "COMMAND\ndemo.ls");
  assert_eq!(render(&result, OutputFormat::Human, TableOptions::default()).unwrap(), "COMMAND\ndemo.ls\n\nknown_limits:\n  (none)");
  assert_eq!(render(&result, OutputFormat::Json, TableOptions::default()).unwrap(), "{\n  \"command\": \"demo.ls\"\n}");
}
