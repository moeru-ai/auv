#[cfg(feature = "tracing")]
use auv_cli_common::TableRow;
#[cfg(feature = "tracing")]
use auv_cli_common::outputs::cli::CliOutput;
#[cfg(feature = "tracing")]
use auv_cli_common::outputs::formats::table::{self, TableOptions};
use serde::Serialize;

use crate::views::query_match::{PlaylistQueryMatchMode, PlaylistQueryResolution};
use crate::views::sidebar::SidebarView;
use crate::{Confidence, PlaylistSidebarScan, SidebarSectionKind};

#[cfg(feature = "tracing")]
pub(crate) fn render_song_list_human(result: &crate::SongListScanResult) -> String {
  let mut lines = vec![
    "NetEase song list scan".to_string(),
    format!("target: {}", result.target),
    format!("items: {}", result.items.len()),
    format!("observations: {}", result.observations.len()),
  ];
  if result.known_limits.is_empty() {
    lines.push("known_limits: (none)".to_string());
  } else {
    lines.push("known_limits:".to_string());
    lines.extend(result.known_limits.iter().map(|limit| format!("  - {limit}")));
  }
  lines.join("\n")
}

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

/// Agent-facing compact JSON output for `playlist ls`.
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonOutput {
  pub command: &'static str,
  pub query: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub min_confidence: Option<String>,
  pub result: PlaylistJsonResult,
  /// Exact-first resolution tier for `query`. `None` when there is no query
  /// (full listing), since resolution only applies to a keyword search.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub query_resolution: Option<QueryResolutionKind>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
}

/// Build the agent-facing JSON output without performing any live scan work.
pub fn build_playlist_json_output(
  scan: &PlaylistSidebarScan,
  keyword: Option<&str>,
  min_confidence: Option<Confidence>,
) -> PlaylistJsonOutput {
  let sidebar = SidebarView::from_projection(scan.projection().clone());
  let item_count = collect_matches_from_sidebar(&sidebar, None).len();
  let raw_matches = collect_matches_from_sidebar(&sidebar, keyword);
  let raw_match_count = raw_matches.len();
  let matches = filter_matches(raw_matches, min_confidence);
  let filtered_count = raw_match_count.saturating_sub(matches.len());
  let query_resolution = keyword.map(|keyword| query_resolution_kind(sidebar.playlist_query_resolution(keyword)));
  PlaylistJsonOutput {
    command: "playlist.ls",
    query: keyword.map(str::to_string),
    min_confidence: min_confidence.map(|confidence| confidence.to_string()),
    result: PlaylistJsonResult {
      item_count,
      match_count: matches.len(),
      filtered_count,
      matches: assign_scan_refs(matches),
    },
    query_resolution,
    known_limits: scan.known_limits().to_vec(),
  }
}

/// One computed playlist listing shared by JSON, table, and human routes.
#[cfg(feature = "tracing")]
pub(crate) struct PlaylistOutput<'a> {
  json: PlaylistJsonOutput,
  scan: &'a PlaylistSidebarScan,
  detail: bool,
}

#[cfg(feature = "tracing")]
impl<'a> PlaylistOutput<'a> {
  pub(crate) fn new(scan: &'a PlaylistSidebarScan, keyword: Option<&str>, min_confidence: Option<Confidence>, detail: bool) -> Self {
    Self {
      json: build_playlist_json_output(scan, keyword, min_confidence),
      scan,
      detail,
    }
  }
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
          level: confidence.short_code().to_string(),
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
    .filter(|candidate| Confidence::from_short_code(&candidate.confidence.level).unwrap_or_default() >= min_confidence)
    .collect()
}

fn is_zero(value: &usize) -> bool {
  *value == 0
}

