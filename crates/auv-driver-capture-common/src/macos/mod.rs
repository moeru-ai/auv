//! Existing macOS capture artifact contracts; their wire shapes are preserved.

pub type AuvResult<T> = Result<T, String>;

pub mod artifact;
pub mod geometry;
pub mod types;

/// A window's size and minimized state as its application reports them over
/// Accessibility.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowAxSize {
  pub size: auv_driver_common::Size,
  pub minimized: bool,
}

#[doc(hidden)]
pub mod error;

/// Bulk byte transfer from Swift: `RustVec<UInt8>` only offers per-element
/// `push`, one FFI call per byte.
///
/// NOTICE(swift-bridge-bulk-bytes): pushing an 81 MB Retina window capture
/// byte by byte took ~2.5 s of a ~4 s capture (release build, 2026-10-07).
/// swift-bridge 0.1.59 generates no bulk constructor for `RustVec`; remove this
/// once it does.
/// # Safety
/// A non-null `bytes` must point to `len` initialized bytes readable for this call.
pub unsafe fn byte_vec_from_raw(bytes: *const u8, len: usize) -> Vec<u8> {
  if bytes.is_null() || len == 0 {
    return Vec::new();
  }
  // SAFETY: Swift passes the base address and count of a live `[UInt8]`
  // buffer (`withUnsafeBufferPointer`) that outlives this call.
  unsafe { std::slice::from_raw_parts(bytes, len) }.to_vec()
}
