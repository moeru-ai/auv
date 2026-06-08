use comfy_table::{Cell, Table, presets::NOTHING};
use serde::Serialize;

use crate::library::{
  Grounding, LibraryQuery, LibraryQueryResult, ResolvedLibraryScope, SteamInstalledApp,
};

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
  let mut table = Table::new();
  table.load_preset(NOTHING);
  table.set_header(["APPID", "NAME", "INSTALL DIR", "SOURCE", "GROUNDING"]);

  if result.apps.is_empty() {
    let mut summary = render_table(table);
    summary.push_str("\n(no matching installed Steam apps)");
    return summary;
  }

  for app in &result.apps {
    table.add_row([
      Cell::new(app.appid),
      Cell::new(&app.name),
      Cell::new(&app.install_dir),
      Cell::new(&app.source),
      Cell::new(grounding_label(app.grounding)),
    ]);
  }

  render_table(table)
}

fn grounding_label(grounding: Grounding) -> &'static str {
  match grounding {
    Grounding::Strong => "strong",
  }
}

fn render_table(table: Table) -> String {
  table
    .to_string()
    .lines()
    .map(str::trim)
    .collect::<Vec<_>>()
    .join("\n")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::library::{
    Grounding, LibraryQuery, LibraryQueryResult, LibrarySource, LibraryStatus,
    ResolvedLibraryScope, SteamInstalledApp,
  };

  fn result() -> LibraryQueryResult {
    LibraryQueryResult {
      query: LibraryQuery {
        name: Some("Balatro".to_string()),
        status: LibraryStatus::Installed,
        source: LibrarySource::Auto,
      },
      resolved_scope: ResolvedLibraryScope {
        status: LibraryStatus::Installed,
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      },
      apps: vec![SteamInstalledApp {
        appid: 2379780,
        name: "Balatro".to_string(),
        install_dir: "Balatro".to_string(),
        library_path: "/tmp/Steam".to_string(),
        manifest_path: "/tmp/Steam/steamapps/appmanifest_2379780.acf".to_string(),
        install_state: "installed".to_string(),
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      }],
      diagnostics: Vec::new(),
    }
  }

  #[test]
  fn json_output_keeps_library_ls_envelope() {
    let result = result();
    let output = build_library_ls_json_output(&result);
    let json = serde_json::to_value(output).expect("output should serialize");

    assert_eq!(json["command"], "library.ls");
    assert_eq!(json["query"]["name"], "Balatro");
    assert_eq!(json["resolved_scope"]["source"], "local_appmanifest");
    assert_eq!(json["apps"][0]["appid"], 2379780);
  }

  #[test]
  fn summary_output_renders_deterministic_table() {
    let summary = render_library_summary(&result());

    assert_eq!(
      summary,
      "APPID    NAME     INSTALL DIR  SOURCE             GROUNDING\n2379780  Balatro  Balatro      local_appmanifest  strong"
    );
  }

  #[test]
  fn summary_output_expands_columns_for_long_values() {
    let mut result = result();
    result.apps[0].name = "A Very Long Steam Game".to_string();
    result.apps[0].install_dir = "A Very Long Steam Game".to_string();

    let summary = render_library_summary(&result);

    assert_eq!(
      summary,
      concat!(
        "APPID    NAME                    INSTALL DIR             SOURCE             GROUNDING\n",
        "2379780  A Very Long Steam Game  A Very Long Steam Game  local_appmanifest  strong",
      )
    );
  }

  #[test]
  fn summary_output_reports_empty_matches() {
    let mut result = result();
    result.apps.clear();

    let summary = render_library_summary(&result);

    assert_eq!(
      summary,
      "APPID  NAME  INSTALL DIR  SOURCE  GROUNDING\n(no matching installed Steam apps)"
    );
  }
}
