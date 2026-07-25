use auv_netease_music::{
  DailyRecommendedPlayResult, DailyRecommendedVerification, LaunchResult, PlaybackStatus, PlaylistPlayResult, PlaylistPlayVerification,
  PlaylistSelectResult, PlaylistSelectVerification, PlaylistSidebarScan, SongListScanResult,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const FORBIDDEN_LOCAL_ARTIFACT_FIELDS: &[&str] = &[
  "artifact",
  "artifacts",
  "artifact_path",
  "artifact_paths",
  "recognition_artifact",
  "sidebar_echo_recognition_artifact",
  "source_artifact",
  "source_artifacts",
];
const FORBIDDEN_DOMAIN_TIMELINE_FIELDS: &[&str] = &["steps", "interaction_events"];

#[test]
fn public_domain_results_do_not_serialize_local_artifact_locators() {
  let select_value = select_fixture();
  assert_round_trip_has_no_local_artifact_fields::<PlaylistSelectResult>(select_value.clone());
  assert_round_trip_has_no_local_artifact_fields::<PlaylistPlayResult>(json!({
    "command": "playlist.play",
    "query": "Coding",
    "select": select_value,
    "verification": {
      "status": "passed",
      "control_state": "pause_visible",
      "observed_bottom_text": null
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_round_trip_has_no_local_artifact_fields::<DailyRecommendedPlayResult>(json!({
    "command": "playlist.play.daily-recommended",
    "app": {},
    "window": {},
    "verification": {
      "status": "passed",
      "evidence": {
        "method": "bottom_playback_control",
        "control_state": "pause_visible",
        "observed_bottom_text": null
      }
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_round_trip_has_no_local_artifact_fields::<SongListScanResult>(json!({
    "command": "playlist.songs.ls",
    "target": "daily-recommended",
    "app": {},
    "window": {},
    "song_list_region": {},
    "items": [],
    "observations": [{
      "observation_index": 0,
      "source_artifact": "/tmp/songs-obs-0000.png",
      "incoming_scroll_delivery_path": null,
      "scroll_motion": null,
      "rows": []
    }],
    "boundary": {
      "top": "unknown",
      "bottom": "unknown",
      "left": "unknown",
      "right": "unknown"
    },
    "diagnostics": [],
    "known_limits": [],
    "artifacts": ["/tmp/songs-obs-0000.png"]
  }));
  assert_round_trip_has_no_local_artifact_fields::<PlaybackStatus>(json!({
    "command": "playback.status",
    "app": {},
    "window": {},
    "playback_exists": true,
    "was_playing": true,
    "control_state": "pause_visible",
    "click_point": null,
    "detail_screen_detected": true,
    "source": "bottom_control",
    "artifacts": ["/tmp/playback.png"],
    "diagnostics": [],
    "known_limits": []
  }));
}

#[test]
fn public_domain_results_do_not_embed_parallel_timelines() {
  assert_round_trip_has_no_domain_timeline::<PlaylistSelectResult>(select_fixture());
  assert_round_trip_has_no_domain_timeline::<PlaylistPlayResult>(json!({
    "command": "playlist.play",
    "query": "Coding",
    "select": select_fixture(),
    "verification": {
      "status": "passed",
      "control_state": "pause_visible",
      "observed_bottom_text": null
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_round_trip_has_no_domain_timeline::<DailyRecommendedPlayResult>(json!({
    "command": "playlist.play.daily-recommended",
    "app": {},
    "window": {},
    "verification": {
      "status": "passed",
      "evidence": {
        "method": "bottom_playback_control",
        "control_state": "pause_visible",
        "observed_bottom_text": null
      }
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_round_trip_has_no_domain_timeline::<LaunchResult>(json!({
    "command": "open-window",
    "window_found": false,
    "window_title": null,
    "process_name": "cloudmusic.exe",
    "executable": null,
  }));

  let path =
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sidebar-scan-proof/hermetic_v0/playlist-sidebar-scan.json");
  let scan: PlaylistSidebarScan =
    serde_json::from_slice(&std::fs::read(path).expect("read playlist scan fixture")).expect("decode playlist scan fixture");
  let output = serde_json::to_value(scan).expect("encode playlist scan");

  assert_no_domain_timeline(&output, "$");
}

#[test]
fn public_domain_results_reject_legacy_step_arrays() {
  assert_rejects_steps::<PlaylistSelectResult>(select_fixture());
  assert_rejects_steps::<PlaylistPlayResult>(json!({
    "command": "playlist.play",
    "query": "Coding",
    "select": select_fixture(),
    "verification": {
      "status": "passed",
      "control_state": "pause_visible",
      "observed_bottom_text": null
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_rejects_steps::<DailyRecommendedPlayResult>(json!({
    "command": "playlist.play.daily-recommended",
    "app": {},
    "window": {},
    "verification": {
      "status": "passed",
      "evidence": {
        "method": "bottom_playback_control",
        "control_state": "pause_visible",
        "observed_bottom_text": null
      }
    },
    "diagnostics": [],
    "known_limits": []
  }));
  assert_rejects_steps::<LaunchResult>(json!({
    "command": "open-window",
    "window_found": false,
    "window_title": null,
    "process_name": "cloudmusic.exe",
    "executable": null
  }));
}

#[test]
fn playlist_select_result_rejects_tracing_identity() {
  let mut value = select_fixture();
  value.as_object_mut().expect("playlist select fixture object").insert("run_id".to_string(), json!("019f9211-8b84-7040-98e3-cfff2947a642"));

  assert!(
    serde_json::from_value::<PlaylistSelectResult>(value).is_err(),
    "the app-owned direct result must not absorb the outer tracing run identity"
  );
}

#[test]
fn playlist_reacquire_uses_one_tagged_result_without_summary_flags() {
  let mut value = select_fixture();
  value.as_object_mut().expect("select fixture object").insert(
    "reacquire".to_string(),
    json!({
      "status": "reacquired",
      "bounds": {
        "x": 59.0,
        "y": 405.0,
        "width": 120.0,
        "height": 20.0
      },
      "strategy": "label_current_viewport",
      "observation_count": 1
    }),
  );

  let result: PlaylistSelectResult = serde_json::from_value(value).expect("typed reacquire result");
  let output = serde_json::to_value(result).expect("encode typed reacquire result");
  let reacquire = &output["reacquire"];
  assert_eq!(reacquire["status"], "reacquired");
  assert!(reacquire.get("outcome").is_none());
  assert!(reacquire.get("summary").is_none());
  assert!(reacquire.get("skipped_rescan_replay").is_none());
}

#[test]
fn verification_types_reject_passed_states_without_required_evidence() {
  assert!(
    serde_json::from_value::<PlaylistSelectVerification>(json!({
      "status": "passed",
      "method": "main_title_ocr_full_window_v1",
      "observed_title": null,
      "note": null
    }))
    .is_err()
  );
  assert!(
    serde_json::from_value::<PlaylistPlayVerification>(json!({
      "status": "passed",
      "method": "bottom_control_icon_with_player_change",
      "control_state": null,
      "observed_bottom_text": null,
      "note": null
    }))
    .is_err()
  );
  assert!(
    serde_json::from_value::<DailyRecommendedVerification>(json!({
      "status": "passed",
      "method": "bottom_control_icon",
      "control_state": null,
      "observed_bottom_text": null,
      "match_count": 0,
      "best_score": null,
      "note": null
    }))
    .is_err()
  );
}

fn select_fixture() -> Value {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select-proof/hermetic_v0/select-result.json");
  serde_json::from_slice(&std::fs::read(path).expect("read playlist-select fixture")).expect("decode playlist-select fixture")
}

fn assert_round_trip_has_no_local_artifact_fields<T>(input: Value)
where
  T: DeserializeOwned + Serialize,
{
  let result: T = serde_json::from_value(input).expect("decode public domain result");
  let output = serde_json::to_value(result).expect("encode public domain result");
  assert_no_forbidden_fields(&output, "$");
}

fn assert_round_trip_has_no_domain_timeline<T>(input: Value)
where
  T: DeserializeOwned + Serialize,
{
  let result: T = serde_json::from_value(input).expect("decode public domain result");
  let output = serde_json::to_value(result).expect("encode public domain result");
  assert_no_domain_timeline(&output, "$");
}

fn assert_rejects_steps<T>(mut input: Value)
where
  T: DeserializeOwned,
{
  input.as_object_mut().expect("domain result fixture must be an object").insert("steps".to_string(), json!([]));
  assert!(serde_json::from_value::<T>(input).is_err(), "legacy `steps` field must not be accepted");
}

fn assert_no_domain_timeline(value: &Value, location: &str) {
  match value {
    Value::Object(object) => {
      for (key, child) in object {
        assert!(
          !FORBIDDEN_DOMAIN_TIMELINE_FIELDS.contains(&key.as_str()),
          "domain result contains parallel timeline field `{key}` at {location}; lifecycle belongs in auv-tracing"
        );
        assert_no_domain_timeline(child, &format!("{location}.{key}"));
      }
    }
    Value::Array(items) => {
      for (index, item) in items.iter().enumerate() {
        assert_no_domain_timeline(item, &format!("{location}[{index}]"));
      }
    }
    _ => {}
  }
}

fn assert_no_forbidden_fields(value: &Value, location: &str) {
  match value {
    Value::Object(object) => {
      for (key, child) in object {
        assert!(
          !FORBIDDEN_LOCAL_ARTIFACT_FIELDS.contains(&key.as_str()),
          "public domain result contains local artifact field `{key}` at {location}"
        );
        assert_no_forbidden_fields(child, &format!("{location}.{key}"));
      }
    }
    Value::Array(items) => {
      for (index, item) in items.iter().enumerate() {
        assert_no_forbidden_fields(item, &format!("{location}[{index}]"));
      }
    }
    _ => {}
  }
}
