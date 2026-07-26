use super::inferred_header;

#[test]
fn header_uses_uppercase_words() {
  assert_eq!(inferred_header("install_dir"), "INSTALL DIR");
}
