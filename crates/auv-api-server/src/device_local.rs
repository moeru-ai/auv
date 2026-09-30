//! Target-local Device enrollment transport. This service is intentionally
//! separate from the shared API router and has no TCP, REST, or paired route.

use std::sync::Arc;

use auv::devices::EnrollmentState;
use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::device_local_service_server::DeviceLocalService;
#[cfg(unix)]
use auv_api_proto::auv::api::daemon::v1::device_local_service_server::DeviceLocalServiceServer;
use tonic::{Request, Response, Status};
use zeroize::Zeroizing;

/// Kernel-authenticated local caller. The requested account name is never an
/// authority token; the enrollment backend resolves and authorizes it against
/// this principal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalOsPrincipal {
  /// Numeric Unix account identity from the accepted socket connection.
  UnixUid(u32),
  /// Windows account SID from an impersonated named-pipe client token.
  WindowsSid(String),
  /// Windows account SID with enabled built-in Administrators membership from
  /// the same impersonated token. Only the verified pipe constructs this.
  #[cfg(windows)]
  WindowsAdministratorSid(String),
}

/// Local credential kind understood by an installed unlock host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialKind {
  OsPassword,
  WindowsPin,
}

/// Non-secret local enrollment metadata. Every current backend stores the
/// credential in its protected OS store, so the wire storage kind is fixed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Enrollment {
  pub user: String,
  pub os_account_id: String,
  pub state: EnrollmentState,
}

/// Non-secret, allowlisted record read from the target-local audit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditEntry {
  pub event: String,
  pub attempt_id: String,
  pub caller: String,
  pub os_account_id: Option<String>,
  pub user: Option<String>,
  pub session_selector: Option<String>,
  pub result: Option<String>,
  pub at_unix_millis: u128,
}

/// Bounded audit page. The cursor is an opaque byte offset returned to the
/// same local reader; callers must not infer account visibility from it.
pub struct AuditPage {
  pub entries: Vec<AuditEntry>,
  pub next_cursor: Option<u64>,
}

/// Credential bytes are moved out of the generated protobuf request as soon
/// as it is decoded. This wrapper deliberately does not implement `Debug`.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
  pub fn as_bytes(&self) -> &[u8] {
    &self.0
  }
}

/// One target-local enrollment operation. The backend must resolve `user` to
/// a stable OS account ID before checking authority or writing any secret.
pub struct EnrollAccount {
  pub user: String,
  pub credential: SecretBytes,
  pub credential_kind: CredentialKind,
}

/// Safe errors exposed by the local enrollment boundary. Implementations
/// must never put native credential error text or secret bytes in these values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalControlError {
  InvalidAccount,
  InvalidCredential,
  PermissionDenied,
  NotFound,
  UnsupportedCredentialKind,
  HostUnavailable,
  Persistence,
}

/// Enrollment policy and vault port, implemented by the target daemon. The
/// service passes the authenticated OS principal separately from user input.
// NOTICE(device-local-gate): The daemon may serve local management while an
// enrollment remains PENDING. READY and remote unlock still require retrieval
// under the installed host identity while the session is locked.
#[tonic::async_trait]
pub trait DeviceLocalControl: Send + Sync + 'static {
  async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<Enrollment, LocalControlError>;
  async fn list_enrollments(&self, principal: &LocalOsPrincipal) -> Result<Vec<Enrollment>, LocalControlError>;
  async fn enroll(&self, principal: &LocalOsPrincipal, request: EnrollAccount) -> Result<Enrollment, LocalControlError>;
  async fn remove_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<(), LocalControlError>;
  async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError>;
  async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError>;
  async fn list_audit(&self, principal: &LocalOsPrincipal, cursor: u64, limit: usize) -> Result<AuditPage, LocalControlError>;
}

struct DeviceLocalGrpc {
  control: Arc<dyn DeviceLocalControl>,
}

#[tonic::async_trait]
impl DeviceLocalService for DeviceLocalGrpc {
  async fn get_enrollment(&self, request: Request<proto::GetEnrollmentRequest>) -> Result<Response<proto::GetEnrollmentResponse>, Status> {
    let principal = peer_principal(&request)?;
    let user = required_user(&request.get_ref().user)?;
    let enrollment = self.control.get_enrollment(&principal, user).await.map_err(status)?;
    Ok(Response::new(proto::GetEnrollmentResponse {
      enrollment: Some(wire_enrollment(enrollment)),
    }))
  }

