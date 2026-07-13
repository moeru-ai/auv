# View Parser IR NetEase Playlist Example Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an example-only `netease_playlist_ls` CLI that uses existing AUV macOS driver APIs to produce a structured NetEase playlist sidebar scan artifact.

**Architecture:** Keep NetEase-specific logic in `examples/netease_playlist_ls.rs`. The example owns parser heuristics, structured artifact types, sidebar region detection, reconstruction, and human rendering. AUV framework crates remain app-agnostic; any missing driver capability discovered during implementation must be recorded as a gap instead of moving NetEase logic into core.

**Tech Stack:** Rust 2024, `auv-driver`, `auv-driver-macos`, `serde`, `serde_json`, `cargo test --example`, macOS-only live execution.

---

## Scope And File Structure

This plan intentionally touches only the NetEase playlist example and reference docs.

**Create:**

- `examples/netease_playlist_ls.rs`
  - Example binary.
  - Contains CLI parsing, example-local `View*` structs, parser, reconstruction, pure tests, live driver adapter, structured JSON output, and human rendering.

**Modify:**

- `docs/ai/references/view-memory/2026-05-28-view-parser-ir-netease-playlist-example-design.md`
  - Only if implementation discovers a real framework gap or adjusts terminology.

**Do not modify in this plan:**

- `src/catalog.rs`
- `src/runtime.rs`
- `src/driver/**`
- `crates/auv-driver/**`
- `crates/auv-driver-macos/**`
- `src/inspect_server/**`

If the example cannot be implemented with the existing typed APIs, stop and record the exact missing API. Do not add NetEase-specific framework code.

## Validation Commands

Run these after relevant tasks:

```bash
cargo fmt --check
cargo test --example netease_playlist_ls
cargo check --example netease_playlist_ls
git diff --check
```

Live manual command, only after pure tests pass and NetEase Cloud Music is open:

```bash
cargo run --example netease_playlist_ls -- --json-out /tmp/auv-netease-playlist-ls.json
```

Expected live behavior: prints a human-readable playlist summary and writes structured JSON. If the sidebar is absent/collapsed or a modal blocks the window, it exits with a structured error message.

---

### Task 1: Example Skeleton And CLI Contract

**Files:**

- Create: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write the failing CLI parsing tests**

Create `examples/netease_playlist_ls.rs` with this initial content:

```rust
use std::path::PathBuf;

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
  let _ = args;
  Err("parse_inputs not implemented".to_string())
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
    let error = parse_inputs(vec!["--bogus".to_string()])
      .expect_err("unknown flag should fail");

    assert!(error.contains("unknown argument --bogus"));
  }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run:

```bash
cargo test --example netease_playlist_ls parse_inputs_ -- --nocapture
```

Expected: tests compile but fail because `parse_inputs` returns `parse_inputs not implemented`.

- [ ] **Step 3: Implement minimal CLI parsing**

Replace `parse_inputs` and add helpers below it:

```rust
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

  let mut index = 0;
  while index < args.len() {
    let flag = &args[index];
    match flag.as_str() {
      "--app-id" => {
        inputs.app_id = required_value(&args, &mut index, flag)?;
      }
      "--json-out" => {
        inputs.json_out = Some(PathBuf::from(required_value(&args, &mut index, flag)?));
      }
      "--max-pages" => {
        inputs.max_pages = parse_usize(&required_value(&args, &mut index, flag)?, flag)?;
      }
      "--max-scrolls" => {
        inputs.max_scrolls = parse_usize(&required_value(&args, &mut index, flag)?, flag)?;
      }
      "--scroll-amount" => {
        inputs.scroll_amount = parse_f64(&required_value(&args, &mut index, flag)?, flag)?;
      }
      "--sidebar-region" => {
        inputs.sidebar_region = Some(parse_ratio_region(&required_value(&args, &mut index, flag)?)?);
      }
      "--print-json" => {
        inputs.print_json = true;
      }
      other => return Err(format!("unknown argument {other}")),
    }
    index += 1;
  }

  if inputs.max_pages == 0 {
    return Err("--max-pages must be greater than zero".to_string());
  }
  if inputs.max_scrolls == 0 {
    return Err("--max-scrolls must be greater than zero".to_string());
  }
  if inputs.scroll_amount <= 0.0 {
    return Err("--scroll-amount must be greater than zero".to_string());
  }

  Ok(inputs)
}

