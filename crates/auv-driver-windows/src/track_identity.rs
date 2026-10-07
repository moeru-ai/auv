//! Media track identity normalization, confidence ladder resolution, and skip verification.
//!
//! Provides deterministic track identification from raw SMTC or application metadata:
//! - Fullwidth/halfwidth and Unicode case-folding normalization.
//! - Degradation ladder resolution (`Full` -> `Partial` -> `TitleOnly` -> `Indeterminate`).
//! - Disambiguation between repeated tracks and real track transitions without forced success.

use serde::{Deserialize, Serialize};

/// Normalizes a single track metadata field:
/// 1. Converts fullwidth space `\u{3000}` to standard space `\u{0020}`.
/// 2. Converts fullwidth ASCII characters `\u{FF01}..=\u{FF5E}` to halfwidth `\u{0021}..=\u{007E}`.
/// 3. Performs Unicode case-folding / lowercase conversion.
/// 4. Trims leading and trailing whitespace.
/// 5. Collapses consecutive internal whitespace characters into a single space.
pub fn normalize_track_field(input: &str) -> String {
  let mapped: String = input
    .chars()
    .map(|c| match c {
      '\u{3000}' => ' ',
      '\u{FF01}'..='\u{FF5E}' => {
        // Safe mapping: (c as u32 - 0xFEE0) maps 0xFF01..=0xFF5E to 0x0021..=0x007E (valid ASCII range)
        char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
      }
      other => other,
    })
    .collect();

  let lower = mapped.to_lowercase();

  let mut result = String::with_capacity(lower.len());
  let mut in_whitespace = false;

  for c in lower.chars() {
    if c.is_whitespace() {
      in_whitespace = true;
    } else {
      if in_whitespace && !result.is_empty() {
        result.push(' ');
      }
      in_whitespace = false;
      result.push(c);
    }
  }

  result
}

/// Degradation ladder representing the confidence and completeness of a track identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackIdentityLevel {
  /// Both title, artist, and album_title are non-empty after normalization.
  Full,
  /// Title and artist are non-empty after normalization, but album_title is empty.
  Partial,
  /// Only title is non-empty after normalization (artist and album fields are empty).
  TitleOnly,
  /// Title is empty, or metadata is in an ambiguous/anomalous state.
  Indeterminate,
}

impl std::fmt::Display for TrackIdentityLevel {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Full => write!(f, "full"),
      Self::Partial => write!(f, "partial"),
      Self::TitleOnly => write!(f, "title_only"),
      Self::Indeterminate => write!(f, "indeterminate"),
    }
  }
}

/// Verdict from evaluating track transition or skip action between two snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackChangeVerdict {
  /// The track has demonstrably changed to a new or re-queued track.
  Changed,
  /// The track is confirmed unchanged.
  Unchanged,
  /// Track change cannot be verified with certainty (e.g. repeated track without position reset,
  /// or indeterminate current metadata).
  Indeterminate,
}

impl std::fmt::Display for TrackChangeVerdict {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Changed => write!(f, "changed"),
      Self::Unchanged => write!(f, "unchanged"),
      Self::Indeterminate => write!(f, "indeterminate"),
    }
  }
}

/// Media track identity tuple used for deterministic track identification
/// and skip verification.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TrackIdentity {
  pub title: String,
  pub artist: String,
  pub album_title: String,
  pub album_artist: String,
}

