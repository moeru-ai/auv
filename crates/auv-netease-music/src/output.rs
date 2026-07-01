use serde::Serialize;

use crate::views::query_match::{PlaylistQueryMatchMode, PlaylistQueryResolution};
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

/// Agent-facing exact-first query resolution tier for `playlist ls --json`.
/// `match_count` alone cannot distinguish "one real hit" from "several
/// substring collisions" (e.g. query `"3"` against labels `"43"`, `"39"`,
/// `"13"`), so callers must read this field instead of inferring intent from
/// `match_count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryResolutionKind {
  UniqueExact,
  UniqueContains,
  Ambiguous,
  NotFound,
}

fn query_resolution_kind(resolution: PlaylistQueryResolution) -> QueryResolutionKind {
  match resolution {
    PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Exact,
    } => QueryResolutionKind::UniqueExact,
    PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Contains,
    } => QueryResolutionKind::UniqueContains,
    PlaylistQueryResolution::Ambiguous => QueryResolutionKind::Ambiguous,
    PlaylistQueryResolution::NotFound => QueryResolutionKind::NotFound,
  }
}

/// Agent-facing view-memory write outcome for `playlist ls --json` when gate is on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ViewMemoryWriteReport {
  pub written: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub skip_reason: Option<String>,
}

/// Maps gate + write result into JSON report; mirrors [`crate::cli::run_playlist`].
pub fn playlist_view_memory_report(
  gate_enabled: bool,
  write_result: Result<(), String>,
) -> Option<ViewMemoryWriteReport> {
  if !gate_enabled {
    return None;
  }
  Some(match write_result {
    Ok(()) => ViewMemoryWriteReport {
      written: true,
      skip_reason: None,
    },
    Err(skip_reason) => ViewMemoryWriteReport {
      written: false,
      skip_reason: Some(skip_reason),
    },
  })
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
  /// Exact-first resolution tier for `query`. `None` when there is no query
  /// (full listing), since resolution only applies to a keyword search.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub query_resolution: Option<QueryResolutionKind>,
  pub matches: Vec<MatchRef>,
  pub scan: &'a PlaylistSidebarScan,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub view_memory: Option<ViewMemoryWriteReport>,
}

/// Build the agent-facing JSON output without performing any live scan work.
pub fn build_playlist_json_output<'a>(
  scan: &'a PlaylistSidebarScan,
  keyword: Option<&str>,
  view_memory: Option<ViewMemoryWriteReport>,
) -> PlaylistJsonOutput<'a> {
  let sidebar = SidebarView::from_projection(scan.projection().clone());
  let item_count = collect_matches_from_sidebar(&sidebar, None).len();
  let matches = collect_matches_from_sidebar(&sidebar, keyword);
  let query_resolution =
    keyword.map(|keyword| query_resolution_kind(sidebar.playlist_query_resolution(keyword)));
  PlaylistJsonOutput {
    command: "playlist",
    query: keyword.map(str::to_string),
    item_count,
    match_count: matches.len(),
    query_resolution,
    matches,
    scan,
    view_memory,
  }
}

