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
