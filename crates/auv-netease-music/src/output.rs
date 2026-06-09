use serde::Serialize;

use crate::views::sidebar::SidebarView;
use crate::{PlaylistSidebarProjection, PlaylistSidebarScan, SidebarSectionKind};

/// One playlist item surfaced by the listing or keyword filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MatchRef {
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
  pub label: String,
  pub candidate_id: Option<String>,
  pub anchor_id: Option<String>,
}

/// Agent-facing JSON output for the `playlist` command. Embeds the raw
/// scan artifact (which carries `schema_version` and `ScrollBoundarySummary`)
/// so an agent can distinguish "not found" from "scan not exhaustive".
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonOutput<'a> {
  pub command: &'static str,
  pub query: Option<String>,
  pub item_count: usize,
  pub match_count: usize,
  pub matches: Vec<MatchRef>,
  pub scan: &'a PlaylistSidebarScan,
}

/// Build the agent-facing JSON output without performing any live scan work.
pub fn build_playlist_json_output<'a>(
  scan: &'a PlaylistSidebarScan,
  keyword: Option<&str>,
) -> PlaylistJsonOutput<'a> {
  let sidebar = SidebarView::from_projection(scan.projection().clone());
  let item_count = collect_matches_from_sidebar(&sidebar, None).len();
  let matches = collect_matches_from_sidebar(&sidebar, keyword);
  PlaylistJsonOutput {
    command: "playlist",
    query: keyword.map(str::to_string),
    item_count,
    match_count: matches.len(),
    matches,
    scan,
  }
}

/// Collect items whose normalized label contains the normalized keyword.
/// `keyword == None` returns every item (full listing).
pub fn collect_matches(
  projection: &PlaylistSidebarProjection,
  keyword: Option<&str>,
) -> Vec<MatchRef> {
  let sidebar = SidebarView::from_projection(projection.clone());
  collect_matches_from_sidebar(&sidebar, keyword)
}

fn collect_matches_from_sidebar(sidebar: &SidebarView, keyword: Option<&str>) -> Vec<MatchRef> {
  sidebar
    .playlists(keyword)
    .into_iter()
    .map(|playlist| MatchRef {
      section_id: playlist.section.id.clone(),
      section_kind: playlist.section.kind,
      item_id: playlist.item.id.clone(),
      label: playlist.item.label.clone(),
      candidate_id: playlist.item.candidate_id.clone(),
      anchor_id: playlist.item.anchor_id.clone(),
    })
    .collect()
}

/// Agent-facing now-playing JSON for the netease CLI. Deliberately a subset of
/// `auv_media_macos`'s output: the like/favorite fields are omitted because
/// NetEase never reports them, so they would always be null here.
#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NowPlayingOutput {
  pub schema_version: &'static str,
  pub present: bool,
  pub is_playing: bool,
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  pub content_item_id: Option<String>,
}

#[cfg(target_os = "macos")]
pub fn build_now_playing_output(state: &auv_media_macos::NowPlayingState) -> NowPlayingOutput {
  NowPlayingOutput {
    schema_version: "now-playing-v0",
    present: state.present,
    is_playing: state.is_playing,
    source_bundle_id: state.source_bundle_id.clone(),
    title: state.title.clone(),
    artist: state.artist.clone(),
    album: state.album.clone(),
    duration_seconds: state.duration_seconds,
    elapsed_seconds: state.elapsed_seconds,
    playback_rate: state.playback_rate,
    content_item_id: state.content_item_id.clone(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{Confidence, PlaylistSidebarItem, SidebarSection};

  fn projection() -> PlaylistSidebarProjection {
    PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "i1".to_string(),
            label: "Daily Mix".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: Some("obs1.candidate.daily".to_string()),
            anchor_id: Some("a1".to_string()),
          },
          PlaylistSidebarItem {
            id: "i2".to_string(),
            label: "Workout".to_string(),
            section_hint: None,
            confidence: Confidence::Low,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    }
  }

  fn projection_with_nav_item() -> PlaylistSidebarProjection {
    PlaylistSidebarProjection {
      sections: vec![
        SidebarSection {
          id: "nav".to_string(),
          kind: SidebarSectionKind::FeatureNav,
          label: Some("推荐".to_string()),
          items: vec![PlaylistSidebarItem {
            id: "nav-daily".to_string(),
            label: "Daily".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: Some("nav-anchor".to_string()),
          }],
        },
        SidebarSection {
          id: "playlist".to_string(),
          kind: SidebarSectionKind::FavoritePlaylists,
          label: Some("收藏的歌单".to_string()),
          items: vec![PlaylistSidebarItem {
            id: "playlist-daily".to_string(),
            label: "Daily".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: Some("playlist-anchor".to_string()),
          }],
        },
      ],
    }
  }

  #[test]
  fn no_keyword_returns_all_items() {
    let matches = collect_matches(&projection(), None);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].label, "Daily Mix");
    assert_eq!(matches[0].anchor_id.as_deref(), Some("a1"));
  }

  #[test]
  fn keyword_filters_case_and_whitespace_insensitively() {
    let matches = collect_matches(&projection(), Some("daily"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].item_id, "i1");
    assert_eq!(matches[0].section_kind, SidebarSectionKind::MyPlaylists);
  }

  #[test]
  fn keyword_without_match_returns_empty() {
    let matches = collect_matches(&projection(), Some("zzz"));
    assert!(matches.is_empty());
  }

  #[test]
  fn collect_matches_uses_sidebar_playlist_sections_only() {
    let matches = collect_matches(&projection_with_nav_item(), Some("daily"));

    assert_eq!(matches.len(), 1);
    assert_eq!(
      matches[0].section_kind,
      SidebarSectionKind::FavoritePlaylists
    );
    assert_eq!(matches[0].item_id, "playlist-daily");
    assert_eq!(matches[0].anchor_id.as_deref(), Some("playlist-anchor"));
  }

  #[test]
  fn build_playlist_json_output_counts_all_items_and_matches() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("daily"));

    assert_eq!(output.command, "playlist");
    assert_eq!(output.query.as_deref(), Some("daily"));
    assert_eq!(output.item_count, 2);
    assert_eq!(output.match_count, 1);
    assert_eq!(output.matches[0].item_id, "i1");
    assert_eq!(
      output.matches[0].candidate_id.as_deref(),
      Some("obs1.candidate.daily")
    );
    assert!(std::ptr::eq(output.scan, &scan));
  }
}
