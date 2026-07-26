#[cfg(target_os = "macos")]
use super::decode_clipboard_snapshot;
#[cfg(target_os = "macos")]
use crate::native::binding::ffi::NativeClipboardSnapshotResponse;

#[cfg(target_os = "macos")]
#[test]
fn decode_clipboard_snapshot_includes_operation_name() {
  let error = decode_clipboard_snapshot(NativeClipboardSnapshotResponse {
    payload: None,
    error_message: Some("pasteboard denied".to_string()),
    recovery_hint: Some("retry after unlocking session".to_string()),
  })
  .unwrap_err();

  assert!(error.contains("capture_clipboard"));
  assert!(error.contains("pasteboard denied"));
}
