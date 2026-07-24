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

#[test]
fn sidebar_artifacts_are_published_from_memory_without_local_mirror() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view_parsers/sidebar/live.rs");
  let source = std::fs::read_to_string(&path).expect("read live sidebar source");
  let function = source
    .split("fn publish_observation_artifacts(")
    .nth(1)
    .and_then(|tail| tail.split("fn finish_artifacts").next())
    .expect("publish_observation_artifacts body");
  let run_artifacts_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/run_artifacts.rs");
  let run_artifacts = std::fs::read_to_string(run_artifacts_path).expect("read run artifact source");
  let task = run_artifacts
    .split("pub(crate) fn spawn_artifact_task(")
    .nth(1)
    .and_then(|tail| tail.split("fn encode_png").next())
    .expect("artifact task adapter");
  let guard = task.find("can_publish_artifacts()").expect("artifact authority guard");
  let spawn = task.find("std::thread::spawn").expect("artifact writer spawn");

  assert!(guard < spawn, "sidebar artifact writer must reject disabled recording before it starts a thread");
  assert!(function.contains("run_artifacts::spawn_artifact_task"), "sidebar capture must delegate context propagation");
  assert!(!function.contains("std::thread::spawn"), "commands must not own tracing context propagation");
  assert!(!function.contains("artifact_dir"), "artifact preparation must not create a second path authority");
  assert!(!function.contains("std::fs::"), "artifact preparation must not mirror bytes through the filesystem");
  assert!(!function.contains(".save("), "artifact preparation must encode images in memory");
  assert!(!function.contains("emit_file"), "artifact preparation must hand bytes directly to auv-tracing");
  assert!(function.contains("emit_png"), "binary artifacts must enter the shared recording adapter from memory");
}

#[test]
fn sidebar_artifact_failures_do_not_change_scan_result() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view_parsers/sidebar/live.rs");
  let source = std::fs::read_to_string(&path).expect("read live sidebar source");

  assert!(
    !source.contains("scan.diagnostics.extend(observer.finish_artifacts())"),
    "recording failures must not be projected into the direct sidebar scan result"
  );
  assert!(
    !source.contains("fn finish_artifacts(self) -> Vec<ParserDiagnostic>"),
    "artifact worker completion must not manufacture domain diagnostics"
  );
}

#[test]
fn playlist_ls_recording_failures_do_not_become_command_limits() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli.rs");
  let source = std::fs::read_to_string(&path).expect("read NetEase CLI source");
  let run_playlist = source
    .split("async fn run_playlist(")
    .nth(1)
    .and_then(|tail| tail.split("async fn run_playlist_select_command(").next())
    .expect("run_playlist body");
  let publication = run_playlist
    .split("match crate::run_artifacts::persist_playlist_ls_artifacts")
    .nth(1)
    .and_then(|tail| tail.split("let output = build_playlist_json_output").next())
    .expect("playlist artifact publication branch");

  assert!(!publication.contains("ls_known_limits.push"), "recording availability must not change the playlist listing's command limits");
  assert!(
    !publication.contains("ls_known_limits\n                .push"),
    "multiline recording failures must not change the playlist listing's command limits"
  );
}

#[test]
fn sidebar_target_probe_artifacts_do_not_use_local_paths_or_control_the_probe_result() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view_parsers/sidebar/target_probe.rs");
  let source = std::fs::read_to_string(&path).expect("read sidebar target probe source");
  let publisher = source
    .split("fn publish_sidebar_target_probe_artifacts(")
    .nth(1)
    .and_then(|tail| tail.split("pub(crate) fn sidebar_target_probe_diagnostic_message").next())
    .expect("publish_sidebar_target_probe_artifacts body");
  let capture = source
    .split("pub(crate) fn capture_sidebar_target_probe(")
    .nth(1)
    .and_then(|tail| tail.split("#[cfg(test)]").next())
    .expect("capture_sidebar_target_probe body");

  for forbidden in [
    "artifact_dir",
    "artifact_stem",
    "std::fs::",
    ".save(",
    "emit_file",
    "PathBuf",
  ] {
    assert!(!publisher.contains(forbidden), "target probe artifact publisher still contains {forbidden:?}");
    assert!(!capture.contains(forbidden), "target probe capture still depends on {forbidden:?}");
  }
  assert!(publisher.contains("emit_png"), "probe images must enter the shared recording adapter from memory");
  assert!(publisher.contains("emit_json"), "probe structures must enter the shared recording adapter as typed JSON");
  assert!(!publisher.contains("Result<"), "recording publication must not become the probe's direct result");
}

#[test]
fn playlist_execution_does_not_route_recording_through_artifact_directories() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/playlist.rs");
  let source = std::fs::read_to_string(&path).expect("read playlist command source");
  let production =
    source.split("#[cfg(target_os = \"macos\")]\npub fn run_playlist_select").nth(1).expect("macOS playlist execution section");

  for forbidden in ["artifact_dir", "std::fs::", ".save(", "emit_file"] {
    assert!(!production.contains(forbidden), "playlist execution still routes recording through {forbidden:?}");
  }
  assert!(production.contains("run_artifacts::emit_png"), "playlist image evidence must be published from memory");
  assert!(production.contains("run_artifacts::emit_json"), "playlist structured evidence must be published directly");
}

