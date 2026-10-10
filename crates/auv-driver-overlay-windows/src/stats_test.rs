use super::{MAX_SAMPLES, Samples};

#[test]
fn only_the_most_recent_samples_are_kept() {
  let mut samples = Samples::default();
  for value in 0..(MAX_SAMPLES + 10) {
    samples.push(value as f64);
  }
  let stats = samples.percentiles();
  assert_eq!(stats.count, MAX_SAMPLES);
  assert_eq!(stats.max, (MAX_SAMPLES + 9) as f64);
  assert!(stats.p50 > 10.0, "the oldest samples were dropped");
}
