use serde::Serialize;

use crate::{PlaylistSidebarProjection, PlaylistSidebarScan, SidebarSectionKind};

/// One playlist item surfaced by the listing or keyword filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MatchRef {
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
  pub label: String,
  pub anchor_id: Option<String>,
}

/// Agent-facing JSON envelope for the `playlist` command. Embeds the raw
/// scan artifact (which carries `schema_version` and `ScrollBoundarySummary`)
/// so an agent can distinguish "not found" from "scan not exhaustive".
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistEnvelope<'a> {
  pub command: &'static str,
  pub query: Option<String>,
  pub item_count: usize,
  pub match_count: usize,
  pub matches: Vec<MatchRef>,
  pub scan: &'a PlaylistSidebarScan,
}

/// Collect items whose normalized label contains the normalized keyword.
/// `keyword == None` returns every item (full listing).
pub fn collect_matches(
  projection: &PlaylistSidebarProjection,
  keyword: Option<&str>,
) -> Vec<MatchRef> {
  let needle = keyword.map(crate::normalize_identity);
  let mut out = Vec::new();
  for section in &projection.sections {
    for item in &section.items {
      if let Some(needle) = &needle {
        if !crate::normalize_identity(&item.label).contains(needle.as_str()) {
          continue;
        }
      }
      out.push(MatchRef {
        section_id: section.id.clone(),
        section_kind: section.kind,
        item_id: item.id.clone(),
        label: item.label.clone(),
        anchor_id: item.anchor_id.clone(),
      });
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{Confidence, PlaylistSidebarItem, SidebarSection};

  fn projection() -> PlaylistSidebarProjection {
    PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "i1".to_string(),
            label: "Daily Mix".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: Some("a1".to_string()),
          },
          PlaylistSidebarItem {
            id: "i2".to_string(),
            label: "Workout".to_string(),
            section_hint: None,
            confidence: Confidence::Low,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    }
  }

  #[test]
  fn no_keyword_returns_all_items() {
    let matches = collect_matches(&projection(), None);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].label, "Daily Mix");
    assert_eq!(matches[0].anchor_id.as_deref(), Some("a1"));
  }

  #[test]
  fn keyword_filters_case_and_whitespace_insensitively() {
    let matches = collect_matches(&projection(), Some("daily"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].item_id, "i1");
    assert_eq!(matches[0].section_kind, SidebarSectionKind::MyPlaylists);
  }

  #[test]
  fn keyword_without_match_returns_empty() {
    let matches = collect_matches(&projection(), Some("zzz"));
    assert!(matches.is_empty());
  }
}
