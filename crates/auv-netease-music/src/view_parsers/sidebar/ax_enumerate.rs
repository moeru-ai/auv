//! Reconstruct a NetEase playlist sidebar projection from a captured AX tree.
//!
//! This is the AX-sourced alternative to the OCR sidebar scan. With
//! `AXEnhancedUserInterface` enabled (see `auv-driver-macos`
//! `set_app_enhanced_user_interface`), the sidebar exposes each playlist as an
//! `AXStaticText` carrying its exact label, so enumeration needs no OCR and no
//! fuzzy matching. The output is the same `PlaylistSidebarProjection` the OCR
//! path produces, so `collect_matches`, the JSON output, and
//! `playlist select|play` consume it unchanged.
//!
//! NOTICE(netease-ax-enumerate): positional AX paths recycle under
//! virtualization, so this works per-capture and callers must dedup by label
//! across captures. This v0 reconstructs a single captured tree; scroll
//! accumulation, the section-total completion oracle, and live wiring into the
//! scan land in follow-up slices (see
//! docs/ai/references/2026-06-30-netease-macos-ax-playlist-design.md).
//!
//! TODO(netease-ax-enumerate-wiring): exercised by unit tests and the
//! `netease_ax_poc` example; wiring into the production scan/CLI (which edits the
//! OCR scan loop in lib.rs) is a follow-up slice that coordinates with the
//! scroll-ax-corroboration lane.

use auv_driver_macos::types::ObservedAxNode;
use auv_view::{Confidence, normalize_identity};

use crate::views::sidebar::{
  PlaylistSidebarItem, PlaylistSidebarProjection, SidebarSection, SidebarSectionKind,
};

const AX_STATIC_TEXT: &str = "AXStaticText";

/// A detected playlist-collection section header, e.g. `创建的歌单 201`.
struct SectionHeader {
  kind: SidebarSectionKind,
  label: String,
  /// Dotted path components of the sibling level the header's section group
  /// sits at. The section's item rows live in a following sibling group at a
  /// higher index, before the next header at the same level.
  sibling_prefix: Vec<String>,
  index: usize,
  /// The header node's y bound. The header scrolls together with its rows, so
  /// `row.y - header_y` is a scroll-invariant per-row position.
  header_y: i64,
}

fn node_label(node: &ObservedAxNode) -> &str {
  if node.value.is_empty() {
    &node.title
  } else {
    &node.value
  }
}

/// Reconstruct a playlist sidebar projection from a captured AX tree.
///
/// Requires the AX enhanced-UI flag to have been enabled before capture;
/// otherwise the tree is the bare window shell and no sections are found.
pub fn sidebar_projection_from_ax_nodes(nodes: &[ObservedAxNode]) -> PlaylistSidebarProjection {
  let headers = detect_headers(nodes);

  let sections = headers
    .iter()
    .enumerate()
    .map(|(position, header)| {
      // The section ends at the next header sharing its sibling level.
      let next_index = headers[position + 1..]
        .iter()
        .find(|next| next.sibling_prefix == header.sibling_prefix && next.index > header.index)
        .map(|next| next.index)
        .unwrap_or(usize::MAX);
      SidebarSection {
        id: format!("ax.{}.{}", header.kind.domain_kind(), header.index),
        kind: header.kind,
        label: Some(header.label.clone()),
        items: collect_section_items(nodes, header, next_index),
      }
    })
    .collect();

  PlaylistSidebarProjection { sections }
}

/// Find playlist-collection headers (`创建的歌单` / `收藏的歌单`) in document order.
fn detect_headers(nodes: &[ObservedAxNode]) -> Vec<SectionHeader> {
  let mut headers = Vec::new();
  for node in nodes {
    if node.role != AX_STATIC_TEXT {
      continue;
    }
    let label = node_label(node);
    let kind = SidebarSectionKind::from_label(label);
    if !kind.is_playlist_collection() {
      continue;
    }
    // Header text sits at `<sibling_prefix>.<index>.<leaf>`; the section's
    // sibling group is `<sibling_prefix>.<index>`.
    let comps: Vec<&str> = node.path.split('.').collect();
    if comps.len() < 2 {
      continue;
    }
    let Ok(index) = comps[comps.len() - 2].parse::<usize>() else {
      continue;
    };
    headers.push(SectionHeader {
      kind,
      label: label.to_string(),
      sibling_prefix: comps[..comps.len() - 2]
        .iter()
        .map(|component| component.to_string())
        .collect(),
      index,
      header_y: node.bounds.y,
    });
  }
  headers
}