fn required_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
  *index += 1;
  args
    .get(*index)
    .cloned()
    .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, String> {
  value
    .parse()
    .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_f64(value: &str, flag: &str) -> Result<f64, String> {
  value
    .parse()
    .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_ratio_region(value: &str) -> Result<RatioRegion, String> {
  let parts = value
    .split(',')
    .map(str::trim)
    .map(|part| {
      part
        .parse::<f64>()
        .map_err(|error| format!("invalid --sidebar-region component {part:?}: {error}"))
    })
    .collect::<Result<Vec<_>, _>>()?;
  let [x, y, width, height] = parts.as_slice() else {
    return Err("--sidebar-region must use x,y,width,height".to_string());
  };
  if *width <= 0.0 || *height <= 0.0 {
    return Err("--sidebar-region width and height must be positive".to_string());
  }
  Ok(RatioRegion::new(*x, *y, *width, *height))
}
```

- [ ] **Step 4: Run the tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls parse_inputs_ -- --nocapture
```

Expected: all `parse_inputs_` tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): add netease playlist ls skeleton"
```

---

### Task 2: Example-Local View IR And Structured Artifact Types

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing serialization and tree tests**

Add these imports at the top:

```rust
use serde::{Deserialize, Serialize};
```

Add this test module content below existing tests:

```rust
  #[test]
  fn view_reconstruction_serializes_tree_with_scrollable_collection() {
    let reconstruction = sample_reconstruction();
    let rendered = serde_json::to_value(&reconstruction).expect("reconstruction serializes");

    assert_eq!(rendered["root"]["kind"], "collection");
    assert_eq!(rendered["root"]["layout"], "v_stack");
    assert_eq!(rendered["root"]["scrollable"]["axis"], "vertical");
    assert_eq!(rendered["root"]["children"][0]["kind"], "section");
    assert_eq!(rendered["root"]["children"][0]["children"][0]["kind"], "item");
    assert_eq!(rendered["root"]["children"][0]["children"][0]["children"][0]["kind"], "text");
    assert_eq!(
      rendered["root"]["children"][0]["children"][0]["anchors"][0]["label"],
      "Coding BGM"
    );
  }

  #[test]
  fn playlist_sidebar_scan_uses_reconstruction_plus_projection() {
    let scan = PlaylistSidebarScan {
      app: ScanAppContext {
        bundle_id: DEFAULT_APP_ID.to_string(),
      },
      window: ScanWindowContext {
        title: Some("NetEase".to_string()),
        window_id: "42".to_string(),
      },
      sidebar_region: Some(ViewRegionRecord {
        name: "playlist_sidebar".to_string(),
        bounds: ViewBounds::new(0.0, 0.1, 0.25, 0.8),
        coordinate_space: "window_ratio".to_string(),
      }),
      observations: Vec::new(),
      reconstruction: sample_reconstruction(),
      projection: PlaylistSidebarProjection {
        sections: vec![SidebarSection {
          node_id: "section.my".to_string(),
          label: "创建的歌单".to_string(),
          kind: SidebarSectionKind::MyPlaylists,
          confidence: Confidence::High,
        }],
        items: vec![PlaylistSidebarItem {
          node_id: "item.coding".to_string(),
          item_id: "playlist.coding".to_string(),
          label: "Coding BGM".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          bounds: ViewBounds::new(12.0, 50.0, 150.0, 28.0),
          source_text: "Coding BGM".to_string(),
          observation_index: 0,
          confidence: Confidence::High,
          diagnostics: Vec::new(),
        }],
      },
      boundary: ScrollBoundarySummary::default(),
      diagnostics: Vec::new(),
      known_limits: Vec::new(),
    };

    let rendered = serde_json::to_string_pretty(&scan).expect("scan serializes");

    assert!(rendered.contains("\"reconstruction\""));
    assert!(rendered.contains("\"projection\""));
    assert!(rendered.contains("\"Coding BGM\""));
  }
```

- [ ] **Step 2: Run the tests and verify they fail**

Run:

```bash
cargo test --example netease_playlist_ls view_reconstruction_serializes_tree_with_scrollable_collection playlist_sidebar_scan_uses_reconstruction_plus_projection -- --nocapture
```

Expected: compile fails because the `View*`, scan, and helper types are not defined.

- [ ] **Step 3: Add the example-local structured types**

Add these types above `fn main()`:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PlaylistSidebarScan {
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: Option<ViewRegionRecord>,
  observations: Vec<SidebarViewportObservation>,
  reconstruction: ViewReconstructionRecord,
  projection: PlaylistSidebarProjection,
  boundary: ScrollBoundarySummary,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ScanAppContext {
  bundle_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ScanWindowContext {
  title: Option<String>,
  window_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewRegionRecord {
  name: String,
  bounds: ViewBounds,
  coordinate_space: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SidebarViewportObservation {
  observation_index: usize,
  viewport: ViewViewportRecord,
  source_artifacts: Vec<String>,
  evidence_nodes: Vec<ViewEvidenceNode>,
  candidates: Vec<SidebarViewportCandidate>,
  viewport_fingerprint: String,
  parser_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewViewportRecord {
  bounds: ViewBounds,
  axis: ViewAxis,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewEvidenceNode {
  evidence_id: String,
  source: ViewEvidenceSource,
  label: Option<String>,
  bounds: ViewBounds,
  confidence: Confidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewEvidenceSource {
  OcrText,
  AxNode,
  IconMatch,
  Visual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct SidebarViewportCandidate {
  candidate_id: String,
  kind: SidebarCandidateKind,
  label: String,
  bounds: ViewBounds,
  confidence: Confidence,
  evidence_ids: Vec<String>,
  diagnostics: Vec<ParserDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarCandidateKind {
  SectionHeader,
  PlaylistItem,
  NavigationItem,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewReconstructionRecord {
  root: ViewNodeRecord,
  anchor_index: Vec<ViewAnchor>,
  landmark_index: Vec<ViewLandmark>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewNodeRecord {
  node_id: String,
  kind: ViewNodeKind,
  domain_kind: Option<String>,
  layout: Option<ViewLayout>,
  label: Option<String>,
  bounds: Option<ViewBounds>,
  scrollable: Option<ViewScrollable>,
  children: Vec<ViewNodeRecord>,
  anchors: Vec<ViewAnchor>,
  landmarks: Vec<ViewLandmark>,
  actions: Vec<ViewAction>,
  source_evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewNodeKind {
  Container,
  Collection,
  Section,
  Item,
  Text,
  Icon,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewLayout {
  VStack,
  HStack,
  Group,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ViewAxis {
  Vertical,
  Horizontal,
  Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ViewScrollable {
  axis: ViewAxis,
  boundary: ScrollBoundarySummary,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ViewAnchor {
  anchor_id: String,
  label: String,
  strength: AnchorStrength,
  evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AnchorStrength {
  Strong,
  Medium,
  Weak,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ViewLandmark {
  landmark_id: String,
  label: String,
  use_for: Vec<LandmarkUse>,
  evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LandmarkUse {
  ViewportPose,
  BoundaryDetection,
  AnchorReacquire,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PlaylistSidebarProjection {
  sections: Vec<SidebarSection>,
  items: Vec<PlaylistSidebarItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SidebarSection {
  node_id: String,
  label: String,
  kind: SidebarSectionKind,
  confidence: Confidence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarSectionKind {
  FeatureNav,
  PlaylistNav,
  MyPlaylists,
  FavoritedPlaylists,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PlaylistSidebarItem {
  node_id: String,
  item_id: String,
  label: String,
  section_hint: Option<SidebarSectionKind>,
  bounds: ViewBounds,
  source_text: String,
  observation_index: usize,
  confidence: Confidence,
  diagnostics: Vec<ParserDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Confidence {
  High,
  Medium,
  Low,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ParserDiagnostic {
  code: String,
  message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
```

Add this helper near the tests, outside the `tests` module if needed by implementation:

```rust
fn sample_reconstruction() -> ViewReconstructionRecord {
  let anchor = ViewAnchor {
    anchor_id: "anchor.coding".to_string(),
    label: "Coding BGM".to_string(),
    strength: AnchorStrength::Strong,
    evidence_ids: vec!["ocr.coding".to_string()],
  };
  let landmark = ViewLandmark {
    landmark_id: "landmark.my".to_string(),
    label: "创建的歌单".to_string(),
    use_for: vec![LandmarkUse::ViewportPose, LandmarkUse::SectionAssignment],
    evidence_ids: vec!["ocr.my".to_string()],
  };
  ViewReconstructionRecord {
    root: ViewNodeRecord {
      node_id: "root.sidebar".to_string(),
      kind: ViewNodeKind::Collection,
      domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
      layout: Some(ViewLayout::VStack),
      label: None,
      bounds: Some(ViewBounds::new(0.0, 0.0, 240.0, 700.0)),
      scrollable: Some(ViewScrollable {
        axis: ViewAxis::Vertical,
        boundary: ScrollBoundarySummary::default(),
      }),
      children: vec![ViewNodeRecord {
        node_id: "section.my".to_string(),
        kind: ViewNodeKind::Section,
        domain_kind: Some("netease.my_playlists".to_string()),
        layout: Some(ViewLayout::VStack),
        label: Some("创建的歌单".to_string()),
        bounds: Some(ViewBounds::new(0.0, 20.0, 240.0, 32.0)),
        scrollable: None,
        children: vec![ViewNodeRecord {
          node_id: "item.coding".to_string(),
          kind: ViewNodeKind::Item,
          domain_kind: Some("netease.playlist_item".to_string()),
          layout: Some(ViewLayout::HStack),
          label: Some("Coding BGM".to_string()),
          bounds: Some(ViewBounds::new(0.0, 50.0, 240.0, 32.0)),
          scrollable: None,
          children: vec![ViewNodeRecord {
            node_id: "item.coding.text".to_string(),
            kind: ViewNodeKind::Text,
            domain_kind: None,
            layout: None,
            label: Some("Coding BGM".to_string()),
            bounds: Some(ViewBounds::new(30.0, 56.0, 120.0, 20.0)),
            scrollable: None,
            children: Vec::new(),
            anchors: Vec::new(),
            landmarks: Vec::new(),
            actions: vec![ViewAction::ObserveOnly],
            source_evidence_ids: vec!["ocr.coding".to_string()],
          }],
          anchors: vec![anchor.clone()],
          landmarks: Vec::new(),
          actions: vec![ViewAction::Open, ViewAction::Select],
          source_evidence_ids: vec!["ocr.coding".to_string()],
        }],
        anchors: Vec::new(),
        landmarks: vec![landmark.clone()],
        actions: vec![ViewAction::ObserveOnly],
        source_evidence_ids: vec!["ocr.my".to_string()],
      }],
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::Scroll],
      source_evidence_ids: Vec::new(),
    },
    anchor_index: vec![anchor],
    landmark_index: vec![landmark],
  }
}
```

- [ ] **Step 4: Run the tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls view_reconstruction_serializes_tree_with_scrollable_collection playlist_sidebar_scan_uses_reconstruction_plus_projection -- --nocapture
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): define playlist scan artifact"
```

---

### Task 3: One-Viewport Parser From OCR Evidence

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing parser tests**

Add these tests:

```rust
  #[test]
  fn parse_viewport_classifies_sections_and_playlist_items() {
    let recognition = fake_recognition(vec![
      ("推荐", 8.0, 8.0, 80.0, 20.0),
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 78.0, 140.0, 20.0),
      ("Jazz", 32.0, 112.0, 90.0, 20.0),
    ]);
    let observation = parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);

    assert_eq!(observation.candidates.len(), 4);
    assert_eq!(observation.candidates[1].kind, SidebarCandidateKind::SectionHeader);
    assert_eq!(observation.candidates[1].label, "创建的歌单");
    assert_eq!(observation.candidates[2].kind, SidebarCandidateKind::PlaylistItem);
    assert_eq!(observation.candidates[2].label, "Coding BGM");
    assert_eq!(observation.evidence_nodes[2].source, ViewEvidenceSource::OcrText);
  }

  #[test]
  fn parse_viewport_keeps_unknown_short_noise_as_evidence_not_item() {
    let recognition = fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("·", 12.0, 74.0, 8.0, 8.0),
    ]);
    let observation = parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);

    assert_eq!(observation.evidence_nodes.len(), 2);
    assert_eq!(observation.candidates.len(), 1);
    assert_eq!(observation.candidates[0].kind, SidebarCandidateKind::SectionHeader);
  }
