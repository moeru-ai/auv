use std::future::Future;
use std::time::Duration;

use auv_driver_common::error::DriverResult;
use futures_lite::future;

use crate::error::backend;

/// Bound Portal calls and response waits at the synchronous driver boundary.
/// Use ashpd's async-io backend so calls also work from a Tokio frontend.
pub(crate) fn run<T>(operation: &str, request: impl Future<Output = ashpd::Result<T>>) -> DriverResult<T> {
  run_within(operation, request, Duration::from_secs(10), "Portal timed out after 10s")
}

/// Like [`run`], for a step that can wait on a permission dialog the person
/// answers (RemoteDesktop/ScreenCast Start, Screenshot).
///
/// NOTICE: a consent dialog left open for more than 10 s failed the action
/// with "Portal timed out after 10s" (GNOME 48: approving a new input session
/// after 15 s failed 3/3). A person needs longer than an automatic portal
/// reply; a restored grant still answers at once.
pub(crate) fn run_with_consent<T>(operation: &str, request: impl Future<Output = ashpd::Result<T>>) -> DriverResult<T> {
  run_within(operation, request, Duration::from_secs(60), "Portal waited 60s for the permission dialog; approve it on the desktop and retry")
}

fn run_within<T>(
  operation: &str,
  request: impl Future<Output = ashpd::Result<T>>,
  limit: Duration,
  timeout_message: &str,
) -> DriverResult<T> {
  future::block_on(future::race(async { request.await.map_err(|error| backend(format!("{operation}: {error}"))) }, async {
    async_io::Timer::after(limit).await;
    Err(backend(format!("{operation}: {timeout_message}")))
  }))
}

/// Register on this exact connection before creating any Portal object. A new
/// connection after a Portal restart registers again; a token is not identity.
/// TODO: proactive Portal-restart recovery across capture and clipboard is
/// deferred until their session invalidation lifecycle is defined. New
/// connections register again; existing sessions are not migrated here.
/// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html
pub(crate) fn session_connection(app_id: Option<&ashpd::AppID>) -> DriverResult<zbus::Connection> {
  run("connect and register Portal application (install its desktop entry with auv doctor --portal-setup)", async {
    let connection = zbus::connection::Builder::session()?.method_timeout(Duration::from_secs(3)).build().await?;
    if let Some(app_id) = app_id {
      ashpd::register_host_app_with_connection(connection.clone(), app_id.clone()).await?;
    }
    Ok(connection)
  })
}

#[cfg(test)]
#[path = "identity_test.rs"]
mod identity_tests;
