use std::collections::HashMap;

use auv_driver_common::error::DriverResult;
use zbus::blocking::{Connection, Proxy};
use zbus::message::Message;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::error::backend;

pub(super) const PORTAL_DESTINATION: &str = "org.freedesktop.portal.Desktop";
pub(super) const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";

pub(super) fn session_connection() -> DriverResult<Connection> {
  Connection::session().map_err(|error| backend(format!("failed to connect to session bus: {error}")))
}

pub(super) fn portal_proxy<'a>(connection: &'a Connection, interface: &'static str) -> DriverResult<Proxy<'a>> {
  Proxy::new(connection, PORTAL_DESTINATION, PORTAL_PATH, interface)
    .map_err(|error| backend(format!("failed to create {interface} proxy: {error}")))
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
  proxy.call_method(method, &(options)).map_err(|error| backend(format!("failed to call {interface}.{method}: {error}")))?;
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
  proxy.call_method(method, &(session_handle, options)).map_err(|error| backend(format!("failed to call {interface}.{method}: {error}")))?;
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
  if let Some(value) = results.get("session_handle") {
    if let Ok(handle) = <&str>::try_from(value) {
      return OwnedObjectPath::try_from(handle.to_string())
        .map_err(|error| backend(format!("portal returned invalid session handle: {error}")));
    }
  }
  expected_session_path(connection, &session_handle_token)
}

pub(super) fn close_session(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  let session = Proxy::new(connection, PORTAL_DESTINATION, session_handle.clone(), "org.freedesktop.portal.Session")
    .map_err(|error| backend(format!("failed to create portal session proxy: {error}")))?;
  session.call_method("Close", &()).map_err(|error| backend(format!("failed to close portal session: {error}")))?;
  Ok(())
}

pub(super) fn portal_token(prefix: &str) -> String {
  format!(
    "auv_{prefix}_{}_{}",
    std::process::id(),
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_micros()).unwrap_or_default()
  )
}

pub(super) fn response_signal<'a>(
  request: &'a Proxy<'_>,
  interface: &'static str,
  method: &'static str,
) -> DriverResult<impl Iterator<Item = Message> + 'a> {
  request.receive_signal("Response").map_err(|error| backend(format!("failed to subscribe to {interface}.{method} response: {error}")))
}

pub(super) fn wait_response(
  responses: &mut impl Iterator<Item = Message>,
  interface: &'static str,
  method: &'static str,
) -> DriverResult<HashMap<String, OwnedValue>> {
  let response = responses.next().ok_or_else(|| backend(format!("{interface}.{method} did not return a response")))?;
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
mod tests {
  use super::*;

  #[test]
  fn portal_token_is_object_path_component_friendly() {
    let token = portal_token("session");

    assert!(token.starts_with("auv_session_"));
    assert!(token.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
  }
}
