use crate::*;

pub(crate) fn scan_sidebar_with_observer(
  reader: &mut impl SidebarScanReader,
  options: ScanOptions,
  category: PlaylistCategory,
  scroll_amount: f64,
  scroll_settle_ms: u64,
) -> PlaylistSidebarScan {
  let top_seek = scroll_to_top_by_motion(reader, top_seek_scroll_budget(options.max_scrolls));
  reader.reset_collection_phase();
  let loop_outcome = scan_with_collection_policy_impl(reader, options, category, None);
  finish_sidebar_scan(top_seek, loop_outcome, scroll_amount, scroll_settle_ms)
}

pub(crate) const QUERY_SCAN_SKIPPED_TOP_REWIND_LIMIT: &str = "query_scan_skipped_top_rewind";
pub(crate) const QUERY_SCAN_TOP_REWIND_APPLIED_LIMIT: &str = "query_scan_top_rewind_applied";

pub(crate) fn scan_sidebar_with_observer_until_query(
  reader: &mut impl SidebarScanReader,
  options: ScanOptions,
  category: PlaylistCategory,
  scroll_amount: f64,
  scroll_settle_ms: u64,
  query: &str,
) -> PlaylistSidebarScan {
  let normalized_query = normalize_identity(query);
  let query_already_visible =
    reader.read_probe().ok().is_some_and(|viewport| viewport_satisfies_query(&viewport, normalized_query.as_str()));

  let top_seek = if query_already_visible {
    // NOTICE(a6c-10b): query-target already in viewport; top rewind would scroll
    // selected numeric labels away before collection starts.
    TopSeekOutcome::default()
  } else {
    scroll_to_top_by_motion(reader, top_seek_scroll_budget(options.max_scrolls))
  };
  reader.reset_collection_phase();
  let mut loop_outcome = scan_with_collection_policy_impl(reader, options, category, Some(normalized_query.as_str()));
  loop_outcome.known_limits.push(if query_already_visible {
    QUERY_SCAN_SKIPPED_TOP_REWIND_LIMIT.to_string()
  } else {
    QUERY_SCAN_TOP_REWIND_APPLIED_LIMIT.to_string()
  });
  finish_sidebar_scan(top_seek, loop_outcome, scroll_amount, scroll_settle_ms)
}

fn finish_sidebar_scan(
  top_seek: TopSeekOutcome,
  loop_outcome: CollectionLoopOutcome,
  scroll_amount: f64,
  scroll_settle_ms: u64,
) -> PlaylistSidebarScan {
  crate::telemetry::emit_sidebar_scan_events(&loop_outcome.viewports, scroll_amount, scroll_settle_ms, loop_outcome.stop_reason);

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
    loop_outcome.viewports,
  );
  scan.diagnostics.extend(top_seek.diagnostics);
  scan.diagnostics.extend(loop_outcome.diagnostics);
  scan.known_limits.extend(top_seek.known_limits);
  scan.known_limits.extend(loop_outcome.known_limits);
  scan.stop_reason = loop_outcome.stop_reason;
  if top_seek.boundary == BoundaryConfidence::Likely {
    apply_top_boundary(&mut scan, top_seek.boundary);
  }
  if matches!(
    loop_outcome.stop_reason,
    Some(
      SidebarScanStopReason::ScrollNoMotionAfterInput
        | SidebarScanStopReason::ScrollNoMotionWithAxScrollbarBottom
        | SidebarScanStopReason::ScrollNoNewSemanticCandidatesAfterInput
        | SidebarScanStopReason::ScrollNoNewSemanticCandidatesWithAxScrollbarBottom
    )
  ) {
    apply_bottom_boundary(&mut scan, BoundaryConfidence::Likely);
  }
  scan
}

