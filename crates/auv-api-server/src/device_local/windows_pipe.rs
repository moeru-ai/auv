//! Dedicated Windows DeviceLocalService pipe and verified peer identity.
//!
//! The pipe ACL controls who may connect. Authorization uses the SID obtained
//! by impersonating the client after a successful pipe read, never that ACL or
//! a process ID alone. No DeviceService route is installed on this listener.

use std::ffi::c_void;
use std::io;
use std::mem::{align_of, offset_of, size_of};
use std::os::windows::io::AsRawHandle;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use auv_api_proto::auv::api::daemon::v1::device_local_service_server::DeviceLocalServiceServer;
use futures_util::stream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tonic::transport::server::Connected;
use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
  ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
  GetTokenInformation, IsWellKnownSid, PSECURITY_DESCRIPTOR, RevertToSelf, SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, TOKEN_GROUPS,
  TOKEN_QUERY, TOKEN_USER, TokenGroups, TokenUser, WinAnonymousSid, WinBuiltinAdministratorsSid,
};
use windows::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows::Win32::System::SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
use windows::core::{PCWSTR, PWSTR};

use super::{DeviceLocalControl, DeviceLocalGrpc, LocalOsPrincipal};

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClientIdentity {
  sid: String,
  administrator: bool,
}

#[derive(Clone, Default)]
pub(super) struct PeerIdentity(Arc<OnceLock<ClientIdentity>>);

impl PeerIdentity {
  pub(super) fn principal(&self) -> Option<LocalOsPrincipal> {
    self.0.get().map(|identity| {
      if identity.administrator {
        LocalOsPrincipal::WindowsAdministratorSid(identity.sid.clone())
      } else {
        LocalOsPrincipal::WindowsSid(identity.sid.clone())
      }
    })
  }

  fn accept_read(&self, identity: ClientIdentity) -> io::Result<()> {
    match self.0.get() {
      Some(existing) if existing == &identity => Ok(()),
      Some(_) => Err(identity_error()),
      None => {
        self.0.set(identity).map_err(|_| identity_error())?;
        Ok(())
      }
    }
  }
}

struct VerifiedPipe {
  pipe: NamedPipeServer,
  identity: PeerIdentity,
}

impl AsyncRead for VerifiedPipe {
  fn poll_read(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
    let filled_before = buffer.filled().len();

    match Pin::new(&mut self.pipe).poll_read(context, buffer) {
      Poll::Ready(Ok(())) if buffer.filled().len() > filled_before => {
        // NOTICE(named-pipe-peer-sid): Microsoft documents that this API uses
        // the security context of the last message read from this pipe. Check
        // every successful read before forwarding those bytes to HTTP/2.
        let handle = HANDLE(self.pipe.as_raw_handle());

        match client_identity(handle).and_then(|identity| self.identity.accept_read(identity)) {
          Ok(()) => Poll::Ready(Ok(())),
          Err(error) => {
            buffer.clear();
            Poll::Ready(Err(error))
          }
        }
      }
      result => result,
    }
  }
}

impl AsyncWrite for VerifiedPipe {
  fn poll_write(mut self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<io::Result<usize>> {
    Pin::new(&mut self.pipe).poll_write(context, buffer)
  }

  fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.pipe).poll_flush(context)
  }

  fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
    Pin::new(&mut self.pipe).poll_shutdown(context)
  }
}

impl Connected for VerifiedPipe {
  type ConnectInfo = PeerIdentity;

  fn connect_info(&self) -> Self::ConnectInfo {
    self.identity.clone()
  }
}

/// Serve only target-local enrollment RPCs on a separate Windows pipe.
///
/// This transport authenticates the connecting SID. The supplied backend must
/// still authorize that SID for each requested account or policy operation.
pub async fn serve_named_pipe(
  name: &str,
  control: Arc<dyn DeviceLocalControl>,
  shutdown: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
  let first = create_pipe(name, true).map_err(|_| "cannot bind DeviceLocalService named pipe".to_string())?;
  let service = DeviceLocalServiceServer::new(DeviceLocalGrpc { control }).max_decoding_message_size(16 * 1024);
  let name = name.to_owned();
  let incoming = stream::try_unfold((first, name), |(pipe, name)| async move {
    pipe.connect().await?;
    let next = create_pipe(&name, false)?;
    Ok::<_, io::Error>(Some((
      VerifiedPipe {
        pipe,
        identity: PeerIdentity::default(),
      },
      (next, name),
    )))
  });
  tonic::transport::Server::builder()
    .add_service(service)
    .serve_with_incoming_shutdown(incoming, shutdown.cancelled_owned())
    .await
    .map_err(|_| "DeviceLocalService named-pipe server failed".to_string())
}

