//! NetEase Music product CLI library: sidebar playlist scan + agent-callable output.

pub mod cli;
pub mod output;
pub mod scroll;

use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use crate::scroll::policies::detection_motion::{MotionDetectionPolicy, MotionEvidence};
use auv_driver::vision::{TextRecognition, TextRecognitionOptions};
// Framework view-parser IR types, utilities, and the `ViewObserver` trait
// live in `auv-view` so other app crates (future QQ Music, etc.) can build
// on the same vocabulary without duplicating the records or re-defining the
// observer contract. Domain types (`PlaylistSidebarScan`, `SidebarSection`,
// the `Sidebar*` candidate flavors, the scan-loop functions) stay in this
// crate because they consume NetEase-shaped observations.
use auv_driver::RatioRect;
use auv_view::{
  AnchorStrength, BoundaryConfidence, CandidateRole, Confidence, LandmarkUse, ParserDiagnostic,
  ReconstructionOutput, ReconstructionPolicy, ScanAppContext, ScanOptions, ScanWindowContext,
  ScrollBoundarySummary, TopSeekOutcome, VIEW_IR_SCHEMA_VERSION, ViewAction, ViewAnchor, ViewAxis,
  ViewBounds, ViewEvidenceNode, ViewEvidenceSource, ViewLandmark, ViewLayout, ViewNodeKind,
  ViewNodeRecord, ViewObservation, ViewObserver, ViewReconstructionRecord, ViewRegionRecord,
  ViewScrollable, ViewViewportRecord, confidence_from_ocr, draw_rect, normalize_identity,
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
use auv_driver::{
  ActivationPolicy, Click, ClickOptions, Driver, InputPolicy, PrepareForInputOptions, Scroll,
  ScrollOptions, Size, WindowClickStrategy, WindowPoint,
};
#[cfg(target_os = "macos")]
use auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot;
#[cfg(target_os = "macos")]
use auv_driver_macos::types::ObservedAxNode;
#[cfg(target_os = "macos")]
use auv_driver_macos::{MacosDriver, MacosDriverSession};

pub const DEFAULT_APP_ID: &str = "com.netease.163music";
pub const DEFAULT_ARTIFACT_DIR: &str = "/tmp/auv-netease-playlist-ls-artifacts";
pub const DEFAULT_DAILY_RECOMMENDED_ARTIFACT_DIR: &str =
  "/tmp/auv-netease-play-daily-recommended-artifacts";
// TODO(netease-scroll-completion): this conservative default is only a
// product-agnostic safety cap, not an account-size estimate or completion
// policy. Full playlist enumeration should derive its budget from section
// counts or stronger scroll-state evidence when that slice is owner-approved.
pub const DEFAULT_MAX_SCROLLS: usize = 12;
// NOTICE(netease-scroll-settle): NetEase sidebar scrolls settle quickly in
// observed captures. Keep the default below generic desktop-action waits so
// playlist listing remains interactive; raise via --scroll-settle-ms if OCR
// evidence becomes unstable on slower machines.
pub const DEFAULT_SCROLL_SETTLE_MS: u64 = 250;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistCategory {
  #[default]
  All,
  Created,
  Favorite,
}

pub(crate) fn positive_scroll_amount(raw: &str) -> Result<f64, String> {
  let parsed = raw
    .parse::<f64>()
    .map_err(|_| "expects a number".to_string())?;
  if !parsed.is_finite() || parsed <= 0.0 {
    return Err("must be greater than 0".to_string());
  }
  Ok(parsed)
}

pub(crate) fn zero_to_one(raw: &str) -> Result<f64, String> {
  let parsed = raw
    .parse::<f64>()
    .map_err(|_| "expects a number".to_string())?;
  if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
    return Err("must be between 0 and 1".to_string());
  }
  Ok(parsed)
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
      artifact_dir: std::path::PathBuf::from(DEFAULT_ARTIFACT_DIR),
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
  pub fn human_summary(&self) -> DailyRecommendedHumanSummary<'_> {
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
    writeln!(
      f,
      "app: id={} name={}",
      optional(result.app.app_id.as_deref()),
      optional(result.app.name.as_deref())
    )?;
    writeln!(
      f,
      "window: title={}",
      optional(result.window.title.as_deref())
    )?;
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
      result
        .verification
        .best_score
        .map(|score| format!(" best_score={score:.3}"))
        .unwrap_or_default()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackControlState {
  PlayVisible,
  PauseVisible,
  Unknown,
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

impl PlaylistSidebarScan {
  fn empty(
    app: ScanAppContext,
    window: ScanWindowContext,
    sidebar_region: ViewRegionRecord,
  ) -> Self {
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

  pub fn human_summary(&self) -> PlaylistSidebarHumanSummary<'_> {
    PlaylistSidebarHumanSummary { scan: self }
  }

  #[cfg(test)]
  pub(crate) fn from_projection_for_tests(projection: PlaylistSidebarProjection) -> Self {
    let mut scan = Self::empty(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
    );
    scan.projection = projection;
    scan
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
        writeln!(
          f,
          "  - {} [{:?}]",
          optional(section.label.as_deref()),
          section.kind
        )?;
        if section.items.is_empty() {
          writeln!(f, "    (no items)")?;
        } else {
          for item in &section.items {
            writeln!(
              f,
              "    - {} confidence={:?} anchor={}",
              item.label,
              item.confidence,
              optional(item.anchor_id.as_deref())
            )?;
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
          diagnostic
            .node_id
            .as_deref()
            .map(|node_id| format!(" node={node_id}"))
            .unwrap_or_default()
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
  fn from_label(label: &str) -> Self {
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

  fn is_known(self) -> bool {
    self != Self::Unknown
  }

  fn is_playlist_collection(self) -> bool {
    matches!(self, Self::MyPlaylists | Self::FavoritePlaylists)
  }

  fn domain_kind(self) -> &'static str {
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InteractionEvent {
  pub event_index: usize,
  pub phase: InteractionPhase,
  pub kind: InteractionEventKind,
  pub observation_index: Option<usize>,
  pub from_observation: Option<usize>,
  pub to_observation: Option<usize>,
  pub viewport_fingerprint: Option<String>,
  pub scroll: Option<ScrollInteraction>,
  pub motion: Option<MotionEvidence>,
  pub artifacts: Vec<String>,
  pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionPhase {
  TopSeek,
  #[default]
  Collect,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionEventKind {
  Probe,
  #[default]
  Observe,
  InputScroll,
  StopDecision,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollInteraction {
  pub axis: ViewAxis,
  pub direction: ScrollDirection,
  pub requested_delta: f64,
  pub policy: String,
  pub delivery_path: Option<String>,
  pub motion: Option<MotionEvidence>,
  pub settle_ms: u64,
  pub anchor: Option<ViewBounds>,
  pub detected_boundary: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
  Up,
  #[default]
  Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarScrollbarBoundary {
  Top,
  Bottom,
  Interior,
}

pub(crate) fn split_csv(value: &str) -> Vec<String> {
  value
    .split(',')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(ToOwned::to_owned)
    .collect()
}

pub(crate) fn push_trimmed(values: &mut Vec<String>, value: String) {
  let value = value.trim();
  if !value.is_empty() && !values.iter().any(|existing| existing == value) {
    values.push(value.to_string());
  }
}

pub(crate) fn push_csv(values: &mut Vec<String>, value: &str) {
  for part in split_csv(value) {
    push_trimmed(values, part);
  }
}

pub(crate) fn push_ocr_language(options: &mut TextRecognitionOptions, language: String) {
  let language = language.trim();
  if language.is_empty() {
    return;
  }
  let languages = options.recognition_languages.get_or_insert_with(Vec::new);
  if !languages.iter().any(|existing| existing == language) {
    languages.push(language.to_string());
  }
}

pub(crate) fn load_custom_words_file(
  values: &mut Vec<String>,
  path: PathBuf,
) -> Result<(), String> {
  let content = std::fs::read_to_string(&path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  for line in content.lines() {
    let word = line.trim();
    if !word.is_empty() && !word.starts_with('#') {
      push_trimmed(values, word.to_string());
    }
  }
  Ok(())
}

pub(crate) fn parse_ratio_region(value: String) -> Result<RatioRect, String> {
  let parts = value
    .split(',')
    .map(str::trim)
    .map(|part| {
      part
        .parse::<f64>()
        .map_err(|_| "--sidebar-region expects x,y,width,height".to_string())
    })
    .collect::<Result<Vec<_>, _>>()?;

  if parts.len() != 4 {
    return Err("--sidebar-region expects x,y,width,height".to_string());
  }

  if parts.iter().any(|part| !part.is_finite()) {
    return Err("--sidebar-region expects finite x,y,width,height".to_string());
  }

  if parts[2] <= 0.0 || parts[3] <= 0.0 {
    return Err("--sidebar-region width and height must be greater than 0".to_string());
  }

  Ok(RatioRect::new(parts[0], parts[1], parts[2], parts[3]))
}

/// Decode a stored playlist sidebar scan artifact and reject unknown wire
/// shapes before interpreting the app-specific fields.
pub fn decode_playlist_sidebar_scan_json(input: &str) -> Result<PlaylistSidebarScan, String> {
  let value: serde_json::Value = serde_json::from_str(input)
    .map_err(|error| format!("invalid playlist sidebar scan JSON: {error}"))?;
  let schema_version = value
    .get("schema_version")
    .and_then(serde_json::Value::as_str)
    .ok_or_else(|| "playlist sidebar scan JSON is missing schema_version".to_string())?;
  if schema_version != VIEW_IR_SCHEMA_VERSION {
    return Err(format!(
      "unsupported playlist sidebar scan schema_version {schema_version:?}; expected {VIEW_IR_SCHEMA_VERSION:?}"
    ));
  }

  serde_json::from_value(value)
    .map_err(|error| format!("invalid playlist sidebar scan shape: {error}"))
}

fn optional(value: Option<&str>) -> &str {
  value
    .filter(|value| !value.trim().is_empty())
    .unwrap_or("-")
}

fn render_optional_bounds(bounds: Option<ViewBounds>) -> String {
  bounds
    .map(|bounds| {
      format!(
        "x={:.1},y={:.1},w={:.1},h={:.1}",
        bounds.x, bounds.y, bounds.width, bounds.height
      )
    })
    .unwrap_or_else(|| "-".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_live_scan(_inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  Err("live NetEase playlist sidebar scan is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_daily_recommended_play(
  _inputs: &DailyRecommendedPlayInputs,
) -> Result<DailyRecommendedPlayResult, String> {
  Err("live NetEase daily recommended play is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_daily_recommended_play(
  inputs: &DailyRecommendedPlayInputs,
) -> Result<DailyRecommendedPlayResult, String> {
  std::fs::create_dir_all(&inputs.artifact_dir).map_err(|error| {
    format!(
      "failed to create {}: {error}",
      inputs.artifact_dir.display()
    )
  })?;

  let driver = MacosDriver::new();
  let session = driver
    .open_local()
    .map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(app))
    .map_err(|error| format!("failed to resolve NetEase window: {error}"))?;

  let app_context = ScanAppContext {
    app_id: window
      .app_bundle_id
      .clone()
      .or_else(|| Some(inputs.app_id.clone())),
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(
      0.0,
      0.0,
      window.frame.size.width,
      window.frame.size.height,
    )),
  };

  let mut run = DailyRecommendedRun {
    session,
    window,
    inputs,
    steps: Vec::new(),
    artifacts: Vec::new(),
    diagnostics: Vec::new(),
    known_limits: Vec::new(),
  };

  run.scroll_sidebar_to_top();
  run.click_text("select-sidebar-recommend", "推荐", |bounds, size| {
    bounds.x < size.width * 0.28
  })?;
  run.open_daily_recommended()?;
  run.click_text("click-play-all", "播放全部", |bounds, _| bounds.y > 0.0)?;
  let mut verification = run.verify_play_icon()?;
  if verification.status != "passed" {
    run.known_limits.push(
      "window-targeted Play All click did not verify playback; retried with foreground click"
        .to_string(),
    );
    run.click_text_foreground(
      "click-play-all-foreground-retry",
      "播放全部",
      |bounds, _| bounds.y > 0.0,
    )?;
    verification = run.verify_play_icon()?;
  }

  Ok(DailyRecommendedPlayResult {
    command: "playlist.play.daily-recommended".to_string(),
    app: app_context,
    window: window_context,
    steps: run.steps,
    verification,
    artifacts: run.artifacts,
    diagnostics: run.diagnostics,
    known_limits: run.known_limits,
  })
}

#[cfg(target_os = "macos")]
struct DailyRecommendedRun<'a> {
  session: MacosDriverSession,
  window: auv_driver::Window,
  inputs: &'a DailyRecommendedPlayInputs,
  steps: Vec<DailyRecommendedPlayStep>,
  artifacts: Vec<String>,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

#[cfg(target_os = "macos")]
impl DailyRecommendedRun<'_> {
  fn scroll_sidebar_to_top(&mut self) {
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let bounds = broad_sidebar_probe_bounds(window_size);
    let anchor = WindowPoint::new(
      bounds.x + bounds.width * 0.5,
      bounds.y + bounds.height * 0.45,
    );
    for index in 0..self.inputs.max_top_scrolls {
      match self.session.window().scroll(
        &self.window,
        anchor,
        Scroll::new(0.0, self.inputs.top_scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(self.inputs.settle_ms),
          ..ScrollOptions::default()
        },
      ) {
        Ok(result) => self.steps.push(DailyRecommendedPlayStep {
          name: format!("scroll-sidebar-top-{index}"),
          target_label: None,
          target_bounds: Some(bounds),
          delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
          fallback_reason: result.fallback_reason,
          artifact: None,
        }),
        Err(error) => {
          self.diagnostics.push(ParserDiagnostic {
            code: "daily_recommended_top_scroll_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          });
          self
            .known_limits
            .push("top seek stopped early after a typed scroll failure".to_string());
          break;
        }
      }
    }
  }

  fn click_text(
    &mut self,
    step_name: &str,
    query: &str,
    guard: impl Fn(ViewBounds, Size) -> bool,
  ) -> Result<(), String> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("{step_name}: capture failed: {error}"))?;
    let artifact = self.write_capture_artifact(step_name, &capture)?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        RatioRect::new(0.0, 0.0, 1.0, 1.0),
        self.inputs.ocr_options.clone(),
      )
      .map_err(|error| format!("{step_name}: OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) = best_text_match(&recognition, query, window_size, guard) else {
      return Err(format!("{step_name}: text {query:?} was not found"));
    };
    let bounds = ViewBounds::new(
      target.bounds.origin.x,
      target.bounds.origin.y,
      target.bounds.size.width,
      target.bounds.size.height,
    );
    let point = target.action_point();
    let result = self
      .session
      .window()
      .click(
        &self.window,
        WindowPoint::new(point.x, point.y),
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click: Click::Single,
          window_strategy: WindowClickStrategy::ChromiumCompatible,
        },
      )
      .map_err(|error| format!("{step_name}: click failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    self.steps.push(DailyRecommendedPlayStep {
      name: step_name.to_string(),
      target_label: Some(target.text.clone()),
      target_bounds: Some(bounds),
      delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
      fallback_reason: result.fallback_reason,
      artifact: Some(artifact),
    });
    Ok(())
  }

  fn click_text_foreground(
    &mut self,
    step_name: &str,
    query: &str,
    guard: impl Fn(ViewBounds, Size) -> bool,
  ) -> Result<(), String> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("{step_name}: capture failed: {error}"))?;
    let artifact = self.write_capture_artifact(step_name, &capture)?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        RatioRect::new(0.0, 0.0, 1.0, 1.0),
        self.inputs.ocr_options.clone(),
      )
      .map_err(|error| format!("{step_name}: OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) = best_text_match(&recognition, query, window_size, guard) else {
      return Err(format!("{step_name}: text {query:?} was not found"));
    };
    let bounds = ViewBounds::new(
      target.bounds.origin.x,
      target.bounds.origin.y,
      target.bounds.size.width,
      target.bounds.size.height,
    );
    let point = target.action_point();
    let screen_point = self
      .session
      .window()
      .to_screen_point(&self.window, WindowPoint::new(point.x, point.y))
      .map_err(|error| format!("{step_name}: screen point projection failed: {error}"))?;
    let lease = self
      .session
      .window()
      .prepare_for_input(
        &self.window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(self.inputs.settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("{step_name}: foreground preparation failed: {error}"))?;
    let click_result = self
      .session
      .input()
      .click_at(screen_point.point(), Click::Single);
    let restore_result = self.session.window().restore_input(lease);
    click_result.map_err(|error| format!("{step_name}: foreground click failed: {error}"))?;
    restore_result.map_err(|error| format!("{step_name}: foreground restore failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    self.steps.push(DailyRecommendedPlayStep {
      name: step_name.to_string(),
      target_label: Some(target.text),
      target_bounds: Some(bounds),
      delivery_path: Some("foreground_system_events".to_string()),
      fallback_reason: Some("window-targeted click did not verify playback".to_string()),
      artifact: Some(artifact),
    });
    Ok(())
  }

  fn open_daily_recommended(&mut self) -> Result<(), String> {
    if self.play_all_is_visible(false)? {
      return Ok(());
    }

    self.click_daily_recommended_card_body()
  }

  fn click_daily_recommended_card_body(&mut self) -> Result<(), String> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("daily recommended card capture failed: {error}"))?;
    let artifact = self.write_capture_artifact("open-daily-recommended-card-body", &capture)?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        RatioRect::new(0.0, 0.0, 1.0, 1.0),
        self.inputs.ocr_options.clone(),
      )
      .map_err(|error| format!("daily recommended card OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) =
      best_text_match(&recognition, "每日推荐", window_size, |bounds, size| {
        bounds.x > size.width * 0.18 && bounds.y < size.height * 0.35
      })
    else {
      return Err("daily recommended card title was not found on recommendation home".to_string());
    };
    let bounds = ViewBounds::new(
      target.bounds.origin.x,
      target.bounds.origin.y,
      target.bounds.size.width,
      target.bounds.size.height,
    );
    let point = daily_recommended_card_click_point(bounds);
    let result = self
      .session
      .window()
      .click(
        &self.window,
        WindowPoint::new(point.x, point.y),
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click: Click::Single,
          window_strategy: WindowClickStrategy::ChromiumCompatible,
        },
      )
      .map_err(|error| format!("daily recommended card body click failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    self.steps.push(DailyRecommendedPlayStep {
      name: "open-daily-recommended-card-body".to_string(),
      target_label: Some(target.text),
      target_bounds: Some(bounds),
      delivery_path: Some(delivery_path_label(result.selected_path).to_string()),
      fallback_reason: result.fallback_reason,
      artifact: Some(artifact),
    });
    if self.play_all_is_visible(true)? {
      Ok(())
    } else {
      Err("daily recommended card body click did not reveal 播放全部".to_string())
    }
  }

  fn play_all_is_visible(&mut self, record_absent_diagnostic: bool) -> Result<bool, String> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("daily recommended fallback capture failed: {error}"))?;
    let artifact = self.write_capture_artifact("open-daily-recommended-fallback", &capture)?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        RatioRect::new(0.0, 0.0, 1.0, 1.0),
        self.inputs.ocr_options.clone(),
      )
      .map_err(|error| format!("daily recommended fallback OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let visible = best_text_match(&recognition, "播放全部", window_size, |bounds, size| {
      bounds.x > size.width * 0.18
    })
    .is_some();
    if visible {
      self
        .known_limits
        .push("Play All was visible while opening Daily Recommended".to_string());
    } else if record_absent_diagnostic {
      self.diagnostics.push(ParserDiagnostic {
        code: "daily_recommended_fallback_not_visible".to_string(),
        message: "neither 每日推荐 nor 播放全部 could be detected".to_string(),
        node_id: None,
      });
    }
    self.steps.push(DailyRecommendedPlayStep {
      name: "open-daily-recommended-fallback-observe".to_string(),
      target_label: Some("播放全部".to_string()),
      target_bounds: None,
      delivery_path: None,
      fallback_reason: None,
      artifact: Some(artifact),
    });
    Ok(visible)
  }

  fn write_capture_artifact(
    &mut self,
    step_name: &str,
    capture: &Capture,
  ) -> Result<String, String> {
    let path = self.inputs.artifact_dir.join(format!("{step_name}.png"));
    capture
      .image
      .save(&path)
      .map_err(|error| format!("failed to save {}: {error}", path.display()))?;
    let rendered = path.display().to_string();
    self.artifacts.push(rendered.clone());
    Ok(rendered)
  }

  fn verify_play_icon(&mut self) -> Result<DailyRecommendedVerification, String> {
    let Some(template) = self.inputs.play_icon_template.as_ref() else {
      return self.verify_bottom_playback_control();
    };
    if !template.exists() {
      return Err(format!("icon template not found: {}", template.display()));
    }

    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("post-click icon capture failed: {error}"))?;
    let screenshot = self.write_capture_artifact("post-click-icon-match", &capture)?;
    let scale = if capture.scale_factor.is_finite() && capture.scale_factor > 0.0 {
      capture.scale_factor
    } else {
      1.0
    };
    let region = auv_driver_macos::types::ObservedRect {
      x: ((capture.image.width() as f64) * 0.30).round() as i64,
      y: ((capture.image.height() as f64) * 0.72).round() as i64,
      width: ((capture.image.width() as f64) * 0.40).round() as i64,
      height: ((capture.image.height() as f64) * 0.24).round() as i64,
    };
    let output = auv_driver_macos::support::template_match::match_template(
      std::path::Path::new(&screenshot),
      template,
      Some(&region),
      self.inputs.play_icon_threshold,
    )?;
    let best_score = output.matches.first().map(|item| item.score);
    let match_count = output.matches.len();
    let verification_json = self.inputs.artifact_dir.join("post-click-icon-match.json");
    let payload = serde_json::json!({
      "template": template.display().to_string(),
      "threshold": self.inputs.play_icon_threshold,
      "match_count": match_count,
      "best_score": best_score,
      "window_scale_factor": scale,
      "search_region_pixels": {
        "x": region.x,
        "y": region.y,
        "width": region.width,
        "height": region.height,
      },
    });
    std::fs::write(
      &verification_json,
      serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize icon verification: {error}"))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", verification_json.display()))?;
    let verification_artifact = verification_json.display().to_string();
    self.artifacts.push(verification_artifact.clone());

    Ok(DailyRecommendedVerification {
      status: if match_count > 0 { "passed" } else { "failed" }.to_string(),
      method: "icon_match".to_string(),
      template: Some(template.display().to_string()),
      control_state: None,
      observed_bottom_text: None,
      match_count,
      best_score,
      artifact: Some(verification_artifact),
      note: Some(
        "icon match searches the bottom-center playback-control region after Play All".to_string(),
      ),
    })
  }

  fn verify_bottom_playback_control(&mut self) -> Result<DailyRecommendedVerification, String> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| format!("post-click playback-state capture failed: {error}"))?;
    let screenshot = self.write_capture_artifact("post-click-playback-state", &capture)?;
    let control_state = classify_bottom_playback_control_state(&capture.image);
    let bottom_text = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        RatioRect::new(0.0, 0.88, 0.46, 0.12),
        self.inputs.ocr_options.clone(),
      )
      .ok()
      .map(|recognition| recognition.text.trim().to_string())
      .filter(|text| !text.is_empty());
    let verification_json = self
      .inputs
      .artifact_dir
      .join("post-click-playback-state.json");
    let payload = serde_json::json!({
      "method": "bottom_control_icon",
      "control_state": control_state,
      "observed_bottom_text": bottom_text,
      "screenshot": screenshot,
    });
    std::fs::write(
      &verification_json,
      serde_json::to_string_pretty(&payload)
        .map_err(|error| format!("failed to serialize playback-state verification: {error}"))?,
    )
    .map_err(|error| format!("failed to write {}: {error}", verification_json.display()))?;
    let verification_artifact = verification_json.display().to_string();
    self.artifacts.push(verification_artifact.clone());

    Ok(DailyRecommendedVerification {
      status: if control_state == PlaybackControlState::PauseVisible {
        "passed"
      } else {
        "failed"
      }
      .to_string(),
      method: "bottom_control_icon".to_string(),
      template: None,
      control_state: Some(control_state),
      observed_bottom_text: bottom_text,
      match_count: 0,
      best_score: None,
      artifact: Some(verification_artifact),
      note: Some(
        "default verification checks the bottom playback control for a pause icon, which is title-independent under shuffle/random playback".to_string(),
      ),
    })
  }
}

#[cfg(target_os = "macos")]
fn best_text_match(
  recognition: &TextRecognition,
  query: &str,
  window_size: Size,
  guard: impl Fn(ViewBounds, Size) -> bool,
) -> Option<auv_driver::vision::RecognizedText> {
  recognition
    .regions
    .iter()
    .filter(|region| normalize_identity(&region.text).contains(&normalize_identity(query)))
    .filter(|region| {
      guard(
        ViewBounds::new(
          region.bounds.origin.x,
          region.bounds.origin.y,
          region.bounds.size.width,
          region.bounds.size.height,
        ),
        window_size,
      )
    })
    .min_by(|left, right| {
      left
        .bounds
        .origin
        .y
        .partial_cmp(&right.bounds.origin.y)
        .unwrap_or(std::cmp::Ordering::Equal)
    })
    .cloned()
}

fn classify_bottom_playback_control_state(image: &RgbaImage) -> PlaybackControlState {
  if image.width() < 80 || image.height() < 80 {
    return PlaybackControlState::Unknown;
  }

  let center_x = image.width() as i32 / 2;
  let center_y = image.height() as i32 - 38;
  let half_width = 24i32;
  let half_height = 22i32;
  let left = (center_x - half_width).max(0);
  let right = (center_x + half_width).min(image.width() as i32 - 1);
  let top = (center_y - half_height).max(0);
  let bottom = (center_y + half_height).min(image.height() as i32 - 1);
  let width = (right - left + 1).max(0) as usize;
  if width == 0 {
    return PlaybackControlState::Unknown;
  }

  let mut occupied_columns = vec![false; width];
  for y in top..=bottom {
    for x in left..=right {
      let pixel = image.get_pixel(x as u32, y as u32);
      if pixel[3] > 120 && pixel[0] > 220 && pixel[1] > 220 && pixel[2] > 220 {
        occupied_columns[(x - left) as usize] = true;
      }
    }
  }

  let mut clusters = Vec::new();
  let mut start = None;
  for (index, occupied) in occupied_columns.iter().copied().enumerate() {
    match (start, occupied) {
      (None, true) => start = Some(index),
      (Some(first), false) => {
        if index.saturating_sub(first) >= 2 {
          clusters.push((first, index - 1));
        }
        start = None;
      }
      _ => {}
    }
  }
  if let Some(first) = start {
    let last = occupied_columns.len() - 1;
    if last.saturating_sub(first) + 1 >= 2 {
      clusters.push((first, last));
    }
  }

  match clusters.len() {
    0 => PlaybackControlState::Unknown,
    1 => PlaybackControlState::PlayVisible,
    _ => PlaybackControlState::PauseVisible,
  }
}

fn daily_recommended_card_click_point(title_bounds: ViewBounds) -> auv_driver::Point {
  // NOTICE(netease-daily-card-hit-target): live NetEase testing showed the
  // OCR title text and bottom title strip on the recommendation card may not
  // activate navigation reliably. Target the cover/body area derived from the
  // title anchor until an owner-approved card geometry detector replaces this
  // local product policy.
  if title_bounds.y < 180.0 {
    auv_driver::Point::new(title_bounds.x + 55.0, title_bounds.y + 80.0)
  } else {
    auv_driver::Point::new(title_bounds.x + 70.0, title_bounds.y - 95.0)
  }
}

#[cfg(target_os = "macos")]
pub fn run_live_scan(inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  let driver = MacosDriver::new();
  let default_app_context = ScanAppContext {
    app_id: Some(inputs.app_id.clone()),
    name: None,
    version: None,
  };
  let mut session = match driver.open_local() {
    Ok(session) => session,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        default_app_context,
        ScanWindowContext::default(),
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "driver_open_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the macOS driver could not be opened",
      ));
    }
  };
  let app = App::bundle(inputs.app_id.clone());
  let window = match session
    .window()
    .resolve(Window::main_visible().owned_by(app))
  {
    Ok(window) => window,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        default_app_context,
        ScanWindowContext::default(),
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "target_window_not_found".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the target window could not be resolved",
      ));
    }
  };
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let app_context = ScanAppContext {
    app_id: window
      .app_bundle_id
      .clone()
      .or_else(|| Some(inputs.app_id.clone())),
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(
      0.0,
      0.0,
      window.frame.size.width,
      window.frame.size.height,
    )),
  };
  let mut pre_scan_diagnostics = Vec::new();
  let mut pre_scan_known_limits = Vec::new();

  let mut capture = match session.window().capture(&window) {
    Ok(capture) => capture,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        app_context,
        window_context,
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "window_capture_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the target window could not be captured",
      ));
    }
  };
  let full_window = RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let mut full_recognition = match session.vision().recognize_text_in_capture_with_options(
    &capture,
    full_window,
    inputs.ocr_options.clone(),
  ) {
    Ok(recognition) => recognition_in_window_space(recognition, &capture),
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        app_context,
        window_context,
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "full_window_ocr_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because full-window OCR failed",
      ));
    }
  };

  if let Some(diagnostic) = detect_blocking_modal(&full_recognition) {
    return Ok(PlaylistSidebarScan::empty_with_diagnostic(
      app_context,
      window_context,
      ViewRegionRecord::default(),
      diagnostic,
      "scan stopped before sidebar observation because a blocking modal was detected",
    ));
  }

  if inputs.sidebar_region.is_none() {
    if let Some(restore) = detect_default_screen_restore(&full_recognition, window_size) {
      if let Err(error) = click_default_screen_restore(&session, &window, restore.point) {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "default_screen_restore_failed".to_string(),
            message: format!(
              "failed to restore NetEase default sidebar screen from {:?}: {error}",
              restore.reason
            ),
            node_id: None,
          },
          "scan stopped before sidebar observation because the default screen restore click failed",
        ));
      }
      if inputs.scroll_settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
      }
      capture = match session.window().capture(&window) {
        Ok(capture) => capture,
        Err(error) => {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "window_capture_failed".to_string(),
              message: error.to_string(),
              node_id: None,
            },
            "scan stopped before sidebar observation because the target window could not be captured after default screen restore",
          ));
        }
      };
      full_recognition = match session.vision().recognize_text_in_capture_with_options(
        &capture,
        full_window,
        inputs.ocr_options.clone(),
      ) {
        Ok(recognition) => recognition_in_window_space(recognition, &capture),
        Err(error) => {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "full_window_ocr_failed".to_string(),
              message: error.to_string(),
              node_id: None,
            },
            "scan stopped before sidebar observation because full-window OCR failed after default screen restore",
          ));
        }
      };
    }
  }

  if inputs.sidebar_region.is_none() {
    let broad_sidebar_bounds = broad_sidebar_probe_bounds(window_size);
    let broad_sidebar_ratio = bounds_to_ratio(broad_sidebar_bounds, &capture);
    let mut top_probe = LiveSidebarObserver {
      session,
      window: window.clone(),
      sidebar_bounds: broad_sidebar_bounds,
      sidebar_ratio: broad_sidebar_ratio,
      ocr_options: inputs.ocr_options.clone(),
      artifact_dir: inputs.artifact_dir.clone(),
      pending_artifacts: Vec::new(),
      scroll_amount: inputs.scroll_amount,
      scroll_settle_ms: inputs.scroll_settle_ms,
      pending_scroll_delivery_path: None,
      previous_sidebar_crop: None,
      motion_policy: MotionDetectionPolicy::default(),
    };
    let top_seek = scroll_to_top_by_motion(&mut top_probe, inputs.max_scrolls);
    pre_scan_diagnostics.extend(top_seek.diagnostics);
    pre_scan_known_limits.extend(top_seek.known_limits);
    let LiveSidebarObserver {
      session: probe_session,
      ..
    } = top_probe;
    session = probe_session;

    capture = match session.window().capture(&window) {
      Ok(capture) => capture,
      Err(error) => {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "window_capture_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          },
          "scan stopped before sidebar observation because the target window could not be captured after top seek",
        ));
      }
    };
    full_recognition = match session.vision().recognize_text_in_capture_with_options(
      &capture,
      full_window,
      inputs.ocr_options.clone(),
    ) {
      Ok(recognition) => recognition_in_window_space(recognition, &capture),
      Err(error) => {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "full_window_ocr_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          },
          "scan stopped before sidebar observation because full-window OCR failed after top seek",
        ));
      }
    };
  }

  let sidebar_region = match detect_sidebar_region(
    inputs.sidebar_region,
    window_size,
    &full_recognition,
  ) {
    Ok(sidebar_region) => sidebar_region,
    Err(diagnostic) => {
      if inputs.sidebar_region.is_none() && diagnostic.code == "sidebar_region_not_found" {
        // NOTICE(netease-default-screen-restore): song-detail and similar
        // transient NetEase surfaces hide the left sidebar. `playlist ls`
        // cannot proceed from that state, so this fallback uses the app's
        // top-left restore affordance once before giving up on sidebar
        // detection. Broader surface classification is deferred until the
        // NetEase preflight contract is owner-approved.
        let restore = DefaultScreenRestore {
          reason: DefaultScreenRestoreReason::MissingSidebarRegion,
          point: song_detail_restore_point(window_size),
        };
        if let Err(error) = click_default_screen_restore(&session, &window, restore.point) {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "default_screen_restore_failed".to_string(),
              message: format!(
                "failed to restore NetEase default sidebar screen from {:?}: {error}",
                restore.reason
              ),
              node_id: None,
            },
            "scan stopped before sidebar observation because the default screen restore click failed",
          ));
        }
        if inputs.scroll_settle_ms > 0 {
          std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
        }
        capture = match session.window().capture(&window) {
          Ok(capture) => capture,
          Err(error) => {
            return Ok(PlaylistSidebarScan::empty_with_diagnostic(
              app_context,
              window_context,
              ViewRegionRecord::default(),
              ParserDiagnostic {
                code: "window_capture_failed".to_string(),
                message: error.to_string(),
                node_id: None,
              },
              "scan stopped before sidebar observation because the target window could not be captured after sidebar restore fallback",
            ));
          }
        };
        full_recognition = match session.vision().recognize_text_in_capture_with_options(
          &capture,
          full_window,
          inputs.ocr_options.clone(),
        ) {
          Ok(recognition) => recognition_in_window_space(recognition, &capture),
          Err(error) => {
            return Ok(PlaylistSidebarScan::empty_with_diagnostic(
              app_context,
              window_context,
              ViewRegionRecord::default(),
              ParserDiagnostic {
                code: "full_window_ocr_failed".to_string(),
                message: error.to_string(),
                node_id: None,
              },
              "scan stopped before sidebar observation because full-window OCR failed after sidebar restore fallback",
            ));
          }
        };
        match detect_sidebar_region(None, window_size, &full_recognition) {
          Ok(sidebar_region) => sidebar_region,
          Err(diagnostic) => {
            return Ok(PlaylistSidebarScan::empty_with_diagnostic(
              app_context,
              window_context,
              ViewRegionRecord::default(),
              diagnostic,
              "scan stopped before sidebar observation because the sidebar region could not be detected after default screen restore",
            ));
          }
        }
      } else {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          diagnostic,
          "scan stopped before sidebar observation because the sidebar region could not be detected",
        ));
      }
    }
  };
  let sidebar_bounds = sidebar_region.bounds.unwrap_or_default();
  let sidebar_ratio = bounds_to_ratio(sidebar_bounds, &capture);
  let mut observer = LiveSidebarObserver {
    session,
    window: window.clone(),
    sidebar_bounds,
    sidebar_ratio,
    ocr_options: inputs.ocr_options.clone(),
    artifact_dir: inputs.artifact_dir.clone(),
    pending_artifacts: Vec::new(),
    scroll_amount: inputs.scroll_amount,
    scroll_settle_ms: inputs.scroll_settle_ms,
    pending_scroll_delivery_path: None,
    previous_sidebar_crop: None,
    motion_policy: MotionDetectionPolicy::default(),
  };
  let mut scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      // NOTICE: NetEase playlist listing no longer has a page completion
      // model. This shared `auv-view::ScanOptions` field remains for other
      // scan loops, but this crate's collection policy intentionally ignores
      // it and stops at section landmarks or scroll boundaries.
      max_pages: 0,
      max_scrolls: inputs.max_scrolls,
    },
    inputs.category,
    inputs.scroll_amount,
    inputs.scroll_settle_ms,
  );
  scan.diagnostics.extend(pre_scan_diagnostics);
  scan.known_limits.extend(pre_scan_known_limits);
  scan.diagnostics.extend(observer.finish_artifacts());
  scan.app = app_context;
  scan.window = window_context;
  scan.sidebar_region = sidebar_region;
  scan.reconstruction.root.bounds = sidebar_bounds;

  Ok(scan)
}

