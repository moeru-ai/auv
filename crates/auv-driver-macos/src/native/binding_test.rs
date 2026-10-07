use super::*;

#[test]
fn bulk_bytes_copy_the_whole_buffer() {
  let bytes: Vec<u8> = (0..=255).cycle().take(10_000).collect();
  assert_eq!(native_byte_vec_from_raw(bytes.as_ptr(), bytes.len()), bytes);
}

#[test]
fn bulk_bytes_from_an_empty_or_null_buffer_are_empty() {
  assert!(native_byte_vec_from_raw(std::ptr::null(), 4).is_empty());
  assert!(native_byte_vec_from_raw([1u8].as_ptr(), 0).is_empty());
}
