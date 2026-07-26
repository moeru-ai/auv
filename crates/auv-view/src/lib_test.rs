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
      source: ViewEvidenceSource::OcrText,
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
fn confidence_owns_names_short_codes_and_ordering() {
  assert_eq!(Confidence::Low.to_string(), "low");
  assert_eq!(Confidence::Medium.short_code(), "M");
  assert_eq!(Confidence::from_short_code("H"), Some(Confidence::High));
  assert_eq!(Confidence::from_short_code("unknown"), None);
  assert!(Confidence::High > Confidence::Medium);
  assert!(Confidence::Medium > Confidence::Low);
}

#[test]
fn viewport_contains_center_uses_geometric_center() {
  let viewport = ViewBounds::new(0.0, 0.0, 100.0, 100.0);
  // Center (50,50) is inside
  assert!(viewport_contains_center(viewport, ViewBounds::new(40.0, 40.0, 20.0, 20.0)));
  // Center (150, 50) is outside despite bounds overlapping
  assert!(!viewport_contains_center(viewport, ViewBounds::new(100.0, 40.0, 100.0, 20.0)));
  // Exact boundary inclusive
  assert!(viewport_contains_center(viewport, ViewBounds::new(90.0, 90.0, 20.0, 20.0)));
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
  assert_eq!(out.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["root", "child-a", "child-b", "grandchild"]);
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
  assert_eq!(out.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), vec!["root", "child"]);
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

  fn observe(&mut self, _observation_index: usize) -> Result<Self::Observation, ParserDiagnostic> {
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
  assert_eq!(outcome.observations.iter().map(|o| o.viewport_fingerprint()).collect::<Vec<_>>(), vec!["a", "b", "b"]);
  assert!(outcome.diagnostics.is_empty());
  assert!(outcome.known_limits.is_empty(), "boundary hit, no cap fired");
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
    assert_eq!(img.get_pixel(x as u32, 2), &color, "x={x} should be painted");
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

  fn candidates<'a>(&self, observation: &'a Self::Observation) -> impl Iterator<Item = &'a Self::Candidate> + 'a
  where
    Self::Candidate: 'a,
  {
    observation.candidates.iter()
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

  fn build_section(&self, _observation: &Self::Observation, candidate: &Self::Candidate) -> (ViewNodeRecord, Self::SectionProjection) {
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

  fn append_item_to_section_projection(&self, section: &mut Self::SectionProjection, item: Self::ItemProjection) {
    section.items.push(item);
  }

  fn build_root(&self, _bounds: ViewBounds, _boundary: ScrollBoundarySummary, section_children: Vec<ViewNodeRecord>) -> ViewNodeRecord {
    ViewNodeRecord {
      id: "root".into(),
      kind: ViewNodeKind::Collection,
      children: section_children,
      ..Default::default()
    }
  }

  fn emit_dedup_diagnostic(&self, candidate: &Self::Candidate, section: &Self::SectionProjection) -> ParserDiagnostic {
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
  assert_eq!(out.sections.len(), 1, "second header with same key must merge");
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
  let dedup = out.diagnostics.iter().find(|d| d.code == "deduplicated_item").expect("dedup diagnostic must fire");
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
    out.diagnostics.iter().any(|d| d.code == "parser_no_reliable_candidates"),
    "evidence + no sections must raise parser_no_reliable_candidates"
  );
}

#[test]
fn reconstruct_does_not_raise_no_reliable_candidates_when_no_evidence() {
  // No evidence at all → silent (don't double-complain).
  let mut o = obs("fp", vec![]);
  o.evidence_present = false;
  let out = reconstruct(&FakePolicy, &[o], ViewBounds::default());
  assert!(out.diagnostics.iter().all(|d| d.code != "parser_no_reliable_candidates"));
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
  assert_eq!(anchor_ids, vec!["anchor.section.h", "anchor.item.i0", "anchor.item.i1"]);
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
