//! One JSON object per line: what the evaluation did, what the driver returned, and what the
//! desktop and the receiving application showed.

use std::io::Write;
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde_json::{Map, Value, json};

pub struct Records {
  file: std::io::BufWriter<std::fs::File>,
  started: Instant,
  run: String,
  scenario: &'static str,
  seq: u64,
}

impl Records {
  pub fn create(path: &Path, run: &str, scenario: &'static str) -> Result<Self> {
    Ok(Self {
      file: std::io::BufWriter::new(std::fs::File::create(path)?),
      started: Instant::now(),
      run: run.to_string(),
      scenario,
      seq: 0,
    })
  }

  /// Appends one record. `kind` says what it is (`setup`, `op`, `minute`, `summary`, ...).
  pub fn write(&mut self, kind: &str, fields: Value) -> Result<()> {
    let mut record = Map::new();
    record.insert("kind".into(), json!(kind));
    record.insert("run".into(), json!(self.run));
    record.insert("scenario".into(), json!(self.scenario));
    record.insert("seq".into(), json!(self.seq));
    record.insert("t_ms".into(), json!(self.started.elapsed().as_secs_f64() * 1000.0));
    record.insert("wall_ms".into(), json!(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64));
    if let Value::Object(fields) = fields {
      record.extend(fields);
    }
    self.seq += 1;
    writeln!(self.file, "{}", Value::Object(record))?;
    self.file.flush()?;
    Ok(())
  }
}

/// Linear-interpolated percentile of `values` (milliseconds), as the earlier Windows latency
/// evidence computes it.
pub fn percentile(values: &[f64], percent: f64) -> f64 {
  if values.is_empty() {
    return f64::NAN;
  }
  let mut sorted = values.to_vec();
  sorted.sort_by(|a, b| a.total_cmp(b));
  let rank = percent / 100.0 * (sorted.len() - 1) as f64;
  let (low, high) = (rank.floor() as usize, rank.ceil() as usize);
  sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
}

#[cfg(test)]
mod tests {
  use super::percentile;

  #[test]
  fn percentile_interpolates_between_ranks() {
    let values = [4.0, 1.0, 3.0, 2.0];
    assert_eq!(percentile(&values, 0.0), 1.0);
    assert_eq!(percentile(&values, 100.0), 4.0);
    assert_eq!(percentile(&values, 50.0), 2.5);
  }

  #[test]
  fn percentile_of_nothing_is_not_a_number() {
    assert!(percentile(&[], 50.0).is_nan());
  }
}