fn create_pipe(name: &str, first: bool) -> io::Result<NamedPipeServer> {
  if !name.starts_with("auv-device-local-") || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')) {
    return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid DeviceLocalService pipe name"));
  }

  // The per-user daemon serves this pipe, so only its own account (OW) and
  // LocalSystem may open it, matching the Unix socket's 0700 directory. SID
  // impersonation and backend policy still authorize every request.
  let sddl = "D:P(A;;GA;;;SY)(A;;GA;;;OW)".to_string();
  let sddl = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
  let mut descriptor = PSECURITY_DESCRIPTOR::default();
  // SAFETY: The UTF-16 buffer is NUL-terminated and stays live for this call;
  // descriptor is a live output. Its allocation outlives pipe creation.
  unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }
    .map_err(|_| identity_error())?;

  let descriptor = SecurityDescriptor(descriptor);
  let mut attributes = SECURITY_ATTRIBUTES {
    nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0.0,
    bInheritHandle: BOOL(0),
  };
  let mut options = ServerOptions::new();
  options.first_pipe_instance(first).reject_remote_clients(true);
  let path = format!(r"\\.\pipe\{name}");
  // SAFETY: Tokio copies SECURITY_ATTRIBUTES during this synchronous call;
  // the descriptor guard outlives it. The pipe rejects remote clients.
  unsafe { options.create_with_security_attributes_raw(&path, (&raw mut attributes).cast()) }
}

fn client_identity(pipe: HANDLE) -> io::Result<ClientIdentity> {
  // SAFETY: `pipe` is the live server end whose read just completed. Microsoft
  // binds this thread to that read's client security context on success.
  unsafe { ImpersonateNamedPipeClient(pipe) }.map_err(|_| identity_error())?;
  let _impersonation = ImpersonationGuard;
  let mut raw_token = HANDLE::default();
  // SAFETY: The current thread is impersonating the last pipe reader. The
  // output handle is owned and closed before this function returns.
  unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw_token) }.map_err(|_| identity_error())?;
  let token = Token(raw_token);
  let mut bytes = 0u32;
  // SAFETY: A null output buffer asks Windows for the required TOKEN_USER size.
  let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut bytes) };

  if bytes < size_of::<TOKEN_USER>() as u32 || align_of::<TOKEN_USER>() > align_of::<usize>() {
    return Err(identity_error());
  }

  let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
  // SAFETY: The word buffer is aligned and large enough for TOKEN_USER and its
  // embedded SID; it stays live through SID conversion below.
  unsafe { GetTokenInformation(token.0, TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }.map_err(|_| identity_error())?;

  if (bytes as usize) < size_of::<TOKEN_USER>() {
    return Err(identity_error());
  }

  // SAFETY: Windows initialized an aligned TOKEN_USER in `data`.
  let token_user = unsafe { data.as_ptr().cast::<TOKEN_USER>().read() };

  if token_user.User.Sid.0.is_null() {
    return Err(identity_error());
  }

  // SAFETY: The SID remains valid in `data`; anonymous identity cannot own an
  // enrollment even if a client changed its token after opening the pipe.
  if unsafe { IsWellKnownSid(token_user.User.Sid, WinAnonymousSid) }.as_bool() {
    return Err(identity_error());
  }

  let mut raw_sid = PWSTR::null();
  // SAFETY: The SID remains live in `data`; Windows returns one LocalAlloc
  // UTF-16 string, which the guard frees after conversion.
  unsafe { ConvertSidToStringSidW(token_user.User.Sid, &mut raw_sid) }.map_err(|_| identity_error())?;

  if raw_sid.is_null() {
    return Err(identity_error());
  }

  let sid = LocalString(raw_sid);
  // SAFETY: ConvertSidToStringSidW returned a NUL-terminated UTF-16 string.
  let value = unsafe { sid.0.to_string() }.map_err(|_| identity_error())?;

  if !value.starts_with("S-1-") {
    return Err(identity_error());
  }

  let administrator = enabled_administrator(token.0)?;
  Ok(ClientIdentity {
    sid: value,
    administrator,
  })
}