pub(crate) fn heuristic_stop_reason_with_ax_corroboration(
  base_reason: SidebarScanStopReason,
  ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>,
) -> Option<SidebarScanStopReason> {
  match (base_reason, ax_scrollbar_boundary) {
    (SidebarScanStopReason::ScrollNoNewSemanticCandidatesAfterInput, Some(SidebarScrollbarBoundary::Bottom)) => {
      Some(SidebarScanStopReason::ScrollNoNewSemanticCandidatesWithAxScrollbarBottom)
    }
    (SidebarScanStopReason::ScrollNoMotionAfterInput, Some(SidebarScrollbarBoundary::Bottom)) => {
      Some(SidebarScanStopReason::ScrollNoMotionWithAxScrollbarBottom)
    }
    (
      SidebarScanStopReason::ScrollNoNewSemanticCandidatesAfterInput | SidebarScanStopReason::ScrollNoMotionAfterInput,
      Some(SidebarScrollbarBoundary::Top | SidebarScrollbarBoundary::Interior),
    ) => None,
    _ => Some(base_reason),
  }
}

pub(crate) fn repeated_fingerprint_stop_reason(ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>) -> SidebarScanStopReason {
  if ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom) {
    SidebarScanStopReason::RepeatedViewportFingerprintWithAxScrollbarBottom
  } else {
    SidebarScanStopReason::RepeatedViewportFingerprint
  }
}

pub(crate) fn motion_stop_threshold(ax_scrollbar_boundary: Option<SidebarScrollbarBoundary>) -> usize {
  if ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom) {
    1
  } else {
    2
  }
}

pub(crate) fn top_seek_scroll_budget(collection_max_scrolls: usize) -> usize {
  collection_max_scrolls.min(LIVE_TOP_SEEK_MAX_SCROLL_INPUTS)
}

