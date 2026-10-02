//! Unit tests for media control types and serialization.

use super::*;

#[test]
fn test_media_playback_status_roundtrip() {
  let statuses = vec![
    MediaPlaybackStatus::Closed,
    MediaPlaybackStatus::Opened,
    MediaPlaybackStatus::Changing,
    MediaPlaybackStatus::Stopped,
    MediaPlaybackStatus::Playing,
    MediaPlaybackStatus::Paused,
    MediaPlaybackStatus::Unknown(99),
  ];

  for s in statuses {
    let json = serde_json::to_string(&s).expect("Serialize failed");
    let deser: MediaPlaybackStatus = serde_json::from_str(&json).expect("Deserialize failed");
    assert_eq!(s, deser);
  }
}

#[test]
fn test_media_track_metadata_defaults() {
  let meta = MediaTrackMetadata::default();
  assert!(meta.title.is_empty());
  assert!(meta.artist.is_empty());
  assert!(meta.album_title.is_empty());
  assert!(meta.genres.is_empty());

  let custom = MediaTrackMetadata {
    title: "晴天".to_string(),
    artist: "周杰伦".to_string(),
    album_title: "叶惠美".to_string(),
    album_artist: "周杰伦".to_string(),
    genres: vec!["Pop".to_string()],
  };

  let json = serde_json::to_string(&custom).expect("Serialize failed");
  let deser: MediaTrackMetadata = serde_json::from_str(&json).expect("Deserialize failed");
  assert_eq!(custom, deser);
}

#[test]
fn test_now_playing_state_serialization() {
  let state = NowPlayingState {
    app_id: "QQMusic.exe".to_string(),
    status: MediaPlaybackStatus::Playing,
    track: Some(MediaTrackMetadata {
      title: "落灯花".to_string(),
      artist: "漆柚".to_string(),
      album_title: "".to_string(),
      album_artist: "".to_string(),
      genres: vec![],
    }),
  };

  let json = serde_json::to_string(&state).expect("Serialize failed");
  assert!(json.contains("QQMusic.exe"));
  assert!(json.contains("落灯花"));
  assert!(json.contains("Playing"));

  let deser: NowPlayingState = serde_json::from_str(&json).expect("Deserialize failed");
  assert_eq!(state, deser);
}
