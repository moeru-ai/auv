//! Restricted, target-local Device lock and unlock audit.
//!
//! Each append is synchronized before the policy can deliver OS input. The
//! record schema is an allowlist: it cannot hold a credential or native error
//! text, and this file is never exposed through paired Device or Run APIs.

use std::fs::File;
#[cfg(any(unix, test))]
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use super::storage_windows::{self, Creation};
use auv::devices::{DeviceEntryEffectKind, DeviceEntryErrorReason};
use auv_api_server::device_local::LocalOsPrincipal;
use serde::{Deserialize, Serialize};

pub(super) struct Audit {
  file: Mutex<File>,
  poisoned: AtomicBool,
  #[allow(dead_code)]
  path: PathBuf,
  #[cfg(windows)]
  _root_guard: File,
}

#[derive(Serialize)]
pub(super) struct Record<'a> {
  pub event: &'static str,
  pub attempt_id: &'a str,
  pub caller: &'a str,
  pub os_account_id: Option<&'a str>,
  pub user: Option<&'a str>,
  pub session_selector: Option<&'a str>,
  pub result: Option<&'static str>,
  pub at_unix_millis: u128,
}

/// Only the schema's non-secret allowlisted fields are returned to a local
/// reader. It is never exposed through paired Device or Run APIs.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AuditEntry {
  pub event: String,
  pub attempt_id: String,
  pub caller: String,
  pub os_account_id: Option<String>,
  pub user: Option<String>,
  pub session_selector: Option<String>,
  pub result: Option<String>,
  pub at_unix_millis: u128,
}

pub(super) struct AuditPage {
  pub entries: Vec<AuditEntry>,
  pub next_cursor: Option<u64>,
}

impl Audit {
  #[cfg(unix)]
  pub(super) fn open(root: &Path) -> Result<Self, DeviceEntryErrorReason> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    let metadata = std::fs::symlink_metadata(root).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;