```

Add this helper inside the test module:

```rust
  fn fake_recognition(rows: Vec<(&str, f64, f64, f64, f64)>) -> auv_driver::vision::TextRecognition {
    auv_driver::vision::TextRecognition {
      text: rows
        .iter()
        .map(|(text, _, _, _, _)| *text)
        .collect::<Vec<_>>()
        .join("\n"),
      regions: rows
        .into_iter()
        .map(|(text, x, y, width, height)| auv_driver::vision::RecognizedText {
          text: text.to_string(),
          bounds: auv_driver::Rect::new(x, y, width, height),
          confidence: Some(0.92),
        })
        .collect(),
    }
  }
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --example netease_playlist_ls parse_viewport_ -- --nocapture
```

Expected: compile fails because `parse_sidebar_viewport` is not defined.

- [ ] **Step 3: Add one-viewport OCR parser**

Add imports at the top:

```rust
use auv_driver::vision::TextRecognition;
```

Add this implementation below the type definitions:

```rust
fn parse_sidebar_viewport(
  observation_index: usize,
  viewport_bounds: ViewBounds,
  recognition: &TextRecognition,
) -> SidebarViewportObservation {
  let mut evidence_nodes = recognition
    .regions
    .iter()
    .enumerate()
    .map(|(index, text)| ViewEvidenceNode {
      evidence_id: format!("obs{observation_index}.ocr{index}"),
      source: ViewEvidenceSource::OcrText,
      label: Some(text.text.trim().to_string()),
      bounds: ViewBounds::new(
        text.bounds.origin.x,
        text.bounds.origin.y,
        text.bounds.size.width,
        text.bounds.size.height,
      ),
      confidence: confidence_from_ocr(text.confidence),
    })
    .collect::<Vec<_>>();
  evidence_nodes.sort_by(|left, right| {
    left
      .bounds
      .y
      .partial_cmp(&right.bounds.y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| {
        left
          .bounds
          .x
          .partial_cmp(&right.bounds.x)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
  });

  let candidates = evidence_nodes
    .iter()
    .filter_map(|node| candidate_from_evidence(observation_index, node))
    .collect::<Vec<_>>();

  SidebarViewportObservation {
    observation_index,
    viewport: ViewViewportRecord {
      bounds: viewport_bounds,
      axis: ViewAxis::Vertical,
    },
    source_artifacts: Vec::new(),
    viewport_fingerprint: viewport_fingerprint(&evidence_nodes),
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
  let kind = classify_sidebar_text(label, node.bounds.x);
  if kind == SidebarCandidateKind::Unknown {
    return None;
  }
  Some(SidebarViewportCandidate {
    candidate_id: format!("obs{observation_index}.candidate.{}", slug(label)),
    kind,
    label: label.to_string(),
    bounds: node.bounds,
    confidence: node.confidence,
    evidence_ids: vec![node.evidence_id.clone()],
    diagnostics: Vec::new(),
  })
}

fn classify_sidebar_text(label: &str, x: f64) -> SidebarCandidateKind {
  if section_kind_from_label(label) != SidebarSectionKind::Unknown {
    return SidebarCandidateKind::SectionHeader;
  }
  if x >= 24.0 {
    return SidebarCandidateKind::PlaylistItem;
  }
  if matches!(label, "推荐" | "发现音乐" | "播客" | "私人漫游" | "最近播放") {
    return SidebarCandidateKind::NavigationItem;
  }
  SidebarCandidateKind::Unknown
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
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls parse_viewport_ -- --nocapture
```

Expected: both parser tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): parse netease sidebar viewport"
```

---

### Task 4: Reconstruction From Viewport Candidates

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing reconstruction tests**

Add tests:

```rust
  #[test]
  fn reconstruct_sidebar_groups_items_under_carried_section() {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 78.0, 140.0, 20.0),
      ]),
    );
    let page1 = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("Jazz", 32.0, 44.0, 90.0, 20.0),
        ("收藏的歌单", 8.0, 82.0, 110.0, 20.0),
        ("Road Trip", 32.0, 118.0, 130.0, 20.0),
      ]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext {
        bundle_id: DEFAULT_APP_ID.to_string(),
      },
      ScanWindowContext {
        title: Some("NetEase".to_string()),
        window_id: "42".to_string(),
      },
      Some(ViewRegionRecord {
        name: "playlist_sidebar".to_string(),
        bounds: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        coordinate_space: "window".to_string(),
      }),
      vec![page0, page1],
    );

    assert_eq!(scan.projection.sections.len(), 2);
    assert_eq!(scan.projection.items.len(), 3);
    assert_eq!(scan.projection.items[0].label, "Coding BGM");
    assert_eq!(scan.projection.items[1].section_hint, Some(SidebarSectionKind::MyPlaylists));
    assert_eq!(
      scan.projection.items[2].section_hint,
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
        ("Coding BGM", 32.0, 78.0, 140.0, 20.0),
      ]),
    );
    let page1 = parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Coding BGM", 32.0, 44.0, 140.0, 20.0)]),
    );

    let scan = reconstruct_playlist_sidebar(
      ScanAppContext {
        bundle_id: DEFAULT_APP_ID.to_string(),
      },
      ScanWindowContext {
        title: None,
        window_id: "42".to_string(),
      },
      None,
      vec![page0, page1],
    );

    assert_eq!(scan.projection.items.len(), 1);
    assert!(scan.diagnostics.iter().any(|diagnostic| diagnostic.code == "deduplicated_item"));
  }
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --example netease_playlist_ls reconstruct_sidebar_ -- --nocapture
```

Expected: compile fails because `reconstruct_playlist_sidebar` is not defined.

- [ ] **Step 3: Implement reconstruction and projection**

Add:

```rust
fn reconstruct_playlist_sidebar(
  app: ScanAppContext,
  window: ScanWindowContext,
  sidebar_region: Option<ViewRegionRecord>,
  observations: Vec<SidebarViewportObservation>,
) -> PlaylistSidebarScan {
  let mut sections = Vec::<SidebarSection>::new();
  let mut items = Vec::<PlaylistSidebarItem>::new();
  let mut diagnostics = Vec::<ParserDiagnostic>::new();
  let mut section_nodes = Vec::<ViewNodeRecord>::new();
  let mut current_section: Option<(SidebarSectionKind, String, String)> = None;
  let mut seen_item_keys = std::collections::BTreeSet::<String>::new();

  for observation in &observations {
    for candidate in &observation.candidates {
      match candidate.kind {
        SidebarCandidateKind::SectionHeader => {
          let section_kind = section_kind_from_label(&candidate.label);
          let node_id = format!("section.{}", slug(&candidate.label));
          if !sections.iter().any(|section| section.node_id == node_id) {
            sections.push(SidebarSection {
              node_id: node_id.clone(),
              label: candidate.label.clone(),
              kind: section_kind,
              confidence: candidate.confidence,
            });
            section_nodes.push(section_node(candidate, section_kind, observation.observation_index));
          }
          current_section = Some((section_kind, candidate.label.clone(), node_id));
        }
        SidebarCandidateKind::PlaylistItem | SidebarCandidateKind::NavigationItem => {
          let section_hint = current_section.as_ref().map(|(kind, _, _)| *kind);
          let section_node_id = current_section
            .as_ref()
            .map(|(_, _, node_id)| node_id.clone())
            .unwrap_or_else(|| "section.unassigned".to_string());
          let item_key = format!(
            "{}:{}",
            section_hint
              .map(|kind| format!("{kind:?}"))
              .unwrap_or_else(|| "unknown".to_string()),
            normalize_identity(&candidate.label)
          );
          if !seen_item_keys.insert(item_key) {
            diagnostics.push(ParserDiagnostic {
              code: "deduplicated_item".to_string(),
              message: format!("deduplicated repeated sidebar item {:?}", candidate.label),
            });
            continue;
          }
          let node_id = format!("item.{}", slug(&candidate.label));
          items.push(PlaylistSidebarItem {
            node_id: node_id.clone(),
            item_id: format!("playlist.{}", slug(&candidate.label)),
            label: candidate.label.clone(),
            section_hint,
            bounds: candidate.bounds,
            source_text: candidate.label.clone(),
            observation_index: observation.observation_index,
            confidence: candidate.confidence,
            diagnostics: Vec::new(),
          });
          attach_item_node(&mut section_nodes, &section_node_id, item_node(&node_id, candidate));
        }
        SidebarCandidateKind::Unknown => {}
      }
    }
  }

  let anchor_index = section_nodes
    .iter()
    .flat_map(collect_anchors)
    .collect::<Vec<_>>();
  let landmark_index = section_nodes
    .iter()
    .flat_map(collect_landmarks)
    .collect::<Vec<_>>();
  let boundary = boundary_summary_from_observations(&observations);
  let root = ViewNodeRecord {
    node_id: "root.sidebar".to_string(),
    kind: ViewNodeKind::Collection,
    domain_kind: Some("netease.sidebar_playlist_collection".to_string()),
    layout: Some(ViewLayout::VStack),
    label: None,
    bounds: sidebar_region.as_ref().map(|region| region.bounds),
    scrollable: Some(ViewScrollable {
      axis: ViewAxis::Vertical,
      boundary: boundary.clone(),
    }),
    children: section_nodes,
    anchors: Vec::new(),
    landmarks: Vec::new(),
    actions: vec![ViewAction::Scroll],
    source_evidence_ids: Vec::new(),
  };

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
    projection: PlaylistSidebarProjection { sections, items },
    boundary,
    diagnostics,
    known_limits: Vec::new(),
  }
}

fn section_node(
  candidate: &SidebarViewportCandidate,
  section_kind: SidebarSectionKind,
  observation_index: usize,
) -> ViewNodeRecord {
  let label = candidate.label.clone();
  ViewNodeRecord {
    node_id: format!("section.{}", slug(&label)),
    kind: ViewNodeKind::Section,
    domain_kind: Some(format!("netease.{section_kind:?}")),
    layout: Some(ViewLayout::VStack),
    label: Some(label.clone()),
    bounds: Some(candidate.bounds),
    scrollable: None,
    children: Vec::new(),
    anchors: vec![ViewAnchor {
      anchor_id: format!("anchor.section.{}", slug(&label)),
      label: label.clone(),
      strength: AnchorStrength::Strong,
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    landmarks: vec![ViewLandmark {
      landmark_id: format!("landmark.section.{}", slug(&label)),
      label,
      use_for: vec![LandmarkUse::ViewportPose, LandmarkUse::SectionAssignment],
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    actions: vec![ViewAction::ObserveOnly],
    source_evidence_ids: candidate.evidence_ids.clone(),
  }
}

fn item_node(node_id: &str, candidate: &SidebarViewportCandidate) -> ViewNodeRecord {
  ViewNodeRecord {
    node_id: node_id.to_string(),
    kind: ViewNodeKind::Item,
    domain_kind: Some("netease.playlist_item".to_string()),
    layout: Some(ViewLayout::HStack),
    label: Some(candidate.label.clone()),
    bounds: Some(candidate.bounds),
    scrollable: None,
    children: vec![ViewNodeRecord {
      node_id: format!("{node_id}.text"),
      kind: ViewNodeKind::Text,
      domain_kind: None,
      layout: None,
      label: Some(candidate.label.clone()),
      bounds: Some(candidate.bounds),
      scrollable: None,
      children: Vec::new(),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      actions: vec![ViewAction::ObserveOnly],
      source_evidence_ids: candidate.evidence_ids.clone(),
    }],
    anchors: vec![ViewAnchor {
      anchor_id: format!("anchor.{node_id}"),
      label: candidate.label.clone(),
      strength: AnchorStrength::Strong,
      evidence_ids: candidate.evidence_ids.clone(),
    }],
    landmarks: Vec::new(),
    actions: vec![ViewAction::Open, ViewAction::Select],
    source_evidence_ids: candidate.evidence_ids.clone(),
  }
}

fn attach_item_node(sections: &mut [ViewNodeRecord], section_node_id: &str, item: ViewNodeRecord) {
  if let Some(section) = sections
    .iter_mut()
    .find(|section| section.node_id == section_node_id)
  {
    section.children.push(item);
  }
}

fn collect_anchors(node: &ViewNodeRecord) -> Vec<ViewAnchor> {
  node
    .anchors
    .iter()
    .cloned()
    .chain(node.children.iter().flat_map(collect_anchors))
    .collect()
}

fn collect_landmarks(node: &ViewNodeRecord) -> Vec<ViewLandmark> {
  node
    .landmarks
    .iter()
    .cloned()
    .chain(node.children.iter().flat_map(collect_landmarks))
    .collect()
}

fn boundary_summary_from_observations(observations: &[SidebarViewportObservation]) -> ScrollBoundarySummary {
  let bottom = if observations
    .windows(2)
    .any(|pair| pair[0].viewport_fingerprint == pair[1].viewport_fingerprint)
  {
    BoundaryConfidence::Likely
  } else {
    BoundaryConfidence::Unknown
  };
  ScrollBoundarySummary {
    bottom,
    ..ScrollBoundarySummary::default()
  }
}
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls reconstruct_sidebar_ -- --nocapture
```

Expected: reconstruction tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): reconstruct playlist sidebar view"
```

---

### Task 5: Sidebar Region Detection And Structured Blocker Errors

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing pure tests for region/blocker detection**

Add tests:

```rust
  #[test]
  fn detect_sidebar_region_uses_manual_region_when_provided() {
    let manual = RatioRegion::new(0.0, 0.1, 0.25, 0.8);
    let region = detect_sidebar_region(
      Some(manual),
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(Vec::new()),
    )
    .expect("manual region should be accepted");

    assert_eq!(region.bounds, ViewBounds::new(0.0, 80.0, 250.0, 640.0));
  }

  #[test]
  fn detect_sidebar_region_fails_when_sidebar_markers_are_absent() {
    let error = detect_sidebar_region(
      None,
      auv_driver::Size::new(1000.0, 800.0),
      &fake_recognition(vec![("搜索", 400.0, 40.0, 80.0, 20.0)]),
    )
    .expect_err("missing sidebar should fail");

    assert_eq!(error.code, "sidebar_region_not_found");
  }

  #[test]
  fn detect_blocking_modal_reports_cancel_or_open_dialog_markers() {
    let diagnostic = detect_blocking_modal(&fake_recognition(vec![
      ("打开", 700.0, 680.0, 60.0, 24.0),
      ("取消", 780.0, 680.0, 60.0, 24.0),
    ]))
    .expect("modal should be detected");

    assert_eq!(diagnostic.code, "blocking_modal_dialog");
  }
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --example netease_playlist_ls detect_sidebar_region_ detect_blocking_modal_ -- --nocapture
```

Expected: compile fails because detection functions are not defined.

- [ ] **Step 3: Add region and modal detection helpers**

Add:

```rust
fn detect_sidebar_region(
  manual: Option<RatioRegion>,
  window_size: auv_driver::Size,
  recognition: &TextRecognition,
) -> Result<ViewRegionRecord, ParserDiagnostic> {
  if let Some(region) = manual {
    return Ok(ViewRegionRecord {
      name: "playlist_sidebar".to_string(),
      bounds: ratio_to_window_bounds(region, window_size),
      coordinate_space: "window".to_string(),
    });
  }

  let sidebar_markers = recognition
    .regions
    .iter()
    .filter(|text| {
      let label = text.text.trim();
      text.bounds.origin.x < window_size.width * 0.38
        && (section_kind_from_label(label) != SidebarSectionKind::Unknown
          || matches!(label, "推荐" | "发现音乐" | "最近播放"))
    })
    .collect::<Vec<_>>();

  if sidebar_markers.is_empty() {
    return Err(ParserDiagnostic {
      code: "sidebar_region_not_found".to_string(),
      message: "could not identify NetEase sidebar markers in the left side of the window".to_string(),
    });
  }

  let max_x = sidebar_markers
    .iter()
    .map(|text| text.bounds.origin.x + text.bounds.size.width)
    .fold(0.0, f64::max)
    .max(window_size.width * 0.18)
    .min(window_size.width * 0.42);

  Ok(ViewRegionRecord {
    name: "playlist_sidebar".to_string(),
    bounds: ViewBounds::new(0.0, 0.0, max_x + 48.0, window_size.height),
    coordinate_space: "window".to_string(),
  })
}

fn ratio_to_window_bounds(region: RatioRegion, window_size: auv_driver::Size) -> ViewBounds {
  ViewBounds::new(
    region.x * window_size.width,
    region.y * window_size.height,
    region.width * window_size.width,
    region.height * window_size.height,
  )
}

fn detect_blocking_modal(recognition: &TextRecognition) -> Option<ParserDiagnostic> {
  let has_cancel = recognition.best_contains("取消").is_some();
  let has_open = recognition.best_contains("打开").is_some() || recognition.best_contains("存储").is_some();
  if has_cancel && has_open {
    Some(ParserDiagnostic {
      code: "blocking_modal_dialog".to_string(),
      message: "detected modal/system dialog markers; sidebar scan is blocked".to_string(),
    })
  } else {
    None
  }
}
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls detect_sidebar_region_ detect_blocking_modal_ -- --nocapture
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): detect netease sidebar region"
```

---

### Task 6: Bounded Scroll Scan Loop With Fake Observer Tests

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing scroll loop tests**

Add tests:

```rust
  #[test]
  fn scan_loop_stops_on_repeated_viewport_fingerprint() {
    let observations = vec![
      parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0), ("Coding", 32.0, 78.0, 100.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        1,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Jazz", 32.0, 44.0, 90.0, 20.0)]),
      ),
      parse_sidebar_viewport(
        2,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![("Jazz", 32.0, 44.0, 90.0, 20.0)]),
      ),
    ];
    let mut observer = FakeSidebarObserver::new(observations);
    let scan = scan_sidebar_with_observer(&mut observer, ScanOptions { max_pages: 10, max_scrolls: 10 });

    assert_eq!(scan.observations.len(), 3);
    assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  }

  #[test]
  fn scan_loop_respects_page_budget() {
    let observations = vec![
      parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 44.0, 90.0, 20.0)])),
      parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("B", 32.0, 44.0, 90.0, 20.0)])),
    ];
    let mut observer = FakeSidebarObserver::new(observations);
    let scan = scan_sidebar_with_observer(&mut observer, ScanOptions { max_pages: 1, max_scrolls: 10 });

    assert_eq!(scan.observations.len(), 1);
    assert!(scan.known_limits.iter().any(|limit| limit.contains("max_pages")));
  }
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --example netease_playlist_ls scan_loop_ -- --nocapture
```

Expected: compile fails because scan loop types are not defined.

- [ ] **Step 3: Add fake-observer-driven scan loop**

Add:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanOptions {
  max_pages: usize,
  max_scrolls: usize,
}

trait SidebarObserver {
  fn observe(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic>;
  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic>;
}

fn scan_sidebar_with_observer(
  observer: &mut impl SidebarObserver,
  options: ScanOptions,
) -> PlaylistSidebarScan {
  let mut observations = Vec::new();
  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();
  let mut previous_fingerprint: Option<String> = None;
  let mut scrolls = 0usize;

  for page_index in 0..options.max_pages {
    let observation = match observer.observe(page_index) {
      Ok(observation) => observation,
      Err(error) => {
        diagnostics.push(error);
        break;
      }
    };
    let repeated = previous_fingerprint
      .as_ref()
      .is_some_and(|fingerprint| *fingerprint == observation.viewport_fingerprint);
    previous_fingerprint = Some(observation.viewport_fingerprint.clone());
    observations.push(observation);
    if repeated {
      break;
    }
    if page_index + 1 >= options.max_pages {
      known_limits.push(format!("stopped after max_pages={}", options.max_pages));
      break;
    }
    if scrolls >= options.max_scrolls {
      known_limits.push(format!("stopped after max_scrolls={}", options.max_scrolls));
      break;
    }
    match observer.scroll_down() {
      Ok(()) => scrolls += 1,
      Err(error) => {
        diagnostics.push(error);
        break;
      }
    }
  }

  let mut scan = reconstruct_playlist_sidebar(
    ScanAppContext {
      bundle_id: DEFAULT_APP_ID.to_string(),
    },
    ScanWindowContext {
      title: None,
      window_id: "fake".to_string(),
    },
    None,
    observations,
  );
  scan.diagnostics.extend(diagnostics);
  scan.known_limits.extend(known_limits);
  scan
}
```

