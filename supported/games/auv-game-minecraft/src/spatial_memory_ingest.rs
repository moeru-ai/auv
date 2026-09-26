//! M2 session ingest: converts telemetry raycast hits into structured spatial landmarks.
//!
//! Enforces deterministic geometry:
//! - Geometry anchor is telemetry raycast coordinates, not VLM guesswork.
//! - Observations without raycast signal are skipped and counted; coordinates are never fabricated.

use serde::{Deserialize, Serialize};

use crate::m2_multi_view::{M2Session, m2_session_withheld_truth};
use crate::memory_maintenance::{MemoryMaintenance, apply_raycast_negative_evidence};
use crate::spatial_memory_store::{ObservationRef, SpatialMemoryStore};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestReport {
  pub landmarks_created: usize,
  pub landmarks_merged: usize,
  pub observations_skipped: usize,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub skipped_reason: Option<String>,
}

/// Pluggable interface for ingesting platform-specific observations into spatial memory.
///
/// NOTICE (Architectural Boundary):
/// This trait marks the platform-dependent perception boundary.
/// The spatial memory core (`SpatialMemoryStore` and `SpatialMemoryQuery`) is platform-independent.
/// However, ingesting raw observations into structured 3D landmarks is inherently platform-specific:
/// - In Minecraft with sidecar telemetry, `MinecraftRaycastIngest` extracts Tier 3 engine raycast hits.
/// - In closed-source games or desktop applications, a new implementation must use that platform's
///   sensing mechanisms (e.g. visual SLAM, monocular depth estimation, or 2D detection + back-projection)
///   to construct landmarks.
pub trait LandmarkIngest {
  fn ingest(&self, store: &mut SpatialMemoryStore, now_millis: u64) -> IngestReport;
}

/// Minecraft telemetry-based ingest using Tier 3 raycast hits from M2 sessions.
pub struct MinecraftRaycastIngest<'a> {
  pub session: &'a M2Session,
  pub maintenance: std::cell::RefCell<MemoryMaintenance>,
  pub negative_evidence_radius_m: f64,
}

impl<'a> MinecraftRaycastIngest<'a> {
  pub fn new(session: &'a M2Session) -> Self {
    Self {
      session,
      maintenance: std::cell::RefCell::new(MemoryMaintenance::default()),
      negative_evidence_radius_m: 0.5,
    }
  }

  pub fn with_maintenance(session: &'a M2Session, maintenance: MemoryMaintenance) -> Self {
    Self {
      session,
      maintenance: std::cell::RefCell::new(maintenance),
      negative_evidence_radius_m: 0.5,
    }
  }

  pub fn with_negative_evidence_radius(mut self, radius_m: f64) -> Self {
    self.negative_evidence_radius_m = radius_m;
    self
  }
}

impl<'a> LandmarkIngest for MinecraftRaycastIngest<'a> {
  fn ingest(&self, store: &mut SpatialMemoryStore, now_millis: u64) -> IngestReport {
    let mut report = IngestReport::default();

    // 1. Lifecycle maintenance: maybe prune stale landmarks at entry
    self.maintenance.borrow_mut().maybe_prune(store, now_millis);

    let withheld_frames = m2_session_withheld_truth(self.session);

    for (i, obs) in self.session.observations.iter().enumerate() {
      let frame = if let Some(view) = self.session.views.get(i) {
        withheld_frames.iter().find(|w| w.frame().spatial_frame_id == view.telemetry_frame_id).map(|w| w.frame())
      } else {
        withheld_frames.get(i).map(|w| w.frame())
      };

      let Some(frame) = frame else {
        report.observations_skipped += 1;
        continue;
      };

      if let Some(hit) = &frame.raycast_hit {
        let obs_ref = ObservationRef {
          observation_id: obs.observation_id.clone(),
          captured_at_millis: obs.captured_at_millis,
        };
        let len_before = store.len();
        store.upsert_from_raycast(hit, &obs_ref);
        let len_after = store.len();
        if len_after > len_before {
          report.landmarks_created += 1;
        } else {
          report.landmarks_merged += 1;
        }

        // 2. Apply negative evidence along the line of sight from player eye to hit block
        let eye = (frame.player_pose.eye_position.x, frame.player_pose.eye_position.y, frame.player_pose.eye_position.z);
        apply_raycast_negative_evidence(store, eye, hit.block_pos, self.negative_evidence_radius_m);
      } else {
        report.observations_skipped += 1;
      }
    }

    report
  }
}

