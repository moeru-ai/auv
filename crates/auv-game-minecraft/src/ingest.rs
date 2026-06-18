use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::types::MinecraftSpatialFrame;

/// Outcome of scanning an append-only telemetry stream for its most recent frame.
#[derive(Clone, Debug, PartialEq)]
pub struct LatestFrameScan {
  /// The most recent successfully parsed frame, if any non-empty line parsed.
  pub frame: Option<MinecraftSpatialFrame>,
  /// Total non-empty lines observed.
  pub line_count: u64,
  /// Non-empty lines that failed to parse as a `MinecraftSpatialFrame`.
  pub malformed_line_count: u64,
}

impl LatestFrameScan {
  fn empty() -> Self {
    Self {
      frame: None,
      line_count: 0,
      malformed_line_count: 0,
    }
  }
}

/// Read the most recent `MinecraftSpatialFrame` from an append-only telemetry
/// JSONL file without loading the whole file into memory.
///
/// The sidecar writes one frame per line, oldest first. Readers consume only
/// flushed durable records, so the freshest binding candidate is the last
/// well-formed line. This streams line by line and retains only the latest
/// parsed frame, so a multi-hundred-megabyte sample costs one line of peak
/// memory rather than the whole file.
#[deprecated(
  note = "live MC-2 bridge must use read_latest_spatial_frame_from_tail; the full-scan variant exists only for tests/imports"
)]
pub fn read_latest_spatial_frame(path: &Path) -> Result<LatestFrameScan, String> {
  let file = std::fs::File::open(path).map_err(|error| {
    format!(
      "failed to open telemetry sample {}: {error}",
      path.display()
    )
  })?;
  scan_latest_spatial_frame(file)
}

/// Read only the newest well-formed frame from the tail of an append-only
/// telemetry JSONL file.
///
/// `read_latest_spatial_frame` preserves full-scan accounting for callers that
/// need total line counts. The MC-2 bridge does not consume those counters; it
/// only needs the freshest durable frame. For large live telemetry files, a
/// full scan turns one bridge invocation into an O(file size) CPU walk. This
/// tail reader instead walks backward from EOF until it finds the newest
/// well-formed non-empty line.
pub fn read_latest_spatial_frame_from_tail(
  path: &Path,
) -> Result<Option<MinecraftSpatialFrame>, String> {
  let mut file = std::fs::File::open(path).map_err(|error| {
    format!(
      "failed to open telemetry sample {}: {error}",
      path.display()
    )
  })?;
  scan_latest_spatial_frame_from_tail(&mut file)
}

/// Core scan over any byte reader. Separated from file opening so the binding
/// logic is unit-testable without touching the filesystem.
#[deprecated(
  note = "live MC-2 bridge must use read_latest_spatial_frame_from_tail; the full-scan variant exists only for tests/imports"
)]
pub fn scan_latest_spatial_frame<R: Read>(reader: R) -> Result<LatestFrameScan, String> {
  let mut buffered = BufReader::new(reader);
  let mut scan = LatestFrameScan::empty();
  let mut line = String::new();

  loop {
    line.clear();
    let read = buffered
      .read_line(&mut line)
      .map_err(|error| format!("failed to read telemetry sample line: {error}"))?;
    if read == 0 {
      break;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
      continue;
    }
    scan.line_count += 1;
    match serde_json::from_str::<MinecraftSpatialFrame>(trimmed) {
      Ok(frame) => scan.frame = Some(frame),
      Err(_) => scan.malformed_line_count += 1,
    }
  }

  Ok(scan)
}

