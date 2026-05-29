use std::path::PathBuf;

use auv_driver::vision::TextRecognition;
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use auv_driver::capture::Capture;
#[cfg(target_os = "macos")]
use auv_driver::selector::{App, Window};
#[cfg(target_os = "macos")]
use auv_driver::{Driver, InputPolicy, RatioRect, Scroll, ScrollOptions, Size};
#[cfg(target_os = "macos")]
use auv_driver_macos::{MacosDriver, MacosDriverSession};

const DEFAULT_APP_ID: &str = "com.netease.163music";
const DEFAULT_ARTIFACT_DIR: &str = "/tmp/auv-netease-playlist-ls-artifacts";
/// Wire-shape contract for `PlaylistSidebarScan` JSON output. Readers must
/// reject artifacts whose `schema_version` is not understood. Bump the value
/// only when the on-wire shape changes in a non-additive way.
const VIEW_IR_SCHEMA_VERSION: &str = "view-ir-v0";
#[cfg(target_os = "macos")]
const LIVE_SCROLL_SETTLE_MS: u64 = 500;

#[derive(Clone, Debug, PartialEq)]
struct Inputs {
  app_id: String,
  json_out: Option<PathBuf>,
  artifact_dir: PathBuf,
  max_pages: usize,
  max_scrolls: usize,
  scroll_amount: f64,
  sidebar_region: Option<RatioRegion>,
  print_json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanOptions {
  max_pages: usize,
  max_scrolls: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RatioRegion {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
}

impl RatioRegion {
  const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }
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
struct PlaylistSidebarScan {
  /// Wire-shape version of this artifact. See `VIEW_IR_SCHEMA_VERSION`.
  schema_version: String,
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  observations: Vec<SidebarViewportObservation>,
  reconstruction: ViewReconstructionRecord,
  projection: PlaylistSidebarProjection,
  boundary: ScrollBoundarySummary,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ScanAppContext {
  app_id: Option<String>,
  name: Option<String>,
  version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ScanWindowContext {
  id: Option<String>,
  title: Option<String>,
  bounds: Option<ViewBounds>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewRegionRecord {
  id: Option<String>,
  name: Option<String>,
  bounds: Option<ViewBounds>,
  coordinate_space: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SidebarViewportObservation {
  observation_index: usize,
  viewport: ViewViewportRecord,
  source_artifacts: Vec<String>,
  viewport_fingerprint: String,
  evidence_nodes: Vec<ViewEvidenceNode>,
  candidates: Vec<SidebarViewportCandidate>,
  parser_notes: Vec<ParserDiagnostic>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewViewportRecord {
  page_index: usize,
  bounds: ViewBounds,
  axis: ViewAxis,
  scroll_offset: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewEvidenceNode {
  id: String,
  source: ViewEvidenceSource,
  label: Option<String>,
  bounds: Option<ViewBounds>,
  confidence: Confidence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewEvidenceSource {
  OcrText,
  AxNode,
  IconMatch,
  #[default]
  Visual,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarCandidateKind {
  SectionHeader,
  PlaylistItem,
  NavigationItem,
  #[default]
  Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewReconstructionRecord {
  root: ViewNodeRecord,
  anchor_index: Vec<ViewAnchor>,
  landmark_index: Vec<ViewLandmark>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewNodeRecord {
  id: String,
  kind: ViewNodeKind,
  domain_kind: Option<String>,
  layout: Option<ViewLayout>,
  label: Option<String>,
  bounds: ViewBounds,
  scrollable: Option<ViewScrollable>,
  anchors: Vec<ViewAnchor>,
  landmarks: Vec<ViewLandmark>,
  actions: Vec<ViewAction>,
  evidence: Vec<ViewEvidenceNode>,
  children: Vec<ViewNodeRecord>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewNodeKind {
  Container,
  Collection,
  Section,
  Item,
  Text,
  Icon,
  #[default]
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewLayout {
  VStack,
  HStack,
  Group,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewAxis {
  #[default]
  Vertical,
  Horizontal,
  Both,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ViewScrollable {
  axis: ViewAxis,
  boundary: ScrollBoundarySummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewAnchor {
  id: String,
  label: String,
  strength: AnchorStrength,
  bounds: ViewBounds,
  evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AnchorStrength {
  #[default]
  Strong,
  Medium,
  Weak,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewLandmark {
  id: String,
  label: String,
  #[serde(rename = "use")]
  landmark_use: LandmarkUse,
  bounds: ViewBounds,
  evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LandmarkUse {
  ViewportPose,
  BoundaryDetection,
  AnchorReacquire,
  #[default]
  SectionAssignment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewAction {
  Open,
  Select,
  Scroll,
  ObserveOnly,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ScrollBoundarySummary {
  top: BoundaryConfidence,
  bottom: BoundaryConfidence,
  left: BoundaryConfidence,
  right: BoundaryConfidence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BoundaryConfidence {
  Confirmed,
  Likely,
  #[default]
  Unknown,
  Contradicted,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PlaylistSidebarProjection {
  sections: Vec<SidebarSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SidebarSection {
  id: String,
  kind: SidebarSectionKind,
  label: Option<String>,
  items: Vec<PlaylistSidebarItem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarSectionKind {
  FeatureNav,
  LibraryNav,
  PlaylistNav,
  MyPlaylists,
  FavoritedPlaylists,
  #[default]
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PlaylistSidebarItem {
  id: String,
  label: String,
  section_hint: Option<SidebarSectionKind>,
  confidence: Confidence,
  candidate_id: Option<String>,
  anchor_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Confidence {
  High,
  Medium,
  #[default]
  Low,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ParserDiagnostic {
  code: String,
  message: String,
  node_id: Option<String>,
}

trait SidebarObserver {
  fn observe(
    &mut self,
    observation_index: usize,
  ) -> Result<SidebarViewportObservation, ParserDiagnostic>;
  fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic>;
  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic>;
  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewBounds {
  x: f64,
  y: f64,
  width: f64,
  height: f64,
}

impl ViewBounds {
  const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }
}

fn main() {
  if let Err(error) = run() {
    eprintln!("{error}");
    std::process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let inputs = parse_inputs(std::env::args().skip(1).collect())?;
  let scan = run_live_scan(&inputs)?;
  let json = serde_json::to_string_pretty(&scan).map_err(|error| error.to_string())?;

  if let Some(path) = &inputs.json_out {
    std::fs::write(path, &json)
      .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
  }

  if inputs.print_json {
    println!("{json}");
  } else {
    println!("{}", render_human_summary(&scan));
  }

  Ok(())
}

fn parse_inputs(args: Vec<String>) -> Result<Inputs, String> {
  let mut inputs = Inputs {
    app_id: DEFAULT_APP_ID.to_string(),
    json_out: None,
    artifact_dir: PathBuf::from(DEFAULT_ARTIFACT_DIR),
    max_pages: 24,
    max_scrolls: 48,
    scroll_amount: 6.0,
    sidebar_region: None,
    print_json: false,
  };

  let mut args = args.into_iter();
  while let Some(arg) = args.next() {
    match arg.as_str() {
      "--app-id" => {
        inputs.app_id = next_value(&mut args, "--app-id")?;
      }
      "--json-out" => {
        inputs.json_out = Some(PathBuf::from(next_value(&mut args, "--json-out")?));
      }
      "--artifact-dir" => {
        inputs.artifact_dir = PathBuf::from(next_value(&mut args, "--artifact-dir")?);
      }
      "--max-pages" => {
        inputs.max_pages = parse_usize("--max-pages", next_value(&mut args, "--max-pages")?)?;
        if inputs.max_pages == 0 {
          return Err("--max-pages must be greater than 0".to_string());
        }
      }
      "--max-scrolls" => {
        inputs.max_scrolls = parse_usize("--max-scrolls", next_value(&mut args, "--max-scrolls")?)?;
        if inputs.max_scrolls == 0 {
          return Err("--max-scrolls must be greater than 0".to_string());
        }
      }
      "--scroll-amount" => {
        inputs.scroll_amount =
          parse_f64("--scroll-amount", next_value(&mut args, "--scroll-amount")?)?;
        if !inputs.scroll_amount.is_finite() || inputs.scroll_amount <= 0.0 {
          return Err("--scroll-amount must be greater than 0".to_string());
        }
      }
      "--sidebar-region" => {
        inputs.sidebar_region = Some(parse_ratio_region(next_value(
          &mut args,
          "--sidebar-region",
        )?)?);
      }
      "--print-json" => {
        inputs.print_json = true;
      }
      other => return Err(format!("unknown argument {other}")),
    }
  }

  Ok(inputs)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
  args
    .next()
    .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_usize(flag: &str, value: String) -> Result<usize, String> {
  value
    .parse()
    .map_err(|_| format!("{flag} expects a positive integer"))
}

fn parse_f64(flag: &str, value: String) -> Result<f64, String> {
  value
    .parse()
    .map_err(|_| format!("{flag} expects a number"))
}

fn parse_ratio_region(value: String) -> Result<RatioRegion, String> {
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

  Ok(RatioRegion::new(parts[0], parts[1], parts[2], parts[3]))
}

fn render_human_summary(scan: &PlaylistSidebarScan) -> String {
  let mut lines = Vec::new();
  lines.push("NetEase playlist sidebar scan".to_string());
  lines.push(format!(
    "app: id={} name={} version={}",
    optional(scan.app.app_id.as_deref()),
    optional(scan.app.name.as_deref()),
    optional(scan.app.version.as_deref())
  ));
  lines.push(format!(
    "window: id={} title={} bounds={}",
    optional(scan.window.id.as_deref()),
    optional(scan.window.title.as_deref()),
    render_optional_bounds(scan.window.bounds)
  ));
  lines.push(format!(
    "sidebar_region: name={} bounds={}",
    optional(scan.sidebar_region.name.as_deref()),
    render_optional_bounds(scan.sidebar_region.bounds)
  ));
  lines.push(format!(
    "boundary: top={:?} bottom={:?} left={:?} right={:?}",
    scan.boundary.top, scan.boundary.bottom, scan.boundary.left, scan.boundary.right
  ));
  lines.push(format!("observations: {}", scan.observations.len()));
  lines.push("sections:".to_string());
  if scan.projection.sections.is_empty() {
    lines.push("  (none)".to_string());
  } else {
    for section in &scan.projection.sections {
      lines.push(format!(
        "  - {} [{:?}]",
        optional(section.label.as_deref()),
        section.kind
      ));
      if section.items.is_empty() {
        lines.push("    (no items)".to_string());
      } else {
        for item in &section.items {
          lines.push(format!(
            "    - {} confidence={:?} anchor={}",
            item.label,
            item.confidence,
            optional(item.anchor_id.as_deref())
          ));
        }
      }
    }
  }
  lines.push("diagnostics:".to_string());
  if scan.diagnostics.is_empty() {
    lines.push("  (none)".to_string());
  } else {
    for diagnostic in &scan.diagnostics {
      lines.push(format!(
        "  - {}: {}{}",
        diagnostic.code,
        diagnostic.message,
        diagnostic
          .node_id
          .as_deref()
          .map(|node_id| format!(" node={node_id}"))
          .unwrap_or_default()
      ));
    }
  }
  lines.push("known_limits:".to_string());
  if scan.known_limits.is_empty() {
    lines.push("  (none)".to_string());
  } else {
    for limit in &scan.known_limits {
      lines.push(format!("  - {limit}"));
    }
  }

  lines.join("\n")
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
fn run_live_scan(_inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  Err("live NetEase playlist sidebar scan is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn run_live_scan(inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  let driver = MacosDriver::new();
  let default_app_context = ScanAppContext {
    app_id: Some(inputs.app_id.clone()),
    name: None,
    version: None,
  };
  let mut session = match driver.open_local() {
    Ok(session) => session,
    Err(error) => {
      return Ok(empty_diagnostic_scan(
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
      return Ok(empty_diagnostic_scan(
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
      return Ok(empty_diagnostic_scan(
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
  let mut full_recognition = match session
    .vision()
    .recognize_text_in_capture(&capture, full_window)
  {
    Ok(recognition) => recognition_in_window_space(recognition, &capture),
    Err(error) => {
      return Ok(empty_diagnostic_scan(
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
    return Ok(empty_diagnostic_scan(
      app_context,
      window_context,
      ViewRegionRecord::default(),
      diagnostic,
      "scan stopped before sidebar observation because a blocking modal was detected",
    ));
  }

  if inputs.sidebar_region.is_none() {
    let broad_sidebar_bounds = broad_sidebar_probe_bounds(window_size);
    let broad_sidebar_ratio = bounds_to_ratio(broad_sidebar_bounds, &capture);
    let mut top_probe = LiveSidebarObserver {
      session,
      window: window.clone(),
      sidebar_bounds: broad_sidebar_bounds,
      sidebar_ratio: broad_sidebar_ratio,
      artifact_dir: inputs.artifact_dir.clone(),
      pending_artifacts: Vec::new(),
      scroll_amount: inputs.scroll_amount,
    };
    let top_seek = scroll_observer_to_top(&mut top_probe, inputs.max_scrolls);
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
        return Ok(empty_diagnostic_scan(
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
    full_recognition = match session
      .vision()
      .recognize_text_in_capture(&capture, full_window)
    {
      Ok(recognition) => recognition_in_window_space(recognition, &capture),
      Err(error) => {
        return Ok(empty_diagnostic_scan(
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
      return Ok(empty_diagnostic_scan(
        app_context,
        window_context,
        ViewRegionRecord::default(),
        diagnostic,
        "scan stopped before sidebar observation because the sidebar region could not be detected",
      ));
    }
  };
  let sidebar_bounds = sidebar_region.bounds.unwrap_or_default();
  let sidebar_ratio = bounds_to_ratio(sidebar_bounds, &capture);
  let mut observer = LiveSidebarObserver {
    session,
    window: window.clone(),
    sidebar_bounds,
    sidebar_ratio,
    artifact_dir: inputs.artifact_dir.clone(),
    pending_artifacts: Vec::new(),
    scroll_amount: inputs.scroll_amount,
  };
  let mut scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: inputs.max_pages,
      max_scrolls: inputs.max_scrolls,
    },
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

fn empty_diagnostic_scan(
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  diagnostic: ParserDiagnostic,
  known_limit: &str,
) -> PlaylistSidebarScan {
  let mut root = empty_root();
  if let Some(bounds) = sidebar_region.bounds {
    root.bounds = bounds;
  }

  PlaylistSidebarScan {
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
    diagnostics: vec![diagnostic],
    known_limits: vec![known_limit.to_string()],
  }
}

fn detect_sidebar_region(
  manual: Option<RatioRegion>,
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
    if let Some(region) = infer_visible_playlist_body_region(&left_regions, window_size) {
      return Ok(sidebar_region_record(region));
    }
    return Err(ParserDiagnostic {
      code: "sidebar_region_not_found".to_string(),
      message: "sidebar markers could not be identified on the left side".to_string(),
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

fn infer_visible_playlist_body_region(
  left_regions: &[&auv_driver::vision::RecognizedText],
  window_size: auv_driver::Size,
) -> Option<ViewBounds> {
  let rows = left_regions
    .iter()
    .filter(|region| {
      let label = region.text.trim();
      label.chars().count() >= 2
        && region.bounds.origin.x >= 48.0
        && region.bounds.origin.x <= window_size.width * 0.24
        && region.bounds.origin.y >= window_size.height * 0.25
    })
    .collect::<Vec<_>>();
  if rows.len() < 3 {
    return None;
  }

  let min_y = (rows
    .iter()
    .map(|region| region.bounds.origin.y)
    .fold(f64::INFINITY, f64::min)
    - 20.0)
    .clamp(0.0, window_size.height);
  let max_x = rows
    .iter()
    .map(|region| region.bounds.origin.x + region.bounds.size.width)
    .fold(0.0, f64::max)
    .max(window_size.width * 0.18)
    .min(window_size.width * 0.42);

  Some(ViewBounds::new(
    0.0,
    min_y,
    max_x + 48.0,
    playlist_sidebar_bottom(window_size) - min_y,
  ))
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

fn ratio_to_window_bounds(region: RatioRegion, window_size: auv_driver::Size) -> ViewBounds {
  ViewBounds::new(
    region.x * window_size.width,
    region.y * window_size.height,
    region.width * window_size.width,
    region.height * window_size.height,
  )
}

fn is_sidebar_marker(label: &str) -> bool {
  section_kind_from_label(label) != SidebarSectionKind::Unknown
    || matches!(label, "推荐" | "发现音乐" | "最近播放")
}

fn is_playlist_section_marker(label: &str) -> bool {
  section_kind_from_label(label) == SidebarSectionKind::MyPlaylists
    || section_kind_from_label(label) == SidebarSectionKind::FavoritedPlaylists
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
    viewport_fingerprint,
    evidence_nodes,
    candidates,
    parser_notes: Vec::new(),
  }
}

fn viewport_contains_center(viewport: ViewBounds, bounds: ViewBounds) -> bool {
  let center_x = bounds.x + bounds.width * 0.5;
  let center_y = bounds.y + bounds.height * 0.5;
  center_x >= viewport.x
    && center_x <= viewport.x + viewport.width
    && center_y >= viewport.y
    && center_y <= viewport.y + viewport.height
}

fn confidence_from_ocr(confidence: Option<f32>) -> Confidence {
  match confidence {
    Some(value) if value >= 0.85 => Confidence::High,
    Some(value) if value >= 0.65 => Confidence::Medium,
    _ => Confidence::Low,
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
  if section_kind_from_label(label) != SidebarSectionKind::Unknown {
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

fn section_kind_from_label(label: &str) -> SidebarSectionKind {
  let label = normalize_section_label(label);
  if label.contains("创建的歌单") || label.contains("我的歌单") {
    SidebarSectionKind::MyPlaylists
  } else if label.contains("收藏的歌单") {
    SidebarSectionKind::FavoritedPlaylists
  } else if label == "我的收藏" {
    SidebarSectionKind::LibraryNav
  } else if matches!(label.as_str(), "推荐" | "音乐服务") {
    SidebarSectionKind::FeatureNav
  } else {
    SidebarSectionKind::Unknown
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

fn viewport_fingerprint(nodes: &[ViewEvidenceNode]) -> String {
  nodes
    .iter()
    .filter_map(|node| node.label.as_deref())
    .map(normalize_identity)
    .collect::<Vec<_>>()
    .join("|")
}

fn normalize_identity(value: &str) -> String {
  value
    .trim()
    .to_lowercase()
    .chars()
    .filter(|ch| !ch.is_whitespace())
    .collect()
}

fn slug(value: &str) -> String {
  normalize_identity(value)
    .chars()
    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
    .collect()
}

fn reconstruct_playlist_sidebar(
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: ViewRegionRecord,
  observations: Vec<SidebarViewportObservation>,
) -> PlaylistSidebarScan {
  let boundary = boundary_summary_from_observations(&observations);
  let mut section_nodes = Vec::new();
  let mut projection_sections = Vec::new();
  let mut diagnostics = observations
    .iter()
    .flat_map(|observation| observation.parser_notes.clone())
    .collect::<Vec<_>>();
  let mut current_section_index = None;
  let mut section_indices = std::collections::HashMap::new();
  let mut seen_items_by_section = Vec::<std::collections::HashSet<String>>::new();

  for observation in &observations {
    for candidate in &observation.candidates {
      match candidate.kind {
        SidebarCandidateKind::SectionHeader => {
          let Some(label) = candidate.label.as_deref().map(str::trim) else {
            continue;
          };
          let kind = section_kind_from_label(label);
          let section_id = format!(
            "section.obs{}.{}.{}",
            observation.observation_index,
            candidate.id,
            slug(label)
          );
          let section_key = (kind, normalize_identity(label));
          if let Some(section_index) = section_indices.get(&section_key).copied() {
            current_section_index = Some(section_index);
          } else {
            section_nodes.push(section_node(
              &section_id,
              kind,
              label,
              candidate,
              observation,
            ));
            projection_sections.push(SidebarSection {
              id: section_id,
              kind,
              label: Some(label.to_string()),
              items: Vec::new(),
            });
            seen_items_by_section.push(std::collections::HashSet::new());
            let section_index = section_nodes.len() - 1;
            section_indices.insert(section_key, section_index);
            current_section_index = Some(section_index);
          }
        }
        SidebarCandidateKind::PlaylistItem | SidebarCandidateKind::NavigationItem => {
          let Some(label) = candidate.label.as_deref().map(str::trim) else {
            continue;
          };
          let section_index = current_section_index.get_or_insert_with(|| {
            let section_id = "section.unassigned".to_string();
            section_nodes.push(ViewNodeRecord {
              id: section_id.clone(),
              kind: ViewNodeKind::Section,
              domain_kind: Some(domain_kind_for_section(SidebarSectionKind::Unknown)),
              layout: Some(ViewLayout::VStack),
              label: None,
              bounds: ViewBounds::default(),
              scrollable: None,
              anchors: Vec::new(),
              landmarks: Vec::new(),
              actions: vec![ViewAction::ObserveOnly],
              evidence: Vec::new(),
              children: Vec::new(),
            });
            projection_sections.push(SidebarSection {
              id: section_id,
              kind: SidebarSectionKind::Unknown,
              label: None,
              items: Vec::new(),
            });
            seen_items_by_section.push(std::collections::HashSet::new());
            section_nodes.len() - 1
          });
          let section_hint = projection_sections[*section_index].kind;
          let dedupe_key = normalize_identity(label);

          if !seen_items_by_section[*section_index].insert(dedupe_key) {
            diagnostics.push(ParserDiagnostic {
              code: "deduplicated_item".to_string(),
              message: format!(
                "deduplicated repeated sidebar item {label:?} in section {:?}",
                section_hint
              ),
              node_id: Some(candidate.id.clone()),
            });
            continue;
          }

          let item_id = format!(
            "item.obs{}.{}.{}",
            observation.observation_index,
            candidate.id,
            slug(label)
          );
          let anchor_id = format!("anchor.{item_id}");
          let node = item_node(&item_id, &anchor_id, label, candidate, observation);
          attach_item_node(&mut section_nodes[*section_index], node);
          projection_sections[*section_index]
            .items
            .push(PlaylistSidebarItem {
              id: item_id,
              label: label.to_string(),
              section_hint: Some(section_hint),
              confidence: candidate.confidence,
              candidate_id: Some(candidate.id.clone()),
              anchor_id: Some(anchor_id),
            });
        }
        SidebarCandidateKind::Unknown => {}
      }
    }
  }

  let has_evidence = observations
    .iter()
    .any(|observation| !observation.evidence_nodes.is_empty());
  let has_candidates = observations
    .iter()
    .any(|observation| !observation.candidates.is_empty());
  if has_evidence && !has_candidates && projection_sections.is_empty() {
    diagnostics.push(ParserDiagnostic {
      code: "parser_no_reliable_candidates".to_string(),
      message: "OCR evidence was observed but no reliable sidebar candidates were accepted"
        .to_string(),
      node_id: None,
    });
  }

  let root = ViewNodeRecord {
    id: "root.sidebar".to_string(),
    kind: ViewNodeKind::Collection,
    domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
    layout: Some(ViewLayout::VStack),
    label: None,
    bounds: sidebar_region.bounds.unwrap_or_default(),
    scrollable: Some(ViewScrollable {
      axis: ViewAxis::Vertical,
      boundary: boundary.clone(),
    }),
    anchors: Vec::new(),
    landmarks: Vec::new(),
    actions: vec![ViewAction::Scroll],
    evidence: Vec::new(),
    children: section_nodes,
  };
  let mut anchor_index = Vec::new();
  let mut landmark_index = Vec::new();
  collect_anchors(&root, &mut anchor_index);
  collect_landmarks(&root, &mut landmark_index);

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
    projection: PlaylistSidebarProjection {
      sections: projection_sections,
    },
    boundary,
    diagnostics,
    known_limits: Vec::new(),
  }
}

fn scan_sidebar_with_observer(
  observer: &mut impl SidebarObserver,
  options: ScanOptions,
) -> PlaylistSidebarScan {
  let mut observations = Vec::new();
  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();
  let top_seek = scroll_observer_to_top(observer, options.max_scrolls);
  diagnostics.extend(top_seek.diagnostics);
  known_limits.extend(top_seek.known_limits);
  let mut previous_fingerprint = None;
  let mut scrolls = 0;

  loop {
    if observations.len() >= options.max_pages {
      known_limits.push(format!("stopped after max_pages={}", options.max_pages));
      break;
    }

    let observation_index = observations.len();
    let observation = match observer.observe(observation_index) {
      Ok(observation) => observation,
      Err(diagnostic) => {
        diagnostics.push(diagnostic);
        break;
      }
    };
    let repeated_fingerprint = previous_fingerprint
      .as_deref()
      .is_some_and(|fingerprint| fingerprint == observation.viewport_fingerprint);
    previous_fingerprint = Some(observation.viewport_fingerprint.clone());
    observations.push(observation);

    if repeated_fingerprint {
      break;
    }

    if observations.len() >= options.max_pages {
      known_limits.push(format!("stopped after max_pages={}", options.max_pages));
      break;
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
    observations,
  );
  scan.diagnostics.extend(diagnostics);
  scan.known_limits.extend(known_limits);
  if top_seek.boundary == BoundaryConfidence::Likely {
    apply_top_boundary(&mut scan, top_seek.boundary);
  }
  scan
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TopSeekOutcome {
  boundary: BoundaryConfidence,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

fn scroll_observer_to_top(
  observer: &mut impl SidebarObserver,
  max_scrolls: usize,
) -> TopSeekOutcome {
  let mut outcome = TopSeekOutcome::default();
  let mut previous_fingerprint = match observer.observe_probe() {
    Ok(observation) => observation.viewport_fingerprint,
    Err(diagnostic) => {
      outcome.diagnostics.push(diagnostic);
      return outcome;
    }
  };

  for _ in 0..max_scrolls {
    if let Err(diagnostic) = observer.scroll_up() {
      outcome.diagnostics.push(diagnostic);
      return outcome;
    }

    let observation = match observer.observe_probe() {
      Ok(observation) => observation,
      Err(diagnostic) => {
        outcome.diagnostics.push(diagnostic);
        return outcome;
      }
    };
    if observation.viewport_fingerprint == previous_fingerprint {
      outcome.boundary = BoundaryConfidence::Likely;
      return outcome;
    }
    previous_fingerprint = observation.viewport_fingerprint;
  }

  outcome
    .known_limits
    .push(format!("top seek stopped after max_scrolls={max_scrolls}"));
  outcome
}

fn apply_top_boundary(scan: &mut PlaylistSidebarScan, top: BoundaryConfidence) {
  scan.boundary.top = top;
  if let Some(scrollable) = scan.reconstruction.root.scrollable.as_mut() {
    scrollable.boundary.top = top;
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
  artifact_dir: PathBuf,
  pending_artifacts: Vec<std::thread::JoinHandle<Result<(), String>>>,
  scroll_amount: f64,
}

#[cfg(target_os = "macos")]
impl LiveSidebarObserver {
  fn capture_observation(
    &mut self,
    observation_index: usize,
  ) -> Result<(RgbaImage, TextRecognition, SidebarViewportObservation), ParserDiagnostic> {
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
      .recognize_text_in_capture(&capture, self.sidebar_ratio)
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_ocr_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;

    let window_recognition = recognition_in_window_space(recognition, &capture);
    let observation =
      parse_sidebar_viewport(observation_index, self.sidebar_bounds, &window_recognition);

    Ok((capture.image.clone(), window_recognition, observation))
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
impl SidebarObserver for LiveSidebarObserver {
  fn observe(
    &mut self,
    observation_index: usize,
  ) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (image, window_recognition, mut observation) =
      self.capture_observation(observation_index)?;
    observation.source_artifacts = self.write_observation_artifacts(
      observation_index,
      image,
      window_recognition,
      observation.clone(),
    );

    Ok(observation)
  }

  fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (_, _, observation) = self.capture_observation(0)?;
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
impl LiveSidebarObserver {
  fn scroll_by(&mut self, vertical_delta: f64) -> Result<(), ParserDiagnostic> {
    let anchor = scroll_anchor_for_bounds(self.sidebar_bounds);
    let screen_point = self
      .session
      .window()
      .to_screen_point(
        &self.window,
        auv_driver::WindowPoint::new(anchor.x, anchor.y),
      )
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_scroll_point_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;
    let point = screen_point.point();
    self
      .session
      .input()
      .scroll_at(
        point,
        Scroll::new(0.0, vertical_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(LIVE_SCROLL_SETTLE_MS),
        },
      )
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_scroll_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;
    Ok(())
  }
}

#[cfg(target_os = "macos")]
fn scroll_anchor_for_bounds(bounds: ViewBounds) -> auv_driver::Point {
  auv_driver::Point::new(
    bounds.x + bounds.width * 0.5,
    bounds.y + bounds.height * 0.75,
  )
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
fn draw_rect(image: &mut RgbaImage, bounds: ViewBounds, color: Rgba<u8>, stroke: i64) {
  let x0 = bounds.x.round() as i64;
  let y0 = bounds.y.round() as i64;
  let x1 = (bounds.x + bounds.width).round() as i64;
  let y1 = (bounds.y + bounds.height).round() as i64;
  for offset in 0..stroke {
    draw_line(image, x0, y0 + offset, x1, y0 + offset, color);
    draw_line(image, x0, y1 - offset, x1, y1 - offset, color);
    draw_line(image, x0 + offset, y0, x0 + offset, y1, color);
    draw_line(image, x1 - offset, y0, x1 - offset, y1, color);
  }
}

#[cfg(target_os = "macos")]
fn draw_line(image: &mut RgbaImage, mut x0: i64, mut y0: i64, x1: i64, y1: i64, color: Rgba<u8>) {
  let dx = (x1 - x0).abs();
  let sx = if x0 < x1 { 1 } else { -1 };
  let dy = -(y1 - y0).abs();
  let sy = if y0 < y1 { 1 } else { -1 };
  let mut error = dx + dy;

  loop {
    put_pixel(image, x0, y0, color);
    if x0 == x1 && y0 == y1 {
      break;
    }
    let doubled = error * 2;
    if doubled >= dy {
      error += dy;
      x0 += sx;
    }
    if doubled <= dx {
      error += dx;
      y0 += sy;
    }
  }
}

#[cfg(target_os = "macos")]
fn put_pixel(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
  if x < 0 || y < 0 || x >= image.width() as i64 || y >= image.height() as i64 {
    return;
  }
  image.put_pixel(x as u32, y as u32, color);
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
    domain_kind: Some(domain_kind_for_section(kind)),
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

fn attach_item_node(section: &mut ViewNodeRecord, item: ViewNodeRecord) {
  section.children.push(item);
}

fn collect_anchors(node: &ViewNodeRecord, anchors: &mut Vec<ViewAnchor>) {
  anchors.extend(node.anchors.clone());
  for child in &node.children {
    collect_anchors(child, anchors);
  }
}

fn collect_landmarks(node: &ViewNodeRecord, landmarks: &mut Vec<ViewLandmark>) {
  landmarks.extend(node.landmarks.clone());
  for child in &node.children {
    collect_landmarks(child, landmarks);
  }
}

fn boundary_summary_from_observations(
  observations: &[SidebarViewportObservation],
) -> ScrollBoundarySummary {
  let mut summary = ScrollBoundarySummary::default();
  if observations
    .windows(2)
    .any(|pair| pair[0].viewport_fingerprint == pair[1].viewport_fingerprint)
  {
    summary.bottom = BoundaryConfidence::Likely;
  }
  summary
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

fn domain_kind_for_section(kind: SidebarSectionKind) -> String {
  match kind {
    SidebarSectionKind::FeatureNav => "netease.feature_nav",
    SidebarSectionKind::LibraryNav => "netease.library_nav",
    SidebarSectionKind::PlaylistNav => "netease.playlist_nav",
    SidebarSectionKind::MyPlaylists => "netease.my_playlists",
    SidebarSectionKind::FavoritedPlaylists => "netease.favorited_playlists",
    SidebarSectionKind::Unknown => "netease.sidebar_section",
  }
  .to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_inputs_uses_safe_defaults() {
    let inputs = parse_inputs(Vec::new()).expect("defaults should parse");

    assert_eq!(inputs.app_id, DEFAULT_APP_ID);
    assert_eq!(inputs.json_out, None);
    assert_eq!(inputs.artifact_dir, PathBuf::from(DEFAULT_ARTIFACT_DIR));
    assert_eq!(inputs.max_pages, 24);
    assert_eq!(inputs.max_scrolls, 48);
    assert_eq!(inputs.scroll_amount, 6.0);
    assert_eq!(inputs.sidebar_region, None);
    assert!(!inputs.print_json);
  }

  #[test]
  fn parse_inputs_accepts_json_and_scan_options() {
    let inputs = parse_inputs(vec![
      "--app-id".to_string(),
      "com.example.music".to_string(),
      "--json-out".to_string(),
      "/tmp/scan.json".to_string(),
      "--artifact-dir".to_string(),
      "/tmp/scan-artifacts".to_string(),
      "--max-pages".to_string(),
      "7".to_string(),
      "--max-scrolls".to_string(),
      "9".to_string(),
      "--scroll-amount".to_string(),
      "3.5".to_string(),
      "--sidebar-region".to_string(),
      "0.0,0.1,0.25,0.8".to_string(),
      "--print-json".to_string(),
    ])
    .expect("arguments should parse");

    assert_eq!(inputs.app_id, "com.example.music");
    assert_eq!(inputs.json_out, Some(PathBuf::from("/tmp/scan.json")));
    assert_eq!(inputs.artifact_dir, PathBuf::from("/tmp/scan-artifacts"));
    assert_eq!(inputs.max_pages, 7);
    assert_eq!(inputs.max_scrolls, 9);
    assert_eq!(inputs.scroll_amount, 3.5);
    assert_eq!(
      inputs.sidebar_region,
      Some(RatioRegion::new(0.0, 0.1, 0.25, 0.8))
    );
    assert!(inputs.print_json);
  }

  #[test]
  fn parse_inputs_rejects_invalid_values() {
    let cases = [
      (vec!["--bogus".to_string()], "unknown argument --bogus"),
      (
        vec!["--scroll-amount".to_string(), "NaN".to_string()],
        "--scroll-amount must be greater than 0",
      ),
      (
        vec![
          "--sidebar-region".to_string(),
          "0.0,NaN,0.25,0.8".to_string(),
        ],
        "--sidebar-region expects finite x,y,width,height",
      ),
    ];

    for (args, expected) in cases {
      let error = parse_inputs(args).expect_err("invalid inputs should fail");
      assert!(error.contains(expected));
    }
  }

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
      Some(RatioRegion::new(0.0, 0.1, 0.25, 0.8)),
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
  fn detect_sidebar_region_infers_visible_playlist_body_without_section_marker() {
    let region = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![
        ("Future Garage", 72.0, 320.0, 140.0, 20.0),
        ("Progressive House", 72.0, 366.0, 170.0, 20.0),
        ("Trance", 72.0, 412.0, 80.0, 20.0),
      ]),
    )
    .expect("visible playlist rows should infer the scroll body");

    assert_eq!(
      region.bounds,
      Some(ViewBounds::new(0.0, 300.0, 290.0, 418.0))
    );
  }

  #[test]
  fn detect_sidebar_region_ignores_main_content_when_inferring_playlist_body() {
    let region = detect_sidebar_region(
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
    .expect("visible playlist rows should infer the scroll body");

    assert_eq!(
      region.bounds,
      Some(ViewBounds::new(0.0, 300.0, 290.0, 418.0))
    );
  }

  #[test]
  fn detect_sidebar_region_fails_when_sidebar_markers_are_absent() {
    let error = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![("搜索", 400.0, 20.0, 60.0, 24.0)]),
    )
    .expect_err("missing left-side sidebar markers should fail");

    assert_eq!(error.code, "sidebar_region_not_found");
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

  #[cfg(target_os = "macos")]
  #[test]
  fn scroll_anchor_uses_lower_playlist_body_hit_area() {
    let anchor = scroll_anchor_for_bounds(ViewBounds::new(0.0, 146.0, 344.0, 908.0));

    assert_eq!(anchor.x, 172.0);
    assert_eq!(anchor.y, 827.0);
  }

  #[test]
  fn parse_viewport_keeps_unknown_short_noise_as_evidence_not_item() {
    let recognition = fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("·", 12.0, 74.0, 8.0, 8.0),
    ]);
    let observation =
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);

    assert_eq!(observation.evidence_nodes.len(), 2);
    assert_eq!(observation.candidates.len(), 1);
    assert_eq!(
      observation.candidates[0].kind,
      SidebarCandidateKind::SectionHeader
    );
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
      ("绚香猫的2025年度歌单", 72.0, 74.0, 180.0, 20.0),
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
  fn section_classification_uses_normalized_exact_labels() {
    assert_eq!(
      section_kind_from_label("创建的歌单 215"),
      SidebarSectionKind::MyPlaylists
    );
    assert_eq!(
      section_kind_from_label("收藏的歌单 12"),
      SidebarSectionKind::FavoritedPlaylists
    );
    assert_eq!(
      section_kind_from_label("我的收藏"),
      SidebarSectionKind::LibraryNav
    );
    assert_eq!(
      section_kind_from_label("食我的收藏"),
      SidebarSectionKind::LibraryNav
    );
    assert_eq!(
      section_kind_from_label("创建的歌单 215 入"),
      SidebarSectionKind::MyPlaylists
    );
    assert_eq!(
      section_kind_from_label("绚香猫的2025年度歌单"),
      SidebarSectionKind::Unknown
    );
    assert_eq!(
      section_kind_from_label("我的收藏夹"),
      SidebarSectionKind::Unknown
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
      Some(SidebarSectionKind::FavoritedPlaylists)
    );
    assert_eq!(scan.reconstruction.root.kind, ViewNodeKind::Collection);
    assert_eq!(scan.reconstruction.root.children.len(), 2);
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
    );

    assert_eq!(scan.window.id, Some("fake".to_string()));
    assert_eq!(scan.observations.len(), 3);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  }

  #[test]
  fn scan_loop_respects_page_budget() {
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
    );

    assert_eq!(scan.observations.len(), 1);
    assert!(
      scan
        .known_limits
        .iter()
        .any(|limit| limit.contains("max_pages"))
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
    );

    assert_eq!(scan.observations.len(), 1);
    assert_eq!(
      scan.observations[0].candidates[0].label.as_deref(),
      Some("创建的歌单")
    );
    assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
  }

  struct FakeSidebarObserver {
    observations: Vec<SidebarViewportObservation>,
    cursor: usize,
  }

  impl FakeSidebarObserver {
    fn new(observations: Vec<SidebarViewportObservation>) -> Self {
      Self {
        observations,
        cursor: 0,
      }
    }

    fn new_at(observations: Vec<SidebarViewportObservation>, cursor: usize) -> Self {
      Self {
        observations,
        cursor,
      }
    }
  }

  impl SidebarObserver for FakeSidebarObserver {
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
      Ok(())
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      self.cursor += 1;
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
