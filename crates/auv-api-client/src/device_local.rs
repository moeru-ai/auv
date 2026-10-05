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
    let name = named_pipe_name(store_root);
    let channel = tonic::transport::Endpoint::from_static("http://localhost")
      .connect_with_connector(tower::service_fn(move |_: http::Uri| {
        let name = name.clone();
        async move { windows_pipe::open_verified(&name).await.map(hyper_util::rt::TokioIo::new) }
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
