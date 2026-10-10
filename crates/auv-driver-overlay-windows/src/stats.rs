//! Bounded sample buffer behind the animator's `FrameStats`.

use std::collections::VecDeque;

use auv_driver_overlay_common::Percentiles;

/// Keeps the most recent samples; a long-lived animator must not grow without bound.
const MAX_SAMPLES: usize = 4096;

#[derive(Default)]
pub(crate) struct Samples {
  values: VecDeque<f64>,
}

impl Samples {
  pub(crate) fn push(&mut self, millis: f64) {
    if self.values.len() == MAX_SAMPLES {
      self.values.pop_front();
    }
    self.values.push_back(millis);
  }

  pub(crate) fn percentiles(&self) -> Percentiles {
    Percentiles::of(self.values.iter().copied())
  }
}

#[cfg(test)]
#[path = "stats_test.rs"]
mod tests;
