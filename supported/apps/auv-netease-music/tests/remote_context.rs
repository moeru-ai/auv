#[cfg(target_os = "macos")]
#[test]
fn remote_now_playing_has_a_tokio_io_reactor() {
  let socket = std::env::temp_dir().join(format!("auv-missing-netease-test-{}.sock", std::process::id()));
  let context = auv::AuvContext {
    device_id: Some("device_test".to_string()),
    run_id: Some("run_test".to_string()),
    daemon_endpoint: Some(format!("unix://{}", socket.display())),
    ..Default::default()
  };

  // ROOT CAUSE:
  //
  // When a plugin inherited a remote Run, tonic opened its Unix socket under
  // `futures_executor::block_on`, so Tokio panicked before reporting the
  // ordinary connection failure. The command frontend must always own a Tokio
  // reactor because local and remote execution share this entrypoint.
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_auv-netease-music"))
    .args(["now-playing", "--format", "json"])
    .env("AUV_CONTEXT", serde_json::to_string(&context).expect("encode test context"))
    .output()
    .expect("execute NetEase plugin");

  assert_eq!(output.status.code(), Some(1));
  let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
  assert!(stderr.contains("now-playing read failed: transport error"), "unexpected stderr: {stderr}");
  assert!(!stderr.contains("panicked"), "Tokio reactor panic regressed: {stderr}");
}
