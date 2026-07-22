//! NetEase Music product CLI library: sidebar playlist scan + agent-callable output.

pub mod app;
pub mod cli;
pub mod commands;
pub mod interaction;
pub mod invoke;
pub mod output;
pub mod recording;
pub mod scroll;
pub mod view_memory;
pub mod view_parsers;
pub mod views;
pub mod windows;

pub use commands::daily_recommended::{run_daily_recommended_play, run_daily_recommended_songs_scan};
pub use commands::launch::{LaunchResult, LaunchStep, OpenWindowInputs, run_open_window};
pub use commands::playback::{
  PlaybackStatus, PlaybackStatusHumanReadable, PlaybackStatusInputs, PlaybackStatusJson, run_playback_status_probe,
};
pub use commands::playlist::{
  PlaylistPlayResult, PlaylistPlayStep, PlaylistPlayVerification, PlaylistSelectResult, PlaylistSelectStep, PlaylistSelectVerification,
  run_playlist_play, run_playlist_play_candidate_id, run_playlist_select,
};
pub use commands::transport::{TransportAction, TransportInputs, TransportResult, run_transport_action};
pub use interaction::{InteractionEvent, InteractionEventKind, InteractionPhase, ScrollDirection, ScrollInteraction};
pub use view_parsers::sidebar::live::{run_live_scan, run_live_scan_until_query};
pub use views::player::PlaybackControlState;
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
use auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot;
#[cfg(target_os = "macos")]
use auv_driver_macos::types::ObservedAxNode;
#[cfg(target_os = "macos")]
use auv_view::draw_rect;

