use super::*;

#[test]
fn publisher_round_trips_and_removes_only_its_descriptor() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), path.parent().unwrap()).unwrap();
  let descriptor = auv::discovery::read_descriptor(&path).unwrap().unwrap();
  assert_eq!(descriptor.endpoint(), "http://127.0.0.1:9847");
  // Target-local commands derive the local socket from this canonical path.
  assert_eq!(descriptor.store_root(), Some(std::fs::canonicalize(directory.path()).unwrap().as_path()));
  drop(publisher);
  assert!(auv::discovery::read_descriptor(&path).unwrap().is_none());
}

#[test]
fn publisher_does_not_remove_a_replacement_descriptor() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), path.parent().unwrap()).unwrap();
  fs::write(&path, br#"{"endpoint":"http://127.0.0.1:1","process_id":1,"instance_id":"replacement"}"#).unwrap();
  drop(publisher);
  assert!(path.exists());
}

#[test]
fn publisher_rejects_a_competing_owner() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  let _publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), path.parent().unwrap()).unwrap();

  let error =
    PublishedDescriptor::publish(path, "http://127.0.0.1:9848".to_string(), Path::new(".")).err().expect("competing owner rejected");
  assert!(error.contains("another AUV API server owns"));
}

// ROOT CAUSE:
//
// If the discovery directory already existed, publish kept whatever mode it
// had, because 0700 was applied only when publish created the directory.
//
// Before the fix, a group- or other-writable directory let another user swap
// the descriptor or socket. The fix removes group/other write from the user's
// own directory and keeps read and traverse.
#[cfg(unix)]
#[test]
fn publisher_removes_group_and_other_write_from_an_owned_directory() {
  use std::os::unix::fs::PermissionsExt;
  let directory = tempfile::tempdir().unwrap();
  fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o777)).unwrap();
  let path = directory.path().join("api-server.json");

  let _publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), directory.path()).unwrap();

  assert_eq!(fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777, 0o755);
  assert!(auv::discovery::read_descriptor(&path).unwrap().is_some());
}

// NOTICE: creating a directory another user owns needs root, so this runs
// only as root; otherwise the uid comparison is covered by review alone.
#[cfg(unix)]
#[test]
fn publisher_refuses_a_directory_another_user_owns() {
  // SAFETY: geteuid has no pointers, allocation, or preconditions.
  if unsafe { libc::geteuid() } != 0 {
    return;
  }
  let directory = tempfile::tempdir().unwrap();
  std::os::unix::fs::chown(directory.path(), Some(65534), None).unwrap();
  let path = directory.path().join("api-server.json");

  let error = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), directory.path()).err().expect("refused");

  assert!(error.contains("not the current user"), "{error}");
  assert!(!path.exists());
}

#[test]
fn publisher_replaces_a_descriptor_left_by_a_dead_owner() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("api-server.json");
  // A SIGKILL-ed owner leaves its descriptor; its lock died with the process.
  fs::write(&path, br#"{"endpoint":"unix:///gone.sock","process_id":1,"instance_id":"dead"}"#).unwrap();

  let _publisher = PublishedDescriptor::publish(path.clone(), "http://127.0.0.1:9847".to_string(), directory.path()).unwrap();

  assert_eq!(auv::discovery::read_descriptor(&path).unwrap().unwrap().endpoint(), "http://127.0.0.1:9847");
}
