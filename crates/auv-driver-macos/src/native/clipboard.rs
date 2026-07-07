// File: src/driver/macos/native/clipboard.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeActionResponse, NativeClipboardSnapshotResponse, capture_clipboard as native_capture_clipboard,
  restore_clipboard as native_restore_clipboard, set_clipboard_text as native_set_clipboard_text,
};
use super::types::AuvResult;

#[cfg(target_os = "macos")]
pub fn capture_clipboard_snapshot() -> AuvResult<String> {
  decode_clipboard_snapshot(native_capture_clipboard())
}

#[cfg(not(target_os = "macos"))]
pub fn capture_clipboard_snapshot() -> AuvResult<String> {
  Err("macOS native clipboard capture is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn restore_clipboard_snapshot(snapshot_payload: &str) -> AuvResult<()> {
  action_result("restore_clipboard", native_restore_clipboard(snapshot_payload.to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn restore_clipboard_snapshot(_snapshot_payload: &str) -> AuvResult<()> {
  Err("macOS native clipboard restore is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn set_clipboard_text(text: &str) -> AuvResult<()> {
  action_result("set_clipboard_text", native_set_clipboard_text(text.to_string()))
}

#[cfg(not(target_os = "macos"))]
pub fn set_clipboard_text(_text: &str) -> AuvResult<()> {
  Err("macOS native clipboard set text is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
fn decode_clipboard_snapshot(response: NativeClipboardSnapshotResponse) -> AuvResult<String> {
  super::error::native_result("capture_clipboard", response.payload, response.error_message, response.recovery_hint)
}

#[cfg(target_os = "macos")]
fn action_result(operation: &str, response: NativeActionResponse) -> AuvResult<()> {
  super::error::native_result(operation, response.ok.then_some(()), response.error_message, response.recovery_hint)
}

#[cfg(test)]
mod tests {
  #[cfg(target_os = "macos")]
  use super::decode_clipboard_snapshot;
  #[cfg(target_os = "macos")]
  use crate::native::types::NativeClipboardSnapshotResponse;

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
}