Add this fake observer inside the test module:

```rust
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
    fn observe(&mut self, _observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
      self
        .observations
        .get(self.cursor)
        .cloned()
        .ok_or_else(|| ParserDiagnostic {
          code: "no_more_fake_observations".to_string(),
          message: "fake observer has no observation for cursor".to_string(),
        })
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      self.cursor += 1;
      Ok(())
    }
  }
```

- [ ] **Step 4: Run tests and verify they pass**

Run:

```bash
cargo test --example netease_playlist_ls scan_loop_ -- --nocapture
```

Expected: scroll loop tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): add playlist sidebar scan loop"
```

---

### Task 7: Live Driver Adapter And JSON/Human Output

**Files:**

- Modify: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Write failing renderer tests**

Add tests:

```rust
  #[test]
  fn render_human_summary_groups_items_by_section() {
    let scan = reconstruct_playlist_sidebar(
      ScanAppContext {
        bundle_id: DEFAULT_APP_ID.to_string(),
      },
      ScanWindowContext {
        title: Some("NetEase".to_string()),
        window_id: "42".to_string(),
      },
      None,
      vec![parse_sidebar_viewport(
        0,
        ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        &fake_recognition(vec![
          ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
          ("Coding BGM", 32.0, 78.0, 140.0, 20.0),
        ]),
      )],
    );

    let rendered = render_human_summary(&scan);

    assert!(rendered.contains("创建的歌单"));
    assert!(rendered.contains("Coding BGM"));
    assert!(rendered.contains("boundary"));
  }
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cargo test --example netease_playlist_ls render_human_summary_groups_items_by_section -- --nocapture
```

Expected: compile fails because `render_human_summary` is not defined.

- [ ] **Step 3: Add renderer, live observer, and wire `run`**

Add imports at the top:

```rust
#[cfg(target_os = "macos")]
use auv_driver::capture::Capture;
#[cfg(target_os = "macos")]
use auv_driver::selector::{App, Window};
#[cfg(target_os = "macos")]
use auv_driver::{Driver, RatioRect};
#[cfg(target_os = "macos")]
use auv_driver_macos::MacosDriver;
```

Replace `run` with:

```rust
fn run() -> Result<(), String> {
  let inputs = parse_inputs(std::env::args().skip(1).collect())?;
  let scan = run_live_scan(&inputs)?;
  let rendered = serde_json::to_string_pretty(&scan)
    .map_err(|error| format!("failed to serialize scan JSON: {error}"))?;
  if let Some(path) = &inputs.json_out {
    std::fs::write(path, rendered.as_bytes())
      .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
  }
  if inputs.print_json {
    println!("{rendered}");
  } else {
    println!("{}", render_human_summary(&scan));
  }
  Ok(())
}
```

Add:

```rust
fn render_human_summary(scan: &PlaylistSidebarScan) -> String {
  let mut lines = vec![
    format!("app: {}", scan.app.bundle_id),
    format!("window: {}", scan.window.title.as_deref().unwrap_or("<untitled>")),
    format!(
      "boundary: top={:?} bottom={:?}",
      scan.boundary.top, scan.boundary.bottom
    ),
  ];
  for section in &scan.projection.sections {
    lines.push(format!("\n[{}]", section.label));
    for item in scan
      .projection
      .items
      .iter()
      .filter(|item| item.section_hint == Some(section.kind))
    {
      lines.push(format!("- {}", item.label));
    }
  }
  if !scan.diagnostics.is_empty() {
    lines.push("\ndiagnostics:".to_string());
    for diagnostic in &scan.diagnostics {
      lines.push(format!("- {}: {}", diagnostic.code, diagnostic.message));
    }
  }
  if !scan.known_limits.is_empty() {
    lines.push("\nknown limits:".to_string());
    for limit in &scan.known_limits {
      lines.push(format!("- {limit}"));
    }
  }
  lines.join("\n")
}

