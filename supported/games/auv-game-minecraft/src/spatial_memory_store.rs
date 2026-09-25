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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryStoreData {
  pub schema_version: u32,
  pub landmarks: HashMap<String, SpatialLandmark>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialMemoryStoreError {
  Io(String),
  Serialization(String),
}

impl fmt::Display for SpatialMemoryStoreError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Io(err) => write!(f, "I/O error in spatial memory store: {err}"),
      Self::Serialization(err) => write!(f, "serialization error in spatial memory store: {err}"),
    }
  }
}

impl std::error::Error for SpatialMemoryStoreError {}

#[derive(Clone, Debug, PartialEq)]
pub struct SpatialMemoryStore {
  landmarks: HashMap<String, SpatialLandmark>,
  path: PathBuf,
}

impl SpatialMemoryStore {
  pub fn open(path: impl AsRef<Path>) -> Result<Self, SpatialMemoryStoreError> {
    let path_buf = path.as_ref().to_path_buf();
    let is_empty_or_missing = !path_buf.exists() || path_buf.metadata().map(|m| m.len() == 0).unwrap_or(false);
    if is_empty_or_missing {
      return Ok(Self {
        landmarks: HashMap::new(),
        path: path_buf,
      });
    }

    let data: SpatialMemoryStoreData =
      read_json_file(&path_buf).map_err(|err| SpatialMemoryStoreError::Serialization(format!("{err:?}")))?;

    Ok(Self {
      landmarks: data.landmarks,
      path: path_buf,
    })
  }

  pub fn save(&self) -> Result<(), SpatialMemoryStoreError> {
    let data = SpatialMemoryStoreData {
      schema_version: SPATIAL_MEMORY_STORE_SCHEMA_VERSION,
      landmarks: self.landmarks.clone(),
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

  pub fn upsert_from_raycast(&mut self, hit: &RaycastHit, obs: &ObservationRef) -> String {
    let mut matching_id = None;
    for (id, landmark) in &self.landmarks {
      let dx = f64::from(landmark.position.x - hit.block_pos.x);
      let dy = f64::from(landmark.position.y - hit.block_pos.y);
      let dz = f64::from(landmark.position.z - hit.block_pos.z);
      let dist = (dx * dx + dy * dy + dz * dz).sqrt();
      if dist < 0.6 {
        matching_id = Some(id.clone());
        break;
      }
    }

    if let Some(id) = matching_id {
      let landmark = self.landmarks.get_mut(&id).expect("landmark exists");
      landmark.observations.push(LandmarkObservation {
        observation_ref: obs.clone(),
        source: LandmarkSource::TelemetryRaycast,
        hit_face: Some(hit.face),
        block_id: Some(hit.block_id.clone()),
      });
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
    assert_eq!(landmark.observations[0].observation_ref.observation_id, "obs-001");
    assert_eq!(landmark.observations[1].observation_ref.observation_id, "obs-002");
    assert_eq!(landmark.status, SpatialClaimStatus::Confirmed);
    assert_eq!(landmark.source, LandmarkSource::TelemetryRaycast);
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
