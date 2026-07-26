use super::*;

#[test]
fn text_mime_matches_portal_plain_text_contract() {
  assert_eq!(TEXT_MIME, "text/plain;charset=utf-8");
}
