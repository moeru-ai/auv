#[cfg(unix)]
#[tokio::test]
async fn explicit_daemon_context_does_not_fall_back_to_local() {
  let directory = tempfile::tempdir().expect("temporary socket directory");
  let missing_socket = directory.path().join("missing.sock");
  let context = auv_core::AuvContext {
    daemon_endpoint: Some(format!("unix://{}", missing_socket.display())),
    ..Default::default()
  };

  assert!(auv_core::Client::from_context(context).await.is_err());
}
