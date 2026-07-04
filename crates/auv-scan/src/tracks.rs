//! Adjacent-segment tracks (`scan-tracks-v0`) — crate-local directory artifact.
//!
//! NOTICE(s9b-artifact-boundary): directory-level artifact beside scan-frame-*.json;
//! not run-level; not scene_state product wire; not runtime-staged producer.
//!
//! NOTICE(s9b-track-id): `track_id` in wire mirrors current adjacent label-based projection
//! (`track-{label}` per association.rs); **not** a stable cross-segment identity claim.
//! N-1 segments do not assert global track continuity or ID-switch policy.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::association::{
  AssociationDiagnostic, AssociationResult, FrameObservation, associate_adjacent_frames,
};
use crate::reader::ScanFrameBundle;
use crate::timeline::DIAG_INSUFFICIENT_FRAMES;

pub const SCAN_TRACKS_SCHEMA_VERSION: &str = "scan-tracks-v0";
pub const SCAN_TRACKS_ARTIFACT_FILE_NAME: &str = "scan-tracks.json";

pub const DIAG_OBSERVATIONS_FRAME_MISMATCH: &str = "observations_frame_mismatch";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanTracksWire {
  pub schema_version: String,
  pub segments: Vec<TrackSegmentWire>,
  pub diagnostics: Vec<TracksDiagnosticWire>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSegmentWire {
  pub from_frame_id: String,
  pub to_frame_id: String,
  pub from_sequence_index: u32,
  pub to_sequence_index: u32,
  pub associations: Vec<AssociationResultWire>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracksDiagnosticWire {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociationDiagnosticWire {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AssociationResultWire {
  Linked {
    track_id: String,
    previous_observation_id: String,
    current_observation_id: String,
  },
  NewTrack {
    track_id: String,
    current_observation_id: String,
  },
  AmbiguousAssociation {
    label: String,
    candidate_observation_ids: Vec<String>,
    diagnostic: AssociationDiagnosticWire,
  },
}

#[derive(Debug, Error)]
pub enum TracksError {
  #[error("schema_version mismatch: expected {SCAN_TRACKS_SCHEMA_VERSION}, found {found}")]
  SchemaMismatch { found: String },
  #[error("missing required field: {0}")]
  MissingField(&'static str),
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

fn association_diagnostic_to_wire(diagnostic: AssociationDiagnostic) -> AssociationDiagnosticWire {
  AssociationDiagnosticWire {
    code: diagnostic.code,
    message: diagnostic.message,
  }
}

fn association_result_to_wire(result: AssociationResult) -> AssociationResultWire {
  match result {
    AssociationResult::Linked {
      track_id,
      previous_observation_id,
      current_observation_id,
    } => AssociationResultWire::Linked {
      track_id,
      previous_observation_id,
      current_observation_id,
    },
    AssociationResult::NewTrack {
      track_id,
      current_observation_id,
    } => AssociationResultWire::NewTrack {
      track_id,
      current_observation_id,
    },
    AssociationResult::AmbiguousAssociation {
      label,
      candidate_observation_ids,
      diagnostic,
    } => AssociationResultWire::AmbiguousAssociation {
      label,
      candidate_observation_ids,
      diagnostic: association_diagnostic_to_wire(diagnostic),
    },
  }
}

fn insufficient_frames_diagnostic(found: usize) -> TracksDiagnosticWire {
  TracksDiagnosticWire {
    code: DIAG_INSUFFICIENT_FRAMES.into(),
    message: format!("tracks requires at least two frames for adjacent segments, found {found}"),
  }
}

fn observations_frame_mismatch_diagnostic(
  frame_count: usize,
  observation_frame_count: usize,
) -> TracksDiagnosticWire {
  TracksDiagnosticWire {
    code: DIAG_OBSERVATIONS_FRAME_MISMATCH.into(),
    message: format!(
      "observations_by_frame length {observation_frame_count} does not match frame count {frame_count}"
    ),
  }
}

/// Build an adjacent multi-segment tracks wire from a frame bundle and per-frame observations.
///
/// Diagnostic precedence: insufficient frames first, then observations mismatch, else N-1 segments.
pub fn build_scan_tracks_from_bundle(
  bundle: &ScanFrameBundle,
  observations_by_frame: &[Vec<FrameObservation>],
) -> ScanTracksWire {
  let frame_count = bundle.frames.len();
  if frame_count < 2 {
    return ScanTracksWire {
      schema_version: SCAN_TRACKS_SCHEMA_VERSION.to_string(),
      segments: Vec::new(),
      diagnostics: vec![insufficient_frames_diagnostic(frame_count)],
    };
  }

  if observations_by_frame.len() != frame_count {
    return ScanTracksWire {
      schema_version: SCAN_TRACKS_SCHEMA_VERSION.to_string(),
      segments: Vec::new(),
      diagnostics: vec![observations_frame_mismatch_diagnostic(
        frame_count,
        observations_by_frame.len(),
      )],
    };
  }

  let segments = bundle
    .frames
    .windows(2)
    .enumerate()
    .map(|(index, window)| {
      let first = &window[0];
      let second = &window[1];
      let associations = associate_adjacent_frames(
        &observations_by_frame[index],
        &observations_by_frame[index + 1],
      )
      .into_iter()
      .map(association_result_to_wire)
      .collect();
      TrackSegmentWire {
        from_frame_id: first.frame_id.clone(),
        to_frame_id: second.frame_id.clone(),
        from_sequence_index: first.sequence_index,
        to_sequence_index: second.sequence_index,
        associations,
      }
    })
    .collect();

  ScanTracksWire {
    schema_version: SCAN_TRACKS_SCHEMA_VERSION.to_string(),
    segments,
    diagnostics: Vec::new(),
  }
}

pub fn write_tracks_artifact(dir: &Path, tracks: &ScanTracksWire) -> Result<PathBuf, TracksError> {
  if tracks.schema_version != SCAN_TRACKS_SCHEMA_VERSION {
    return Err(TracksError::SchemaMismatch {
      found: tracks.schema_version.clone(),
    });
  }
  fs::create_dir_all(dir)?;
  let path = dir.join(SCAN_TRACKS_ARTIFACT_FILE_NAME);
  let json = serde_json::to_string_pretty(tracks)?;
  let mut file = fs::File::create(&path)?;
  file.write_all(json.as_bytes())?;
  file.write_all(b"\n")?;
  Ok(path)
}

pub fn read_tracks_artifact(path: &Path) -> Result<ScanTracksWire, TracksError> {
  let bytes = fs::read(path)?;
  let value: serde_json::Value = serde_json::from_slice(&bytes)?;
  let Some(schema_version) = value.get("schema_version") else {
    return Err(TracksError::MissingField("schema_version"));
  };
  let Some(schema_version) = schema_version.as_str() else {
    return Err(TracksError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  };
  if schema_version != SCAN_TRACKS_SCHEMA_VERSION {
    return Err(TracksError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  }
  serde_json::from_value(value).map_err(TracksError::from)
}

/// Structured text projection for tracks consumption (no IO).
pub fn format_scan_tracks_text(tracks: &ScanTracksWire) -> String {
  let mut lines = Vec::new();
  for segment in &tracks.segments {
    lines.push(format!(
      "[tracks.segment] from={} to={} from_index={} to_index={}",
      segment.from_frame_id,
      segment.to_frame_id,
      segment.from_sequence_index,
      segment.to_sequence_index,
    ));
    for association in &segment.associations {
      match association {
        AssociationResultWire::Linked {
          track_id,
          previous_observation_id,
          current_observation_id,
        } => lines.push(format!(
          "[tracks.association] status=linked track_id={track_id} previous_observation_id={previous_observation_id} current_observation_id={current_observation_id}"
        )),
        AssociationResultWire::NewTrack {
          track_id,
          current_observation_id,
        } => lines.push(format!(
          "[tracks.association] status=new_track track_id={track_id} current_observation_id={current_observation_id}"
        )),
        AssociationResultWire::AmbiguousAssociation {
          label,
          candidate_observation_ids,
          diagnostic,
        } => lines.push(format!(
          "[tracks.association] status=ambiguous_association label={label} candidate_observation_ids=[{}] diagnostic_code={} diagnostic_message={}",
          candidate_observation_ids.join(","),
          diagnostic.code,
          diagnostic.message,
        )),
      }
    }
  }
  for diagnostic in &tracks.diagnostics {
    lines.push(format!(
      "[tracks.diagnostic] code={} message={}",
      diagnostic.code, diagnostic.message
    ));
  }
  lines.join("\n")
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;
  use std::sync::atomic::{AtomicU64, Ordering};

  use serde::Deserialize;

  use super::*;
  use crate::frame::{SCAN_FRAME_SCHEMA_VERSION, ScanBounds, ScanFrame, ScanImageRef};
  use crate::producer::produce_frames_from_fixture_dir;
  use crate::reader::load_scan_frames_from_dir;

  static TRACKS_TEST_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

  fn next_temp_dir(prefix: &str) -> PathBuf {
    let seq = TRACKS_TEST_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
      "auv-scan-{prefix}-{}-{}-{seq}",
      std::process::id(),
      prefix
    ))
  }

  fn tracks_fixture_dir(scenario: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/tracks")
      .join(scenario)
  }

  fn temporal_fixture_dir(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan")
      .join(relative)
  }

  #[derive(Debug, Deserialize)]
  struct ObservationFixture {
    observation_id: String,
    label: String,
  }

  #[derive(Debug, Deserialize)]
  struct TracksManifestSegment {
    associations: Vec<AssociationResultWire>,
  }

  #[derive(Debug, Deserialize)]
  struct TracksManifest {
    scenario: String,
    frame_fixture: String,
    observations_by_frame: Vec<Vec<ObservationFixture>>,
    segments: Vec<TracksManifestSegment>,
  }

  fn load_tracks_manifest(scenario: &str) -> TracksManifest {
    let path = tracks_fixture_dir(scenario).join("manifest.json");
    let text = fs::read_to_string(&path).expect("read manifest");
    serde_json::from_str(&text).expect("parse manifest")
  }

  fn observations_from_fixture(raw: &[Vec<ObservationFixture>]) -> Vec<Vec<FrameObservation>> {
    raw
      .iter()
      .map(|frame| {
        frame
          .iter()
          .map(|obs| FrameObservation {
            observation_id: obs.observation_id.clone(),
            label: obs.label.clone(),
          })
          .collect()
      })
      .collect()
  }

  fn sample_frame(frame_id: &str, sequence_index: u32) -> ScanFrame {
    ScanFrame {
      schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
      frame_id: frame_id.into(),
      sequence_index,
      captured_at_millis: 1_700_000_000_000 + u64::from(sequence_index) * 1000,
      window_bounds: ScanBounds {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
      },
      viewport_bounds: None,
      image: ScanImageRef {
        file_name: format!("{frame_id}.png"),
        width: 8,
        height: 8,
        media_type: "image/png".into(),
      },
    }
  }

  fn handbuilt_bundle(frames: Vec<ScanFrame>) -> ScanFrameBundle {
    ScanFrameBundle {
      frames,
      source_dir: PathBuf::from("/tmp"),
      loaded_json_paths: Vec::new(),
    }
  }

  fn produce_bundle_from_manifest(manifest: &TracksManifest) -> ScanFrameBundle {
    let fixture_dir = temporal_fixture_dir(&manifest.frame_fixture);
    let out_dir = next_temp_dir(&manifest.scenario);
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    load_scan_frames_from_dir(&out_dir).expect("load")
  }

  #[test]
  fn build_scan_tracks_matches_two_frame_linked_manifest() {
    let manifest = load_tracks_manifest("two_frame_linked_v0");
    let bundle = produce_bundle_from_manifest(&manifest);
    let observations = observations_from_fixture(&manifest.observations_by_frame);
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.diagnostics.is_empty());
    assert_eq!(tracks.segments.len(), 1);
    assert_eq!(manifest.segments.len(), 1);
    assert_eq!(
      tracks.segments[0].associations,
      manifest.segments[0].associations
    );
  }

  #[test]
  fn build_scan_tracks_matches_three_frame_linked_manifest() {
    let manifest = load_tracks_manifest("three_frame_linked_v0");
    let bundle = produce_bundle_from_manifest(&manifest);
    let observations = observations_from_fixture(&manifest.observations_by_frame);
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.diagnostics.is_empty());
    assert_eq!(tracks.segments.len(), 2);
    assert_eq!(manifest.segments.len(), 2);
    for (segment, expected) in tracks.segments.iter().zip(manifest.segments.iter()) {
      assert_eq!(segment.associations, expected.associations);
    }
  }

  #[test]
  fn build_scan_tracks_four_frame_handbuilt_smoke() {
    let bundle = handbuilt_bundle(vec![
      sample_frame("a", 0),
      sample_frame("b", 1),
      sample_frame("c", 2),
      sample_frame("d", 3),
    ]);
    let observations = vec![
      vec![FrameObservation {
        observation_id: "o0".into(),
        label: "widget".into(),
      }],
      vec![FrameObservation {
        observation_id: "o1".into(),
        label: "widget".into(),
      }],
      vec![FrameObservation {
        observation_id: "o2".into(),
        label: "widget".into(),
      }],
      vec![FrameObservation {
        observation_id: "o3".into(),
        label: "widget".into(),
      }],
    ];
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.diagnostics.is_empty());
    assert_eq!(tracks.segments.len(), 3);
  }

  #[test]
  fn build_scan_tracks_insufficient_frames() {
    let bundle = handbuilt_bundle(vec![sample_frame("only", 0)]);
    let observations = vec![vec![FrameObservation {
      observation_id: "o0".into(),
      label: "widget".into(),
    }]];
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.segments.is_empty());
    assert_eq!(tracks.diagnostics.len(), 1);
    assert_eq!(tracks.diagnostics[0].code, DIAG_INSUFFICIENT_FRAMES);
    let text = format_scan_tracks_text(&tracks);
    assert!(text.contains("[tracks.diagnostic]"));
    assert!(text.contains(DIAG_INSUFFICIENT_FRAMES));
  }

  #[test]
  fn build_scan_tracks_insufficient_frames_takes_priority_over_mismatch() {
    let bundle = handbuilt_bundle(vec![sample_frame("only", 0)]);
    let observations = vec![
      vec![FrameObservation {
        observation_id: "o0".into(),
        label: "widget".into(),
      }],
      vec![FrameObservation {
        observation_id: "o1".into(),
        label: "widget".into(),
      }],
    ];
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.segments.is_empty());
    assert_eq!(tracks.diagnostics.len(), 1);
    assert_eq!(tracks.diagnostics[0].code, DIAG_INSUFFICIENT_FRAMES);
    assert_ne!(tracks.diagnostics[0].code, DIAG_OBSERVATIONS_FRAME_MISMATCH);
  }

  #[test]
  fn build_scan_tracks_observations_frame_mismatch() {
    let bundle = handbuilt_bundle(vec![sample_frame("a", 0), sample_frame("b", 1)]);
    let observations = vec![vec![FrameObservation {
      observation_id: "o0".into(),
      label: "widget".into(),
    }]];
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.segments.is_empty());
    assert_eq!(tracks.diagnostics.len(), 1);
    assert_eq!(tracks.diagnostics[0].code, DIAG_OBSERVATIONS_FRAME_MISMATCH);
    let text = format_scan_tracks_text(&tracks);
    assert!(text.contains(DIAG_OBSERVATIONS_FRAME_MISMATCH));
  }

  #[test]
  fn build_scan_tracks_preserves_ambiguous_association_on_handbuilt_bundle() {
    let bundle = handbuilt_bundle(vec![sample_frame("frame-a", 0), sample_frame("frame-b", 1)]);
    let observations = vec![
      vec![
        FrameObservation {
          observation_id: "o0-a1".into(),
          label: "dup".into(),
        },
        FrameObservation {
          observation_id: "o0-a2".into(),
          label: "dup".into(),
        },
      ],
      vec![FrameObservation {
        observation_id: "o1-a".into(),
        label: "dup".into(),
      }],
    ];
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    assert!(tracks.diagnostics.is_empty());
    assert_eq!(tracks.segments.len(), 1);
    assert!(matches!(
      &tracks.segments[0].associations[0],
      AssociationResultWire::AmbiguousAssociation {
        label,
        diagnostic,
        ..
      } if label == "dup" && diagnostic.code == "ambiguous_association"
    ));
  }

  #[test]
  fn write_read_tracks_artifact_roundtrip() {
    let manifest = load_tracks_manifest("two_frame_linked_v0");
    let bundle = produce_bundle_from_manifest(&manifest);
    let observations = observations_from_fixture(&manifest.observations_by_frame);
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    let out_dir = next_temp_dir("tracks-roundtrip");
    let _ = fs::remove_dir_all(&out_dir);
    let written = write_tracks_artifact(&out_dir, &tracks).expect("write");
    let read_back = read_tracks_artifact(&written).expect("read");
    assert_eq!(read_back, tracks);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn read_tracks_artifact_rejects_unknown_schema_version() {
    let dir = next_temp_dir("tracks-bad-schema");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(SCAN_TRACKS_ARTIFACT_FILE_NAME);
    fs::write(
      &path,
      r#"{"schema_version":"scan-tracks-v99","segments":[],"diagnostics":[]}"#,
    )
    .unwrap();
    let err = read_tracks_artifact(&path).expect_err("schema");
    assert!(matches!(err, TracksError::SchemaMismatch { .. }));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn format_scan_tracks_text_includes_markers() {
    let manifest = load_tracks_manifest("two_frame_linked_v0");
    let bundle = produce_bundle_from_manifest(&manifest);
    let observations = observations_from_fixture(&manifest.observations_by_frame);
    let tracks = build_scan_tracks_from_bundle(&bundle, &observations);
    let text = format_scan_tracks_text(&tracks);
    assert!(text.contains("[tracks.segment]"));
    assert!(text.contains("[tracks.association]"));
    assert!(text.contains("status=linked"));
  }
}
