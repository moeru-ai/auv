//! Measurements a live overlay keeps, so frame time and event latency are checkable
//! numbers instead of impressions.

/// Distribution of one measurement in milliseconds. Percentiles use linear
/// interpolation and no outlier filtering.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Percentiles {
  pub count: usize,
  pub p50: f64,
  pub p95: f64,
  pub max: f64,
}

impl Percentiles {
  pub fn of(values: impl IntoIterator<Item = f64>) -> Self {
    let mut sorted: Vec<f64> = values.into_iter().collect();
    if sorted.is_empty() {
      return Self::default();
    }
    sorted.sort_by(|a, b| a.total_cmp(b));
    let at = |fraction: f64| {
      let rank = fraction * (sorted.len() as f64 - 1.0);
      let low = rank.floor() as usize;
      let high = (low + 1).min(sorted.len() - 1);
      sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
    };
    Self {
      count: sorted.len(),
      p50: at(0.5),
      p95: at(0.95),
      max: sorted[sorted.len() - 1],
    }
  }
}

/// What a live overlay measured while it ran.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameStats {
  /// Frames drawn, excluding frames skipped because nothing changed.
  pub frames: u64,
  /// Animated frames that started more than 1.5 frame intervals after the previous one.
  pub late_frames: u64,
  pub present_failures: u64,
  pub last_failure: Option<String>,
  /// Cost of composing and presenting one frame.
  pub frame_ms: Percentiles,
  /// From the driver reporting an event to the frame that shows it being handed to the
  /// window system. The screen updates at the next compositor tick after this.
  pub event_latency_ms: Percentiles,
}

#[cfg(test)]
#[path = "stats_test.rs"]
mod tests;
