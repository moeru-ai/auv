//! Target-local Device entry administration over its dedicated OS socket.

use std::path::{Path, PathBuf};

use auv_cli_common::TableRow;
use clap::{Args, Subcommand};

/// Options shared by every target-local command: which local `auv serve` to
/// administer, and how to render the result.
#[derive(Clone, Debug, Args)]
pub struct LocalStore {
  /// Store root that the local `auv serve` uses (its `--store-root`). Defaults
  /// to the store of the discovered local daemon, then `.auv/store` under the
  /// current directory.
  #[arg(long, value_name = "PATH", global = true)]
  pub store_root: Option<PathBuf>,
  /// Render machine-readable JSON.
  #[arg(long, global = true)]
  pub json: bool,
}

#[derive(Clone, Debug, Args)]
pub struct EnrollmentsArgs {
  #[command(flatten)]
  pub store: LocalStore,
  #[command(subcommand)]
  pub command: EnrollmentsCommand,
}

#[derive(Clone, Debug, Args)]
pub struct AccessPolicyArgs {
  #[command(flatten)]
  pub store: LocalStore,
  #[command(subcommand)]
  pub command: PolicyCommand,
}

#[derive(Clone, Debug, Args)]
pub struct AuditArgs {
  #[command(flatten)]
  pub store: LocalStore,
  #[command(subcommand)]
  pub command: AuditCommand,
}

/// One target-local request, whichever `auv devices` subcommand parsed it.
#[derive(Clone, Debug)]
pub enum LocalRequest {
  Enrollments(EnrollmentsCommand),
  Policy(PolicyCommand),
  Audit(AuditCommand),
}

#[derive(Clone, Debug, Subcommand)]
pub enum EnrollmentsCommand {
  /// Enroll an OS account: store its login credential, entered only on this
  /// Device's terminal (the login password on macOS and Linux, the Windows
  /// PIN on Windows).
  Add {
    /// OS account to enroll. An administrator may enroll another account.
    #[arg(long)]
    user: String,
  },
  /// Inspect one account's enrollment metadata.
  Get {
    /// OS account whose enrollment to show.
    #[arg(long)]
    user: String,
  },
  /// List visible enrollment metadata.
  List,
  /// Delete one account's enrollment.
  Remove {
    /// OS account whose enrollment to delete.
    #[arg(long)]
    user: String,
  },
}

/// One switch covers listing, locking and unlocking: a paired Device may do
/// all of them on this Device's sessions, or none.
#[derive(Clone, Debug, Subcommand)]
pub enum PolicyCommand {
  /// Show whether paired Devices may list, lock and unlock sessions here.
  Status,
  /// Let paired Devices list, lock and unlock sessions on this Device.
  Enable,
  /// Stop paired Devices from listing, locking or unlocking sessions here.
  /// Enrollments are kept.
  Disable,
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuditCommand {
  /// List audit records, oldest first.
  List {
    /// Continue from the token that the previous page printed.
    #[arg(long, value_name = "TOKEN")]
    page_token: Option<String>,
    /// Records per page.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=100))]
    limit: u32,
  },
}

/// Where the administered store came from, so a missing service can say so.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StoreSource {
  Flag,
  Discovered,
  Default,
}

/// Picks the store whose local service to contact: `--store-root`, then the
/// store that the discovered local daemon published, then `auv serve`'s own
/// default. Relative flags follow `auv serve`'s rule and resolve against the
/// current directory.
fn local_store(
  project_root: &Path,
  flag: Option<&Path>,
  discovered: impl FnOnce() -> Result<Option<PathBuf>, String>,
) -> Result<(PathBuf, StoreSource), String> {
  if let Some(path) = flag {
    let path = if path.is_absolute() {
      path.to_path_buf()
    } else {
      project_root.join(path)
    };
    return Ok((path, StoreSource::Flag));
  }
  if let Some(path) = discovered()? {
    return Ok((path, StoreSource::Discovered));
  }
  Ok((project_root.join(".auv").join("store"), StoreSource::Default))
}

