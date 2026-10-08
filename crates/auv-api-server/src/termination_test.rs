use super::*;

// ROOT CAUSE:
//
// If the daemon or a Runner received SIGTERM, the process ended with the
// default action because only Ctrl-C (SIGINT) was handled.
//
// Before the fix, the daemon left its Unix socket and discovery descriptor
// behind and a Runner skipped releasing held input. `requested` now completes
// on SIGTERM too, so both run their shutdown path.
#[cfg(unix)]
#[tokio::test]
async fn sigterm_requests_termination() {
  // Install a SIGTERM handler before signalling this test process, so the
  // signal can never take the default action and end the test binary.
  let _keep_handler = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("install SIGTERM handler");
  let requested = tokio::spawn(requested());
  // Let `requested` install its own handlers before the signal arrives.
  tokio::time::sleep(std::time::Duration::from_millis(50)).await;

  let status = std::process::Command::new("/bin/kill")
    .args(["-TERM", &std::process::id().to_string()])
    .status()
    .expect("send SIGTERM to this test process");
  assert!(status.success());

  tokio::time::timeout(std::time::Duration::from_secs(5), requested)
    .await
    .expect("SIGTERM must complete the termination request")
    .expect("termination task");
}
