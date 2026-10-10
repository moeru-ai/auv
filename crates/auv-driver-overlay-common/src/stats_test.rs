use super::Percentiles;

#[test]
fn no_samples_report_zeroes() {
  assert_eq!(Percentiles::of([]), Percentiles::default());
}

#[test]
fn percentiles_interpolate_between_ranks_without_filtering_outliers() {
  let stats = Percentiles::of([4.0, 1.0, 2.0, 3.0, 100.0]);
  assert_eq!(stats.count, 5);
  assert_eq!(stats.p50, 3.0);
  assert!((stats.p95 - 80.8).abs() < 1e-9, "rank 3.8 lies between 4 and 100, got {}", stats.p95);
  assert_eq!(stats.max, 100.0);
}