#[cfg(unix)]
fn socket_path(store_root: &Path, source: StoreSource) -> Result<PathBuf, String> {
  auv_api_client::device_local::unix_socket_path(store_root).map_err(|error| format!("{} ({error})", no_local_daemon(store_root, source)))
}

/// The local service exists only while `auv serve` runs on the same store.
fn no_local_daemon(store_root: &Path, source: StoreSource) -> String {
  let store = store_root.display();
  match source {
    StoreSource::Flag => format!("no local `auv serve` owned by this user is running on store {store} (from --store-root)"),
    StoreSource::Discovered => {
      format!(
        "the discovered local `auv serve` (store {store}) is not answering; check that it is still running; if it is, its `auv` build may predate this client's Device-local pipe naming — restart `auv serve` with this build instead of pairing again"
      )
    }
    StoreSource::Default => format!(
      "no local `auv serve` is registered, and none runs on the default store {store}; start `auv serve`, or pass --store-root with the store it uses"
    ),
  }
}

#[cfg(any(unix, windows))]
pub async fn run(options: LocalStore, request: LocalRequest, project_root: &Path) -> Result<i32, String> {
  use auv_api_client::device_local::DeviceLocalClient;
  use auv_api_proto::auv::api::daemon::v1 as proto;

  let (store_root, source) =
    local_store(project_root, options.store_root.as_deref(), || auv::discovery::discovered_store_root().map_err(|error| error.to_string()))?;
  let missing = |error: &dyn std::fmt::Display| format!("{} ({error})", no_local_daemon(&store_root, source));
  #[cfg(unix)]
  let mut client = {
    let path = socket_path(&store_root, source)?;
    // TODO(device-local-cross-uid): A root CLI cannot administer a user-owned
    // daemon until an explicit target-UID path and server-identity gate exists.
    auv_api_client::device_local::verify_unix_socket_directory(&path).map_err(|error| missing(&error))?;
    DeviceLocalClient::connect_unix(&path).await.map_err(|error| missing(&error))?
  };
  #[cfg(windows)]
  let mut client = DeviceLocalClient::connect_windows(&store_root).await.map_err(|error| missing(&error))?;
  let service = client.service();
  let json = options.json;

  match request {
    LocalRequest::Enrollments(EnrollmentsCommand::Add { user }) => {
      let mut credential = read_hidden_credential()?;
      let request = proto::EnrollRequest {
        user,
        // The generated gRPC encoder takes ownership of this buffer. The
        // temporary terminal buffer is wiped on all earlier error paths.
        credential: std::mem::take(&mut *credential),
      };
      let enrollment = service
        .enroll(request)
        .await
        // EnrollRequest contains secret bytes; never format its error body.
        .map_err(|status| format!("enrollment failed ({})", status.code()))?
        .into_inner()
        .enrollment;
      print_enrollments(&[required_enrollment(enrollment)?], json, false)?;
    }
    LocalRequest::Enrollments(EnrollmentsCommand::Get { user }) => {
      let enrollment = service
        .get_enrollment(proto::GetEnrollmentRequest { user })
        .await
        .map_err(|status| format!("get enrollment failed ({})", status.code()))?
        .into_inner()
        .enrollment;
      print_enrollments(&[required_enrollment(enrollment)?], json, false)?;
    }
    LocalRequest::Enrollments(EnrollmentsCommand::List) => {
      let entries = service
        .list_enrollments(proto::ListEnrollmentsRequest {})
        .await
        .map_err(|status| format!("list enrollments failed ({})", status.code()))?
        .into_inner()
        .enrollments;

      print_enrollments(&entries, json, true)?;
    }
    LocalRequest::Enrollments(EnrollmentsCommand::Remove { user }) => {
      service
        .remove_enrollment(proto::RemoveEnrollmentRequest { user: user.clone() })
        .await
        .map_err(|status| format!("remove enrollment failed ({})", status.code()))?;
      if json {
        print_json(&serde_json::json!({ "removed": user }))?;
      } else {
        println!("enrollment removed: {user}");
      }
    }
    LocalRequest::Policy(command) => {
      let enabled = match command {
        PolicyCommand::Status => {
          service
            .get_policy(proto::GetPolicyRequest {})
            .await
            .map_err(|status| format!("read access policy failed ({})", status.code()))?
            .into_inner()
            .enabled
        }
        PolicyCommand::Enable | PolicyCommand::Disable => {
          service
            .set_policy(proto::SetPolicyRequest {
              enabled: matches!(command, PolicyCommand::Enable),
            })
            .await
            .map_err(|status| format!("change access policy failed ({})", status.code()))?
            .into_inner()
            .enabled
        }
      };
      if json {
        print_json(&serde_json::json!({ "enabled": enabled }))?;
      } else {
        let state = if enabled { "enabled" } else { "disabled" };
        println!("session access from paired Devices: {state}");
      }
    }

    LocalRequest::Audit(AuditCommand::List { page_token, limit }) => {
      let page = service
        .list_audit(proto::ListAuditRequest {
          page_token: page_token.unwrap_or_default(),
          limit,
        })
        .await
        .map_err(|status| format!("list audit failed ({})", status.code()))?
        .into_inner();
      print_audit(page, json)?;
    }
  }

  Ok(0)
}