fn detect_sidebar_region(
  manual: Option<RatioRect>,
  window_size: auv_driver::Size,
  recognition: &TextRecognition,
) -> Result<ViewRegionRecord, ParserDiagnostic> {
  if let Some(region) = manual {
    return Ok(sidebar_region_record(ratio_to_window_bounds(
      region,
      window_size,
    )));
  }

  let left_limit = window_size.width * 0.38;
  let left_regions = recognition
    .regions
    .iter()
    .filter(|region| region.bounds.origin.x < left_limit)
    .collect::<Vec<_>>();
  let mut markers = left_regions
    .iter()
    .filter(|region| is_sidebar_marker(region.text.trim()))
    .map(|region| {
      (
        region.bounds.origin.x + region.bounds.size.width,
        region.bounds.origin.y,
        region.text.trim(),
      )
    })
    .collect::<Vec<_>>();

  if markers.is_empty() {
    return Err(ParserDiagnostic {
      code: "sidebar_region_not_found".to_string(),
      message: "sidebar markers could not be identified on the left side; refusing to infer sidebar bounds from unanchored list rows".to_string(),
      node_id: None,
    });
  }

  markers.sort_by(|left, right| {
    left
      .0
      .partial_cmp(&right.0)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  let max_x = markers
    .last()
    .map(|marker| marker.0)
    .unwrap_or_default()
    .max(window_size.width * 0.18)
    .min(window_size.width * 0.42);
  let y = markers
    .iter()
    .filter(|marker| is_playlist_section_marker(marker.2))
    .map(|marker| marker.1)
    .min_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
    .unwrap_or(0.0)
    .clamp(0.0, window_size.height);

  Ok(sidebar_region_record(ViewBounds::new(
    0.0,
    y,
    max_x + 48.0,
    playlist_sidebar_bottom(window_size) - y,
  )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DefaultScreenRestoreReason {
  SongDetailScreen,
  MissingSidebarRegion,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DefaultScreenRestore {
  reason: DefaultScreenRestoreReason,
  point: auv_driver::Point,
}

fn detect_default_screen_restore(
  recognition: &TextRecognition,
  window_size: auv_driver::Size,
) -> Option<DefaultScreenRestore> {
  if recognition.regions.iter().any(|region| {
    region.bounds.origin.x < window_size.width * 0.38 && is_sidebar_marker(region.text.trim())
  }) {
    return None;
  }

  let has_song_detail_controls =
    recognition.best_contains("评论").is_some() && recognition.best_contains("收藏").is_some();
  if !has_song_detail_controls {
    return None;
  }

  Some(DefaultScreenRestore {
    reason: DefaultScreenRestoreReason::SongDetailScreen,
    point: song_detail_restore_point(window_size),
  })
}

fn song_detail_restore_point(_window_size: auv_driver::Size) -> auv_driver::Point {
  // NOTICE: This is a learned window-local logical point for the song-detail
  // back affordance. The older heuristic point `(40, 48)` landed left and below
  // the actual clickable target in the live macOS client.
  auv_driver::Point::new(82.602, 16.336)
}

#[cfg(target_os = "macos")]
fn click_default_screen_restore(
  session: &MacosDriverSession,
  window: &auv_driver::Window,
  point: auv_driver::Point,
) -> Result<(), String> {
  let lease = session
    .window()
    .prepare_for_input(
      window,
      PrepareForInputOptions {
        activation: ActivationPolicy::Foreground {
          settle: std::time::Duration::ZERO,
        },
        preserve_frontmost: false,
        install_focus_guard: false,
        settle: std::time::Duration::ZERO,
      },
    )
    .map_err(|error| format!("foreground preparation failed: {error}"))?;
  let global_x = window.frame.origin.x + point.x;
  let global_y = window.frame.origin.y + point.y;
  // NOTICE: Route this restore through the foreground global HID path. Some
  // app-rendered affordances do not reliably react to typed/window-targeted
  // clicks; `click_point` carries the mouse-move + settle behavior that makes
  // this class of click observable to those controls.
  let click_result = auv_driver_macos::native::pointer::click_point(global_x, global_y, 0, 1, 80);
  let restore_result = session.window().restore_input(lease);
  click_result.map_err(|error| format!("foreground restore click failed: {error}"))?;
  restore_result.map_err(|error| format!("foreground restore cleanup failed: {error}"))?;
  Ok(())
}

fn playlist_sidebar_bottom(window_size: auv_driver::Size) -> f64 {
  (window_size.height - 82.0).clamp(0.0, window_size.height)
}

fn broad_sidebar_probe_bounds(window_size: auv_driver::Size) -> ViewBounds {
  let width = (window_size.width * 0.24)
    .max(280.0)
    .min(window_size.width * 0.42);
  ViewBounds::new(0.0, 0.0, width, playlist_sidebar_bottom(window_size))
}

fn sidebar_region_record(bounds: ViewBounds) -> ViewRegionRecord {
  ViewRegionRecord {
    id: None,
    name: Some("playlist_sidebar".to_string()),
    bounds: Some(bounds),
    coordinate_space: Some("window".to_string()),
  }
}

fn ratio_to_window_bounds(region: RatioRect, window_size: auv_driver::Size) -> ViewBounds {
  ViewBounds::new(
    region.x * window_size.width,
    region.y * window_size.height,
    region.width * window_size.width,
    region.height * window_size.height,
  )
}

fn is_sidebar_marker(label: &str) -> bool {
  SidebarSectionKind::from_label(label).is_known()
    || matches!(label, "推荐" | "发现音乐" | "最近播放")
}

fn is_playlist_section_marker(label: &str) -> bool {
  SidebarSectionKind::from_label(label).is_playlist_collection()
}

fn detect_blocking_modal(recognition: &TextRecognition) -> Option<ParserDiagnostic> {
  let has_cancel = recognition.best_contains("取消").is_some();
  let has_dialog_action =
    recognition.best_contains("打开").is_some() || recognition.best_contains("存储").is_some();

  (has_cancel && has_dialog_action).then(|| ParserDiagnostic {
    code: "blocking_modal_dialog".to_string(),
    message: "blocking open or save dialog markers were detected".to_string(),
    node_id: None,
  })
}

fn parse_sidebar_viewport(
  observation_index: usize,
  viewport_bounds: ViewBounds,
  recognition: &TextRecognition,
) -> SidebarViewportObservation {
  let mut evidence_nodes = recognition
    .regions
    .iter()
    .enumerate()
    .filter(|(_, region)| {
      viewport_contains_center(
        viewport_bounds,
        ViewBounds::new(
          region.bounds.origin.x,
          region.bounds.origin.y,
          region.bounds.size.width,
          region.bounds.size.height,
        ),
      )
    })
    .map(|(index, region)| ViewEvidenceNode {
      id: format!("obs{observation_index}.ocr{index}"),
      source: ViewEvidenceSource::OcrText,
      label: Some(region.text.trim().to_string()),
      bounds: Some(ViewBounds::new(
        region.bounds.origin.x,
        region.bounds.origin.y,
        region.bounds.size.width,
        region.bounds.size.height,
      )),
      confidence: confidence_from_ocr(region.confidence),
    })
    .collect::<Vec<_>>();

  evidence_nodes.sort_by(|left, right| {
    let left_bounds = left.bounds.unwrap_or_default();
    let right_bounds = right.bounds.unwrap_or_default();
    left_bounds
      .y
      .partial_cmp(&right_bounds.y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| {
        left_bounds
          .x
          .partial_cmp(&right_bounds.x)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
  });

  let candidates = evidence_nodes
    .iter()
    .filter_map(|node| candidate_from_evidence(observation_index, node))
    .collect::<Vec<_>>();
  let viewport_fingerprint = viewport_fingerprint(&evidence_nodes);

  SidebarViewportObservation {
    observation_index,
    viewport: ViewViewportRecord {
      page_index: observation_index,
      bounds: viewport_bounds,
      axis: ViewAxis::Vertical,
      scroll_offset: None,
    },
    source_artifacts: Vec::new(),
    incoming_scroll_delivery_path: None,
    scroll_motion: None,
    viewport_fingerprint,
    evidence_nodes,
    candidates,
    parser_notes: Vec::new(),
    ax_scrollbar_boundary: None,
  }
}

fn candidate_from_evidence(
  observation_index: usize,
  node: &ViewEvidenceNode,
) -> Option<SidebarViewportCandidate> {
  let label = node.label.as_deref()?.trim();
  if label.chars().count() < 2 {
    return None;
  }
  let bounds = node.bounds?;
  let kind = classify_sidebar_text(label, bounds.x);
  if kind == SidebarCandidateKind::Unknown {
    return None;
  }

  Some(SidebarViewportCandidate {
    id: format!(
      "obs{observation_index}.candidate.{}.{}",
      candidate_source_component(observation_index, &node.id),
      slug(label)
    ),
    kind,
    label: Some(label.to_string()),
    bounds: Some(bounds),
    evidence_ids: vec![node.id.clone()],
    confidence: node.confidence,
  })
}

fn candidate_source_component(observation_index: usize, evidence_id: &str) -> &str {
  evidence_id
    .strip_prefix(&format!("obs{observation_index}."))
    .unwrap_or(evidence_id)
}

fn classify_sidebar_text(label: &str, x: f64) -> SidebarCandidateKind {
  if SidebarSectionKind::from_label(label).is_known() {
    SidebarCandidateKind::SectionHeader
  } else if x >= 24.0 {
    SidebarCandidateKind::PlaylistItem
  } else if matches!(
    label,
    "推荐" | "发现音乐" | "播客" | "私人漫游" | "最近播放"
  ) {
    SidebarCandidateKind::NavigationItem
  } else {
    SidebarCandidateKind::Unknown
  }
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

fn reconstruct_playlist_sidebar(
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  observations: Vec<SidebarViewportObservation>,
) -> PlaylistSidebarScan {
  let sidebar_bounds = sidebar_region.bounds.unwrap_or_default();
  let ReconstructionOutput {
    root,
    anchor_index,
    landmark_index,
    sections,
    diagnostics,
    boundary,
  } = reconstruct(&NeteasePolicy, &observations, sidebar_bounds);

  PlaylistSidebarScan {
    schema_version: VIEW_IR_SCHEMA_VERSION.to_string(),
    app,
    window,
    sidebar_region,
    observations,
    reconstruction: ViewReconstructionRecord {
      root,
      anchor_index,
      landmark_index,
    },
    projection: PlaylistSidebarProjection { sections },
    boundary,
    interaction_events: Vec::new(),
    diagnostics,
    known_limits: Vec::new(),
  }
}

/// `ReconstructionPolicy` impl that injects NetEase's classification +
/// node/projection construction into the framework's generic reconstruct
/// loop. All node id formats, anchor id formats, the section-header /
/// item / unknown classification, the root container's domain_kind, and
/// the dedup diagnostic wording live here — `auv-view` knows none of it.
struct NeteasePolicy;

impl ReconstructionPolicy for NeteasePolicy {
  type Candidate = SidebarViewportCandidate;
  type SectionKey = (SidebarSectionKind, String);
  type SectionProjection = SidebarSection;
  type ItemProjection = PlaylistSidebarItem;
  type Observation = SidebarViewportObservation;

  fn candidates<'a>(
    &self,
    observation: &'a Self::Observation,
  ) -> impl Iterator<Item = &'a Self::Candidate> + 'a
  where
    Self::Candidate: 'a,
  {
    observation.candidates.iter()
  }

  fn classify(&self, candidate: &Self::Candidate) -> CandidateRole<Self::SectionKey> {
    let Some(label) = candidate.label.as_deref().map(str::trim) else {
      return CandidateRole::Unknown;
    };
    match candidate.kind {
      SidebarCandidateKind::SectionHeader => {
        let kind = SidebarSectionKind::from_label(label);
        CandidateRole::Header {
          section_key: (kind, normalize_identity(label)),
        }
      }
      SidebarCandidateKind::PlaylistItem | SidebarCandidateKind::NavigationItem => {
        CandidateRole::Item {
          dedupe_key: normalize_identity(label),
        }
      }
      SidebarCandidateKind::Unknown => CandidateRole::Unknown,
    }
  }

  fn build_section(
    &self,
    observation: &Self::Observation,
    candidate: &Self::Candidate,
  ) -> (ViewNodeRecord, Self::SectionProjection) {
    let label = candidate
      .label
      .as_deref()
      .map(str::trim)
      .unwrap_or_default();
    let kind = SidebarSectionKind::from_label(label);
    let section_id = format!(
      "section.obs{}.{}.{}",
      observation.observation_index,
      candidate.id,
      slug(label)
    );
    let node = section_node(&section_id, kind, label, candidate, observation);
    let projection = SidebarSection {
      id: section_id,
      kind,
      label: Some(label.to_string()),
      items: Vec::new(),
    };
    (node, projection)
  }

  fn build_unassigned_section(&self) -> (ViewNodeRecord, Self::SectionProjection) {
    let section_id = "section.unassigned".to_string();
    let node = ViewNodeRecord {
      id: section_id.clone(),
      kind: ViewNodeKind::Section,
      domain_kind: Some(SidebarSectionKind::Unknown.domain_kind().to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: ViewBounds::default(),
      scrollable: None,
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::ObserveOnly],
      evidence: Vec::new(),
      children: Vec::new(),
    };
    let projection = SidebarSection {
      id: section_id,
      kind: SidebarSectionKind::Unknown,
      label: None,
      items: Vec::new(),
    };
    (node, projection)
  }

  fn build_item(
    &self,
    observation: &Self::Observation,
    candidate: &Self::Candidate,
    section: &Self::SectionProjection,
  ) -> (ViewNodeRecord, Self::ItemProjection) {
    let label = candidate
      .label
      .as_deref()
      .map(str::trim)
      .unwrap_or_default();
    let item_id = format!(
      "item.obs{}.{}.{}",
      observation.observation_index,
      candidate.id,
      slug(label)
    );
    let anchor_id = format!("anchor.{item_id}");
    let node = item_node(&item_id, &anchor_id, label, candidate, observation);
    let projection = PlaylistSidebarItem {
      id: item_id,
      label: label.to_string(),
      section_hint: Some(section.kind),
      confidence: candidate.confidence,
      candidate_id: Some(candidate.id.clone()),
      anchor_id: Some(anchor_id),
    };
    (node, projection)
  }

  fn append_item_to_section_projection(
    &self,
    section: &mut Self::SectionProjection,
    item: Self::ItemProjection,
  ) {
    section.items.push(item);
  }

  fn build_root(
    &self,
    sidebar_bounds: ViewBounds,
    boundary: ScrollBoundarySummary,
    section_children: Vec<ViewNodeRecord>,
  ) -> ViewNodeRecord {
    ViewNodeRecord {
      id: "root.sidebar".to_string(),
      kind: ViewNodeKind::Collection,
      domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: sidebar_bounds,
      scrollable: Some(ViewScrollable {
        axis: ViewAxis::Vertical,
        boundary,
      }),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::Scroll],
      evidence: Vec::new(),
      children: section_children,
    }
  }

  fn emit_dedup_diagnostic(
    &self,
    candidate: &Self::Candidate,
    section: &Self::SectionProjection,
  ) -> ParserDiagnostic {
    let label = candidate.label.as_deref().unwrap_or("");
    ParserDiagnostic {
      code: "deduplicated_item".to_string(),
      message: format!(
        "deduplicated repeated sidebar item {label:?} in section {:?}",
        section.kind
      ),
      node_id: Some(candidate.id.clone()),
    }
  }
}

fn scan_sidebar_with_observer(
  observer: &mut impl SidebarScanObserver,
  options: ScanOptions,
  category: PlaylistCategory,
  scroll_amount: f64,
  scroll_settle_ms: u64,
) -> PlaylistSidebarScan {
  let top_seek = scroll_to_top_by_motion(observer, options.max_scrolls);
  observer.reset_collection_phase();
  let loop_outcome = scan_with_collection_policy(observer, options, category);
  let interaction_events = build_standalone_interaction_events(
    &loop_outcome.observations,
    scroll_amount,
    scroll_settle_ms,
    loop_outcome.stop_reason.as_deref(),
  );

  let mut scan = reconstruct_playlist_sidebar(
    ScanAppContext {
      app_id: Some(DEFAULT_APP_ID.to_string()),
      name: None,
      version: None,
    },
    ScanWindowContext {
      id: Some("fake".to_string()),
      title: None,
      bounds: None,
    },
    ViewRegionRecord::default(),
    loop_outcome.observations,
  );
  scan.diagnostics.extend(top_seek.diagnostics);
  scan.diagnostics.extend(loop_outcome.diagnostics);
  scan.known_limits.extend(top_seek.known_limits);
  scan.known_limits.extend(loop_outcome.known_limits);
  scan.interaction_events = interaction_events;
  if top_seek.boundary == BoundaryConfidence::Likely {
    apply_top_boundary(&mut scan, top_seek.boundary);
  }
  if matches!(
    loop_outcome.stop_reason.as_deref(),
    Some("scroll_no_motion_after_input") | Some("scroll_no_new_semantic_candidates_after_input")
  ) {
    apply_bottom_boundary(&mut scan, BoundaryConfidence::Likely);
  }
  scan
}

fn heuristic_stop_reason_with_ax_corroboration(
  base_reason: &'static str,
  ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>,
) -> Option<&'static str> {
  match (base_reason, ax_scrollbar_boundary) {
    ("scroll_no_new_semantic_candidates_after_input", Some(SidebarScrollbarBoundary::Bottom)) => {
      Some("scroll_no_new_semantic_candidates_with_ax_scrollbar_bottom")
    }
    ("scroll_no_motion_after_input", Some(SidebarScrollbarBoundary::Bottom)) => {
      Some("scroll_no_motion_with_ax_scrollbar_bottom")
    }
    (
      "scroll_no_new_semantic_candidates_after_input" | "scroll_no_motion_after_input",
      Some(SidebarScrollbarBoundary::Top | SidebarScrollbarBoundary::Interior),
    ) => None,
    _ => Some(base_reason),
  }
}

fn repeated_fingerprint_stop_reason(
  ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>,
) -> &'static str {
  if ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom) {
    "repeated_viewport_fingerprint_with_ax_scrollbar_bottom"
  } else {
    "repeated_viewport_fingerprint"
  }
}

trait SidebarScanObserver: ViewObserver<Observation = SidebarViewportObservation> {
  fn reset_collection_phase(&mut self) {}

  fn observe_scroll_seek(
    &mut self,
    observation_index: usize,
  ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    self.observe(observation_index)
  }
}

fn scroll_to_top_by_motion(
  observer: &mut impl SidebarScanObserver,
  max_scrolls: usize,
) -> TopSeekOutcome {
  let mut outcome = TopSeekOutcome::default();
  observer.reset_collection_phase();

  if let Err(diagnostic) = observer.observe_scroll_seek(0) {
    outcome.diagnostics.push(diagnostic);
    return outcome;
  }

  for scroll_index in 0..max_scrolls {
    if let Err(diagnostic) = observer.scroll_up() {
      outcome.diagnostics.push(diagnostic);
      return outcome;
    }

    let observation = match observer.observe_scroll_seek(scroll_index + 1) {
      Ok(observation) => observation,
      Err(diagnostic) => {
        outcome.diagnostics.push(diagnostic);
        return outcome;
      }
    };

    if successful_scroll_delivery_path(observation.incoming_scroll_delivery_path.as_deref()) {
      if observation
        .scroll_motion
        .as_ref()
        .is_some_and(|motion| motion.no_motion)
      {
        outcome.boundary = BoundaryConfidence::Likely;
        return outcome;
      }
    }
  }

  outcome.known_limits.push(format!(
    "top seek stopped after max_scrolls={max_scrolls} without repeated sidebar pixels"
  ));
  outcome
}

struct CollectionLoopOutcome {
  observations: Vec<SidebarViewportObservation>,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
  stop_reason: Option<String>,
}

fn scan_with_collection_policy(
  observer: &mut impl ViewObserver<Observation = SidebarViewportObservation>,
  options: ScanOptions,
  category: PlaylistCategory,
) -> CollectionLoopOutcome {
  let mut policy = CollectionPolicy::new(category);
  let mut observations = Vec::new();
  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();
  let mut previous_fingerprint: Option<String> = None;
  let mut seen_semantic_candidates = HashSet::new();
  let mut consecutive_no_new_semantic_candidates_after_scroll = 0usize;
  let mut consecutive_no_motion_after_scroll = 0usize;
  let mut observed_scroll_motion_after_successful_input = false;
  let mut scrolls = 0;
  let mut stop_reason = None;

  loop {
    let observation_index = observations.len();
    let observation = match observer.observe(observation_index) {
      Ok(observation) => observation,
      Err(diagnostic) => {
        diagnostics.push(diagnostic);
        break;
      }
    };
    let fingerprint = observation.viewport_fingerprint().to_string();
    let repeated_fingerprint = previous_fingerprint
      .as_deref()
      .is_some_and(|prev| prev == fingerprint.as_str());
    previous_fingerprint = Some(fingerprint);
    let ax_scrollbar_boundary = observation.ax_scrollbar_boundary;
    let observation = policy.apply(observation);
    let introduced_new_semantic_candidates =
      record_page_semantic_candidates(&observation, &mut seen_semantic_candidates);
    let reached_stop_landmark = policy.reached_stop_landmark();
    let started = policy.start_seen();
    let successful_scroll_input =
      successful_scroll_delivery_path(observation.incoming_scroll_delivery_path.as_deref());
    if started && !seen_semantic_candidates.is_empty() && successful_scroll_input {
      if introduced_new_semantic_candidates {
        consecutive_no_new_semantic_candidates_after_scroll = 0;
      } else {
        consecutive_no_new_semantic_candidates_after_scroll += 1;
      }
    } else {
      consecutive_no_new_semantic_candidates_after_scroll = 0;
    }
    if successful_scroll_input {
      if let Some(motion) = observation.scroll_motion.as_ref() {
        if motion.no_motion && observed_scroll_motion_after_successful_input {
          consecutive_no_motion_after_scroll += 1;
        } else if motion.no_motion {
          consecutive_no_motion_after_scroll = 0;
        } else {
          observed_scroll_motion_after_successful_input = true;
          consecutive_no_motion_after_scroll = 0;
        }
      } else {
        consecutive_no_motion_after_scroll = 0;
      }
    } else {
      consecutive_no_motion_after_scroll = 0;
    }
    observations.push(observation);

    if reached_stop_landmark {
      stop_reason = Some("reached_stop_landmark".to_string());
      break;
    }

    // NOTICE(netease-scroll-stop): exact viewport fingerprints are kept as a
    // backward-compatible loop-boundary signal, but they are no longer the only
    // scroll stop detector. Motion evidence covers the real NetEase case where
    // OCR text drifts enough that exact fingerprints do not repeat at bottom.
    if repeated_fingerprint {
      stop_reason = Some(repeated_fingerprint_stop_reason(ax_scrollbar_boundary).to_string());
      break;
    }

    // NOTICE(netease-scroll-semantic-boundary): repeated "no new semantic
    // candidates after scroll" is a stronger completion signal than crop
    // motion alone because it tracks the actual playlist/sidebar IR that this
    // crate exports. It remains heuristic until a future slice corroborates it
    // with scroll-bar, AX scroll-state, or provider-reported bounds.
    if consecutive_no_new_semantic_candidates_after_scroll >= 2 {
      if let Some(reason) = heuristic_stop_reason_with_ax_corroboration(
        "scroll_no_new_semantic_candidates_after_input",
        ax_scrollbar_boundary,
      ) {
        stop_reason = Some(reason.to_string());
        break;
      }
    }

    // NOTICE(netease-scroll-motion-boundary): identical sidebar pixels only
    // count as bottom evidence after a successful scroll delivery path and at
    // least one prior post-scroll motion observation. This prevents launch
    // state, failed/noop input, or already-stuck captures from being promoted
    // into a false bottom boundary.
    if consecutive_no_motion_after_scroll >= 2 {
      if let Some(reason) = heuristic_stop_reason_with_ax_corroboration(
        "scroll_no_motion_after_input",
        ax_scrollbar_boundary,
      ) {
        stop_reason = Some(reason.to_string());
        break;
      }
    }

    if scrolls >= options.max_scrolls {
      known_limits.push(format!("stopped after max_scrolls={}", options.max_scrolls));
      break;
    }

    if let Err(diagnostic) = observer.scroll_down() {
      diagnostics.push(diagnostic);
      break;
    }
    scrolls += 1;
  }

  if !policy.start_seen() {
    if let Some(limit) = policy.missing_start_limit() {
      known_limits.push(limit);
    }
  }

  CollectionLoopOutcome {
    observations,
    diagnostics,
    known_limits,
    stop_reason,
  }
}

fn successful_scroll_delivery_path(path: Option<&str>) -> bool {
  !matches!(path, None | Some("noop" | "unsupported"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SemanticCandidateKey {
  kind: SidebarCandidateKind,
  label: String,
  section_hint: Option<SidebarSectionKind>,
}

fn record_page_semantic_candidates(
  observation: &SidebarViewportObservation,
  seen: &mut HashSet<SemanticCandidateKey>,
) -> bool {
  let mut introduced_new = false;
  let mut current_section = None;

  for candidate in &observation.candidates {
    let Some(label) = candidate.label.as_deref().map(str::trim) else {
      continue;
    };
    let normalized_label = normalize_identity(label);
    if normalized_label.is_empty() {
      continue;
    }

    let section_hint = match candidate.kind {
      SidebarCandidateKind::SectionHeader => {
        let section = SidebarSectionKind::from_label(label);
        current_section = Some(section);
        Some(section)
      }
      SidebarCandidateKind::PlaylistItem => current_section,
      SidebarCandidateKind::NavigationItem => None,
      SidebarCandidateKind::Unknown => continue,
    };

    if seen.insert(SemanticCandidateKey {
      kind: candidate.kind,
      label: normalized_label,
      section_hint,
    }) {
      introduced_new = true;
    }
  }

  introduced_new
}

struct CollectionPolicy {
  category: PlaylistCategory,
  started: bool,
  stopped: bool,
}

impl CollectionPolicy {
  fn new(category: PlaylistCategory) -> Self {
    Self {
      category,
      started: category == PlaylistCategory::All,
      stopped: false,
    }
  }

  fn apply(&mut self, mut observation: SidebarViewportObservation) -> SidebarViewportObservation {
    if self.category == PlaylistCategory::All {
      return observation;
    }

    let mut accepted = Vec::new();
    for candidate in observation.candidates {
      if self.stopped {
        break;
      }

      let section_kind = candidate
        .label
        .as_deref()
        .map(SidebarSectionKind::from_label)
        .unwrap_or(SidebarSectionKind::Unknown);

      if candidate.kind == SidebarCandidateKind::SectionHeader {
        match self.category {
          PlaylistCategory::Created if section_kind == SidebarSectionKind::MyPlaylists => {
            self.started = true;
            accepted.push(candidate);
          }
          PlaylistCategory::Created
            if self.started && section_kind == SidebarSectionKind::FavoritePlaylists =>
          {
            self.stopped = true;
            break;
          }
          PlaylistCategory::Favorite if section_kind == SidebarSectionKind::FavoritePlaylists => {
            self.started = true;
            accepted.push(candidate);
          }
          PlaylistCategory::Favorite if self.started => {
            accepted.push(candidate);
          }
          _ => {}
        }
      } else if self.started {
        accepted.push(candidate);
      }
    }
    observation.candidates = accepted;
    observation
  }

  fn reached_stop_landmark(&self) -> bool {
    self.stopped
  }

  fn start_seen(&self) -> bool {
    self.started
  }

  fn missing_start_limit(&self) -> Option<String> {
    match self.category {
      PlaylistCategory::All => None,
      PlaylistCategory::Created => {
        Some("category created scan ended without seeing created playlist landmark".to_string())
      }
      PlaylistCategory::Favorite => {
        Some("category favorite scan ended without seeing favorite playlist landmark".to_string())
      }
    }
  }
}

fn build_standalone_interaction_events(
  observations: &[SidebarViewportObservation],
  scroll_amount: f64,
  scroll_settle_ms: u64,
  stop_reason: Option<&str>,
) -> Vec<InteractionEvent> {
  let mut events = Vec::new();
  for (index, observation) in observations.iter().enumerate() {
    events.push(InteractionEvent {
      event_index: events.len(),
      phase: InteractionPhase::Collect,
      kind: InteractionEventKind::Observe,
      observation_index: Some(observation.observation_index),
      from_observation: None,
      to_observation: None,
      viewport_fingerprint: Some(observation.viewport_fingerprint.clone()),
      scroll: None,
      motion: observation.scroll_motion.clone(),
      artifacts: observation.source_artifacts.clone(),
      note: None,
    });

    if index + 1 < observations.len() {
      let mut artifacts = observation.source_artifacts.clone();
      for artifact in &observations[index + 1].source_artifacts {
        if !artifacts.iter().any(|existing| existing == artifact) {
          artifacts.push(artifact.clone());
        }
      }
      events.push(InteractionEvent {
        event_index: events.len(),
        phase: InteractionPhase::Collect,
        kind: InteractionEventKind::InputScroll,
        observation_index: None,
        from_observation: Some(observation.observation_index),
        to_observation: Some(observations[index + 1].observation_index),
        viewport_fingerprint: None,
        scroll: Some(ScrollInteraction {
          axis: ViewAxis::Vertical,
          direction: ScrollDirection::Down,
          requested_delta: -scroll_amount,
          policy: "background_preferred".to_string(),
          delivery_path: observations[index + 1]
            .incoming_scroll_delivery_path
            .clone(),
          motion: observations[index + 1].scroll_motion.clone(),
          settle_ms: scroll_settle_ms,
          anchor: None,
          detected_boundary: "unknown".to_string(),
        }),
        motion: observations[index + 1].scroll_motion.clone(),
        artifacts,
        note: Some(
          "standalone event; durable trace should use view.parse.scroll.<index> spans".to_string(),
        ),
      });
    }
  }
  if let Some(stop_reason) = stop_reason {
    events.push(InteractionEvent {
      event_index: events.len(),
      phase: InteractionPhase::Collect,
      kind: InteractionEventKind::StopDecision,
      observation_index: observations
        .last()
        .map(|observation| observation.observation_index),
      from_observation: None,
      to_observation: None,
      viewport_fingerprint: observations
        .last()
        .map(|observation| observation.viewport_fingerprint.clone()),
      scroll: None,
      motion: observations
        .last()
        .and_then(|observation| observation.scroll_motion.clone()),
      artifacts: observations
        .last()
        .map(|observation| observation.source_artifacts.clone())
        .unwrap_or_default(),
      note: Some(stop_reason.to_string()),
    });
  }
  events
}

fn apply_top_boundary(scan: &mut PlaylistSidebarScan, top: BoundaryConfidence) {
  scan.boundary.top = top;
  if let Some(scrollable) = scan.reconstruction.root.scrollable.as_mut() {
    scrollable.boundary.top = top;
  }
}

fn apply_bottom_boundary(scan: &mut PlaylistSidebarScan, bottom: BoundaryConfidence) {
  scan.boundary.bottom = bottom;
  if let Some(scrollable) = scan.reconstruction.root.scrollable.as_mut() {
    scrollable.boundary.bottom = bottom;
  }
}

#[cfg(target_os = "macos")]
fn sidebar_ax_scrollbar_boundary(
  nodes: &[ObservedAxNode],
  window: &auv_driver::Window,
  sidebar_bounds: ViewBounds,
) -> Option<SidebarScrollbarBoundary> {
  let sidebar_screen_bounds = ViewBounds::new(
    window.frame.origin.x + sidebar_bounds.x,
    window.frame.origin.y + sidebar_bounds.y,
    sidebar_bounds.width,
    sidebar_bounds.height,
  );
  let sidebar_right = sidebar_screen_bounds.x + sidebar_screen_bounds.width;
  let sidebar_bottom = sidebar_screen_bounds.y + sidebar_screen_bounds.height;

  let scrollbar = nodes
    .iter()
    .filter(|node| {
      node.role == "AXScrollBar"
        && node.bounds.width > 0
        && node.bounds.height > 0
        && node.bounds.height > node.bounds.width
    })
    .filter(|node| {
      let node_top = node.bounds.y as f64;
      let node_bottom = node_top + node.bounds.height as f64;
      let vertical_overlap =
        (node_bottom.min(sidebar_bottom) - node_top.max(sidebar_screen_bounds.y)).max(0.0);
      let overlap_ratio = vertical_overlap / node.bounds.height as f64;
      let center_x = node.bounds.x as f64 + (node.bounds.width as f64 / 2.0);
      overlap_ratio >= 0.5
        && center_x >= sidebar_screen_bounds.x
        && center_x <= sidebar_right + 20.0
    })
    .max_by(|left, right| {
      let left_overlap = scrollbar_overlap_score(left, sidebar_screen_bounds);
      let right_overlap = scrollbar_overlap_score(right, sidebar_screen_bounds);
      left_overlap.total_cmp(&right_overlap)
    })?;

  vertical_scrollbar_boundary_from_nodes(nodes, scrollbar)
}

#[cfg(target_os = "macos")]
fn scrollbar_overlap_score(node: &ObservedAxNode, sidebar_screen_bounds: ViewBounds) -> f64 {
  let sidebar_right = sidebar_screen_bounds.x + sidebar_screen_bounds.width;
  let sidebar_bottom = sidebar_screen_bounds.y + sidebar_screen_bounds.height;
  let node_top = node.bounds.y as f64;
  let node_bottom = node_top + node.bounds.height as f64;
  let vertical_overlap =
    (node_bottom.min(sidebar_bottom) - node_top.max(sidebar_screen_bounds.y)).max(0.0);
  let overlap_ratio = vertical_overlap / node.bounds.height as f64;
  let node_right = node.bounds.x as f64 + node.bounds.width as f64;
  let right_edge_distance = (sidebar_right - node_right).abs();
  overlap_ratio * 1000.0 - right_edge_distance
}

#[cfg(target_os = "macos")]
fn vertical_scrollbar_boundary_from_nodes(
  nodes: &[ObservedAxNode],
  scrollbar: &ObservedAxNode,
) -> Option<SidebarScrollbarBoundary> {
  let path_prefix = format!("{}.", scrollbar.path);
  let mut increment_page_height = None;
  let mut decrement_page_height = None;

  for node in nodes
    .iter()
    .filter(|node| node.path.starts_with(path_prefix.as_str()))
  {
    match node.subrole.as_str() {
      "AXIncrementPage" => increment_page_height = Some(node.bounds.height),
      "AXDecrementPage" => decrement_page_height = Some(node.bounds.height),
      _ => {}
    }
  }

  match (increment_page_height, decrement_page_height) {
    (Some(height), _) if height <= 1 => Some(SidebarScrollbarBoundary::Bottom),
    (_, Some(height)) if height <= 1 => Some(SidebarScrollbarBoundary::Top),
    (Some(_), Some(_)) => Some(SidebarScrollbarBoundary::Interior),
    _ => None,
  }
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
struct LiveSidebarObserver {
  session: MacosDriverSession,
  window: auv_driver::Window,
  sidebar_bounds: ViewBounds,
  sidebar_ratio: RatioRect,
  ocr_options: TextRecognitionOptions,
  artifact_dir: PathBuf,
  pending_artifacts: Vec<std::thread::JoinHandle<Result<(), String>>>,
  scroll_amount: f64,
  scroll_settle_ms: u64,
  pending_scroll_delivery_path: Option<String>,
  previous_sidebar_crop: Option<RgbaImage>,
  motion_policy: MotionDetectionPolicy,
}

#[cfg(target_os = "macos")]
impl LiveSidebarObserver {
  fn capture_observation(
    &mut self,
    observation_index: usize,
  ) -> Result<(RgbaImage, f64, TextRecognition, SidebarViewportObservation), ParserDiagnostic> {
    let capture = self
      .session
      .window()
      .capture(&self.window)
      .map_err(|error| ParserDiagnostic {
        code: "window_capture_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        self.sidebar_ratio,
        self.ocr_options.clone(),
      )
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_ocr_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;

    let window_recognition = recognition_in_window_space(recognition, &capture);
    let observation =
      parse_sidebar_viewport(observation_index, self.sidebar_bounds, &window_recognition);

    Ok((
      capture.image.clone(),
      capture.scale_factor,
      window_recognition,
      observation,
    ))
  }

  fn write_observation_artifacts(
    &mut self,
    observation_index: usize,
    image: RgbaImage,
    recognition: TextRecognition,
    observation: SidebarViewportObservation,
  ) -> Vec<String> {
    let base = format!("obs-{observation_index:04}");
    let screenshot = self.artifact_dir.join(format!("{base}-window.png"));
    let overlay = self.artifact_dir.join(format!("{base}-overlay.png"));
    let recognition_json = self.artifact_dir.join(format!("{base}-recognition.json"));
    let observation_json = self.artifact_dir.join(format!("{base}-observation.json"));
    let paths = vec![
      screenshot.clone(),
      overlay.clone(),
      recognition_json.clone(),
      observation_json.clone(),
    ];
    let artifact_dir = self.artifact_dir.clone();
    let sidebar_bounds = self.sidebar_bounds;
    self.pending_artifacts.push(std::thread::spawn(move || {
      std::fs::create_dir_all(&artifact_dir)
        .map_err(|error| format!("failed to create {}: {error}", artifact_dir.display()))?;
      image
        .save(&screenshot)
        .map_err(|error| format!("failed to save {}: {error}", screenshot.display()))?;

      let mut overlay_image = image.clone();
      draw_overlay(&mut overlay_image, sidebar_bounds, &observation);
      overlay_image
        .save(&overlay)
        .map_err(|error| format!("failed to save {}: {error}", overlay.display()))?;

      let recognition_payload = serde_json::to_string_pretty(&recognition)
        .map_err(|error| format!("failed to serialize recognition: {error}"))?;
      std::fs::write(&recognition_json, recognition_payload)
        .map_err(|error| format!("failed to write {}: {error}", recognition_json.display()))?;

      let observation_payload = serde_json::to_string_pretty(&observation)
        .map_err(|error| format!("failed to serialize observation: {error}"))?;
      std::fs::write(&observation_json, observation_payload)
        .map_err(|error| format!("failed to write {}: {error}", observation_json.display()))?;

      Ok(())
    }));

    paths
      .into_iter()
      .map(|path| path.display().to_string())
      .collect()
  }

  fn finish_artifacts(self) -> Vec<ParserDiagnostic> {
    self
      .pending_artifacts
      .into_iter()
      .filter_map(|handle| match handle.join() {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(ParserDiagnostic {
          code: "artifact_write_failed".to_string(),
          message: error,
          node_id: None,
        }),
        Err(_) => Some(ParserDiagnostic {
          code: "artifact_write_panicked".to_string(),
          message: "background artifact writer panicked".to_string(),
          node_id: None,
        }),
      })
      .collect()
  }
}

#[cfg(target_os = "macos")]
impl ViewObserver for LiveSidebarObserver {
  type Observation = SidebarViewportObservation;

  fn observe(
    &mut self,
    observation_index: usize,
  ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (image, scale_factor, window_recognition, mut observation) =
      self.capture_observation(observation_index)?;
    // NOTICE(netease-scroll-ax-window-targeting): corroboration currently asks
    // macOS for the app's focused/first AX window because the typed AX capture
    // API does not yet accept a concrete native window ref. Re-open this only
    // if NetEase starts surfacing multiple competing windows during playlist
    // scans.
    observation.ax_scrollbar_boundary = self.capture_ax_scrollbar_boundary();
    let sidebar_crop = crop_image(&image, self.sidebar_bounds, scale_factor);
    let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
    observation.scroll_motion = incoming_scroll_delivery_path
      .as_ref()
      .and(self.previous_sidebar_crop.as_ref())
      .map(|previous| self.motion_policy.compare(previous, &sidebar_crop));
    self.previous_sidebar_crop = Some(sidebar_crop);
    observation.incoming_scroll_delivery_path = incoming_scroll_delivery_path;
    observation.source_artifacts = self.write_observation_artifacts(
      observation_index,
      image,
      window_recognition,
      observation.clone(),
    );

    Ok(observation)
  }

  fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (_, _, _, observation) = self.capture_observation(0)?;
    Ok(observation)
  }

  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by(self.scroll_amount)
  }

  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by(-self.scroll_amount)
  }
}

#[cfg(target_os = "macos")]
impl SidebarScanObserver for LiveSidebarObserver {
  fn reset_collection_phase(&mut self) {
    // NOTICE(netease-scroll-phase-state): top-seek rewind and collection reuse
    // the same observer instance. Clear transient scroll/crop state so the
    // first collected observation does not inherit rewind-phase motion
    // metadata.
    self.pending_scroll_delivery_path = None;
    self.previous_sidebar_crop = None;
  }

  fn observe_scroll_seek(
    &mut self,
    observation_index: usize,
  ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (image, scale_factor, _, mut observation) = self.capture_observation(observation_index)?;
    observation.ax_scrollbar_boundary = self.capture_ax_scrollbar_boundary();
    let sidebar_crop = crop_image(&image, self.sidebar_bounds, scale_factor);
    let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
    observation.scroll_motion = incoming_scroll_delivery_path
      .as_ref()
      .and(self.previous_sidebar_crop.as_ref())
      .map(|previous| self.motion_policy.compare(previous, &sidebar_crop));
    self.previous_sidebar_crop = Some(sidebar_crop);
    observation.incoming_scroll_delivery_path = incoming_scroll_delivery_path;
    Ok(observation)
  }
}

#[cfg(target_os = "macos")]
impl LiveSidebarObserver {
  fn capture_ax_scrollbar_boundary(&self) -> Option<SidebarScrollbarBoundary> {
    let app = self
      .window
      .app_bundle_id
      .as_deref()
      .filter(|bundle_id| !bundle_id.trim().is_empty())
      .unwrap_or(DEFAULT_APP_ID);
    let snapshot = capture_ax_tree_snapshot(app, 8, 64).ok()?;
    sidebar_ax_scrollbar_boundary(&snapshot.snapshot.nodes, &self.window, self.sidebar_bounds)
  }

  fn scroll_anchor(&self) -> auv_driver::Point {
    auv_driver::Point::new(
      self.sidebar_bounds.x + self.sidebar_bounds.width * 0.5,
      self.sidebar_bounds.y + self.sidebar_bounds.height * 0.75,
    )
  }

  fn scroll_by(&mut self, vertical_delta: f64) -> Result<(), ParserDiagnostic> {
    let anchor = self.scroll_anchor();
    let result = self
      .session
      .window()
      .scroll(
        &self.window,
        WindowPoint::new(anchor.x, anchor.y),
        Scroll::new(0.0, vertical_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(self.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_scroll_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;
    self.pending_scroll_delivery_path = Some(delivery_path_label(result.selected_path).to_string());
    Ok(())
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
    auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll => {
      "window_targeted_keyboard_scroll"
    }
    auv_driver::InputDeliveryPath::ClipboardPaste => "clipboard_paste",
    auv_driver::InputDeliveryPath::ForegroundSystemEvents => "foreground_system_events",
    auv_driver::InputDeliveryPath::Unsupported => "unsupported",
  }
}

#[cfg(target_os = "macos")]
fn recognition_in_window_space(
  mut recognition: TextRecognition,
  capture: &Capture,
) -> TextRecognition {
  for region in &mut recognition.regions {
    region.bounds.origin.x -= capture.bounds.origin.x;
    region.bounds.origin.y -= capture.bounds.origin.y;
  }
  recognition
}

#[cfg(target_os = "macos")]
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
fn draw_overlay(
  image: &mut RgbaImage,
  sidebar_bounds: ViewBounds,
  observation: &SidebarViewportObservation,
) {
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
  RatioRect::new(
    bounds.x / width,
    bounds.y / height,
    bounds.width / width,
    bounds.height / height,
  )
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

fn candidate_evidence(
  candidate: &SidebarViewportCandidate,
  observation: &SidebarViewportObservation,
) -> Vec<ViewEvidenceNode> {
  candidate
    .evidence_ids
    .iter()
    .filter_map(|id| {
      observation
        .evidence_nodes
        .iter()
        .find(|node| node.id == *id)
        .cloned()
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scroll::policies::detection_motion::MotionEvidence;

  #[test]
  fn parse_viewport_classifies_sections_and_playlist_items() {
    let recognition = fake_recognition(vec![
      ("推荐", 8.0, 10.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ("Jazz", 32.0, 106.0, 80.0, 20.0),
    ]);
    let observation =
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);

    assert_eq!(observation.candidates.len(), 4);
    assert_eq!(
      observation.candidates[1].kind,
      SidebarCandidateKind::SectionHeader
    );
    assert_eq!(
      observation.candidates[1].label,
      Some("创建的歌单".to_string())
    );
    assert_eq!(
      observation.candidates[2].kind,
      SidebarCandidateKind::PlaylistItem
    );
    assert_eq!(
      observation.candidates[2].label,
      Some("Coding BGM".to_string())
    );
    assert_eq!(
      observation.evidence_nodes[2].source,
      ViewEvidenceSource::OcrText
    );
  }

  #[test]
  fn detect_sidebar_region_uses_manual_region_when_provided() {
    let region = detect_sidebar_region(
      Some(RatioRect::new(0.0, 0.1, 0.25, 0.8)),
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(Vec::new()),
    )
    .expect("manual sidebar region should be accepted");

    assert_eq!(region.name, Some("playlist_sidebar".to_string()));
    assert_eq!(
      region.bounds,
      Some(ViewBounds::new(0.0, 80.0, 250.0, 640.0))
    );
    assert_eq!(region.coordinate_space, Some("window".to_string()));
  }

  #[test]
  fn detect_sidebar_region_starts_at_playlist_marker() {
    let region = detect_sidebar_region(
      None,
      auv_driver::Size::new(1646.0, 1053.0),
      &fake_recognition(vec![
        ("推荐", 8.0, 20.0, 40.0, 20.0),
        ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 485.0, 120.0, 20.0),
        ("Reverberation", 98.0, 994.0, 160.0, 20.0),
      ]),
    )
    .expect("playlist marker should define the scroll body");

    assert_eq!(
      region.bounds,
      Some(ViewBounds::new(0.0, 443.0, 344.28, 528.0))
    );
  }

  #[test]
  fn parse_viewport_ignores_bottom_player_bar_outside_sidebar_bounds() {
    let recognition = fake_recognition(vec![
      ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
      ("Coding BGM", 72.0, 485.0, 120.0, 20.0),
      ("Reverberation", 98.0, 994.0, 160.0, 20.0),
      ("1w+", 322.0, 1003.0, 19.0, 9.0),
      ("伊藤賢", 98.0, 1018.0, 45.0, 17.0),
    ]);

    let observation =
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 443.0, 344.0, 528.0), &recognition);

    assert!(
      observation
        .candidates
        .iter()
        .any(|candidate| candidate.label.as_deref() == Some("Coding BGM"))
    );
    assert!(
      observation
        .candidates
        .iter()
        .all(|candidate| candidate.label.as_deref() != Some("Reverberation"))
    );
    assert!(
      observation
        .candidates
        .iter()
        .all(|candidate| candidate.label.as_deref() != Some("1w+"))
    );
  }

  #[test]
  fn detect_sidebar_region_falls_back_to_full_sidebar_without_playlist_marker() {
    let region = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![("推荐", 8.0, 20.0, 40.0, 20.0)]),
    )
    .expect("navigation marker should preserve full sidebar fallback");

    assert_eq!(region.bounds, Some(ViewBounds::new(0.0, 0.0, 228.0, 718.0)));
  }

  #[test]
  fn detect_sidebar_region_rejects_unanchored_playlist_like_rows() {
    let error = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![
        ("Future Garage", 72.0, 320.0, 140.0, 20.0),
        ("Progressive House", 72.0, 366.0, 170.0, 20.0),
        ("Trance", 72.0, 412.0, 80.0, 20.0),
      ]),
    )
    .expect_err("playlist-like rows without a sidebar marker should not anchor the sidebar");

    assert_eq!(error.code, "sidebar_region_not_found");
  }

  #[test]
  fn detect_sidebar_region_ignores_main_content_without_sidebar_marker() {
    let error = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![
        ("网易云音乐", 52.0, 40.0, 100.0, 20.0),
        ("Future Garage", 72.0, 320.0, 140.0, 20.0),
        ("Progressive House", 72.0, 366.0, 170.0, 20.0),
        ("Trance", 72.0, 412.0, 80.0, 20.0),
        ("每日推荐", 430.0, 300.0, 120.0, 30.0),
        ("推荐歌单", 520.0, 520.0, 150.0, 30.0),
      ]),
    )
    .expect_err("main content rows should not anchor the sidebar");

    assert_eq!(error.code, "sidebar_region_not_found");
  }

  #[test]
  fn detect_default_screen_restore_targets_song_detail_back_affordance() {
    let restore = detect_default_screen_restore(
      &fake_recognition(vec![
        ("私藏推荐", 90.0, 86.0, 120.0, 28.0),
        ("评论", 760.0, 182.0, 80.0, 28.0),
        ("收藏", 880.0, 182.0, 80.0, 28.0),
      ]),
      auv_driver::Size::new(1646.0, 1053.0),
    )
    .expect("song detail screen should expose a restore click");

    assert_eq!(restore.reason, DefaultScreenRestoreReason::SongDetailScreen);
    assert_eq!(restore.point, auv_driver::Point::new(82.602, 16.336));
  }

  #[test]
  fn detect_default_screen_restore_ignores_normal_sidebar_screen() {
    let restore = detect_default_screen_restore(
      &fake_recognition(vec![
        ("推荐", 8.0, 20.0, 40.0, 20.0),
        ("评论", 760.0, 182.0, 80.0, 28.0),
        ("收藏", 880.0, 182.0, 80.0, 28.0),
      ]),
      auv_driver::Size::new(1646.0, 1053.0),
    );

    assert_eq!(restore, None);
  }

  #[test]
  fn detect_blocking_modal_reports_cancel_or_open_dialog_markers() {
    let diagnostic = detect_blocking_modal(&fake_recognition(vec![
      ("打开", 760.0, 720.0, 80.0, 32.0),
      ("取消", 860.0, 720.0, 80.0, 32.0),
    ]))
    .expect("open dialog markers should be reported as blocking modal");

    assert_eq!(diagnostic.code, "blocking_modal_dialog");
  }

  #[test]
  fn parse_viewport_assigns_unique_candidate_ids_for_duplicate_cjk_labels() {
    let recognition = fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("中文歌单", 32.0, 74.0, 120.0, 20.0),
      ("中文歌单", 32.0, 106.0, 120.0, 20.0),
    ]);
    let observation =
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);
    let candidate_ids = observation
      .candidates
      .iter()
      .map(|candidate| candidate.id.as_str())
      .collect::<Vec<_>>();
    let unique_candidate_ids = candidate_ids
      .iter()
      .copied()
      .collect::<std::collections::HashSet<_>>();

    assert_eq!(observation.candidates.len(), 3);
    assert_eq!(
      candidate_ids,
      vec![
        "obs0.candidate.ocr0._____",
        "obs0.candidate.ocr1.____",
        "obs0.candidate.ocr2.____"
      ]
    );
    assert_eq!(unique_candidate_ids.len(), observation.candidates.len());
  }

  #[test]
  fn parse_viewport_treats_playlist_named_rows_as_items_not_sections() {
    let recognition = fake_recognition(vec![
      ("创建的歌单 215", 8.0, 42.0, 120.0, 20.0),
      ("年度精选歌单", 72.0, 74.0, 180.0, 20.0),
      ("猫音歌单", 72.0, 106.0, 120.0, 20.0),
    ]);
    let observation =
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 280.0, 400.0), &recognition);

    assert_eq!(
      observation.candidates[0].kind,
      SidebarCandidateKind::SectionHeader
    );
    assert_eq!(
      observation.candidates[1].kind,
      SidebarCandidateKind::PlaylistItem
    );
    assert_eq!(
      observation.candidates[2].kind,
      SidebarCandidateKind::PlaylistItem
    );
  }

  #[test]
  fn reconstruct_sidebar_groups_items_under_carried_section() {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    let page1 = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("Jazz", 32.0, 42.0, 80.0, 20.0),
        ("收藏的歌单", 8.0, 74.0, 110.0, 20.0),
        ("Road Trip", 32.0, 106.0, 120.0, 20.0),
      ]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![page0, page1],
    );

    assert_eq!(scan.projection.sections.len(), 2);
    assert_eq!(
      scan
        .projection
        .sections
        .iter()
        .map(|section| section.items.len())
        .sum::<usize>(),
      3
    );
    assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
    assert_eq!(
      scan.projection.sections[0].items[1].section_hint,
      Some(SidebarSectionKind::MyPlaylists)
    );
    assert_eq!(
      scan.projection.sections[1].items[0].section_hint,
      Some(SidebarSectionKind::FavoritePlaylists)
    );
    assert_eq!(scan.reconstruction.root.kind, ViewNodeKind::Collection);
    assert_eq!(scan.reconstruction.root.children.len(), 2);
  }

  #[test]
  fn created_category_scan_stops_at_favorite_landmark_before_scrolling_again() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
        ]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Road Trip", 32.0, 74.0, 120.0, 20.0),
        ]),
      ),
      parse_sidebar_viewport(
        2,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Should Not Scan", 32.0, 42.0, 140.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::Created,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 2);
    assert_eq!(observer.cursor, 1);
    assert_eq!(scan.projection.sections.len(), 1);
    assert_eq!(
      scan.projection.sections[0].kind,
      SidebarSectionKind::MyPlaylists
    );
    assert_eq!(scan.projection.sections[0].items.len(), 1);
    assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
    assert!(scan.interaction_events.iter().any(|event| {
      event.kind == InteractionEventKind::StopDecision
        && event.note.as_deref() == Some("reached_stop_landmark")
    }));
  }

  #[test]
  fn favorite_category_starts_collecting_at_favorite_landmark() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
        ]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Road Trip", 32.0, 74.0, 120.0, 20.0),
        ]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::Favorite,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.projection.sections.len(), 1);
    assert_eq!(
      scan.projection.sections[0].kind,
      SidebarSectionKind::FavoritePlaylists
    );
    assert_eq!(scan.projection.sections[0].items.len(), 1);
    assert_eq!(scan.projection.sections[0].items[0].label, "Road Trip");
  }

  #[test]
  fn reconstruct_sidebar_records_observe_and_scroll_interaction_events() {
    let mut first = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    first.source_artifacts = vec!["obs-0000-window.png".to_string()];
    let mut second = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)]),
    );
    second.source_artifacts = vec!["obs-0001-window.png".to_string()];
    second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
    let mut observer = FakeSidebarObserver::new(vec![first, second]);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 2,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      250,
    );

    assert!(scan.interaction_events.iter().any(|event| {
      event.kind == InteractionEventKind::Observe
        && event.observation_index == Some(0)
        && event.artifacts == vec!["obs-0000-window.png"]
    }));
    assert!(scan.interaction_events.iter().any(|event| {
      event.kind == InteractionEventKind::InputScroll
        && event.from_observation == Some(0)
        && event.to_observation == Some(1)
        && event.artifacts
          == vec![
            "obs-0000-window.png".to_string(),
            "obs-0001-window.png".to_string(),
          ]
        && event.scroll.as_ref().is_some_and(|scroll| {
          scroll.settle_ms == 250
            && scroll.delivery_path.as_deref() == Some("window_targeted_wheel")
        })
    }));
  }

  #[test]
  fn decode_playlist_sidebar_scan_json_accepts_current_schema() {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    let scan = reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![page0],
    );

    let json = serde_json::to_string(&scan).expect("scan should serialize");
    let decoded = decode_playlist_sidebar_scan_json(&json).expect("current schema should decode");

    assert_eq!(decoded, scan);
  }

  #[test]
  fn decode_playlist_sidebar_scan_json_rejects_missing_or_unknown_schema() {
    let missing = r#"{"projection":{"sections":[]}}"#;
    let missing_error = decode_playlist_sidebar_scan_json(missing)
      .expect_err("missing schema version should be rejected");
    assert!(missing_error.contains("missing schema_version"));

    let unknown = r#"{"schema_version":"view-ir-v999","projection":{"sections":[]}}"#;
    let unknown_error = decode_playlist_sidebar_scan_json(unknown)
      .expect_err("unknown schema version should be rejected");
    assert!(unknown_error.contains("unsupported playlist sidebar scan schema_version"));
  }

  #[test]
  fn reconstruct_sidebar_deduplicates_repeated_item_labels_in_same_section() {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    let page1 = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Coding BGM", 32.0, 42.0, 120.0, 20.0)]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![page0, page1],
    );

    assert_eq!(scan.projection.sections[0].items.len(), 1);
    assert!(
      scan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "deduplicated_item")
    );
  }

  #[test]
  fn reconstruct_sidebar_reports_ocr_evidence_without_reliable_candidates() {
    // ROOT CAUSE:
    //
    // If OCR produced evidence but every node was rejected as an unreliable
    // sidebar candidate, reconstruction returned a clean empty projection.
    //
    // Before the fix, JSON consumers could not distinguish an empty sidebar
    // from a parser rejection. The fix keeps that boundary explicit.
    let observation = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("搜索框占位", 8.0, 42.0, 120.0, 20.0)]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![observation],
    );

    assert!(scan.projection.sections.is_empty());
    assert!(
      scan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "parser_no_reliable_candidates")
    );
  }

  #[test]
  fn reconstruct_sidebar_deduplicates_items_per_actual_section() {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    let page1 = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Coding BGM", 32.0, 42.0, 120.0, 20.0)]),
    );
    let page2 = parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("我的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext::default(),
      ScanWindowContext::default(),
      ViewRegionRecord::default(),
      vec![page0, page1, page2],
    );

    assert_eq!(scan.projection.sections.len(), 2);
    assert_eq!(
      scan.projection.sections[0].kind,
      SidebarSectionKind::MyPlaylists
    );
    assert_eq!(scan.projection.sections[0].items.len(), 1);
    assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
    assert_eq!(
      scan.projection.sections[1].kind,
      SidebarSectionKind::MyPlaylists
    );
    assert_eq!(scan.projection.sections[1].items.len(), 1);
    assert_eq!(scan.projection.sections[1].items[0].label, "Coding BGM");
    assert_eq!(
      scan
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "deduplicated_item")
        .count(),
      1
    );
  }

  #[test]
  fn scan_loop_stops_on_repeated_viewport_fingerprint() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
        ]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        2,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.window.id, Some("fake".to_string()));
    assert_eq!(scan.observations.len(), 3);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
    assert!(
      scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("repeated_viewport_fingerprint"))
    );
  }

  #[test]
  fn scan_loop_stops_after_two_scrolls_with_no_motion_evidence() {
    let mut first = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]),
    );
    first.viewport_fingerprint = "page-a".to_string();
    let mut second = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("B", 32.0, 42.0, 80.0, 20.0)]),
    );
    second.viewport_fingerprint = "page-b".to_string();
    second.scroll_motion = Some(MotionEvidence {
      estimated_shift_y: 9,
      normalized_diff: 0.24,
      no_motion: false,
    });
    let mut third = parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("C", 32.0, 42.0, 80.0, 20.0)]),
    );
    third.viewport_fingerprint = "page-c".to_string();
    third.scroll_motion = Some(MotionEvidence {
      estimated_shift_y: 0,
      normalized_diff: 0.0,
      no_motion: true,
    });
    let mut fourth = parse_sidebar_viewport(
      3,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("D", 32.0, 42.0, 80.0, 20.0)]),
    );
    fourth.viewport_fingerprint = "page-d".to_string();
    fourth.scroll_motion = Some(MotionEvidence {
      estimated_shift_y: 0,
      normalized_diff: 0.0,
      no_motion: true,
    });
    let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 4);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
    assert!(
      scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("scroll_no_motion_after_input"))
    );
    assert!(
      !scan
        .known_limits
        .iter()
        .any(|limit| limit.contains("max_scrolls"))
    );
  }

  #[test]
  fn scan_loop_does_not_stop_scroll_on_no_motion_without_prior_motion() {
    let observations = (0..4)
      .map(|index| {
        let mut observation = parse_sidebar_viewport(
          index,
          ViewBounds::new(0.0, 0.0, 240.0, 400.0),
          &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]),
        );
        observation.viewport_fingerprint = format!("page-{index}");
        if index > 0 {
          observation.scroll_motion = Some(MotionEvidence {
            estimated_shift_y: 0,
            normalized_diff: 0.0,
            no_motion: true,
          });
        }
        observation
      })
      .collect::<Vec<_>>();
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 3,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 4);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Unknown);
    assert!(
      !scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("scroll_no_motion_after_input"))
    );
    assert!(
      scan
        .known_limits
        .iter()
        .any(|limit| limit.contains("max_scrolls=3"))
    );
  }

  #[test]
  fn scan_loop_does_not_stop_scroll_on_no_motion_from_noop_delivery() {
    let observations = (0..4)
      .map(|index| {
        let mut observation = parse_sidebar_viewport(
          index,
          ViewBounds::new(0.0, 0.0, 240.0, 400.0),
          &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]),
        );
        observation.viewport_fingerprint = format!("page-{index}");
        if index > 0 {
          observation.incoming_scroll_delivery_path = Some("noop".to_string());
          observation.scroll_motion = Some(MotionEvidence {
            estimated_shift_y: 0,
            normalized_diff: 0.0,
            no_motion: true,
          });
        }
        observation
      })
      .collect::<Vec<_>>();
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 3,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 4);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Unknown);
    assert!(
      !scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("scroll_no_motion_after_input"))
    );
  }

  #[test]
  fn scan_loop_stops_after_two_scrolls_with_no_new_semantic_candidates() {
    let mut first = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    first.viewport_fingerprint = "page-a".to_string();
    let mut second = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 106.0, 120.0, 20.0),
      ]),
    );
    second.viewport_fingerprint = "page-b".to_string();
    second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
    let mut third = parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 138.0, 120.0, 20.0),
      ]),
    );
    third.viewport_fingerprint = "page-c".to_string();
    third.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
    let fourth = parse_sidebar_viewport(
      3,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Fresh Playlist", 32.0, 170.0, 120.0, 20.0),
      ]),
    );
    let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 3);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
    assert!(
      scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input"))
    );
    assert!(
      !scan
        .known_limits
        .iter()
        .any(|limit| limit.contains("max_scrolls"))
    );
  }

  #[test]
  fn scan_loop_ignores_scroll_no_new_semantic_candidates_from_noop_delivery() {
    let mut first = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    first.viewport_fingerprint = "page-a".to_string();
    let mut second = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 106.0, 120.0, 20.0),
      ]),
    );
    second.viewport_fingerprint = "page-b".to_string();
    second.incoming_scroll_delivery_path = Some("noop".to_string());
    let mut third = parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 138.0, 120.0, 20.0),
      ]),
    );
    third.viewport_fingerprint = "page-c".to_string();
    third.incoming_scroll_delivery_path = Some("noop".to_string());
    let mut fourth = parse_sidebar_viewport(
      3,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Fresh Playlist", 32.0, 170.0, 120.0, 20.0),
      ]),
    );
    fourth.viewport_fingerprint = "page-d".to_string();
    let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 4);
    assert!(
      !scan
        .interaction_events
        .iter()
        .any(|event| event.kind == InteractionEventKind::StopDecision
          && event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input"))
    );
  }

  #[test]
  fn favorite_category_does_not_stop_on_no_new_candidates_before_start_landmark() {
    let mut first = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]),
    );
    first.viewport_fingerprint = "page-a".to_string();
    let mut second = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]),
    );
    second.viewport_fingerprint = "page-b".to_string();
    second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
    let mut third = parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Road Trip", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    third.viewport_fingerprint = "page-c".to_string();
    third.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
    let mut observer = FakeSidebarObserver::new(vec![first, second, third]);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 10,
        max_scrolls: 10,
      },
      PlaylistCategory::Favorite,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.projection.sections.len(), 1);
    assert_eq!(
      scan.projection.sections[0].kind,
      SidebarSectionKind::FavoritePlaylists
    );
    assert_eq!(scan.projection.sections[0].items.len(), 1);
    assert_eq!(scan.projection.sections[0].items[0].label, "Road Trip");
    assert!(
      !scan
        .interaction_events
        .iter()
        .any(|event| event.note.as_deref()
          == Some("scroll_no_new_semantic_candidates_after_input"))
    );
  }

  #[test]
  fn crop_image_projects_logical_sidebar_bounds_into_capture_pixels() {
    let mut image = RgbaImage::new(16, 16);
    for y in 0..16 {
      for x in 0..16 {
        image.put_pixel(x, y, Rgba([x as u8, y as u8, 0, 255]));
      }
    }

    let cropped = crop_image(&image, ViewBounds::new(2.0, 3.0, 4.0, 5.0), 2.0);

    assert_eq!(cropped.width(), 8);
    assert_eq!(cropped.height(), 10);
    assert_eq!(cropped.get_pixel(0, 0), &Rgba([4, 6, 0, 255]));
    assert_eq!(cropped.get_pixel(7, 9), &Rgba([11, 15, 0, 255]));
  }

  #[test]
  fn daily_recommended_card_click_point_targets_card_body_from_title_bounds() {
    let bounds = ViewBounds::new(430.0, 102.0, 72.0, 20.0);

    let point = daily_recommended_card_click_point(bounds);

    assert_eq!(point, auv_driver::Point::new(485.0, 182.0));
  }

  #[test]
  fn daily_recommended_card_click_point_handles_bottom_title_bounds() {
    let bounds = ViewBounds::new(430.0, 278.0, 145.0, 36.0);

    let point = daily_recommended_card_click_point(bounds);

    assert_eq!(point, auv_driver::Point::new(500.0, 183.0));
  }

  #[test]
  fn classify_playback_control_state_distinguishes_pause_from_play_icon() {
    let pause = playback_control_fixture(PlaybackControlState::PauseVisible);
    let play = playback_control_fixture(PlaybackControlState::PlayVisible);

    assert_eq!(
      classify_bottom_playback_control_state(&pause),
      PlaybackControlState::PauseVisible
    );
    assert_eq!(
      classify_bottom_playback_control_state(&play),
      PlaybackControlState::PlayVisible
    );
  }

  fn playback_control_fixture(state: PlaybackControlState) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(200, 120, Rgba([14, 15, 24, 255]));
    match state {
      PlaybackControlState::PauseVisible => {
        paint_control_columns(&mut image, &[92..=96, 104..=108]);
      }
      PlaybackControlState::PlayVisible => {
        paint_control_columns(&mut image, &[96..=108]);
      }
      PlaybackControlState::Unknown => {}
    }
    image
  }

  fn paint_control_columns(image: &mut RgbaImage, columns: &[std::ops::RangeInclusive<u32>]) {
    for column in columns {
      for x in column.clone() {
        for y in 72..=94 {
          image.put_pixel(x, y, Rgba([255, 255, 255, 255]));
        }
      }
    }
  }

  #[test]
  fn scan_loop_ignores_shared_page_budget_and_scans_until_boundary() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("B", 32.0, 42.0, 80.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new(observations);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 1,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 2);
    assert!(
      !scan
        .known_limits
        .iter()
        .any(|limit| limit.contains("max_pages"))
    );
    assert!(
      scan
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "no_more_fake_observations")
    );
  }

  #[test]
  fn scan_loop_rewinds_to_top_before_collecting_pages() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Middle Playlist", 32.0, 42.0, 120.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new_at(observations, 1);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 1,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 2);
    assert_eq!(
      scan.observations[0].candidates[0].label.as_deref(),
      Some("创建的歌单")
    );
    assert_eq!(
      scan.observations[1].candidates[0].label.as_deref(),
      Some("Middle Playlist")
    );
    assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
  }

  #[test]
  fn scan_loop_clears_top_seek_scroll_metadata_before_collection() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Middle Playlist", 32.0, 42.0, 120.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new_at(observations, 1);

    let scan = scan_sidebar_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 1,
        max_scrolls: 10,
      },
      PlaylistCategory::All,
      300.0,
      DEFAULT_SCROLL_SETTLE_MS,
    );

    assert_eq!(scan.observations.len(), 2);
    assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
    assert_eq!(scan.observations[0].incoming_scroll_delivery_path, None);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn vertical_scrollbar_boundary_prefers_page_button_height_over_plain_scrollbar_geometry() {
    let window = auv_driver::Window {
      reference: auv_driver::WindowRef {
        id: "42".to_string(),
      },
      title: Some("网易云音乐".to_string()),
      app_name: Some("网易云音乐".to_string()),
      app_bundle_id: Some("com.netease.163music".to_string()),
      process_id: Some(42),
      frame: auv_driver::Rect::new(100.0, 200.0, 400.0, 600.0),
      coordinate_space: auv_driver::geometry::CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };
    let sidebar_bounds = ViewBounds::new(20.0, 30.0, 160.0, 520.0);
    let nodes = vec![
      auv_driver_macos::types::ObservedAxNode {
        depth: 2,
        path: "0.0.1".to_string(),
        role: "AXScrollBar".to_string(),
        subrole: String::new(),
        title: String::new(),
        description: String::new(),
        help: String::new(),
        identifier: "_NS:1".to_string(),
        placeholder: String::new(),
        value: "0.6".to_string(),
        bounds: auv_driver_macos::types::ObservedRect {
          x: 272,
          y: 260,
          width: 18,
          height: 480,
        },
      },
      auv_driver_macos::types::ObservedAxNode {
        depth: 3,
        path: "0.0.1.3".to_string(),
        role: "AXButton".to_string(),
        subrole: "AXIncrementPage".to_string(),
        title: String::new(),
        description: String::new(),
        help: String::new(),
        identifier: String::new(),
        placeholder: String::new(),
        value: String::new(),
        bounds: auv_driver_macos::types::ObservedRect {
          x: 272,
          y: 260,
          width: 18,
          height: 0,
        },
      },
      auv_driver_macos::types::ObservedAxNode {
        depth: 3,
        path: "0.0.1.4".to_string(),
        role: "AXButton".to_string(),
        subrole: "AXDecrementPage".to_string(),
        title: String::new(),
        description: String::new(),
        help: String::new(),
        identifier: String::new(),
        placeholder: String::new(),
        value: String::new(),
        bounds: auv_driver_macos::types::ObservedRect {
          x: 272,
          y: 740,
          width: 18,
          height: 24,
        },
      },
    ];

    assert_eq!(
      sidebar_ax_scrollbar_boundary(&nodes, &window, sidebar_bounds),
      Some(SidebarScrollbarBoundary::Bottom)
    );
  }

  struct FakeSidebarObserver {
    observations: Vec<SidebarViewportObservation>,
    cursor: usize,
    pending_scroll_delivery_path: Option<String>,
    last_scroll_seek_cursor: Option<usize>,
  }

  impl FakeSidebarObserver {
    fn new(observations: Vec<SidebarViewportObservation>) -> Self {
      Self {
        observations,
        cursor: 0,
        pending_scroll_delivery_path: None,
        last_scroll_seek_cursor: None,
      }
    }

    fn new_at(observations: Vec<SidebarViewportObservation>, cursor: usize) -> Self {
      Self {
        observations,
        cursor,
        pending_scroll_delivery_path: None,
        last_scroll_seek_cursor: None,
      }
    }
  }

  impl SidebarScanObserver for FakeSidebarObserver {
    fn reset_collection_phase(&mut self) {
      self.pending_scroll_delivery_path = None;
      self.last_scroll_seek_cursor = None;
    }

    fn observe_scroll_seek(
      &mut self,
      observation_index: usize,
    ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
      let cursor = self.cursor;
      let mut observation = self.observe(observation_index)?;
      if observation.scroll_motion.is_none() {
        if let Some(previous_cursor) = self.last_scroll_seek_cursor {
          if observation.incoming_scroll_delivery_path.is_some() {
            let no_motion = cursor == previous_cursor;
            observation.scroll_motion = Some(MotionEvidence {
              estimated_shift_y: if no_motion { 0 } else { 1 },
              normalized_diff: if no_motion { 0.0 } else { 0.2 },
              no_motion,
            });
          }
        }
      }
      self.last_scroll_seek_cursor = Some(cursor);
      Ok(observation)
    }
  }

  impl ViewObserver for FakeSidebarObserver {
    type Observation = SidebarViewportObservation;

    fn observe(
      &mut self,
      observation_index: usize,
    ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
      let mut observation =
        self
          .observations
          .get(self.cursor)
          .cloned()
          .ok_or_else(|| ParserDiagnostic {
            code: "no_more_fake_observations".to_string(),
            message: "fake sidebar observer has no more observations".to_string(),
            node_id: None,
          })?;
      let pending_scroll_delivery_path = self.pending_scroll_delivery_path.take();
      if observation.incoming_scroll_delivery_path.is_none() {
        observation.incoming_scroll_delivery_path = pending_scroll_delivery_path;
      }
      observation.observation_index = observation_index;
      observation.viewport.page_index = observation_index;
      Ok(observation)
    }

    fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic> {
      self
        .observations
        .get(self.cursor)
        .cloned()
        .ok_or_else(|| ParserDiagnostic {
          code: "no_more_fake_observations".to_string(),
          message: "fake sidebar observer has no more observations".to_string(),
          node_id: None,
        })
    }

    fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
      self.cursor = self.cursor.saturating_sub(1);
      self.pending_scroll_delivery_path = Some("fake_scroll_up".to_string());
      Ok(())
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      self.cursor += 1;
      self.pending_scroll_delivery_path = Some("fake_scroll_down".to_string());
      Ok(())
    }
  }

  fn fake_recognition(
    rows: Vec<(&str, f64, f64, f64, f64)>,
  ) -> auv_driver::vision::TextRecognition {
    auv_driver::vision::TextRecognition {
      text: rows
        .iter()
        .map(|(text, _, _, _, _)| *text)
        .collect::<Vec<_>>()
        .join("\n"),
      regions: rows
        .into_iter()
        .map(
          |(text, x, y, width, height)| auv_driver::vision::RecognizedText {
            text: text.to_string(),
            bounds: auv_driver::Rect::new(x, y, width, height),
            confidence: Some(0.92),
          },
        )
        .collect(),
    }
  }
}
