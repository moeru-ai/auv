//! NetEase Music product CLI library: sidebar playlist scan + agent-callable output.

pub mod api;
pub mod app;
#[cfg(feature = "tracing")]
pub mod cli;
pub mod commands;
pub mod models;
pub mod output;
pub mod runner;
pub mod scroll;
mod telemetry;
pub mod view_parsers;
pub mod views;
pub mod windows;

#[cfg(test)]
mod recognition_test_data;

pub use app::{AppViews, NeteaseCloudMusic, ViewRead, ViewReuse, ViewScope, run_songs_scan};
pub use commands::daily_recommended::{run_daily_recommended_play, run_daily_recommended_songs_scan};
pub use commands::launch::{LaunchResult, OpenWindowInputs, run_open_window};
pub use commands::playback::{
  PlaybackStatus, PlaybackStatusHumanReadable, PlaybackStatusInputs, PlaybackStatusJson, run_playback_status_probe,
};
pub use commands::playlist::{
  PlaylistPlayResult, PlaylistPlayVerification, PlaylistSelectResult, PlaylistSelectTitleOcrTier, PlaylistSelectVerification,
  PlaylistSelectVerificationEvidence, run_playlist_play, run_playlist_play_ref, run_playlist_select, run_playlist_select_ref,
};
pub use commands::transport::{TransportAction, TransportInputs, TransportResult, run_transport_action};
pub use models::{DailyRecommendedRef, FeaturedEntry, FeaturedEntryKind, PlaylistRef, PlaylistSection, SongSource};
pub use view_parsers::sidebar::live::{run_live_scan, run_live_scan_until_query};
pub use views::daily_recommended::DailyRecommendedView;
pub use views::main::MainView;
pub use views::player::PlaybackControlState;
pub use views::recommended::{FeaturedEntriesView, FeaturedEntryView, RecommendedView};
pub use views::sidebar::{PlaylistSidebarItem, PlaylistSidebarProjection, SidebarSection, SidebarSectionKind};

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::scroll::policies::detection_motion::{MotionDetectionPolicy, MotionEvidence};
use crate::view_parsers::sidebar::*;
use crate::views::player::classify_bottom_playback_control_state;
use crate::views::screen;
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};
// Framework view-parser IR types, utilities, and the `ViewObserver` trait
// live in `auv-view` so other app crates (future QQ Music, etc.) can build
// on the same vocabulary without duplicating the records or re-defining the
// observer contract. Domain types (`PlaylistSidebarScan`, `SidebarSection`,
// the `Sidebar*` candidate flavors, the scan-loop functions) stay in this
// crate because they consume NetEase-shaped observations.
use auv_driver::{RatioRect, Size};
use auv_view::{
  AnchorStrength, BoundaryConfidence, CandidateRole, Confidence, LandmarkUse, ParserDiagnostic, ReconstructionOutput, ReconstructionPolicy,
  ScanAppContext, ScanOptions, ScanWindowContext, ScrollBoundarySummary, TopSeekOutcome, VIEW_IR_SCHEMA_VERSION, ViewAction, ViewAnchor,
  ViewAxis, ViewBounds, ViewEvidenceNode, ViewEvidenceSource, ViewLandmark, ViewLayout, ViewNodeKind, ViewNodeRecord, ViewObservation,
  ViewObserver, ViewReconstructionRecord, ViewRegionRecord, ViewScrollable, ViewViewportRecord, confidence_from_ocr, normalize_identity,
  reconstruct, slug, viewport_contains_center, viewport_fingerprint,
};
use clap::ValueEnum;
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use auv_driver::capture::Capture;
#[cfg(target_os = "macos")]
use auv_driver::selector::{App, Window};
#[cfg(target_os = "macos")]
use auv_driver::{ActivationPolicy, Click, InputPolicy, LocalDriverSession, PrepareForInputOptions, Scroll, ScrollOptions, WindowPoint};
#[cfg(target_os = "macos")]
use auv_driver_macos::native::tree::capture_ax_tree_snapshot;
#[cfg(target_os = "macos")]
use auv_driver_macos::types::ObservedAxNode;
#[cfg(target_os = "macos")]
use auv_view::draw_rect;