#[cfg(not(any(unix, windows)))]
pub async fn run(_store_root: Option<&Path>, _request: LocalRequest, _project_root: &Path) -> Result<i32, String> {
  Err("local Device administration requires a supported OS-local transport".to_string())
}

#[cfg(any(unix, windows))]
fn required_enrollment(
  enrollment: Option<auv_api_proto::auv::api::daemon::v1::Enrollment>,
) -> Result<auv_api_proto::auv::api::daemon::v1::Enrollment, String> {
  enrollment.ok_or_else(|| "the local service returned no enrollment".to_string())
}

#[cfg(any(unix, windows))]
fn enrollment_state(enrollment: &auv_api_proto::auv::api::daemon::v1::Enrollment) -> &'static str {
  use auv_api_proto::auv::api::daemon::v1::EnrollmentState;

  match EnrollmentState::try_from(enrollment.state) {
    Ok(EnrollmentState::Ready) => "ready",
    Ok(EnrollmentState::Suspended) => "suspended",
    Ok(EnrollmentState::Pending) => "pending",
    _ => "unknown",
  }
}

#[derive(TableRow)]
struct EnrollmentTableRow {
  user: String,
  #[table(header = "OS ACCOUNT")]
  os_account_id: String,
  state: &'static str,
}

/// Renders enrollments as a table or JSON. A list renders a JSON array even
/// when it has one entry; a single lookup renders one object.
#[cfg(any(unix, windows))]
fn print_enrollments(enrollments: &[auv_api_proto::auv::api::daemon::v1::Enrollment], json: bool, list: bool) -> Result<(), String> {
  if json {
    let values = enrollments
      .iter()
      .map(|enrollment| {
        serde_json::json!({
          "user": enrollment.user,
          "os_account_id": enrollment.os_account_id,
          "state": enrollment_state(enrollment),
        })
      })
      .collect::<Vec<_>>();
    return match (list, values.as_slice()) {
      (false, [value]) => print_json(value),
      _ => print_json(&values),
    };
  }
  let rows = enrollments
    .iter()
    .map(|enrollment| EnrollmentTableRow {
      user: enrollment.user.clone(),
      os_account_id: enrollment.os_account_id.clone(),
      state: enrollment_state(enrollment),
    })
    .collect::<Vec<_>>();
  crate::commands::devices::print_table(&rows, "(no enrollments)");
  Ok(())
}

#[derive(TableRow)]
struct AuditTableRow {
  time: String,
  event: String,
  user: Option<String>,
  session: Option<String>,
  result: Option<String>,
  caller: String,
  attempt: String,
}

/// Audit timestamps are UTC milliseconds; render them as RFC 3339.
fn audit_time(at_unix_millis: u64) -> String {
  i64::try_from(at_unix_millis)
    .ok()
    .and_then(chrono::DateTime::from_timestamp_millis)
    .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    .unwrap_or_else(|| at_unix_millis.to_string())
}

