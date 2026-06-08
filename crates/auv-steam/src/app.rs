use crate::library::{
  LibraryDiagnostic, LibraryQuery, LibraryQueryResult, SteamError, SteamLibraryStore,
  SteamlocateSource,
};

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