/// Return the label nodes of a section's list container, in document order.
///
/// NOTICE: the list container is the FIRST label-bearing sibling group after the
/// header, and it must hold >= 2 rows. Taking the *first* such group (not any
/// later one) keeps a collapsed section from reaching past the sidebar footer
/// into the main content panel (a real capture put a settings list there, which
/// an "any group with >= 2 rows" rule wrongly absorbed); the >= 2 guard rejects a
/// lone footer/user label.
/// TODO(netease-ax-enumerate-shape): a structural per-row shape check would also
/// admit a genuine one-playlist section and tolerate a stray label between a
/// header and its list; deferred until that case appears.
fn collect_section_container<'a>(
  nodes: &'a [ObservedAxNode],
  header: &SectionHeader,
  next_index: usize,
) -> Vec<&'a ObservedAxNode> {
  let prefix_len = header.sibling_prefix.len();

  // Group candidate label nodes by their sibling-level index, in document order.
  let mut groups: Vec<(usize, Vec<&ObservedAxNode>)> = Vec::new();
  for node in nodes {
    if node.role != AX_STATIC_TEXT {
      continue;
    }
    let label = node_label(node);
    // Skip empties and any recognized header/nav label.
    if label.is_empty() || SidebarSectionKind::from_label(label).is_known() {
      continue;
    }
    let comps: Vec<&str> = node.path.split('.').collect();
    if comps.len() <= prefix_len + 1 || comps[..prefix_len] != header.sibling_prefix[..] {
      continue;
    }
    let Ok(group) = comps[prefix_len].parse::<usize>() else {
      continue;
    };
    if group <= header.index || group >= next_index {
      continue;
    }
    match groups.iter_mut().find(|(existing, _)| *existing == group) {
      Some((_, rows)) => rows.push(node),
      None => groups.push((group, vec![node])),
    }
  }

  match groups.into_iter().min_by_key(|(group, _)| *group) {
    Some((_, rows)) if rows.len() >= 2 => rows,
    _ => Vec::new(),
  }
}

/// Collect a section's playlist items, deduped by normalized label.
///
/// Label dedup is intentional here because this projection feeds keyword match /
/// select. For exhaustive enumeration that must keep same-named playlists, use
/// [`created_rows_with_offset`] instead.
fn collect_section_items(
  nodes: &[ObservedAxNode],
  header: &SectionHeader,
  next_index: usize,
) -> Vec<PlaylistSidebarItem> {
  let mut seen = std::collections::HashSet::new();
  let mut items = Vec::new();
  for node in collect_section_container(nodes, header, next_index) {
    let label = node_label(node);
    let normalized = normalize_identity(label);
    if normalized.is_empty() || !seen.insert(normalized) {
      continue;
    }
    items.push(PlaylistSidebarItem {
      id: format!("ax.{}.{}", header.kind.domain_kind(), items.len()),
      label: label.to_string(),
      section_hint: Some(header.kind),
      confidence: Confidence::High,
      candidate_id: None,
      anchor_id: None,
    });
  }
  items
}

