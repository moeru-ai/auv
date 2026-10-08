//! Process termination requests shared by the daemon and its Runners.
//!
//! The default action of SIGTERM, SIGHUP and SIGINT ends a process without
//! running its shutdown path: the daemon leaves its Unix socket and discovery
//! descriptor behind, and a Runner keeps mouse buttons and keys it holds
//! pressed. launchd and systemd stop services with SIGTERM, and Ctrl-C in a
//! terminal sends SIGINT to the daemon and its Runner children together.

/// Completes when the process is asked to terminate: SIGINT, SIGTERM or
/// SIGHUP on Unix, Ctrl-C or Ctrl-Break on Windows. A signal whose handler
/// cannot be installed is skipped, never treated as a request.
pub async fn requested() {
  #[cfg(unix)]
  {
    use tokio::signal::unix::{SignalKind, signal};
    let mut handlers = [
      SignalKind::interrupt(),
      SignalKind::terminate(),
      SignalKind::hangup(),
    ]
    .into_iter()
    .filter_map(|kind| signal(kind).ok())
    .collect::<Vec<_>>();
    if handlers.is_empty() {
      return std::future::pending().await;
    }
    let received = handlers.iter_mut().map(|handler| Box::pin(handler.recv()));
    futures_util::future::select_all(received).await;
  }
  #[cfg(windows)]
  {
    use tokio::signal::windows::{ctrl_break, ctrl_c};
    match (ctrl_c(), ctrl_break()) {
      (Ok(mut interrupt), Ok(mut brk)) => {
        tokio::select! {
          _ = interrupt.recv() => {}
          _ = brk.recv() => {}
        }
      }
      (Ok(mut interrupt), Err(_)) => {
        interrupt.recv().await;
      }
      (Err(_), Ok(mut brk)) => {
        brk.recv().await;
      }
      (Err(_), Err(_)) => std::future::pending().await,
    }
  }
  #[cfg(not(any(unix, windows)))]
  std::future::pending::<()>().await;
}

#[cfg(test)]
#[path = "termination_test.rs"]
mod tests;
