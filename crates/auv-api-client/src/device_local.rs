//! Dedicated target-local Device control transport.
//!
//! This client deliberately has no TCP, paired Device, or discovery constructor.

use std::path::Path;

#[cfg(unix)]
use std::path::PathBuf;

use auv_api_proto::auv::api::daemon::v1::device_local_service_client::DeviceLocalServiceClient;
use tonic::transport::Channel;
#[cfg(unix)]
use tonic::transport::Endpoint;

#[cfg(windows)]
mod windows_pipe;

/// A DeviceLocalService connection made only through a verified local IPC peer.
pub struct DeviceLocalClient {
  service: DeviceLocalServiceClient<Channel>,
}

impl DeviceLocalClient {
  /// Connect to a target-local socket only when its peer has this process's UID.
  #[cfg(unix)]
  pub async fn connect_unix(path: &Path) -> Result<Self, tonic::transport::Error> {
    let path = path.to_path_buf();
    // Tonic requires an HTTP origin; the connector discards it and opens only
    // the caller-selected Unix socket. Never route this through Client::connect.
    let channel = Endpoint::from_static("http://localhost")
      .connect_with_connector(tower::service_fn(move |_: http::Uri| {
        let path = path.clone();
        async move {
          let stream = tokio::net::UnixStream::connect(path).await?;

          if stream.peer_cred()?.uid() != current_euid() {
            return Err(std::io::Error::new(
              std::io::ErrorKind::PermissionDenied,
              "Device-local service peer UID differs from this process",
            ));
          }

          Ok(hyper_util::rt::TokioIo::new(stream))
        }
      }))
      .await?;
    Ok(Self {
      service: DeviceLocalServiceClient::new(channel),
    })
  }

  /// Connect to the dedicated local pipe after verifying its server process
  /// runs as this process's own user, the per-user daemon for `store_root`.
  #[cfg(windows)]
  pub async fn connect_windows(store_root: &Path) -> Result<Self, tonic::transport::Error> {
    let store_root = store_root.to_path_buf();
    let channel = tonic::transport::Endpoint::from_static("http://localhost")
      .connect_with_connector(tower::service_fn(move |_: http::Uri| {
        let store_root = store_root.clone();
        async move {
          // The shared normalization rule lives in `canonical_pipe_name`; both
          // the plain and verbatim spellings of one store resolve to one pipe.
          let name = canonical_pipe_name(&store_root)?;
          windows_pipe::open_verified(&name).await.map(hyper_util::rt::TokioIo::new)
        }
      }))
      .await?;
    Ok(Self {
      service: DeviceLocalServiceClient::new(channel),
    })
  }

  /// Access the local-only generated service after the socket connection.
  pub fn service(&mut self) -> &mut DeviceLocalServiceClient<Channel> {
    &mut self.service
  }
}

/// A short, store-specific socket path for the target-local service. The
/// canonical store path keeps CLI and daemon names equal across symlinked
/// project roots; the private child of the system temporary directory keeps
/// deep project paths out of the Unix socket address limit. The effective UID
/// fixes the authority independently of a user-replaceable store parent.
#[cfg(unix)]
pub fn unix_socket_path(store_root: &Path) -> std::io::Result<PathBuf> {
  use std::os::unix::ffi::OsStrExt;
  use std::os::unix::fs::{MetadataExt, PermissionsExt};

  use sha2::{Digest as _, Sha256};

  let temporary_root = std::fs::canonicalize("/tmp")?;
  let temporary_metadata = std::fs::metadata(&temporary_root)?;

  if !temporary_metadata.is_dir() || temporary_metadata.uid() != 0 || temporary_metadata.permissions().mode() & 0o1000 == 0 {
    return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "system temporary directory must be root-owned and sticky"));
  }

  let canonical_store = std::fs::canonicalize(store_root)?;
  let owner = current_euid();
  let digest = Sha256::digest(canonical_store.as_os_str().as_bytes());
  let suffix = digest[..12].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
  let directory = temporary_root.join(format!("auv-device-local-{owner}-{suffix}"));
  Ok(directory.join("socket"))
}

/// Reject a substituted or accessible socket parent before sending local
/// enrollment credentials to the service at the derived path.
#[cfg(unix)]
pub fn verify_unix_socket_directory(socket_path: &Path) -> std::io::Result<()> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};

  let owner = current_euid();
  let parent = socket_path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "socket has no parent"))?;
  let metadata = std::fs::symlink_metadata(parent)?;

  if !metadata.file_type().is_dir() || metadata.uid() != owner || metadata.permissions().mode() & 0o777 != 0o700 {
    return Err(std::io::Error::new(
      std::io::ErrorKind::PermissionDenied,
      "Device-local socket directory must be owned by this process with mode 0700",
    ));
  }

  Ok(())
}

#[cfg(unix)]
fn current_euid() -> u32 {
  // SAFETY: geteuid has no pointers, arguments, or preconditions.
  unsafe { libc::geteuid() }
}

