//! Read-only observation of the physical macOS console session.
//!
//! The daemon and the signed Aqua helper call this capability independently.
//! Sharing its parser prevents a different UID, name, UUID, or lock rule on
//! either side of credential delivery.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConsoleSession {
  selector: String,
  user: String,
  uid: u32,
  locked: bool,
}

impl ConsoleSession {
  pub fn selector(&self) -> &str {
    &self.selector
  }

  pub fn user(&self) -> &str {
    &self.user
  }

  pub fn uid(&self) -> u32 {
    self.uid
  }

  pub fn is_locked(&self) -> bool {
    self.locked
  }

  /// Lock state may change, but UID, OS account name, and login UUID must not.
  pub fn same_identity(&self, other: &Self) -> bool {
    self.uid == other.uid && self.user == other.user && self.selector == other.selector
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObserveError {
  Unavailable,
  UnknownState,
  Ambiguous,
}

/// Observe one fully logged-in physical console session from IORegistry.
///
/// `None` means no completed login. A missing lock key, malformed account
/// identity, or multiple active consoles is never treated as an unlocked user.
pub fn observe_console() -> Result<Option<ConsoleSession>, ObserveError> {
  // TODO(device-entry-other-macos-sessions): Only the physical console is
  // observed in this release. Add background sessions after owner approval.
  let registry =
    Command::new("/usr/sbin/ioreg").args(["-r", "-n", "Root", "-d", "1", "-a"]).output().map_err(|_| ObserveError::Unavailable)?;

  if !registry.status.success() {
    return Err(ObserveError::Unavailable);
  }

  // `ioreg -a` emits a binary plist. The fixed platform tool keeps plist
  // decoding behind this narrow macOS capability.
  let mut converter = Command::new("/usr/bin/plutil")
    .args(["-convert", "json", "-o", "-", "-"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|_| ObserveError::Unavailable)?;
  converter.stdin.take().ok_or(ObserveError::Unavailable)?.write_all(&registry.stdout).map_err(|_| ObserveError::Unavailable)?;
  let output = converter.wait_with_output().map_err(|_| ObserveError::Unavailable)?;

  if !output.status.success() {
    return Err(ObserveError::Unavailable);
  }

  parse_snapshot(&output.stdout)
}

fn parse_snapshot(bytes: &[u8]) -> Result<Option<ConsoleSession>, ObserveError> {
  let value: Value = serde_json::from_slice(bytes).map_err(|_| ObserveError::UnknownState)?;
  let root = value.as_array().and_then(|items| items.first()).ok_or(ObserveError::UnknownState)?;
  let locked = root.get("IOConsoleLocked").and_then(Value::as_bool).ok_or(ObserveError::UnknownState)?;
  let users = root.get("IOConsoleUsers").and_then(Value::as_array).ok_or(ObserveError::UnknownState)?;
  let mut active = users.iter().filter(|user| {
    user.get("kCGSSessionOnConsoleKey").and_then(Value::as_bool) == Some(true)
      && user.get("kCGSessionLoginDoneKey").and_then(Value::as_bool) == Some(true)
  });
  let Some(user) = active.next() else {
    return Ok(None);
  };

  if active.next().is_some() {
    return Err(ObserveError::Ambiguous);
  }

  let name = user
    .get("kCGSSessionUserNameKey")
    .and_then(Value::as_str)
    .filter(|name| !name.is_empty() && *name != "loginwindow")
    .ok_or(ObserveError::UnknownState)?;

  let uid = user
    .get("kCGSSessionUserIDKey")
    .and_then(Value::as_u64)
    .and_then(|uid| u32::try_from(uid).ok())
    .filter(|uid| *uid != 0)
    .ok_or(ObserveError::UnknownState)?;

  let uuid =
    user.get("CGSSessionUniqueSessionUUID").and_then(Value::as_str).filter(|uuid| valid_uuid(uuid)).ok_or(ObserveError::UnknownState)?;

  // NOTICE: IORegistry lock facts were observed on macOS 26.3. Installed
  // host retrieval and unlock still require a separate live gate.
  Ok(Some(ConsoleSession {
    selector: format!("macos:{uuid}"),
    user: name.to_owned(),
    uid,
    locked,
  }))
}

fn valid_uuid(uuid: &str) -> bool {
  let bytes = uuid.as_bytes();
  bytes.len() == 36
    && bytes.iter().enumerate().all(|(index, byte)| {
      if [8, 13, 18, 23].contains(&index) {
        *byte == b'-'
      } else {
        byte.is_ascii_hexdigit()
      }
    })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn snapshot(locked: Option<bool>, users: &str) -> Vec<u8> {
    let locked = locked.map(|value| format!("\"IOConsoleLocked\":{value},")).unwrap_or_default();
    format!("[{{{locked}\"IOConsoleUsers\":[{users}]}}]").into_bytes()
  }

  fn user(on_console: bool, login_done: bool) -> String {
    format!(
      "{{\"kCGSSessionOnConsoleKey\":{on_console},\"kCGSessionLoginDoneKey\":{login_done},\"kCGSSessionUserNameKey\":\"neko\",\"kCGSSessionUserIDKey\":501,\"CGSSessionUniqueSessionUUID\":\"EDFAAA3D-E075-4A31-A5F5-A066E6508D23\"}}"
    )
  }

  #[test]
  fn preserves_identity_across_lock_changes() {
    let user = user(true, true);
    let locked = parse_snapshot(&snapshot(Some(true), &user)).unwrap().unwrap();
    let usable = parse_snapshot(&snapshot(Some(false), &user)).unwrap().unwrap();

    assert!(locked.same_identity(&usable));
    assert!(locked.is_locked());
    assert!(!usable.is_locked());
    assert_eq!(locked.user(), "neko");
    assert_eq!(locked.uid(), 501);
    assert_eq!(locked.selector(), "macos:EDFAAA3D-E075-4A31-A5F5-A066E6508D23");
  }

  #[test]
  fn excludes_incomplete_login_and_rejects_unknown_lock() {
    assert!(parse_snapshot(&snapshot(Some(true), &user(true, false))).unwrap().is_none());
    assert_eq!(parse_snapshot(&snapshot(None, &user(true, true))), Err(ObserveError::UnknownState));
  }

  #[test]
  fn rejects_ambiguous_or_invalid_identity() {
    let user = user(true, true);

    assert_eq!(parse_snapshot(&snapshot(Some(true), &format!("{user},{user}"))), Err(ObserveError::Ambiguous));
    assert_eq!(parse_snapshot(&snapshot(Some(true), &user.replace("neko", "loginwindow"))), Err(ObserveError::UnknownState));
    assert_eq!(
      parse_snapshot(&snapshot(Some(true), &user.replace("EDFAAA3D-E075-4A31-A5F5-A066E6508D23", "BAD-UUID"))),
      Err(ObserveError::UnknownState)
    );
  }

  #[test]
  fn rejects_replaced_console_identity() {
    let user = user(true, true);
    let selected = parse_snapshot(&snapshot(Some(true), &user)).unwrap().unwrap();
    let changed_uuid = user.replace("EDFAAA3D-E075-4A31-A5F5-A066E6508D23", "2DC7C153-9924-4323-A561-218F4EAFA75E");
    let replacement = parse_snapshot(&snapshot(Some(false), &changed_uuid)).unwrap().unwrap();

    assert!(!selected.same_identity(&replacement));

    let changed_uid = user.replace("\"kCGSSessionUserIDKey\":501", "\"kCGSSessionUserIDKey\":502");
    let replacement = parse_snapshot(&snapshot(Some(false), &changed_uid)).unwrap().unwrap();

    assert!(!selected.same_identity(&replacement));
  }
}
