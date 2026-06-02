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

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Current wire-shape version for view-parser IR artifacts.
///
/// Product crates must use this value when emitting top-level view IR JSON so
/// readers can reject unknown shapes before interpreting app-specific fields.
pub const VIEW_IR_SCHEMA_VERSION: &str = "view-ir-v0";

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
  OcrText,
  AxNode,
  IconMatch,
  #[default]
  Visual,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
  High,
  Medium,
  #[default]
  Low,
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
  value
    .trim()
    .to_lowercase()
    .chars()
    .filter(|ch| !ch.is_whitespace())
    .collect()
}

/// Slug form of a label: `normalize_identity` then map every non-
/// alphanumeric ASCII char to `_`. Used to build deterministic candidate /
/// node IDs from raw OCR text.
pub fn slug(value: &str) -> String {
  normalize_identity(value)
    .chars()
    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
    .collect()
}

/// Viewport fingerprint = pipe-joined normalized labels of the evidence
/// nodes that were visible in this observation. Used to detect repeated
/// viewports (stuck scroll / loop boundary) per the diagnostic policy.
pub fn viewport_fingerprint(nodes: &[ViewEvidenceNode]) -> String {
  nodes
    .iter()
    .filter_map(|node| node.label.as_deref())
    .map(normalize_identity)
    .collect::<Vec<_>>()
    .join("|")
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
  center_x >= viewport.x
    && center_x <= viewport.x + viewport.width
    && center_y >= viewport.y
    && center_y <= viewport.y + viewport.height
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
  outcome
    .known_limits
    .push(format!("top seek stopped after max_scrolls={max_scrolls}"));
  outcome
}

