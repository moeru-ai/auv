use super::*;

#[test]
fn successful_restore_rotates_the_single_use_token_without_a_version_wrapper() {
  let temporary = tempfile::tempdir().expect("temporary directory");
  let store = RestoreTokenStore::new(temporary.path().to_path_buf());

  store
    .rotate(RestoreTokenKind::ScreenCast, |current| {
      assert_eq!(current, None);
      Ok(((), Some("first-opaque-token".to_string())))
    })
    .expect("first authorization token");
  store
    .rotate(RestoreTokenKind::ScreenCast, |current| {
      assert_eq!(current, Some("first-opaque-token"));
      Ok(((), Some("replacement-opaque-token".to_string())))
    })
    .expect("restored authorization token");

  assert_eq!(fs::read_to_string(temporary.path().join("screencast-token")).unwrap(), "replacement-opaque-token");
}

#[test]
fn failed_restore_keeps_the_previous_token_for_a_later_retry() {
  let temporary = tempfile::tempdir().expect("temporary directory");
  let store = RestoreTokenStore::new(temporary.path().to_path_buf());
  store.rotate(RestoreTokenKind::RemoteDesktopInput, |_| Ok(((), Some("input-token".to_string())))).expect("initial token");

  let error = store
    .rotate::<()>(RestoreTokenKind::RemoteDesktopInput, |current| {
      assert_eq!(current, Some("input-token"));
      Err(backend("portal request failed"))
    })
    .expect_err("failed portal request");

  assert!(error.to_string().contains("portal request failed"));
  assert_eq!(fs::read_to_string(temporary.path().join("remote-desktop-input-token")).unwrap(), "input-token");
}

#[test]
fn successful_start_without_a_replacement_removes_a_consumed_token() {
  let temporary = tempfile::tempdir().expect("temporary directory");
  let store = RestoreTokenStore::new(temporary.path().to_path_buf());
  store.rotate(RestoreTokenKind::ScreenCast, |_| Ok(((), Some("old-token".to_string())))).unwrap();

  store
    .rotate(RestoreTokenKind::ScreenCast, |current| {
      assert_eq!(current, Some("old-token"));
      Ok(((), None))
    })
    .expect("successful non-persistent start");

  assert!(!temporary.path().join("screencast-token").exists());
}