fn enabled_administrator(token: HANDLE) -> io::Result<bool> {
  let mut bytes = 0u32;
  // SAFETY: A null output buffer queries the required TokenGroups size.
  let _ = unsafe { GetTokenInformation(token, TokenGroups, None, 0, &mut bytes) };
  let header = offset_of!(TOKEN_GROUPS, Groups);

  if (bytes as usize) < header || bytes > 64 * 1024 || align_of::<SID_AND_ATTRIBUTES>() > align_of::<usize>() {
    return Err(identity_error());
  }

  let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
  // SAFETY: This aligned buffer has the queried capacity and stays live while
  // Windows writes TokenGroups and group SIDs are inspected below.
  unsafe { GetTokenInformation(token, TokenGroups, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }.map_err(|_| identity_error())?;

  if (bytes as usize) < header {
    return Err(identity_error());
  }

  // SAFETY: The successful query initialized GroupCount at the buffer start.
  let count = unsafe { data.as_ptr().cast::<u32>().read() as usize };

  if count > (bytes as usize - header) / size_of::<SID_AND_ATTRIBUTES>() {
    return Err(identity_error());
  }

  // SAFETY: The checked count fits the initialized TokenGroups buffer.
  let groups = unsafe { data.as_ptr().cast::<u8>().add(header).cast::<SID_AND_ATTRIBUTES>() };

  for index in 0..count {
    // SAFETY: Each SID_AND_ATTRIBUTES lies inside the checked live buffer.
    let group = unsafe { groups.add(index).read() };

    if group.Sid.0.is_null() {
      return Err(identity_error());
    }

    // SAFETY: Windows returned this SID in the live TokenGroups allocation.
    if group.Attributes & SE_GROUP_ENABLED as u32 != 0
      && group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0
      && unsafe { IsWellKnownSid(group.Sid, WinBuiltinAdministratorsSid) }.as_bool()
    {
      return Ok(true);
    }
  }

  Ok(false)
}

struct ImpersonationGuard;

impl Drop for ImpersonationGuard {
  fn drop(&mut self) {
    // SAFETY: This guard is created immediately after impersonation succeeds,
    // on the same poll_read thread, and no `.await` can move it elsewhere.
    if unsafe { RevertToSelf() }.is_err() {
      // A service thread must never continue under a client's security token.
      std::process::abort();
    }
  }
}

struct Token(HANDLE);
impl Drop for Token {
  fn drop(&mut self) {
    // SAFETY: OpenThreadToken returned one owned handle.
    let _ = unsafe { CloseHandle(self.0) };
  }
}

struct LocalString(PWSTR);
impl Drop for LocalString {
  fn drop(&mut self) {
    // SAFETY: ConvertSidToStringSidW returned one LocalAlloc allocation.
    let _ = unsafe { LocalFree(HLOCAL(self.0.0.cast::<c_void>())) };
  }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
impl Drop for SecurityDescriptor {
  fn drop(&mut self) {
    // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW returned
    // one LocalAlloc allocation.
    let _ = unsafe { LocalFree(HLOCAL(self.0.0)) };
  }
}

fn identity_error() -> io::Error {
  io::Error::new(io::ErrorKind::PermissionDenied, "verified named-pipe client SID is required")
}

#[cfg(test)]
mod tests {
  use std::os::windows::ffi::OsStrExt;
  use std::os::windows::io::{FromRawHandle, OwnedHandle};
  use std::sync::Mutex;

  use auv_api_proto::auv::api::daemon::v1 as proto;
  use auv_api_proto::auv::api::daemon::v1::device_local_service_server::DeviceLocalService;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio::net::windows::named_pipe::ClientOptions;
  use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
  };

  use super::*;
  use crate::device_local::{AuditPage, EnrollAccount, Enrollment, LocalControlError, LocalOsPrincipal};

  #[test]
  fn peer_identity_rejects_group_or_sid_change_after_first_read() {
    let peer = PeerIdentity::default();
    let ordinary = ClientIdentity {
      sid: "S-1-5-21-1001".into(),
      administrator: false,
    };

    assert!(peer.accept_read(ordinary.clone()).is_ok());
    assert!(peer.accept_read(ordinary.clone()).is_ok());
    assert!(matches!(peer.principal(), Some(LocalOsPrincipal::WindowsSid(sid)) if sid == ordinary.sid));
    assert!(
      peer
        .accept_read(ClientIdentity {
          administrator: true,
          ..ordinary.clone()
        })
        .is_err()
    );
    assert!(
      peer
        .accept_read(ClientIdentity {
          sid: "S-1-5-21-1002".into(),
          ..ordinary
        })
        .is_err()
    );

    let administrator = PeerIdentity::default();
    administrator
      .accept_read(ClientIdentity {
        sid: "S-1-5-21-2000".into(),
        administrator: true,
      })
      .unwrap();

    assert!(matches!(administrator.principal(), Some(LocalOsPrincipal::WindowsAdministratorSid(sid)) if sid == "S-1-5-21-2000"));
  }

  #[test]
  fn native_token_groups_are_readable() {
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut raw_token = HANDLE::default();
    // SAFETY: This opens the current process token only for a read-only test.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) }.unwrap();
    let token = Token(raw_token);

    assert!(enabled_administrator(token.0).is_ok());
  }

  #[tokio::test]
  async fn pipe_read_captures_the_actual_client_token_claim() {
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = format!(r"\\.\pipe\auv-device-local-test-{}-{nonce}", std::process::id());
    let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&path).unwrap();
    let writer = tokio::spawn(async move {
      let mut client = ClientOptions::new().open(&path).unwrap();
      client.write_all(b"x").await.unwrap();
    });
    server.connect().await.unwrap();
    let mut verified = VerifiedPipe {
      pipe: server,
      identity: PeerIdentity::default(),
    };
    let mut byte = [0];
    verified.read_exact(&mut byte).await.unwrap();
    writer.await.unwrap();

    assert_eq!(byte, *b"x");

    let principal = verified.identity.principal().unwrap();
    let mut raw_token = HANDLE::default();
    // SAFETY: This opens the current process token only for a read-only
    // comparison with the same-process pipe client's impersonation token.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) }.unwrap();
    let token = Token(raw_token);
    let expected_admin = enabled_administrator(token.0).unwrap();

    assert_eq!(matches!(principal, LocalOsPrincipal::WindowsAdministratorSid(_)), expected_admin);
  }

  #[derive(Default)]
  struct PrincipalProbe(Mutex<Option<String>>);

  #[tonic::async_trait]
  impl DeviceLocalControl for PrincipalProbe {
    async fn get_enrollment(&self, _: &LocalOsPrincipal, _: &str) -> Result<Enrollment, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn list_enrollments(&self, _: &LocalOsPrincipal) -> Result<Vec<Enrollment>, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn enroll(&self, _: &LocalOsPrincipal, _: EnrollAccount) -> Result<Enrollment, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn remove_enrollment(&self, _: &LocalOsPrincipal, _: &str) -> Result<(), LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
      let sid = match principal {
        LocalOsPrincipal::WindowsSid(sid) | LocalOsPrincipal::WindowsAdministratorSid(sid) => sid,
        _ => return Err(LocalControlError::PermissionDenied),
      };
      *self.0.lock().unwrap() = Some(sid.clone());
      Ok(true)
    }

    async fn set_policy(&self, _: &LocalOsPrincipal, _: bool) -> Result<bool, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }

    async fn list_audit(&self, _: &LocalOsPrincipal, _: u64, _: usize) -> Result<AuditPage, LocalControlError> {
      Err(LocalControlError::HostUnavailable)
    }
  }

  #[tokio::test]
  async fn dedicated_pipe_rejects_missing_identity_before_control() {
    let probe = Arc::new(PrincipalProbe::default());
    let direct = DeviceLocalGrpc {
      control: probe.clone(),
    };
    let missing = direct.get_policy(tonic::Request::new(proto::GetPolicyRequest {})).await.unwrap_err();

    assert_eq!(missing.code(), tonic::Code::PermissionDenied);
    assert!(probe.0.lock().unwrap().is_none());
  }

  #[tokio::test]
  async fn owner_opens_the_actual_local_pipe_acl() {
    // ROOT CAUSE:
    //
    // A service-created pipe denied the console account before gRPC because
    // Windows requires FILE_READ_ATTRIBUTES when opening a named pipe, even
    // though the client requested only data, READ_CONTROL, and SYNCHRONIZE.
    // Before the fix, CreateFileW returned ERROR_ACCESS_DENIED with 0x00120003.
    // The per-user daemon now owns this pipe and grants its owner full access.
    let name = format!("auv-device-local-acl-test-{}", std::process::id());
    let _server = create_pipe(&name, true).expect("create the real Device-local pipe ACL");
    let path = format!(r"\\.\pipe\{name}");
    let wide = std::ffi::OsStr::new(&path).encode_wide().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: The pipe path is NUL-terminated and stays live through the call.
    // The returned handle is transferred to OwnedHandle on success.
    let raw = unsafe {
      CreateFileW(
        PCWSTR(wide.as_ptr()),
        0x0012_0003,
        FILE_SHARE_MODE(0),
        None,
        OPEN_EXISTING,
        FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
        HANDLE::default(),
      )
    }
    .expect("the daemon's own user should open the dedicated pipe");
    // SAFETY: CreateFileW returned one owned handle.
    let _client = unsafe { OwnedHandle::from_raw_handle(raw.0) };
  }
}