impl TrackIdentity {
  /// Constructs a new [`TrackIdentity`].
  pub fn new(title: impl Into<String>, artist: impl Into<String>, album_title: impl Into<String>, album_artist: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      artist: artist.into(),
      album_title: album_title.into(),
      album_artist: album_artist.into(),
    }
  }

  /// Constructs a title-only [`TrackIdentity`].
  pub fn title_only(title: impl Into<String>) -> Self {
    Self {
      title: title.into(),
      artist: String::new(),
      album_title: String::new(),
      album_artist: String::new(),
    }
  }

  /// Returns true if all fields are empty or only whitespace.
  pub fn is_empty(&self) -> bool {
    normalize_track_field(&self.title).is_empty()
      && normalize_track_field(&self.artist).is_empty()
      && normalize_track_field(&self.album_title).is_empty()
      && normalize_track_field(&self.album_artist).is_empty()
  }

  /// Resolves the identity confidence level according to the explicit degradation ladder:
  /// - `Full`: title, artist, and album_title are non-empty after normalization
  /// - `Partial`: title and artist are non-empty after normalization (but album_title is empty)
  /// - `TitleOnly`: only title is non-empty after normalization
  /// - `Indeterminate`: title is empty or ambiguous
  pub fn level(&self) -> TrackIdentityLevel {
    let norm = self.normalized();
    if norm.title.is_empty() {
      TrackIdentityLevel::Indeterminate
    } else if !norm.artist.is_empty() {
      if !norm.album_title.is_empty() {
        TrackIdentityLevel::Full
      } else {
        TrackIdentityLevel::Partial
      }
    } else if norm.album_title.is_empty() && norm.album_artist.is_empty() {
      TrackIdentityLevel::TitleOnly
    } else {
      TrackIdentityLevel::Indeterminate
    }
  }

  /// Returns a normalized copy of this track identity where each field
  /// has been processed with [`normalize_track_field`].
  pub fn normalized(&self) -> NormalizedTrackIdentity {
    NormalizedTrackIdentity {
      title: normalize_track_field(&self.title),
      artist: normalize_track_field(&self.artist),
      album_title: normalize_track_field(&self.album_title),
      album_artist: normalize_track_field(&self.album_artist),
    }
  }
}

/// A normalized view of a [`TrackIdentity`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NormalizedTrackIdentity {
  pub title: String,
  pub artist: String,
  pub album_title: String,
  pub album_artist: String,
}

impl From<&crate::media::MediaTrackMetadata> for TrackIdentity {
  fn from(meta: &crate::media::MediaTrackMetadata) -> Self {
    Self {
      title: meta.title.clone(),
      artist: meta.artist.clone(),
      album_title: meta.album_title.clone(),
      album_artist: meta.album_artist.clone(),
    }
  }
}

impl From<crate::media::MediaTrackMetadata> for TrackIdentity {
  fn from(meta: crate::media::MediaTrackMetadata) -> Self {
    Self {
      title: meta.title,
      artist: meta.artist,
      album_title: meta.album_title,
      album_artist: meta.album_artist,
    }
  }
}

impl From<&crate::media::NowPlayingState> for TrackIdentity {
  fn from(state: &crate::media::NowPlayingState) -> Self {
    match &state.track {
      Some(track) => track.into(),
      None => Self::default(),
    }
  }
}

