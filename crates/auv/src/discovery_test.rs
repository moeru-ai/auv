use super::*;

#[test]
fn missing_descriptor_is_not_a_discovery_error() {
  let directory = tempfile::tempdir().unwrap();
  assert!(read_descriptor(&directory.path().join("missing.json")).unwrap().is_none());
}

#[test]
fn descriptor_round_trips_without_a_redundant_version_field() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let descriptor =
    Descriptor::for_current_process("http://127.0.0.1:9847".to_string(), "instance".to_string(), std::path::PathBuf::from("/srv/auv/store"));
  fs::write(&path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
  let decoded = read_descriptor(&path).unwrap().unwrap();
  assert_eq!(decoded.endpoint(), "http://127.0.0.1:9847");
  assert_eq!(decoded.instance_id(), "instance");
  assert_eq!(decoded.store_root(), Some(Path::new("/srv/auv/store")));
  assert!(!fs::read_to_string(path).unwrap().contains("version"));
}

#[test]
fn descriptor_without_a_store_root_still_reads() {
  // Daemons before 2026-10-11 published only the endpoint and instance ID.
  // Their descriptors must stay readable; target-local commands then fall
  // back to their default store.
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  fs::write(&path, br#"{"endpoint":"unix:///tmp/auv.sock","instance_id":"old"}"#).unwrap();

  let decoded = read_descriptor(&path).unwrap().unwrap();
  assert_eq!(decoded.endpoint(), "unix:///tmp/auv.sock");
  assert_eq!(decoded.store_root(), None);
}

#[test]
fn explicit_endpoint_is_resolved_without_discovery() {
  let endpoint = resolve(Some("http://example.com:9847")).unwrap().expect("explicit endpoint");
  assert_eq!(endpoint.to_string(), "http://example.com:9847");
}
