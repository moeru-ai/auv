use super::*;

#[test]
fn publisher_round_trips_and_removes_only_its_descriptor() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string()).unwrap();
  assert_eq!(auv::discovery::read_descriptor(&path).unwrap().unwrap().endpoint(), "http://127.0.0.1:9847");
  drop(publisher);
  assert!(auv::discovery::read_descriptor(&path).unwrap().is_none());
}

#[test]
fn publisher_does_not_remove_a_replacement_descriptor() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string()).unwrap();
  fs::write(&path, br#"{"endpoint":"http://127.0.0.1:1","process_id":1,"instance_id":"replacement"}"#).unwrap();
  drop(publisher);
  assert!(path.exists());
}

#[test]
fn publisher_rejects_a_competing_owner() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let _publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string()).unwrap();

  let error = PublishedDescriptor::publish(path, "http://127.0.0.1:9848".to_string()).err().expect("competing owner rejected");
  assert!(error.contains("another AUV API server owns"));
}
