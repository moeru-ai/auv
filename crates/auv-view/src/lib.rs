//! Generic view-parser IR shared by AUV app crates.
//!
//! v0 extraction: these types previously lived inside
//! `auv-netease-music/src/lib.rs`. They are framework-level and are not
//! NetEase-specific. App crates (NetEase, future QQ Music, etc.) build
//! their domain projections on top of these types instead of redefining
//! them per app.
//!
//! NOTICE(pub-fields-v0):
//!
//! Every type below exposes `pub` fields. v0 keeps the framework crate's
//! API surface intentionally wide so app crates can construct records
//! via struct literals without going through constructors. Tighten the
//! surface (constructors, builders, `non_exhaustive`) only when a real
//! consumer pressure shows up.
//!
//! Cross-references:
//!
//! - `docs/ai/references/2026-05-29-view-parser-ir-shapes-v0.md` is the
//!   spec these types target. The spec's `ViewNodeId` / `ViewCandidateId`
//!   newtype IDs are NOT yet adopted here; v0 stays at plain `String`
//!   ids to match the existing `auv-netease-music` shape and avoid a
//!   second migration. A future revision can promote the ids to
//!   newtypes once `playlist get <anchor>` lands and requires stable
//!   cross-run identity.

use std::fmt;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Current wire-shape version for view-parser IR artifacts.
///
/// Product crates must use this value when emitting top-level view IR JSON so
/// readers can reject unknown shapes before interpreting app-specific fields.
pub const VIEW_IR_SCHEMA_VERSION: &str = "view-ir-v0";

