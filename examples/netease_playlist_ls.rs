use std::path::PathBuf;

use auv_driver::vision::TextRecognition;
use serde::{Deserialize, Serialize};

const DEFAULT_APP_ID: &str = "com.netease.163music";

#[derive(Clone, Debug, PartialEq)]
struct Inputs {
  app_id: String,
  json_out: Option<PathBuf>,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct PlaylistSidebarScan {
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
  let _inputs = parse_inputs(std::env::args().skip(1).collect())?;
  Err("live implementation is added in later tasks".to_string())
}

fn parse_inputs(args: Vec<String>) -> Result<Inputs, String> {
  let mut inputs = Inputs {
    app_id: DEFAULT_APP_ID.to_string(),
    json_out: None,
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
  let mut marker_right_edges = recognition
    .regions
    .iter()
    .filter(|region| region.bounds.origin.x < left_limit)
    .filter(|region| is_sidebar_marker(region.text.trim()))
    .map(|region| region.bounds.origin.x + region.bounds.size.width)
    .collect::<Vec<_>>();

  if marker_right_edges.is_empty() {
    return Err(ParserDiagnostic {
      code: "sidebar_region_not_found".to_string(),
      message: "sidebar markers could not be identified on the left side".to_string(),
      node_id: None,
    });
  }

  marker_right_edges
    .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
  let max_x = marker_right_edges
    .last()
    .copied()
    .unwrap_or_default()
    .max(window_size.width * 0.18)
    .min(window_size.width * 0.42);

  Ok(sidebar_region_record(ViewBounds::new(
    0.0,
    0.0,
    max_x + 48.0,
    window_size.height,
  )))
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
  if label.contains("创建") || label.contains("我的歌单") {
    SidebarSectionKind::MyPlaylists
  } else if label.contains("收藏") {
    SidebarSectionKind::FavoritedPlaylists
  } else if label.contains("歌单") {
    SidebarSectionKind::PlaylistNav
  } else if matches!(label, "推荐" | "音乐服务") {
    SidebarSectionKind::FeatureNav
  } else {
    SidebarSectionKind::Unknown
  }
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
      id: Some("fake-sidebar-window".to_string()),
      title: None,
      bounds: None,
    },
    ViewRegionRecord::default(),
    observations,
  );
  scan.diagnostics.extend(diagnostics);
  scan.known_limits.extend(known_limits);
  scan
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
    SidebarSectionKind::PlaylistNav => "netease.playlist_nav",
    SidebarSectionKind::MyPlaylists => "netease.my_playlists",
    SidebarSectionKind::FavoritedPlaylists => "netease.favorited_playlists",
    SidebarSectionKind::Unknown => "netease.sidebar_section",
  }
  .to_string()
}

