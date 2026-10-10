//! Target-local Device entry administration over its dedicated OS socket.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};

/// The control store of the local `auv serve` that these commands administer.
#[derive(Clone, Debug, Args)]
pub struct LocalStore {
  /// Store root that the local `auv serve` uses (its `--store-root`). Defaults
  /// to `.auv/store` under the current directory.
  #[arg(long, value_name = "PATH", global = true)]
  pub store_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub struct CredentialsArgs {
  #[command(flatten)]
  pub store: LocalStore,
  #[command(subcommand)]
  pub command: CredentialsCommand,
}

#[derive(Clone, Debug, Args)]
pub struct UnlockPolicyArgs {
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
  Credentials(CredentialsCommand),
  Policy(PolicyCommand),
  Audit(AuditCommand),
}

#[derive(Clone, Debug, Subcommand)]
pub enum CredentialsCommand {
  /// Store an OS login credential entered only on this Device's terminal:
  /// the login password on macOS and Linux, the Windows PIN on Windows.
  Enroll {
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

#[derive(Clone, Debug, Subcommand)]
pub enum PolicyCommand {
  /// Show whether paired Devices may unlock this Device.
  Get,
  /// Allow or reject remote unlock and session listing from paired Devices.
  Set {
    #[arg(long, action = clap::ArgAction::Set, value_parser = clap::value_parser!(bool))]
    enabled: bool,
  },
}

#[derive(Clone, Debug, Subcommand)]
pub enum AuditCommand {
  /// List audit records, oldest first.
  List {
    #[arg(long, default_value_t = 0)]
    cursor: u64,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=100))]
    limit: u32,
  },
}

/// Match the same store-root and relative-path rules as `auv serve`.
#[cfg(any(unix, windows))]
fn store_root_path(project_root: &Path, store_root: Option<&Path>) -> PathBuf {
  match store_root {
    Some(path) if path.is_absolute() => path.to_path_buf(),
    Some(path) => project_root.join(path),
    None => project_root.join(".auv").join("store"),
  }
}

#[cfg(unix)]
fn socket_path(project_root: &Path, store_root: Option<&Path>) -> Result<PathBuf, String> {
  let store_root = store_root_path(project_root, store_root);
  auv_api_client::device_local::unix_socket_path(&store_root).map_err(|error| format!("{}: {error}", no_local_daemon(&store_root)))
}

/// The local service exists only while `auv serve` runs on the same store, so
/// a missing service usually means a different `--store-root`.
#[cfg(any(unix, windows))]
fn no_local_daemon(store_root: &Path) -> String {
  format!(
    "no local `auv serve` owned by this user is running on store {}; start it, or pass --store-root with the store that `auv serve` uses",
    store_root.display()
  )
}

