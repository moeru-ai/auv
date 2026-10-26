use super::*;

// The test binary links glibc, so it requires versioned glibc symbols.
#[test]
fn reads_glibc_requirements_of_a_linked_binary() {
  let executable = std::env::current_exe().unwrap();
  let versions = glibc_requirements(&executable).unwrap();
  assert!(!versions.is_empty());
  assert!(versions.iter().all(|version| version.starts_with("2.")), "{versions:?}");
}
