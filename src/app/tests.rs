use std::fs;

use super::infra::resolve_probe_path;

#[test]
fn resolve_probe_path_accepts_probe_directory() {
  let root = std::env::temp_dir().join(format!("auv-app-probe-path-{}", crate::model::now_millis()));
  fs::create_dir_all(&root).expect("fixture directory");
  fs::write(root.join("probe.json"), "{}").expect("fixture probe");
  assert_eq!(resolve_probe_path(&root).expect("directory should resolve"), root.join("probe.json"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_probe_path_rejects_missing_path() {
  let path = std::env::temp_dir().join(format!("auv-app-probe-missing-{}", crate::model::now_millis()));
  assert!(resolve_probe_path(&path).expect_err("missing path").contains("does not exist"));
}
