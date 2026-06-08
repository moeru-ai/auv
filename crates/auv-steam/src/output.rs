use std::fmt::Write as _;

use serde::Serialize;

use crate::library::{
  Grounding, LibraryQuery, LibraryQueryResult, ResolvedLibraryScope, SteamInstalledApp,
};

const LIBRARY_SUMMARY_HEADER: &str = "APPID    NAME     INSTALL DIR  SOURCE             GROUNDING";

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
  let mut summary = LIBRARY_SUMMARY_HEADER.to_string();

  if result.apps.is_empty() {
    summary.push_str("\n(no matching installed Steam apps)");
    return summary;
  }

  for app in &result.apps {
    write!(
      summary,
      "\n{:<9}{:<9}{:<13}{:<19}{}",
      app.appid,
      app.name,
      app.install_dir,
      app.source,
      grounding_label(app.grounding)
    )
    .expect("writing to String should not fail");
  }

  summary
}

fn grounding_label(grounding: Grounding) -> &'static str {
  match grounding {
    Grounding::Strong => "strong",
  }
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
  fn summary_output_reports_empty_matches() {
    let mut result = result();
    result.apps.clear();

    let summary = render_library_summary(&result);

    assert_eq!(
      summary,
      "APPID    NAME     INSTALL DIR  SOURCE             GROUNDING\n(no matching installed Steam apps)"
    );
  }
}
