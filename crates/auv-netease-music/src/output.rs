use serde::Serialize;
use std::path::PathBuf;

use crate::views::query_match::{PlaylistQueryMatchMode, PlaylistQueryResolution};
use crate::views::sidebar::SidebarView;
use crate::{Confidence, PlaylistSidebarProjection, PlaylistSidebarScan, SidebarSectionKind};

/// One playlist item surfaced by the listing or keyword filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MatchRef {
  #[serde(rename = "ref")]
  pub scan_ref: String,
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
  pub label: String,
  pub candidate_id: Option<String>,
  pub anchor_id: Option<String>,
  pub confidence: ConfidenceRef,
  pub source_evidence: MatchSourceEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConfidenceRef {
  pub level: String,
  pub reason: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MatchSourceEvidence {
  pub source: &'static str,
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonResult {
  pub item_count: usize,
  pub match_count: usize,
  #[serde(default, skip_serializing_if = "is_zero")]
  pub filtered_count: usize,
  pub matches: Vec<MatchRef>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonArtifacts {
  pub scan_cache_path: String,
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
pub fn playlist_view_memory_report(gate_enabled: bool, write_result: Result<(), String>) -> Option<ViewMemoryWriteReport> {
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

/// Agent-facing compact JSON output for `playlist ls`.
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonOutput {
  pub command: &'static str,
  pub query: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub min_confidence: Option<String>,
  pub summary: String,
  pub result: PlaylistJsonResult,
  pub artifacts: PlaylistJsonArtifacts,
  /// Exact-first resolution tier for `query`. `None` when there is no query
  /// (full listing), since resolution only applies to a keyword search.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub query_resolution: Option<QueryResolutionKind>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub view_memory: Option<ViewMemoryWriteReport>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub run_id: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
}

/// Build the agent-facing JSON output without performing any live scan work.
pub fn build_playlist_json_output(
  scan: &PlaylistSidebarScan,
  keyword: Option<&str>,
  min_confidence: Option<Confidence>,
  scan_cache_path: PathBuf,
  view_memory: Option<ViewMemoryWriteReport>,
  run_id: Option<String>,
  known_limits: Vec<String>,
) -> PlaylistJsonOutput {
  let sidebar = SidebarView::from_projection(scan.projection().clone());
  let item_count = collect_matches_from_sidebar(&sidebar, None).len();
  let raw_matches = collect_matches_from_sidebar(&sidebar, keyword);
  let raw_match_count = raw_matches.len();
  let matches = filter_matches(raw_matches, min_confidence);
  let filtered_count = raw_match_count.saturating_sub(matches.len());
  let query_resolution = keyword.map(|keyword| query_resolution_kind(sidebar.playlist_query_resolution(keyword)));
  let summary = match keyword {
    Some(_) => format!("{} playlists observed, {} matches", item_count, matches.len()),
    None => format!("{item_count} playlists observed"),
  };
  PlaylistJsonOutput {
    command: "playlist.ls",
    query: keyword.map(str::to_string),
    min_confidence: min_confidence.map(confidence_name),
    summary,
    result: PlaylistJsonResult {
      item_count,
      match_count: matches.len(),
      filtered_count,
      matches: assign_scan_refs(matches),
    },
    artifacts: PlaylistJsonArtifacts {
      scan_cache_path: scan_cache_path.display().to_string(),
    },
    query_resolution,
    view_memory,
    run_id,
    known_limits,
  }
}

/// Collect items that match the keyword with exact-first resolution.
/// `keyword == None` returns every item (full listing).
pub fn collect_matches(projection: &PlaylistSidebarProjection, keyword: Option<&str>) -> Vec<MatchRef> {
  let sidebar = SidebarView::from_projection(projection.clone());
  assign_scan_refs(collect_matches_from_sidebar(&sidebar, keyword))
}

fn collect_matches_from_sidebar(sidebar: &SidebarView, keyword: Option<&str>) -> Vec<MatchRef> {
  sidebar
    .playlists(keyword)
    .into_iter()
    .map(|playlist| {
      let confidence = playlist.item.confidence;
      MatchRef {
        scan_ref: String::new(),
        section_id: playlist.section.id.clone(),
        section_kind: playlist.section.kind,
        item_id: playlist.item.id.clone(),
        label: playlist.item.label.clone(),
        candidate_id: playlist.item.candidate_id.clone(),
        anchor_id: playlist.item.anchor_id.clone(),
        confidence: ConfidenceRef {
          level: confidence_code(confidence).to_string(),
          reason: "existing scan confidence and query match",
        },
        source_evidence: MatchSourceEvidence {
          source: "playlist_sidebar_projection",
          section_id: playlist.section.id.clone(),
          section_kind: playlist.section.kind,
          item_id: playlist.item.id.clone(),
        },
      }
    })
    .collect()
}

fn assign_scan_refs(matches: Vec<MatchRef>) -> Vec<MatchRef> {
  matches
    .into_iter()
    .enumerate()
    .map(|(index, mut candidate)| {
      candidate.scan_ref = format!("pl_{index}");
      candidate
    })
    .collect()
}

fn filter_matches(matches: Vec<MatchRef>, min_confidence: Option<Confidence>) -> Vec<MatchRef> {
  let Some(min_confidence) = min_confidence else {
    return matches;
  };
  matches
    .into_iter()
    .filter(|candidate| {
      let confidence = match candidate.confidence.level.as_str() {
        "H" => Confidence::High,
        "M" => Confidence::Medium,
        _ => Confidence::Low,
      };
      confidence_rank(confidence) >= confidence_rank(min_confidence)
    })
    .collect()
}

pub(crate) fn confidence_code(confidence: Confidence) -> &'static str {
  // TODO(playlist-confidence-scale-v1): XH/XL and numeric scores are deferred
  // until raw OCR/source scores are approved for playlist match refs.
  match confidence {
    Confidence::High => "H",
    Confidence::Medium => "M",
    Confidence::Low => "L",
  }
}

pub(crate) fn confidence_name(confidence: Confidence) -> String {
  match confidence {
    Confidence::High => "high",
    Confidence::Medium => "medium",
    Confidence::Low => "low",
  }
  .to_string()
}

fn confidence_rank(confidence: Confidence) -> u8 {
  match confidence {
    Confidence::High => 3,
    Confidence::Medium => 2,
    Confidence::Low => 1,
  }
}

fn is_zero(value: &usize) -> bool {
  *value == 0
}

pub(crate) fn render_playlist_human_output(
  scan: &PlaylistSidebarScan,
  keyword: Option<&str>,
  min_confidence: Option<Confidence>,
  detail: bool,
  run_id: Option<&str>,
  known_limits: &[String],
  scan_cache_path: Option<&str>,
) -> String {
  let sidebar = SidebarView::from_projection(scan.projection().clone());
  let item_count = collect_matches_from_sidebar(&sidebar, None).len();
  let raw_matches = collect_matches_from_sidebar(&sidebar, keyword);
  let raw_match_count = raw_matches.len();
  let matches = assign_scan_refs(filter_matches(raw_matches, min_confidence));
  let filtered_count = raw_match_count.saturating_sub(matches.len());
  let mut output = String::new();

  match keyword {
    Some(query) => {
      output.push_str(&format!("{item_count} playlists observed. {} matches for {query:?}.\n", matches.len()));
      if filtered_count > 0 {
        if let Some(min_confidence) = min_confidence {
          output.push_str(&format!("filtered {filtered_count} below min-confidence {}\n", confidence_name(min_confidence)));
        }
      }
      output.push('\n');
      for candidate in &matches {
        output.push_str(&format!("* {:<3} {:<5} {}\n", candidate.confidence.level, candidate.scan_ref, candidate.label));
        if detail {
          output.push_str(&format!(
            "      source=playlist_sidebar_projection section={:?} item_id={} candidate_id={} anchor_id={}\n",
            candidate.section_kind,
            candidate.item_id,
            optional(candidate.candidate_id.as_deref()),
            optional(candidate.anchor_id.as_deref())
          ));
        }
      }
      if detail {
        if let Some(query) = keyword {
          output.push_str(&format!(
            "query_resolution={}\n",
            query_resolution_name(query_resolution_kind(sidebar.playlist_query_resolution(query)))
          ));
        }
        append_detail_footer(&mut output, scan, run_id, known_limits, scan_cache_path);
      } else if let Some(candidate_id) = matches.iter().find_map(|match_ref| match_ref.candidate_id.as_deref()) {
        output.push_str(&format!("\nUse: auv-netease-music playlist play --candidate-id {candidate_id}\nMore: --detail, --json\n"));
      } else if !matches.is_empty() {
        output.push_str("\nMore: --detail, --json\n");
      } else {
        output.push_str("\nMore: --detail, --json\n");
      }
    }
    None => {
      output.push_str(&format!("{item_count} playlists observed.\n\nSections:\n"));
      for (kind, count) in section_counts(scan) {
        output.push_str(&format!("  {kind:?}: {count}\n"));
      }
      if detail {
        output.push('\n');
        for candidate in &matches {
          output.push_str(&format!("* {:<3} {:<5} {}\n", candidate.confidence.level, candidate.scan_ref, candidate.label));
          output.push_str(&format!(
            "      source=playlist_sidebar_projection section={:?} item_id={} candidate_id={} anchor_id={}\n",
            candidate.section_kind,
            candidate.item_id,
            optional(candidate.candidate_id.as_deref()),
            optional(candidate.anchor_id.as_deref())
          ));
        }
        append_detail_footer(&mut output, scan, run_id, known_limits, scan_cache_path);
      } else {
        output.push_str("\nMore: use a keyword, --detail, or --json.\n");
      }
    }
  }

  output.trim_end().to_string()
}

fn section_counts(scan: &PlaylistSidebarScan) -> Vec<(SidebarSectionKind, usize)> {
  scan.projection().sections.iter().map(|section| (section.kind, section.items.len())).collect()
}

fn append_detail_footer(
  output: &mut String,
  scan: &PlaylistSidebarScan,
  run_id: Option<&str>,
  known_limits: &[String],
  scan_cache_path: Option<&str>,
) {
  if let Some(path) = scan_cache_path {
    output.push_str(&format!("scan_cache_path={path}\n"));
  }
  if let Some(run_id) = run_id {
    output.push_str(&format!("run_id={run_id}\n"));
  }
  output.push_str("diagnostics:\n");
  if scan.diagnostics().is_empty() {
    output.push_str("  (none)\n");
  } else {
    for diagnostic in scan.diagnostics() {
      output.push_str(&format!("  - {}: {}\n", diagnostic.code, diagnostic.message));
    }
  }
  output.push_str("known_limits:\n");
  if known_limits.is_empty() && scan.known_limits().is_empty() {
    output.push_str("  (none)\n");
  } else {
    for limit in scan.known_limits().iter().chain(known_limits.iter()) {
      output.push_str(&format!("  - {limit}\n"));
    }
  }
}

fn query_resolution_name(kind: QueryResolutionKind) -> &'static str {
  match kind {
    QueryResolutionKind::UniqueExact => "unique_exact",
    QueryResolutionKind::UniqueContains => "unique_contains",
    QueryResolutionKind::Ambiguous => "ambiguous",
    QueryResolutionKind::NotFound => "not_found",
  }
}

fn optional(value: Option<&str>) -> &str {
  value.unwrap_or("(none)")
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
    assert_eq!(matches[0].scan_ref, "pl_0");
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
    assert_eq!(matches[0].section_kind, SidebarSectionKind::FavoritePlaylists);
    assert_eq!(matches[0].item_id, "playlist-daily");
    assert_eq!(matches[0].anchor_id.as_deref(), Some("playlist-anchor"));
  }

  #[test]
  fn build_playlist_json_output_counts_all_items_and_matches() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("daily"), None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.command, "playlist.ls");
    assert_eq!(output.query.as_deref(), Some("daily"));
    assert_eq!(output.result.item_count, 2);
    assert_eq!(output.result.match_count, 1);
    assert_eq!(output.result.matches[0].item_id, "i1");
    assert_eq!(output.result.matches[0].candidate_id.as_deref(), Some("obs1.candidate.daily"));
    assert_eq!(output.artifacts.scan_cache_path, "/tmp/playlist-scan-cache.json");
    assert!(output.view_memory.is_none());
  }

  #[test]
  fn build_playlist_json_output_has_no_query_resolution_without_keyword() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, None, None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.query_resolution, None);
  }

  #[test]
  fn confidence_codes_map_existing_levels() {
    assert_eq!(confidence_code(Confidence::High), "H");
    assert_eq!(confidence_code(Confidence::Medium), "M");
    assert_eq!(confidence_code(Confidence::Low), "L");
  }

  #[test]
  fn min_confidence_filters_lower_confidence_matches() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "high".to_string(),
            label: "Daily High".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: Some("obs.high".to_string()),
            anchor_id: Some("anchor.high".to_string()),
          },
          PlaylistSidebarItem {
            id: "medium".to_string(),
            label: "Daily Medium".to_string(),
            section_hint: None,
            confidence: Confidence::Medium,
            candidate_id: Some("obs.medium".to_string()),
            anchor_id: Some("anchor.medium".to_string()),
          },
          PlaylistSidebarItem {
            id: "low".to_string(),
            label: "Daily Low".to_string(),
            section_hint: None,
            confidence: Confidence::Low,
            candidate_id: Some("obs.low".to_string()),
            anchor_id: Some("anchor.low".to_string()),
          },
        ],
      }],
    });

    let output = build_playlist_json_output(
      &scan,
      Some("daily"),
      Some(Confidence::Medium),
      "/tmp/playlist-scan-cache.json".into(),
      None,
      None,
      Vec::new(),
    );

    assert_eq!(output.result.match_count, 2);
    assert_eq!(output.result.filtered_count, 1);
    assert_eq!(output.result.matches.iter().map(|candidate| candidate.confidence.level.as_str()).collect::<Vec<_>>(), ["H", "M"]);
  }

  #[test]
  fn compact_json_omits_raw_scan_and_includes_refs_resolution_limits_and_memory() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());
    let output = build_playlist_json_output(
      &scan,
      Some("daily"),
      None,
      "/tmp/playlist-scan-cache.json".into(),
      Some(ViewMemoryWriteReport {
        written: true,
        skip_reason: None,
      }),
      Some("run_abc".to_string()),
      vec!["scan stopped after max_scrolls=2".to_string()],
    );

    let json: serde_json::Value = serde_json::to_value(output).expect("serialize output");

    assert!(json.get("scan").is_none());
    assert_eq!(json["artifacts"]["scan_cache_path"], "/tmp/playlist-scan-cache.json");
    assert_eq!(json["run_id"], "run_abc");
    assert_eq!(json["known_limits"][0], "scan stopped after max_scrolls=2");
    assert_eq!(json["query_resolution"], "unique_contains");
    assert_eq!(json["view_memory"]["written"], true);
    assert_eq!(json["result"]["matches"][0]["ref"], "pl_0");
    assert_eq!(json["result"]["matches"][0]["candidate_id"], "obs1.candidate.daily");
    assert_eq!(json["result"]["matches"][0]["anchor_id"], "a1");
    assert_eq!(json["result"]["matches"][0]["confidence"]["level"], "H");
    assert_eq!(json["result"]["matches"][0]["source_evidence"]["source"], "playlist_sidebar_projection");
  }

  #[test]
  fn no_query_human_output_is_bounded_summary() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let rendered = render_playlist_human_output(&scan, None, None, false, None, &[], None);

    assert!(rendered.contains("2 playlists observed."));
    assert!(rendered.contains("Sections:"));
    assert!(rendered.contains("MyPlaylists: 2"));
    assert!(rendered.contains("More: use a keyword, --detail, or --json."));
    assert!(!rendered.contains("Daily Mix"));
    assert!(!rendered.contains("confidence=High"));
  }

  #[test]
  fn query_human_output_renders_ranked_refs_confidence_codes_and_hidden_count() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "high".to_string(),
            label: "Daily High".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: Some("obs.high".to_string()),
            anchor_id: Some("anchor.high".to_string()),
          },
          PlaylistSidebarItem {
            id: "low".to_string(),
            label: "Daily Low".to_string(),
            section_hint: None,
            confidence: Confidence::Low,
            candidate_id: Some("obs.low".to_string()),
            anchor_id: Some("anchor.low".to_string()),
          },
        ],
      }],
    });

    let rendered = render_playlist_human_output(&scan, Some("daily"), Some(Confidence::Medium), false, None, &[], None);

    assert!(rendered.contains("2 playlists observed. 1 matches for \"daily\"."));
    assert!(rendered.contains("filtered 1 below min-confidence medium"));
    assert!(rendered.contains("H   pl_0"));
    assert!(rendered.contains("Daily High"));
    assert!(rendered.contains("playlist play --candidate-id obs.high"));
    assert!(!rendered.contains("Daily Low"));
  }

  #[test]
  fn query_human_output_omits_candidate_id_hint_when_candidate_id_missing() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![PlaylistSidebarItem {
          id: "high".to_string(),
          label: "Daily High".to_string(),
          section_hint: None,
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: Some("anchor.high".to_string()),
        }],
      }],
    });

    let rendered = render_playlist_human_output(&scan, Some("daily"), None, false, None, &[], None);

    assert!(rendered.contains("Daily High"));
    assert!(!rendered.contains("playlist play --candidate-id"));
    assert!(!rendered.contains("(none)"));
    assert!(rendered.contains("More: --detail, --json"));
  }

  #[test]
  fn detail_human_output_adds_evidence_without_full_scan_dump() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let rendered = render_playlist_human_output(
      &scan,
      Some("daily"),
      None,
      true,
      Some("run_abc"),
      &["limit one".to_string()],
      Some("/tmp/playlist-scan-cache.json"),
    );

    assert!(rendered.contains("section=MyPlaylists"));
    assert!(rendered.contains("candidate_id=obs1.candidate.daily"));
    assert!(rendered.contains("anchor_id=a1"));
    assert!(rendered.contains("query_resolution=unique_contains"));
    assert!(rendered.contains("scan_cache_path=/tmp/playlist-scan-cache.json"));
    assert!(rendered.contains("run_id=run_abc"));
    assert!(rendered.contains("known_limits:"));
    assert!(!rendered.contains("observations:"));
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

    let output = build_playlist_json_output(&scan, Some("3"), None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.result.match_count, 1);
    assert_eq!(output.result.matches[0].item_id, "p3");
    assert_eq!(output.query_resolution, Some(QueryResolutionKind::UniqueExact));
  }

  #[test]
  fn build_playlist_json_output_reports_unique_contains_resolution() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("daily"), None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.result.match_count, 1);
    assert_eq!(output.query_resolution, Some(QueryResolutionKind::UniqueContains));
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

    let output = build_playlist_json_output(&scan, Some("3"), None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.result.match_count, 2);
    assert_eq!(output.query_resolution, Some(QueryResolutionKind::Ambiguous));
  }

  #[test]
  fn build_playlist_json_output_reports_not_found_resolution() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(projection());

    let output = build_playlist_json_output(&scan, Some("zzz"), None, "/tmp/playlist-scan-cache.json".into(), None, None, Vec::new());

    assert_eq!(output.result.match_count, 0);
    assert_eq!(output.query_resolution, Some(QueryResolutionKind::NotFound));
  }

  #[test]
  fn playlist_ls_json_includes_view_memory_write_report() {
    use auv_view::VIEW_IR_SCHEMA_VERSION;
    use auv_view::memory::{VIEW_MEMORY_SCHEMA_VERSION, memory_file_path, parse_memory_file};

    use crate::view_memory::{PLAYLIST_SIDEBAR_SCOPE_ID, enabled_with_env, write_from_scan_when_enabled};

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

    let scan = crate::decode_playlist_sidebar_scan_json(&minimal_writable_scan_json(serde_json::json!([]))).expect("writable scan");

    let artifact_dir = std::env::temp_dir().join(format!("auv-netease-playlist-ls-json-vm-{}", std::process::id()));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir: artifact_dir.clone(),
      ..crate::Inputs::with_defaults()
    };

    let write_result = write_from_scan_when_enabled(true, &inputs, &scan);
    let view_memory =
      playlist_view_memory_report(enabled_with_env(Some("1")), write_result.clone()).expect("gate on should emit view_memory");
    assert!(view_memory.written);
    assert!(view_memory.skip_reason.is_none());

    let output =
      build_playlist_json_output(&scan, Some("Test"), None, "/tmp/playlist-scan-cache.json".into(), Some(view_memory), None, Vec::new());
    let json: serde_json::Value = serde_json::to_value(&output).expect("serialize output");
    assert_eq!(json["view_memory"]["written"], true);
    assert!(json["view_memory"].get("skip_reason").is_none());

    let path = memory_file_path(&artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    let memory = parse_memory_file(&path).expect("written memory should parse");
    assert_eq!(memory.schema_version, VIEW_MEMORY_SCHEMA_VERSION);

    let blocking_scan = crate::decode_playlist_sidebar_scan_json(&minimal_writable_scan_json(serde_json::json!([{
      "code": "parser_no_reliable_candidates",
      "message": "blocking",
      "node_id": null
    }])))
    .expect("blocking scan");

    let fail_result = write_from_scan_when_enabled(true, &inputs, &blocking_scan).expect_err("blocking diagnostics should not write");
    let fail_report = playlist_view_memory_report(true, Err(fail_result.clone())).expect("gate on");
    assert!(!fail_report.written);
    assert_eq!(fail_report.skip_reason.as_deref(), Some(fail_result.as_str()));

    let fail_output =
      build_playlist_json_output(&blocking_scan, None, None, "/tmp/playlist-scan-cache.json".into(), Some(fail_report), None, Vec::new());
    let fail_json: serde_json::Value = serde_json::to_value(&fail_output).expect("serialize fail");
    assert_eq!(fail_json["view_memory"]["written"], false);
    assert_eq!(fail_json["view_memory"]["skip_reason"].as_str(), Some(fail_result.as_str()));

    let gate_off = build_playlist_json_output(
      &scan,
      None,
      None,
      "/tmp/playlist-scan-cache.json".into(),
      playlist_view_memory_report(false, Ok(())),
      None,
      Vec::new(),
    );
    let gate_off_json: serde_json::Value = serde_json::to_value(&gate_off).expect("gate off json");
    assert!(gate_off_json.get("view_memory").is_none());

    let _ = std::fs::remove_dir_all(&artifact_dir);
  }

  #[test]
  fn build_playlist_json_output_includes_run_id_and_known_limits() {
    let scan = {
      let json = serde_json::json!({
        "schema_version": "view-ir-v0",
        "app": {},
        "window": {},
        "sidebar_region": {"bounds": {"x": 0.0, "y": 220.0, "width": 240.0, "height": 400.0}},
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
            "children": []
          },
          "anchor_index": [],
          "landmark_index": []
        },
        "projection": {"sections": []},
        "boundary": {"top": "unknown", "bottom": "unknown", "left": "unknown", "right": "unknown"},
        "diagnostics": [],
        "known_limits": []
      });
      crate::decode_playlist_sidebar_scan_json(&json.to_string()).expect("scan")
    };
    let limits = vec!["artifact-dir mirror incomplete; durable source is store run run_abc".to_string()];
    let output = build_playlist_json_output(
      &scan,
      None,
      None,
      "/tmp/playlist-scan-cache.json".into(),
      None,
      Some("run_abc".to_string()),
      limits.clone(),
    );
    assert_eq!(output.run_id.as_deref(), Some("run_abc"));
    assert_eq!(output.known_limits, limits);
    let json = serde_json::to_string(&output).expect("json");
    assert!(json.contains("known_limits"));
    assert!(json.contains("run_abc"));
  }
}
