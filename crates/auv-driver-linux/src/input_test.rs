use super::*;

#[test]
fn reserved_result_uses_shared_input_schema() {
  let result = reserved_input_result("not wired yet");

  assert_eq!(result.selected_path, InputDeliveryPath::Unsupported);
  assert_eq!(result.attempts.len(), 1);
}

#[test]
fn paste_text_returns_typed_input_action_result() {
  let _: fn(&Arc<Mutex<LinuxDriverSessionState>>, PasteTextOptions) -> DriverResult<InputActionResult> = paste_text;
}
