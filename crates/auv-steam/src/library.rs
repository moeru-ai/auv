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

#[derive(Clone, Debug, Error)]
pub enum SteamError {
  #[error("Steam could not be located")]
  NotFound,
}

impl LibrarySource {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Local => "local",
      Self::Web => "web",
      Self::Ui => "ui",
    }
  }
}

impl LibraryDiagnostic {
  fn error(code: impl Into<String>, message: impl Into<String>, path: Option<String>) -> Self {
    Self {
      severity: LibraryDiagnosticSeverity::Error,
      code: code.into(),
      message: message.into(),
      path,
    }
  }

  #[allow(dead_code)]
  fn warning(code: impl Into<String>, message: impl Into<String>, path: Option<String>) -> Self {
    Self {
      severity: LibraryDiagnosticSeverity::Warning,
      code: code.into(),
      message: message.into(),
      path,
    }
  }
}

pub fn resolve_scope(query: &LibraryQuery) -> Result<ResolvedLibraryScope, LibraryDiagnostic> {
  match query.status {
    LibraryStatus::Installed => {}
    LibraryStatus::Owned => {
      // TODO(steam-owned-library-v1): owned-but-not-installed discovery is
      // deferred until an owner-approved slice chooses Steam Web/API,
      // authenticated local cache, or UI observation as the evidence source.
      return Err(LibraryDiagnostic::error(
        "unsupported_library_status",
        "owned Steam library status is not implemented in v0; use --status installed",
        None,
      ));
    }
    LibraryStatus::All => {
      // TODO(steam-library-all-v1): all-library merging is deferred until
      // installed and owned sources define precedence, duplicate handling, and
      // grounding reporting for shared appids.
      return Err(LibraryDiagnostic::error(
        "unsupported_library_scope",
        "all Steam library status is not implemented in v0; use --status installed",
        None,
      ));
    }
  }

  match query.source {
    LibrarySource::Auto | LibrarySource::Local => Ok(ResolvedLibraryScope {
      status: LibraryStatus::Installed,
      source: "local_appmanifest".to_string(),
      grounding: Grounding::Strong,
    }),
    LibrarySource::Web => {
      // TODO(steam-owned-library-v1): web-backed ownership is deferred until an
      // owner-approved slice defines credentials, profile visibility, and
      // account-library evidence.
      Err(LibraryDiagnostic::error(
        "unsupported_library_source",
        "Steam web library source is not implemented in v0; use --source local",
        None,
      ))
    }
    LibrarySource::Ui => {
      // TODO(steam-ui-library-source-v1): Steam UI observation is deferred
      // because v0 has a stronger local manifest source for installed games and
      // no accepted UI parser contract for owned-library state.
      Err(LibraryDiagnostic::error(
        "unsupported_library_source",
        "Steam ui library source is not implemented in v0; use --source local",
        None,
      ))
    }
  }
}

pub fn query_installed_apps(
  query: LibraryQuery,
  apps: Vec<SteamInstalledApp>,
  diagnostics: Vec<LibraryDiagnostic>,
) -> Result<LibraryQueryResult, LibraryDiagnostic> {
  let resolved_scope = resolve_scope(&query)?;
  let normalized_name = query.name.as_deref().map(normalize_match_text);
  let apps = apps
    .into_iter()
    .filter(|app| {
      normalized_name
        .as_ref()
        .is_none_or(|needle| normalize_match_text(&app.name).contains(needle))
    })
    .collect();

  Ok(LibraryQueryResult {
    query,
    resolved_scope,
    apps,
    diagnostics,
  })
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstalledAppRead {
  pub apps: Vec<SteamInstalledApp>,
  pub diagnostics: Vec<LibraryDiagnostic>,
}

pub trait InstalledAppSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError>;
}

pub struct SteamLibraryStore<S> {
  source: S,
}

impl<S> SteamLibraryStore<S>
where
  S: InstalledAppSource,
{
  pub fn new(source: S) -> Self {
    Self { source }
  }

  pub fn query(&self, query: LibraryQuery) -> Result<LibraryQueryResult, LibraryDiagnostic> {
    resolve_scope(&query)?;
    let read = self
      .source
      .installed_apps()
      .map_err(|error| LibraryDiagnostic::error("steam_not_found", error.to_string(), None))?;
    query_installed_apps(query, read.apps, read.diagnostics)
  }
}

