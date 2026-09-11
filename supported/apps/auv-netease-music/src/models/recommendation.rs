use serde::{Deserialize, Serialize};

use super::PlaylistRef;

/// A semantic reference to the singleton Daily Recommended entry.
///
/// The reference deliberately carries no coordinates or parser candidate id.
/// An operation that consumes it must locate the entry again in the current
/// Recommended view before delivering input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DailyRecommendedRef;

/// App-domain source from which a song collection can be opened and read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongSource {
  DailyRecommended(DailyRecommendedRef),
  Playlist(PlaylistRef),
}

impl From<DailyRecommendedRef> for SongSource {
  fn from(reference: DailyRecommendedRef) -> Self {
    Self::DailyRecommended(reference)
  }
}

impl From<PlaylistRef> for SongSource {
  fn from(reference: PlaylistRef) -> Self {
    Self::Playlist(reference)
  }
}

/// NetEase feature represented by one card in the Recommended view's leading
/// horizontal collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeaturedEntryKind {
  DailyRecommended,
  PrivateRadar,
  HeartbeatMode,
  PrivateRoaming,
  SimilarSongs,
  ElectronicDaily,
  Lia,
}

/// App-domain value projected from a card in the Recommended view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturedEntry {
  kind: FeaturedEntryKind,
  title: String,
}

impl FeaturedEntry {
  pub(crate) fn from_observed_title(kind: FeaturedEntryKind, title: impl Into<String>) -> Self {
    Self {
      kind,
      title: title.into(),
    }
  }

  pub fn kind(&self) -> FeaturedEntryKind {
    self.kind
  }

  pub fn title(&self) -> &str {
    &self.title
  }

  pub fn daily_recommended_ref(&self) -> Option<DailyRecommendedRef> {
    (self.kind == FeaturedEntryKind::DailyRecommended).then_some(DailyRecommendedRef)
  }
}
