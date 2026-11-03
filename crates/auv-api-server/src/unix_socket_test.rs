use std::os::unix::fs::PermissionsExt;

use super::*;

fn mode(path: &Path) -> u32 {
  std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

// ROOT CAUSE:
//
// If a registered daemon was killed with SIGKILL, its socket stayed on disk
// because Drop never ran.
//
// Before the fix, the next `auv serve` failed with "API Unix socket path
// already exists". The fix removes an owned socket whose connect is refused.
#[tokio::test]
async fn stale_socket_is_removed() {
  let directory = tempfile::tempdir().unwrap();
  let socket = directory.path().join("auv.sock");
  drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());

  clear_stale(&socket, "test socket").await.unwrap();

  assert!(std::fs::symlink_metadata(&socket).is_err());
}

#[tokio::test]
async fn live_socket_is_left_in_place() {
  let directory = tempfile::tempdir().unwrap();
  let socket = directory.path().join("auv.sock");
  let _live = std::os::unix::net::UnixListener::bind(&socket).unwrap();

  let error = clear_stale(&socket, "test socket").await.unwrap_err();

  assert!(error.contains("live listener"), "{error}");
  assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok());
}

#[tokio::test]
async fn non_socket_and_symlink_are_refused() {
  let directory = tempfile::tempdir().unwrap();
  let file = directory.path().join("auv.sock");
  std::fs::write(&file, b"owner data").unwrap();
  let link = directory.path().join("link.sock");
  let target = directory.path().join("target.sock");
  drop(std::os::unix::net::UnixListener::bind(&target).unwrap());
  std::os::unix::fs::symlink(&target, &link).unwrap();

  assert!(clear_stale(&file, "test socket").await.unwrap_err().contains("not an owned socket"));
  assert!(clear_stale(&link, "test socket").await.unwrap_err().contains("not an owned socket"));
  assert_eq!(std::fs::read(&file).unwrap(), b"owner data");
  assert!(target.exists());
}

// NOTICE: another user's socket is refused by the same uid comparison, but a
// test cannot create one without root, so it is covered only when root runs it.
#[tokio::test]
async fn foreign_socket_is_refused() {
  if current_euid() != 0 {
    return;
  }
  let directory = tempfile::tempdir().unwrap();
  let socket = directory.path().join("auv.sock");
  drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
  std::os::unix::fs::lchown(&socket, Some(65534), None).unwrap();

  assert!(clear_stale(&socket, "test socket").await.unwrap_err().contains("not an owned socket"));
  assert!(std::fs::symlink_metadata(&socket).is_ok());
}

#[tokio::test]
async fn shared_parent_gets_a_private_socket_without_staging_leftovers() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o777)).unwrap();
  let socket = directory.path().join("auv.sock");

  let _listener = bind_private(&socket).unwrap();

  assert_eq!(mode(&socket), 0o600);
  assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok());
  let entries = std::fs::read_dir(directory.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
  assert_eq!(entries, vec![std::ffi::OsString::from("auv.sock")]);
}

#[tokio::test]
async fn missing_parent_is_created_private() {
  let directory = tempfile::tempdir().unwrap();
  let parent = directory.path().join("state");
  let socket = parent.join("auv.sock");

  let _listener = bind_private(&socket).unwrap();

  assert_eq!(mode(&parent), 0o700);
  assert_eq!(mode(&socket), 0o600);
}

#[tokio::test]
async fn existing_path_is_never_replaced() {
  let directory = tempfile::tempdir().unwrap();
  std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
  let socket = directory.path().join("auv.sock");
  std::fs::write(&socket, b"owner data").unwrap();

  assert!(bind_private(&socket).is_err());
  assert_eq!(std::fs::read(&socket).unwrap(), b"owner data");
  assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1, "staging directory removed");
}
