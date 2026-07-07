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

  strip_leading_icon_noise(label)
}

fn strip_leading_icon_noise(label: String) -> String {
  if label.ends_with("我的收藏") && label != "我的收藏" {
    return "我的收藏".to_string();
  }
  label
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{PlaylistSidebarItem, SidebarSection, SidebarSectionKind};
  use auv_view::Confidence;

  #[test]
  fn exists_when_projection_has_known_sidebar_playlist_section() {
    let view = SidebarView::from_projection(projection(vec![playlist_section(SidebarSectionKind::MyPlaylists, vec![])]));

    assert_eq!(view.state(), SidebarState::Present);
    assert!(view.exists());
  }

  #[test]
  fn find_playlist_reads_projection_and_returns_matching_item_anchor() {
    let view = SidebarView::from_projection(projection(vec![playlist_section(
      SidebarSectionKind::MyPlaylists,
      vec![
        playlist_item("daily-mix", "Daily Mix", Some("anchor-daily")),
        playlist_item("workout", "Workout", None),
      ],
    )]));

    let item = view.find_playlist("daily").expect("playlist match");

    assert_eq!(item.id, "daily-mix");
    assert_eq!(item.anchor_id.as_deref(), Some("anchor-daily"));
  }

  #[test]
  fn absent_sidebar_does_not_match_playlist() {
    let view = SidebarView::absent();

    assert_eq!(view.state(), SidebarState::Absent);
    assert!(!view.exists());
    assert!(view.find_playlist("daily").is_none());
  }

  #[test]
  fn unknown_sidebar_does_not_claim_absence() {
    let view = SidebarView::unknown();

    assert_eq!(view.state(), SidebarState::Unknown);
    assert!(!view.exists());
    assert!(view.find_playlist("daily").is_none());
  }

  #[test]
  fn unknown_section_items_are_playlist_fallback_when_header_is_not_visible() {
    let view = SidebarView::from_projection(projection(vec![SidebarSection {
      id: "section.unassigned".to_string(),
      kind: SidebarSectionKind::Unknown,
      label: None,
      items: vec![playlist_item(
        "future-garage",
        "我喜欢的风格 | Future Garage",
        Some("anchor-future"),
      )],
    }]));

    let item = view.find_playlist("future garage").expect("unassigned playlist row should match");

    assert_eq!(view.state(), SidebarState::Present);
    assert!(view.exists());
    assert_eq!(item.id, "future-garage");
    assert_eq!(item.anchor_id.as_deref(), Some("anchor-future"));
  }

  #[test]
  fn non_playlist_sections_do_not_satisfy_playlist_search() {
    let view = SidebarView::from_projection(projection(vec![SidebarSection {
      id: "feature-nav".to_string(),
      kind: SidebarSectionKind::FeatureNav,
      label: Some("推荐".to_string()),
      items: vec![playlist_item("daily-route", "Daily", Some("anchor-nav"))],
    }]));

    assert!(view.exists());
    assert!(view.find_playlist("daily").is_none());
  }

  #[test]
  fn playlists_exact_beats_contains_for_numeric_query() {
    let view = SidebarView::from_projection(projection(vec![playlist_section(
      SidebarSectionKind::MyPlaylists,
      vec![
        playlist_item("p43", "43", None),
        playlist_item("p39", "39", None),
        playlist_item("p3", "3", None),
      ],
    )]));

    let matches = view.playlists(Some("3"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].item.id, "p3");
  }

  #[test]
  fn playlists_contains_fallback_for_partial_label() {
    let view = SidebarView::from_projection(projection(vec![playlist_section(
      SidebarSectionKind::MyPlaylists,
      vec![playlist_item("human-machine", "人造器械", None)],
    )]));

    let matches = view.playlists(Some("人造"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].item.id, "human-machine");
  }

  #[test]
  fn playlists_returns_all_ambiguous_contains_matches() {
    let view = SidebarView::from_projection(projection(vec![playlist_section(
      SidebarSectionKind::MyPlaylists,
      vec![
        playlist_item("p43", "43", None),
        playlist_item("p13", "13", None),
      ],
    )]));

    let matches = view.playlists(Some("3"));
    assert_eq!(matches.len(), 2);
  }

  fn projection(sections: Vec<SidebarSection>) -> PlaylistSidebarProjection {
    PlaylistSidebarProjection { sections }
  }

  fn playlist_section(kind: SidebarSectionKind, items: Vec<PlaylistSidebarItem>) -> SidebarSection {
    SidebarSection {
      id: "playlist-section".to_string(),
      kind,
      label: Some("我的歌单".to_string()),
      items,
    }
  }

  fn playlist_item(id: &str, label: &str, anchor_id: Option<&str>) -> PlaylistSidebarItem {
    PlaylistSidebarItem {
      id: id.to_string(),
      label: label.to_string(),
      section_hint: None,
      confidence: Confidence::High,
      candidate_id: None,
      anchor_id: anchor_id.map(str::to_string),
    }
  }
}