/// Created-playlist rows as `(offset, label)`, where `offset = row.y - header.y`.
///
/// The created header and its rows live in the same scroll container, so this
/// offset is **scroll-invariant** — it is the row's fixed position in the list.
/// That makes it a stable per-row id that survives same-named playlists (their
/// labels are equal, and often truncated, but their offsets differ), where dedup
/// or sequence-stitching by label would drop one. Callers enumerating across
/// scroll captures dedup by clustering offsets (rows are tens of px apart;
/// re-renders of a row land at the same offset). Rows are returned in document
/// (top-to-bottom) order.
pub fn created_rows_with_offset(nodes: &[ObservedAxNode]) -> Vec<(i64, String)> {
  let headers = detect_headers(nodes);
  let Some((position, header)) = headers
    .iter()
    .enumerate()
    .find(|(_, header)| header.kind == SidebarSectionKind::MyPlaylists)
  else {
    return Vec::new();
  };
  let next_index = headers[position + 1..]
    .iter()
    .find(|next| next.sibling_prefix == header.sibling_prefix && next.index > header.index)
    .map(|next| next.index)
    .unwrap_or(usize::MAX);

  collect_section_container(nodes, header, next_index)
    .into_iter()
    .map(|node| {
      (
        node.bounds.y - header.header_y,
        node_label(node).to_string(),
      )
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::output::collect_matches;
  use auv_driver_macos::types::{ObservedAxNode, ObservedRect};

  fn node(path: &str, role: &str, label: &str) -> ObservedAxNode {
    ObservedAxNode {
      depth: path.split('.').count().saturating_sub(1),
      path: path.to_string(),
      role: role.to_string(),
      subrole: String::new(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: String::new(),
      value: label.to_string(),
      focused: false,
      bounds: ObservedRect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
      },
    }
  }

  fn node_at(path: &str, role: &str, label: &str, y: i64) -> ObservedAxNode {
    let mut node = node(path, role, label);
    node.bounds.y = y;
    node
  }

  #[test]
  fn created_rows_with_offset_keeps_same_named_rows_by_position() {
    // Header scrolled above the viewport (negative y); two rows share a
    // (truncated) name at different y -> different offsets -> both kept, where a
    // label dedup or sequence-stitch would collapse them into one.
    let nodes = vec![
      node_at("0.0.0.0.0.0.24.0", AX_STATIC_TEXT, "创建的歌单 20", -8000),
      node_at(
        "0.0.0.0.0.0.27.0.1",
        AX_STATIC_TEXT,
        "TVアニメ「ご注文はうさぎですか",
        400,
      ),
      node_at(
        "0.0.0.0.0.0.27.1.1",
        AX_STATIC_TEXT,
        "TVアニメ「ご注文はうさぎですか",
        446,
      ),
      node_at("0.0.0.0.0.0.27.2.1", AX_STATIC_TEXT, "別の歌単", 492),
    ];

    let rows = created_rows_with_offset(&nodes);

    assert_eq!(
      rows,
      vec![
        (8400, "TVアニメ「ご注文はうさぎですか".to_string()),
        (8446, "TVアニメ「ご注文はうさぎですか".to_string()),
        (8492, "別の歌単".to_string()),
      ]
    );
  }

  // Synthetic fixture mirroring the NetEase 3.x sidebar *structure* (node paths
  // plus the universal UI strings the logic keys on); playlist and user names are
  // placeholders, never real account data. Layout: nav items, a created list
  // under group 27 (with a duplicate label), a collapsed 收藏 section, the footer
  // user row (top-index 34), and the main content panel (top-index 39) showing a
  // settings list. None of the footer/main-content text may be treated as a
  // playlist — the live PoC regressed here, absorbing the settings rows.
  fn sidebar_fixture() -> Vec<ObservedAxNode> {
    vec![
      node("0.0.0.0.0.0.4.0", AX_STATIC_TEXT, "推荐"),
      node("0.0.0.0.0.0.16.1.0", AX_STATIC_TEXT, "我喜欢的音乐"),
      node("0.0.0.0.0.0.24.0", AX_STATIC_TEXT, "创建的歌单 3"),
      node("0.0.0.0.0.0.27.0.0.0", "AXImage", ""),
      node("0.0.0.0.0.0.27.0.1", AX_STATIC_TEXT, "Playlist Alpha"),
      node("0.0.0.0.0.0.27.1.1", AX_STATIC_TEXT, "Playlist Beta"),
      node("0.0.0.0.0.0.27.2.1", AX_STATIC_TEXT, "Playlist Alpha"),
      node("0.0.0.0.0.0.28.0", AX_STATIC_TEXT, "收藏的歌单 5"),
      node("0.0.0.0.0.0.34.1.0", AX_STATIC_TEXT, "Account Holder"),
      node("0.0.0.0.0.0.39.0.0", AX_STATIC_TEXT, "设置"),
      node("0.0.0.0.0.0.39.0.1", AX_STATIC_TEXT, "账号"),
    ]
  }

  #[test]
  fn reconstructs_created_section_dedups_and_excludes_nav_and_footer() {
    let projection = sidebar_projection_from_ax_nodes(&sidebar_fixture());
    assert_eq!(projection.sections.len(), 2);

    let created = &projection.sections[0];
    assert_eq!(created.kind, SidebarSectionKind::MyPlaylists);
    assert_eq!(created.label.as_deref(), Some("创建的歌单 3"));
    let labels: Vec<&str> = created
      .items
      .iter()
      .map(|item| item.label.as_str())
      .collect();
    assert_eq!(labels, vec!["Playlist Alpha", "Playlist Beta"]);

    let favorite = &projection.sections[1];
    assert_eq!(favorite.kind, SidebarSectionKind::FavoritePlaylists);
    assert!(
      favorite.items.is_empty(),
      "a collapsed section must yield no items, not the footer or main content"
    );
    assert!(
      projection
        .sections
        .iter()
        .all(|section| section.items.iter().all(|item| item.label != "设置")),
      "the main content settings list must not be absorbed as playlist items"
    );
  }

  #[test]
  fn projection_is_consumable_by_collect_matches() {
    let projection = sidebar_projection_from_ax_nodes(&sidebar_fixture());

    // Nav and footer are excluded; only the two created playlists remain.
    assert_eq!(collect_matches(&projection, None).len(), 2);

    let hit = collect_matches(&projection, Some("alpha"));
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].label, "Playlist Alpha");
  }

  #[test]
  fn bare_shell_without_enhanced_ui_yields_no_sections() {
    let bare = vec![
      node("0", "AXWindow", "NetEaseMusic"),
      node("0.0", "AXGroup", ""),
    ];
    assert!(sidebar_projection_from_ax_nodes(&bare).sections.is_empty());
  }
}
