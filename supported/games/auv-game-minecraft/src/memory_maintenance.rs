//! Spatial memory lifecycle maintenance and negative observation evidence.
//!
//! Provides:
//! - `MemoryMaintenance`: periodic pruning of expired and low-confidence landmarks
//! - `apply_raycast_negative_evidence`: penalizing landmarks along a raycast line-of-sight
//!   that were not hit, indicating they are no longer present or valid.

use crate::spatial_memory_store::SpatialMemoryStore;
use crate::types::BlockPosition;

/// Lifecycle maintenance manager for periodic store pruning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryMaintenance {
  pub prune_interval_millis: u64,
  last_prune_millis: u64,
}

impl Default for MemoryMaintenance {
  fn default() -> Self {
    Self::new(60_000)
  }
}

impl MemoryMaintenance {
  pub fn new(prune_interval_millis: u64) -> Self {
    Self {
      prune_interval_millis,
      last_prune_millis: 0,
    }
  }

  pub fn last_prune_millis(&self) -> u64 {
    self.last_prune_millis
  }

  /// Prune stale landmarks if elapsed time since last prune >= prune_interval_millis.
  /// Returns the number of pruned landmarks; returns 0 if interval has not elapsed yet.
  pub fn maybe_prune(&mut self, store: &mut SpatialMemoryStore, now_millis: u64) -> usize {
    if self.last_prune_millis == 0 || now_millis.saturating_sub(self.last_prune_millis) >= self.prune_interval_millis {
      self.last_prune_millis = now_millis;
      store.prune_stale(now_millis)
    } else {
      0
    }
  }
}

