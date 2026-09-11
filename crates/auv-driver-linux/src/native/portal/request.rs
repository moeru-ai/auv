use std::collections::HashMap;
use std::time::Duration;

use auv_driver_common::error::DriverResult;
use futures_lite::{StreamExt, future};
use serde::Serialize;
use zbus::blocking::{Connection, Proxy};
use zbus::message::Message;
use zbus::proxy::SignalStream;
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Value};

use crate::error::backend;

pub(super) const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
pub(super) const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn session_connection() -> DriverResult<Connection> {
  Connection::session().map_err(|error| backend(format!("failed to connect to session bus: {error}")))
}

pub(super) fn portal_proxy<'a>(connection: &'a Connection, interface: &'static str) -> DriverResult<Proxy<'a>> {
  Proxy::new(connection, PORTAL_DESTINATION, PORTAL_PATH, interface)
    .map_err(|error| backend(format!("failed to create {interface} proxy: {error}")))
}

pub(super) fn interface_version(connection: &Connection, interface: &'static str) -> DriverResult<u32> {
  let proxy = portal_proxy(connection, interface)?;
  future::block_on(future::race(
    async { proxy.inner().get_property("version").await.map_err(|error| backend(format!("failed to read {interface}.version: {error}"))) },
    async {
      async_io::Timer::after(PORTAL_RESPONSE_TIMEOUT).await;
      Err(backend(format!("timed out after {}s reading {interface}.version", PORTAL_RESPONSE_TIMEOUT.as_secs())))
    },
  ))
}

pub(super) fn call_method<B>(proxy: &Proxy<'_>, interface: &'static str, method: &'static str, body: &B) -> DriverResult<Message>
where
  B: Serialize + DynamicType,
{
  future::block_on(future::race(
    async {
      proxy.inner().call_method(method, body).await.map_err(|error| backend(format!("failed to call {interface}.{method}: {error}")))
    },
    async {
      async_io::Timer::after(PORTAL_RESPONSE_TIMEOUT).await;
      Err(backend(format!("timed out after {}s calling {interface}.{method}", PORTAL_RESPONSE_TIMEOUT.as_secs())))
    },
  ))
}

pub(super) fn restore_token(results: &HashMap<String, OwnedValue>, interface: &'static str) -> DriverResult<Option<String>> {
  let Some(value) = results.get("restore_token") else {
    return Ok(None);
  };
  <&str>::try_from(value)
    .map(|value| Some(value.to_string()))
    .map_err(|error| backend(format!("failed to decode {interface} restore token: {error}")))
}

pub(super) fn call_request(
  connection: &Connection,
  interface: &'static str,
  method: &'static str,
  options: HashMap<&str, Value<'_>>,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = portal_token("request");
  let request = portal_request_proxy(connection, &handle_token)?;
  let mut responses = response_signal(&request, interface, method)?;
  let proxy = portal_proxy(connection, interface)?;
  let mut options = options;
  options.insert("handle_token", Value::from(handle_token.as_str()));
  call_method(&proxy, interface, method, &(options))?;
  wait_response(&mut responses, interface, method)
}

pub(super) fn session_request(
  connection: &Connection,
  interface: &'static str,
  method: &'static str,
  session_handle: &OwnedObjectPath,
  options: HashMap<&str, Value<'_>>,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = portal_token("request");
  let request = portal_request_proxy(connection, &handle_token)?;
  let mut responses = response_signal(&request, interface, method)?;
  let proxy = portal_proxy(connection, interface)?;
  let mut options = options;
  options.insert("handle_token", Value::from(handle_token.as_str()));
  call_method(&proxy, interface, method, &(session_handle, options))?;
  wait_response(&mut responses, interface, method)
}

pub(super) fn create_remote_desktop_session(connection: &Connection) -> DriverResult<OwnedObjectPath> {
  create_session(connection, "org.freedesktop.portal.RemoteDesktop")
}

pub(super) fn create_session(connection: &Connection, interface: &'static str) -> DriverResult<OwnedObjectPath> {
  let session_handle_token = portal_token("session");
  let mut options = HashMap::new();
  options.insert("session_handle_token", Value::from(session_handle_token.as_str()));
  let results = call_request(connection, interface, "CreateSession", options)?;
  if let Some(value) = results.get("session_handle")
    && let Ok(handle) = <&str>::try_from(value)
  {
    return OwnedObjectPath::try_from(handle.to_string())
      .map_err(|error| backend(format!("portal returned invalid session handle: {error}")));
  }
  expected_session_path(connection, &session_handle_token)
}

pub(super) fn close_session(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  const SESSION_INTERFACE: &str = "org.freedesktop.portal.Session";
  let session = Proxy::new(connection, PORTAL_DESTINATION, session_handle.clone(), SESSION_INTERFACE)
    .map_err(|error| backend(format!("failed to create portal session proxy: {error}")))?;
  call_method(&session, SESSION_INTERFACE, "Close", &())?;
  Ok(())
}

pub(super) fn portal_token(prefix: &str) -> String {
  format!(
    "auv_{prefix}_{}_{}",
    std::process::id(),
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_micros()).unwrap_or_default()
  )
}