pub(crate) fn sidebar_rescan_target_seek_budget(max_scrolls: usize, target_viewport_index: usize) -> usize {
  max_scrolls.max(target_viewport_index).saturating_add(4)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarTargetSeekStep {
  Found(usize),
  ScrollNext(usize),
}

pub(crate) fn next_sidebar_target_seek_step(attempt: usize, max_attempts: usize, found: bool) -> Option<SidebarTargetSeekStep> {
  if found {
    return Some(SidebarTargetSeekStep::Found(attempt));
  }
  if attempt + 1 >= max_attempts {
    return None;
  }
  Some(SidebarTargetSeekStep::ScrollNext(attempt))
}

pub(crate) trait SidebarScanReader: ViewportReader<Viewport = SidebarViewport> {
  fn reset_collection_phase(&mut self) {}

  fn scroll_seek_batch_size(&self) -> usize {
    1
  }

  fn scroll_seek_up(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_up()
  }

  fn observe_scroll_seek(&mut self, viewport_index: usize) -> Result<SidebarViewport, ParserDiagnostic> {
    self.read_viewport(viewport_index)
  }

  fn scroll_down_for_query_recovery(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_down()
  }
}

pub(crate) fn scroll_to_top_by_motion(reader: &mut impl SidebarScanReader, max_scrolls: usize) -> TopSeekOutcome {
  let mut outcome = TopSeekOutcome::default();
  reader.reset_collection_phase();

  if let Err(diagnostic) = reader.observe_scroll_seek(0) {
    outcome.diagnostics.push(diagnostic);
    return outcome;
  }

  let mut scrolls = 0usize;
  let mut sample_index = 1usize;
  while scrolls < max_scrolls {
    let batch = reader.scroll_seek_batch_size().max(1).min(max_scrolls - scrolls);
    for _ in 0..batch {
      if let Err(diagnostic) = reader.scroll_seek_up() {
        outcome.diagnostics.push(diagnostic);
        return outcome;
      }
      scrolls += 1;
    }

    let viewport = match reader.observe_scroll_seek(sample_index) {
      Ok(viewport) => viewport,
      Err(diagnostic) => {
        outcome.diagnostics.push(diagnostic);
        return outcome;
      }
    };
    sample_index += 1;

    if successful_scroll_delivery_path(viewport.incoming_scroll_delivery_path.as_deref())
      && viewport.scroll_motion.as_ref().is_some_and(|motion| motion.no_motion)
    {
      outcome.boundary = BoundaryConfidence::Likely;
      return outcome;
    }
  }

  outcome.known_limits.push(format!("top seek stopped after max_scrolls={max_scrolls} without repeated sidebar pixels"));
  outcome
}

pub(crate) fn empty_scroll_seek_viewport(viewport_index: usize, viewport_bounds: ViewBounds) -> SidebarViewport {
  SidebarViewport {
    viewport_index,
    viewport: ViewViewportRecord {
      page_index: viewport_index,
      bounds: viewport_bounds,
      axis: ViewAxis::Vertical,
      scroll_offset: None,
    },
    incoming_scroll_delivery_path: None,
    scroll_motion: None,
    viewport_fingerprint: String::new(),
    evidence_nodes: Vec::new(),
    candidates: Vec::new(),
    parser_notes: Vec::new(),
    ax_scrollbar_boundary: None,
  }
}

pub(crate) struct CollectionLoopOutcome {
  viewports: Vec<SidebarViewport>,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
  stop_reason: Option<SidebarScanStopReason>,
}

fn scan_with_collection_policy_impl(
  reader: &mut impl SidebarScanReader,
  options: ScanOptions,
  category: PlaylistCategory,
  normalized_query: Option<&str>,
) -> CollectionLoopOutcome {
  let mut policy = CollectionPolicy::new(category);
  let mut viewports = Vec::new();
  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();
  let mut previous_fingerprint: Option<String> = None;
  let mut seen_semantic_candidates = HashSet::new();
  let mut consecutive_no_new_semantic_candidates_after_scroll = 0usize;
  let mut consecutive_no_motion_after_scroll = 0usize;
  let mut observed_scroll_motion_after_successful_input = false;
  let mut query_seen = normalized_query.is_none_or(str::is_empty);
  let mut scrolls = 0;
  let mut stop_reason = None;

  loop {
    let viewport_index = viewports.len();
    let viewport = match reader.read_viewport(viewport_index) {
      Ok(viewport) => viewport,
      Err(diagnostic) => {
        diagnostics.push(diagnostic);
        break;
      }
    };
    let fingerprint = viewport.viewport_fingerprint().to_string();
    let repeated_fingerprint = previous_fingerprint.as_deref().is_some_and(|prev| prev == fingerprint.as_str());
    previous_fingerprint = Some(fingerprint);
    let ax_scrollbar_boundary = viewport.ax_scrollbar_boundary;
    let viewport = policy.apply(viewport);
    if !query_seen {
      if let Some(query) = normalized_query {
        query_seen = viewport_satisfies_query(&viewport, query);
      }
    }
    let introduced_new_semantic_candidates = record_page_semantic_candidates(&viewport, &mut seen_semantic_candidates);
    let reached_stop_landmark = policy.reached_stop_landmark();
    let started = policy.start_seen();
    let successful_scroll_input = successful_scroll_delivery_path(viewport.incoming_scroll_delivery_path.as_deref());
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
      if let Some(motion) = viewport.scroll_motion.as_ref() {
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
    viewports.push(viewport);

    if reached_stop_landmark {
      stop_reason = Some(SidebarScanStopReason::ReachedStopLandmark);
      break;
    }

    // NOTICE(netease-scroll-stop): exact viewport fingerprints are kept as a
    // backward-compatible loop-boundary signal, but they are no longer the only
    // scroll stop detector. Motion evidence covers the real NetEase case where
    // OCR text drifts enough that exact fingerprints do not repeat at bottom.
    if repeated_fingerprint && (query_seen || ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom)) {
      stop_reason = Some(repeated_fingerprint_stop_reason(ax_scrollbar_boundary));
      break;
    }

    // NOTICE(netease-scroll-semantic-boundary): repeated "no new semantic
    // candidates after scroll" is a stronger completion signal than crop
    // motion alone because it tracks the actual playlist/sidebar IR that this
    // crate exports. It remains heuristic until a future slice corroborates it
    // with scroll-bar, AX scroll-state, or provider-reported bounds.
    if consecutive_no_new_semantic_candidates_after_scroll >= 2
      && (query_seen || ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom))
    {
      if let Some(reason) =
        heuristic_stop_reason_with_ax_corroboration(SidebarScanStopReason::ScrollNoNewSemanticCandidatesAfterInput, ax_scrollbar_boundary)
      {
        stop_reason = Some(reason);
        break;
      }
    }

    // NOTICE(netease-scroll-motion-boundary): identical sidebar pixels only
    // count as bottom evidence after a successful scroll delivery path and at
    // least one prior post-scroll motion viewport. This prevents launch
    // state, failed/noop input, or already-stuck captures from being promoted
    // into a false bottom boundary.
    if consecutive_no_motion_after_scroll >= motion_stop_threshold(ax_scrollbar_boundary)
      && (query_seen || ax_scrollbar_boundary == Some(SidebarScrollbarBoundary::Bottom))
    {
      if let Some(reason) =
        heuristic_stop_reason_with_ax_corroboration(SidebarScanStopReason::ScrollNoMotionAfterInput, ax_scrollbar_boundary)
      {
        stop_reason = Some(reason);
        break;
      }
    }

    if scrolls >= options.max_scrolls {
      known_limits.push(format!("stopped after max_scrolls={}", options.max_scrolls));
      break;
    }

    let use_query_recovery_scroll =
      !query_seen && (consecutive_no_motion_after_scroll > 0 || consecutive_no_new_semantic_candidates_after_scroll >= 2);
    let scroll_result = if use_query_recovery_scroll {
      reader.scroll_down_for_query_recovery()
    } else {
      reader.scroll_down()
    };
    if let Err(diagnostic) = scroll_result {
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
    viewports,
    diagnostics,
    known_limits,
    stop_reason,
  }
}

fn viewport_satisfies_query(viewport: &SidebarViewport, query: &str) -> bool {
  let labels: Vec<&str> = viewport
    .candidates
    .iter()
    .filter(|candidate| candidate.kind == SidebarCandidateKind::PlaylistItem)
    .filter_map(|candidate| candidate.label.as_deref())
    .collect();
  crate::views::query_match::playlist_query_resolution_is_unique_exact(crate::views::query_match::resolve_playlist_query_from_labels(
    &labels, query,
  ))
}

pub(crate) fn successful_scroll_delivery_path(path: Option<&str>) -> bool {
  !matches!(path, None | Some("noop" | "unsupported"))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SemanticCandidateKey {
  kind: SidebarCandidateKind,
  label: String,
  section_hint: Option<SidebarSectionKind>,
}

pub(crate) fn record_page_semantic_candidates(viewport: &SidebarViewport, seen: &mut HashSet<SemanticCandidateKey>) -> bool {
  let mut introduced_new = false;
  let mut current_section = None;

  for candidate in &viewport.candidates {
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

pub(crate) struct CollectionPolicy {
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

  fn apply(&mut self, mut viewport: SidebarViewport) -> SidebarViewport {
    if self.category == PlaylistCategory::All {
      return viewport;
    }

    let mut accepted = Vec::new();
    for candidate in viewport.candidates {
      if self.stopped {
        break;
      }

      let section_kind = candidate.label.as_deref().map(SidebarSectionKind::from_label).unwrap_or(SidebarSectionKind::Unknown);

      if candidate.kind == SidebarCandidateKind::SectionHeader {
        match self.category {
          PlaylistCategory::Created if section_kind == SidebarSectionKind::MyPlaylists => {
            self.started = true;
            accepted.push(candidate);
          }
          PlaylistCategory::Created if self.started && section_kind == SidebarSectionKind::FavoritePlaylists => {
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
    viewport.candidates = accepted;
    viewport
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
      PlaylistCategory::Created => Some("category created scan ended without seeing created playlist landmark".to_string()),
      PlaylistCategory::Favorite => Some("category favorite scan ended without seeing favorite playlist landmark".to_string()),
    }
  }
}

pub(crate) fn apply_top_boundary(scan: &mut PlaylistSidebarScan, top: BoundaryConfidence) {
  scan.boundary.top = top;
  if let Some(scrollable) = scan.reconstruction.root.scrollable.as_mut() {
    scrollable.boundary.top = top;
  }
}

pub(crate) fn apply_bottom_boundary(scan: &mut PlaylistSidebarScan, bottom: BoundaryConfidence) {
  scan.boundary.bottom = bottom;
  if let Some(scrollable) = scan.reconstruction.root.scrollable.as_mut() {
    scrollable.boundary.bottom = bottom;
  }
}
