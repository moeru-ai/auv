//! Deterministic, typed, file-backed spatial memory store for structured landmarks.
//!
//! Replaces 3DGS as memory by anchoring landmarks to engine/telemetry coordinates
//! rather than VLM guesses.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use auv_file::{JsonWriteOptions, read_json_file, write_json_file};
use serde::{Deserialize, Serialize};

use crate::spatial_memory_observation::SpatialClaimStatus;
use crate::types::{BlockFace, BlockPosition, RaycastHit};

pub const SPATIAL_MEMORY_STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkKind {
  BlockSurface,
  Object,
  Region,
  PathNode,
}

impl LandmarkKind {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::BlockSurface => "block_surface",
      Self::Object => "object",
      Self::Region => "region",
      Self::PathNode => "path_node",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandmarkSource {
  TelemetryRaycast,
  MultiViewTriangulation,
  VlmHypothesis,
  VisualPerception,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationRef {
  pub observation_id: String,
  pub captured_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LandmarkObservation {
  pub observation_ref: ObservationRef,
  pub source: LandmarkSource,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub hit_face: Option<BlockFace>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub block_id: Option<String>,
}

fn default_landmark_confidence() -> f64 {
  0.90
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialLandmark {
  pub landmark_id: String,
  pub kind: LandmarkKind,
  pub position: BlockPosition,
  pub first_observed: ObservationRef,
  pub observations: Vec<LandmarkObservation>,
  pub status: SpatialClaimStatus,
  pub source: LandmarkSource,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub surface_face: Option<BlockFace>,
  #[serde(default)]
  pub observation_count: u32,
  #[serde(default)]
  pub last_observed_millis: u64,
  #[serde(default)]
  pub consecutive_misses: u32,
  #[serde(default = "default_landmark_confidence")]
  pub confidence: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryConfig {
  pub dedup_radius_m: f64,
  pub stale_threshold_millis: u64,
  pub min_confidence: f64,
  pub max_consecutive_misses: u32,
}

impl Default for SpatialMemoryConfig {
  fn default() -> Self {
    Self {
      dedup_radius_m: 0.6,
      stale_threshold_millis: 300_000,
      min_confidence: 0.3,
      max_consecutive_misses: 5,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryStoreData {
  pub schema_version: u32,
  pub landmarks: HashMap<String, SpatialLandmark>,
  #[serde(default)]
  pub config: Option<SpatialMemoryConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialMemoryStoreError {
  Io(String),
  Serialization(String),
  LandmarkNotFound(String),
}

impl fmt::Display for SpatialMemoryStoreError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(err) => write!(f, "I/O error in spatial memory store: {err}"),
      Self::Serialization(err) => write!(f, "serialization error in spatial memory store: {err}"),
      Self::LandmarkNotFound(id) => write!(f, "landmark not found in spatial memory store: {id}"),
    }
  }
}

impl std::error::Error for SpatialMemoryStoreError {}

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialMemoryStore {
  landmarks: HashMap<String, SpatialLandmark>,
  path: PathBuf,
  config: SpatialMemoryConfig,
}

impl SpatialMemoryStore {
  pub fn open_with_config(path: impl AsRef<Path>, config: SpatialMemoryConfig) -> Result<Self, SpatialMemoryStoreError> {
    let path_buf = path.as_ref().to_path_buf();
    let is_empty_or_missing = !path_buf.exists() || path_buf.metadata().map(|m| m.len() == 0).unwrap_or(false);
    if is_empty_or_missing {
      return Ok(Self {
        landmarks: HashMap::new(),
        path: path_buf,
        config,
      });
    }

    let data: SpatialMemoryStoreData =
      read_json_file(&path_buf).map_err(|err| SpatialMemoryStoreError::Serialization(format!("{err:?}")))?;

    Ok(Self {
      landmarks: data.landmarks,
      path: path_buf,
      config: data.config.unwrap_or(config),
    })
  }

  pub fn open(path: impl AsRef<Path>) -> Result<Self, SpatialMemoryStoreError> {
    Self::open_with_config(path, SpatialMemoryConfig::default())
  }

  pub fn config(&self) -> &SpatialMemoryConfig {
    &self.config
  }

  pub fn set_config(&mut self, config: SpatialMemoryConfig) {
    self.config = config;
  }

  pub fn save(&self) -> Result<(), SpatialMemoryStoreError> {
    let data = SpatialMemoryStoreData {
      schema_version: SPATIAL_MEMORY_STORE_SCHEMA_VERSION,
      landmarks: self.landmarks.clone(),
      config: Some(self.config),
    };

    write_json_file(
      &self.path,
      &data,
      JsonWriteOptions {
        create_parent_dirs: true,
        trailing_newline: true,
      },
    )
    .map_err(|err| SpatialMemoryStoreError::Io(format!("{err:?}")))
  }

  /// Record a successful observation of an existing landmark.
  /// Increments observation_count, resets consecutive_misses to 0,
  /// updates last_observed_millis, and updates confidence (0.90 base + 0.02 per extra observation, max 0.99).
  pub fn record_observation(&mut self, landmark_id: &str, now_millis: u64) -> Result<(), SpatialMemoryStoreError> {
    let landmark = self.landmarks.get_mut(landmark_id).ok_or_else(|| SpatialMemoryStoreError::LandmarkNotFound(landmark_id.to_string()))?;

    if landmark.observation_count == 0 && !landmark.observations.is_empty() {
      landmark.observation_count = landmark.observations.len() as u32;
    }
    landmark.observation_count += 1;
    landmark.consecutive_misses = 0;
    landmark.last_observed_millis = now_millis;
    let extra = (landmark.observation_count.saturating_sub(1) as f64) * 0.02;
    landmark.confidence = (0.90 + extra).min(0.99);
    Ok(())
  }

  /// Record a negative observation (ray passed through landmark position without hit).
  /// Increments consecutive_misses and penalizes confidence by -0.1 (clamped to 0.0).
  /// Note: The caller is responsible for line-of-sight determination; the store only tallies misses.
  pub fn record_miss(&mut self, landmark_id: &str) -> Result<(), SpatialMemoryStoreError> {
    let landmark = self.landmarks.get_mut(landmark_id).ok_or_else(|| SpatialMemoryStoreError::LandmarkNotFound(landmark_id.to_string()))?;

    landmark.consecutive_misses += 1;
    landmark.confidence = (landmark.confidence - 0.1).max(0.0);
    Ok(())
  }

  /// Prune stale or low-confidence landmarks based on configured thresholds:
  /// - `now_millis - last_observed_millis > stale_threshold_millis`
  /// - `confidence < min_confidence`
  /// - `consecutive_misses >= max_consecutive_misses`
  /// Returns the number of pruned landmarks.
  pub fn prune_stale(&mut self, now_millis: u64) -> usize {
    let before_len = self.landmarks.len();
    let stale_threshold = self.config.stale_threshold_millis;
    let min_conf = self.config.min_confidence;
    let max_misses = self.config.max_consecutive_misses;

    self.landmarks.retain(|_id, lm| {
      let is_expired =
        stale_threshold > 0 && lm.last_observed_millis > 0 && now_millis.saturating_sub(lm.last_observed_millis) > stale_threshold;
      let is_low_confidence = lm.confidence < min_conf;
      let is_too_many_misses = max_misses > 0 && lm.consecutive_misses >= max_misses;

      !is_expired && !is_low_confidence && !is_too_many_misses
    });

    before_len - self.landmarks.len()
  }

  pub fn upsert_from_raycast(&mut self, hit: &RaycastHit, obs: &ObservationRef) -> String {
    let mut matching_id = None;
    for (id, landmark) in &self.landmarks {
      let dx = f64::from(landmark.position.x - hit.block_pos.x);
      let dy = f64::from(landmark.position.y - hit.block_pos.y);
      let dz = f64::from(landmark.position.z - hit.block_pos.z);
      let dist = (dx * dx + dy * dy + dz * dz).sqrt();
      if dist < self.config.dedup_radius_m {
        matching_id = Some(id.clone());
        break;
      }
    }

    if let Some(id) = matching_id {
      let _ = self.record_observation(&id, obs.captured_at_millis);
      let landmark = self.landmarks.get_mut(&id).expect("landmark exists");
      landmark.observations.push(LandmarkObservation {
        observation_ref: obs.clone(),
        source: LandmarkSource::TelemetryRaycast,
        hit_face: Some(hit.face),
        block_id: Some(hit.block_id.clone()),
      });
      landmark.surface_face = Some(hit.face);
      id
    } else {
      let kind = LandmarkKind::BlockSurface;
      let landmark_id = format!("lm-{}-{}-{}-{}", hit.block_pos.x, hit.block_pos.y, hit.block_pos.z, kind.as_str());
      let landmark = SpatialLandmark {
        landmark_id: landmark_id.clone(),
        kind,
        position: hit.block_pos,
        first_observed: obs.clone(),
        observations: vec![LandmarkObservation {
          observation_ref: obs.clone(),
          source: LandmarkSource::TelemetryRaycast,
          hit_face: Some(hit.face),
          block_id: Some(hit.block_id.clone()),
        }],
        status: SpatialClaimStatus::Confirmed,
        source: LandmarkSource::TelemetryRaycast,
        description: None,
        surface_face: Some(hit.face),
        observation_count: 1,
        last_observed_millis: obs.captured_at_millis,
        consecutive_misses: 0,
        confidence: 0.90,
      };
      self.landmarks.insert(landmark_id.clone(), landmark);
      landmark_id
    }
  }

  /// Upsert a landmark observed via visual perception (2D detection + depth back-projection).
  ///
  /// Heterogeneous Multi-Engine Fusion Semantics:
  /// - Source is `LandmarkSource::VisualPerception`.
  /// - New visual perception landmarks receive `SpatialClaimStatus::Candidate` because visual
  ///   detection is less authoritative than telemetry raycast.
  /// - Initial confidence = (detection_confidence * 0.5).clamp(0.1, 0.5).
  /// - If merged with an existing landmark within `dedup_radius_m`:
  ///   - Higher authority status wins: if either is `Confirmed` (e.g. from raycast), the merged status is `Confirmed`.
  ///   - Description is updated or enriched with the visual perception semantic label.
  ///   - An observation record is appended with `LandmarkSource::VisualPerception`.
  ///   - `last_observed_millis` is updated and `consecutive_misses` is reset to 0.
  pub fn upsert_from_perception(
    &mut self,
    block_pos: BlockPosition,
    label: &str,
    detection_confidence: f64,
    obs: &ObservationRef,
  ) -> String {
    let mut matching_id = None;
    for (id, landmark) in &self.landmarks {
      let dx = f64::from(landmark.position.x - block_pos.x);
      let dy = f64::from(landmark.position.y - block_pos.y);
      let dz = f64::from(landmark.position.z - block_pos.z);
      let dist = (dx * dx + dy * dy + dz * dz).sqrt();
      if dist < self.config.dedup_radius_m {
        matching_id = Some(id.clone());
        break;
      }
    }

    if let Some(id) = matching_id {
      let _ = self.record_observation(&id, obs.captured_at_millis);
      let landmark = self.landmarks.get_mut(&id).expect("landmark exists");
      landmark.observations.push(LandmarkObservation {
        observation_ref: obs.clone(),
        source: LandmarkSource::VisualPerception,
        hit_face: None,
        block_id: None,
      });
      // Multi-engine fusion: status takes higher authority (Confirmed > Candidate)
      // Confirmed stays Confirmed; if it was Candidate, it stays Candidate.
      // Semantic label from visual perception enriches the description:
      landmark.description = Some(label.to_string());
      id
    } else {
      let kind = LandmarkKind::Object;
      let landmark_id = format!("lm-{}-{}-{}-{}", block_pos.x, block_pos.y, block_pos.z, kind.as_str());
      let confidence = (detection_confidence * 0.5).clamp(0.1, 0.5);
      let landmark = SpatialLandmark {
        landmark_id: landmark_id.clone(),
        kind,
        position: block_pos,
        first_observed: obs.clone(),
        observations: vec![LandmarkObservation {
          observation_ref: obs.clone(),
          source: LandmarkSource::VisualPerception,
          hit_face: None,
          block_id: None,
        }],
        status: SpatialClaimStatus::Candidate,
        source: LandmarkSource::VisualPerception,
        description: Some(label.to_string()),
        surface_face: None,
        observation_count: 1,
        last_observed_millis: obs.captured_at_millis,
        consecutive_misses: 0,
        confidence,
      };
      self.landmarks.insert(landmark_id.clone(), landmark);
      landmark_id
    }
  }

  pub fn get(&self, id: &str) -> Option<&SpatialLandmark> {
    self.landmarks.get(id)
  }

  pub fn query_radius(&self, center: BlockPosition, radius_m: f64) -> Vec<&SpatialLandmark> {
    self
      .landmarks
      .values()
      .filter(|landmark| {
        let dx = f64::from(landmark.position.x - center.x);
        let dy = f64::from(landmark.position.y - center.y);
        let dz = f64::from(landmark.position.z - center.z);
        (dx * dx + dy * dy + dz * dz).sqrt() <= radius_m
      })
      .collect()
  }

  pub fn len(&self) -> usize {
    self.landmarks.len()
  }

  pub fn is_empty(&self) -> bool {
    self.landmarks.is_empty()
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  pub fn landmarks(&self) -> &HashMap<String, SpatialLandmark> {
    &self.landmarks
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::NamedTempFile;

  #[test]
  fn upsert_deduplication_merges_nearby_raycast() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();
    let hit1 = RaycastHit {
      block_pos: BlockPosition::new(10, 64, -20),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs1 = ObservationRef {
      observation_id: "obs-001".to_string(),
      captured_at_millis: 1000,
    };
    let id1 = store.upsert_from_raycast(&hit1, &obs1);

    assert_eq!(store.len(), 1);
    assert_eq!(id1, "lm-10-64--20-block_surface");

    let hit2 = RaycastHit {
      block_pos: BlockPosition::new(10, 64, -20),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs2 = ObservationRef {
      observation_id: "obs-002".to_string(),
      captured_at_millis: 2000,
    };
    let id2 = store.upsert_from_raycast(&hit2, &obs2);

    assert_eq!(id1, id2);
    assert_eq!(store.len(), 1);

    let landmark = store.get(&id1).unwrap();
    assert_eq!(landmark.first_observed.observation_id, "obs-001");
    assert_eq!(landmark.observations.len(), 2);
    assert_eq!(landmark.observation_count, 2);
    assert_eq!(landmark.last_observed_millis, 2000);
    assert_eq!(landmark.consecutive_misses, 0);
    assert_eq!(landmark.surface_face, Some(BlockFace::Up));
    assert_eq!(landmark.confidence, 0.92);
    assert_eq!(landmark.observations[0].observation_ref.observation_id, "obs-001");
    assert_eq!(landmark.observations[1].observation_ref.observation_id, "obs-002");
    assert_eq!(landmark.status, SpatialClaimStatus::Confirmed);
    assert_eq!(landmark.source, LandmarkSource::TelemetryRaycast);
  }

  #[test]
  fn record_observation_increments_count_and_raises_confidence() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();
    let hit = RaycastHit {
      block_pos: BlockPosition::new(5, 60, 5),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 1000,
    };
    let id = store.upsert_from_raycast(&hit, &obs);

    let lm = store.get(&id).unwrap();
    assert_eq!(lm.observation_count, 1);
    assert_eq!(lm.confidence, 0.90);

    store.record_observation(&id, 2000).unwrap();
    let lm = store.get(&id).unwrap();
    assert_eq!(lm.observation_count, 2);
    assert_eq!(lm.last_observed_millis, 2000);
    assert_eq!(lm.confidence, 0.92);

    store.record_observation(&id, 3000).unwrap();
    let lm = store.get(&id).unwrap();
    assert_eq!(lm.observation_count, 3);
    assert_eq!(lm.last_observed_millis, 3000);
    assert!((lm.confidence - 0.94).abs() < 1e-6);
  }

  #[test]
  fn record_miss_penalizes_confidence_and_tallies_misses() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();
    let hit = RaycastHit {
      block_pos: BlockPosition::new(5, 60, 5),
      face: BlockFace::North,
      block_id: "minecraft:stone".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 1000,
    };
    let id = store.upsert_from_raycast(&hit, &obs);

    for _ in 0..6 {
      store.record_miss(&id).unwrap();
    }

    let lm = store.get(&id).unwrap();
    assert_eq!(lm.consecutive_misses, 6);
    // 0.90 - 6 * 0.10 = 0.30
    assert!((lm.confidence - 0.30).abs() < 1e-6);

    // One more miss drops below 0.30 to 0.20
    store.record_miss(&id).unwrap();
    let lm = store.get(&id).unwrap();
    assert!((lm.confidence - 0.20).abs() < 1e-6);
  }

  #[test]
  fn prune_stale_removes_expired_and_low_confidence_landmarks() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();
    // Landmark 1: expired
    let hit1 = RaycastHit {
      block_pos: BlockPosition::new(1, 60, 1),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs1 = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 1000,
    };
    let id1 = store.upsert_from_raycast(&hit1, &obs1);

    // Landmark 2: fresh
    let hit2 = RaycastHit {
      block_pos: BlockPosition::new(10, 60, 10),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs2 = ObservationRef {
      observation_id: "obs-2".to_string(),
      captured_at_millis: 350_000,
    };
    let id2 = store.upsert_from_raycast(&hit2, &obs2);

    // Landmark 3: low confidence (below 0.3)
    let hit3 = RaycastHit {
      block_pos: BlockPosition::new(20, 60, 20),
      face: BlockFace::Up,
      block_id: "minecraft:stone".to_string(),
    };
    let obs3 = ObservationRef {
      observation_id: "obs-3".to_string(),
      captured_at_millis: 350_000,
    };
    let id3 = store.upsert_from_raycast(&hit3, &obs3);
    for _ in 0..7 {
      store.record_miss(&id3).unwrap();
    }

    assert_eq!(store.len(), 3);

    // At now = 400_000:
    // id1: 400_000 - 1_000 = 399_000 > 300_000 -> stale
    // id2: 400_000 - 350_000 = 50_000 <= 300_000 -> kept
    // id3: confidence 0.20 < 0.30 and consecutive_misses 7 >= 5 -> pruned
    let pruned = store.prune_stale(400_000);
    assert_eq!(pruned, 2);
    assert_eq!(store.len(), 1);
    assert!(store.get(&id2).is_some());
    assert!(store.get(&id1).is_none());
    assert!(store.get(&id3).is_none());
  }

  #[test]
  fn backward_compatible_deserialization_from_old_json() {
    let old_json = r#"{
      "schema_version": 1,
      "landmarks": {
        "lm-legacy": {
          "landmark_id": "lm-legacy",
          "kind": "block_surface",
          "position": {"x": 0, "y": 64, "z": 0},
          "first_observed": {"observation_id": "obs-0", "captured_at_millis": 500},
          "observations": [],
          "status": "confirmed",
          "source": "telemetry_raycast"
        }
      }
    }"#;

    let data: SpatialMemoryStoreData = serde_json::from_str(old_json).expect("deserialize old json");
    let lm = data.landmarks.get("lm-legacy").expect("landmark present");
    assert_eq!(lm.observation_count, 0);
    assert_eq!(lm.consecutive_misses, 0);
    assert_eq!(lm.confidence, 0.90);
    assert_eq!(lm.surface_face, None);
  }

  #[test]
  fn custom_config_parameterizes_dedup_radius() {
    let mut store = SpatialMemoryStore::open_with_config(
      "memory.json",
      SpatialMemoryConfig {
        dedup_radius_m: 1.5,
        ..Default::default()
      },
    )
    .unwrap();

    let hit1 = RaycastHit {
      block_pos: BlockPosition::new(0, 60, 0),
      face: BlockFace::Up,
      block_id: "minecraft:dirt".to_string(),
    };
    let hit2 = RaycastHit {
      block_pos: BlockPosition::new(0, 61, 0), // 1.0 meter away
      face: BlockFace::Up,
      block_id: "minecraft:dirt".to_string(),
    };
    let obs1 = ObservationRef {
      observation_id: "obs-1".to_string(),
      captured_at_millis: 100,
    };
    let obs2 = ObservationRef {
      observation_id: "obs-2".to_string(),
      captured_at_millis: 200,
    };

    let id1 = store.upsert_from_raycast(&hit1, &obs1);
    let id2 = store.upsert_from_raycast(&hit2, &obs2);

    // With dedup_radius_m = 1.5, distance 1.0 m merges
    assert_eq!(id1, id2);
    assert_eq!(store.len(), 1);
  }

  #[test]
  fn persistence_round_trip_saves_and_reloads_store() {
    let tmp = NamedTempFile::new().unwrap();
    let tmp_path = tmp.path().to_path_buf();

    let mut store = SpatialMemoryStore::open(&tmp_path).unwrap();
    let hit = RaycastHit {
      block_pos: BlockPosition::new(-5, 70, 15),
      face: BlockFace::East,
      block_id: "minecraft:oak_planks".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-100".to_string(),
      captured_at_millis: 5000,
    };
    store.upsert_from_raycast(&hit, &obs);
    store.save().unwrap();

    let reloaded = SpatialMemoryStore::open(&tmp_path).unwrap();
    assert_eq!(reloaded.len(), 1);
    let landmark = reloaded.get("lm--5-70-15-block_surface").unwrap();
    assert_eq!(landmark.position, BlockPosition::new(-5, 70, 15));
    assert_eq!(landmark.observations.len(), 1);
    assert_eq!(landmark.observation_count, 1);
    assert_eq!(landmark.surface_face, Some(BlockFace::East));
  }

  #[test]
  fn query_radius_returns_only_contained_landmarks() {
    let mut store = SpatialMemoryStore::open("memory.json").unwrap();
    let hit_inside = RaycastHit {
      block_pos: BlockPosition::new(0, 0, 0),
      face: BlockFace::Up,
      block_id: "minecraft:dirt".to_string(),
    };
    let hit_outside = RaycastHit {
      block_pos: BlockPosition::new(10, 0, 0),
      face: BlockFace::Up,
      block_id: "minecraft:dirt".to_string(),
    };
    let obs = ObservationRef {
      observation_id: "obs-0".to_string(),
      captured_at_millis: 1,
    };
    store.upsert_from_raycast(&hit_inside, &obs);
    store.upsert_from_raycast(&hit_outside, &obs);

    let inside = store.query_radius(BlockPosition::new(0, 0, 0), 5.0);
    assert_eq!(inside.len(), 1);
    assert_eq!(inside[0].position, BlockPosition::new(0, 0, 0));
  }
}