#[cfg(any(unix, windows))]
fn print_audit(page: auv_api_proto::auv::api::daemon::v1::ListAuditResponse, json: bool) -> Result<(), String> {
  if json {
    let entries = page
      .entries
      .iter()
      .map(|entry| {
        serde_json::json!({
          "time": audit_time(entry.at_unix_millis),
          "at_unix_millis": entry.at_unix_millis,
          "event": entry.event,
          "attempt_id": entry.attempt_id,
          "caller": entry.caller,
          "os_account_id": entry.os_account_id,
          "user": entry.user,
          "session_selector": entry.session_selector,
          "result": entry.result,
        })
      })
      .collect::<Vec<_>>();
    let next = (!page.next_page_token.is_empty()).then_some(page.next_page_token.as_str());
    return print_json(&serde_json::json!({ "entries": entries, "next_page_token": next }));
  }
  let rows = page
    .entries
    .into_iter()
    .map(|entry| AuditTableRow {
      time: audit_time(entry.at_unix_millis),
      event: entry.event,
      user: entry.user,
      session: entry.session_selector,
      result: entry.result,
      caller: entry.caller,
      attempt: entry.attempt_id,
    })
    .collect::<Vec<_>>();
  crate::commands::devices::print_table(&rows, "(no audit records)");
  if !page.next_page_token.is_empty() {
    println!("more records: auv devices access audit list --page-token {}", page.next_page_token);
  }
  Ok(())
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
  println!("{}", serde_json::to_string_pretty(value).map_err(|error| format!("failed to encode JSON output: {error}"))?);
  Ok(())
}

#[cfg(unix)]
fn read_hidden_credential() -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
  use std::fs::OpenOptions;
  use std::io::{Read as _, Write as _};

  use rustix::termios::{LocalModes, OptionalActions, tcgetattr, tcsetattr};
  use zeroize::{Zeroize as _, Zeroizing};

  // A separate controlling terminal prevents a pipe, process argument, or
  // environment variable from silently becoming a credential source.
  let mut tty = OpenOptions::new()
    .read(true)
    .write(true)
    .open("/dev/tty")
    .map_err(|_| "enrollment requires this Device's interactive terminal".to_string())?;

  let original = tcgetattr(&tty).map_err(|_| "failed to read terminal mode".to_string())?;
  let restoration_tty = tty.try_clone().map_err(|_| "failed to retain terminal mode".to_string())?;
  let mut hidden = original.clone();
  hidden.local_modes.remove(LocalModes::ECHO | LocalModes::ECHONL);
  tcsetattr(&tty, OptionalActions::Now, &hidden).map_err(|_| "failed to hide terminal input".to_string())?;
  struct Restore {
    tty: std::fs::File,
    original: rustix::termios::Termios,
    restored: bool,
  }

  impl Restore {
    fn restore(&mut self) -> Result<(), String> {
      rustix::termios::tcsetattr(&self.tty, rustix::termios::OptionalActions::Now, &self.original)
        .map_err(|_| "failed to restore terminal mode".to_string())?;
      self.restored = true;
      Ok(())
    }
  }

  impl Drop for Restore {
    fn drop(&mut self) {
      if !self.restored {
        let _ = rustix::termios::tcsetattr(&self.tty, rustix::termios::OptionalActions::Now, &self.original);
      }
    }
  }

  let mut restore = Restore {
    tty: restoration_tty,
    original,
    restored: false,
  };
  tty.write_all(b"OS login password (this Device only): ").map_err(|_| "failed to write terminal prompt".to_string())?;
  tty.flush().map_err(|_| "failed to flush terminal prompt".to_string())?;
  let mut credential = Zeroizing::new(Vec::new());

  loop {
    let mut byte = [0_u8; 1];
    let count = tty.read(&mut byte).map_err(|_| "failed to read terminal credential".to_string())?;

    if count == 0 {
      return Err("terminal closed before credential was entered".to_string());
    }

    if byte[0] == b'\n' {
      break;
    }

    if credential.len() >= 1024 {
      return Err("credential exceeds the 1024-byte limit".to_string());
    }

    credential.push(byte[0]);
    byte.zeroize();
  }

  tty.write_all(b"\n").map_err(|_| "failed to finish terminal prompt".to_string())?;
  restore.restore()?;

  if credential.is_empty() {
    return Err("credential cannot be empty".to_string());
  }

  std::str::from_utf8(&credential).map_err(|_| "credential must be UTF-8".to_string())?;
  Ok(credential)
}

