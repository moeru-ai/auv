//! Replay gate for the real `m2-live-session` capture.
//!
//! The live session (three real Minecraft views with 1.92 m / 2.64 m / 4.47 m eye
//! translations, captured 2026-09-12 via printwindow.windows against a live game)
//! lives outside the repo in the Windows working dir. Point `M2_LIVE_SESSION_DIR`
//! at the session root (the directory containing `v01/`, `v02/`, `v03/`, each with
//! `screenshot.png` and `telemetry.jsonl`). When the variable is unset, the test
//! skips honestly instead of failing CI.

use std::path::PathBuf;

use auv_game_minecraft::m2_multi_view::{M2ViewCaptureInput, M2ViewRole, build_m2_session_from_captures, m2_session_report};
use auv_game_minecraft::spatial_memory_observation::ObservationInputEvent;

struct LiveView {
  dir: &'static str,
  role: M2ViewRole,
  screenshot_artifact_ref: &'static str,
  capture_monotonic_timestamp_ms: u64,
}

/// Recorded 2026-09-12. Artifact URIs and capture clocks come from the session's
/// own `session-report.json`; translations were anchor->translate 1.92 m,
/// anchor->revisit 2.64 m, translate->revisit 4.47 m.
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
fn live_session_passes_m2_capture_gate() {
  let session_dir: PathBuf = match std::env::var("M2_LIVE_SESSION_DIR") {
    Ok(dir) => PathBuf::from(dir),
    Err(_) => {
      eprintln!("M2_LIVE_SESSION_DIR is not set; skipping live-session replay");
      return;
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
  let report = m2_session_report(&session);
  assert!(report.meets_m2_capture_gate, "m2-live-session must pass the capture gate; failures: {:?}", report.gate_failures);
  println!("m2-live-session gate report:\n{}", serde_json::to_string_pretty(&report).expect("report serializes"));
}
