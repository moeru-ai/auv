use auv_view::normalize_identity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistLabelMatchTier {
  Exact,
  Contains,
  None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistQueryMatchMode {
  Exact,
  Contains,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistQueryResolution {
  Unique { mode: PlaylistQueryMatchMode },
  NotFound,
  Ambiguous,
}

pub(crate) fn playlist_label_match_tier(normalized_label: &str, normalized_query: &str) -> PlaylistLabelMatchTier {
  if normalized_query.is_empty() {
    return PlaylistLabelMatchTier::None;
  }
  if normalized_label == normalized_query {
    return PlaylistLabelMatchTier::Exact;
  }
  if normalized_label.contains(normalized_query) || normalized_query.contains(normalized_label) {
    return PlaylistLabelMatchTier::Contains;
  }
  PlaylistLabelMatchTier::None
}

pub(crate) fn resolve_playlist_query_from_labels(labels: &[&str], query: &str) -> PlaylistQueryResolution {
  let normalized_query = normalize_identity(query);
  if normalized_query.is_empty() {
    return PlaylistQueryResolution::NotFound;
  }

  let mut exact_count = 0usize;
  let mut contains_count = 0usize;

  for label in labels {
    let normalized_label = normalize_identity(label);
    match playlist_label_match_tier(&normalized_label, &normalized_query) {
      PlaylistLabelMatchTier::Exact => exact_count += 1,
      PlaylistLabelMatchTier::Contains => contains_count += 1,
      PlaylistLabelMatchTier::None => {}
    }
  }

  if exact_count == 1 {
    return PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Exact,
    };
  }
  if exact_count > 1 {
    return PlaylistQueryResolution::Ambiguous;
  }
  if contains_count == 1 {
    return PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Contains,
    };
  }
  if contains_count > 1 {
    return PlaylistQueryResolution::Ambiguous;
  }
  PlaylistQueryResolution::NotFound
}

pub(crate) fn playlist_query_resolution_is_unique_exact(resolution: PlaylistQueryResolution) -> bool {
  matches!(
    resolution,
    PlaylistQueryResolution::Unique {
      mode: PlaylistQueryMatchMode::Exact,
    }
  )
}

#[cfg(test)]
pub(crate) fn filter_labels_by_resolution<'a>(
  labels: impl Iterator<Item = &'a str>,
  query: &str,
  resolution: PlaylistQueryResolution,
) -> Vec<&'a str> {
  let normalized_query = normalize_identity(query);
  labels
    .filter(|label| {
      let tier = playlist_label_match_tier(&normalize_identity(label), &normalized_query);
      match resolution {
        PlaylistQueryResolution::Unique {
          mode: PlaylistQueryMatchMode::Exact,
        } => tier == PlaylistLabelMatchTier::Exact,
        PlaylistQueryResolution::Unique {
          mode: PlaylistQueryMatchMode::Contains,
        } => tier == PlaylistLabelMatchTier::Contains,
        PlaylistQueryResolution::Ambiguous => tier == PlaylistLabelMatchTier::Exact || tier == PlaylistLabelMatchTier::Contains,
        PlaylistQueryResolution::NotFound => false,
      }
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn exact_beats_contains_for_numeric_query() {
    let labels = ["43", "39", "3"];
    let resolution = resolve_playlist_query_from_labels(&labels, "3");
    assert_eq!(
      resolution,
      PlaylistQueryResolution::Unique {
        mode: PlaylistQueryMatchMode::Exact
      }
    );
    let matched = filter_labels_by_resolution(labels.into_iter(), "3", resolution);
    assert_eq!(matched, vec!["3"]);
  }

  #[test]
  fn contains_fallback_when_no_exact_match() {
    let labels = ["人造器械"];
    let resolution = resolve_playlist_query_from_labels(&labels, "人造");
    assert_eq!(
      resolution,
      PlaylistQueryResolution::Unique {
        mode: PlaylistQueryMatchMode::Contains
      }
    );
  }

  #[test]
  fn ambiguous_when_only_contains_collide() {
    let labels = ["43", "13"];
    let resolution = resolve_playlist_query_from_labels(&labels, "3");
    assert_eq!(resolution, PlaylistQueryResolution::Ambiguous);
  }

  #[test]
  fn scan_query_seen_requires_unique_exact_match() {
    assert!(playlist_query_resolution_is_unique_exact(resolve_playlist_query_from_labels(&["3"], "3")));
    assert!(!playlist_query_resolution_is_unique_exact(resolve_playlist_query_from_labels(&["43"], "3")));
    assert!(playlist_query_resolution_is_unique_exact(resolve_playlist_query_from_labels(&["43", "3"], "3")));
    assert!(!playlist_query_resolution_is_unique_exact(resolve_playlist_query_from_labels(&["43", "13"], "3")));
  }
}
