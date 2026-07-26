use auv_cli_common::TableRow;
use auv_cli_common::outputs::cli::CliOutput;
use auv_cli_common::outputs::formats::table::{self, TableOptions};
use serde::Serialize;

use crate::library::{Grounding, LibraryQuery, LibraryQueryResult, ResolvedLibraryScope, SteamInstalledApp};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LibraryLsJsonOutput<'a> {
  pub command: &'static str,
  pub query: &'a LibraryQuery,
  pub resolved_scope: &'a ResolvedLibraryScope,
  pub apps: &'a [SteamInstalledApp],
  pub diagnostics: &'a [crate::library::LibraryDiagnostic],
}

pub fn build_library_ls_json_output(result: &LibraryQueryResult) -> LibraryLsJsonOutput<'_> {
  LibraryLsJsonOutput {
    command: "library.ls",
    query: &result.query,
    resolved_scope: &result.resolved_scope,
    apps: &result.apps,
    diagnostics: &result.diagnostics,
  }
}

pub fn render_library_summary(result: &LibraryQueryResult) -> String {
  result.to_table_print(TableOptions::default())
}

fn grounding_label(grounding: &Grounding) -> &'static str {
  match grounding {
    Grounding::Strong => "strong",
  }
}

#[derive(TableRow)]
struct LibraryAppRow<'a> {
  appid: u32,
  name: &'a str,
  install_dir: &'a str,
  source: &'a str,
  #[table(display_with = "grounding_label")]
  grounding: Grounding,
}

impl CliOutput for LibraryQueryResult {
  fn to_json(&self) -> impl Serialize {
    build_library_ls_json_output(self)
  }

  fn to_table_print(&self, options: TableOptions<'_>) -> String {
    let rows = self
      .apps
      .iter()
      .map(|app| LibraryAppRow {
        appid: app.appid,
        name: &app.name,
        install_dir: &app.install_dir,
        source: &app.source,
        grounding: app.grounding,
      })
      .collect::<Vec<_>>();
    table::render(&rows, options.empty_message("(no matching installed Steam apps)"))
  }
}
