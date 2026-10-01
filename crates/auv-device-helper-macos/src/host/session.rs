//! Independent same-session readback through the shared macOS observer.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use auv_driver_macos::device_session::{ConsoleSession, ObserveError, observe_console};

use crate::{HostError, InputFailure};

use super::vault;

pub(super) fn unlock(home: &Path, uid: u32, selector: &str, started: Instant) -> Result<(), HostError> {
  if !selected(uid, selector)?.is_locked() {
    return Err(HostError::NotLocked);
  }

  // The Keychain read occurs only after the exact selected locked session is
  // observed. Recheck it before native delivery in case login state changed.
  let secret = vault::read(home, uid)?;

  if !selected(uid, selector)?.is_locked() {
    return Err(HostError::NotLocked);
  }

  // Reserve ten seconds for independent readback under the host's 18-second
  // request limit. A failed posting deadline never submits Return.
  let posting_budget = (started + Duration::from_secs(7)).saturating_duration_since(Instant::now()).as_secs_f64();

  if posting_budget <= 0.0 {
    return Err(HostError::InputUnavailableAt(InputFailure::DeadlineExceeded));
  }

  auv_driver_macos::device_session_unlock::submit(&secret, uid, selector, posting_budget).map_err(input_error)?;
  drop(secret);

  let deadline = Instant::now() + Duration::from_secs(10);

  loop {
    if !selected(uid, selector)?.is_locked() {
      return Ok(());
    }

    if Instant::now() >= deadline {
      return Err(HostError::OutcomeUnverified);
    }

    thread::sleep(Duration::from_millis(100));
  }
}

fn input_error(error: InputFailure) -> HostError {
  match error {
    InputFailure::Unavailable => HostError::InputUnavailable,
    stage => HostError::InputUnavailableAt(stage),
  }
}

pub(super) fn probe_locked(home: &Path, uid: u32, selector: &str) -> Result<(), HostError> {
  if !selected(uid, selector)?.is_locked() {
    return Err(HostError::NotLocked);
  }

  drop(vault::read(home, uid)?);

  if !selected(uid, selector)?.is_locked() {
    return Err(HostError::NotLocked);
  }

  Ok(())
}

fn selected(uid: u32, selector: &str) -> Result<ConsoleSession, HostError> {
  // The helper runs in the Aqua context and makes its own fresh IORegistry
  // observation; a daemon precheck cannot authorize a replacement session.
  let current = observe_console()
    .map_err(|error| match error {
      ObserveError::Ambiguous => HostError::StaleSession,
      ObserveError::Unavailable | ObserveError::UnknownState => HostError::Unavailable,
    })?
    .ok_or(HostError::StaleSession)?;

  if current.uid() != uid || current.selector() != selector {
    return Err(HostError::StaleSession);
  }

  Ok(current)
}
