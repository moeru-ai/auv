use super::*;
use crate::{PlaylistSidebarItem, SidebarSection, SidebarSectionKind};
use auv_view::Confidence;

#[test]
fn section_label_recovers_favorite_collection_from_ocr_prefix_noise() {
  assert_eq!(normalize_section_label("噪声我的收藏"), "我的收藏");
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
