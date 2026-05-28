use std::path::PathBuf;

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
  title: Option<String>,
  bounds: Option<ViewBounds>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewRegionRecord {
  id: Option<String>,
  bounds: Option<ViewBounds>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct SidebarViewportObservation {
  viewport: ViewViewportRecord,
  evidence: Vec<ViewEvidenceNode>,
  candidates: Vec<SidebarViewportCandidate>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewViewportRecord {
  page_index: usize,
  bounds: ViewBounds,
  scroll_offset: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct ViewEvidenceNode {
  id: String,
  source: ViewEvidenceSource,
  label: Option<String>,
  bounds: Option<ViewBounds>,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
}