#[cfg(feature = "tracing")]
impl CliOutput for PlaylistOutput<'_> {
  fn to_json(&self) -> impl Serialize {
    &self.json
  }

  fn to_table_print(&self, options: TableOptions<'_>) -> String {
    render_playlist_table(&self.json.result.matches, options)
  }

  fn to_human(&self, options: TableOptions<'_>) -> String {
    let scan = self.scan;
    let detail = self.detail;
    let keyword = self.json.query.as_deref();
    let item_count = self.json.result.item_count;
    let matches = &self.json.result.matches;
    let filtered_count = self.json.result.filtered_count;
    let mut output = String::new();

    match keyword {
      Some(query) => {
        output.push_str(&format!("{item_count} playlists observed. {} matches for {query:?}.\n", matches.len()));
        if filtered_count > 0 {
          if let Some(min_confidence) = &self.json.min_confidence {
            output.push_str(&format!("filtered {filtered_count} below min-confidence {min_confidence}\n"));
          }
        }
        output.push('\n');
        for candidate in matches {
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
          if let Some(query_resolution) = self.json.query_resolution {
            output.push_str(&format!("query_resolution={}\n", query_resolution_name(query_resolution)));
          }
          append_detail_footer(&mut output, scan);
        } else {
          output.push_str("\nMore: --detail, --json\n");
        }
      }
      None => {
        if detail {
          output.push_str(&format!("{item_count} playlists observed.\n\nSections:\n"));
          for section in &scan.projection().sections {
            output.push_str(&format!("  {:?}: {}\n", section.kind, section.items.len()));
          }
          output.push('\n');
          for candidate in matches {
            output.push_str(&format!("* {:<3} {:<5} {}\n", candidate.confidence.level, candidate.scan_ref, candidate.label));
            output.push_str(&format!(
              "      source=playlist_sidebar_projection section={:?} item_id={} candidate_id={} anchor_id={}\n",
              candidate.section_kind,
              candidate.item_id,
              optional(candidate.candidate_id.as_deref()),
              optional(candidate.anchor_id.as_deref())
            ));
          }
          append_detail_footer(&mut output, scan);
        } else {
          output.push_str(&self.to_table_print(options.empty_message("(no playlists observed)")));
          output.push_str("\n\nMore: use a keyword, --detail, or --json.\n");
        }
      }
    }

    output.trim_end().to_string()
  }
}

#[cfg(feature = "tracing")]
fn render_playlist_table(matches: &[MatchRef], options: TableOptions<'_>) -> String {
  let rows = matches
    .iter()
    .map(|candidate| PlaylistTableRow {
      name: &candidate.label,
      section: candidate.section_kind,
      confidence: &candidate.confidence.level,
      anchor_id: candidate.anchor_id.as_deref(),
    })
    .collect::<Vec<_>>();
  table::render(&rows, options)
}

#[derive(TableRow)]
#[cfg(feature = "tracing")]
struct PlaylistTableRow<'a> {
  name: &'a str,
  #[table(display_with = "section_kind_name")]
  section: SidebarSectionKind,
  #[table(display_with = "confidence_level_name")]
  confidence: &'a str,
  anchor_id: Option<&'a str>,
}

#[cfg(feature = "tracing")]
fn section_kind_name(kind: &SidebarSectionKind) -> &'static str {
  match kind {
    SidebarSectionKind::FeatureNav => "feature_nav",
    SidebarSectionKind::LibraryNav => "library_nav",
    SidebarSectionKind::PlaylistNav => "playlist_nav",
    SidebarSectionKind::MyPlaylists => "my_playlists",
    SidebarSectionKind::FavoritePlaylists => "favorite_playlists",
    SidebarSectionKind::Unknown => "unknown",
  }
}

#[cfg(feature = "tracing")]
fn confidence_level_name(level: &str) -> &'static str {
  match level {
    "H" => "high",
    "M" => "medium",
    _ => "low",
  }
}

#[cfg(feature = "tracing")]
fn append_detail_footer(output: &mut String, scan: &PlaylistSidebarScan) {
  output.push_str("diagnostics:\n");
  if scan.diagnostics().is_empty() {
    output.push_str("  (none)\n");
  } else {
    for diagnostic in scan.diagnostics() {
      output.push_str(&format!("  - {}: {}\n", diagnostic.code, diagnostic.message));
    }
  }
  output.push_str("known_limits:\n");
  if scan.known_limits().is_empty() {
    output.push_str("  (none)\n");
  } else {
    for limit in scan.known_limits() {
      output.push_str(&format!("  - {limit}\n"));
    }
  }
}

#[cfg(feature = "tracing")]
fn query_resolution_name(kind: QueryResolutionKind) -> &'static str {
  match kind {
    QueryResolutionKind::UniqueExact => "unique_exact",
    QueryResolutionKind::UniqueContains => "unique_contains",
    QueryResolutionKind::Ambiguous => "ambiguous",
    QueryResolutionKind::NotFound => "not_found",
  }
}

#[cfg(feature = "tracing")]
fn optional(value: Option<&str>) -> &str {
  value.unwrap_or("(none)")
}

#[cfg(all(target_os = "macos", feature = "tracing"))]
pub(crate) fn now_playing_for_app(
  state: auv_media_macos::NowPlayingState,
  app_id: &str,
) -> (auv_media_macos::NowPlayingState, auv_media_macos::output::NowPlayingOutput) {
  let state = if state.source_bundle_id.as_deref() == Some(app_id) {
    state
  } else {
    auv_media_macos::NowPlayingState::default()
  };
  let output = auv_media_macos::output::build_now_playing_output(&state);
  (state, output)
}