  async fn list_enrollments(
    &self,
    request: Request<proto::ListEnrollmentsRequest>,
  ) -> Result<Response<proto::ListEnrollmentsResponse>, Status> {
    let principal = peer_principal(&request)?;
    let enrollments = self.control.list_enrollments(&principal).await.map_err(status)?;
    Ok(Response::new(proto::ListEnrollmentsResponse {
      enrollments: enrollments.into_iter().map(wire_enrollment).collect(),
    }))
  }

  async fn enroll(&self, request: Request<proto::EnrollRequest>) -> Result<Response<proto::EnrollResponse>, Status> {
    let principal = peer_principal(&request);
    let mut input = request.into_inner();
    let credential = SecretBytes(Zeroizing::new(std::mem::take(&mut input.credential)));
    let principal = principal?;
    let user = required_user(&input.user)?.to_owned();
    // A bounded, locally entered UTF-8 secret excludes accidental binary
    // payloads and makes the persistent vault input contract explicit.
    if credential.as_bytes().is_empty() || credential.as_bytes().len() > 1024 || std::str::from_utf8(credential.as_bytes()).is_err() {
      return Err(Status::invalid_argument("credential must be 1..=1024 UTF-8 bytes"));
    }

    let credential_kind = match proto::EnrollmentCredentialKind::try_from(input.credential_kind) {
      Ok(proto::EnrollmentCredentialKind::OsPassword) => CredentialKind::OsPassword,
      Ok(proto::EnrollmentCredentialKind::WindowsPin) => CredentialKind::WindowsPin,
      _ => return Err(Status::invalid_argument("unsupported credential_kind")),
    };
    match proto::EnrollmentStorageKind::try_from(input.storage_kind) {
      Ok(proto::EnrollmentStorageKind::Protected) => {}
      // TODO(device-entry-plaintext): The wire keeps the administrator-only
      // plaintext fallback from the credential decision, but no platform has
      // a restricted plaintext store. Add a backend storage choice only with
      // an owner-approved store and removal gate.
      Ok(proto::EnrollmentStorageKind::PlaintextFile) => {
        return Err(Status::failed_precondition("storage kind is unavailable on this host"));
      }
      _ => return Err(Status::invalid_argument("storage_kind must be explicit")),
    }
    let enrollment = self
      .control
      .enroll(
        &principal,
        EnrollAccount {
          user,
          credential,
          credential_kind,
        },
      )
      .await
      .map_err(status)?;
    Ok(Response::new(proto::EnrollResponse {
      enrollment: Some(wire_enrollment(enrollment)),
    }))
  }

  async fn remove_enrollment(
    &self,
    request: Request<proto::RemoveEnrollmentRequest>,
  ) -> Result<Response<proto::RemoveEnrollmentResponse>, Status> {
    let principal = peer_principal(&request)?;
    let user = required_user(&request.get_ref().user)?;
    self.control.remove_enrollment(&principal, user).await.map_err(status)?;
    Ok(Response::new(proto::RemoveEnrollmentResponse {}))
  }

  async fn get_policy(&self, request: Request<proto::GetPolicyRequest>) -> Result<Response<proto::GetPolicyResponse>, Status> {
    let principal = peer_principal(&request)?;
    let enabled = self.control.get_policy(&principal).await.map_err(status)?;
    Ok(Response::new(proto::GetPolicyResponse { enabled }))
  }

  async fn set_policy(&self, request: Request<proto::SetPolicyRequest>) -> Result<Response<proto::SetPolicyResponse>, Status> {
    let principal = peer_principal(&request)?;
    let enabled = self.control.set_policy(&principal, request.get_ref().enabled).await.map_err(status)?;
    Ok(Response::new(proto::SetPolicyResponse { enabled }))
  }

  async fn list_audit(&self, request: Request<proto::ListAuditRequest>) -> Result<Response<proto::ListAuditResponse>, Status> {
    let principal = peer_principal(&request)?;
    let input = request.get_ref();

    if !(1..=100).contains(&input.limit) {
      return Err(Status::invalid_argument("limit must be 1..=100"));
    }

    let page = self.control.list_audit(&principal, input.cursor, input.limit as usize).await.map_err(status)?;
    let entries = page
      .entries
      .into_iter()
      .map(|entry| {
        Ok(proto::AuditEntry {
          event: entry.event,
          attempt_id: entry.attempt_id,
          caller: entry.caller,
          os_account_id: entry.os_account_id,
          user: entry.user,
          session_selector: entry.session_selector,
          result: entry.result,
          at_unix_millis: u64::try_from(entry.at_unix_millis).map_err(|_| Status::internal("local audit timestamp is out of range"))?,
        })
      })
      .collect::<Result<Vec<_>, Status>>()?;
    Ok(Response::new(proto::ListAuditResponse {
      entries,
      next_cursor: page.next_cursor,
    }))
  }
}

