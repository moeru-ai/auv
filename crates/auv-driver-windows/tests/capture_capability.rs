#[cfg(feature = "latency-telemetry")]
#[test]
fn aggregate_and_capture_share_one_telemetry_buffer() {
  // ROOT CAUSE:
  //
  // Moving capture telemetry into another crate can split recording from its
  // consumer. The existing driver controls must still see backend events.
  use auv_driver_windows::latency;

  latency::clear_records();
  latency::start_recording();
  auv_driver_capture::windows::latency::record_latency_event("capture_window", 3.0, Some((20, 10)), Some("wgc.windows"), None);
  latency::record_latency_event("press_key", 1.0, None, Some("SendInput"), None);
  latency::stop_recording();

  let records = latency::take_records();
  assert_eq!(records.len(), 2);
  assert_eq!(records[0].path, "capture_window");
  assert_eq!(records[0].resolution, Some((20, 10)));
  assert_eq!(records[0].backend.as_deref(), Some("wgc.windows"));
  assert_eq!(records[1].path, "press_key");
  assert!(latency::take_records().is_empty());
}