#[test]
fn playlist_followups_use_explicit_typed_scan_uri_without_local_lineage_state() {
  let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
  let manifest_source = std::fs::read_to_string(&manifest).expect("read NetEase public inputs");
  let inputs = manifest_source.split("pub struct Inputs").nth(1).and_then(|tail| tail.split("impl Inputs").next()).expect("Inputs section");
  assert!(!inputs.contains("artifact_dir"), "playlist inputs must not carry an internal recording directory");

  let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli.rs");
  let cli_source = std::fs::read_to_string(&cli).expect("read NetEase CLI source");
  for command in ["PlaylistSelectCommand", "PlaylistPlayCommand"] {
    let section = cli_source
      .split(&format!("pub(crate) struct {command}"))
      .nth(1)
      .and_then(|tail| tail.split("}\n").next())
      .unwrap_or_else(|| panic!("{command} section"));
    assert!(section.contains("scan_uri: Option<ArtifactUri>"), "{command} must carry the caller-selected canonical scan URI");
  }
  assert!(
    cli_source.contains("playlist play --candidate-id requires --scan-uri"),
    "candidate IDs must not resolve through hidden latest-scan state"
  );

  let run_artifacts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/run_artifacts.rs");
  let run_artifacts_source = std::fs::read_to_string(&run_artifacts).expect("read NetEase run artifacts source");
  for forbidden in [
    "VIEW_MEMORY_RUN_LINEAGE_FILE",
    "LineageManifestError",
    "lineage_manifest_path",
    "read_lineage_manifest",
    "write_lineage_manifest",
  ] {
    assert!(!run_artifacts_source.contains(forbidden), "run artifacts still maintain a second local authority through {forbidden}");
  }
}

#[test]
fn daily_recommended_execution_does_not_route_recording_through_artifact_directories() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/daily_recommended.rs");
  let source = std::fs::read_to_string(&path).expect("read daily recommended source");
  let production = source
    .split("#[cfg(target_os = \"macos\")]\npub fn run_daily_recommended_songs_scan")
    .nth(1)
    .and_then(|tail| tail.split("#[cfg(target_os = \"macos\")]\npub(crate) fn best_text_match").next())
    .expect("macOS daily recommended execution section");

  for forbidden in [
    "artifact_dir",
    "std::fs::",
    ".save(",
    "emit_file",
    "write_capture_artifact",
  ] {
    assert!(!production.contains(forbidden), "daily recommended execution still routes recording through {forbidden:?}");
  }
  assert!(production.contains("run_artifacts::emit_png"), "daily recommended image evidence must be published from memory");
  assert!(production.contains("run_artifacts::emit_json"), "daily recommended structured evidence must be published directly");
  assert!(production.contains("match_template(&capture.image"), "template matching must consume the existing in-memory capture");
}

#[test]
fn daily_recommended_inputs_do_not_expose_internal_artifact_directories() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
  let source = std::fs::read_to_string(&path).expect("read NetEase public inputs");
  let daily = source
    .split("pub struct DailyRecommendedPlayInputs")
    .nth(1)
    .and_then(|tail| tail.split("pub struct DailyRecommendedPlayResult").next())
    .expect("DailyRecommendedPlayInputs section");
  let songs = source
    .split("pub struct SongListInputs")
    .nth(1)
    .and_then(|tail| tail.split("pub struct SongListScanResult").next())
    .expect("SongListInputs section");

  assert!(!daily.contains("artifact_dir"), "daily recommended inputs still expose an internal recording directory");
  assert!(!songs.contains("artifact_dir"), "song-list inputs still expose an internal recording directory");
}

#[test]
fn playback_probe_does_not_expose_or_use_an_internal_artifact_directory() {
  let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands/playback.rs");
  let source = std::fs::read_to_string(&path).expect("read playback probe source");
  let inputs = source
    .split("pub struct PlaybackStatusInputs")
    .nth(1)
    .and_then(|tail| tail.split("pub struct PlaybackStatus").next())
    .expect("PlaybackStatusInputs section");
  let macos = source
    .split("#[cfg(target_os = \"macos\")]\npub fn run_playback_status_probe")
    .nth(1)
    .and_then(|tail| tail.split("#[cfg(target_os = \"macos\")]\nfn recognition_in_window_space").next())
    .expect("macOS playback probe section");

  assert!(!inputs.contains("artifact_dir"), "playback inputs still expose an internal recording directory");
  for forbidden in [
    "artifact_dir",
    "std::fs::",
    ".save(",
    "emit_file",
    "write_capture_artifact",
  ] {
    assert!(!macos.contains(forbidden), "playback probe still routes recording through {forbidden:?}");
  }
  assert!(macos.contains("run_artifacts::emit_png"), "playback captures must be published from memory");
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
