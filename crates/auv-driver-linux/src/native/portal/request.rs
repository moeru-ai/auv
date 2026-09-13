use std::future::Future;
use std::time::Duration;

use auv_driver_common::error::DriverResult;
use futures_lite::future;

use crate::error::backend;

/// Bound Portal calls and response waits at the synchronous driver boundary.
/// Use ashpd's async-io backend so calls also work from a Tokio frontend.
pub(crate) fn run<T>(operation: &str, request: impl Future<Output = ashpd::Result<T>>) -> DriverResult<T> {
  future::block_on(future::race(async { request.await.map_err(|error| backend(format!("{operation}: {error}"))) }, async {
    async_io::Timer::after(Duration::from_secs(10)).await;
    Err(backend(format!("{operation}: Portal timed out after 10s")))
  }))
}

/// Register on this exact connection before creating any Portal object. A new
/// connection after a Portal restart registers again; a token is not identity.
/// TODO: proactive Portal-restart recovery across capture and clipboard is
/// deferred until their session invalidation lifecycle is defined. New
/// connections register again; existing sessions are not migrated here.
/// https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html
pub(crate) fn session_connection(app_id: Option<&str>) -> DriverResult<zbus::Connection> {
  run("connect and register Portal application (install its desktop entry with auv doctor --portal-setup)", async {
    let connection = zbus::connection::Builder::session()?.method_timeout(Duration::from_secs(3)).build().await?;
    if let Some(app_id) = app_id {
      ashpd::register_host_app_with_connection(connection.clone(), app_id.parse()?).await?;
    }
    Ok(connection)
  })
}

#[cfg(test)]
#[path = "identity_test.rs"]
mod identity_tests;