/// Stable, store-specific Windows pipe name shared by a local host and CLI.
/// The path is never placed in an API request or credential-bearing argument.
///
/// Production callers use [`canonical_pipe_name`], which normalizes the store
/// root first; this function is the unchanged legacy entry kept for external
/// callers and hashes the path bytes exactly as given, so handing it two
/// spellings of one directory yields two different names.
// ROOT CAUSE (Windows health audit F-PIPE-1):
//
// The daemon bound the hash of its plain-form store root while the CLI dialed
// the hash of the descriptor's verbatim-canonical form, so
// `auv devices access enrollments|policy|audit` on the documented default
// path dialed a pipe that was never bound.
//
// Before the fix, every caller hashed its input bytes unchanged through this
// function. The fix moves production callers to [`canonical_pipe_name`],
// which canonicalizes before hashing and keeps older CLIs compatible with a
// fixed daemon: they hash the descriptor's verbatim form as-is, and
// canonicalizing a verbatim path is the identity on Windows.
#[cfg(windows)]
pub fn named_pipe_name(store_root: &Path) -> String {
  use std::os::windows::ffi::OsStrExt;

  use sha2::{Digest as _, Sha256};

  let mut hash = Sha256::new();

  for unit in store_root.as_os_str().encode_wide() {
    hash.update(unit.to_le_bytes());
  }

  let digest = hash.finalize();
  let suffix = digest[..12].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
  format!("auv-device-local-{suffix}")
}

/// The production Device-local pipe name: the legacy hash over the
/// CANONICALIZED store root.
///
/// This is the one shared rule for the binder and the dialer: Windows
/// `fs::canonicalize` publishes the verbatim `\\?\` form, and hashing
/// different spellings of one directory (plain `C:\...` from the daemon's
/// config versus verbatim from a published discovery descriptor) would give
/// the two sides different pipe names. Failure is an explicit error — the
/// caller cannot resolve a store root that does not exist — never a second
/// hash over unnormalized bytes.
#[cfg(windows)]
pub fn canonical_pipe_name(store_root: &Path) -> std::io::Result<String> {
  let canonical = store_root.canonicalize()?;
  Ok(named_pipe_name(&canonical))
}

#[cfg(all(test, windows))]
mod windows_tests {
  use super::*;

  // ROOT CAUSE (Windows health audit F-PIPE-1):
  //
  // The daemon bound the hash of its plain-form store root while the CLI
  // dialed the hash of the discovery descriptor's verbatim-canonical form, so
  // the two sides of the documented default path derived different pipe names
  // and every `auv devices access` command failed on Windows.
  //
  // Before the fix, production callers hashed their input bytes unchanged
  // through `named_pipe_name`. The fix moves them to `canonical_pipe_name`,
  // so the plain form, the verbatim form, and the normalized output of one
  // directory all produce one name, and a different directory still produces
  // a different name.
  #[test]
  fn canonical_pipe_name_matches_across_store_root_spellings() {
    let raw = std::env::temp_dir().join(format!("auv-pipe-name-test-{}", std::process::id()));
    std::fs::create_dir_all(&raw).expect("create the private test store root");
    let canonical = raw.canonicalize().expect("canonicalize the test store root");

    // Windows `fs::canonicalize` publishes the verbatim `\\?\` form; the rule
    // depends on that behavior, so pin it.
    let as_os = canonical.as_os_str().to_string_lossy().into_owned();
    assert!(as_os.starts_with(r"\\?\"), "canonical form is verbatim: {as_os}");

    let from_raw = canonical_pipe_name(&raw).expect("plain spelling resolves");
    let from_canonical = canonical_pipe_name(&canonical).expect("verbatim spelling resolves");

    assert_eq!(from_raw, from_canonical, "one directory, one pipe name");

    // A different directory must not collide.
    let other = raw.join("other");
    std::fs::create_dir_all(&other).expect("create the sibling directory");
    let from_other = canonical_pipe_name(&other).expect("sibling spelling resolves");
    assert_ne!(from_raw, from_other);

    std::fs::remove_dir_all(&raw).ok();
  }

  // A store root that does not exist must be refused explicitly — no name is
  // derived from a directory the daemon could never have bound.
  #[test]
  fn canonical_pipe_name_rejects_a_missing_store_root() {
    let missing = std::env::temp_dir().join(format!("auv-pipe-name-missing-{}", std::process::id()));
    assert!(canonical_pipe_name(&missing).is_err(), "a missing store root must not produce a pipe name");
  }

  // The legacy entry keeps hashing its input bytes unchanged: that contract
  // is what lets a fixed daemon serve older CLIs, which dial the descriptor's
  // verbatim form as-is. Pin that it does NOT normalize, so the two entries
  // stay distinguishable.
  #[test]
  fn legacy_named_pipe_name_hashes_the_given_bytes_without_normalizing() {
    let raw = std::env::temp_dir().join(format!("auv-pipe-name-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&raw).expect("create the private test store root");
    let canonical = raw.canonicalize().expect("canonicalize the test store root");

    let legacy_raw = named_pipe_name(&raw);
    let legacy_canonical = named_pipe_name(&canonical);
    assert_ne!(legacy_raw, legacy_canonical, "the legacy entry hashes the given spelling");

    assert_eq!(legacy_canonical, canonical_pipe_name(&raw).expect("production entry"), "the production entry hashes the canonical form");

    std::fs::remove_dir_all(&raw).ok();
  }
}