fn scan_latest_spatial_frame_from_tail<R: Read + Seek>(
  reader: &mut R,
) -> Result<Option<MinecraftSpatialFrame>, String> {
  const TAIL_CHUNK_BYTES: usize = 64 * 1024;

  let file_len = reader
    .seek(SeekFrom::End(0))
    .map_err(|error| format!("failed to seek telemetry sample tail: {error}"))?;
  if file_len == 0 {
    return Ok(None);
  }

  let mut position = file_len;
  let mut carry = Vec::new();
  let mut chunk = vec![0_u8; TAIL_CHUNK_BYTES];

  while position > 0 {
    let read_len = usize::try_from(position.min(TAIL_CHUNK_BYTES as u64))
      .map_err(|error| format!("telemetry chunk length overflow: {error}"))?;
    position -= read_len as u64;
    reader
      .seek(SeekFrom::Start(position))
      .map_err(|error| format!("failed to seek telemetry sample chunk: {error}"))?;
    reader
      .read_exact(&mut chunk[..read_len])
      .map_err(|error| format!("failed to read telemetry sample tail chunk: {error}"))?;

    let mut combined = Vec::with_capacity(read_len + carry.len());
    combined.extend_from_slice(&chunk[..read_len]);
    combined.extend_from_slice(&carry);

    let mut line_end = combined.len();
    let mut prefix_end = line_end;
    for index in (0..combined.len()).rev() {
      if combined[index] != b'\n' {
        continue;
      }

      let line = &combined[index + 1..line_end];
      if let Some(frame) = parse_frame_line(line)? {
        return Ok(Some(frame));
      }
      prefix_end = index;
      line_end = index;
    }

    carry = combined[..prefix_end].to_vec();
  }

  parse_frame_line(&carry)
}