fn required_user(user: &str) -> Result<&str, Status> {
  if user.is_empty() || user.trim() != user || user.contains('\0') {
    return Err(Status::invalid_argument("user must be a nonempty OS account name"));
  }

  Ok(user)
}

fn wire_enrollment(value: Enrollment) -> proto::Enrollment {
  proto::Enrollment {
    user: value.user,
    os_account_id: value.os_account_id,
    state: match value.state {
      EnrollmentState::Pending => proto::EnrollmentState::Pending as i32,
      EnrollmentState::Ready => proto::EnrollmentState::Ready as i32,
      EnrollmentState::Suspended => proto::EnrollmentState::Suspended as i32,
    },
    storage_kind: proto::EnrollmentStorageKind::Protected as i32,
  }
}

fn status(error: LocalControlError) -> Status {
  match error {
    LocalControlError::InvalidAccount => Status::invalid_argument("invalid OS account"),
    LocalControlError::InvalidCredential => Status::invalid_argument("invalid credential input"),
    LocalControlError::PermissionDenied => Status::permission_denied("local OS caller cannot manage this account"),
    LocalControlError::NotFound => Status::not_found("enrollment not found"),
    LocalControlError::UnsupportedCredentialKind => Status::failed_precondition("credential kind is unavailable on this host"),
    LocalControlError::HostUnavailable => Status::unavailable("installed unlock host cannot retrieve this credential"),
    LocalControlError::Persistence => Status::internal("local enrollment store failed"),
  }
}

#[cfg(unix)]
fn peer_principal<T>(request: &Request<T>) -> Result<LocalOsPrincipal, Status> {
  request
    .extensions()
    .get::<tonic::transport::server::UdsConnectInfo>()
    .and_then(|info| info.peer_cred.as_ref())
    .map(|credentials| LocalOsPrincipal::UnixUid(credentials.uid()))
    .ok_or_else(|| Status::permission_denied("verified Unix peer credentials are required"))
}

#[cfg(windows)]
mod windows_pipe;
#[cfg(windows)]
pub use windows_pipe::serve_named_pipe;

#[cfg(windows)]
fn peer_principal<T>(request: &Request<T>) -> Result<LocalOsPrincipal, Status> {
  request
    .extensions()
    .get::<windows_pipe::PeerIdentity>()
    .and_then(windows_pipe::PeerIdentity::principal)
    .ok_or_else(|| Status::permission_denied("verified local peer SID is required"))
}

#[cfg(not(any(unix, windows)))]
fn peer_principal<T>(_request: &Request<T>) -> Result<LocalOsPrincipal, Status> {
  Err(Status::permission_denied("verified local peer identity is required"))
}

/// Serves only DeviceLocalService over a dedicated Unix socket. Its parent
/// directory must already be owned by this process and not writable by other
/// users. Mode 0666 on the socket does not bypass parent traversal rules;
/// callers must be able to reach the path and every RPC checks the kernel
/// supplied peer credentials. The current daemon composition uses a 0700
/// parent, admitting its own UID and root only.
#[cfg(unix)]
pub async fn serve_unix(
  path: &std::path::Path,
  control: Arc<dyn DeviceLocalControl>,
  shutdown: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let parent = path.parent().ok_or_else(|| "DeviceLocalService socket requires a parent directory".to_string())?;
  let metadata = std::fs::symlink_metadata(parent).map_err(|error| format!("cannot inspect DeviceLocalService socket parent: {error}"))?;

  if !metadata.file_type().is_dir() || metadata.uid() != current_euid() || metadata.permissions().mode() & 0o022 != 0 {
    return Err("DeviceLocalService socket parent must be an owned directory without group/other write permission".into());
  }

  clear_stale_socket(path).await?;
  let listener = tokio::net::UnixListener::bind(path).map_err(|error| format!("cannot bind DeviceLocalService Unix socket: {error}"))?;
  let cleanup = SocketCleanup::new(path)?;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))
    .map_err(|error| format!("cannot set DeviceLocalService socket permissions: {error}"))?;

  let service = DeviceLocalServiceServer::new(DeviceLocalGrpc { control }).max_decoding_message_size(16 * 1024);
  let result = tonic::transport::Server::builder()
    .add_service(service)
    .serve_with_incoming_shutdown(tokio_stream::wrappers::UnixListenerStream::new(listener), shutdown.cancelled_owned())
    .await
    .map_err(|error| format!("DeviceLocalService Unix server failed: {error}"));
  drop(cleanup);
  result
}

