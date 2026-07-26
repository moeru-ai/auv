use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::types::MinecraftSpatialFrame;

/// Read only the newest well-formed frame from the tail of an append-only
/// telemetry JSONL file.
///
/// `read_latest_spatial_frame` preserves full-scan accounting for callers that
/// need total line counts. The MC-2 bridge does not consume those counters; it
/// only needs the freshest durable frame. For large live telemetry files, a
/// full scan turns one bridge invocation into an O(file size) CPU walk. This
/// tail reader instead walks backward from EOF until it finds the newest
/// well-formed non-empty line.
pub fn read_latest_spatial_frame_from_tail(path: &Path) -> Result<Option<MinecraftSpatialFrame>, String> {
  let mut file = std::fs::File::open(path).map_err(|error| format!("failed to open telemetry sample {}: {error}", path.display()))?;
  scan_latest_spatial_frame_from_tail(&mut file)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TailFrameWaitConfig {
  pub wait_budget_ms: u64,
  pub poll_interval_ms: u64,
}

impl TailFrameWaitConfig {
  pub const fn new(wait_budget_ms: u64, poll_interval_ms: u64) -> Self {
    Self {
      wait_budget_ms,
      poll_interval_ms,
    }
  }
}

pub fn read_latest_spatial_frame_newer_than(
  path: &Path,
  min_monotonic_timestamp_ms: u64,
  wait: TailFrameWaitConfig,
) -> Result<Option<MinecraftSpatialFrame>, String> {
  let deadline = Instant::now().checked_add(Duration::from_millis(wait.wait_budget_ms)).unwrap_or_else(Instant::now);

  loop {
    let frame = read_latest_spatial_frame_from_tail(path)?;
    if frame.as_ref().is_some_and(|frame| frame.monotonic_timestamp_ms > min_monotonic_timestamp_ms) {
      return Ok(frame);
    }

    let now = Instant::now();
    if now >= deadline {
      return Ok(frame);
    }

    let remaining = deadline.saturating_duration_since(now);
    let sleep_for = remaining.min(Duration::from_millis(wait.poll_interval_ms.max(1)));
    sleep(sleep_for);
  }
}

fn scan_latest_spatial_frame_from_tail<R: Read + Seek>(reader: &mut R) -> Result<Option<MinecraftSpatialFrame>, String> {
  const TAIL_CHUNK_BYTES: usize = 64 * 1024;

  let file_len = reader.seek(SeekFrom::End(0)).map_err(|error| format!("failed to seek telemetry sample tail: {error}"))?;
  if file_len == 0 {
    return Ok(None);
  }

  let mut position = file_len;
  let mut carry = Vec::new();
  let mut chunk = vec![0_u8; TAIL_CHUNK_BYTES];

  while position > 0 {
    let read_len =
      usize::try_from(position.min(TAIL_CHUNK_BYTES as u64)).map_err(|error| format!("telemetry chunk length overflow: {error}"))?;
    position -= read_len as u64;
    reader.seek(SeekFrom::Start(position)).map_err(|error| format!("failed to seek telemetry sample chunk: {error}"))?;
    reader.read_exact(&mut chunk[..read_len]).map_err(|error| format!("failed to read telemetry sample tail chunk: {error}"))?;

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
  let trimmed = std::str::from_utf8(bytes).map_err(|error| format!("telemetry sample tail is not valid UTF-8: {error}"))?.trim();
  if trimmed.is_empty() {
    return Ok(None);
  }
  Ok(serde_json::from_str::<MinecraftSpatialFrame>(trimmed).ok())
}

#[cfg(test)]
#[path = "ingest_test.rs"]
mod tests;