/// Convenience wrapper for ingesting an M2 session via `MinecraftRaycastIngest`.
///
/// Iterates over observations in the session, extracts the corresponding
/// raycast hit from the bound telemetry frame, and upserts it into the store.
/// Observations lacking a valid raycast hit are skipped honestly.
pub fn ingest_m2_session(store: &mut SpatialMemoryStore, session: &M2Session) -> IngestReport {
  let now_millis = session.observations.last().map(|o| o.captured_at_millis).unwrap_or(0);
  MinecraftRaycastIngest::new(session).ingest(store, now_millis)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::m2_multi_view::{M2ViewCaptureInput, M2ViewRole, build_m2_session_from_captures};
  use crate::spatial_memory_observation::SpatialClaimStatus;
  use crate::spatial_memory_store::LandmarkSource;
  use crate::types::{BlockFace, BlockPosition, MinecraftSpatialFrame, PlayerPose, RaycastHit, Vec3, Viewport};

  fn test_frame(id: &str, eye: Vec3, hit: Option<RaycastHit>) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: id.to_string(),
      world_tick: 100,
      monotonic_timestamp_ms: 1000,
      telemetry_session_id: Some("test-session".to_string()),
      viewport: Viewport::new(854, 480),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: PlayerPose {
        eye_position: eye,
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: hit,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: Vec::new(),
    }
  }

  #[test]
  fn ingest_synthetic_session_creates_and_merges_landmarks() {
    let hit_shared = RaycastHit {
      block_pos: BlockPosition::new(10, 65, 20),
      face: BlockFace::Up,
      block_id: "minecraft:grass_block".to_string(),
    };

    let captures = vec![
      M2ViewCaptureInput {
        telemetry_path: None,
        frame: Some(test_frame("f1", Vec3::new(10.0, 66.0, 18.0), Some(hit_shared.clone()))),
        screenshot_artifact_ref: "auv://runs/r1/artifacts/s1".to_string(),
        capture_monotonic_timestamp_ms: Some(1000),
        role: M2ViewRole::Anchor,
        input_history: Vec::new(),
      },
      M2ViewCaptureInput {
        telemetry_path: None,
        frame: Some(test_frame("f2", Vec3::new(11.0, 66.0, 18.0), Some(hit_shared.clone()))),
        screenshot_artifact_ref: "auv://runs/r1/artifacts/s2".to_string(),
        capture_monotonic_timestamp_ms: Some(2000),
        role: M2ViewRole::Translate,
        input_history: Vec::new(),
      },
      M2ViewCaptureInput {
        telemetry_path: None,
        frame: Some(test_frame("f3", Vec3::new(10.5, 66.0, 18.5), None)),
        screenshot_artifact_ref: "auv://runs/r1/artifacts/s3".to_string(),
        capture_monotonic_timestamp_ms: Some(3000),
        role: M2ViewRole::Revisit,
        input_history: Vec::new(),
      },
    ];

    let session = build_m2_session_from_captures(Some("test-m2".to_string()), &captures).expect("valid session");
    let mut store = SpatialMemoryStore::open("test_mem.json").unwrap();
    let report = ingest_m2_session(&mut store, &session);

    assert_eq!(report.landmarks_created, 1);
    assert_eq!(report.landmarks_merged, 1);
    assert_eq!(report.observations_skipped, 1);
    assert_eq!(store.len(), 1);

    let lm = store.get("lm-10-65-20-block_surface").expect("landmark exists");
    assert_eq!(lm.observations.len(), 2);
    assert_eq!(lm.observation_count, 2);
    assert_eq!(lm.status, SpatialClaimStatus::Confirmed);
    assert_eq!(lm.source, LandmarkSource::TelemetryRaycast);
  }

  #[test]
  fn ingest_via_landmark_ingest_trait() {
    let hit = RaycastHit {
      block_pos: BlockPosition::new(-3, 62, 100),
      face: BlockFace::North,
      block_id: "minecraft:sand".to_string(),
    };

    let captures = vec![M2ViewCaptureInput {
      telemetry_path: None,
      frame: Some(test_frame("trait-f1", Vec3::new(-3.0, 63.0, 98.0), Some(hit))),
      screenshot_artifact_ref: "auv://runs/r1/artifacts/trait-s1".to_string(),
      capture_monotonic_timestamp_ms: Some(1000),
      role: M2ViewRole::Anchor,
      input_history: Vec::new(),
    }];

    let session = build_m2_session_from_captures(Some("test-trait".to_string()), &captures).expect("valid session");
    let mut store = SpatialMemoryStore::open("trait_mem.json").unwrap();

    let ingest_impl: Box<dyn LandmarkIngest> = Box::new(MinecraftRaycastIngest::new(&session));
    let report = ingest_impl.ingest(&mut store, 1000);

    assert_eq!(report.landmarks_created, 1);
    assert_eq!(report.landmarks_merged, 0);
    assert_eq!(report.observations_skipped, 0);
    assert_eq!(store.len(), 1);
  }

  #[test]
  fn test_production_ingest_executes_maintenance_and_negative_evidence() {
    let mut store = SpatialMemoryStore::open("test_prod_ingest.json").unwrap();

    // 1. Put an existing stale landmark in store (captured at t = 100)
    let hit_stale = RaycastHit {
      block_pos: BlockPosition::new(10, 60, 10),
      face: BlockFace::Up,
      block_id: "minecraft:oak_log".to_string(),
    };
    let obs_stale = ObservationRef {
      observation_id: "obs-stale".to_string(),
      captured_at_millis: 100,
    };
    let stale_id = store.upsert_from_raycast(&hit_stale, &obs_stale);

    // 2. Put an existing blocking landmark directly on the path of the future raycast
    // Ray will travel from eye (10.0, 66.0, 10.0) to hit (10, 65, 20)
    // Landmark is at (10, 65, 15), within 0.5m of the ray
    let hit_blocking = RaycastHit {
      block_pos: BlockPosition::new(10, 65, 15),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs_blocking = ObservationRef {
      observation_id: "obs-blocking".to_string(),
      captured_at_millis: 350_000,
    };
    let blocking_id = store.upsert_from_raycast(&hit_blocking, &obs_blocking);
    assert_eq!(store.get(&blocking_id).unwrap().consecutive_misses, 0);

    assert_eq!(store.len(), 2);

    // 3. Create an M2 session whose raycast hits (10, 65, 20) at t = 400_000
    let hit_target = RaycastHit {
      block_pos: BlockPosition::new(10, 65, 20),
      face: BlockFace::North,
      block_id: "minecraft:diamond_block".to_string(),
    };
    let captures = vec![M2ViewCaptureInput {
      telemetry_path: None,
      frame: Some(test_frame("prod-f1", Vec3::new(10.0, 66.0, 10.0), Some(hit_target))),
      screenshot_artifact_ref: "auv://runs/r1/artifacts/prod-s1".to_string(),
      capture_monotonic_timestamp_ms: Some(400_000),
      role: M2ViewRole::Anchor,
      input_history: Vec::new(),
    }];

    let session = build_m2_session_from_captures(Some("test-prod-lifecycle".to_string()), &captures).expect("valid session");

    // 4. Run ingest via MinecraftRaycastIngest in the production path
    let ingest = MinecraftRaycastIngest::new(&session);
    let report = ingest.ingest(&mut store, 400_000);

    assert_eq!(report.landmarks_created, 1);

    // 5. Invariant verifications:
    // a. Stale landmark at (10, 60, 10) was pruned via maintenance.maybe_prune in production path:
    assert!(store.get(&stale_id).is_none(), "stale landmark must be pruned during ingest");

    // b. Blocking landmark at (10, 65, 15) received negative evidence in production path:
    let blocking_lm = store.get(&blocking_id).expect("blocking landmark should remain until threshold exceeded");
    assert_eq!(blocking_lm.consecutive_misses, 1, "blocking landmark must receive a miss from passing ray");
    assert!(blocking_lm.confidence < 0.90, "confidence must be penalized");

    // c. Target landmark at (10, 65, 20) was created with 0 misses:
    let target_lm = store.get("lm-10-65-20-block_surface").expect("target landmark must exist");
    assert_eq!(target_lm.consecutive_misses, 0, "hit target must not be penalized");
    assert_eq!(target_lm.confidence, 0.90);
  }
}