#[cfg(unix)]
async fn clear_stale_socket(path: &std::path::Path) -> Result<(), String> {
  use std::os::unix::fs::{FileTypeExt, MetadataExt};
  let existing = match std::fs::symlink_metadata(path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
    Err(error) => return Err(format!("cannot inspect DeviceLocalService socket path: {error}")),
  };

  if !existing.file_type().is_socket() || existing.uid() != current_euid() {
    return Err("DeviceLocalService socket path is not an owned socket".into());
  }

  match tokio::time::timeout(std::time::Duration::from_millis(250), tokio::net::UnixStream::connect(path)).await {
    Ok(Ok(_)) => return Err("DeviceLocalService socket already has a live listener".into()),
    Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
    Ok(Err(_)) | Err(_) => return Err("DeviceLocalService socket liveness could not be established".into()),
  }

  // NOTICE(device-local-stale-socket): A process killed without Drop leaves a
  // socket inode. The owned parent is not group/other writable, and the
  // metadata store's process lock excludes another daemon using this root.
  // Recheck the inode before removing the refused listener's pathname.
  let current = std::fs::symlink_metadata(path).map_err(|_| "DeviceLocalService socket changed during stale recovery".to_string())?;

  if !current.file_type().is_socket()
    || current.uid() != existing.uid()
    || current.dev() != existing.dev()
    || current.ino() != existing.ino()
  {
    return Err("DeviceLocalService socket changed during stale recovery".into());
  }

  std::fs::remove_file(path).map_err(|error| format!("cannot remove stale DeviceLocalService socket: {error}"))
}

#[cfg(unix)]
fn current_euid() -> u32 {
  // SAFETY: geteuid has no pointers, allocation, or preconditions.
  unsafe { libc::geteuid() }
}

#[cfg(unix)]
struct SocketCleanup {
  path: std::path::PathBuf,
  device: u64,
  inode: u64,
}

#[cfg(unix)]
impl SocketCleanup {
  fn new(path: &std::path::Path) -> Result<Self, String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path).map_err(|error| format!("cannot inspect DeviceLocalService Unix socket: {error}"))?;
    Ok(Self {
      path: path.to_owned(),
      device: metadata.dev(),
      inode: metadata.ino(),
    })
  }
}