fn sample_reconstruction() -> ViewReconstructionRecord {
  let anchor = ViewAnchor {
    id: "anchor.coding".to_string(),
    label: "Coding BGM".to_string(),
    strength: AnchorStrength::Strong,
    bounds: ViewBounds::new(0.0, 50.0, 240.0, 32.0),
    evidence_ids: vec!["ocr.coding".to_string()],
  };
  let landmark = ViewLandmark {
    id: "landmark.my".to_string(),
    label: "创建的歌单".to_string(),
    landmark_use: LandmarkUse::SectionAssignment,
    bounds: ViewBounds::new(0.0, 20.0, 240.0, 32.0),
    evidence_ids: Vec::new(),
  };

  ViewReconstructionRecord {
    root: ViewNodeRecord {
      id: "root.sidebar".to_string(),
      kind: ViewNodeKind::Collection,
      domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: ViewBounds::new(0.0, 0.0, 240.0, 700.0),
      scrollable: Some(ViewScrollable {
        axis: ViewAxis::Vertical,
        boundary: ScrollBoundarySummary::default(),
      }),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::Scroll],
      evidence: Vec::new(),
      children: vec![ViewNodeRecord {
        id: "section.my".to_string(),
        kind: ViewNodeKind::Section,
        domain_kind: Some("netease.my_playlists".to_string()),
        layout: Some(ViewLayout::VStack),
        label: Some("创建的歌单".to_string()),
        bounds: ViewBounds::new(0.0, 20.0, 240.0, 32.0),
        scrollable: None,
        anchors: Vec::new(),
        landmarks: vec![landmark.clone()],
        actions: vec![ViewAction::ObserveOnly],
        evidence: Vec::new(),
        children: vec![ViewNodeRecord {
          id: "item.coding".to_string(),
          kind: ViewNodeKind::Item,
          domain_kind: Some("netease.playlist_item".to_string()),
          layout: Some(ViewLayout::HStack),
          label: Some("Coding BGM".to_string()),
          bounds: ViewBounds::new(0.0, 50.0, 240.0, 32.0),
          scrollable: None,
          anchors: vec![anchor.clone()],
          landmarks: Vec::new(),
          actions: vec![ViewAction::Open, ViewAction::Select],
          evidence: Vec::new(),
          children: vec![ViewNodeRecord {
            id: "item.coding.text".to_string(),
            kind: ViewNodeKind::Text,
            domain_kind: None,
            layout: None,
            label: Some("Coding BGM".to_string()),
            bounds: ViewBounds::new(30.0, 56.0, 120.0, 20.0),
            scrollable: None,
            anchors: Vec::new(),
            landmarks: Vec::new(),
            actions: vec![ViewAction::ObserveOnly],
            evidence: vec![ViewEvidenceNode {
              id: "ocr.coding".to_string(),
              source: ViewEvidenceSource::OcrText,
              label: Some("Coding BGM".to_string()),
              bounds: Some(ViewBounds::new(30.0, 56.0, 120.0, 20.0)),
              confidence: Confidence::High,
            }],
            children: Vec::new(),
          }],
        }],
      }],
    },
    anchor_index: vec![anchor],
    landmark_index: vec![landmark],
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_inputs_uses_safe_defaults() {
    let inputs = parse_inputs(Vec::new()).expect("defaults should parse");

    assert_eq!(inputs.app_id, DEFAULT_APP_ID);
    assert_eq!(inputs.json_out, None);
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
  fn parse_inputs_rejects_unknown_flag() {
    let error = parse_inputs(vec!["--bogus".to_string()]).expect_err("unknown flag should fail");
    assert!(error.contains("unknown argument --bogus"));
  }

  #[test]
  fn parse_inputs_rejects_non_finite_scroll_amount() {
    let error = parse_inputs(vec!["--scroll-amount".to_string(), "NaN".to_string()])
      .expect_err("non-finite scroll amount should fail");

    assert!(error.contains("--scroll-amount must be greater than 0"));
  }

  #[test]
  fn parse_inputs_rejects_non_finite_sidebar_region_component() {
    let error = parse_inputs(vec![
      "--sidebar-region".to_string(),
      "0.0,NaN,0.25,0.8".to_string(),
    ])
    .expect_err("non-finite sidebar region component should fail");

    assert!(error.contains("--sidebar-region expects finite x,y,width,height"));
  }

  #[test]
  fn view_reconstruction_serializes_tree_with_scrollable_collection() {
    let reconstruction = sample_reconstruction();
    let value = serde_json::to_value(&reconstruction).expect("reconstruction should serialize");

    assert_eq!(value["root"]["kind"], "collection");
    assert_eq!(value["root"]["layout"], "v_stack");
    assert_eq!(value["root"]["scrollable"]["axis"], "vertical");
    assert_eq!(value["root"]["children"][0]["kind"], "section");
    assert_eq!(value["root"]["children"][0]["children"][0]["kind"], "item");
    assert_eq!(
      value["root"]["children"][0]["children"][0]["children"][0]["kind"],
      "text"
    );
    assert_eq!(value["anchor_index"][0]["label"], "Coding BGM");
  }

  #[test]
  fn playlist_sidebar_scan_uses_reconstruction_plus_projection() {
    let reconstruction = sample_reconstruction();
    let scan = PlaylistSidebarScan {
      app: ScanAppContext::default(),
      window: ScanWindowContext::default(),
      sidebar_region: ViewRegionRecord::default(),
      observations: Vec::new(),
      reconstruction,
      projection: PlaylistSidebarProjection {
        sections: vec![SidebarSection {
          id: "section.my".to_string(),
          kind: SidebarSectionKind::MyPlaylists,
          label: Some("创建的歌单".to_string()),
          items: vec![PlaylistSidebarItem {
            id: "item.coding".to_string(),
            label: "Coding BGM".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: Some("candidate.coding".to_string()),
            anchor_id: Some("anchor.coding".to_string()),
          }],
        }],
      },
      boundary: ScrollBoundarySummary::default(),
      diagnostics: Vec::new(),
      known_limits: Vec::new(),
    };

    let json = serde_json::to_string_pretty(&scan).expect("scan should serialize");

    assert!(json.contains("\"reconstruction\""));
    assert!(json.contains("\"projection\""));
    assert!(json.contains("Coding BGM"));
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