pub struct SteamlocateSource {
  steam_dir: steamlocate::SteamDir,
}

impl SteamlocateSource {
  pub fn locate() -> Result<Self, SteamError> {
    let steam_dir = steamlocate::SteamDir::locate().map_err(|_| SteamError::NotFound)?;
    Ok(Self { steam_dir })
  }
}

impl InstalledAppSource for SteamlocateSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
    // NOTICE(steam-library-manifest-parser): Steam library discovery is
    // delegated to `steamlocate`, which already handles platform-specific Steam
    // directory lookup and parses Steam KeyValues/VDF app manifests through
    // `keyvalues-serde`. Keep AUV's layer focused on domain resolution,
    // diagnostics, launch evidence, and verification instead of carrying a
    // local VDF parser.
    let mut apps = Vec::new();
    let mut diagnostics = Vec::new();

    for library in self
      .steam_dir
      .libraries()
      .map_err(|_| SteamError::NotFound)?
    {
      let library = match library {
        Ok(library) => library,
        Err(error) => {
          diagnostics.push(LibraryDiagnostic::warning(
            "library_folder_unreadable",
            format!("failed to read Steam library folder: {error}"),
            None,
          ));
          continue;
        }
      };

      for app in library.apps() {
        let app = match app {
          Ok(app) => app,
          Err(error) => {
            diagnostics.push(LibraryDiagnostic::warning(
              "manifest_parse_failed",
              format!("failed to parse Steam app manifest: {error}"),
              Some(library.path().display().to_string()),
            ));
            continue;
          }
        };
        let manifest_path = library
          .path()
          .join("steamapps")
          .join(format!("appmanifest_{}.acf", app.app_id));
        apps.push(SteamInstalledApp {
          appid: app.app_id,
          name: app
            .name
            .unwrap_or_else(|| format!("Steam App {}", app.app_id)),
          install_dir: app.install_dir,
          library_path: library.path().display().to_string(),
          manifest_path: manifest_path.display().to_string(),
          install_state: "installed".to_string(),
          source: "local_appmanifest".to_string(),
          grounding: Grounding::Strong,
        });
      }
    }

    Ok(InstalledAppRead { apps, diagnostics })
  }
}