/// Collect items that match the keyword with exact-first resolution.
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

    let output = build_playlist_json_output(&scan, Some("daily"), None);

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
    assert!(output.view_memory.is_none());
  }

  #[test]
  fn build_playlist_json_output_has_no_query_resolution_without_keyword() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, None, None);

    assert_eq!(output.query_resolution, None);
  }

  fn numeric_playlist_projection() -> PlaylistSidebarProjection {
    PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-numeric".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "p43".to_string(),
            label: "43".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
          PlaylistSidebarItem {
            id: "p39".to_string(),
            label: "39".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
          PlaylistSidebarItem {
            id: "p3".to_string(),
            label: "3".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    }
  }

  // ROOT CAUSE:
  //
  // If a caller only reads `match_count`, `playlist ls "3"` against labels
  // "43"/"39"/"3" and against labels "43"/"13" both look like plausible hit
  // counts (1 and 2 respectively) with no way to tell a real unique-exact
  // match apart from a substring collision. A6c-10a live probe of `playlist
  // ls "3"` returned `match_count = 13` with no exact "3" among the results,
  // and that result was verbally misread as "found many 3s" instead of
  // "no exact hit, only contains-collisions". `query_resolution` makes the
  // tier explicit so `match_count` alone can never be over-interpreted again.
  #[test]
  fn build_playlist_json_output_reports_unique_exact_resolution() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(numeric_playlist_projection());

    let output = build_playlist_json_output(&scan, Some("3"), None);

    assert_eq!(output.match_count, 1);
    assert_eq!(output.matches[0].item_id, "p3");
    assert_eq!(
      output.query_resolution,
      Some(QueryResolutionKind::UniqueExact)
    );
  }

  #[test]
  fn build_playlist_json_output_reports_unique_contains_resolution() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("daily"), None);

    assert_eq!(output.match_count, 1);
    assert_eq!(
      output.query_resolution,
      Some(QueryResolutionKind::UniqueContains)
    );
  }

  #[test]
  fn build_playlist_json_output_reports_ambiguous_resolution() {
    let projection = PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-ambiguous".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "p43".to_string(),
            label: "43".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
          PlaylistSidebarItem {
            id: "p13".to_string(),
            label: "13".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    };
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection);

    let output = build_playlist_json_output(&scan, Some("3"), None);

    assert_eq!(output.match_count, 2);
    assert_eq!(
      output.query_resolution,
      Some(QueryResolutionKind::Ambiguous)
    );
  }

  #[test]
  fn build_playlist_json_output_reports_not_found_resolution() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("zzz"), None);

    assert_eq!(output.match_count, 0);
    assert_eq!(output.query_resolution, Some(QueryResolutionKind::NotFound));
  }

  #[test]
  fn playlist_ls_json_includes_view_memory_write_report() {
    use auv_view::VIEW_IR_SCHEMA_VERSION;
    use auv_view::memory::{VIEW_MEMORY_SCHEMA_VERSION, memory_file_path, parse_memory_file};

    use crate::view_memory::{
      PLAYLIST_SIDEBAR_SCOPE_ID, enabled_with_env, write_from_scan_when_enabled,
    };

    fn minimal_writable_scan_json(diagnostics: serde_json::Value) -> String {
      serde_json::json!({
        "schema_version": VIEW_IR_SCHEMA_VERSION,
        "app": {},
        "window": {},
        "sidebar_region": {
          "bounds": {"x": 0.0, "y": 220.0, "width": 240.0, "height": 400.0}
        },
        "observations": [],
        "reconstruction": {
          "root": {
            "id": "root.sidebar",
            "kind": "collection",
            "bounds": {"x": 0.0, "y": 0.0, "width": 240.0, "height": 400.0},
            "anchors": [],
            "landmarks": [],
            "actions": [],
            "evidence": [],
            "children": [{
              "id": "item.test",
              "kind": "item",
              "label": "Test Playlist",
              "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
              "anchors": [{
                "id": "anchor.test",
                "label": "Test Playlist",
                "strength": "strong",
                "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
                "evidence_ids": []
              }],
              "landmarks": [],
              "actions": [],
              "evidence": [],
              "children": []
            }]
          },
          "anchor_index": [{
            "id": "anchor.test",
            "label": "Test Playlist",
            "strength": "strong",
            "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
            "evidence_ids": []
          }],
          "landmark_index": []
        },
        "projection": {"sections": []},
        "boundary": {
          "top": "unknown",
          "bottom": "unknown",
          "left": "unknown",
          "right": "unknown"
        },
        "diagnostics": diagnostics,
        "known_limits": []
      })
      .to_string()
    }

    let scan =
      crate::decode_playlist_sidebar_scan_json(&minimal_writable_scan_json(serde_json::json!([])))
        .expect("writable scan");

    let artifact_dir = std::env::temp_dir().join(format!(
      "auv-netease-playlist-ls-json-vm-{}",
      std::process::id()
    ));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir: artifact_dir.clone(),
      ..crate::Inputs::with_defaults()
    };

    let write_result = write_from_scan_when_enabled(true, &inputs, &scan);
    let view_memory =
      playlist_view_memory_report(enabled_with_env(Some("1")), write_result.clone())
        .expect("gate on should emit view_memory");
    assert!(view_memory.written);
    assert!(view_memory.skip_reason.is_none());

    let output = build_playlist_json_output(&scan, Some("Test"), Some(view_memory));
    let json: serde_json::Value = serde_json::to_value(&output).expect("serialize output");
    assert_eq!(json["view_memory"]["written"], true);
    assert!(json["view_memory"].get("skip_reason").is_none());

    let path = memory_file_path(&artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    let memory = parse_memory_file(&path).expect("written memory should parse");
    assert_eq!(memory.schema_version, VIEW_MEMORY_SCHEMA_VERSION);

    let blocking_scan =
      crate::decode_playlist_sidebar_scan_json(&minimal_writable_scan_json(serde_json::json!([{
        "code": "parser_no_reliable_candidates",
        "message": "blocking",
        "node_id": null
      }])))
      .expect("blocking scan");

    let fail_result = write_from_scan_when_enabled(true, &inputs, &blocking_scan)
      .expect_err("blocking diagnostics should not write");
    let fail_report = playlist_view_memory_report(true, Err(fail_result.clone())).expect("gate on");
    assert!(!fail_report.written);
    assert_eq!(
      fail_report.skip_reason.as_deref(),
      Some(fail_result.as_str())
    );

    let fail_output = build_playlist_json_output(&blocking_scan, None, Some(fail_report));
    let fail_json: serde_json::Value = serde_json::to_value(&fail_output).expect("serialize fail");
    assert_eq!(fail_json["view_memory"]["written"], false);
    assert_eq!(
      fail_json["view_memory"]["skip_reason"].as_str(),
      Some(fail_result.as_str())
    );

    let gate_off =
      build_playlist_json_output(&scan, None, playlist_view_memory_report(false, Ok(())));
    let gate_off_json: serde_json::Value = serde_json::to_value(&gate_off).expect("gate off json");
    assert!(gate_off_json.get("view_memory").is_none());

    let _ = std::fs::remove_dir_all(&artifact_dir);
  }
}