/// Evaluates whether a track change occurred between `prev` and `curr`.
///
/// Returns `(verdict, level)` where:
/// - If `curr.level() == Indeterminate`: returns `(Indeterminate, Indeterminate)`.
/// - If `prev.is_empty()`: returns `(Changed, curr.level())`.
/// - Compares only fields that are non-empty in BOTH snapshots (missing fields cannot serve
///   as conflict evidence; e.g. an earlier TitleOnly snapshot followed by a Partial snapshot
///   with identical title and now-populated artist must not be misjudged as a track change).
/// - If any mutually non-empty field differs (e.g. title, artist, album_title, album_artist):
///   returns `(Changed, curr.level())`.
/// - If no fields can be compared (no shared non-empty fields):
///   returns `(Indeterminate, curr.level())`.
/// - If all mutually non-empty fields match:
///   - If `has_position_reset` is true (an explicit disambiguation signal such as playback
///     position reset or track id change): returns `(Changed, curr.level())`.
///   - If `has_position_reset` is false: returns `(Indeterminate, curr.level())`. Specifically for
///     skip verification, repeated or unconfirmed tracks with no disambiguation signal must return `Indeterminate`
///     (`confirmed: false`, never forced success).
pub fn evaluate_track_change(
  prev: &TrackIdentity,
  curr: &TrackIdentity,
  has_position_reset: bool,
) -> (TrackChangeVerdict, TrackIdentityLevel) {
  let curr_level = curr.level();
  if curr_level == TrackIdentityLevel::Indeterminate {
    return (TrackChangeVerdict::Indeterminate, TrackIdentityLevel::Indeterminate);
  }

  if prev.is_empty() {
    return (TrackChangeVerdict::Changed, curr_level);
  }

  let prev_norm = prev.normalized();
  let curr_norm = curr.normalized();

  let mut compared_count = 0;
  let mut has_conflict = false;

  if !prev_norm.title.is_empty() && !curr_norm.title.is_empty() {
    compared_count += 1;
    if prev_norm.title != curr_norm.title {
      has_conflict = true;
    }
  }

  if !prev_norm.artist.is_empty() && !curr_norm.artist.is_empty() {
    compared_count += 1;
    if prev_norm.artist != curr_norm.artist {
      has_conflict = true;
    }
  }

  if !prev_norm.album_title.is_empty() && !curr_norm.album_title.is_empty() {
    compared_count += 1;
    if prev_norm.album_title != curr_norm.album_title {
      has_conflict = true;
    }
  }

  if !prev_norm.album_artist.is_empty() && !curr_norm.album_artist.is_empty() {
    compared_count += 1;
    if prev_norm.album_artist != curr_norm.album_artist {
      has_conflict = true;
    }
  }

  if has_conflict {
    return (TrackChangeVerdict::Changed, curr_level);
  }

  if compared_count == 0 {
    return (TrackChangeVerdict::Indeterminate, curr_level);
  }

  if has_position_reset {
    (TrackChangeVerdict::Changed, curr_level)
  } else {
    (TrackChangeVerdict::Indeterminate, curr_level)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_normalize_fullwidth_to_halfwidth() {
    // Fullwidth ASCII letters and numbers
    assert_eq!(normalize_track_field("ＡＢＣ"), "abc");
    assert_eq!(normalize_track_field("ａｂｃ"), "abc");
    assert_eq!(normalize_track_field("１２３"), "123");

    // Fullwidth punctuation
    assert_eq!(normalize_track_field("！（）"), "!()");
    assert_eq!(normalize_track_field("～"), "~");
    assert_eq!(normalize_track_field("：？"), ":?");

    // Fullwidth space \u{3000}
    assert_eq!(normalize_track_field("Hello\u{3000}World"), "hello world");

    // Mixed fullwidth and non-ASCII Chinese characters
    assert_eq!(normalize_track_field("周杰伦　（Ｊａｙ　Ｃｈｏｕ）！"), "周杰伦 (jay chou)!");
  }

  #[test]
  fn test_normalize_whitespace_collapsing_and_trimming() {
    // Leading and trailing whitespace
    assert_eq!(normalize_track_field("   hello world   "), "hello world");
    assert_eq!(normalize_track_field("\t\r\nhello world\n\t"), "hello world");

    // Consecutive internal whitespace collapsing
    assert_eq!(normalize_track_field("hello      world"), "hello world");
    assert_eq!(normalize_track_field("hello \t\r\n world"), "hello world");
    assert_eq!(normalize_track_field("hello\u{3000}\u{3000}world"), "hello world");

    // Degenerate empty / whitespace-only strings
    assert_eq!(normalize_track_field(""), "");
    assert_eq!(normalize_track_field("     "), "");
    assert_eq!(normalize_track_field("\u{3000}\u{3000}"), "");
    assert_eq!(normalize_track_field("\t \r \n"), "");
  }

  #[test]
  fn test_degradation_ladder() {
    // 1. Full: title, artist, album_title all non-empty
    let full = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "周杰伦");
    assert_eq!(full.level(), TrackIdentityLevel::Full);

    let full_no_album_artist = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "");
    assert_eq!(full_no_album_artist.level(), TrackIdentityLevel::Full);

    // 2. Partial: title and artist non-empty, album_title empty
    let partial = TrackIdentity::new("晴天", "周杰伦", "", "");
    assert_eq!(partial.level(), TrackIdentityLevel::Partial);

    let partial_whitespace_album = TrackIdentity::new("晴天", "周杰伦", "   ", "");
    assert_eq!(partial_whitespace_album.level(), TrackIdentityLevel::Partial);

    // 3. TitleOnly: only title is non-empty
    let title_only = TrackIdentity::title_only("晴天");
    assert_eq!(title_only.level(), TrackIdentityLevel::TitleOnly);

    let title_only_spaces = TrackIdentity::new("晴天", "   ", "   ", "   ");
    assert_eq!(title_only_spaces.level(), TrackIdentityLevel::TitleOnly);

    // 4. Indeterminate: title is empty or ambiguous
    let empty_title = TrackIdentity::new("", "周杰伦", "叶惠美", "");
    assert_eq!(empty_title.level(), TrackIdentityLevel::Indeterminate);

    let whitespace_title = TrackIdentity::new("   ", "周杰伦", "叶惠美", "");
    assert_eq!(whitespace_title.level(), TrackIdentityLevel::Indeterminate);

    // Ambiguous: title is present, artist is empty, but album_title is present
    let ambiguous_album = TrackIdentity::new("晴天", "", "叶惠美", "");
    assert_eq!(ambiguous_album.level(), TrackIdentityLevel::Indeterminate);

    // Ambiguous: title is present, artist is empty, but album_artist is present
    let ambiguous_album_artist = TrackIdentity::new("晴天", "", "", "周杰伦");
    assert_eq!(ambiguous_album_artist.level(), TrackIdentityLevel::Indeterminate);

    // Empty identity
    let all_empty = TrackIdentity::default();
    assert_eq!(all_empty.level(), TrackIdentityLevel::Indeterminate);
    assert!(all_empty.is_empty());
  }

  #[test]
  fn test_repeated_track_disambiguation() {
    // Same title and artist without position reset -> Indeterminate
    let prev = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "");
    let curr = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "");
    let (verdict, level) = evaluate_track_change(&prev, &curr, false);
    assert_eq!(verdict, TrackChangeVerdict::Indeterminate);
    assert_eq!(level, TrackIdentityLevel::Full);

    // Same title and artist with position reset -> Changed
    let (verdict_reset, level_reset) = evaluate_track_change(&prev, &curr, true);
    assert_eq!(verdict_reset, TrackChangeVerdict::Changed);
    assert_eq!(level_reset, TrackIdentityLevel::Full);

    // Partial level: same title and artist, no album, without position reset -> Indeterminate
    let prev_partial = TrackIdentity::new("晴天", "周杰伦", "", "");
    let curr_partial = TrackIdentity::new("晴天", "周杰伦", "", "");
    let (v_p, l_p) = evaluate_track_change(&prev_partial, &curr_partial, false);
    assert_eq!(v_p, TrackChangeVerdict::Indeterminate);
    assert_eq!(l_p, TrackIdentityLevel::Partial);

    // Partial level with position reset -> Changed
    let (v_pr, l_pr) = evaluate_track_change(&prev_partial, &curr_partial, true);
    assert_eq!(v_pr, TrackChangeVerdict::Changed);
    assert_eq!(l_pr, TrackIdentityLevel::Partial);

    // TitleOnly level: same title without position reset -> Indeterminate
    let prev_to = TrackIdentity::title_only("晴天");
    let curr_to = TrackIdentity::title_only("晴天");
    let (v_to, l_to) = evaluate_track_change(&prev_to, &curr_to, false);
    assert_eq!(v_to, TrackChangeVerdict::Indeterminate);
    assert_eq!(l_to, TrackIdentityLevel::TitleOnly);

    // TitleOnly level with position reset -> Changed
    let (v_tor, l_tor) = evaluate_track_change(&prev_to, &curr_to, true);
    assert_eq!(v_tor, TrackChangeVerdict::Changed);
    assert_eq!(l_tor, TrackIdentityLevel::TitleOnly);

    // Fullwidth normalization equivalent repeated track
    let prev_fw = TrackIdentity::new("ABC", "XYZ", "ALBUM", "");
    let curr_fw = TrackIdentity::new(" ＡＢＣ ", " ＸＹＺ ", " ＡＬＢＵＭ ", "");
    let (v_fw, _) = evaluate_track_change(&prev_fw, &curr_fw, false);
    assert_eq!(v_fw, TrackChangeVerdict::Indeterminate);

    let (v_fwr, _) = evaluate_track_change(&prev_fw, &curr_fw, true);
    assert_eq!(v_fwr, TrackChangeVerdict::Changed);
  }

  #[test]
  fn test_different_track_detection() {
    let base = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "");

    // 1. Different title -> Changed
    let diff_title = TrackIdentity::new("七里香", "周杰伦", "叶惠美", "");
    let (v1, l1) = evaluate_track_change(&base, &diff_title, false);
    assert_eq!(v1, TrackChangeVerdict::Changed);
    assert_eq!(l1, TrackIdentityLevel::Full);

    // 2. Same title, different artist -> Changed
    let diff_artist = TrackIdentity::new("晴天", "翻唱歌手", "叶惠美", "");
    let (v2, l2) = evaluate_track_change(&base, &diff_artist, false);
    assert_eq!(v2, TrackChangeVerdict::Changed);
    assert_eq!(l2, TrackIdentityLevel::Full);

    // 3. Same title and artist, different album -> Changed
    let diff_album = TrackIdentity::new("晴天", "周杰伦", "地表最强演唱会", "");
    let (v3, l3) = evaluate_track_change(&base, &diff_album, false);
    assert_eq!(v3, TrackChangeVerdict::Changed);
    assert_eq!(l3, TrackIdentityLevel::Full);

    // 4. Empty prev (initial track start) -> Changed
    let empty_prev = TrackIdentity::default();
    let (v4, l4) = evaluate_track_change(&empty_prev, &base, false);
    assert_eq!(v4, TrackChangeVerdict::Changed);
    assert_eq!(l4, TrackIdentityLevel::Full);

    // 5. Indeterminate curr -> Indeterminate
    let indet_curr = TrackIdentity::new("", "周杰伦", "叶惠美", "");
    let (v5, l5) = evaluate_track_change(&base, &indet_curr, true);
    assert_eq!(v5, TrackChangeVerdict::Indeterminate);
    assert_eq!(l5, TrackIdentityLevel::Indeterminate);
  }

  #[test]
  fn test_serde_and_display() {
    assert_eq!(TrackIdentityLevel::Full.to_string(), "full");
    assert_eq!(TrackIdentityLevel::Partial.to_string(), "partial");
    assert_eq!(TrackIdentityLevel::TitleOnly.to_string(), "title_only");
    assert_eq!(TrackIdentityLevel::Indeterminate.to_string(), "indeterminate");

    assert_eq!(serde_json::to_string(&TrackIdentityLevel::Full).unwrap(), "\"full\"");
    assert_eq!(serde_json::to_string(&TrackIdentityLevel::TitleOnly).unwrap(), "\"title_only\"");

    assert_eq!(serde_json::from_str::<TrackIdentityLevel>("\"full\"").unwrap(), TrackIdentityLevel::Full);
    assert_eq!(serde_json::from_str::<TrackIdentityLevel>("\"title_only\"").unwrap(), TrackIdentityLevel::TitleOnly);

    assert_eq!(TrackChangeVerdict::Changed.to_string(), "changed");
    assert_eq!(TrackChangeVerdict::Unchanged.to_string(), "unchanged");
    assert_eq!(TrackChangeVerdict::Indeterminate.to_string(), "indeterminate");

    assert_eq!(serde_json::to_string(&TrackChangeVerdict::Changed).unwrap(), "\"changed\"");
    assert_eq!(serde_json::from_str::<TrackChangeVerdict>("\"indeterminate\"").unwrap(), TrackChangeVerdict::Indeterminate);

    let identity = TrackIdentity::new("Title", "Artist", "Album", "AlbumArtist");
    let json = serde_json::to_string(&identity).unwrap();
    let deserialized: TrackIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(identity, deserialized);
  }

  #[test]
  fn test_conversion_from_media_track_metadata() {
    let meta = crate::media::MediaTrackMetadata {
      title: "Song".to_string(),
      artist: "Singer".to_string(),
      album_title: "Collection".to_string(),
      album_artist: "Singer".to_string(),
      genres: vec!["Pop".to_string()],
    };

    let id: TrackIdentity = (&meta).into();
    assert_eq!(id.title, "Song");
    assert_eq!(id.artist, "Singer");
    assert_eq!(id.album_title, "Collection");
    assert_eq!(id.album_artist, "Singer");
    assert_eq!(id.level(), TrackIdentityLevel::Full);
  }

  #[test]
  fn test_incomplete_metadata_handling() {
    // Regression test for P2 issue:
    // Missing metadata in one snapshot must not be treated as a conflict.
    // E.g. prev is TitleOnly ("晴天"), curr is Partial ("晴天", "周杰伦").
    let prev_title_only = TrackIdentity::title_only("晴天");
    let curr_with_artist = TrackIdentity::new("晴天", "周杰伦", "", "");

    // Without position reset: should be Indeterminate (same track, newly completed artist metadata)
    let (v1, l1) = evaluate_track_change(&prev_title_only, &curr_with_artist, false);
    assert_eq!(v1, TrackChangeVerdict::Indeterminate);
    assert_eq!(l1, TrackIdentityLevel::Partial);

    // With position reset: should be Changed
    let (v2, l2) = evaluate_track_change(&prev_title_only, &curr_with_artist, true);
    assert_eq!(v2, TrackChangeVerdict::Changed);
    assert_eq!(l2, TrackIdentityLevel::Partial);

    // Reverse: prev has artist, curr lost artist (e.g. transient query degradation)
    let (v3, l3) = evaluate_track_change(&curr_with_artist, &prev_title_only, false);
    assert_eq!(v3, TrackChangeVerdict::Indeterminate);
    assert_eq!(l3, TrackIdentityLevel::TitleOnly);

    // Partial ("晴天", "周杰伦") -> Full ("晴天", "周杰伦", "叶惠美") without reset -> Indeterminate
    let full = TrackIdentity::new("晴天", "周杰伦", "叶惠美", "");
    let (v4, l4) = evaluate_track_change(&curr_with_artist, &full, false);
    assert_eq!(v4, TrackChangeVerdict::Indeterminate);
    assert_eq!(l4, TrackIdentityLevel::Full);

    // Different title with incomplete metadata -> Changed
    let diff_title_with_artist = TrackIdentity::new("七里香", "周杰伦", "", "");
    let (v5, l5) = evaluate_track_change(&prev_title_only, &diff_title_with_artist, false);
    assert_eq!(v5, TrackChangeVerdict::Changed);
    assert_eq!(l5, TrackIdentityLevel::Partial);

    // Conflicting artist where both are present -> Changed
    let diff_artist = TrackIdentity::new("晴天", "翻唱歌手", "", "");
    let (v6, l6) = evaluate_track_change(&curr_with_artist, &diff_artist, false);
    assert_eq!(v6, TrackChangeVerdict::Changed);
    assert_eq!(l6, TrackIdentityLevel::Partial);

    // Zero shared non-empty fields: prev has only artist, curr has only title -> Indeterminate
    let prev_artist_only = TrackIdentity::new("", "周杰伦", "", "");
    let (v7, l7) = evaluate_track_change(&prev_artist_only, &prev_title_only, false);
    assert_eq!(v7, TrackChangeVerdict::Indeterminate);
    assert_eq!(l7, TrackIdentityLevel::TitleOnly);
  }
}