fn normalize_match_text(value: &str) -> String {
  value
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Default)]
  struct FakeInstalledAppSource {
    apps: Vec<SteamInstalledApp>,
    diagnostics: Vec<LibraryDiagnostic>,
    error: Option<SteamError>,
  }

  impl InstalledAppSource for FakeInstalledAppSource {
    fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
      if let Some(error) = &self.error {
        return Err(error.clone());
      }
      Ok(InstalledAppRead {
        apps: self.apps.clone(),
        diagnostics: self.diagnostics.clone(),
      })
    }
  }

  fn fake_app(appid: u32, name: &str) -> SteamInstalledApp {
    SteamInstalledApp {
      appid,
      name: name.to_string(),
      install_dir: name.to_string(),
      library_path: "/tmp/Steam".to_string(),
      manifest_path: format!("/tmp/Steam/steamapps/appmanifest_{appid}.acf"),
      install_state: "installed".to_string(),
      source: "local_appmanifest".to_string(),
      grounding: Grounding::Strong,
    }
  }

  #[test]
  fn store_reads_source_and_applies_query() {
    let source = FakeInstalledAppSource {
      apps: vec![
        fake_app(2379780, "Balatro"),
        fake_app(220200, "Kerbal Space Program"),
      ],
      diagnostics: Vec::new(),
      error: None,
    };
    let store = SteamLibraryStore::new(source);
    let query = LibraryQuery {
      name: Some("lat".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };

    let result = store.query(query).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].appid, 2379780);
  }

  #[test]
  fn store_rejects_unsupported_scope_before_reading_source() {
    let source = FakeInstalledAppSource {
      apps: vec![fake_app(2379780, "Balatro")],
      diagnostics: Vec::new(),
      error: Some(SteamError::NotFound),
    };
    let store = SteamLibraryStore::new(source);
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Owned,
      source: LibrarySource::Auto,
    };

    let diagnostic = store.query(query).expect_err("owned is unsupported");

    assert_eq!(diagnostic.code, "unsupported_library_status");
  }

  #[test]
  fn store_maps_source_failure_to_steam_not_found() {
    let source = FakeInstalledAppSource {
      apps: Vec::new(),
      diagnostics: Vec::new(),
      error: Some(SteamError::NotFound),
    };
    let store = SteamLibraryStore::new(source);
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };

    let diagnostic = store
      .query(query)
      .expect_err("source failure should map to diagnostic");

    assert_eq!(diagnostic.code, "steam_not_found");
    assert_eq!(diagnostic.severity, LibraryDiagnosticSeverity::Error);
  }

  #[test]
  fn installed_auto_resolves_to_local_appmanifest() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };

    let scope = resolve_scope(&query).expect("installed auto should resolve");

    assert_eq!(
      scope,
      ResolvedLibraryScope {
        status: LibraryStatus::Installed,
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      }
    );
  }

  #[test]
  fn installed_local_resolves_to_local_appmanifest() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };

    let scope = resolve_scope(&query).expect("installed local should resolve");

    assert_eq!(scope.source, "local_appmanifest");
    assert_eq!(scope.grounding, Grounding::Strong);
  }

  #[test]
  fn owned_status_is_explicitly_unsupported_in_v0() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Owned,
      source: LibrarySource::Auto,
    };

    let diagnostic = resolve_scope(&query).expect_err("owned should be deferred");

    assert_eq!(diagnostic.code, "unsupported_library_status");
    assert_eq!(diagnostic.severity, LibraryDiagnosticSeverity::Error);
    assert!(diagnostic.message.contains("owned"));
  }

  #[test]
  fn all_status_is_explicitly_unsupported_in_v0() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::All,
      source: LibrarySource::Auto,
    };

    let diagnostic = resolve_scope(&query).expect_err("all should be deferred");

    assert_eq!(diagnostic.code, "unsupported_library_scope");
    assert!(diagnostic.message.contains("all"));
  }

  #[test]
  fn web_and_ui_sources_are_explicitly_unsupported_in_v0() {
    for source in [LibrarySource::Web, LibrarySource::Ui] {
      let query = LibraryQuery {
        name: None,
        status: LibraryStatus::Installed,
        source,
      };

      let diagnostic = resolve_scope(&query).expect_err("web/ui should be deferred");

      assert_eq!(diagnostic.code, "unsupported_library_source");
      assert!(diagnostic.message.contains(source.as_str()));
    }
  }

  #[test]
  fn query_apps_without_name_returns_all_apps() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![
      fake_app(2379780, "Balatro"),
      fake_app(220200, "Kerbal Space Program"),
    ];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 2);
    assert_eq!(result.resolved_scope.source, "local_appmanifest");
    assert!(result.diagnostics.is_empty());
  }

  #[test]
  fn query_apps_filters_by_case_insensitive_contains_match() {
    let query = LibraryQuery {
      name: Some("LAT".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };
    let apps = vec![
      fake_app(2379780, "Balatro"),
      fake_app(220200, "Kerbal Space Program"),
    ];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].appid, 2379780);
  }

  #[test]
  fn query_apps_collapses_repeated_query_whitespace() {
    let query = LibraryQuery {
      name: Some("space   program".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![
      fake_app(2379780, "Balatro"),
      fake_app(220200, "Kerbal Space Program"),
    ];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].name, "Kerbal Space Program");
  }

  #[test]
  fn query_apps_no_match_is_successful_empty_result() {
    let query = LibraryQuery {
      name: Some("missing".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![fake_app(2379780, "Balatro")];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert!(result.apps.is_empty());
    assert!(result.diagnostics.is_empty());
  }

  #[test]
  fn query_apps_preserves_warning_diagnostics() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let diagnostics = vec![LibraryDiagnostic::warning(
      "manifest_parse_failed",
      "failed to parse appmanifest_1.acf",
      Some("/tmp/Steam/steamapps/appmanifest_1.acf".to_string()),
    )];

    let result = query_installed_apps(query, vec![fake_app(2379780, "Balatro")], diagnostics)
      .expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.diagnostics[0].code, "manifest_parse_failed");
  }
}