pub(super) fn response_signal<'a>(request: &'a Proxy<'_>, interface: &'static str, method: &'static str) -> DriverResult<SignalStream<'a>> {
  future::block_on(future::race(
    async {
      request
        .inner()
        .receive_signal("Response")
        .await
        .map_err(|error| backend(format!("failed to subscribe to {interface}.{method} response: {error}")))
    },
    async {
      async_io::Timer::after(PORTAL_RESPONSE_TIMEOUT).await;
      Err(backend(format!("timed out after {}s subscribing to {interface}.{method} portal response", PORTAL_RESPONSE_TIMEOUT.as_secs())))
    },
  ))
}

pub(super) fn wait_response(
  responses: &mut SignalStream<'_>,
  interface: &'static str,
  method: &'static str,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let response: Message = future::block_on(future::race(
    async { responses.next().await.ok_or_else(|| backend(format!("{interface}.{method} did not return a response"))) },
    async {
      async_io::Timer::after(PORTAL_RESPONSE_TIMEOUT).await;
      // TODO(portal-request-cancellation): do not synchronously call
      // Request.Close here because a stalled portal can also stall that method
      // reply. Add no-reply cancellation only when the D-Bus lifecycle owner
      // can guarantee cleanup without weakening this deadline.
      Err(backend(format!("timed out after {}s waiting for {interface}.{method} portal response", PORTAL_RESPONSE_TIMEOUT.as_secs())))
    },
  ))?;
  let (code, results): (u32, HashMap<String, OwnedValue>) =
    response.body().deserialize().map_err(|error| backend(format!("failed to decode {interface}.{method} response: {error}")))?;
  if code == 0 {
    Ok(results)
  } else {
    let reason = match code {
      1 => "cancelled or denied by the portal",
      2 => "failed",
      _ => "returned an unknown response code",
    };
    Err(backend(format!("{interface}.{method} {reason} (response code {code})")))
  }
}

pub(super) fn portal_request_proxy<'a>(connection: &'a Connection, handle_token: &str) -> DriverResult<Proxy<'a>> {
  let unique_name =
    connection.unique_name().ok_or_else(|| backend("session bus connection has no unique name"))?.trim_start_matches(':').replace('.', "_");
  let path = format!("/org/freedesktop/portal/desktop/request/{unique_name}/{handle_token}");
  Proxy::new(connection, PORTAL_DESTINATION, path, "org.freedesktop.portal.Request")
    .map_err(|error| backend(format!("failed to create portal request proxy: {error}")))
}

fn expected_session_path(connection: &Connection, session_handle_token: &str) -> DriverResult<OwnedObjectPath> {
  let unique_name =
    connection.unique_name().ok_or_else(|| backend("session bus connection has no unique name"))?.trim_start_matches(':').replace('.', "_");
  OwnedObjectPath::try_from(format!("/org/freedesktop/portal/desktop/session/{unique_name}/{session_handle_token}"))
    .map_err(|error| backend(format!("failed to build portal session path: {error}")))
}

#[cfg(test)]
#[path = "request_test.rs"]
mod tests;
