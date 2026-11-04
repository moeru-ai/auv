//! Request activity: in-flight admitted requests and the time the last one
//! finished, for daemons that exit after a quiet period.
//!
//! A request counts from the moment it passes authentication until its
//! response body ends or is dropped, so a long gRPC stream, including Runner
//! traffic the daemon proxies, keeps the daemon busy for its whole life.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// Shared by every listener of one daemon instance.
#[derive(Debug)]
pub(crate) struct Activity {
  in_flight: AtomicUsize,
  last_finished: Mutex<Instant>,
}

impl Activity {
  /// Starts quiet: a daemon that never receives a request exits one timeout
  /// after it starts serving.
  pub(crate) fn new() -> Arc<Self> {
    Arc::new(Self {
      in_flight: AtomicUsize::new(0),
      last_finished: Mutex::new(Instant::now()),
    })
  }

  fn begin(self: &Arc<Self>) -> InFlight {
    self.in_flight.fetch_add(1, Ordering::SeqCst);
    InFlight(Arc::clone(self))
  }

  /// How long no admitted request has been in flight; `None` while one is.
  fn quiet_for(&self) -> Option<std::time::Duration> {
    if self.in_flight.load(Ordering::SeqCst) > 0 {
      return None;
    }
    Some(self.last_finished.lock().expect("activity clock").elapsed())
  }
}

/// One admitted request. Dropping it stamps the finish time before the count
/// falls, so the watcher never sees zero in flight with a stale clock.
struct InFlight(Arc<Activity>);

impl Drop for InFlight {
  fn drop(&mut self) {
    *self.0.last_finished.lock().expect("activity clock") = Instant::now();
    self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
  }
}

/// Counts the request until its response body is finished or dropped.
// NOTICE(activity-websocket-upgrade): a WebSocket upgrade's 101 response has
// an empty body, so the upgraded session is not counted after the handshake.
// The daemons that use the idle exit (CLI auto-start, Windows logon task)
// serve gRPC over local IPC only; count upgraded sessions if a REST WebSocket
// client must keep such a daemon alive.
pub(crate) async fn track(State(activity): State<Arc<Activity>>, request: Request, next: Next) -> Response {
  let in_flight = activity.begin();
  let response = next.run(request).await;
  response.map(|body| {
    Body::new(TrackedBody {
      inner: body,
      _in_flight: in_flight,
    })
  })
}

struct TrackedBody {
  inner: Body,
  _in_flight: InFlight,
}

impl http_body::Body for TrackedBody {
  type Data = bytes::Bytes;
  type Error = axum::Error;

  fn poll_frame(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
    Pin::new(&mut self.inner).poll_frame(context)
  }

  fn is_end_stream(&self) -> bool {
    self.inner.is_end_stream()
  }

  fn size_hint(&self) -> http_body::SizeHint {
    self.inner.size_hint()
  }
}

/// Cancels `shutdown` once no admitted request has been in flight for
/// `timeout`. The caller's shutdown path drains listeners and stops Runners.
pub(crate) async fn shutdown_when_quiet(activity: Arc<Activity>, shutdown: CancellationToken, timeout: std::time::Duration) {
  let poll_interval = timeout.min(std::time::Duration::from_secs(1));
  let mut interval = tokio::time::interval(poll_interval);
  interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  loop {
    tokio::select! {
      _ = shutdown.cancelled() => return,
      _ = interval.tick() => {
        if activity.quiet_for().is_some_and(|quiet| quiet >= timeout) {
          tracing::info!(timeout_seconds = timeout.as_secs(), "no request in flight for the idle exit timeout; stopping the daemon");
          shutdown.cancel();
          return;
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use http_body_util::BodyExt as _;
  use tower::ServiceExt as _;

  fn router(activity: &Arc<Activity>) -> axum::Router {
    axum::Router::new()
      .route("/", axum::routing::get(|| async { "ok" }))
      .layer(axum::middleware::from_fn_with_state(Arc::clone(activity), track))
  }

  #[tokio::test]
  async fn a_request_stays_in_flight_until_its_response_body_is_dropped() {
    let activity = Activity::new();
    let response = router(&activity).oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();

    // The handler returned, but a streaming body would still be sending.
    assert_eq!(activity.quiet_for(), None);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, "ok");
    assert!(activity.quiet_for().is_some());
  }

  #[tokio::test(start_paused = true)]
  async fn the_daemon_stops_only_after_a_quiet_period_with_nothing_in_flight() {
    let timeout = std::time::Duration::from_secs(300);
    let activity = Activity::new();
    let shutdown = CancellationToken::new();
    let watcher = tokio::spawn(shutdown_when_quiet(Arc::clone(&activity), shutdown.clone(), timeout));

    let long_stream = activity.begin();
    tokio::time::sleep(timeout * 2).await;
    assert!(!shutdown.is_cancelled(), "an in-flight request keeps the daemon alive");

    drop(long_stream);
    tokio::time::sleep(timeout - std::time::Duration::from_secs(5)).await;
    assert!(!shutdown.is_cancelled(), "idle counts from the end of the last request");

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    assert!(shutdown.is_cancelled());
    watcher.await.unwrap();
  }
}
