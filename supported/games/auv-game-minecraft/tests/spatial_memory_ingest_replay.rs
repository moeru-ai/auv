//! Replay test: ingest real M2 session into structured spatial memory.
//!
//! Validates:
//! - All landmarks are generated directly from telemetry raycast hits.
//! - Landmark count and coordinates match session raycast records 1-to-1 without hallucination.
//! - All landmarks are Confirmed and sourced from TelemetryRaycast.
//! - Offline replay only; Minecraft is not launched.

use std::path::PathBuf;

use auv_game_minecraft::m2_multi_view::{M2ViewCaptureInput, M2ViewRole, build_m2_session_from_captures, m2_session_withheld_truth};
use auv_game_minecraft::spatial_memory_ingest::ingest_m2_session;
use auv_game_minecraft::spatial_memory_observation::{ObservationInputEvent, SpatialClaimStatus};
use auv_game_minecraft::spatial_memory_store::{LandmarkSource, SpatialMemoryStore};
use auv_game_minecraft::types::BlockPosition;

struct LiveView {
  dir: &'static str,
  role: M2ViewRole,
  screenshot_artifact_ref: &'static str,
  capture_monotonic_timestamp_ms: u64,
}

const LIVE_VIEWS: [LiveView; 3] = [
  LiveView {
    dir: "v01",
    role: M2ViewRole::Anchor,
    screenshot_artifact_ref: "auv://runs/01a094a5-4ba2-7a71-94cc-31773a91d4f9/artifacts/01a094a5-4c79-7110-9082-85b328432473",
    capture_monotonic_timestamp_ms: 9193984,
  },
  LiveView {
    dir: "v02",
    role: M2ViewRole::Translate,
    screenshot_artifact_ref: "auv://runs/01a094a7-67a3-7a51-87fa-9fd89d88afd9/artifacts/01a094a7-6876-77a2-b018-b9181ea24c83",
    capture_monotonic_timestamp_ms: 9332234,
  },
  LiveView {
    dir: "v03",
    role: M2ViewRole::Revisit,
    screenshot_artifact_ref: "auv://runs/01a094a7-8783-7843-8786-02b01cc272f9/artifacts/01a094a7-8855-7e22-8d5b-42cb5b61e26c",
    capture_monotonic_timestamp_ms: 9340390,
  },
];

#[test]
fn replay_m2_live_session_ingests_into_landmarks_matching_raycast() {
  let session_dir: PathBuf = match std::env::var("M2_LIVE_SESSION_DIR") {
    Ok(dir) => PathBuf::from(dir),
    Err(_) => {
      let default_path = PathBuf::from("F:/auv/.tmp/m2-session");
      if default_path.is_dir() {
        default_path
      } else {
        eprintln!("M2 live session dir not found; skipping replay test");
        return;
      }
    }
  };

  let captures: Vec<M2ViewCaptureInput> = LIVE_VIEWS
    .iter()
    .map(|view| {
      let view_dir = session_dir.join(view.dir);
      let telemetry_path = view_dir.join("telemetry.jsonl");
      assert!(telemetry_path.is_file(), "missing telemetry: {}", telemetry_path.display());
      assert!(view_dir.join("screenshot.png").is_file(), "missing screenshot for {}", view.dir);
      M2ViewCaptureInput {
        telemetry_path: Some(telemetry_path),
        frame: None,
        screenshot_artifact_ref: view.screenshot_artifact_ref.to_string(),
        capture_monotonic_timestamp_ms: Some(view.capture_monotonic_timestamp_ms),
        role: view.role,
        input_history: Vec::<ObservationInputEvent>::new(),
      }
    })
    .collect();

  let session = build_m2_session_from_captures(Some("m2-live-session".to_string()), &captures).expect("live session captures must ingest");

  // Collect all raycast hits from the session telemetry truth
  let withheld = m2_session_withheld_truth(&session);
  let session_raycasts: Vec<BlockPosition> =
    withheld.iter().filter_map(|w| w.frame().raycast_hit.as_ref().map(|hit| hit.block_pos)).collect();
  let total_session_raycasts = session_raycasts.len();
  assert!(total_session_raycasts > 0, "session must have at least one raycast hit");

  let temp_file = tempfile::NamedTempFile::new().expect("temp file");
  let mut store = SpatialMemoryStore::open(temp_file.path()).expect("open store");
  let report = ingest_m2_session(&mut store, &session);

  // Assertions per Definition of Done:
  // 1. report.landmarks_created + report.landmarks_merged matches total raycast hits in session
  assert_eq!(
    report.landmarks_created + report.landmarks_merged,
    total_session_raycasts,
    "processed raycasts ({}) must equal total session raycasts ({})",
    report.landmarks_created + report.landmarks_merged,
    total_session_raycasts
  );

  // 2. Observations without raycast are skipped
  assert_eq!(report.observations_skipped, 1, "v03 has no raycast and must be skipped");

  // 3. Every landmark position can be found in session raycast records (no coordinates fabricated)
  for landmark in store.landmarks().values() {
    assert!(session_raycasts.contains(&landmark.position), "landmark position {:?} must exist in session raycast hits", landmark.position);

    // 4. Source and status assertions
    assert_eq!(landmark.source, LandmarkSource::TelemetryRaycast, "landmark source must be TelemetryRaycast");
    assert_eq!(landmark.status, SpatialClaimStatus::Confirmed, "landmark status must be Confirmed");
  }

  // In the real M2 session, v01 and v02 hit the exact same block (-22, 81, 43), so 1 created, 1 merged
  assert_eq!(report.landmarks_created, 1);
  assert_eq!(report.landmarks_merged, 1);
  assert_eq!(store.len(), 1);

  let lm = store.get("lm--22-81-43-block_surface").expect("landmark (-22, 81, 43) exists");
  assert_eq!(lm.position, BlockPosition::new(-22, 81, 43));
  assert_eq!(lm.observations.len(), 2);
}
