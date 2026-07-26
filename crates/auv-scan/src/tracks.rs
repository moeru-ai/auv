//! Adjacent-segment tracks (`scan-tracks-v0`).
//!
//! NOTICE(s9b-track-id): `track_id` in wire mirrors current adjacent label-based projection
//! (`track-{label}` per association.rs); **not** a stable cross-segment identity claim.
//! N-1 segments do not assert global track continuity or ID-switch policy.

use serde::{Deserialize, Serialize};

use crate::association::{AssociationResult, FrameObservation, associate_adjacent_frames};
use crate::reader::ScanFrameBundle;
use crate::timeline::DIAG_INSUFFICIENT_FRAMES;

pub const SCAN_TRACKS_SCHEMA_VERSION: &str = "scan-tracks-v0";

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
  pub associations: Vec<AssociationResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracksDiagnosticWire {
  pub code: String,
  pub message: String,
}

fn insufficient_frames_diagnostic(found: usize) -> TracksDiagnosticWire {
  TracksDiagnosticWire {
    code: DIAG_INSUFFICIENT_FRAMES.into(),
    message: format!("tracks requires at least two frames for adjacent segments, found {found}"),
  }
}

fn observations_frame_mismatch_diagnostic(frame_count: usize, observation_frame_count: usize) -> TracksDiagnosticWire {
  TracksDiagnosticWire {
    code: DIAG_OBSERVATIONS_FRAME_MISMATCH.into(),
    message: format!("observations_by_frame length {observation_frame_count} does not match frame count {frame_count}"),
  }
}

/// Build an adjacent multi-segment tracks wire from a frame bundle and per-frame observations.
///
/// Diagnostic precedence: insufficient frames first, then observations mismatch, else N-1 segments.
pub fn build_scan_tracks_from_bundle(bundle: &ScanFrameBundle, observations_by_frame: &[Vec<FrameObservation>]) -> ScanTracksWire {
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
      let associations = associate_adjacent_frames(&observations_by_frame[index], &observations_by_frame[index + 1]);
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

/// Structured text projection for tracks consumption (no IO).
pub fn format_scan_tracks_text(tracks: &ScanTracksWire) -> String {
  let mut lines = Vec::new();
  for segment in &tracks.segments {
    lines.push(format!(
      "[tracks.segment] from={} to={} from_index={} to_index={}",
      segment.from_frame_id, segment.to_frame_id, segment.from_sequence_index, segment.to_sequence_index,
    ));
    for association in &segment.associations {
      match association {
        AssociationResult::Linked {
          track_id,
          previous_observation_id,
          current_observation_id,
        } => lines.push(format!(
          "[tracks.association] status=linked track_id={track_id} previous_observation_id={previous_observation_id} current_observation_id={current_observation_id}"
        )),
        AssociationResult::NewTrack {
          track_id,
          current_observation_id,
        } => lines.push(format!(
          "[tracks.association] status=new_track track_id={track_id} current_observation_id={current_observation_id}"
        )),
        AssociationResult::AmbiguousAssociation {
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
    lines.push(format!("[tracks.diagnostic] code={} message={}", diagnostic.code, diagnostic.message));
  }
  lines.join("\n")
}