pub const DEFAULT_APP_ID: &str = "com.netease.163music";
// TODO(netease-scroll-completion): this conservative default is only a
// product-agnostic safety cap, not an account-size estimate or completion
// policy. Full playlist enumeration should derive its budget from section
// counts or stronger scroll-state evidence when that slice is owner-approved.
pub const DEFAULT_MAX_SCROLLS: usize = 12;
// NOTICE(netease-scroll-settle): NetEase sidebar scrolls settle quickly in
// observed captures. Keep the default below generic desktop-action waits so
// playlist listing remains interactive; raise via --scroll-settle-ms if OCR
// evidence becomes unstable on slower machines.
pub const DEFAULT_SCROLL_SETTLE_MS: u64 = 50;
const LIVE_TOP_SEEK_MAX_SCROLL_INPUTS: usize = 32;
const LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER: f64 = 8.0;
const LIVE_FAST_SEEK_BATCH_SCROLLS: usize = 4;
const LIVE_FAST_SEEK_SAMPLE_INTERVAL_MS: u64 = 40;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistCategory {
  #[default]
  All,
  Created,
  Favorite,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
  pub app_id: String,
  pub max_scrolls: usize,
  pub scroll_amount: f64,
  pub scroll_settle_ms: u64,
  pub sidebar_region: Option<RatioRect>,
  pub ocr_options: TextRecognitionOptions,
  pub category: PlaylistCategory,
}