#[cfg(target_os = "macos")]
fn run_live_scan(inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  let driver = MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::bundle(inputs.app_id.clone())))
    .map_err(|error| error.to_string())?;
  let initial_capture = session.window().capture(&window).map_err(|error| error.to_string())?;
  let full_region = RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let full_ocr = session
    .vision()
    .recognize_text_in_capture(&initial_capture, full_region)
    .map_err(|error| error.to_string())?;
  if let Some(blocker) = detect_blocking_modal(&full_ocr) {
    return Ok(PlaylistSidebarScan {
      app: ScanAppContext {
        bundle_id: inputs.app_id.clone(),
      },
      window: ScanWindowContext {
        title: window.title.clone(),
        window_id: window.reference.id.clone(),
      },
      sidebar_region: None,
      observations: Vec::new(),
      reconstruction: ViewReconstructionRecord {
        root: empty_root(),
        anchor_index: Vec::new(),
        landmark_index: Vec::new(),
      },
      projection: PlaylistSidebarProjection {
        sections: Vec::new(),
        items: Vec::new(),
      },
      boundary: ScrollBoundarySummary::default(),
      diagnostics: vec![blocker],
      known_limits: Vec::new(),
    });
  }
  let sidebar_region = detect_sidebar_region(inputs.sidebar_region, initial_capture.bounds.size, &full_ocr)
    .map_err(|diagnostic| diagnostic.message)?;
  let mut observer = LiveSidebarObserver {
    session: &session,
    window: &window,
    sidebar_region: sidebar_region.clone(),
    scroll_amount: inputs.scroll_amount,
  };
  let mut scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: inputs.max_pages,
      max_scrolls: inputs.max_scrolls,
    },
  );
  scan.app.bundle_id = inputs.app_id.clone();
  scan.window = ScanWindowContext {
    title: window.title.clone(),
    window_id: window.reference.id.clone(),
  };
  scan.sidebar_region = Some(sidebar_region);
  Ok(scan)
}