    if !metadata.file_type().is_dir() {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    if metadata.uid() != current_euid() || metadata.permissions().mode() & 0o077 != 0 {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    let path = root.join("device-entry-audit.jsonl");
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(&path).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    let metadata = file.metadata().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;

    if !metadata.file_type().is_file() {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    if metadata.uid() != current_euid() || metadata.permissions().mode() & 0o077 != 0 {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    validate_existing(&file)?;
    // A newly created audit pathname must also survive a process crash.
    File::open(root).and_then(|directory| directory.sync_all()).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    Ok(Self {
      file: Mutex::new(file),
      poisoned: AtomicBool::new(false),
      path,
    })
  }

  #[cfg(windows)]
  pub(super) fn open(root: &Path) -> Result<Self, DeviceEntryErrorReason> {
    let root_guard = storage_windows::directory(root).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    let path = root.join("device-entry-audit.jsonl");
    let file = storage_windows::file(root, "device-entry-audit.jsonl", Creation::OpenOrCreate)
      .map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    validate_existing(&file)?;
    file.sync_all().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    Ok(Self {
      file: Mutex::new(file),
      poisoned: AtomicBool::new(false),
      path,
      _root_guard: root_guard,
    })
  }

  pub(super) fn append(&self, mut record: Record<'_>) -> Result<(), DeviceEntryErrorReason> {
    self.require_healthy()?;
    record.at_unix_millis = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?.as_millis();
    let mut bytes = serde_json::to_vec(&record).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    bytes.push(b'\n');
    let mut file = self.file.lock().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    self.require_healthy()?;
    // The Windows handle has exact SYSTEM-only security rather than the
    // standard OpenOptions append flag; reads may have moved its cursor.
    #[cfg(windows)]
    let write_result = file.seek(SeekFrom::End(0)).and_then(|_| file.write_all(&bytes)).and_then(|_| file.sync_data());
    #[cfg(unix)]
    let write_result = file.write_all(&bytes).and_then(|_| file.sync_data());

    if write_result.is_err() {
      // A short write can leave a torn JSONL record. Do not append another
      // attempt after any write or durability failure in this process.
      self.poisoned.store(true, Ordering::Release);

      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    Ok(())
  }

  /// Reads a bounded page from the local audit. Root or LocalSystem can read
  /// all accounts; other verified local principals see only their account ID.
  pub(super) fn read_for_principal(
    &self,
    principal: &LocalOsPrincipal,
    cursor: u64,
    limit: usize,
  ) -> Result<AuditPage, DeviceEntryErrorReason> {
    self.require_healthy()?;
    let own_id = match principal {
      #[cfg(unix)]
      LocalOsPrincipal::UnixUid(0) => None,
      #[cfg(unix)]
      LocalOsPrincipal::UnixUid(uid) => Some(format!("uid:{uid}")),
      #[cfg(windows)]
      LocalOsPrincipal::WindowsSid(sid) if sid == "S-1-5-18" => None,
      #[cfg(windows)]
      LocalOsPrincipal::WindowsAdministratorSid(sid) if sid.starts_with("S-1-") && sid.len() <= 128 => None,
      #[cfg(windows)]
      LocalOsPrincipal::WindowsSid(sid) if sid.starts_with("S-1-") => Some(sid.clone()),
      _ => return Err(DeviceEntryErrorReason::AuditUnavailable),
    };

    if limit == 0 || limit > 100 {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    let mut file = self.file.lock().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    self.require_healthy()?;
    let length = file.metadata().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?.len();

    if cursor > length {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    if cursor > 0 {
      file.seek(SeekFrom::Start(cursor - 1)).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
      let mut previous = [0];
      file.read_exact(&mut previous).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;

      if previous[0] != b'\n' {
        return Err(DeviceEntryErrorReason::AuditUnavailable);
      }
    }

    file.seek(SeekFrom::Start(cursor)).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    let mut reader = BufReader::new(&mut *file);
    let mut entries = Vec::new();
    let mut scanned = 0;

    while entries.len() < limit && scanned < 1000 {
      let Some(line) = read_bounded_line(&mut reader)? else {
        break;
      };

      scanned += 1;
      let entry: AuditEntry = serde_json::from_slice(&line).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;

      if own_id.as_deref().is_none_or(|id| entry.os_account_id.as_deref() == Some(id)) {
        entries.push(entry);
      }
    }

    let position = reader.stream_position().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
    Ok(AuditPage {
      entries,
      next_cursor: (position < length).then_some(position),
    })
  }

  fn require_healthy(&self) -> Result<(), DeviceEntryErrorReason> {
    if self.poisoned.load(Ordering::Acquire) {
      Err(DeviceEntryErrorReason::AuditUnavailable)
    } else {
      Ok(())
    }
  }

  /// A request-owned outcome writer can fail outside `write_all` or
  /// `sync_data`; do not admit later attempts after that interval was lost.
  pub(super) fn poison(&self) {
    self.poisoned.store(true, Ordering::Release);
  }
}

fn validate_existing(file: &File) -> Result<(), DeviceEntryErrorReason> {
  let mut copy = file.try_clone().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
  copy.seek(SeekFrom::Start(0)).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
  let mut reader = BufReader::new(copy);

  while let Some(line) = read_bounded_line(&mut reader)? {
    let _: AuditEntry = serde_json::from_slice(&line).map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;
  }

  Ok(())
}

fn read_bounded_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, DeviceEntryErrorReason> {
  const MAX_LINE_BYTES: usize = 8 * 1024;
  let mut line = Vec::new();

  loop {
    let available = reader.fill_buf().map_err(|_| DeviceEntryErrorReason::AuditUnavailable)?;

    if available.is_empty() {
      return if line.is_empty() {
        Ok(None)
      } else {
        Err(DeviceEntryErrorReason::AuditUnavailable)
      };
    }

    let take = available.iter().position(|byte| *byte == b'\n').map_or(available.len(), |position| position + 1);

    if line.len() + take > MAX_LINE_BYTES {
      return Err(DeviceEntryErrorReason::AuditUnavailable);
    }

    let ends_line = available[take - 1] == b'\n';
    line.extend_from_slice(&available[..take]);
    reader.consume(take);

    if ends_line {
      return Ok(Some(line));
    }
  }
}

#[cfg(unix)]
fn current_euid() -> u32 {
  // SAFETY: geteuid has no arguments or pointers and cannot access Rust memory.
  unsafe { libc::geteuid() }
}

pub(super) fn result_name(result: &Result<DeviceEntryEffectKind, DeviceEntryErrorReason>) -> &'static str {
  match result {
    Ok(kind) => kind.as_str(),
    Err(reason) => reason.as_str(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[cfg(unix)]
  use std::os::unix::fs::PermissionsExt;

  #[test]
  fn rejects_publicly_readable_existing_audit() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.path().join("device-entry-audit.jsonl");
    std::fs::write(&path, b"").unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    #[cfg(unix)]
    assert!(matches!(Audit::open(root.path()), Err(DeviceEntryErrorReason::AuditUnavailable)));
  }

  #[cfg(unix)]
  #[test]
  fn local_reader_filters_by_stable_uid_and_pages_with_a_bounded_cursor() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let audit = Audit::open(root.path()).unwrap();

    for (id, selector) in [
      ("uid:501", "macos:a"),
      ("uid:502", "macos:b"),
      ("uid:501", "macos:c"),
    ] {
      audit
        .append(Record {
          event: "outcome",
          attempt_id: "test-attempt",
          caller: "paired-device:test",
          os_account_id: Some(id),
          user: Some("local-user"),
          session_selector: Some(selector),
          result: Some("UNLOCKED_EXISTING_SESSION"),
          at_unix_millis: 0,
        })
        .unwrap();
    }

    let first = audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), 0, 2).unwrap();

    assert_eq!(first.entries.len(), 2);

    let second = audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), first.next_cursor.unwrap(), 2).unwrap();

    assert_eq!(second.entries.len(), 1);
    assert!(second.next_cursor.is_none());

    let own = audit.read_for_principal(&LocalOsPrincipal::UnixUid(501), 0, 100).unwrap();

    assert_eq!(own.entries.len(), 2);
    assert!(own.entries.iter().all(|entry| entry.os_account_id.as_deref() == Some("uid:501")));
    assert_eq!(audit.read_for_principal(&LocalOsPrincipal::UnixUid(502), 0, 100).unwrap().entries.len(), 1);
    assert!(matches!(audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), 1, 10), Err(DeviceEntryErrorReason::AuditUnavailable)));
  }

  #[cfg(windows)]
  #[test]
  fn local_reader_filters_by_verified_sid() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("reader-fixture.jsonl");
    let file = OpenOptions::new().create_new(true).read(true).write(true).open(&path).unwrap();
    let audit = Audit {
      _root_guard: file.try_clone().unwrap(),
      file: Mutex::new(file),
      poisoned: AtomicBool::new(false),
      path,
    };

    for sid in ["S-1-5-21-1001", "S-1-5-21-1002"] {
      audit
        .append(Record {
          event: "outcome",
          attempt_id: "test-attempt",
          caller: "paired-device:test",
          os_account_id: Some(sid),
          user: None,
          session_selector: None,
          result: Some("UNLOCKED_EXISTING_SESSION"),
          at_unix_millis: 0,
        })
        .unwrap();
    }

    let own = audit.read_for_principal(&LocalOsPrincipal::WindowsSid("S-1-5-21-1001".into()), 0, 100).unwrap();

    assert_eq!(own.entries.len(), 1);
    assert_eq!(own.entries[0].os_account_id.as_deref(), Some("S-1-5-21-1001"));

    let system = audit.read_for_principal(&LocalOsPrincipal::WindowsSid("S-1-5-18".into()), 0, 100).unwrap();

    assert_eq!(system.entries.len(), 2);

    let administrator = audit.read_for_principal(&LocalOsPrincipal::WindowsAdministratorSid("S-1-5-21-2000".into()), 0, 100).unwrap();

    assert_eq!(administrator.entries.len(), 2);
  }

  #[cfg(unix)]
  #[test]
  fn append_failure_poison_prevents_later_attempts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.path().join("audit.jsonl");
    std::fs::write(&path, b"").unwrap();
    let audit = Audit {
      file: Mutex::new(File::open(&path).unwrap()),
      poisoned: AtomicBool::new(false),
      path: path.clone(),
    };
    let record = || Record {
      event: "attempt",
      attempt_id: "test",
      caller: "paired-device:test",
      os_account_id: None,
      user: None,
      session_selector: None,
      result: None,
      at_unix_millis: 0,
    };

    assert!(matches!(audit.append(record()), Err(DeviceEntryErrorReason::AuditUnavailable)));

    *audit.file.lock().unwrap() = OpenOptions::new().append(true).open(&path).unwrap();
    assert!(matches!(audit.append(record()), Err(DeviceEntryErrorReason::AuditUnavailable)));
    assert!(std::fs::read(&path).unwrap().is_empty());
  }

  #[cfg(unix)]
  #[test]
  fn restart_rejects_torn_audit_line() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("device-entry-audit.jsonl"), b"{\"event\":\"attempt\"").unwrap();

    assert!(matches!(Audit::open(root.path()), Err(DeviceEntryErrorReason::AuditUnavailable)));
  }
}
