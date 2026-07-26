use serde::Serialize;
use thiserror::Error;

use super::formats::table::TableOptions;

/// The presentation selected by a command frontend.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutputFormat {
  /// Compact table output without supplementary human context.
  Table,
  /// Compact table output followed by app-owned supplementary context.
  #[default]
  Human,
  /// The app-owned structured result as pretty JSON.
  Json,
}

#[derive(Debug, Error)]
pub enum OutputError {
  #[error("failed to render JSON output: {0}")]
  Json(#[from] serde_json::Error),
}

/// App-owned output presented through the shared CLI format routes.
pub trait CliOutput {
  fn to_json(&self) -> impl Serialize;

  fn to_table_print(&self, options: TableOptions<'_>) -> String;

  /// Additional human-only context appended after the compact table.
  fn human_details(&self, _options: TableOptions<'_>) -> Option<String> {
    None
  }

  fn to_human(&self, options: TableOptions<'_>) -> String {
    let mut output = self.to_table_print(options);
    if let Some(details) = self.human_details(options).filter(|details| !details.is_empty()) {
      if !output.is_empty() {
        output.push_str("\n\n");
      }
      output.push_str(&details);
    }
    output
  }
}

/// Render one output value without deciding where stdout or files live.
pub fn render<O>(output: &O, format: OutputFormat, options: TableOptions<'_>) -> Result<String, OutputError>
where
  O: CliOutput,
{
  // TODO(cli-output-target-v1): stdout and durable/atomic file writes remain
  // frontend-owned until their error and persistence semantics are unified.
  match format {
    OutputFormat::Table => Ok(output.to_table_print(options)),
    OutputFormat::Human => Ok(output.to_human(options)),
    OutputFormat::Json => Ok(serde_json::to_string_pretty(&output.to_json())?),
  }
}

#[cfg(test)]
#[path = "cli_test.rs"]
mod tests;