/// Run the per-observation scan loop: observe, push, check repeated
/// fingerprint (boundary), check page/scroll caps, scroll down, repeat.
/// The loop stops on the first of: repeated fingerprint, observer error,
/// `max_pages` cap, `max_scrolls` cap.
pub fn scan_with_observer<O>(
  observer: &mut O,
  options: ScanOptions,
) -> ScanLoopOutcome<O::Observation>
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
    let repeated_fingerprint = previous_fingerprint
      .as_deref()
      .is_some_and(|prev| prev == fingerprint.as_str());
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
  if observations
    .windows(2)
    .any(|pair| pair[0].viewport_fingerprint() == pair[1].viewport_fingerprint())
  {
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
  fn candidates<'a>(
    &self,
    observation: &'a Self::Observation,
  ) -> Box<dyn Iterator<Item = &'a Self::Candidate> + 'a>;

  /// Decide whether a candidate is a section header, a section item, or
  /// neither. The returned `SectionKey` (for headers) is used to dedup
  /// across observations; two headers with equal keys merge into one
  /// section.
  fn classify(&self, candidate: &Self::Candidate) -> CandidateRole<Self::SectionKey>;

  /// Build the section node + the section projection record for a
  /// newly-encountered header candidate. Called the first time a given
  /// section key is seen.
  fn build_section(
    &self,
    observation: &Self::Observation,
    candidate: &Self::Candidate,
  ) -> (ViewNodeRecord, Self::SectionProjection);

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
  fn attach_item_to_section_node(
    &self,
    section_node: &mut ViewNodeRecord,
    item_node: ViewNodeRecord,
  ) {
    section_node.children.push(item_node);
  }

  /// Append the item projection to the section projection. Apps with
  /// `items: Vec<_>` on their section type implement this with a push;
  /// apps with non-`Vec` containers replace the strategy.
  fn append_item_to_section_projection(
    &self,
    section: &mut Self::SectionProjection,
    item: Self::ItemProjection,
  );

  /// Build the root container that holds every section node. Called once
  /// at the end of `reconstruct`. The app picks the root id, domain_kind,
  /// layout, scroll axis, and bounds.
  fn build_root(
    &self,
    sidebar_bounds: ViewBounds,
    boundary: ScrollBoundarySummary,
    section_children: Vec<ViewNodeRecord>,
  ) -> ViewNodeRecord;

  /// Emit the diagnostic when an item duplicate is detected. The policy
  /// owns the wording (NetEase uses `"deduplicated repeated sidebar item
  /// {label:?} in section {section_hint:?}"`); the framework owns the
  /// detection.
  fn emit_dedup_diagnostic(
    &self,
    candidate: &Self::Candidate,
    section: &Self::SectionProjection,
  ) -> ParserDiagnostic;
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
pub fn reconstruct<P>(
  policy: &P,
  observations: &[P::Observation],
  sidebar_bounds: ViewBounds,
) -> ReconstructionOutput<P::SectionProjection>
where
  P: ReconstructionPolicy,
{
  use std::collections::{HashMap, HashSet};

  let boundary = boundary_summary_from_observations(observations);
  let mut section_nodes: Vec<ViewNodeRecord> = Vec::new();
  let mut section_projections: Vec<P::SectionProjection> = Vec::new();
  let mut diagnostics: Vec<ParserDiagnostic> = observations
    .iter()
    .flat_map(|observation| observation.parser_notes().iter().cloned())
    .collect();
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
            diagnostics
              .push(policy.emit_dedup_diagnostic(candidate, &section_projections[section_index]));
            continue;
          }
          let (item_node, item_projection) =
            policy.build_item(observation, candidate, &section_projections[section_index]);
          policy.attach_item_to_section_node(&mut section_nodes[section_index], item_node);
          policy.append_item_to_section_projection(
            &mut section_projections[section_index],
            item_projection,
          );
        }
        CandidateRole::Unknown => {}
      }
    }
  }

  let any_evidence = observations
    .iter()
    .any(|observation| observation.has_evidence());
  if any_evidence && section_projections.is_empty() {
    diagnostics.push(ParserDiagnostic {
      code: "parser_no_reliable_candidates".to_string(),
      message: "OCR evidence was observed but no reliable sidebar candidates were accepted"
        .to_string(),
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
pub fn draw_line(
  image: &mut RgbaImage,
  mut x0: i64,
  mut y0: i64,
  x1: i64,
  y1: i64,
  color: Rgba<u8>,
) {
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
mod tests {
  use super::*;

  #[test]
  fn normalize_identity_lowercases_and_drops_whitespace() {
    assert_eq!(normalize_identity("  Hello World  "), "helloworld");
    assert_eq!(normalize_identity("我 的 歌单"), "我的歌单");
    assert_eq!(normalize_identity(""), "");
  }

  #[test]
  fn slug_maps_non_alnum_to_underscore() {
    assert_eq!(slug("Hello World"), "helloworld");
    assert_eq!(slug("My-Playlist!"), "my_playlist_");
    assert_eq!(slug("我的歌单"), "____"); // Chinese chars are non-ASCII-alphanumeric
  }

  #[test]
  fn viewport_fingerprint_joins_normalized_labels_with_pipe() {
    let nodes = vec![
      ViewEvidenceNode {
        id: "a".into(),
        source: ViewEvidenceSource::OcrText,
        label: Some("Liked Songs".into()),
        bounds: None,
        confidence: Confidence::High,
      },
      ViewEvidenceNode {
        id: "b".into(),
        source: ViewEvidenceSource::OcrText,
        label: Some("Daily Mix 1".into()),
        bounds: None,
        confidence: Confidence::Medium,
      },
      ViewEvidenceNode {
        // labels: None nodes are skipped
        id: "c".into(),
        source: ViewEvidenceSource::AxNode,
        label: None,
        bounds: None,
        confidence: Confidence::Low,
      },
    ];
    assert_eq!(viewport_fingerprint(&nodes), "likedsongs|dailymix1");
  }

  #[test]
  fn confidence_from_ocr_threshold_mapping() {
    assert_eq!(confidence_from_ocr(Some(0.95)), Confidence::High);
    assert_eq!(confidence_from_ocr(Some(0.85)), Confidence::High); // boundary inclusive
    assert_eq!(confidence_from_ocr(Some(0.80)), Confidence::Medium);
    assert_eq!(confidence_from_ocr(Some(0.65)), Confidence::Medium); // boundary inclusive
    assert_eq!(confidence_from_ocr(Some(0.50)), Confidence::Low);
    assert_eq!(confidence_from_ocr(None), Confidence::Low);
  }

  #[test]
  fn viewport_contains_center_uses_geometric_center() {
    let viewport = ViewBounds::new(0.0, 0.0, 100.0, 100.0);
    // Center (50,50) is inside
    assert!(viewport_contains_center(
      viewport,
      ViewBounds::new(40.0, 40.0, 20.0, 20.0)
    ));
    // Center (150, 50) is outside despite bounds overlapping
    assert!(!viewport_contains_center(
      viewport,
      ViewBounds::new(100.0, 40.0, 100.0, 20.0)
    ));
    // Exact boundary inclusive
    assert!(viewport_contains_center(
      viewport,
      ViewBounds::new(90.0, 90.0, 20.0, 20.0)
    ));
  }

  #[test]
  fn collect_anchors_walks_tree_in_preorder() {
    let anchor = |id: &str| ViewAnchor {
      id: id.into(),
      label: id.into(),
      strength: AnchorStrength::Strong,
      bounds: ViewBounds::default(),
      evidence_ids: Vec::new(),
    };
    let root = ViewNodeRecord {
      anchors: vec![anchor("root")],
      children: vec![
        ViewNodeRecord {
          anchors: vec![anchor("child-a")],
          ..Default::default()
        },
        ViewNodeRecord {
          anchors: vec![anchor("child-b")],
          children: vec![ViewNodeRecord {
            anchors: vec![anchor("grandchild")],
            ..Default::default()
          }],
          ..Default::default()
        },
      ],
      ..Default::default()
    };
    let mut out = Vec::new();
    collect_anchors(&root, &mut out);
    assert_eq!(
      out.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
      vec!["root", "child-a", "child-b", "grandchild"]
    );
  }

  #[test]
  fn collect_landmarks_walks_tree_in_preorder() {
    let landmark = |id: &str| ViewLandmark {
      id: id.into(),
      label: id.into(),
      landmark_use: LandmarkUse::SectionAssignment,
      bounds: ViewBounds::default(),
      evidence_ids: Vec::new(),
    };
    let root = ViewNodeRecord {
      landmarks: vec![landmark("root")],
      children: vec![ViewNodeRecord {
        landmarks: vec![landmark("child")],
        ..Default::default()
      }],
      ..Default::default()
    };
    let mut out = Vec::new();
    collect_landmarks(&root, &mut out);
    assert_eq!(
      out.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(),
      vec!["root", "child"]
    );
  }

  // ------------------------------------------------------------------------
  // Scan-loop / top-seek coverage. FakeObservation + FakeObserver are
  // programmable per-test (provide a queue of fingerprints; flag scrolls as
  // failing if needed). These tests lock the loop's termination contract:
  // repeated fingerprint, error handling, and both caps.
  // ------------------------------------------------------------------------

  #[derive(Clone, Debug)]
  struct FakeObservation {
    fingerprint: String,
  }

  impl ViewObservation for FakeObservation {
    fn viewport_fingerprint(&self) -> &str {
      &self.fingerprint
    }
  }

  #[derive(Default)]
  struct FakeObserver {
    fingerprints: Vec<&'static str>,
    cursor: usize,
    fail_observe_after: Option<usize>,
    fail_scroll_down_after: Option<usize>,
    fail_scroll_up_after: Option<usize>,
    scroll_up_calls: usize,
    scroll_down_calls: usize,
  }

  impl FakeObserver {
    fn new(fingerprints: Vec<&'static str>) -> Self {
      Self {
        fingerprints,
        ..Self::default()
      }
    }

    fn diagnostic(code: &str) -> ParserDiagnostic {
      ParserDiagnostic {
        code: code.to_string(),
        message: code.to_string(),
        node_id: None,
      }
    }

    fn take_at(&self, index: usize) -> Result<FakeObservation, ParserDiagnostic> {
      self
        .fingerprints
        .get(index)
        .map(|fp| FakeObservation {
          fingerprint: (*fp).to_string(),
        })
        .ok_or_else(|| Self::diagnostic("no_more_fake_observations"))
    }
  }

  impl ViewObserver for FakeObserver {
    type Observation = FakeObservation;

    fn observe(
      &mut self,
      _observation_index: usize,
    ) -> Result<Self::Observation, ParserDiagnostic> {
      if let Some(after) = self.fail_observe_after {
        if self.cursor >= after {
          return Err(Self::diagnostic("observe_failed"));
        }
      }
      let observation = self.take_at(self.cursor)?;
      self.cursor += 1;
      Ok(observation)
    }

    fn observe_probe(&mut self) -> Result<Self::Observation, ParserDiagnostic> {
      self.take_at(self.cursor)
    }

    fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
      self.scroll_up_calls += 1;
      if let Some(after) = self.fail_scroll_up_after {
        if self.scroll_up_calls > after {
          return Err(Self::diagnostic("scroll_up_failed"));
        }
      }
      // For top-seek tests we mutate cursor so the next probe sees the next
      // fingerprint in the queue, simulating an actually-scrolled viewport.
      self.cursor = self.cursor.saturating_sub(0); // no-op: probe re-reads same cursor
      Ok(())
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      self.scroll_down_calls += 1;
      if let Some(after) = self.fail_scroll_down_after {
        if self.scroll_down_calls > after {
          return Err(Self::diagnostic("scroll_down_failed"));
        }
      }
      Ok(())
    }
  }

  #[test]
  fn scan_with_observer_stops_on_repeated_fingerprint() {
    let mut observer = FakeObserver::new(vec!["a", "b", "b"]);
    let outcome = scan_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 16,
        max_scrolls: 16,
      },
    );

    assert_eq!(outcome.observations.len(), 3);
    assert_eq!(
      outcome
        .observations
        .iter()
        .map(|o| o.viewport_fingerprint())
        .collect::<Vec<_>>(),
      vec!["a", "b", "b"]
    );
    assert!(outcome.diagnostics.is_empty());
    assert!(
      outcome.known_limits.is_empty(),
      "boundary hit, no cap fired"
    );
  }

  #[test]
  fn scan_with_observer_stops_at_max_pages_and_records_known_limit() {
    let mut observer = FakeObserver::new(vec!["a", "b", "c", "d", "e"]);
    let outcome = scan_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 2,
        max_scrolls: 16,
      },
    );

    assert_eq!(outcome.observations.len(), 2);
    assert!(outcome.diagnostics.is_empty());
    assert_eq!(outcome.known_limits.len(), 1);
    assert!(outcome.known_limits[0].contains("max_pages=2"));
  }

  #[test]
  fn scan_with_observer_stops_at_max_scrolls_and_records_known_limit() {
    let mut observer = FakeObserver::new(vec!["a", "b", "c", "d", "e"]);
    let outcome = scan_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 16,
        max_scrolls: 1,
      },
    );

    // First observation (cursor 0 → "a"), scroll #1 OK; second observation
    // (cursor 1 → "b"), scroll cap exceeded, break before scroll #2.
    assert_eq!(outcome.observations.len(), 2);
    assert!(outcome.diagnostics.is_empty());
    assert_eq!(outcome.known_limits.len(), 1);
    assert!(outcome.known_limits[0].contains("max_scrolls=1"));
  }

  #[test]
  fn scan_with_observer_records_diagnostic_and_breaks_on_observe_error() {
    let mut observer = FakeObserver::new(vec!["a", "b"]);
    observer.fail_observe_after = Some(1);
    let outcome = scan_with_observer(
      &mut observer,
      ScanOptions {
        max_pages: 16,
        max_scrolls: 16,
      },
    );

    // First observation succeeds; second errors before being pushed.
    assert_eq!(outcome.observations.len(), 1);
    assert_eq!(outcome.diagnostics.len(), 1);
    assert_eq!(outcome.diagnostics[0].code, "observe_failed");
  }

  #[test]
  fn scroll_to_top_reports_likely_boundary_on_repeated_fingerprint() {
    // Probe sees "a"; after scroll_up, probe sees "a" again — view didn't
    // move, declare top boundary as Likely.
    let mut observer = FakeObserver::new(vec!["a", "a"]);
    let outcome = scroll_to_top(&mut observer, 8);

    assert_eq!(outcome.boundary, BoundaryConfidence::Likely);
    assert!(outcome.diagnostics.is_empty());
    assert!(outcome.known_limits.is_empty());
    assert_eq!(observer.scroll_up_calls, 1);
  }

  #[test]
  fn scroll_to_top_records_known_limit_when_max_scrolls_exhausted() {
    // Every probe returns a different fingerprint forever; top-seek runs
    // out of scrolls without seeing a repeat.
    struct AlwaysNew {
      counter: usize,
    }
    impl ViewObserver for AlwaysNew {
      type Observation = FakeObservation;
      fn observe(&mut self, _: usize) -> Result<Self::Observation, ParserDiagnostic> {
        unreachable!("top-seek does not call observe")
      }
      fn observe_probe(&mut self) -> Result<Self::Observation, ParserDiagnostic> {
        let fp = format!("fp-{}", self.counter);
        self.counter += 1;
        Ok(FakeObservation { fingerprint: fp })
      }
      fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
        Ok(())
      }
      fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
        unreachable!("top-seek does not call scroll_down")
      }
    }

    let mut observer = AlwaysNew { counter: 0 };
    let outcome = scroll_to_top(&mut observer, 3);

    assert_eq!(outcome.boundary, BoundaryConfidence::Unknown);
    assert_eq!(outcome.known_limits.len(), 1);
    assert!(outcome.known_limits[0].contains("max_scrolls=3"));
  }

  #[test]
  fn boundary_summary_likely_on_adjacent_repeat() {
    let obs = vec![
      FakeObservation {
        fingerprint: "a".into(),
      },
      FakeObservation {
        fingerprint: "b".into(),
      },
      FakeObservation {
        fingerprint: "b".into(),
      },
    ];
    let summary = boundary_summary_from_observations(&obs);
    assert_eq!(summary.bottom, BoundaryConfidence::Likely);
    assert_eq!(summary.top, BoundaryConfidence::Unknown);
  }

  #[test]
  fn boundary_summary_unknown_when_no_adjacent_repeat() {
    let obs = vec![
      FakeObservation {
        fingerprint: "a".into(),
      },
      FakeObservation {
        fingerprint: "b".into(),
      },
      FakeObservation {
        fingerprint: "c".into(),
      },
    ];
    let summary = boundary_summary_from_observations(&obs);
    assert_eq!(summary.bottom, BoundaryConfidence::Unknown);
  }

  #[test]
  fn boundary_summary_unknown_on_non_adjacent_repeat() {
    // Non-adjacent fingerprint repeat should NOT trigger Likely — only
    // adjacent identical pairs do. Other repeats are handled by
    // RepeatedViewport diagnostics in the policy spec.
    let obs = vec![
      FakeObservation {
        fingerprint: "a".into(),
      },
      FakeObservation {
        fingerprint: "b".into(),
      },
      FakeObservation {
        fingerprint: "a".into(),
      },
    ];
    let summary = boundary_summary_from_observations(&obs);
    assert_eq!(summary.bottom, BoundaryConfidence::Unknown);
  }

  #[test]
  fn put_pixel_clamps_out_of_bounds() {
    let mut img = RgbaImage::new(4, 4);
    let color = Rgba([1, 2, 3, 255]);
    // In-bounds writes apply.
    put_pixel(&mut img, 0, 0, color);
    put_pixel(&mut img, 3, 3, color);
    assert_eq!(img.get_pixel(0, 0), &color);
    assert_eq!(img.get_pixel(3, 3), &color);
    // Out-of-bounds writes are silently dropped.
    put_pixel(&mut img, -1, 2, color);
    put_pixel(&mut img, 2, -1, color);
    put_pixel(&mut img, 4, 2, color);
    put_pixel(&mut img, 2, 4, color);
    // Untouched cell stays default (0,0,0,0).
    assert_eq!(img.get_pixel(2, 2), &Rgba([0, 0, 0, 0]));
  }

  #[test]
  fn draw_line_paints_horizontal_segment() {
    let mut img = RgbaImage::new(8, 4);
    let color = Rgba([10, 20, 30, 255]);
    draw_line(&mut img, 1, 2, 5, 2, color);
    for x in 1..=5 {
      assert_eq!(
        img.get_pixel(x as u32, 2),
        &color,
        "x={x} should be painted"
      );
    }
    assert_eq!(img.get_pixel(0, 2), &Rgba([0, 0, 0, 0]));
    assert_eq!(img.get_pixel(6, 2), &Rgba([0, 0, 0, 0]));
  }

  #[test]
  fn draw_rect_outlines_bounds_with_stroke() {
    let mut img = RgbaImage::new(10, 10);
    let color = Rgba([200, 100, 50, 255]);
    draw_rect(&mut img, ViewBounds::new(2.0, 2.0, 6.0, 6.0), color, 1);
    // Corners on the rectangle are painted.
    assert_eq!(img.get_pixel(2, 2), &color);
    assert_eq!(img.get_pixel(8, 2), &color);
    assert_eq!(img.get_pixel(2, 8), &color);
    assert_eq!(img.get_pixel(8, 8), &color);
    // Interior is not painted.
    assert_eq!(img.get_pixel(5, 5), &Rgba([0, 0, 0, 0]));
    // Outside is not painted.
    assert_eq!(img.get_pixel(1, 1), &Rgba([0, 0, 0, 0]));
    assert_eq!(img.get_pixel(9, 9), &Rgba([0, 0, 0, 0]));
  }

  // ------------------------------------------------------------------------
  // Reconstruction policy coverage. A minimal FakePolicy + Fake records
  // drive `reconstruct` through every branch: section dedup by key, item
  // dedup within section, unassigned-section fallback, evidence-but-no-
  // candidates diagnostic, and anchor/landmark collection. These tests
  // lock the contract the framework promises to NeteasePolicy and any
  // future app policy.
  // ------------------------------------------------------------------------

  #[derive(Clone, Debug)]
  enum FakeCandidateKind {
    Header,
    Item,
    Unknown,
  }

  #[derive(Clone, Debug)]
  struct FakeCandidate {
    id: String,
    label: String,
    kind: FakeCandidateKind,
    /// Section key when kind == Header.
    section_key: Option<String>,
  }

  #[derive(Debug)]
  struct FakeReconstructObservation {
    fingerprint: String,
    candidates: Vec<FakeCandidate>,
    parser_notes_vec: Vec<ParserDiagnostic>,
    evidence_present: bool,
  }

  impl ViewObservation for FakeReconstructObservation {
    fn viewport_fingerprint(&self) -> &str {
      &self.fingerprint
    }
    fn parser_notes(&self) -> &[ParserDiagnostic] {
      &self.parser_notes_vec
    }
    fn has_evidence(&self) -> bool {
      self.evidence_present
    }
  }

  #[derive(Debug, PartialEq, Eq)]
  struct FakeSection {
    id: String,
    label: String,
    items: Vec<FakeItem>,
  }

  #[derive(Debug, PartialEq, Eq)]
  struct FakeItem {
    id: String,
    label: String,
  }

  struct FakePolicy;

  impl ReconstructionPolicy for FakePolicy {
    type Candidate = FakeCandidate;
    type SectionKey = String;
    type SectionProjection = FakeSection;
    type ItemProjection = FakeItem;
    type Observation = FakeReconstructObservation;

    fn candidates<'a>(
      &self,
      observation: &'a Self::Observation,
    ) -> Box<dyn Iterator<Item = &'a Self::Candidate> + 'a> {
      Box::new(observation.candidates.iter())
    }

    fn classify(&self, candidate: &Self::Candidate) -> CandidateRole<Self::SectionKey> {
      match candidate.kind {
        FakeCandidateKind::Header => CandidateRole::Header {
          section_key: candidate.section_key.clone().unwrap_or_default(),
        },
        FakeCandidateKind::Item => CandidateRole::Item {
          dedupe_key: candidate.label.to_lowercase(),
        },
        FakeCandidateKind::Unknown => CandidateRole::Unknown,
      }
    }

    fn build_section(
      &self,
      _observation: &Self::Observation,
      candidate: &Self::Candidate,
    ) -> (ViewNodeRecord, Self::SectionProjection) {
      let id = format!("section.{}", candidate.id);
      let node = ViewNodeRecord {
        id: id.clone(),
        kind: ViewNodeKind::Section,
        label: Some(candidate.label.clone()),
        anchors: vec![ViewAnchor {
          id: format!("anchor.{id}"),
          label: candidate.label.clone(),
          strength: AnchorStrength::Medium,
          bounds: ViewBounds::default(),
          evidence_ids: Vec::new(),
        }],
        ..Default::default()
      };
      let section = FakeSection {
        id,
        label: candidate.label.clone(),
        items: Vec::new(),
      };
      (node, section)
    }

    fn build_unassigned_section(&self) -> (ViewNodeRecord, Self::SectionProjection) {
      let node = ViewNodeRecord {
        id: "section.unassigned".into(),
        kind: ViewNodeKind::Section,
        ..Default::default()
      };
      let section = FakeSection {
        id: "section.unassigned".into(),
        label: "unassigned".into(),
        items: Vec::new(),
      };
      (node, section)
    }

    fn build_item(
      &self,
      _observation: &Self::Observation,
      candidate: &Self::Candidate,
      _section: &Self::SectionProjection,
    ) -> (ViewNodeRecord, Self::ItemProjection) {
      let id = format!("item.{}", candidate.id);
      let anchor_id = format!("anchor.{id}");
      let node = ViewNodeRecord {
        id: id.clone(),
        kind: ViewNodeKind::Item,
        label: Some(candidate.label.clone()),
        anchors: vec![ViewAnchor {
          id: anchor_id,
          label: candidate.label.clone(),
          strength: AnchorStrength::Strong,
          bounds: ViewBounds::default(),
          evidence_ids: Vec::new(),
        }],
        landmarks: vec![ViewLandmark {
          id: format!("landmark.{id}"),
          label: candidate.label.clone(),
          landmark_use: LandmarkUse::AnchorReacquire,
          bounds: ViewBounds::default(),
          evidence_ids: Vec::new(),
        }],
        ..Default::default()
      };
      let item = FakeItem {
        id,
        label: candidate.label.clone(),
      };
      (node, item)
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
      _bounds: ViewBounds,
      _boundary: ScrollBoundarySummary,
      section_children: Vec<ViewNodeRecord>,
    ) -> ViewNodeRecord {
      ViewNodeRecord {
        id: "root".into(),
        kind: ViewNodeKind::Collection,
        children: section_children,
        ..Default::default()
      }
    }

    fn emit_dedup_diagnostic(
      &self,
      candidate: &Self::Candidate,
      section: &Self::SectionProjection,
    ) -> ParserDiagnostic {
      ParserDiagnostic {
        code: "deduplicated_item".into(),
        message: format!("dup {} under {}", candidate.label, section.label),
        node_id: Some(candidate.id.clone()),
      }
    }
  }

  fn header(id: &str, label: &str, section_key: &str) -> FakeCandidate {
    FakeCandidate {
      id: id.into(),
      label: label.into(),
      kind: FakeCandidateKind::Header,
      section_key: Some(section_key.into()),
    }
  }

  fn item(id: &str, label: &str) -> FakeCandidate {
    FakeCandidate {
      id: id.into(),
      label: label.into(),
      kind: FakeCandidateKind::Item,
      section_key: None,
    }
  }

  fn obs(fingerprint: &str, candidates: Vec<FakeCandidate>) -> FakeReconstructObservation {
    FakeReconstructObservation {
      fingerprint: fingerprint.into(),
      candidates,
      parser_notes_vec: Vec::new(),
      evidence_present: true,
    }
  }

  #[test]
  fn reconstruct_dedups_sections_by_key_across_observations() {
    // Same section header label appears in obs 0 and obs 1; only one section
    // should be produced.
    let observations = vec![
      obs(
        "fp-0",
        vec![
          header("h0", "My Playlists", "my_playlists"),
          item("i0", "Liked Songs"),
        ],
      ),
      obs(
        "fp-1",
        vec![
          header("h1", "My Playlists", "my_playlists"),
          item("i1", "Daily Mix 1"),
        ],
      ),
    ];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    assert_eq!(
      out.sections.len(),
      1,
      "second header with same key must merge"
    );
    assert_eq!(out.sections[0].items.len(), 2);
    assert_eq!(out.sections[0].items[0].label, "Liked Songs");
    assert_eq!(out.sections[0].items[1].label, "Daily Mix 1");
  }

  #[test]
  fn reconstruct_dedups_items_within_section_and_emits_diagnostic() {
    // Same item label appears twice under the same section; the second
    // attempt emits a diagnostic and is not appended.
    let observations = vec![obs(
      "fp",
      vec![
        header("h", "Recommended", "recommended"),
        item("i0", "Discover Weekly"),
        item("i1", "Discover Weekly"),
      ],
    )];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    assert_eq!(out.sections.len(), 1);
    assert_eq!(out.sections[0].items.len(), 1);
    let dedup = out
      .diagnostics
      .iter()
      .find(|d| d.code == "deduplicated_item")
      .expect("dedup diagnostic must fire");
    assert!(dedup.message.contains("Discover Weekly"));
    assert_eq!(dedup.node_id.as_deref(), Some("i1"));
  }

  #[test]
  fn reconstruct_builds_unassigned_section_for_items_before_any_header() {
    // Item appears before any header; framework creates an unassigned
    // section lazily.
    let observations = vec![obs("fp", vec![item("i0", "Orphan Track")])];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    assert_eq!(out.sections.len(), 1);
    assert_eq!(out.sections[0].id, "section.unassigned");
    assert_eq!(out.sections[0].items.len(), 1);
  }

  #[test]
  fn reconstruct_raises_no_reliable_candidates_when_evidence_but_no_sections() {
    // Observation has evidence but the only candidates are Unknown — no
    // sections, framework raises the parser_no_reliable_candidates note.
    let observations = vec![obs(
      "fp",
      vec![FakeCandidate {
        id: "u".into(),
        label: "".into(),
        kind: FakeCandidateKind::Unknown,
        section_key: None,
      }],
    )];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    assert_eq!(out.sections.len(), 0);
    assert!(
      out
        .diagnostics
        .iter()
        .any(|d| d.code == "parser_no_reliable_candidates"),
      "evidence + no sections must raise parser_no_reliable_candidates"
    );
  }

  #[test]
  fn reconstruct_does_not_raise_no_reliable_candidates_when_no_evidence() {
    // No evidence at all → silent (don't double-complain).
    let mut o = obs("fp", vec![]);
    o.evidence_present = false;
    let out = reconstruct(&FakePolicy, &[o], ViewBounds::default());
    assert!(
      out
        .diagnostics
        .iter()
        .all(|d| d.code != "parser_no_reliable_candidates")
    );
  }

  #[test]
  fn reconstruct_forwards_observation_parser_notes_into_diagnostics() {
    let mut o = obs("fp", vec![]);
    o.parser_notes_vec = vec![ParserDiagnostic {
      code: "preview".into(),
      message: "from observation".into(),
      node_id: None,
    }];
    o.evidence_present = false;
    let out = reconstruct(&FakePolicy, &[o], ViewBounds::default());
    assert!(out.diagnostics.iter().any(|d| d.code == "preview"));
  }

  #[test]
  fn reconstruct_collects_anchors_and_landmarks_in_preorder() {
    // Section node has an anchor; each item has an anchor + landmark.
    // Pre-order walk: root has no anchor, section.anchor first, then per-item
    // anchors in declaration order.
    let observations = vec![obs(
      "fp",
      vec![header("h", "S", "s"), item("i0", "A"), item("i1", "B")],
    )];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    let anchor_ids: Vec<&str> = out.anchor_index.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
      anchor_ids,
      vec!["anchor.section.h", "anchor.item.i0", "anchor.item.i1"]
    );
    let landmark_ids: Vec<&str> = out.landmark_index.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(landmark_ids, vec!["landmark.item.i0", "landmark.item.i1"]);
  }

  #[test]
  fn reconstruct_boundary_summary_reports_likely_on_adjacent_repeat() {
    // Same fingerprint twice in a row → boundary.bottom = Likely (inherited
    // from boundary_summary_from_observations).
    let observations = vec![obs("fp-a", vec![]), obs("fp-a", vec![])];
    let out = reconstruct(&FakePolicy, &observations, ViewBounds::default());
    assert_eq!(out.boundary.bottom, BoundaryConfidence::Likely);
  }
}
