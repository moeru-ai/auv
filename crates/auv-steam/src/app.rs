use crate::library::{LibraryDiagnostic, LibraryQuery, LibraryQueryResult, SteamError, SteamLibraryStore, SteamlocateSource};

pub struct Steam {
  library: SteamLibraryStore<SteamlocateSource>,
}

impl Steam {
  pub fn locate() -> Result<Self, SteamError> {
    Ok(Self {
      library: SteamLibraryStore::new(SteamlocateSource::locate()?),
    })
  }

  pub fn library_apps(&self, query: LibraryQuery) -> Result<LibraryQueryResult, LibraryDiagnostic> {
    self.library.query(query)
  }
}

pub fn query_local_library_apps(query: LibraryQuery) -> Result<LibraryQueryResult, LibraryDiagnostic> {
  let steam = Steam::locate().map_err(|error| LibraryDiagnostic {
    severity: crate::library::LibraryDiagnosticSeverity::Error,
    code: "steam_not_found".to_string(),
    message: error.to_string(),
    path: None,
  })?;
  steam.library_apps(query)
}
