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
#[path = "query_match_test.rs"]
mod tests;
