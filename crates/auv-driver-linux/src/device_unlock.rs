//! Existing GNOME Wayland session unlock through logind.
//!
//! This adapter is deliberately scoped to one physical session owned by the
//! caller's effective UID. A target host must authorize the remote Device
//! request, local enrollment, and audit before calling it.

use std::time::{Duration, Instant};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

const LOGIN_DEST: &str = "org.freedesktop.login1";
const LOGIN_PATH: &str = "/org/freedesktop/login1";
const LOGIN_MANAGER: &str = "org.freedesktop.login1.Manager";
const LOGIN_SESSION: &str = "org.freedesktop.login1.Session";

/// One physical, local GNOME Wayland session of the calling OS account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GnomeSession {
  /// logind's session ID, valid only while this OS session exists.
  pub id: String,
  /// logind session creation time in microseconds since the Unix epoch.
  /// A session ID alone can be reused after logout.
  pub started_at_micros: u64,
  /// The OS user name returned by logind.
  pub user: String,
  /// The stable numeric OS account identity returned by logind.
  pub uid: u32,
  /// The physical seat reported by logind.
  pub seat: String,
  /// The state requested through logind's `LockedHint`.
  pub lock_state: LockState,
}

impl GnomeSession {
  /// Opaque selector for this particular logind session instance.
  pub fn selector(&self) -> String {
    format!("linux-logind:{}:{}:{}", self.id, self.uid, self.started_at_micros)
  }

  /// Checks that a fresh observation still refers to the same login.
  pub fn same_login(&self, current: &Self) -> bool {
    self.id == current.id
      && self.started_at_micros == current.started_at_micros
      && self.uid == current.uid
      && self.user == current.user
      && self.seat == current.seat
  }
}

/// The lock state reported by logind for this session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockState {
  Locked,
  Usable,
  /// TODO: No longer produced from GNOME disagreement. Keep this public
  /// variant for the Device mapping until an owner-approved contract change.
  Unknown,
}

/// Result of an attempt after reading the same session again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnlockOutcome {
  AlreadyUsable,
  UnlockedExistingSession,
}

/// A non-secret failure class for the Device policy layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UnlockError {
  #[error("logind session inventory is unavailable")]
  ServiceUnavailable,
  #[error("the selected GNOME session is stale")]
  StaleSession,
  #[error("the selected session is not a physical GNOME Wayland session owned by this host identity")]
  UnsupportedOsState,
  #[error("logind session unlock could not be verified")]
  OutcomeUnverified,
}

/// Lists the caller's physical GNOME Wayland sessions. A Device user selector
/// must reject ambiguity, while an explicit session selector can choose one.
/// A daemon serving more than one user needs separate per-user hosts; a root
/// process cannot borrow a user's session bus and claim that user's authority.
pub fn list_user_sessions() -> Result<Vec<GnomeSession>, UnlockError> {
  let system = system_connection()?;
  let manager = Proxy::new(&system, LOGIN_DEST, LOGIN_PATH, LOGIN_MANAGER).map_err(|_| UnlockError::ServiceUnavailable)?;
  let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> =
    manager.call("ListSessions", &()).map_err(|_| UnlockError::ServiceUnavailable)?;
  // SAFETY: `geteuid` reads the calling process credential and takes no
  // pointers or mutable state from Rust.
  let effective_uid = unsafe { libc::geteuid() };
  let mut eligible = Vec::new();

  for (id, uid, user, _seat, path) in sessions {
    if uid != effective_uid {
      continue;
    }

    let session = Proxy::new(&system, LOGIN_DEST, path.as_str(), LOGIN_SESSION).map_err(|_| UnlockError::ServiceUnavailable)?;
    let current_id: String = session.get_property("Id").map_err(|_| UnlockError::ServiceUnavailable)?;
    let (current_uid, _): (u32, OwnedObjectPath) = session.get_property("User").map_err(|_| UnlockError::ServiceUnavailable)?;
    let current_user: String = session.get_property("Name").map_err(|_| UnlockError::ServiceUnavailable)?;

    if current_id != id || current_uid != uid || current_user != user {
      return Err(UnlockError::StaleSession);
    }

    let kind: String = session.get_property("Type").map_err(|_| UnlockError::ServiceUnavailable)?;
    let class: String = session.get_property("Class").map_err(|_| UnlockError::ServiceUnavailable)?;
    let remote: bool = session.get_property("Remote").map_err(|_| UnlockError::ServiceUnavailable)?;
    let active: bool = session.get_property("Active").map_err(|_| UnlockError::ServiceUnavailable)?;
    let (seat, _): (String, OwnedObjectPath) = session.get_property("Seat").map_err(|_| UnlockError::ServiceUnavailable)?;

    if !eligible_session(&kind, &class, remote, active) || seat.is_empty() {
      continue;
    }

    // LockedHint is logind's requested lock state. It does not prove that
    // GNOME rendered or dismissed its lock UI; that requires an installed gate.
    let locked_hint: bool = session.get_property("LockedHint").map_err(|_| UnlockError::ServiceUnavailable)?;
    // `Timestamp` is the creation time of this session (org.freedesktop.login1(5)).
    // It disambiguates a reused logind ID after logout and a fresh login.
    let started_at_micros: u64 = session.get_property("Timestamp").map_err(|_| UnlockError::ServiceUnavailable)?;

    if started_at_micros == 0 {
      return Err(UnlockError::ServiceUnavailable);
    }

    eligible.push(GnomeSession {
      id,
      started_at_micros,
      user,
      uid,
      seat,
      lock_state: observed_lock_state(locked_hint),
    });
  }

  Ok(eligible)
}

