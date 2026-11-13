//! Latency telemetry instrumentation for Windows driver critical paths.
//!
//! Provides zero-overhead (compile-time gated) execution timing for:
//! - `capture_display` (xcap/GDI and Windows.Graphics.Capture full-screen capture)
//! - `capture_window` (PrintWindow and Windows.Graphics.Capture window capture)
//! - Input injection: `click_at`, `press_key`, `scroll_at` (SendInput injection latency only, settle excluded)
//! - `recognize_text_in_capture` (system OCR on sub-regions)
//!
//! When the `latency-telemetry` feature is disabled, all functions compile to
//! zero-cost no-op inline routines without any memory or instruction overhead.

#[cfg(feature = "latency-telemetry")]
use std::sync::Mutex;
#[cfg(feature = "latency-telemetry")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "latency-telemetry")]
use std::time::{SystemTime, UNIX_EPOCH};

/// Structured record of a single driver path execution latency.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LatencyRecord {
  /// Unix epoch timestamp in milliseconds when the call completed.
  pub timestamp_unix_ms: u64,
  /// Driver path identifier (e.g. "capture_display", "capture_window", "click_at", "press_key", "scroll_at", "recognize_text_in_capture").
  pub path: String,
  /// Execution latency in milliseconds (high precision float).
  pub latency_ms: f64,
  /// Target or captured resolution (width, height) if applicable.
  pub resolution: Option<(u32, u32)>,
  /// Underlying platform backend tag (e.g. "xcap.windows", "printwindow.windows", "SendInput", "Windows.Media.Ocr").
  pub backend: Option<String>,
  /// Additional context (e.g. target window title, key chord, button, or selector).
  pub details: Option<String>,
}

#[cfg(feature = "latency-telemetry")]
static TELEMETRY_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "latency-telemetry")]
static TELEMETRY_BUFFER: Mutex<Vec<LatencyRecord>> = Mutex::new(Vec::new());

#[cfg(feature = "latency-telemetry")]
pub fn start_recording() {
  TELEMETRY_ACTIVE.store(true, Ordering::SeqCst);
}

#[cfg(feature = "latency-telemetry")]
pub fn stop_recording() {
  TELEMETRY_ACTIVE.store(false, Ordering::SeqCst);
}

#[cfg(feature = "latency-telemetry")]
#[inline]
pub fn is_recording() -> bool {
  TELEMETRY_ACTIVE.load(Ordering::SeqCst)
}

#[cfg(feature = "latency-telemetry")]
pub fn clear_records() {
  if let Ok(mut buf) = TELEMETRY_BUFFER.lock() {
    buf.clear();
  }
}

#[cfg(feature = "latency-telemetry")]
pub fn take_records() -> Vec<LatencyRecord> {
  if let Ok(mut buf) = TELEMETRY_BUFFER.lock() {
    std::mem::take(&mut *buf)
  } else {
    Vec::new()
  }
}

#[cfg(feature = "latency-telemetry")]
pub fn record_latency_event(path: &str, latency_ms: f64, resolution: Option<(u32, u32)>, backend: Option<&str>, details: Option<&str>) {
  if !is_recording() {
    return;
  }
  let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

  let record = LatencyRecord {
    timestamp_unix_ms: now,
    path: path.to_string(),
    latency_ms,
    resolution,
    backend: backend.map(|s| s.to_string()),
    details: details.map(|s| s.to_string()),
  };

  if let Ok(mut buf) = TELEMETRY_BUFFER.lock() {
    buf.push(record);
  }
}

pub fn save_records_to_jsonl(records: &[LatencyRecord], path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
  use std::io::Write;
  if let Some(parent) = path.as_ref().parent() {
    std::fs::create_dir_all(parent)?;
  }
  let mut file = std::fs::File::create(path)?;
  for record in records {
    let line = serde_json::to_string(record).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    writeln!(file, "{}", line)?;
  }
  Ok(())
}

// ============================================================================
// Zero-overhead stubs when latency-telemetry feature is disabled
// ============================================================================

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn start_recording() {}

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn stop_recording() {}

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn is_recording() -> bool {
  false
}

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn clear_records() {}

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn take_records() -> Vec<LatencyRecord> {
  Vec::new()
}

#[cfg(not(feature = "latency-telemetry"))]
#[inline(always)]
pub fn record_latency_event(_path: &str, _latency_ms: f64, _resolution: Option<(u32, u32)>, _backend: Option<&str>, _details: Option<&str>) {
}
