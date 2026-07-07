//! Adjacent-segment timeline (`scan-timeline-v0`) — crate-local directory artifact.
//!
//! NOTICE(s9a-contract-revision): Builder emits N-1 adjacent segments when `len >= 2`;
//! S1-4b two-frame-only cap removed. Wire schema unchanged (`scan-timeline-v0`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::motion::{MotionResult, estimate_viewport_motion_between};
use crate::reader::ScanFrameBundle;

pub const SCAN_TIMELINE_SCHEMA_VERSION: &str = "scan-timeline-v0";
pub const SCAN_TIMELINE_ARTIFACT_FILE_NAME: &str = "scan-timeline.json";

pub const DIAG_INSUFFICIENT_FRAMES: &str = "insufficient_frames";
// NOTICE(s9a-legacy): S1-4b diagnostic code; builder no longer emits this (deprecated-by-production).
pub const DIAG_UNSUPPORTED_FRAME_COUNT: &str = "unsupported_frame_count";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScanTimelineWire {
  pub schema_version: String,
  pub segments: Vec<TimelineSegmentWire>,
  pub diagnostics: Vec<TimelineDiagnosticWire>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineSegmentWire {
  pub from_frame_id: String,
  pub to_frame_id: String,
  pub from_sequence_index: u32,
  pub to_sequence_index: u32,
  pub motion: TimelineMotionWire,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TimelineMotionWire {
  Estimated {
    delta_x: i64,
    delta_y: i64,
    confidence: f64,
  },
  Unknown {
    code: String,
    message: String,
  },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineDiagnosticWire {
  pub code: String,
  pub message: String,
}

#[derive(Debug, Error)]
pub enum TimelineError {
  #[error("schema_version mismatch: expected {SCAN_TIMELINE_SCHEMA_VERSION}, found {found}")]
  SchemaMismatch { found: String },
  #[error("missing required field: {0}")]
  MissingField(&'static str),
  #[error(transparent)]
  Io(#[from] std::io::Error),
  #[error("json parse error: {0}")]
  Json(#[from] serde_json::Error),
}

fn motion_to_wire(motion: MotionResult) -> TimelineMotionWire {
  match motion {
    MotionResult::Estimated(estimate) => TimelineMotionWire::Estimated {
      delta_x: estimate.delta_x,
      delta_y: estimate.delta_y,
      confidence: estimate.confidence,
    },
    MotionResult::Unknown(unknown) => TimelineMotionWire::Unknown {
      code: unknown.code,
      message: unknown.message,
    },
  }
}

fn insufficient_frames_diagnostic(found: usize) -> TimelineDiagnosticWire {
  TimelineDiagnosticWire {
    code: DIAG_INSUFFICIENT_FRAMES.into(),
    message: format!("timeline requires at least two frames for adjacent segments, found {found}"),
  }
}

/// Build an adjacent multi-segment timeline wire from a frame bundle (`N-1` segments when `N >= 2`).
pub fn build_scan_timeline_from_bundle(bundle: &ScanFrameBundle) -> ScanTimelineWire {
  let frame_count = bundle.frames.len();
  if frame_count < 2 {
    return ScanTimelineWire {
      schema_version: SCAN_TIMELINE_SCHEMA_VERSION.to_string(),
      segments: Vec::new(),
      diagnostics: vec![insufficient_frames_diagnostic(frame_count)],
    };
  }

  let segments = bundle
    .frames
    .windows(2)
    .map(|window| {
      let first = &window[0];
      let second = &window[1];
      TimelineSegmentWire {
        from_frame_id: first.frame_id.clone(),
        to_frame_id: second.frame_id.clone(),
        from_sequence_index: first.sequence_index,
        to_sequence_index: second.sequence_index,
        motion: motion_to_wire(estimate_viewport_motion_between(first, second)),
      }
    })
    .collect();

  ScanTimelineWire {
    schema_version: SCAN_TIMELINE_SCHEMA_VERSION.to_string(),
    segments,
    diagnostics: Vec::new(),
  }
}

pub fn write_timeline_artifact(dir: &Path, timeline: &ScanTimelineWire) -> Result<PathBuf, TimelineError> {
  if timeline.schema_version != SCAN_TIMELINE_SCHEMA_VERSION {
    return Err(TimelineError::SchemaMismatch {
      found: timeline.schema_version.clone(),
    });
  }
  fs::create_dir_all(dir)?;
  let path = dir.join(SCAN_TIMELINE_ARTIFACT_FILE_NAME);
  let json = serde_json::to_string_pretty(timeline)?;
  let mut file = fs::File::create(&path)?;
  file.write_all(json.as_bytes())?;
  file.write_all(b"\n")?;
  Ok(path)
}

pub fn read_timeline_artifact(path: &Path) -> Result<ScanTimelineWire, TimelineError> {
  let bytes = fs::read(path)?;
  let value: serde_json::Value = serde_json::from_slice(&bytes)?;
  let Some(schema_version) = value.get("schema_version") else {
    return Err(TimelineError::MissingField("schema_version"));
  };
  let Some(schema_version) = schema_version.as_str() else {
    return Err(TimelineError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  };
  if schema_version != SCAN_TIMELINE_SCHEMA_VERSION {
    return Err(TimelineError::SchemaMismatch {
      found: schema_version.to_string(),
    });
  }
  serde_json::from_value(value).map_err(TimelineError::from)
}

/// Structured text projection for timeline consumption (no IO).
pub fn format_scan_timeline_text(timeline: &ScanTimelineWire) -> String {
  let mut lines = Vec::new();
  for segment in &timeline.segments {
    lines.push(format!(
      "[timeline.segment] from={} to={} from_index={} to_index={}",
      segment.from_frame_id, segment.to_frame_id, segment.from_sequence_index, segment.to_sequence_index,
    ));
    match &segment.motion {
      TimelineMotionWire::Estimated {
        delta_x,
        delta_y,
        confidence,
      } => lines.push(format!("[timeline.motion] status=estimated delta_x={delta_x} delta_y={delta_y} confidence={confidence}")),
      TimelineMotionWire::Unknown { code, message } => lines.push(format!("[timeline.motion] status=unknown code={code} message={message}")),
    }
  }
  for diagnostic in &timeline.diagnostics {
    lines.push(format!("[timeline.diagnostic] code={} message={}", diagnostic.code, diagnostic.message));
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
  use crate::reader::{ScanFrameBundle, ScanInspectError, load_scan_frames_from_dir};

  static TIMELINE_TEST_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

  fn next_temp_dir(prefix: &str) -> PathBuf {
    let seq = TIMELINE_TEST_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("auv-scan-{prefix}-{}-{}-{seq}", std::process::id(), prefix))
  }

  fn two_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0")
  }

  fn three_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/three_frame_v0")
  }

  #[derive(Debug, Deserialize)]
  struct TwoFrameManifestMotion {
    delta_x: i64,
    delta_y: i64,
    confidence: f64,
  }

  #[derive(Debug, Deserialize)]
  struct TwoFrameManifest {
    motion: TwoFrameManifestMotion,
  }

  #[derive(Debug, Deserialize)]
  struct ThreeFrameManifestSegment {
    delta_x: i64,
    delta_y: i64,
    confidence: f64,
  }

  #[derive(Debug, Deserialize)]
  struct ThreeFrameManifest {
    segments: Vec<ThreeFrameManifestSegment>,
  }

  fn sample_frame(frame_id: &str, sequence_index: u32, y: i64) -> ScanFrame {
    ScanFrame {
      schema_version: SCAN_FRAME_SCHEMA_VERSION.to_string(),
      frame_id: frame_id.into(),
      sequence_index,
      captured_at_millis: 1_700_000_000_000 + u64::from(sequence_index) * 1000,
      window_bounds: ScanBounds {
        x: 0,
        y,
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

  #[test]
  fn build_scan_timeline_matches_two_frame_manifest() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = next_temp_dir("timeline-manifest");
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.diagnostics.is_empty());
    assert_eq!(timeline.segments.len(), 1);

    let manifest_path = fixture_dir.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: TwoFrameManifest = serde_json::from_str(&manifest_text).expect("parse manifest");

    match &timeline.segments[0].motion {
      TimelineMotionWire::Estimated {
        delta_x,
        delta_y,
        confidence,
      } => {
        assert_eq!(*delta_x, manifest.motion.delta_x);
        assert_eq!(*delta_y, manifest.motion.delta_y);
        assert_eq!(*confidence, manifest.motion.confidence);
      }
      other => panic!("expected estimated motion, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_scan_timeline_matches_three_frame_manifest() {
    let fixture_dir = three_frame_fixture_dir();
    let out_dir = next_temp_dir("timeline-three-manifest");
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.diagnostics.is_empty());
    assert_eq!(timeline.segments.len(), 2);

    let manifest_path = fixture_dir.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: ThreeFrameManifest = serde_json::from_str(&manifest_text).expect("parse manifest");
    assert_eq!(manifest.segments.len(), 2);

    for (segment, expected) in timeline.segments.iter().zip(manifest.segments.iter()) {
      match &segment.motion {
        TimelineMotionWire::Estimated {
          delta_x,
          delta_y,
          confidence,
        } => {
          assert_eq!(*delta_x, expected.delta_x);
          assert_eq!(*delta_y, expected.delta_y);
          assert_eq!(*confidence, expected.confidence);
        }
        other => panic!("expected estimated motion, got {other:?}"),
      }
    }
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn build_scan_timeline_four_frame_handbuilt_smoke() {
    let bundle = handbuilt_bundle(vec![
      sample_frame("a", 0, 0),
      sample_frame("b", 1, 12),
      sample_frame("c", 2, 20),
      sample_frame("d", 3, 28),
    ]);
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.diagnostics.is_empty());
    assert_eq!(timeline.segments.len(), 3);
  }

  #[test]
  fn write_read_timeline_artifact_roundtrip() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = next_temp_dir("timeline-roundtrip");
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let timeline = build_scan_timeline_from_bundle(&bundle);
    let written = write_timeline_artifact(&out_dir, &timeline).expect("write");
    let read_back = read_timeline_artifact(&written).expect("read");
    assert_eq!(read_back, timeline);
    let _ = fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn read_timeline_artifact_rejects_unknown_schema_version() {
    let dir = next_temp_dir("timeline-bad-schema");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(SCAN_TIMELINE_ARTIFACT_FILE_NAME);
    fs::write(&path, r#"{"schema_version":"scan-timeline-v99","segments":[],"diagnostics":[]}"#).unwrap();
    let err = read_timeline_artifact(&path).expect_err("schema");
    assert!(matches!(err, TimelineError::SchemaMismatch { .. }));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn write_timeline_artifact_allows_empty_segments_with_diagnostics() {
    let dir = next_temp_dir("timeline-empty-segments");
    let _ = fs::remove_dir_all(&dir);
    let bundle = handbuilt_bundle(vec![sample_frame("only", 0, 0)]);
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.segments.is_empty());
    assert_eq!(timeline.diagnostics.len(), 1);
    assert_eq!(timeline.diagnostics[0].code, DIAG_INSUFFICIENT_FRAMES);

    let written = write_timeline_artifact(&dir, &timeline).expect("write");
    let read_back = read_timeline_artifact(&written).expect("read");
    assert_eq!(read_back, timeline);

    let three_frame_bundle = handbuilt_bundle(vec![
      sample_frame("a", 0, 0),
      sample_frame("b", 1, 1),
      sample_frame("c", 2, 2),
    ]);
    let timeline_three = build_scan_timeline_from_bundle(&three_frame_bundle);
    assert_eq!(timeline_three.segments.len(), 2);
    assert!(timeline_three.diagnostics.is_empty());
    let written_three = write_timeline_artifact(&dir, &timeline_three).expect("write three");
    let read_three = read_timeline_artifact(&written_three).expect("read three");
    assert_eq!(read_three, timeline_three);
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn build_scan_timeline_insufficient_frames() {
    let bundle = handbuilt_bundle(vec![sample_frame("only", 0, 0)]);
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.segments.is_empty());
    assert_eq!(timeline.diagnostics[0].code, DIAG_INSUFFICIENT_FRAMES);
    let text = format_scan_timeline_text(&timeline);
    assert!(text.contains("[timeline.diagnostic]"));
    assert!(text.contains(DIAG_INSUFFICIENT_FRAMES));
  }

  #[test]
  fn load_scan_frames_rejects_duplicate_sequence_index_in_directory() {
    let dir = next_temp_dir("timeline-duplicate-reader");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let frame_a = sample_frame("frame-a", 0, 0);
    let frame_b = sample_frame("frame-b", 0, 12);
    let json_a = serde_json::to_string_pretty(&frame_a).unwrap();
    let json_b = serde_json::to_string_pretty(&frame_b).unwrap();
    fs::write(dir.join("scan-frame-0001.json"), json_a).unwrap();
    fs::write(dir.join("scan-frame-0002.json"), json_b).unwrap();
    let err = load_scan_frames_from_dir(&dir).expect_err("duplicate sequence_index");
    assert!(matches!(err, ScanInspectError::DuplicateSequenceIndex { .. }));
    let _ = fs::remove_dir_all(&dir);
  }

  #[test]
  fn build_scan_timeline_preserves_motion_unknown_on_handbuilt_bundle() {
    let bundle = handbuilt_bundle(vec![
      sample_frame("frame-a", 1, 0),
      sample_frame("frame-b", 0, 12),
    ]);
    let timeline = build_scan_timeline_from_bundle(&bundle);
    assert!(timeline.diagnostics.is_empty());
    assert_eq!(timeline.segments.len(), 1);
    match &timeline.segments[0].motion {
      TimelineMotionWire::Unknown { code, .. } => assert_eq!(code, "motion_unknown"),
      other => panic!("expected unknown motion, got {other:?}"),
    }
  }

  #[test]
  fn format_scan_timeline_text_includes_markers() {
    let fixture_dir = two_frame_fixture_dir();
    let out_dir = next_temp_dir("timeline-text");
    let _ = fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let timeline = build_scan_timeline_from_bundle(&bundle);
    let text = format_scan_timeline_text(&timeline);
    assert!(text.contains("[timeline.segment]"));
    assert!(text.contains("[timeline.motion]"));
    assert!(text.contains("delta_y=12"));
    let _ = fs::remove_dir_all(&out_dir);
  }
}
