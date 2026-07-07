use auv_steam::library::{
  Grounding, InstalledAppRead, InstalledAppSource, LibraryDiagnostic, LibraryDiagnosticSeverity, LibraryQuery, LibrarySource, LibraryStatus,
  SteamError, SteamInstalledApp, SteamLibraryStore,
};

#[derive(Clone, Debug)]
struct FakeInstalledAppSource {
  read: InstalledAppRead,
}

impl InstalledAppSource for FakeInstalledAppSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
    Ok(self.read.clone())
  }
}

fn fake_app(appid: u32, name: &str) -> SteamInstalledApp {
  SteamInstalledApp {
    appid,
    name: name.to_string(),
    install_dir: name.to_string(),
    library_path: "/fixture/SteamLibrary".to_string(),
    manifest_path: format!("/fixture/SteamLibrary/steamapps/appmanifest_{appid}.acf"),
    install_state: "installed".to_string(),
    source: "local_appmanifest".to_string(),
    grounding: Grounding::Strong,
  }
}

fn installed_name_query(name: &str) -> LibraryQuery {
  LibraryQuery {
    name: Some(name.to_string()),
    status: LibraryStatus::Installed,
    source: LibrarySource::Local,
  }
}

#[test]
fn fixture_source_query_filters_installed_apps_by_normalized_contains_name() {
  let source = FakeInstalledAppSource {
    read: InstalledAppRead {
      apps: vec![
        fake_app(2379780, "Balatro"),
        fake_app(220200, "Kerbal Space Program"),
      ],
      diagnostics: Vec::new(),
    },
  };
  let store = SteamLibraryStore::new(source);

  let result = store.query(installed_name_query("LAT")).expect("fixture-backed query should succeed");

  assert_eq!(result.apps.len(), 1);
  assert_eq!(result.apps[0].appid, 2379780);
  assert_eq!(result.apps[0].name, "Balatro");
}

#[test]
fn fixture_source_query_preserves_warning_diagnostics() {
  let warning = LibraryDiagnostic {
    severity: LibraryDiagnosticSeverity::Warning,
    code: "manifest_parse_failed".to_string(),
    message: "failed to parse appmanifest_1.acf".to_string(),
    path: Some("/fixture/SteamLibrary/steamapps/appmanifest_1.acf".to_string()),
  };
  let source = FakeInstalledAppSource {
    read: InstalledAppRead {
      apps: vec![fake_app(2379780, "Balatro")],
      diagnostics: vec![warning.clone()],
    },
  };
  let store = SteamLibraryStore::new(source);

  let result = store.query(installed_name_query("lat")).expect("fixture-backed query should succeed");

  assert_eq!(result.apps.len(), 1);
  assert_eq!(result.diagnostics, vec![warning]);
}
