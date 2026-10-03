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

#[cfg(target_os = "windows")]
#[test]
fn test_smtc_media_manager_lifecycle() {
  let mgr = match SmtcMediaManager::new() {
    Ok(m) => m,
    Err(e) => {
      // Running in headless or service session without WinRT SMTC support is acceptable
      eprintln!("Skipping SMTC live test: SMTC manager unavailable: {e}");
      return;
    }
  };

  // 1. current_session should return Ok(Some(_)) or Ok(None), never an unhandled error for no session
  let curr = mgr.current_session();
  assert!(curr.is_ok(), "current_session failed unexpectedly: {:?}", curr.err());

  // 2. list_sessions should return a valid vector of sessions
  let sessions = mgr.list_sessions();
  assert!(sessions.is_ok(), "list_sessions failed unexpectedly: {:?}", sessions.err());

  // 3. find_session with a bogus app_id should return Ok(None)
  let found = mgr.find_session("non_existent_app_id_99999999");
  assert_eq!(found.unwrap().map(|s| s.app_id().to_string()), None);

  // 4. now_playing should succeed (returning Some(state) or None)
  let now = mgr.now_playing();
  assert!(now.is_ok(), "now_playing failed unexpectedly: {:?}", now.err());
}

#[cfg(target_os = "windows")]
#[test]
fn test_audio_volume_controller_not_found_handling() {
  let bogus_pid = 999_999_999;

  let vol_res = AudioVolumeController::get_process_volume(bogus_pid);
  assert!(vol_res.is_err(), "Expected error for nonexistent PID");
  let err_msg = vol_res.unwrap_err().to_string();
  assert!(err_msg.contains("No active audio session found for PID 999999999"), "Unexpected error message: {err_msg}");

  let set_res = AudioVolumeController::set_process_volume(bogus_pid, 0.5);
  assert!(set_res.is_err(), "Expected error for set_process_volume on nonexistent PID");

  let mute_res = AudioVolumeController::get_process_mute(bogus_pid);
  assert!(mute_res.is_err(), "Expected error for get_process_mute on nonexistent PID");

  let set_mute_res = AudioVolumeController::set_process_mute(bogus_pid, true);
  assert!(set_mute_res.is_err(), "Expected error for set_process_mute on nonexistent PID");
}
