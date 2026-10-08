use crate::*;

pub(crate) fn parse_sidebar_viewport(viewport_index: usize, viewport_bounds: ViewBounds, recognition: &TextRecognition) -> SidebarViewport {
  let mut evidence_nodes = recognition
    .regions
    .iter()
    .enumerate()
    .filter(|(_, region)| {
      viewport_contains_center(
        viewport_bounds,
        ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height),
      )
    })
    .map(|(index, region)| ViewEvidenceNode {
      id: format!("obs{viewport_index}.ocr{index}"),
      source: ViewEvidenceSource::OcrText,
      label: Some(region.text.trim().to_string()),
      bounds: Some(ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height)),
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
      .then_with(|| left_bounds.x.partial_cmp(&right_bounds.x).unwrap_or(std::cmp::Ordering::Equal))
  });

  let candidates = evidence_nodes.iter().filter_map(|node| candidate_from_evidence(viewport_index, node)).collect::<Vec<_>>();
  let viewport_fingerprint = viewport_fingerprint(&evidence_nodes);

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
    viewport_fingerprint,
    evidence_nodes,
    candidates,
    parser_notes: Vec::new(),
    ax_scrollbar_boundary: None,
  }
}

pub(crate) fn is_single_ascii_digit_query(query: &str) -> bool {
  query.chars().count() == 1 && query.chars().all(|char| char.is_ascii_digit())
}

pub(crate) fn candidate_from_evidence(viewport_index: usize, node: &ViewEvidenceNode) -> Option<SidebarViewportCandidate> {
  let label = node.label.as_deref()?.trim();
  let bounds = node.bounds?;
  // NOTICE(a6c-8): live Case B has playlist rows named with a single ASCII
  // digit (owner account). Sidebar-body OCR text under 2 chars is normally
  // discarded as noise, but a lone digit at the playlist-row x threshold is
  // real playlist evidence, not noise — narrow the drop to non-numeric or
  // non-playlist-x single chars only.
  let is_single_digit_playlist_row = is_single_ascii_digit_query(label) && bounds.x >= 24.0;
  if label.chars().count() < 2 && !is_single_digit_playlist_row {
    return None;
  }
  let kind = classify_sidebar_text(label, bounds.x);
  if kind == SidebarCandidateKind::Unknown || (kind == SidebarCandidateKind::PlaylistItem && is_cover_art_text(bounds)) {
    return None;
  }

  let source_component = node.id.strip_prefix(&format!("obs{viewport_index}.")).unwrap_or(&node.id);
  Some(SidebarViewportCandidate {
    id: format!("obs{viewport_index}.candidate.{source_component}.{}", slug(label)),
    kind,
    label: Some(label.to_string()),
    bounds: Some(bounds),
    evidence_ids: vec![node.id.clone()],
    confidence: node.confidence,
  })
}

/// Text that OCR read off a playlist cover thumbnail rather than a row label.
///
/// NOTICE(netease-cover-art-text): native-resolution captures make the small
/// print on cover thumbnails legible, so OCR returns it as extra rows. In the
/// 2026-10-07 live sidebar every thumbnail spans x 32..64 pt and labels start
/// at x ~70 pt with heights of 13..18 pt; cover text either ends left of the
/// label column or is under 10 pt tall. Revisit if NetEase changes its row
/// layout.
fn is_cover_art_text(bounds: ViewBounds) -> bool {
  bounds.x + bounds.width <= 66.0 || bounds.height < 10.0
}

pub(crate) fn classify_sidebar_text(label: &str, x: f64) -> SidebarCandidateKind {
  if SidebarSectionKind::from_label(label).is_known() {
    SidebarCandidateKind::SectionHeader
  } else if x >= 24.0 {
    SidebarCandidateKind::PlaylistItem
  } else if matches!(label, "推荐" | "发现音乐" | "播客" | "私人漫游" | "最近播放") {
    SidebarCandidateKind::NavigationItem
  } else {
    SidebarCandidateKind::Unknown
  }
}
#[cfg(test)]
#[path = "parse_test.rs"]
mod tests;