#[cfg(not(target_os = "macos"))]
fn run_live_scan(_inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  Err("netease_playlist_ls live scan is only available on macOS".to_string())
}

fn empty_root() -> ViewNodeRecord {
  ViewNodeRecord {
    node_id: "root.empty".to_string(),
    kind: ViewNodeKind::Collection,
    domain_kind: None,
    layout: Some(ViewLayout::VStack),
    label: None,
    bounds: None,
    scrollable: Some(ViewScrollable {
      axis: ViewAxis::Vertical,
      boundary: ScrollBoundarySummary::default(),
    }),
    children: Vec::new(),
    anchors: Vec::new(),
    landmarks: Vec::new(),
    actions: vec![ViewAction::ObserveOnly],
    source_evidence_ids: Vec::new(),
  }
}
```

Add the live observer:

```rust
#[cfg(target_os = "macos")]
struct LiveSidebarObserver<'a> {
  session: &'a auv_driver_macos::MacosDriverSession,
  window: &'a auv_driver::Window,
  sidebar_region: ViewRegionRecord,
  scroll_amount: f64,
}

#[cfg(target_os = "macos")]
impl SidebarObserver for LiveSidebarObserver<'_> {
  fn observe(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let capture = self.session.window().capture(self.window).map_err(|error| ParserDiagnostic {
      code: "capture_failed".to_string(),
      message: error.to_string(),
    })?;
    let region = bounds_to_ratio(self.sidebar_region.bounds, &capture);
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture(&capture, region)
      .map_err(|error| ParserDiagnostic {
        code: "ocr_failed".to_string(),
        message: error.to_string(),
      })?;
    Ok(parse_sidebar_viewport(
      observation_index,
      self.sidebar_region.bounds,
      &recognition,
    ))
  }

  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
    let center = auv_driver::WindowPoint::new(
      self.sidebar_region.bounds.x + self.sidebar_region.bounds.width / 2.0,
      self.sidebar_region.bounds.y + self.sidebar_region.bounds.height / 2.0,
    );
    let screen = self
      .session
      .window()
      .to_screen_point(self.window, center)
      .map_err(|error| ParserDiagnostic {
        code: "scroll_point_projection_failed".to_string(),
        message: error.to_string(),
      })?;
    auv_driver_macos::native::pointer::scroll_point(
      screen.point().x,
      screen.point().y,
      0.0,
      -self.scroll_amount,
    )
    .map_err(|error| ParserDiagnostic {
      code: "scroll_failed".to_string(),
      message: error.to_string(),
    })
  }
}