/// Unlocks one selected existing session through logind and verifies that
/// logind reports it usable before returning a positive effect. The readback
/// does not itself prove that GNOME dismissed the lock UI.
pub fn unlock_user_session(session: &GnomeSession) -> Result<UnlockOutcome, UnlockError> {
  let before = selected_session(session)?;

  match before.lock_state {
    LockState::Usable => return Ok(UnlockOutcome::AlreadyUsable),
    LockState::Unknown => return Err(UnlockError::OutcomeUnverified),
    LockState::Locked => {}
  }

  let system = system_connection()?;
  let manager = Proxy::new(&system, LOGIN_DEST, LOGIN_PATH, LOGIN_MANAGER).map_err(|_| UnlockError::ServiceUnavailable)?;
  let path: OwnedObjectPath = manager.call("GetSession", &(session.id.as_str(),)).map_err(|_| UnlockError::StaleSession)?;
  let selected = Proxy::new(&system, LOGIN_DEST, path.as_str(), LOGIN_SESSION).map_err(|_| UnlockError::ServiceUnavailable)?;
  // GetSession can resolve a reused ID after the earlier snapshot. Validate
  // the exact login identity on the object that will receive Unlock.
  let current_id: String = selected.get_property("Id").map_err(|_| UnlockError::StaleSession)?;
  let started_at_micros: u64 = selected.get_property("Timestamp").map_err(|_| UnlockError::StaleSession)?;
  let (uid, _): (u32, OwnedObjectPath) = selected.get_property("User").map_err(|_| UnlockError::StaleSession)?;
  let user: String = selected.get_property("Name").map_err(|_| UnlockError::StaleSession)?;
  let (seat, _): (String, OwnedObjectPath) = selected.get_property("Seat").map_err(|_| UnlockError::StaleSession)?;
  // Active can change while the login identity remains the same. Recheck the
  // delivery object's console eligibility immediately before calling Unlock.
  let kind: String = selected.get_property("Type").map_err(|_| UnlockError::StaleSession)?;
  let class: String = selected.get_property("Class").map_err(|_| UnlockError::StaleSession)?;
  let remote: bool = selected.get_property("Remote").map_err(|_| UnlockError::StaleSession)?;
  let active: bool = selected.get_property("Active").map_err(|_| UnlockError::StaleSession)?;

  if current_id != session.id
    || started_at_micros != session.started_at_micros
    || uid != session.uid
    || user != session.user
    || seat != session.seat
    || !eligible_session(&kind, &class, remote, active)
  {
    return Err(UnlockError::StaleSession);
  }
  // The earlier inventory may have observed a lock that the user cleared
  // while we resolved the delivery object. Recheck logind's lock state on
  // that exact object before invoking logind Unlock.
  let locked_hint: bool = selected.get_property("LockedHint").map_err(|_| UnlockError::ServiceUnavailable)?;

  if !locked_hint {
    return Ok(UnlockOutcome::AlreadyUsable);
  }

  selected.call::<_, _, ()>("Unlock", &()).map_err(|_| UnlockError::ServiceUnavailable)?;

  let deadline = Instant::now() + Duration::from_secs(5);

  loop {
    match selected_session(session) {
      Ok(current) if current.lock_state == LockState::Usable => return Ok(UnlockOutcome::UnlockedExistingSession),
      Ok(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
      Ok(_) => return Err(UnlockError::OutcomeUnverified),
      Err(UnlockError::StaleSession) => return Err(UnlockError::StaleSession),
      Err(_) => return Err(UnlockError::OutcomeUnverified),
    }
  }
}

fn selected_session(selected: &GnomeSession) -> Result<GnomeSession, UnlockError> {
  let current = list_user_sessions()?.into_iter().find(|current| current.id == selected.id).ok_or(UnlockError::StaleSession)?;

  if !selected.same_login(&current) {
    return Err(UnlockError::StaleSession);
  }

  Ok(current)
}

fn observed_lock_state(locked_hint: bool) -> LockState {
  if locked_hint {
    LockState::Locked
  } else {
    LockState::Usable
  }
}

fn eligible_session(kind: &str, class: &str, remote: bool, active: bool) -> bool {
  kind == "wayland" && class == "user" && !remote && active
}

fn system_connection() -> Result<Connection, UnlockError> {
  zbus::blocking::connection::Builder::system()
    .map_err(|_| UnlockError::ServiceUnavailable)?
    .method_timeout(Duration::from_secs(3))
    .build()
    .map_err(|_| UnlockError::ServiceUnavailable)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reports_the_logind_hint_as_session_state() {
    assert_eq!(observed_lock_state(false), LockState::Usable);
    assert_eq!(observed_lock_state(true), LockState::Locked);
  }

  #[test]
  fn reused_logind_id_does_not_identify_the_old_session() {
    let old = GnomeSession {
      id: "52".into(),
      started_at_micros: 1_000,
      user: "neko".into(),
      uid: 1000,
      seat: "seat0".into(),
      lock_state: LockState::Locked,
    };
    let renewed = GnomeSession {
      started_at_micros: 2_000,
      ..old.clone()
    };

    assert_eq!(old.selector(), "linux-logind:52:1000:1000");
    assert!(!old.same_login(&renewed));
  }

  #[test]
  fn native_delivery_requires_still_active_local_gnome_session() {
    assert!(eligible_session("wayland", "user", false, true));
    assert!(!eligible_session("wayland", "user", false, false));
    assert!(!eligible_session("wayland", "user", true, true));
    assert!(!eligible_session("x11", "user", false, true));
    assert!(!eligible_session("wayland", "greeter", false, true));
  }
}