#[cfg(any(unix, windows))]
pub async fn run(store_root: Option<&Path>, request: LocalRequest, project_root: &Path) -> Result<i32, String> {
  use auv_api_client::device_local::DeviceLocalClient;
  use auv_api_proto::auv::api::daemon::v1 as proto;

  #[cfg(unix)]
  let mut client = {
    let path = socket_path(project_root, store_root)?;
    let missing = || no_local_daemon(&store_root_path(project_root, store_root));
    // TODO(device-local-cross-uid): A root CLI cannot administer a user-owned
    // daemon until an explicit target-UID path and server-identity gate exists.
    auv_api_client::device_local::verify_unix_socket_directory(&path).map_err(|error| format!("{} ({error})", missing()))?;
    DeviceLocalClient::connect_unix(&path).await.map_err(|error| format!("{} ({error})", missing()))?
  };
  #[cfg(windows)]
  let mut client = {
    let store_root = store_root_path(project_root, store_root);
    DeviceLocalClient::connect_windows(&store_root).await.map_err(|error| format!("{} ({error})", no_local_daemon(&store_root)))?
  };
  let service = client.service();

  match request {
    LocalRequest::Credentials(CredentialsCommand::Enroll { user }) => {
      // Each platform host accepts exactly one credential kind, so the CLI
      // derives it instead of asking for a choice that has one valid answer.
      // TODO(device-entry-windows-password): The installed Windows worker
      // targets only the PIN provider. When OS-password enrollment passes an
      // owner-observed locked-session gate there, add an explicit choice here.
      #[cfg(windows)]
      let kind = proto::EnrollmentCredentialKind::WindowsPin;
      #[cfg(not(windows))]
      let kind = proto::EnrollmentCredentialKind::OsPassword;

      let mut credential = read_hidden_credential()?;
      let request = proto::EnrollRequest {
        user,
        // The generated gRPC encoder takes ownership of this buffer. The
        // temporary terminal buffer is wiped on all earlier error paths.
        credential: std::mem::take(&mut *credential),
        credential_kind: kind as i32,
        // TODO(device-entry-plaintext): No backend offers the plaintext
        // fallback yet; expose a storage choice only when one does.
        storage_kind: proto::EnrollmentStorageKind::Protected as i32,
      };
      let enrollment = service
        .enroll(request)
        .await
        // EnrollRequest contains secret bytes; never format its error body.
        .map_err(|status| format!("enrollment failed ({})", status.code()))?
        .into_inner()
        .enrollment;
      print_enrollment(enrollment)?;
    }
    LocalRequest::Credentials(CredentialsCommand::Get { user }) => {
      let enrollment = service
        .get_enrollment(proto::GetEnrollmentRequest { user })
        .await
        .map_err(|status| format!("get enrollment failed ({})", status.code()))?
        .into_inner()
        .enrollment;
      print_enrollment(enrollment)?;
    }
    LocalRequest::Credentials(CredentialsCommand::List) => {
      let entries = service
        .list_enrollments(proto::ListEnrollmentsRequest {})
        .await
        .map_err(|status| format!("list enrollments failed ({})", status.code()))?
        .into_inner()
        .enrollments;

      for enrollment in entries {
        print_enrollment(Some(enrollment))?;
      }
    }
    LocalRequest::Credentials(CredentialsCommand::Remove { user }) => {
      service
        .remove_enrollment(proto::RemoveEnrollmentRequest { user })
        .await
        .map_err(|status| format!("remove enrollment failed ({})", status.code()))?;
      println!("enrollment removed");
    }
    LocalRequest::Policy(command) => {
      let enabled = match command {
        PolicyCommand::Get => {
          service
            .get_policy(proto::GetPolicyRequest {})
            .await
            .map_err(|status| format!("get policy failed ({})", status.code()))?
            .into_inner()
            .enabled
        }
        PolicyCommand::Set { enabled } => {
          service
            .set_policy(proto::SetPolicyRequest { enabled })
            .await
            .map_err(|status| format!("set policy failed ({})", status.code()))?
            .into_inner()
            .enabled
        }
      };
      println!("remote unlock enabled: {enabled}");
    }

    LocalRequest::Audit(AuditCommand::List { cursor, limit }) => {
      let page = service
        .list_audit(proto::ListAuditRequest { cursor, limit })
        .await
        .map_err(|status| format!("list audit failed ({})", status.code()))?
        .into_inner();

      for entry in page.entries {
        println!(
          "{}\t{}\t{}\t{}\t{}\t{}",
          entry.at_unix_millis,
          entry.event,
          entry.attempt_id,
          entry.user.unwrap_or_default(),
          entry.session_selector.unwrap_or_default(),
          entry.result.unwrap_or_default()
        );
      }

      if let Some(next) = page.next_cursor {
        println!("next cursor: {next}");
      }
    }
  }

  Ok(0)
}

#[cfg(not(any(unix, windows)))]
pub async fn run(_store_root: Option<&Path>, _request: LocalRequest, _project_root: &Path) -> Result<i32, String> {
  Err("local Device administration requires a supported OS-local transport".to_string())
}

#[cfg(any(unix, windows))]
fn print_enrollment(enrollment: Option<auv_api_proto::auv::api::daemon::v1::Enrollment>) -> Result<(), String> {
  use auv_api_proto::auv::api::daemon::v1::{EnrollmentState, EnrollmentStorageKind};

  let enrollment = enrollment.ok_or_else(|| "the local service returned no enrollment".to_string())?;
  let state = match EnrollmentState::try_from(enrollment.state) {
    Ok(EnrollmentState::Ready) => "ready",
    Ok(EnrollmentState::Suspended) => "suspended",
    Ok(EnrollmentState::Pending) => "pending",
    _ => "unknown",
  };
  let storage = match EnrollmentStorageKind::try_from(enrollment.storage_kind) {
    Ok(EnrollmentStorageKind::Protected) => "protected",
    Ok(EnrollmentStorageKind::PlaintextFile) => "plaintext-file",
    _ => "unknown",
  };
  println!("{}\t{}\t{}\t{}", enrollment.user, enrollment.os_account_id, state, storage);
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
    let socket = socket_path(root.path(), Some(Path::new("state"))).unwrap();

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
    let expected = socket_path(root.path(), Some(Path::new("state"))).unwrap();
    symlink(root.path(), store.join("control")).unwrap();

    assert_eq!(socket_path(root.path(), Some(Path::new("state"))).unwrap(), expected);
  }
}
