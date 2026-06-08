use crate::library::{LibraryQuery, LibraryQueryResult, SteamError};

pub struct Steam;

impl Steam {
  pub fn locate() -> Result<Self, SteamError> {
    Ok(Self)
  }

  pub fn library_apps(&self, _query: LibraryQuery) -> Result<LibraryQueryResult, SteamError> {
    Err(SteamError::NotFound)
  }
}
