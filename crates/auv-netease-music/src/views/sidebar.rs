use auv_view::{Confidence, normalize_identity};
use serde::{Deserialize, Serialize};

use crate::views::query_match::{
  PlaylistLabelMatchTier, PlaylistQueryMatchMode, PlaylistQueryResolution, playlist_label_match_tier, resolve_playlist_query_from_labels,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSidebarProjection {
  pub sections: Vec<SidebarSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarSection {
  pub id: String,
  pub kind: SidebarSectionKind,
  pub label: Option<String>,
  pub items: Vec<PlaylistSidebarItem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarSectionKind {
  FeatureNav,
  LibraryNav,
  PlaylistNav,
  MyPlaylists,
  FavoritePlaylists,
  #[default]
  Unknown,
}

impl SidebarSectionKind {
  pub(crate) fn from_label(label: &str) -> Self {
    let label = normalize_section_label(label);
    if label.contains("创建的歌单") || label.contains("我的歌单") {
      Self::MyPlaylists
    } else if label.contains("收藏的歌单") {
      Self::FavoritePlaylists
    } else if label == "我的收藏" {
      Self::LibraryNav
    } else if matches!(label.as_str(), "推荐" | "音乐服务") {
      Self::FeatureNav
    } else {
      Self::Unknown
    }
  }

  pub(crate) fn is_known(self) -> bool {
    self != Self::Unknown
  }

  pub(crate) fn is_playlist_collection(self) -> bool {
    matches!(self, Self::MyPlaylists | Self::FavoritePlaylists)
  }

  pub(crate) fn domain_kind(self) -> &'static str {
    match self {
      Self::FeatureNav => "netease.feature_nav",
      Self::LibraryNav => "netease.library_nav",
      Self::PlaylistNav => "netease.playlist_nav",
      Self::MyPlaylists => "netease.my_playlists",
      Self::FavoritePlaylists => "netease.favorite_playlists",
      Self::Unknown => "netease.sidebar_section",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSidebarItem {
  pub id: String,
  pub label: String,
  pub section_hint: Option<SidebarSectionKind>,
  pub confidence: Confidence,
  pub candidate_id: Option<String>,
  pub anchor_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarState {
  /// A reconstructed NetEase sidebar section is available.
  Present,
  /// The caller knows the sidebar is not available in this view.
  Absent,
  /// Reconstruction ran, but did not identify a known sidebar section.
  Unknown,
}

/// Read-only sidebar facade backed by a reconstructed playlist sidebar projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarView {
  state: SidebarState,
  projection: Option<PlaylistSidebarProjection>,
  playlist_lookup: Vec<PlaylistLookupEntry>,
}

impl SidebarView {
  /// Build a view for a caller-proven absent sidebar.
  pub fn absent() -> Self {
    Self {
      state: SidebarState::Absent,
      projection: None,
      playlist_lookup: Vec::new(),
    }
  }

  /// Build a view when the sidebar was not reconstructed by this observation.
  pub fn unknown() -> Self {
    Self {
      state: SidebarState::Unknown,
      projection: None,
      playlist_lookup: Vec::new(),
    }
  }

  /// Build a sidebar view from reconstructed sidebar data.
  pub fn from_projection(projection: PlaylistSidebarProjection) -> Self {
    let playlist_lookup = playlist_lookup(&projection);
    let state = if projection.sections.iter().any(is_known_sidebar_section) || !playlist_lookup.is_empty() {
      SidebarState::Present
    } else {
      SidebarState::Unknown
    };

    Self {
      state,
      playlist_lookup,
      projection: Some(projection),
    }
  }

  /// Return the sidebar availability state derived for this view.
  pub fn state(&self) -> SidebarState {
    self.state
  }

  /// Whether this view has a known reconstructed NetEase sidebar section.
  pub fn exists(&self) -> bool {
    self.state == SidebarState::Present
  }

  /// Find the first created/favorite playlist that uniquely matches `keyword`
  /// using exact-first query resolution.
  pub fn find_playlist(&self, keyword: &str) -> Option<&PlaylistSidebarItem> {
    let matches = self.playlists(Some(keyword));
    if matches.len() == 1 {
      Some(matches[0].item)
    } else {
      None
    }
  }

  /// Return created/favorite playlists that match `keyword` with exact-first
  /// resolution (unique exact, else unique contains, else none or all ambiguous).
  ///
  /// `keyword == None` returns every playlist item in playlist collection sections.
  pub fn playlists(&self, keyword: Option<&str>) -> Vec<PlaylistRef<'_>> {
    let Some(projection) = self.projection.as_ref() else {
      return Vec::new();
    };

    let all_refs = self.collect_playlist_refs(projection);
    let Some(keyword) = keyword else {
      return all_refs;
    };

    let (resolution, normalized_query) = Self::resolve_query(&all_refs, keyword);

    all_refs
      .into_iter()
      .filter(|playlist| playlist_label_matches_resolution(&normalize_identity(&playlist.item.label), &normalized_query, resolution))
      .collect()
  }

  /// Report the exact-first resolution for `keyword` without filtering the
  /// underlying items, so a caller can tell "matched exactly one" apart from
  /// "several labels contain the query" instead of inferring it from the
  /// match count alone.
  pub(crate) fn playlist_query_resolution(&self, keyword: &str) -> PlaylistQueryResolution {
    let Some(projection) = self.projection.as_ref() else {
      return PlaylistQueryResolution::NotFound;
    };
    let all_refs = self.collect_playlist_refs(projection);
    Self::resolve_query(&all_refs, keyword).0
  }

  fn resolve_query(refs: &[PlaylistRef<'_>], keyword: &str) -> (PlaylistQueryResolution, String) {
    let labels: Vec<&str> = refs.iter().map(|playlist| playlist.item.label.as_str()).collect();
    (resolve_playlist_query_from_labels(&labels, keyword), normalize_identity(keyword))
  }

  fn collect_playlist_refs<'a>(&self, projection: &'a PlaylistSidebarProjection) -> Vec<PlaylistRef<'a>> {
    self
      .playlist_lookup
      .iter()
      .filter_map(|entry| {
        let section = projection.sections.get(entry.section_index)?;
        let item = section.items.get(entry.item_index)?;
        Some(PlaylistRef { section, item })
      })
      .collect()
  }
}

fn playlist_label_matches_resolution(normalized_label: &str, normalized_query: &str, resolution: PlaylistQueryResolution) -> bool {
  let tier = playlist_label_match_tier(normalized_label, normalized_query);
  match resolution {
    PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Exact,
    } => tier == PlaylistLabelMatchTier::Exact,
    PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Contains,
    } => tier == PlaylistLabelMatchTier::Contains,
    PlaylistQueryResolution::Ambiguous => tier == PlaylistLabelMatchTier::Exact || tier == PlaylistLabelMatchTier::Contains,
    PlaylistQueryResolution::NotFound => false,
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaylistLookupEntry {
  section_index: usize,
  item_index: usize,
  normalized_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaylistRef<'a> {
  pub section: &'a SidebarSection,
  pub item: &'a PlaylistSidebarItem,
}

fn playlist_lookup(projection: &PlaylistSidebarProjection) -> Vec<PlaylistLookupEntry> {
  let has_playlist_collection = projection.sections.iter().any(|section| is_playlist_collection(section.kind));

  projection
    .sections
    .iter()
    .enumerate()
    .filter(|(_, section)| is_playlist_collection(section.kind) || (!has_playlist_collection && section.kind == SidebarSectionKind::Unknown))
    .flat_map(|(section_index, section)| {
      section.items.iter().enumerate().map(move |(item_index, item)| PlaylistLookupEntry {
        section_index,
        item_index,
        normalized_label: normalize_identity(&item.label),
      })
    })
    .collect()
}

fn is_known_sidebar_section(section: &SidebarSection) -> bool {
  section.kind != SidebarSectionKind::Unknown
}

fn is_playlist_collection(kind: SidebarSectionKind) -> bool {
  matches!(kind, SidebarSectionKind::MyPlaylists | SidebarSectionKind::FavoritePlaylists)
}

fn normalize_section_label(label: &str) -> String {
  let label = label
    .trim()
    .trim_end_matches(|char: char| char.is_ascii_digit() || char.is_whitespace())
    .trim_end_matches(|char| matches!(char, '⌃' | '⌄' | '˄' | '˅' | '^' | '∨' | '⌵' | '入'))
    .trim_end_matches(|char: char| char.is_ascii_digit() || char.is_whitespace())
    .trim()
    .to_string();

  // NetEase's disclosure icon can be recognized as arbitrary prefix noise;
  // the stable section suffix is sufficient to recover this app-local label.
  if label.ends_with("我的收藏") && label != "我的收藏" {
    return "我的收藏".to_string();
  }
  label
}

#[cfg(test)]
#[path = "sidebar_test.rs"]
mod tests;