pub const DEFAULT_APP_ID: &str = "com.netease.163music";
pub const DEFAULT_ARTIFACT_DIR: &str = "/tmp/auv-netease-playlist-ls-artifacts";
pub const DEFAULT_DAILY_RECOMMENDED_ARTIFACT_DIR: &str = "/tmp/auv-netease-play-daily-recommended-artifacts";
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
  pub artifact_dir: PathBuf,
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
      artifact_dir: PathBuf::from(DEFAULT_ARTIFACT_DIR),
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
  pub artifact_dir: PathBuf,
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
      artifact_dir: PathBuf::from(DEFAULT_DAILY_RECOMMENDED_ARTIFACT_DIR),
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
pub struct DailyRecommendedPlayResult {
  pub command: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub steps: Vec<DailyRecommendedPlayStep>,
  pub verification: DailyRecommendedVerification,
  pub artifacts: Vec<String>,
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
    writeln!(f, "steps:")?;
    for step in &result.steps {
      writeln!(
        f,
        "  - {} target={} delivery={}",
        step.name,
        optional(step.target_label.as_deref()),
        optional(step.delivery_path.as_deref())
      )?;
    }
    writeln!(
      f,
      "verification: {}{}",
      result.verification.status,
      result.verification.best_score.map(|score| format!(" best_score={score:.3}")).unwrap_or_default()
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

#[derive(Clone, Debug, PartialEq)]
pub struct SongListInputs {
  pub app_id: String,
  pub artifact_dir: PathBuf,
  pub max_scrolls: usize,
  pub scroll_amount: f64,
  pub scroll_settle_ms: u64,
  pub ocr_options: TextRecognitionOptions,
}

impl SongListInputs {
  pub fn with_defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      artifact_dir: PathBuf::from("/tmp/auv-netease-song-list-artifacts"),
      max_scrolls: DEFAULT_MAX_SCROLLS,
      scroll_amount: 520.0,
      scroll_settle_ms: DEFAULT_SCROLL_SETTLE_MS,
      ocr_options: TextRecognitionOptions::default(),
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
  pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SongListObservation {
  pub observation_index: usize,
  pub source_artifact: Option<String>,
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
pub struct DailyRecommendedPlayStep {
  pub name: String,
  pub target_label: Option<String>,
  pub target_bounds: Option<ViewBounds>,
  pub delivery_path: Option<String>,
  pub fallback_reason: Option<String>,
  pub artifact: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DailyRecommendedVerification {
  pub status: String,
  pub method: String,
  pub template: Option<String>,
  pub control_state: Option<PlaybackControlState>,
  pub observed_bottom_text: Option<String>,
  pub match_count: usize,
  pub best_score: Option<f64>,
  pub artifact: Option<String>,
  pub note: Option<String>,
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
  /// Standalone interaction evidence for the product CLI.
  ///
  /// TODO(view-parser-trace-layout-v0): once this crate writes through AUV run
  /// storage, migrate these local events to `view.parse.observe.<index>` and
  /// `view.parse.scroll.<index>` spans instead of treating this field as the
  /// durable trace contract.
  #[serde(default)]
  interaction_events: Vec<InteractionEvent>,
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
      interaction_events: Vec::new(),
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
    let (observation_index, bounds) = playlist
      .item
      .candidate_id
      .as_deref()
      .and_then(|candidate_id| self.candidate_bounds(candidate_id))
      .map(|(index, bounds)| (Some(index), Some(bounds)))
      .unwrap_or((None, None));

    Ok(PlaylistSelectTarget {
      label: playlist.item.label.clone(),
      section_id: playlist.section.id.clone(),
      section_kind: playlist.section.kind,
      item_id: playlist.item.id.clone(),
      anchor_id: playlist.item.anchor_id.clone(),
      candidate_id: playlist.item.candidate_id.clone(),
      observation_index,
      bounds,
    })
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
        let (observation_index, bounds) =
          self.candidate_bounds(candidate_id).map(|(index, bounds)| (Some(index), Some(bounds))).unwrap_or((None, None));
        return Ok(PlaylistSelectTarget {
          label: item.label.clone(),
          section_id: section.id.clone(),
          section_kind: section.kind,
          item_id: item.id.clone(),
          anchor_id: item.anchor_id.clone(),
          candidate_id: item.candidate_id.clone(),
          observation_index,
          bounds,
        });
      }
    }

    Err(format!("no playlist candidate_id matched {candidate_id:?} in the canonical playlist scan"))
  }

  pub fn to_human_readable(&self) -> PlaylistSidebarHumanSummary<'_> {
    PlaylistSidebarHumanSummary { scan: self }
  }

  #[cfg(test)]
  pub(crate) fn from_projection_for_tests(projection: PlaylistSidebarProjection) -> Self {
    let mut scan = Self::empty(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default());
    scan.projection = projection;
    scan
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
  /// Local artifact paths written by the standalone NetEase CLI.
  ///
  /// TODO(view-artifact-ref-v1): replace these path strings with
  /// `contract::ArtifactRef` only after this crate writes through AUV run
  /// storage. Pulling the root contract into this app crate now would invert
  /// the intended crate boundary.
  source_artifacts: Vec<String>,
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
pub fn decode_playlist_sidebar_scan_json(input: &str) -> Result<PlaylistSidebarScan, String> {
  let value: serde_json::Value = serde_json::from_str(input).map_err(|error| format!("invalid playlist sidebar scan JSON: {error}"))?;
  let schema_version = value
    .get("schema_version")
    .and_then(serde_json::Value::as_str)
    .ok_or_else(|| "playlist sidebar scan JSON is missing schema_version".to_string())?;
  if schema_version != VIEW_IR_SCHEMA_VERSION {
    return Err(format!("unsupported playlist sidebar scan schema_version {schema_version:?}; expected {VIEW_IR_SCHEMA_VERSION:?}"));
  }

  serde_json::from_value(value).map_err(|error| format!("invalid playlist sidebar scan shape: {error}"))
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
fn delivery_path_label(path: auv_driver::InputDeliveryPath) -> &'static str {
  match path {
    auv_driver::InputDeliveryPath::Noop => "noop",
    auv_driver::InputDeliveryPath::AxPress => "ax_press",
    auv_driver::InputDeliveryPath::AxFocus => "ax_focus",
    auv_driver::InputDeliveryPath::AxSetValue => "ax_set_value",
    auv_driver::InputDeliveryPath::AxScroll => "ax_scroll",
    auv_driver::InputDeliveryPath::AxSelectedText => "ax_selected_text",
    auv_driver::InputDeliveryPath::WindowTargetedMouse => "window_targeted_mouse",
    auv_driver::InputDeliveryPath::WindowTargetedWheel => "window_targeted_wheel",
    auv_driver::InputDeliveryPath::WindowTargetedKeyboard => "window_targeted_keyboard",
    auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll => "window_targeted_keyboard_scroll",
    auv_driver::InputDeliveryPath::ClipboardPaste => "clipboard_paste",
    auv_driver::InputDeliveryPath::ForegroundSystemEvents => "foreground_system_events",
    auv_driver::InputDeliveryPath::Unsupported => "unsupported",
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
mod tests {
  use super::*;

  #[test]
  fn playlist_select_target_resolves_candidate_bounds_from_scan_observation() {
    let candidate_id = "obs2.candidate.ocr1.human_machine";
    let bounds = ViewBounds::new(71.0, 166.0, 72.0, 15.0);
    let mut scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "section-created".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("创建的歌单".to_string()),
        items: vec![PlaylistSidebarItem {
          id: "item-human-machine".to_string(),
          label: "人造器械".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: Some(candidate_id.to_string()),
          anchor_id: Some("anchor-human-machine".to_string()),
        }],
      }],
    });
    scan.observations.push(SidebarViewportObservation {
      observation_index: 2,
      candidates: vec![SidebarViewportCandidate {
        id: candidate_id.to_string(),
        kind: SidebarCandidateKind::PlaylistItem,
        label: Some("人造器械".to_string()),
        bounds: Some(bounds),
        evidence_ids: Vec::new(),
        confidence: Confidence::High,
      }],
      ..SidebarViewportObservation::default()
    });

    let target = scan.select_target("人造").expect("single playlist match should resolve");

    assert_eq!(target.label, "人造器械");
    assert_eq!(target.item_id, "item-human-machine");
    assert_eq!(target.anchor_id.as_deref(), Some("anchor-human-machine"));
    assert_eq!(target.observation_index, Some(2));
    assert_eq!(target.bounds, Some(bounds));
  }

  #[test]
  fn playlist_select_target_resolves_by_candidate_id() {
    let candidate_id = "obs6.candidate.ocr4.trance_vol_2";
    let bounds = ViewBounds::new(72.0, 492.0, 148.0, 16.0);
    let mut scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "section-favorite".to_string(),
        kind: SidebarSectionKind::FavoritePlaylists,
        label: Some("收藏的歌单".to_string()),
        items: vec![PlaylistSidebarItem {
          id: "item-trance-vol-2".to_string(),
          label: "我喜欢的风格 | Trance Vol.2".to_string(),
          section_hint: Some(SidebarSectionKind::FavoritePlaylists),
          confidence: Confidence::High,
          candidate_id: Some(candidate_id.to_string()),
          anchor_id: Some("anchor-trance-vol-2".to_string()),
        }],
      }],
    });
    scan.observations.push(SidebarViewportObservation {
      observation_index: 6,
      candidates: vec![SidebarViewportCandidate {
        id: candidate_id.to_string(),
        kind: SidebarCandidateKind::PlaylistItem,
        label: Some("我喜欢的风格 | Trance Vol.2".to_string()),
        bounds: Some(bounds),
        evidence_ids: Vec::new(),
        confidence: Confidence::High,
      }],
      ..SidebarViewportObservation::default()
    });

    let target = scan.select_target_by_candidate_id(candidate_id).expect("candidate id should resolve");

    assert_eq!(target.label, "我喜欢的风格 | Trance Vol.2");
    assert_eq!(target.candidate_id.as_deref(), Some(candidate_id));
    assert_eq!(target.observation_index, Some(6));
    assert_eq!(target.bounds, Some(bounds));
  }

  #[test]
  fn playlist_select_target_prefers_exact_numeric_label() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "section-created".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("创建的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "item-43".to_string(),
            label: "43".to_string(),
            section_hint: Some(SidebarSectionKind::MyPlaylists),
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
          PlaylistSidebarItem {
            id: "item-3".to_string(),
            label: "3".to_string(),
            section_hint: Some(SidebarSectionKind::MyPlaylists),
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    });

    let target = scan.select_target("3").expect("exact numeric match");
    assert_eq!(target.label, "3");
    assert_eq!(target.item_id, "item-3");
  }

  #[test]
  fn playlist_select_target_reports_ambiguous_contains_numeric_query() {
    let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "section-created".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("创建的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "item-43".to_string(),
            label: "43".to_string(),
            section_hint: Some(SidebarSectionKind::MyPlaylists),
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
          PlaylistSidebarItem {
            id: "item-13".to_string(),
            label: "13".to_string(),
            section_hint: Some(SidebarSectionKind::MyPlaylists),
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    });

    let error = scan.select_target("3").expect_err("ambiguous contains");
    assert!(error.contains("matched 2 items"));
  }
}
