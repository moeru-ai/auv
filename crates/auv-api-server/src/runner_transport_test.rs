use super::*;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

// ROOT CAUSE:
//
// If a Runner process received SIGTERM, SIGINT (Ctrl-C reaches the daemon and
// its Runners together) or SIGHUP, it ended with the default action and never
// ran its cleanup, so input it held stayed pressed. Handling the signal alone
// was not enough: graceful shutdown then waited on the parent connection,
// which stays open.
//
// Before the fix, the Runner died on the signal. Now a termination request
// ends the IPC stream as a parent disconnect would, so serving stops and the
// cleanup runs.
#[tokio::test]
async fn a_termination_request_ends_the_runner_stream_like_a_parent_disconnect() {
  // Install a SIGTERM handler before signalling this test process, so the
  // signal can never take the default action and end the test binary.
  let _keep_handler = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("install SIGTERM handler");
  let (runner_side, mut daemon_side) = tokio::net::UnixStream::pair().expect("socket pair");
  let mut stream = InheritedStream {
    inner: runner_side,
    disconnected: None,
    termination: Termination::new(),
  };

  // Before termination, the stream carries the daemon's bytes.
  daemon_side.write_all(b"ping").await.expect("daemon writes");
  let mut buffer = [0_u8; 4];
  stream.read_exact(&mut buffer).await.expect("runner reads");
  assert_eq!(&buffer, b"ping");

  // A pending read registers the termination handler; then SIGTERM arrives
  // while the daemon side is still connected.
  let read = tokio::spawn(async move {
    let mut buffer = [0_u8; 16];
    stream.read(&mut buffer).await
  });
  tokio::time::sleep(std::time::Duration::from_millis(50)).await;
  let status = std::process::Command::new("/bin/kill")
    .args(["-TERM", &std::process::id().to_string()])
    .status()
    .expect("send SIGTERM to this test process");
  assert!(status.success());

  let read = tokio::time::timeout(std::time::Duration::from_secs(5), read)
    .await
    .expect("termination must end a pending read")
    .expect("read task")
    .expect("read result");
  assert_eq!(read, 0, "a terminated Runner stream reports end of stream");
  drop(daemon_side);
}