#[cfg(unix)]
impl Drop for SocketCleanup {
  fn drop(&mut self) {
    use std::os::unix::fs::MetadataExt;

    if let Ok(metadata) = std::fs::symlink_metadata(&self.path)
      && metadata.dev() == self.device
      && metadata.ino() == self.inode
    {
      let _ = std::fs::remove_file(&self.path);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn account_name_rejects_empty_and_ambiguous_input() {
    assert!(required_user("").is_err());
    assert!(required_user(" neko").is_err());
    assert!(required_user("neko\0root").is_err());
    assert_eq!(required_user("neko").unwrap(), "neko");
  }

  #[test]
  fn local_service_requires_connection_identity() {
    let request = Request::new(proto::ListEnrollmentsRequest {});

    assert_eq!(peer_principal(&request).unwrap_err().code(), tonic::Code::PermissionDenied);
  }

  #[test]
  fn credential_error_never_exposes_native_detail() {
    let error = status(LocalControlError::InvalidCredential);

    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    assert_eq!(error.message(), "invalid credential input");
  }

  #[cfg(unix)]
  struct TestControl {
    observed_uid: std::sync::Mutex<Option<u32>>,
    policy_enabled: std::sync::Mutex<bool>,
  }

  #[cfg(unix)]
  #[tonic::async_trait]
  impl DeviceLocalControl for TestControl {
    async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<Enrollment, LocalControlError> {
      let LocalOsPrincipal::UnixUid(uid) = principal else {
        return Err(LocalControlError::PermissionDenied);
      };

      *self.observed_uid.lock().unwrap() = Some(*uid);
      if user != "neko" {
        return Err(LocalControlError::PermissionDenied);
      }

      Ok(Enrollment {
        user: user.to_owned(),
        os_account_id: uid.to_string(),
        state: EnrollmentState::Ready,
      })
    }

    async fn list_enrollments(&self, _principal: &LocalOsPrincipal) -> Result<Vec<Enrollment>, LocalControlError> {
      Ok(Vec::new())
    }

    async fn enroll(&self, _principal: &LocalOsPrincipal, _request: EnrollAccount) -> Result<Enrollment, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn remove_enrollment(&self, _principal: &LocalOsPrincipal, _user: &str) -> Result<(), LocalControlError> {
      Ok(())
    }

    async fn get_policy(&self, _principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
      Ok(*self.policy_enabled.lock().unwrap())
    }

    async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError> {
      if principal != &LocalOsPrincipal::UnixUid(0) {
        return Err(LocalControlError::PermissionDenied);
      }

      *self.policy_enabled.lock().unwrap() = enabled;
      Ok(enabled)
    }

    async fn list_audit(&self, principal: &LocalOsPrincipal, cursor: u64, limit: usize) -> Result<AuditPage, LocalControlError> {
      let LocalOsPrincipal::UnixUid(uid) = principal else {
        return Err(LocalControlError::PermissionDenied);
      };

      assert_eq!(limit, 1);

      if cursor != 0 {
        return Ok(AuditPage {
          entries: Vec::new(),
          next_cursor: None,
        });
      }

      Ok(AuditPage {
        entries: vec![AuditEntry {
          event: "outcome".into(),
          attempt_id: "test-attempt".into(),
          caller: "paired-device:test".into(),
          os_account_id: Some(format!("uid:{uid}")),
          user: Some("neko".into()),
          session_selector: None,
          result: Some("UNLOCKED_EXISTING_SESSION".into()),
          at_unix_millis: 1,
        }],
        next_cursor: Some(42),
      })
    }
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn dedicated_unix_server_uses_real_peer_uid_and_denies_other_account() {
    use auv_api_proto::auv::api::daemon::v1::device_local_service_client::DeviceLocalServiceClient;
    use auv_api_proto::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
    use tonic::transport::Endpoint;

    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("device-local.sock");
    let control = Arc::new(TestControl {
      observed_uid: std::sync::Mutex::new(None),
      policy_enabled: std::sync::Mutex::new(true),
    });
    let shutdown = tokio_util::sync::CancellationToken::new();
    let server = tokio::spawn({
      let control = Arc::clone(&control);
      let shutdown = shutdown.clone();
      let socket = socket.clone();
      async move { serve_unix(&socket, control, shutdown).await }
    });

    for _ in 0..100 {
      if socket.exists() {
        break;
      }

      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(socket.exists(), "dedicated socket did not bind");

    let channel = Endpoint::try_from("http://[::]:50051")
      .unwrap()
      .connect_with_connector(tower::service_fn(move |_: tonic::codegen::http::Uri| {
        let socket = socket.clone();
        async move { tokio::net::UnixStream::connect(socket).await.map(hyper_util::rt::TokioIo::new) }
      }))
      .await
      .unwrap();

    let mut public_client = DeviceServiceClient::new(channel.clone());
    let missing_public_route = public_client.list_user_sessions(proto::ListUserSessionsRequest {}).await.unwrap_err();

    assert_eq!(missing_public_route.code(), tonic::Code::Unimplemented);

    let mut client = DeviceLocalServiceClient::new(channel);
    let own = client
      .get_enrollment(proto::GetEnrollmentRequest {
        user: "neko".into(),
      })
      .await
      .unwrap()
      .into_inner();

    assert_eq!(own.enrollment.unwrap().os_account_id, current_euid().to_string());
    assert_eq!(*control.observed_uid.lock().unwrap(), Some(current_euid()));

    let denied = client
      .get_enrollment(proto::GetEnrollmentRequest {
        user: "another".into(),
      })
      .await
      .unwrap_err();

    assert_eq!(denied.code(), tonic::Code::PermissionDenied);

    let invalid = client
      .enroll(proto::EnrollRequest {
        user: "neko".into(),
        credential: Vec::new(),
        credential_kind: proto::EnrollmentCredentialKind::OsPassword as i32,
        storage_kind: proto::EnrollmentStorageKind::Protected as i32,
      })
      .await
      .unwrap_err();

    assert_eq!(invalid.code(), tonic::Code::InvalidArgument);

    // The plaintext fallback stays on the wire but has no backend store, so
    // the protocol boundary rejects it before the local control port runs.
    let plaintext = client
      .enroll(proto::EnrollRequest {
        user: "neko".into(),
        credential: b"fixture-only".to_vec(),
        credential_kind: proto::EnrollmentCredentialKind::OsPassword as i32,
        storage_kind: proto::EnrollmentStorageKind::PlaintextFile as i32,
      })
      .await
      .unwrap_err();

    assert_eq!(plaintext.code(), tonic::Code::FailedPrecondition);

    let unavailable = client
      .enroll(proto::EnrollRequest {
        user: "neko".into(),
        credential: b"fixture-only".to_vec(),
        credential_kind: proto::EnrollmentCredentialKind::OsPassword as i32,
        storage_kind: proto::EnrollmentStorageKind::Protected as i32,
      })
      .await
      .unwrap_err();

    assert_eq!(unavailable.code(), tonic::Code::Unavailable);

    assert!(client.get_policy(proto::GetPolicyRequest {}).await.unwrap().into_inner().enabled);

    let changed = client.set_policy(proto::SetPolicyRequest { enabled: false }).await;

    if current_euid() == 0 {
      assert!(!changed.unwrap().into_inner().enabled);
      assert!(!client.get_policy(proto::GetPolicyRequest {}).await.unwrap().into_inner().enabled);
    } else {
      assert_eq!(changed.unwrap_err().code(), tonic::Code::PermissionDenied);
      assert!(client.get_policy(proto::GetPolicyRequest {}).await.unwrap().into_inner().enabled);
    }

    let invalid_page = client
      .list_audit(proto::ListAuditRequest {
        cursor: 0,
        limit: 101,
      })
      .await
      .unwrap_err();

    assert_eq!(invalid_page.code(), tonic::Code::InvalidArgument);

    let first_page = client
      .list_audit(proto::ListAuditRequest {
        cursor: 0,
        limit: 1,
      })
      .await
      .unwrap()
      .into_inner();

    assert_eq!(first_page.entries.len(), 1);
    assert_eq!(first_page.entries[0].os_account_id.as_deref(), Some(format!("uid:{}", current_euid()).as_str()));
    assert_eq!(first_page.next_cursor, Some(42));

    let last_page = client
      .list_audit(proto::ListAuditRequest {
        cursor: 42,
        limit: 1,
      })
      .await
      .unwrap()
      .into_inner();

    assert!(last_page.entries.is_empty());
    assert_eq!(last_page.next_cursor, None);

    shutdown.cancel();
    server.await.unwrap().unwrap();
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn restarts_after_refused_socket_but_preserves_live_listener() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("device-local.sock");
    let abandoned = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    drop(abandoned);

    let control = Arc::new(TestControl {
      observed_uid: std::sync::Mutex::new(None),
      policy_enabled: std::sync::Mutex::new(true),
    });
    let shutdown = tokio_util::sync::CancellationToken::new();
    let task = tokio::spawn({
      let socket = socket.clone();
      let shutdown = shutdown.clone();
      async move { serve_unix(&socket, control, shutdown).await }
    });
    let mut reachable = false;

    for _ in 0..100 {
      if tokio::net::UnixStream::connect(&socket).await.is_ok() {
        reachable = true;
        break;
      }

      tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(reachable, "server did not reclaim the refused socket");

    shutdown.cancel();
    task.await.unwrap().unwrap();

    assert!(!socket.exists());

    let live = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let control = Arc::new(TestControl {
      observed_uid: std::sync::Mutex::new(None),
      policy_enabled: std::sync::Mutex::new(true),
    });
    let error = serve_unix(&socket, control, tokio_util::sync::CancellationToken::new()).await.unwrap_err();

    assert!(error.contains("live listener"));
    assert!(socket.exists(), "live listener pathname must not be unlinked");

    drop(live);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn refuses_non_socket_at_local_path() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("device-local.sock");
    std::fs::write(&socket, b"owner data").unwrap();
    let control = Arc::new(TestControl {
      observed_uid: std::sync::Mutex::new(None),
      policy_enabled: std::sync::Mutex::new(true),
    });
    let error = serve_unix(&socket, control, tokio_util::sync::CancellationToken::new()).await.unwrap_err();

    assert!(error.contains("not an owned socket"));
    assert_eq!(std::fs::read(&socket).unwrap(), b"owner data");
  }
}
