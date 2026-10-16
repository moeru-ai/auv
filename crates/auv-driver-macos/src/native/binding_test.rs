use super::*;

#[test]
fn bulk_bytes_copy_the_whole_buffer() {
  let mut bytes: Vec<u8> = (0..=255).cycle().take(10_000).collect();
  let copied = native_byte_vec_from_raw(bytes.as_ptr(), bytes.len());
  assert_eq!(copied, bytes);
  bytes.fill(0);
  assert_eq!(copied[255], 255, "the result must own its bytes independently of the Swift buffer");
}

#[test]
fn bulk_bytes_from_an_empty_or_null_buffer_are_empty() {
  assert!(native_byte_vec_from_raw(std::ptr::null(), 4).is_empty());
  assert!(native_byte_vec_from_raw([1u8].as_ptr(), 0).is_empty());
}

#[test]
#[ignore = "opt-in local timing budget for a full-resolution pixel copy"]
fn capture_pixel_transfer_full_frame_stays_within_local_budget() {
  // Per-byte FFI pushes took seconds for a Retina frame. Exercise the shared
  // Rust copy here; this crate's measure_capture example covers the full Swift path.
  let source: Vec<u8> = (0..3024 * 1964 * 4).map(|index| (index % 256) as u8).collect();
  let start = std::time::Instant::now();
  let result = native_byte_vec_from_raw(source.as_ptr(), source.len());
  let elapsed = start.elapsed();
  eprintln!("full-frame pixel copy: {elapsed:?}");
  assert_eq!(result, source);
  assert!(elapsed < std::time::Duration::from_millis(500), "pixel copy exceeded local budget: {elapsed:?}");
}
