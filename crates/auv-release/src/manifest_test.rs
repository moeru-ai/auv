use super::*;

#[test]
fn file_entries_hash_every_file_but_the_manifest() {
  let root = tempfile::tempdir().unwrap();
  std::fs::create_dir_all(root.path().join("lib/auv")).unwrap();
  std::fs::write(root.path().join("auv"), b"abc").unwrap();
  std::fs::write(root.path().join("lib/auv/libx.so.1"), b"").unwrap();
  std::fs::write(root.path().join(layout::MANIFEST), b"{}").unwrap();

  let entries = file_entries(root.path()).unwrap();
  assert_eq!(
    entries,
    vec![
      File {
        path: "auv".into(),
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        size: 3,
      },
      File {
        path: "lib/auv/libx.so.1".into(),
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        size: 0,
      },
    ]
  );
}

#[test]
fn manifest_keeps_the_published_field_names() {
  let manifest = Manifest {
    schema_version: SCHEMA_VERSION,
    name: "auv".into(),
    version: "1.2.3".into(),
    target: "x86_64-unknown-linux-gnu".into(),
    glibc_baseline: "2.36".into(),
    layout: Layout::current(),
    libraries: vec![],
    models: vec![],
    files: vec![],
  };
  let value = serde_json::to_value(&manifest).unwrap();
  assert_eq!(value["schemaVersion"], 1);
  assert_eq!(value["glibcBaseline"], "2.36");
  assert_eq!(value["layout"]["libraryDirectory"], "lib/auv");
  assert_eq!(value["layout"]["tessdataDirectory"], "share/auv/tessdata");
  assert_eq!(system_license("libc6"), "licenses/system/libc6.copyright");
}