/// Raycast negative evidence:
/// A ray travels from `eye` to `hit_block`. Any other landmark along this line segment
/// (within `radius_m`) that did not block the ray indicates that the landmark was expected
/// to occlude the ray but failed to do so -> its position is invalidated -> `store.record_miss` is called.
///
/// Geometric Invariants:
/// - The hit block itself is excluded (it received a confirmed hit, not a miss).
/// - Points behind the player (`t <= 0.0`) or beyond the hit block (`t >= 1.0`) are outside the ray path.
/// - Only points with projection factor `0.0 < t < 1.0` and perpendicular distance `<= radius_m` are penalized.
///
/// Returns the number of penalized landmarks.
pub fn apply_raycast_negative_evidence(
  store: &mut SpatialMemoryStore,
  eye: (f64, f64, f64),
  hit_block: BlockPosition,
  radius_m: f64,
) -> usize {
  let a = eye;
  // Use geometric center of hit block in world coordinates
  let b = (f64::from(hit_block.x) + 0.5, f64::from(hit_block.y) + 0.5, f64::from(hit_block.z) + 0.5);
  let ab = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
  let ab_len_sq = ab.0 * ab.0 + ab.1 * ab.1 + ab.2 * ab.2;

  if ab_len_sq < 1e-9 {
    return 0;
  }

  let mut missed_ids = Vec::new();

  for (id, landmark) in store.landmarks() {
    // 1. Exclude the hit block itself (it received a confirmed hit)
    if landmark.position == hit_block {
      continue;
    }

    // 2. Landmark coordinates (continuous position if present, otherwise block center)
    let p = if let Some(cp) = landmark.continuous_position {
      cp
    } else {
      (f64::from(landmark.position.x) + 0.5, f64::from(landmark.position.y) + 0.5, f64::from(landmark.position.z) + 0.5)
    };

    // Vector AP
    let ap = (p.0 - a.0, p.1 - a.1, p.2 - a.2);
    // Projection factor t along ray segment AB
    let t = (ap.0 * ab.0 + ap.1 * ab.1 + ap.2 * ab.2) / ab_len_sq;

    // Only landmarks strictly along the segment between eye and hit_block (0.0 < t < 1.0)
    if t > 0.0 && t < 1.0 {
      let proj = (a.0 + t * ab.0, a.1 + t * ab.1, a.2 + t * ab.2);
      let dist = ((p.0 - proj.0).powi(2) + (p.1 - proj.1).powi(2) + (p.2 - proj.2).powi(2)).sqrt();
      if dist <= radius_m {
        missed_ids.push(id.clone());
      }
    }
  }

  let count = missed_ids.len();
  for id in missed_ids {
    let _ = store.record_miss(&id);
  }

  count
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::spatial_memory_store::{ObservationRef, SpatialMemoryConfig, SpatialMemoryStore};
  use crate::types::{BlockFace, BlockPosition, RaycastHit};

  #[test]
  fn test_apply_raycast_negative_evidence_penalizes_intersecting_landmark() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();

    // 1. Landmark A at (0, 81, 0)
    let hit_a = RaycastHit {
      block_pos: BlockPosition::new(0, 81, 0),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs_a = ObservationRef {
      observation_id: "obs-a".to_string(),
      captured_at_millis: 100,
    };
    let id_a = store.upsert_from_raycast(&hit_a, &obs_a);
    let lm_a_initial = store.get(&id_a).unwrap();
    assert_eq!(lm_a_initial.consecutive_misses, 0);
    assert_eq!(lm_a_initial.confidence, 0.90);

    // 2. Landmark B at target hit block (0, 81, 10)
    let hit_b = RaycastHit {
      block_pos: BlockPosition::new(0, 81, 10),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs_b = ObservationRef {
      observation_id: "obs-b".to_string(),
      captured_at_millis: 200,
    };
    let id_b = store.upsert_from_raycast(&hit_b, &obs_b);

    // 3. Raycast from eye (0.5, 82.0, -5.0) to hit_block (0, 81, 10) (center: 0.5, 81.5, 10.5).
    // The ray passes directly above landmark A at z ~ 0.5 (y ~ 81.82).
    // The distance to A's center (0.5, 81.5, 0.5) is ~0.32m, within default radius 0.5m.
    let eye = (0.5, 82.0, -5.0);
    let penalized = apply_raycast_negative_evidence(&mut store, eye, hit_b.block_pos, 0.5);

    assert_eq!(penalized, 1);
    let lm_a_after = store.get(&id_a).unwrap();
    assert_eq!(lm_a_after.consecutive_misses, 1);
    assert!((lm_a_after.confidence - 0.80).abs() < 1e-6);

    // Hit block B must NOT be penalized (it is Confirmed)
    let lm_b_after = store.get(&id_b).unwrap();
    assert_eq!(lm_b_after.consecutive_misses, 0);
    assert_eq!(lm_b_after.confidence, 0.90);
  }

  #[test]
  fn test_maybe_prune_interval_gating() {
    let mut store = SpatialMemoryStore::open_with_config(
      "memory.json",
      SpatialMemoryConfig {
        stale_threshold_millis: 10_000,
        ..Default::default()
      },
    )
    .unwrap();

    let hit = RaycastHit {
      block_pos: BlockPosition::new(1, 64, 1),
      face: BlockFace::Up,
      block_id: "minecraft:oak_log".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 1000,
    };
    store.upsert_from_raycast(&hit, &obs);

    let mut maintenance = MemoryMaintenance::new(60_000);

    // First call at t = 2000: last_prune was 0, so it initializes and prunes (0 stale)
    let pruned1 = maintenance.maybe_prune(&mut store, 2000);
    assert_eq!(pruned1, 0);
    assert_eq!(maintenance.last_prune_millis(), 2000);

    // At t = 15_000: landmark is now stale (15_000 - 1000 = 14_000 > 10_000),
    // but prune interval (60_000) has NOT elapsed (15_000 - 2000 = 13_000 < 60_000).
    // maybe_prune must return 0 without pruning:
    let pruned2 = maintenance.maybe_prune(&mut store, 15_000);
    assert_eq!(pruned2, 0);
    assert_eq!(store.len(), 1, "landmark should not be pruned before interval elapses");

    // At t = 65_000: interval has elapsed (65_000 - 2000 = 63_000 >= 60_000).
    // maybe_prune must prune the stale landmark:
    let pruned3 = maintenance.maybe_prune(&mut store, 65_000);
    assert_eq!(pruned3, 1);
    assert_eq!(store.len(), 0);
    assert_eq!(maintenance.last_prune_millis(), 65_000);
  }
}