impl Inputs {
  pub fn with_defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      max_scrolls: DEFAULT_MAX_SCROLLS,
      scroll_amount: 300.0,
      scroll_settle_ms: DEFAULT_SCROLL_SETTLE_MS,
      sidebar_region: None,
      ocr_options: TextRecognitionOptions::default(),
      category: PlaylistCategory::All,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DailyRecommendedPlayInputs {
  pub app_id: String,
  pub max_top_scrolls: usize,
  pub top_scroll_amount: f64,
  pub settle_ms: u64,
  // TODO(netease-daily-artifact-discovery): automatic template/artifact
  // discovery is deferred until an owner-approved invoke/run-storage read-side
  // slice defines how product commands should consume prior AUV artifacts.
  pub play_icon_template: Option<PathBuf>,
  pub play_icon_threshold: f64,
  pub ocr_options: TextRecognitionOptions,
}

impl DailyRecommendedPlayInputs {
  pub fn with_defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      max_top_scrolls: 8,
      top_scroll_amount: 420.0,
      settle_ms: 350,
      play_icon_template: None,
      play_icon_threshold: 0.72,
      ocr_options: TextRecognitionOptions::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailyRecommendedPlayResult {
  pub command: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub verification: DailyRecommendedVerification,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

impl DailyRecommendedPlayResult {
  pub fn to_human_readable(&self) -> DailyRecommendedHumanSummary<'_> {
    DailyRecommendedHumanSummary { result: self }
  }
}

pub struct DailyRecommendedHumanSummary<'a> {
  result: &'a DailyRecommendedPlayResult,
}

impl fmt::Display for DailyRecommendedHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let result = self.result;
    writeln!(f, "NetEase daily recommended play")?;
    writeln!(f, "app: id={} name={}", optional(result.app.app_id.as_deref()), optional(result.app.name.as_deref()))?;
    writeln!(f, "window: title={}", optional(result.window.title.as_deref()))?;
    writeln!(
      f,
      "verification: {}{}",
      if result.verification.passed() {
        "passed"
      } else {
        "failed"
      },
      result.verification.best_score().map(|score| format!(" best_score={score:.3}")).unwrap_or_default()
    )?;
    if result.diagnostics.is_empty() {
      write!(f, "diagnostics: (none)")
    } else {
      writeln!(f, "diagnostics:")?;
      for diagnostic in &result.diagnostics {
        writeln!(f, "  - {}: {}", diagnostic.code, diagnostic.message)?;
      }
      Ok(())
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongListScanResult {
  pub command: String,
  pub target: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub song_list_region: ViewRegionRecord,
  pub items: Vec<SongListItem>,
  pub observations: Vec<SongListObservation>,
  pub boundary: ScrollBoundarySummary,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongListObservation {
  pub observation_index: usize,
  pub incoming_scroll_delivery_path: Option<String>,
  pub scroll_motion: Option<MotionEvidence>,
  pub rows: Vec<SongListItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongListItem {
  pub id: String,
  pub index: Option<u32>,
  pub title: String,
  pub row_text: String,
  pub bounds: Option<ViewBounds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DailyRecommendedVerification {
  Passed {
    evidence: DailyRecommendedVerificationEvidence,
  },
  Failed {
    evidence: DailyRecommendedVerificationEvidence,
  },
}

impl DailyRecommendedVerification {
  pub fn passed(&self) -> bool {
    matches!(self, Self::Passed { .. })
  }

  pub fn best_score(&self) -> Option<f64> {
    match self.evidence() {
      DailyRecommendedVerificationEvidence::IconMatch { best_score, .. } => *best_score,
      DailyRecommendedVerificationEvidence::BottomPlaybackControl { .. } => None,
    }
  }

  fn evidence(&self) -> &DailyRecommendedVerificationEvidence {
    match self {
      Self::Passed { evidence } | Self::Failed { evidence } => evidence,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum DailyRecommendedVerificationEvidence {
  IconMatch {
    threshold: f64,
    match_count: usize,
    best_score: Option<f64>,
  },
  BottomPlaybackControl {
    control_state: PlaybackControlState,
    observed_bottom_text: Option<String>,
  },
}

/// Top-level scan artifact for one `netease_playlist_ls` run.
///
/// Every `id` field reachable from this struct (on `ViewNodeRecord`,
/// `ViewAnchor`, `ViewLandmark`, `SidebarSection`, `PlaylistSidebarItem`,
/// and the `candidate_id` / `anchor_id` references on items) is
/// **parse-scoped**: it is unique within this single scan only and is not
/// guaranteed to be stable across runs or app versions. Cross-run lookups
/// (e.g. a future `playlist get <anchor_id>`) must not rely on these as
/// durable identifiers without first introducing content-derived IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarScanStopReason {
  ReachedStopLandmark,
  RepeatedViewportFingerprint,
  RepeatedViewportFingerprintWithAxScrollbarBottom,
  ScrollNoNewSemanticCandidatesAfterInput,
  ScrollNoNewSemanticCandidatesWithAxScrollbarBottom,
  ScrollNoMotionAfterInput,
  ScrollNoMotionWithAxScrollbarBottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
  Up,
  #[default]
  Down,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSidebarScan {
  /// Wire-shape version of this artifact. See `VIEW_IR_SCHEMA_VERSION`.
  schema_version: String,
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  observations: Vec<SidebarViewportObservation>,
  reconstruction: ViewReconstructionRecord,
  projection: PlaylistSidebarProjection,
  boundary: ScrollBoundarySummary,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  stop_reason: Option<SidebarScanStopReason>,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistSelectTarget {
  pub label: String,
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
  pub anchor_id: Option<String>,
  pub candidate_id: Option<String>,
  pub observation_index: Option<usize>,
  pub bounds: Option<ViewBounds>,
}

impl PlaylistSidebarScan {
  fn empty(app: ScanAppContext, window: ScanWindowContext, sidebar_region: ViewRegionRecord) -> Self {
    let mut root = empty_root();
    if let Some(bounds) = sidebar_region.bounds {
      root.bounds = bounds;
    }

    Self {
      schema_version: VIEW_IR_SCHEMA_VERSION.to_string(),
      app,
      window,
      sidebar_region,
      observations: Vec::new(),
      reconstruction: ViewReconstructionRecord {
        root,
        anchor_index: Vec::new(),
        landmark_index: Vec::new(),
      },
      projection: PlaylistSidebarProjection::default(),
      boundary: ScrollBoundarySummary::default(),
      stop_reason: None,
      diagnostics: Vec::new(),
      known_limits: Vec::new(),
    }
  }

  fn empty_with_diagnostic(
    app: ScanAppContext,
    window: ScanWindowContext,
    sidebar_region: ViewRegionRecord,
    diagnostic: ParserDiagnostic,
    known_limit: impl Into<String>,
  ) -> Self {
    let mut scan = Self::empty(app, window, sidebar_region);
    scan.diagnostics.push(diagnostic);
    scan.known_limits.push(known_limit.into());
    scan
  }

  pub fn app(&self) -> &ScanAppContext {
    &self.app
  }

  pub fn window(&self) -> &ScanWindowContext {
    &self.window
  }

  pub fn sidebar_region(&self) -> &ViewRegionRecord {
    &self.sidebar_region
  }

  pub fn observations_len(&self) -> usize {
    self.observations.len()
  }

  pub fn reconstruction(&self) -> &ViewReconstructionRecord {
    &self.reconstruction
  }

  pub fn projection(&self) -> &PlaylistSidebarProjection {
    &self.projection
  }

  pub fn boundary(&self) -> &ScrollBoundarySummary {
    &self.boundary
  }

  pub fn stop_reason(&self) -> Option<SidebarScanStopReason> {
    self.stop_reason
  }

  pub fn diagnostics(&self) -> &[ParserDiagnostic] {
    &self.diagnostics
  }

  pub fn known_limits(&self) -> &[String] {
    &self.known_limits
  }

  pub fn select_target(&self, query: &str) -> Result<PlaylistSelectTarget, String> {
    let query = query.trim();
    if query.is_empty() {
      return Err("playlist select query must not be empty".to_string());
    }

    let sidebar = crate::views::sidebar::SidebarView::from_projection(self.projection.clone());
    let matches = sidebar.playlists(Some(query));
    let [playlist] = matches.as_slice() else {
      return match matches.len() {
        0 => Err(format!("no playlist matched {query:?}")),
        count => Err(format!("playlist query {query:?} matched {count} items; refine the query")),
      };
    };
    Ok(self.select_target_from_parts(playlist.section, playlist.item))
  }

  /// Resolve an exact semantic playlist reference in the canonical scan.
  pub fn select_target_ref(&self, reference: &PlaylistRef) -> Result<PlaylistSelectTarget, String> {
    let normalized_label = normalize_identity(reference.label());
    let matches = self
      .projection
      .sections
      .iter()
      .filter(|section| playlist_section_matches_ref(section.kind, reference.section()))
      .flat_map(|section| {
        section.items.iter().filter(|item| normalize_identity(&item.label) == normalized_label).map(move |item| (section, item))
      })
      .collect::<Vec<_>>();
    let [(section, item)] = matches.as_slice() else {
      return match matches.len() {
        0 => Err(format!("no playlist matched reference section={:?} label={:?}", reference.section(), reference.label())),
        count => Err(format!("playlist reference section={:?} label={:?} matched {count} items", reference.section(), reference.label())),
      };
    };
    Ok(self.select_target_from_parts(section, item))
  }

  pub fn select_target_by_candidate_id(&self, candidate_id: &str) -> Result<PlaylistSelectTarget, String> {
    let candidate_id = candidate_id.trim();
    if candidate_id.is_empty() {
      return Err("playlist candidate_id must not be empty".to_string());
    }

    for section in &self.projection.sections {
      if !matches!(section.kind, SidebarSectionKind::MyPlaylists | SidebarSectionKind::FavoritePlaylists) {
        continue;
      }
      for item in &section.items {
        if item.candidate_id.as_deref() != Some(candidate_id) {
          continue;
        }
        return Ok(self.select_target_from_parts(section, item));
      }
    }

    Err(format!("no playlist candidate_id matched {candidate_id:?} in the canonical playlist scan"))
  }

  pub fn to_human_readable(&self) -> PlaylistSidebarHumanSummary<'_> {
    PlaylistSidebarHumanSummary { scan: self }
  }

  fn candidate_bounds(&self, candidate_id: &str) -> Option<(usize, ViewBounds)> {
    self.observations.iter().find_map(|observation| {
      observation
        .candidates
        .iter()
        .find(|candidate| candidate.id == candidate_id)
        .and_then(|candidate| candidate.bounds)
        .map(|bounds| (observation.observation_index, bounds))
    })
  }

  fn select_target_from_parts(&self, section: &SidebarSection, item: &PlaylistSidebarItem) -> PlaylistSelectTarget {
    let (observation_index, bounds) = item
      .candidate_id
      .as_deref()
      .and_then(|candidate_id| self.candidate_bounds(candidate_id))
      .map(|(index, bounds)| (Some(index), Some(bounds)))
      .unwrap_or((None, None));
    PlaylistSelectTarget {
      label: item.label.clone(),
      section_id: section.id.clone(),
      section_kind: section.kind,
      item_id: item.id.clone(),
      anchor_id: item.anchor_id.clone(),
      candidate_id: item.candidate_id.clone(),
      observation_index,
      bounds,
    }
  }
}

fn playlist_section_matches_ref(kind: SidebarSectionKind, reference: PlaylistSection) -> bool {
  matches!(
    (kind, reference),
    (SidebarSectionKind::MyPlaylists, PlaylistSection::Created) | (SidebarSectionKind::FavoritePlaylists, PlaylistSection::Favorite)
  )
}

pub struct PlaylistSidebarHumanSummary<'a> {
  scan: &'a PlaylistSidebarScan,
}

impl fmt::Display for PlaylistSidebarHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let scan = self.scan;
    writeln!(f, "NetEase playlist sidebar scan")?;
    writeln!(
      f,
      "app: id={} name={} version={}",
      optional(scan.app.app_id.as_deref()),
      optional(scan.app.name.as_deref()),
      optional(scan.app.version.as_deref())
    )?;
    writeln!(
      f,
      "window: id={} title={} bounds={}",
      optional(scan.window.id.as_deref()),
      optional(scan.window.title.as_deref()),
      render_optional_bounds(scan.window.bounds)
    )?;
    writeln!(
      f,
      "sidebar_region: name={} bounds={}",
      optional(scan.sidebar_region.name.as_deref()),
      render_optional_bounds(scan.sidebar_region.bounds)
    )?;
    writeln!(
      f,
      "boundary: top={:?} bottom={:?} left={:?} right={:?}",
      scan.boundary.top, scan.boundary.bottom, scan.boundary.left, scan.boundary.right
    )?;
    writeln!(f, "observations: {}", scan.observations.len())?;
    writeln!(f, "sections:")?;
    if scan.projection.sections.is_empty() {
      writeln!(f, "  (none)")?;
    } else {
      for section in &scan.projection.sections {
        writeln!(f, "  - {} [{:?}]", optional(section.label.as_deref()), section.kind)?;
        if section.items.is_empty() {
          writeln!(f, "    (no items)")?;
        } else {
          for item in &section.items {
            writeln!(f, "    - {} confidence={:?} anchor={}", item.label, item.confidence, optional(item.anchor_id.as_deref()))?;
          }
        }
      }
    }
    writeln!(f, "diagnostics:")?;
    if scan.diagnostics.is_empty() {
      writeln!(f, "  (none)")?;
    } else {
      for diagnostic in &scan.diagnostics {
        writeln!(
          f,
          "  - {}: {}{}",
          diagnostic.code,
          diagnostic.message,
          diagnostic.node_id.as_deref().map(|node_id| format!(" node={node_id}")).unwrap_or_default()
        )?;
      }
    }
    writeln!(f, "known_limits:")?;
    if scan.known_limits.is_empty() {
      write!(f, "  (none)")
    } else {
      for (index, limit) in scan.known_limits.iter().enumerate() {
        if index > 0 {
          writeln!(f)?;
        }
        write!(f, "  - {limit}")?;
      }
      Ok(())
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SidebarViewportObservation {
  observation_index: usize,
  viewport: ViewViewportRecord,
  incoming_scroll_delivery_path: Option<String>,
  scroll_motion: Option<MotionEvidence>,
  viewport_fingerprint: String,
  evidence_nodes: Vec<ViewEvidenceNode>,
  candidates: Vec<SidebarViewportCandidate>,
  parser_notes: Vec<ParserDiagnostic>,
  /// Transient live-only AX corroboration for scroll completion.
  ///
  /// `PlaylistSidebarScan` does not persist this yet; it only helps the
  /// collection loop decide whether a heuristic stop is being contradicted or
  /// corroborated by the app's visible scroll state.
  #[serde(skip, default)]
  ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>,
}

impl ViewObservation for SidebarViewportObservation {
  fn viewport_fingerprint(&self) -> &str {
    &self.viewport_fingerprint
  }
  fn parser_notes(&self) -> &[ParserDiagnostic] {
    &self.parser_notes
  }
  fn has_evidence(&self) -> bool {
    !self.evidence_nodes.is_empty()
  }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SidebarViewportCandidate {
  id: String,
  kind: SidebarCandidateKind,
  label: Option<String>,
  bounds: Option<ViewBounds>,
  evidence_ids: Vec<String>,
  confidence: Confidence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarCandidateKind {
  SectionHeader,
  PlaylistItem,
  NavigationItem,
  #[default]
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarScrollbarBoundary {
  Top,
  Bottom,
  Interior,
}

/// Decode a stored playlist sidebar scan artifact and reject unknown wire
/// shapes before interpreting the app-specific fields.
#[derive(Debug, thiserror::Error)]
pub enum PlaylistSidebarScanDecodeError {
  #[error("invalid playlist sidebar scan JSON: {0}")]
  InvalidJson(#[source] serde_json::Error),
  #[error("playlist sidebar scan JSON is missing schema_version")]
  MissingSchemaVersion,
  #[error("unsupported playlist sidebar scan schema_version {actual:?}; expected {VIEW_IR_SCHEMA_VERSION:?}")]
  UnsupportedSchemaVersion { actual: String },
  #[error("invalid playlist sidebar scan shape: {0}")]
  InvalidShape(#[source] serde_json::Error),
}

pub fn decode_playlist_sidebar_scan_json(input: &str) -> Result<PlaylistSidebarScan, PlaylistSidebarScanDecodeError> {
  let value: serde_json::Value = serde_json::from_str(input).map_err(PlaylistSidebarScanDecodeError::InvalidJson)?;
  let schema_version =
    value.get("schema_version").and_then(serde_json::Value::as_str).ok_or(PlaylistSidebarScanDecodeError::MissingSchemaVersion)?;
  if schema_version != VIEW_IR_SCHEMA_VERSION {
    return Err(PlaylistSidebarScanDecodeError::UnsupportedSchemaVersion {
      actual: schema_version.to_string(),
    });
  }

  serde_json::from_value(value).map_err(PlaylistSidebarScanDecodeError::InvalidShape)
}

fn optional(value: Option<&str>) -> &str {
  value.filter(|value| !value.trim().is_empty()).unwrap_or("-")
}

fn render_optional_bounds(bounds: Option<ViewBounds>) -> String {
  bounds
    .map(|bounds| format!("x={:.1},y={:.1},w={:.1},h={:.1}", bounds.x, bounds.y, bounds.width, bounds.height))
    .unwrap_or_else(|| "-".to_string())
}

fn empty_root() -> ViewNodeRecord {
  ViewNodeRecord {
    id: "root.sidebar".to_string(),
    kind: ViewNodeKind::Collection,
    domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
    layout: Some(ViewLayout::VStack),
    label: None,
    bounds: ViewBounds::default(),
    scrollable: Some(ViewScrollable {
      axis: ViewAxis::Vertical,
      boundary: ScrollBoundarySummary::default(),
    }),
    anchors: Vec::new(),
    landmarks: Vec::new(),
    actions: vec![ViewAction::Scroll],
    evidence: Vec::new(),
    children: Vec::new(),
  }
}

#[cfg(target_os = "macos")]
fn recognition_in_window_space(mut recognition: TextRecognition, capture: &Capture) -> TextRecognition {
  for region in &mut recognition.regions {
    region.bounds.origin.x -= capture.bounds.origin.x;
    region.bounds.origin.y -= capture.bounds.origin.y;
  }
  recognition
}

fn crop_image(image: &RgbaImage, bounds: ViewBounds, scale_factor: f64) -> RgbaImage {
  let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
    scale_factor
  } else {
    1.0
  };
  let x = (bounds.x * scale).max(0.0).floor() as u32;
  let y = (bounds.y * scale).max(0.0).floor() as u32;
  let right = ((bounds.x + bounds.width) * scale).ceil().max(0.0) as u32;
  let bottom = ((bounds.y + bounds.height) * scale).ceil().max(0.0) as u32;
  let right = right.min(image.width());
  let bottom = bottom.min(image.height());
  if x >= right || y >= bottom {
    return RgbaImage::new(0, 0);
  }

  let mut crop = RgbaImage::new(right - x, bottom - y);
  for crop_y in 0..crop.height() {
    for crop_x in 0..crop.width() {
      crop.put_pixel(crop_x, crop_y, *image.get_pixel(x + crop_x, y + crop_y));
    }
  }
  crop
}

#[cfg(target_os = "macos")]
fn draw_overlay(image: &mut RgbaImage, sidebar_bounds: ViewBounds, observation: &SidebarViewportObservation) {
  draw_rect(image, sidebar_bounds, Rgba([255, 64, 64, 255]), 3);
  for evidence in &observation.evidence_nodes {
    if let Some(bounds) = evidence.bounds {
      draw_rect(image, bounds, Rgba([64, 160, 255, 255]), 2);
    }
  }
  for candidate in &observation.candidates {
    if let Some(bounds) = candidate.bounds {
      let color = match candidate.kind {
        SidebarCandidateKind::SectionHeader => Rgba([255, 210, 64, 255]),
        SidebarCandidateKind::PlaylistItem => Rgba([64, 230, 120, 255]),
        SidebarCandidateKind::NavigationItem => Rgba([200, 120, 255, 255]),
        SidebarCandidateKind::Unknown => Rgba([160, 160, 160, 255]),
      };
      draw_rect(image, bounds, color, 3);
    }
  }
}

#[cfg(target_os = "macos")]
fn bounds_to_ratio(bounds: ViewBounds, capture: &Capture) -> RatioRect {
  let width = capture.bounds.size.width.max(1.0);
  let height = capture.bounds.size.height.max(1.0);
  RatioRect::new(bounds.x / width, bounds.y / height, bounds.width / width, bounds.height / height)
}

fn section_node(
  id: &str,
  kind: SidebarSectionKind,
  label: &str,
  candidate: &SidebarViewportCandidate,
  observation: &SidebarViewportObservation,
) -> ViewNodeRecord {
  ViewNodeRecord {
    id: id.to_string(),
    kind: ViewNodeKind::Section,
    domain_kind: Some(kind.domain_kind().to_string()),
    layout: Some(ViewLayout::VStack),
    label: Some(label.to_string()),
    bounds: candidate.bounds.unwrap_or_default(),
    scrollable: None,
    anchors: vec![ViewAnchor {
      id: format!("anchor.{id}"),
      label: label.to_string(),
      strength: AnchorStrength::Medium,
      bounds: candidate.bounds.unwrap_or_default(),
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    landmarks: vec![ViewLandmark {
      id: format!("landmark.{id}"),
      label: label.to_string(),
      landmark_use: LandmarkUse::SectionAssignment,
      bounds: candidate.bounds.unwrap_or_default(),
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    actions: vec![ViewAction::ObserveOnly],
    evidence: candidate_evidence(candidate, observation),
    children: Vec::new(),
  }
}

fn item_node(
  id: &str,
  anchor_id: &str,
  label: &str,
  candidate: &SidebarViewportCandidate,
  observation: &SidebarViewportObservation,
) -> ViewNodeRecord {
  let evidence = candidate_evidence(candidate, observation);
  let bounds = candidate.bounds.unwrap_or_default();

  ViewNodeRecord {
    id: id.to_string(),
    kind: ViewNodeKind::Item,
    domain_kind: Some("netease.playlist_item".to_string()),
    layout: Some(ViewLayout::HStack),
    label: Some(label.to_string()),
    bounds,
    scrollable: None,
    anchors: vec![ViewAnchor {
      id: anchor_id.to_string(),
      label: label.to_string(),
      strength: AnchorStrength::Strong,
      bounds,
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    landmarks: Vec::new(),
    actions: vec![ViewAction::Open, ViewAction::Select],
    evidence: Vec::new(),
    children: vec![ViewNodeRecord {
      id: format!("{id}.text"),
      kind: ViewNodeKind::Text,
      domain_kind: None,
      layout: None,
      label: Some(label.to_string()),
      bounds,
      scrollable: None,
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::ObserveOnly],
      evidence,
      children: Vec::new(),
    }],
  }
}

fn candidate_evidence(candidate: &SidebarViewportCandidate, observation: &SidebarViewportObservation) -> Vec<ViewEvidenceNode> {
  candidate.evidence_ids.iter().filter_map(|id| observation.evidence_nodes.iter().find(|node| node.id == *id).cloned()).collect()
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
