//! Adjacent-segment timeline (`scan-timeline-v0`).
//!
//! NOTICE(s9a-contract-revision): Builder emits N-1 adjacent segments when `len >= 2`;
//! S1-4b two-frame-only cap removed. Wire schema unchanged (`scan-timeline-v0`).

use serde::{Deserialize, Serialize};

use crate::motion::{MotionResult, estimate_viewport_motion_between};
use crate::reader::ScanFrameBundle;

pub const SCAN_TIMELINE_SCHEMA_VERSION: &str = "scan-timeline-v0";

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