#[cfg(target_os = "macos")]
fn bounds_to_ratio(bounds: ViewBounds, capture: &Capture) -> RatioRect {
  RatioRect::new(
    bounds.x / capture.bounds.size.width,
    bounds.y / capture.bounds.size.height,
    bounds.width / capture.bounds.size.width,
    bounds.height / capture.bounds.size.height,
  )
}
```

`auv_driver_macos::native::pointer::scroll_point` is a doc-hidden compatibility surface. It is acceptable inside this example only to avoid moving NetEase logic into framework crates. Record a framework gap in the design doc noting that the typed `WindowApi` lacks a public region/window scroll method for examples.

- [ ] **Step 4: Run renderer test**

Run:

```bash
cargo test --example netease_playlist_ls render_human_summary_groups_items_by_section -- --nocapture
```

Expected: renderer test passes.

- [ ] **Step 5: Run full example tests/check**

Run:

```bash
cargo test --example netease_playlist_ls
cargo check --example netease_playlist_ls
```

Expected: all tests pass and example checks. If compile fails because the doc-hidden scroll compatibility function is unavailable, stop and ask for design guidance with the exact compiler error instead of adding a NetEase-specific driver API.

- [ ] **Step 6: Commit**

```bash
git add examples/netease_playlist_ls.rs
git commit -m "feat(examples): wire netease playlist ls"
```

---

### Task 8: Documentation Gap Notes And Final Verification

**Files:**

- Modify: `docs/ai/references/view-memory/2026-05-28-view-parser-ir-netease-playlist-example-design.md`

- [ ] **Step 1: Add implementation notes if gaps were found**

If implementation found a driver/API gap, add a dated bullet under `Open Investigation Items` in the design doc. Use this exact format:

```markdown
- 2026-05-28 implementation note: `<file-or-api>` did not expose `<capability>`.
  The NetEase example keeps parser logic in `examples/` and records this as a
  framework gap instead of adding NetEase-specific code to core.