#[cfg(windows)]
fn read_hidden_credential() -> Result<zeroize::Zeroizing<Vec<u8>>, String> {
  use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

  use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE};
  use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
  use windows::Win32::System::Console::{
    CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, GetConsoleMode, ReadConsoleW, SetConsoleMode, WriteConsoleW,
  };
  use windows::core::w;
  use zeroize::Zeroizing;

  // CONIN$ and CONOUT$ require an attached local console even when standard
  // handles were redirected. Never accept PIN bytes from stdin or a pipe.
  // NOTICE(device-local-console): ReadConsoleW in line mode lets the console
  // host edit the entry while ENABLE_ECHO_INPUT is off. See Microsoft's
  // ReadConsole and GetConsoleMode documentation. Revisit this only if a
  // target-local GUI credential surface is approved.
  let input = unsafe {
    CreateFileW(
      w!("CONIN$"),
      GENERIC_READ.0 | GENERIC_WRITE.0,
      FILE_SHARE_READ | FILE_SHARE_WRITE,
      None,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL,
      HANDLE::default(),
    )
  }
  .map_err(|_| "enrollment requires this Device's interactive console".to_string())?;
  // SAFETY: CreateFileW returned one owned handle, transferred here.
  let input = unsafe { OwnedHandle::from_raw_handle(input.0) };
  let output = unsafe {
    CreateFileW(
      w!("CONOUT$"),
      GENERIC_WRITE.0,
      FILE_SHARE_READ | FILE_SHARE_WRITE,
      None,
      OPEN_EXISTING,
      FILE_ATTRIBUTE_NORMAL,
      HANDLE::default(),
    )
  }
  .map_err(|_| "enrollment requires this Device's interactive console".to_string())?;
  // SAFETY: CreateFileW returned one owned handle, transferred here.
  let output = unsafe { OwnedHandle::from_raw_handle(output.0) };
  let input_handle = HANDLE(input.as_raw_handle());
  let output_handle = HANDLE(output.as_raw_handle());
  let mut original = CONSOLE_MODE(0);
  // SAFETY: The attached console input handle and output mode pointer are live.
  unsafe { GetConsoleMode(input_handle, &mut original) }.map_err(|_| "failed to read console mode".to_string())?;
  struct RestoreMode(HANDLE, CONSOLE_MODE);
  impl Drop for RestoreMode {
    fn drop(&mut self) {
      // SAFETY: The input handle stays owned through this guard's drop.
      let _ = unsafe { SetConsoleMode(self.0, self.1) };
    }
  }

  let _restore = RestoreMode(input_handle, original);
  let hidden = CONSOLE_MODE((original.0 | ENABLE_LINE_INPUT.0) & !ENABLE_ECHO_INPUT.0);
  // SAFETY: The input handle remains live and this mode preserves line editing.
  unsafe { SetConsoleMode(input_handle, hidden) }.map_err(|_| "failed to hide console input".to_string())?;
  let prompt = "Windows PIN (this Device only): ".encode_utf16().collect::<Vec<_>>();
  // SAFETY: The output handle is a console screen buffer; prompt contains no secret.
  unsafe { WriteConsoleW(output_handle, &prompt, None, None) }.map_err(|_| "failed to write console prompt".to_string())?;
  let mut buffer = Zeroizing::new(vec![0u16; 256]);
  let mut count = 0u32;
  // SAFETY: The buffer has room for 256 UTF-16 code units and is live for the read.
  unsafe { ReadConsoleW(input_handle, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut count, None) }
    .map_err(|_| "failed to read console credential".to_string())?;

  let newline = "\r\n".encode_utf16().collect::<Vec<_>>();
  // SAFETY: The output handle remains a live console screen buffer.
  unsafe { WriteConsoleW(output_handle, &newline, None, None) }.map_err(|_| "failed to finish console prompt".to_string())?;
  let entered = &buffer[..count as usize];
  let entered = entered.strip_suffix(&[b'\r' as u16, b'\n' as u16]).or_else(|| entered.strip_suffix(&[b'\n' as u16])).unwrap_or(entered);

  if entered.is_empty() || entered.len() > 128 || entered.iter().any(|unit| *unit == 0) {
    return Err("credential must contain 1..=128 UTF-16 characters".to_string());
  }

  let text = Zeroizing::new(String::from_utf16(entered).map_err(|_| "credential must be valid Unicode".to_string())?);
  Ok(Zeroizing::new(text.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  #[cfg(unix)]
  fn socket_is_resolved_only_from_the_local_store() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("state");
    std::fs::create_dir(&store).unwrap();
    std::fs::create_dir(store.join("control")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(store.join("control"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = socket_path(&store, StoreSource::Flag).unwrap();

    assert!(socket.starts_with(std::fs::canonicalize("/tmp").unwrap()));
    assert_eq!(socket.file_name().unwrap(), "socket");
    assert_eq!(socket, auv_api_client::device_local::unix_socket_path(&store).unwrap());
  }

  #[test]
  #[cfg(unix)]
  fn replacing_control_directory_cannot_redirect_local_socket() {
    use std::os::unix::fs::symlink;

    // ROOT CAUSE:
    //
    // If the socket name follows a replaceable control directory's UID, a
    // privileged CLI can connect to an attacker-owned local service.
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("state");
    std::fs::create_dir(&store).unwrap();
    let expected = socket_path(&store, StoreSource::Flag).unwrap();
    symlink(root.path(), store.join("control")).unwrap();

    assert_eq!(socket_path(&store, StoreSource::Flag).unwrap(), expected);
  }

  #[test]
  fn store_flag_wins_then_the_discovered_daemon_then_the_default() {
    let project = Path::new("/work/project");
    let discovered = || Ok(Some(PathBuf::from("/home/neko/.local/state/auv/daemon-store")));

    assert_eq!(local_store(project, Some(Path::new("state")), discovered).unwrap(), (project.join("state"), StoreSource::Flag));
    assert_eq!(local_store(project, Some(Path::new("/srv/store")), discovered).unwrap(), (PathBuf::from("/srv/store"), StoreSource::Flag));
    assert_eq!(
      local_store(project, None, discovered).unwrap(),
      (PathBuf::from("/home/neko/.local/state/auv/daemon-store"), StoreSource::Discovered)
    );
    assert_eq!(local_store(project, None, || Ok(None)).unwrap(), (project.join(".auv").join("store"), StoreSource::Default));
  }

  #[test]
  fn discovered_store_is_not_consulted_when_a_flag_is_given() {
    // An unreadable discovery descriptor must not block an explicit store.
    let (store, source) = local_store(Path::new("/work"), Some(Path::new("/srv/store")), || Err("broken descriptor".into())).unwrap();

    assert_eq!((store.as_path(), source), (Path::new("/srv/store"), StoreSource::Flag));
    assert!(local_store(Path::new("/work"), None, || Err("broken descriptor".into())).is_err());
  }

  #[test]
  fn missing_service_message_names_where_the_store_came_from() {
    let store = Path::new("/srv/store");

    assert!(no_local_daemon(store, StoreSource::Flag).contains("(from --store-root)"));
    assert!(no_local_daemon(store, StoreSource::Discovered).contains("discovered local `auv serve`"));
    assert!(no_local_daemon(store, StoreSource::Default).contains("pass --store-root"));
  }

  #[test]
  fn audit_time_renders_utc_milliseconds_as_rfc3339() {
    assert_eq!(audit_time(1_760_140_800_123), "2025-10-11T00:00:00.123Z");
    assert_eq!(audit_time(u64::MAX), u64::MAX.to_string());
  }
}
