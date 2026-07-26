use super::*;

#[test]
fn seek_duration_rejects_negative() {
  assert!(seek_duration_from_seconds(-1.0).is_err());
}

#[test]
fn seek_duration_rejects_nan() {
  assert!(seek_duration_from_seconds(f64::NAN).is_err());
}

#[test]
fn seek_duration_rejects_infinity() {
  assert!(seek_duration_from_seconds(f64::INFINITY).is_err());
}

#[test]
fn seek_duration_rejects_overflow_past_duration_max() {
  // `Duration::from_secs_f64` panics on values above `Duration::MAX`
  // (~1.84e19 seconds). 1e20 is comfortably past that and would have
  // panicked the CLI under the old code path.
  assert!(seek_duration_from_seconds(1e20).is_err());
}
