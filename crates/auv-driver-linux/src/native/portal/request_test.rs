use super::*;

#[test]
fn portal_token_is_object_path_component_friendly() {
  let token = portal_token("session");

  assert!(token.starts_with("auv_session_"));
  assert!(token.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
}
