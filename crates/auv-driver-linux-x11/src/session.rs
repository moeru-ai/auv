use auv_driver_common::{DriverError, DriverResult};
use enigo::{Enigo, Settings};
use std::{
  ffi::OsString,
  sync::{Arc, Mutex, MutexGuard, OnceLock},
};

/// Connected X11 display and serialized foreground input state.
///
/// Clones share their input connection. Keep DISPLAY/XAUTHORITY unchanged for
/// the entire process lifetime; xcap caches its connection process-wide.
#[derive(Clone, Debug)]
pub struct X11DriverSession {
  pub(crate) input: Arc<Mutex<Enigo>>,
  pub(crate) wheel_remainder: Arc<Mutex<(f64, f64)>>,
  environment: Environment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Environment {
  display: String,
  authority: Option<OsString>,
}

// NOTICE: xcap 0.6.2 caches XCB_CONNECTION_AND_INDEX globally in
// `src/linux/utils.rs:24`. Reject switching displays instead of capturing a
// different server from the input target. Remove this restriction when xcap
// exposes an explicitly owned connection for each capture session.
static CAPTURE_ENVIRONMENT: OnceLock<Environment> = OnceLock::new();

pub(crate) fn invalid(message: impl Into<String>) -> DriverError {
  DriverError::InvalidInput {
    message: message.into(),
  }
}

pub(crate) fn backend(error: impl std::fmt::Display) -> DriverError {
  DriverError::Backend {
    message: error.to_string(),
  }
}

fn environment() -> DriverResult<Environment> {
  let display = std::env::var("DISPLAY").map_err(|_| invalid("DISPLAY must name an existing X11 server"))?;
  let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
  let wayland = std::env::var_os("WAYLAND_DISPLAY");
  validate_environment(&display, &session_type, wayland.as_deref())?;
  Ok(Environment {
    display,
    authority: std::env::var_os("XAUTHORITY"),
  })
}

fn validate_environment(display: &str, session_type: &str, wayland: Option<&std::ffi::OsStr>) -> DriverResult<()> {
  if display.trim().is_empty() {
    return Err(invalid("DISPLAY cannot be empty"));
  }
  if session_type.eq_ignore_ascii_case("wayland") || wayland.is_some_and(|value| !value.is_empty()) {
    return Err(invalid("X11 driver requires an isolated X11 process environment; use auv-driver-linux for Wayland"));
  }
  Ok(())
}

pub(crate) fn open() -> DriverResult<X11DriverSession> {
  let environment = environment()?;
  let pinned = CAPTURE_ENVIRONMENT.get_or_init(|| environment.clone());
  if pinned != &environment {
    return Err(invalid("xcap is pinned to another DISPLAY/XAUTHORITY; start a separate process"));
  }
  // Explicitly select the same display xcap reads, with only Enigo's x11rb
  // feature compiled in; neither backend can silently switch to Wayland.
  let input = Enigo::new(&Settings {
    x11_display: Some(environment.display.clone()),
    ..Settings::default()
  })
  .map_err(backend)?;
  Ok(X11DriverSession {
    input: Arc::new(Mutex::new(input)),
    wheel_remainder: Arc::new(Mutex::new((0.0, 0.0))),
    environment,
  })
}

impl X11DriverSession {
  /// Observes physical X11 monitor pixels without activating applications.
  pub fn display(&self) -> crate::DisplayApi<'_> {
    crate::DisplayApi { session: self }
  }

  /// Delivers foreground XTEST events, serialized across session clones.
  pub fn input(&self) -> crate::InputApi<'_> {
    crate::InputApi { session: self }
  }

  pub(crate) fn check_environment(&self) -> DriverResult<()> {
    if environment()? != self.environment {
      return Err(invalid("DISPLAY/XAUTHORITY changed after opening the X11 session"));
    }
    Ok(())
  }

  pub(crate) fn lock_input(&self) -> DriverResult<MutexGuard<'_, Enigo>> {
    self.check_environment()?;
    self.input.lock().map_err(|_| backend("X11 input mutex poisoned; open a new session"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn rejects_wayland_before_capture_backend_can_switch() {
    assert!(validate_environment(":1", "wayland", None).is_err());
    assert!(validate_environment(":1", "x11", Some("wayland-0".as_ref())).is_err());
    assert!(validate_environment(" ", "x11", None).is_err());
    assert!(validate_environment(":99", "", None).is_ok());
  }
}
