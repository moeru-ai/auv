use serde::Serialize;

use crate::library::{LibraryQuery, LibraryQueryResult, ResolvedLibraryScope, SteamInstalledApp};

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
  format!("Steam installed library apps: {}", result.apps.len())
}