```

If no gap was found, add this note instead:

```markdown
- 2026-05-28 implementation note: the first example used existing typed
  `auv-driver-macos` capture and OCR APIs. No NetEase-specific logic was moved
  into framework crates.
```

- [ ] **Step 2: Run full verification**

Run:

```bash
cargo fmt --check
cargo test --example netease_playlist_ls
cargo check --example netease_playlist_ls
git diff --check
```

Expected: all commands pass.

- [ ] **Step 3: Optional live smoke**

Only if NetEase Cloud Music is open and screen/OCR permissions are available, run:

```bash
cargo run --example netease_playlist_ls -- --json-out /tmp/auv-netease-playlist-ls.json
```

Expected: command completes with a human-readable summary and writes `/tmp/auv-netease-playlist-ls.json`. If it exits with a structured sidebar/modal/OCR diagnostic, preserve the JSON and screenshot/OCR context before changing parser logic.

- [ ] **Step 4: Commit**

```bash
git add examples/netease_playlist_ls.rs docs/ai/references/view-memory/2026-05-28-view-parser-ir-netease-playlist-example-design.md
git commit -m "docs: record netease playlist example gaps"
```

If the design doc did not change in this task, commit only the example verification fixups with:

```bash
git add examples/netease_playlist_ls.rs
git commit -m "test(examples): verify netease playlist ls"
```

---

## Self-Review

Spec coverage:

- Example-only NetEase logic: Tasks 1-7 keep implementation in `examples/netease_playlist_ls.rs`.
- Structured artifact first: Tasks 2, 4, and 7 define JSON and render human output from it.
- `ViewObservation` as evidence IR and `ViewReconstruction` as operable tree: Tasks 2-4 implement this split.
- `ViewScrollable` as capability, not node kind: Task 2 models `scrollable: Option<ViewScrollable>`.
- Parser supports app/view/region/item shape: Tasks 5 and 7 cover app/window, region, and item parsing inside the example.
- Sidebar resized/missing and modal blocker: Task 5 handles region detection and modal blocker diagnostics.
- Scroll boundary uncertainty: Tasks 4 and 6 record repeated-fingerprint boundary evidence and known limits.
- Inspect viewer and command catalog migration excluded: Task 8 only records gaps.

No W3C HTML/ARIA role taxonomy is needed for v0 because the plan intentionally keeps the node kind set minimal and does not attempt to mirror HTML or ARIA roles. Revisit W3C references only when adding roles such as button, input, dialog, grid, or link.
