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
