use serde::{Deserialize, Serialize};

/// Playlist collection that owns a user-visible sidebar playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistSection {
  Created,
  Favorite,
}

/// Semantic playlist reference that can be resolved again in a later scan.
///
/// Parse-scoped item, candidate, and anchor ids deliberately do not belong to
/// this value. When duplicate labels exist inside one collection, resolving
/// the reference must report ambiguity instead of choosing one arbitrarily.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlaylistRef {
  section: PlaylistSection,
  label: String,
}

impl PlaylistRef {
  pub fn new(section: PlaylistSection, label: impl Into<String>) -> Result<Self, String> {
    let label = label.into().trim().to_string();
    if label.is_empty() {
      return Err("playlist reference label must not be empty".to_string());
    }
    Ok(Self { section, label })
  }

  pub fn section(&self) -> PlaylistSection {
    self.section
  }

  pub fn label(&self) -> &str {
    &self.label
  }
}