fn parse_frame_line(bytes: &[u8]) -> Result<Option<MinecraftSpatialFrame>, String> {
  let trimmed = std::str::from_utf8(bytes)
    .map_err(|error| format!("telemetry sample tail is not valid UTF-8: {error}"))?
    .trim();
  if trimmed.is_empty() {
    return Ok(None);
  }
  Ok(serde_json::from_str::<MinecraftSpatialFrame>(trimmed).ok())
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use super::*;
  use crate::types::{BlockPosition, NearbyBlock, PlayerPose, Vec3, Viewport};

  fn frame_line(id: &str, tick: u64, ts: u64) -> String {
    let frame = MinecraftSpatialFrame {
      spatial_frame_id: id.to_string(),
      world_tick: tick,
      monotonic_timestamp_ms: ts,
      viewport: Viewport::new(1708, 960),
      view_matrix: [0.0; 16],
      projection_matrix: [0.0; 16],
      player_pose: PlayerPose {
        eye_position: Vec3::new(-3.5, 70.62, -9.5),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    };
    serde_json::to_string(&frame).expect("frame serializes")
  }

  fn oversized_frame_line(id: &str, tick: u64, ts: u64, block_count: usize) -> String {
    let mut frame = MinecraftSpatialFrame {
      spatial_frame_id: id.to_string(),
      world_tick: tick,
      monotonic_timestamp_ms: ts,
      viewport: Viewport::new(1708, 960),
      view_matrix: [0.0; 16],
      projection_matrix: [0.0; 16],
      player_pose: PlayerPose {
        eye_position: Vec3::new(-3.5, 70.62, -9.5),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    };
    frame.nearby_blocks = (0..block_count)
      .map(|index| NearbyBlock {
        block_pos: BlockPosition::new(index as i32, 70, -9),
        block_id: "minecraft:stone".to_string(),
      })
      .collect();
    serde_json::to_string(&frame).expect("oversized frame serializes")
  }

  #[test]
  fn returns_last_frame_from_multiple_lines() {
    let body = format!(
      "{}\n{}\n{}\n",
      frame_line("frame-1", 1, 1000),
      frame_line("frame-2", 2, 2000),
      frame_line("frame-3", 3, 3000),
    );
    let scan = scan_latest_spatial_frame(body.as_bytes()).expect("scan succeeds");
    assert_eq!(scan.line_count, 3);
    assert_eq!(scan.malformed_line_count, 0);
    let frame = scan.frame.expect("a frame is present");
    assert_eq!(frame.spatial_frame_id, "frame-3");
    assert_eq!(frame.world_tick, 3);
    assert_eq!(frame.monotonic_timestamp_ms, 3000);
  }

  #[test]
  fn skips_blank_lines_without_counting_them() {
    let body = format!(
      "\n{}\n   \n{}\n\n",
      frame_line("a", 1, 10),
      frame_line("b", 2, 20)
    );
    let scan = scan_latest_spatial_frame(body.as_bytes()).expect("scan succeeds");
    assert_eq!(scan.line_count, 2);
    assert_eq!(scan.malformed_line_count, 0);
    assert_eq!(scan.frame.expect("frame").spatial_frame_id, "b");
  }

  #[test]
  fn counts_malformed_lines_and_keeps_last_valid_frame() {
    let body = format!(
      "{}\nnot json\n{}\n{{\"partial\":true}}\n",
      frame_line("valid-1", 1, 10),
      frame_line("valid-2", 2, 20),
    );
    let scan = scan_latest_spatial_frame(body.as_bytes()).expect("scan succeeds");
    assert_eq!(scan.line_count, 4);
    assert_eq!(scan.malformed_line_count, 2);
    assert_eq!(scan.frame.expect("frame").spatial_frame_id, "valid-2");
  }

  #[test]
  fn empty_stream_yields_no_frame() {
    let scan = scan_latest_spatial_frame("".as_bytes()).expect("scan succeeds");
    assert_eq!(scan.line_count, 0);
    assert_eq!(scan.malformed_line_count, 0);
    assert!(scan.frame.is_none());
  }

  #[test]
  fn all_malformed_yields_no_frame_but_counts_lines() {
    let scan = scan_latest_spatial_frame("nope\nstill nope\n".as_bytes()).expect("scan succeeds");
    assert_eq!(scan.line_count, 2);
    assert_eq!(scan.malformed_line_count, 2);
    assert!(scan.frame.is_none());
  }

  #[test]
  fn tail_scan_returns_last_valid_frame() {
    let body = format!(
      "{}\n{}\n{}\n",
      frame_line("frame-1", 1, 1000),
      frame_line("frame-2", 2, 2000),
      frame_line("frame-3", 3, 3000),
    );
    let mut cursor = Cursor::new(body.into_bytes());

    let frame = scan_latest_spatial_frame_from_tail(&mut cursor)
      .expect("tail scan succeeds")
      .expect("frame is present");

    assert_eq!(frame.spatial_frame_id, "frame-3");
    assert_eq!(frame.world_tick, 3);
    assert_eq!(frame.monotonic_timestamp_ms, 3000);
  }

  #[test]
  fn tail_scan_skips_trailing_blank_and_malformed_lines() {
    let body = format!(
      "{}\n{}\nnot json\n   \n",
      frame_line("valid-1", 1, 1000),
      frame_line("valid-2", 2, 2000),
    );
    let mut cursor = Cursor::new(body.into_bytes());

    let frame = scan_latest_spatial_frame_from_tail(&mut cursor)
      .expect("tail scan succeeds")
      .expect("frame is present");

    assert_eq!(frame.spatial_frame_id, "valid-2");
    assert_eq!(frame.world_tick, 2);
  }

  #[test]
  fn tail_scan_handles_line_larger_than_chunk() {
    let big = oversized_frame_line("frame-big", 9, 9000, 2500);
    assert!(big.len() > 64 * 1024);
    let body = format!("{}\n{}\n", frame_line("frame-1", 1, 1000), big);
    let mut cursor = Cursor::new(body.into_bytes());

    let frame = scan_latest_spatial_frame_from_tail(&mut cursor)
      .expect("tail scan succeeds")
      .expect("frame is present");

    assert_eq!(frame.spatial_frame_id, "frame-big");
    assert_eq!(frame.world_tick, 9);
    assert_eq!(frame.monotonic_timestamp_ms, 9000);
  }
}
