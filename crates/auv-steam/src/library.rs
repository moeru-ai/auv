use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStatus {
  #[default]
  Installed,
  Owned,
  All,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySource {
  #[default]
  Auto,
  Local,
  Web,
  Ui,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grounding {
  Strong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryDiagnosticSeverity {
  Warning,
  Error,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryQuery {
  pub name: Option<String>,
  pub status: LibraryStatus,
  pub source: LibrarySource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedLibraryScope {
  pub status: LibraryStatus,
  pub source: String,
  pub grounding: Grounding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SteamInstalledApp {
  pub appid: u32,
  pub name: String,
  pub install_dir: String,
  pub library_path: String,
  pub manifest_path: String,
  pub install_state: String,
  pub source: String,
  pub grounding: Grounding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryDiagnostic {
  pub severity: LibraryDiagnosticSeverity,
  pub code: String,
  pub message: String,
  pub path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryQueryResult {
  pub query: LibraryQuery,
  pub resolved_scope: ResolvedLibraryScope,
  pub apps: Vec<SteamInstalledApp>,
  pub diagnostics: Vec<LibraryDiagnostic>,
}

#[derive(Debug, Error)]
pub enum SteamError {
  #[error("Steam could not be located")]
  NotFound,
}