pub mod memory;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanAppContext {
  pub app_id: Option<String>,
  pub name: Option<String>,
  pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScanWindowContext {
  pub id: Option<String>,
  pub title: Option<String>,
  pub bounds: Option<ViewBounds>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewRegionRecord {
  pub id: Option<String>,
  pub name: Option<String>,
  pub bounds: Option<ViewBounds>,
  pub coordinate_space: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewViewportRecord {
  pub page_index: usize,
  pub bounds: ViewBounds,
  pub axis: ViewAxis,
  pub scroll_offset: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewEvidenceNode {
  pub id: String,
  pub source: ViewEvidenceSource,
  pub label: Option<String>,
  pub bounds: Option<ViewBounds>,
  pub confidence: Confidence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewEvidenceSource {
  #[default]
  OcrText,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewReconstructionRecord {
  pub root: ViewNodeRecord,
  pub anchor_index: Vec<ViewAnchor>,
  pub landmark_index: Vec<ViewLandmark>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewNodeRecord {
  pub id: String,
  pub kind: ViewNodeKind,
  pub domain_kind: Option<String>,
  pub layout: Option<ViewLayout>,
  pub label: Option<String>,
  pub bounds: ViewBounds,
  pub scrollable: Option<ViewScrollable>,
  pub anchors: Vec<ViewAnchor>,
  pub landmarks: Vec<ViewLandmark>,
  pub actions: Vec<ViewAction>,
  pub evidence: Vec<ViewEvidenceNode>,
  pub children: Vec<ViewNodeRecord>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewNodeKind {
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
pub enum ViewLayout {
  VStack,
  HStack,
  Group,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewAxis {
  #[default]
  Vertical,
  Horizontal,
  Both,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewScrollable {
  pub axis: ViewAxis,
  pub boundary: ScrollBoundarySummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewAnchor {
  pub id: String,
  pub label: String,
  pub strength: AnchorStrength,
  pub bounds: ViewBounds,
  pub evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStrength {
  #[default]
  Strong,
  Medium,
  Weak,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewLandmark {
  pub id: String,
  pub label: String,
  #[serde(rename = "use")]
  pub landmark_use: LandmarkUse,
  pub bounds: ViewBounds,
  pub evidence_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkUse {
  ViewportPose,
  BoundaryDetection,
  AnchorReacquire,
  #[default]
  SectionAssignment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewAction {
  Open,
  Select,
  Scroll,
  ObserveOnly,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrollBoundarySummary {
  pub top: BoundaryConfidence,
  pub bottom: BoundaryConfidence,
  pub left: BoundaryConfidence,
  pub right: BoundaryConfidence,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryConfidence {
  Confirmed,
  Likely,
  #[default]
  Unknown,
  Contradicted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
  #[default]
  Low,
  Medium,
  High,
}

impl Confidence {
  /// Compact presentation code used when horizontal space is constrained.
  pub const fn short_code(self) -> &'static str {
    // TODO(confidence-scale-v1): XH/XL and numeric scores are deferred until
    // raw OCR/source scores are approved as part of the shared view contract.
    match self {
      Self::Low => "L",
      Self::Medium => "M",
      Self::High => "H",
    }
  }

  pub fn from_short_code(value: &str) -> Option<Self> {
    match value {
      "L" => Some(Self::Low),
      "M" => Some(Self::Medium),
      "H" => Some(Self::High),
      _ => None,
    }
  }
}

impl fmt::Display for Confidence {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(match self {
      Self::Low => "low",
      Self::Medium => "medium",
      Self::High => "high",
    })
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParserDiagnostic {
  /// Machine-readable diagnostic code.
  ///
  /// TODO(view-diagnostic-kind-v1): keep this as a string until actual
  /// NetEase/parser emissions are classified against
  /// `view-parser-diagnostic-policy-v0.md`. Promote to a typed kind only
  /// after infra errors, parser diagnostics, and test fakes have distinct
  /// lanes; forcing them into one enum now would encode the wrong policy.
  pub code: String,
  pub message: String,
  pub node_id: Option<String>,
}

/// NOTICE(view-bounds-rect-duplication-v0):
///
/// `ViewBounds` and `auv_driver::geometry::Rect` carry the same concept
/// (axis-aligned rectangle in some coordinate space) with the same f64
/// shape, which the workspace primitive-reuse guideline (AGENTS.md,
/// commit 7b520c0) calls out as a duplicate that should normally be
/// collapsed onto the existing primitive.
///
/// v0 keeps both because their **wire shapes differ**:
///
/// - `ViewBounds` serializes flat: `{"x":…,"y":…,"width":…,"height":…}`.
///   Stored NetEase scan JSON (`PlaylistSidebarScan`) is full of this
///   shape, and `auv-netease-music` schema-version rejection (commit
///   0ff745d) locks readers into the current layout.
/// - `auv_driver::geometry::Rect` serializes nested:
///   `{"origin":{"x":…,"y":…},"size":{"width":…,"height":…}}`.
///   It is used by driver capture / window geometry where the
///   `Point` / `Size` typed wrappers (`ScreenPoint`, `WindowPoint`)
///   matter at construction sites.
///
/// Unification therefore needs a wire-shape migration plan (versioned
/// reader, fixture re-records, possibly a serde adapter) before the
/// duplicate type can be deleted. Until that lands, do not "fix" this
/// by adding a `From<Rect>` or by inverting the dep direction —
/// `auv-view` must stay free of `auv-driver` so platform-agnostic
/// crates can keep depending on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewBounds {
  pub x: f64,
  pub y: f64,
  pub width: f64,
  pub height: f64,
}

impl ViewBounds {
  pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
    Self {
      x,
      y,
      width,
      height,
    }
  }
}

// --------------------------------------------------------------------------
// Pure framework utilities. These were lifted from `auv-netease-music`'s
// `lib.rs`; they hold no domain knowledge and any view-parser app can call
// them. Tests live next to the functions to lock the behavior so future
// tuning (e.g. confidence thresholds) is intentional.
// --------------------------------------------------------------------------

/// Normalize a label for identity comparisons: lowercase + trim + drop all
/// whitespace. Matches the "normalized label equality" rule from the
/// merge-fixtures spec.
pub fn normalize_identity(value: &str) -> String {
  value.trim().to_lowercase().chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Slug form of a label: `normalize_identity` then map every non-
/// alphanumeric ASCII char to `_`. Used to build deterministic candidate /
/// node IDs from raw OCR text.
pub fn slug(value: &str) -> String {
  normalize_identity(value).chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }).collect()
}

/// Viewport fingerprint = pipe-joined normalized labels of the evidence
/// nodes that were visible in this observation. Used to detect repeated
/// viewports (stuck scroll / loop boundary) per the diagnostic policy.
pub fn viewport_fingerprint(nodes: &[ViewEvidenceNode]) -> String {
  nodes.iter().filter_map(|node| node.label.as_deref()).map(normalize_identity).collect::<Vec<_>>().join("|")
}

/// REVIEW(confidence-thresholds-v1): the 0.85 / 0.65 split was tuned for
/// Apple Vision OCR scores observed during NetEase capture work. Any view
/// parser using a different OCR provider may need different thresholds;
/// the constants are not load-bearing across providers. When a second
/// provider lands, parameterize via config rather than branching the
/// function.
pub fn confidence_from_ocr(confidence: Option<f32>) -> Confidence {
  match confidence {
    Some(value) if value >= 0.85 => Confidence::High,
    Some(value) if value >= 0.65 => Confidence::Medium,
    _ => Confidence::Low,
  }
}

/// Does the viewport bounding box contain the geometric center of the
/// other box? Used by per-viewport candidate filtering to drop evidence
/// that drifts outside the visible viewport between observations.
pub fn viewport_contains_center(viewport: ViewBounds, bounds: ViewBounds) -> bool {
  let center_x = bounds.x + bounds.width * 0.5;
  let center_y = bounds.y + bounds.height * 0.5;
  center_x >= viewport.x && center_x <= viewport.x + viewport.width && center_y >= viewport.y && center_y <= viewport.y + viewport.height
}

/// Walk a `ViewNodeRecord` tree and accumulate every anchor attached to
/// any node into `anchors`. Order is pre-order (this node, then children).
pub fn collect_anchors(node: &ViewNodeRecord, anchors: &mut Vec<ViewAnchor>) {
  anchors.extend(node.anchors.clone());
  for child in &node.children {
    collect_anchors(child, anchors);
  }
}

/// Walk a `ViewNodeRecord` tree and accumulate every landmark attached to
/// any node into `landmarks`. Order is pre-order (this node, then
/// children).
pub fn collect_landmarks(node: &ViewNodeRecord, landmarks: &mut Vec<ViewLandmark>) {
  landmarks.extend(node.landmarks.clone());
  for child in &node.children {
    collect_landmarks(child, landmarks);
  }
}

// --------------------------------------------------------------------------
// Observer seam. The `ViewObserver` trait is the contract that any view-
// parser observer (live driver-backed, recorded-fixture-backed, fake test
// double) must satisfy. The `Observation` associated type stays domain-
// shaped so the framework crate never names a per-app observation
// record. Scan loops that consume an observer continue to live in the
// app crate today because they read app-specific fields off `Observation`
// (e.g. `viewport_fingerprint`); pull them up only when a second app
// applies the pressure.
// --------------------------------------------------------------------------

pub trait ViewObserver {
  /// Domain observation shape (e.g. `SidebarViewportObservation` in
  /// `auv-netease-music`). Kept as an associated type so the framework
  /// crate never names a per-app record.
  type Observation;

  /// Capture the observation for the given scan-loop step.
  fn observe(&mut self, observation_index: usize) -> Result<Self::Observation, ParserDiagnostic>;

  /// Capture a probe observation without advancing the scan-loop index.
  /// Used for top-seek and boundary probing.
  fn observe_probe(&mut self) -> Result<Self::Observation, ParserDiagnostic>;

  /// Scroll the underlying view up by the observer's configured amount.
  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic>;

  /// Scroll the underlying view down by the observer's configured amount.
  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic>;
}

/// Minimum surface a domain observation type must expose so the framework
/// scan loops can run against it without naming the per-app shape. v0
/// needs `viewport_fingerprint` (drives repeated-viewport detection) plus
/// `parser_notes` and `has_evidence` (drive `reconstruct`'s diagnostic
/// aggregation and the "evidence-but-no-candidates" detector). Default
/// impls keep existing consumers backwards-compatible.
pub trait ViewObservation {
  fn viewport_fingerprint(&self) -> &str;

  /// Parser notes raised during this observation, forwarded into the
  /// reconstruction's `diagnostics`. Default: empty slice.
  fn parser_notes(&self) -> &[ParserDiagnostic] {
    &[]
  }

  /// Whether the observation gathered any evidence at all. Used by
  /// `reconstruct` to decide whether to raise a
  /// `parser_no_reliable_candidates` diagnostic when no candidates were
  /// accepted. Default: false (an observation type with no evidence
  /// notion never participates in that detector).
  fn has_evidence(&self) -> bool {
    false
  }
}

/// Knobs the scan loop reads to decide when to stop. Cap on observation
/// count (`max_pages`) is independent from cap on scroll calls
/// (`max_scrolls`) so apps can prevent runaway parses without coupling
/// the two dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanOptions {
  pub max_pages: usize,
  pub max_scrolls: usize,
}

/// Outcome of the top-seek pre-loop. `boundary` is `Likely` when two
/// consecutive scroll-up + probe attempts produced the same fingerprint
/// (the view didn't move, almost certainly at the top). Diagnostics and
/// known limits carry the observer's reports so callers can attach them
/// to whatever scan result they construct.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopSeekOutcome {
  pub boundary: BoundaryConfidence,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

/// What `scan_with_observer` returns: the observations the loop captured
/// plus the diagnostics and known limits the loop accumulated. `Obs` is
/// the observer's `Observation` associated type so the result stays
/// per-app even though the loop is framework code.
#[derive(Clone, Debug)]
pub struct ScanLoopOutcome<Obs> {
  pub observations: Vec<Obs>,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

/// Drive the observer back to the top of its scrollable surface. v0
/// strategy: probe → scroll up → probe again; if the fingerprint is
/// unchanged, the view is already (or now) at the top and we report
/// `BoundaryConfidence::Likely`. Bounded by `max_scrolls` so a broken
/// observer cannot loop forever.
pub fn scroll_to_top<O>(observer: &mut O, max_scrolls: usize) -> TopSeekOutcome
where
  O: ViewObserver,
  O::Observation: ViewObservation,
{
  let mut outcome = TopSeekOutcome::default();
  let mut previous_fingerprint = match observer.observe_probe() {
    Ok(observation) => observation.viewport_fingerprint().to_string(),
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
    let fingerprint = observation.viewport_fingerprint();
    if fingerprint == previous_fingerprint {
      outcome.boundary = BoundaryConfidence::Likely;
      return outcome;
    }
    previous_fingerprint = fingerprint.to_string();
  }
  outcome.known_limits.push(format!("top seek stopped after max_scrolls={max_scrolls}"));
  outcome
}

/// Run the per-observation scan loop: observe, push, check repeated
/// fingerprint (boundary), check page/scroll caps, scroll down, repeat.
/// The loop stops on the first of: repeated fingerprint, observer error,
/// `max_pages` cap, `max_scrolls` cap.
pub fn scan_with_observer<O>(observer: &mut O, options: ScanOptions) -> ScanLoopOutcome<O::Observation>
where
  O: ViewObserver,
  O::Observation: ViewObservation,
{
  let mut observations: Vec<O::Observation> = Vec::new();
  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();
  let mut previous_fingerprint: Option<String> = None;
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
    let fingerprint = observation.viewport_fingerprint().to_string();
    let repeated_fingerprint = previous_fingerprint.as_deref().is_some_and(|prev| prev == fingerprint.as_str());
    previous_fingerprint = Some(fingerprint);
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

  ScanLoopOutcome {
    observations,
    diagnostics,
    known_limits,
  }
}

/// Derive a `ScrollBoundarySummary` from a slice of observations by
/// looking for adjacent identical viewport fingerprints. v0 only
/// populates `bottom = Likely` on a match — top boundaries come from
/// `scroll_to_top`, not from observing the scan loop's output, because
/// the loop scrolls downward and never re-probes upward.
pub fn boundary_summary_from_observations<O>(observations: &[O]) -> ScrollBoundarySummary
where
  O: ViewObservation,
{
  let mut summary = ScrollBoundarySummary::default();
  if observations.windows(2).any(|pair| pair[0].viewport_fingerprint() == pair[1].viewport_fingerprint()) {
    summary.bottom = BoundaryConfidence::Likely;
  }
  summary
}

// --------------------------------------------------------------------------
// Reconstruction policy seam. The framework owns the loop, the per-section
// item dedup, the section indexing, and the anchor/landmark collection. The
// app crate provides a `ReconstructionPolicy` impl that knows how to:
//   - classify candidates (header vs item vs unknown),
//   - derive a section key for header dedup,
//   - construct domain section / item nodes and projection records,
//   - build the root container.
// This lets a second app (future QQ Music, etc.) reuse the reconstruction
// pipeline without duplicating the ~150-line walk that lived in NetEase.
// --------------------------------------------------------------------------

/// What the framework should do with a candidate after `classify` reads it.
///
/// `Header` opens (or merges into) a section identified by `section_key`.
/// `Item` lands inside the current section; `dedupe_key` is checked against
/// items previously added to that section.
/// `Unknown` is silently skipped.
pub enum CandidateRole<SectionKey> {
  Header { section_key: SectionKey },
  Item { dedupe_key: String },
  Unknown,
}

/// What `reconstruct` returns. `sections` is the app's projection record
/// list in declaration order; `root` is the tree the app's policy built.
pub struct ReconstructionOutput<SectionProjection> {
  pub root: ViewNodeRecord,
  pub anchor_index: Vec<ViewAnchor>,
  pub landmark_index: Vec<ViewLandmark>,
  pub sections: Vec<SectionProjection>,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub boundary: ScrollBoundarySummary,
}

/// Policy injected by an app crate into `reconstruct`. Associated types
/// keep the framework crate from naming the app's candidate / projection
/// records.
///
/// Method-call discipline: `build_section` / `build_item` /
/// `build_unassigned_section` / `build_root` are called by the framework
/// at well-defined moments. Don't call them yourself from inside the
/// policy. `emit_dedup_diagnostic` is invoked once per de-duplicated item
/// so the app keeps control of the diagnostic message text.
pub trait ReconstructionPolicy {
  type Candidate;
  type SectionKey: std::hash::Hash + Eq + Clone;
  type SectionProjection;
  type ItemProjection;
  type Observation: ViewObservation;

  /// Iterate the candidates carried by one observation. Order is
  /// observation order (the policy must not re-sort).
  ///
  /// Rust 2024 RPITIT: the returned iterator type is implementer-chosen
  /// and bound to `'a`, so apps can return `observation.candidates.iter()`
  /// (or any other zero-allocation iterator) without boxing. The
  /// `Self::Candidate: 'a` bound is required for the `&'a Self::Candidate`
  /// items to be well-formed when the associated type is not `'static`.
  fn candidates<'a>(&self, observation: &'a Self::Observation) -> impl Iterator<Item = &'a Self::Candidate> + 'a
  where
    Self::Candidate: 'a;

  /// Decide whether a candidate is a section header, a section item, or
  /// neither. The returned `SectionKey` (for headers) is used to dedup
  /// across observations; two headers with equal keys merge into one
  /// section.
  fn classify(&self, candidate: &Self::Candidate) -> CandidateRole<Self::SectionKey>;

  /// Build the section node + the section projection record for a
  /// newly-encountered header candidate. Called the first time a given
  /// section key is seen.
  fn build_section(&self, observation: &Self::Observation, candidate: &Self::Candidate) -> (ViewNodeRecord, Self::SectionProjection);

  /// Build the fallback section that absorbs items appearing before any
  /// header. Called at most once per `reconstruct`, lazily.
  fn build_unassigned_section(&self) -> (ViewNodeRecord, Self::SectionProjection);

  /// Build the item node + the item projection record for a candidate
  /// that has passed the per-section dedup check. The current section's
  /// projection is provided read-only so the policy can compute fields
  /// like `section_hint`.
  fn build_item(
    &self,
    observation: &Self::Observation,
    candidate: &Self::Candidate,
    section: &Self::SectionProjection,
  ) -> (ViewNodeRecord, Self::ItemProjection);

  /// Attach the item node to its parent section node. Default impl appends
  /// to `section_node.children`.
  fn attach_item_to_section_node(&self, section_node: &mut ViewNodeRecord, item_node: ViewNodeRecord) {
    section_node.children.push(item_node);
  }

  /// Append the item projection to the section projection. Apps with
  /// `items: Vec<_>` on their section type implement this with a push;
  /// apps with non-`Vec` containers replace the strategy.
  fn append_item_to_section_projection(&self, section: &mut Self::SectionProjection, item: Self::ItemProjection);

  /// Build the root container that holds every section node. Called once
  /// at the end of `reconstruct`. The app picks the root id, domain_kind,
  /// layout, scroll axis, and bounds.
  fn build_root(&self, sidebar_bounds: ViewBounds, boundary: ScrollBoundarySummary, section_children: Vec<ViewNodeRecord>)
  -> ViewNodeRecord;

  /// Emit the diagnostic when an item duplicate is detected. The policy
  /// owns the wording (NetEase uses `"deduplicated repeated sidebar item
  /// {label:?} in section {section_hint:?}"`); the framework owns the
  /// detection.
  fn emit_dedup_diagnostic(&self, candidate: &Self::Candidate, section: &Self::SectionProjection) -> ParserDiagnostic;
}

/// Run the framework reconstruction loop against the policy. The loop:
///
/// 1. Carries forward each observation's `parser_notes` into diagnostics.
/// 2. Walks every candidate in observation order. Headers create or merge
///    into a section keyed by `policy.classify().section_key`. Items land
///    under the current section (or a lazily-built unassigned section if
///    no header has appeared yet) after passing a per-section dedup check.
/// 3. Emits one `parser_no_reliable_candidates` diagnostic if any
///    observation had evidence but the whole scan produced no projection
///    sections.
/// 4. Asks the policy to build the root, then walks the resulting tree to
///    collect anchors and landmarks in pre-order.
///
/// The boundary returned is `boundary_summary_from_observations(observations)`.
pub fn reconstruct<P>(policy: &P, observations: &[P::Observation], sidebar_bounds: ViewBounds) -> ReconstructionOutput<P::SectionProjection>
where
  P: ReconstructionPolicy,
{
  use std::collections::{HashMap, HashSet};

  let boundary = boundary_summary_from_observations(observations);
  let mut section_nodes: Vec<ViewNodeRecord> = Vec::new();
  let mut section_projections: Vec<P::SectionProjection> = Vec::new();
  let mut diagnostics: Vec<ParserDiagnostic> =
    observations.iter().flat_map(|observation| observation.parser_notes().iter().cloned()).collect();
  let mut current_section_index: Option<usize> = None;
  let mut section_indices: HashMap<P::SectionKey, usize> = HashMap::new();
  let mut seen_items_by_section: Vec<HashSet<String>> = Vec::new();

  for observation in observations {
    for candidate in policy.candidates(observation) {
      match policy.classify(candidate) {
        CandidateRole::Header { section_key } => {
          if let Some(&idx) = section_indices.get(&section_key) {
            current_section_index = Some(idx);
          } else {
            let (node, projection) = policy.build_section(observation, candidate);
            section_nodes.push(node);
            section_projections.push(projection);
            seen_items_by_section.push(HashSet::new());
            let idx = section_nodes.len() - 1;
            section_indices.insert(section_key, idx);
            current_section_index = Some(idx);
          }
        }
        CandidateRole::Item { dedupe_key } => {
          let section_index = *current_section_index.get_or_insert_with(|| {
            let (node, projection) = policy.build_unassigned_section();
            section_nodes.push(node);
            section_projections.push(projection);
            seen_items_by_section.push(HashSet::new());
            section_nodes.len() - 1
          });
          if !seen_items_by_section[section_index].insert(dedupe_key) {
            diagnostics.push(policy.emit_dedup_diagnostic(candidate, &section_projections[section_index]));
            continue;
          }
          let (item_node, item_projection) = policy.build_item(observation, candidate, &section_projections[section_index]);
          policy.attach_item_to_section_node(&mut section_nodes[section_index], item_node);
          policy.append_item_to_section_projection(&mut section_projections[section_index], item_projection);
        }
        CandidateRole::Unknown => {}
      }
    }
  }

  let any_evidence = observations.iter().any(|observation| observation.has_evidence());
  if any_evidence && section_projections.is_empty() {
    diagnostics.push(ParserDiagnostic {
      code: "parser_no_reliable_candidates".to_string(),
      message: "OCR evidence was observed but no reliable sidebar candidates were accepted".to_string(),
      node_id: None,
    });
  }

  let root = policy.build_root(sidebar_bounds, boundary.clone(), section_nodes);
  let mut anchor_index = Vec::new();
  let mut landmark_index = Vec::new();
  collect_anchors(&root, &mut anchor_index);
  collect_landmarks(&root, &mut landmark_index);

  ReconstructionOutput {
    root,
    anchor_index,
    landmark_index,
    sections: section_projections,
    diagnostics,
    boundary,
  }
}

// --------------------------------------------------------------------------
// Pixel-level drawing helpers. Used by view-parser apps that want to render
// overlay diagnostics (which evidence node was matched, which candidate
// kind it became, where the region was detected) on top of a captured
// screenshot. These helpers are pure pixel ops over `image::RgbaImage`;
// they hold no NetEase or other domain knowledge. App-specific overlay
// composition (color choice per candidate kind, what to draw) stays in
// the app crate.
// --------------------------------------------------------------------------

/// Draw the outline of `bounds` on `image` with `color`, growing the
/// stroke inward by `stroke` pixels. Out-of-bounds pixels are silently
/// dropped by `put_pixel`.
pub fn draw_rect(image: &mut RgbaImage, bounds: ViewBounds, color: Rgba<u8>, stroke: i64) {
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

/// Bresenham line from `(x0,y0)` to `(x1,y1)` on `image` with `color`.
/// Out-of-bounds pixels are silently dropped by `put_pixel`.
pub fn draw_line(image: &mut RgbaImage, mut x0: i64, mut y0: i64, x1: i64, y1: i64, color: Rgba<u8>) {
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

/// Set the pixel at `(x,y)` to `color`, doing nothing if the coordinate
/// is outside `image`. The clamp lets callers project window-local
/// bounds onto a capture without first intersecting against the capture
/// rectangle.
pub fn put_pixel(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
  if x < 0 || y < 0 || x >= image.width() as i64 || y >= image.height() as i64 {
    return;
  }
  image.put_pixel(x as u32, y as u32, color);
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
