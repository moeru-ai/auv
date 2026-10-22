//! Listener authentication and authenticated request context.

use std::sync::Arc;

use axum::http::{Extensions, header};
use tonic::Status;

use crate::control::{CallerId, Pairing, PairingError};

/// How a caller proves itself to a local (Unix socket or named pipe)
/// listener. Paired TCP listeners take [`RemoteAuthMethod`] instead.
#[derive(Clone)]
#[non_exhaustive]
pub enum LocalAuthMethod {
  /// Presents the token of the process that launched the daemon, so other
  /// processes of the same OS user cannot borrow its desktop permissions.
  LauncherToken(LauncherToken),
  /// Presents an active paired Device bearer. The launcher issues one per app
  /// and can revoke each; paired callers cannot issue further tokens.
  PairedDevice,
  /// Any process the OS reports as the daemon's user.
  OsUser,
}

/// How a caller proves itself to a remote (TCP) listener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemoteAuthMethod {
  /// Presents an active paired Device bearer.
  PairedDevice,
}

/// Rejects local method sets that admit nobody or leave no caller able to
/// issue pairing tokens, which only owner authority may do.
pub fn validate_local_auth_methods(methods: &[LocalAuthMethod]) -> Result<(), String> {
  if methods.is_empty() {
    return Err("local auth must allow at least one method".into());
  }
  let launcher_tokens = methods.iter().filter(|method| matches!(method, LocalAuthMethod::LauncherToken(_))).count();
  if launcher_tokens > 1 {
    return Err("local auth may allow only one launcher token".into());
  }
  if launcher_tokens == 0 && !methods.iter().any(|method| matches!(method, LocalAuthMethod::OsUser)) {
    return Err("local auth must allow launcher-token or os-user: only owner authority can issue pairing tokens".into());
  }
  Ok(())
}

/// Rejects an empty remote method set.
pub fn validate_remote_auth_methods(methods: &[RemoteAuthMethod]) -> Result<(), String> {
  if methods.is_empty() {
    return Err("remote auth must allow at least one method".into());
  }
  Ok(())
}

/// Who a local caller proved to be. [`Principal::authority`] decides what
/// that identity may do, separately from how it was proven.
enum Principal {
  Launcher,
  OsUser,
  PairedDevice(CallerId),
}

impl Principal {
  // NOTICE: Authority is fixed per principal for now. Configurable roles would
  // replace this mapping without changing how callers authenticate.
  fn authority(self) -> CallerId {
    match self {
      Self::Launcher | Self::OsUser => CallerId::local_owner(),
      Self::PairedDevice(caller) => caller,
    }
  }
}

/// Secret generated for one daemon launch and held by the launching process.
/// Callers present it as `authorization: Bearer <token>`.
#[derive(Clone)]
pub struct LauncherToken(Arc<zeroize::Zeroizing<String>>);

impl LauncherToken {
  /// Accepts exactly 64 lowercase hexadecimal characters (32 random bytes).
  pub fn parse(value: &str) -> Result<Self, String> {
    if value.len() != 64 || !value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
      return Err("launcher token must be 64 lowercase hexadecimal characters".into());
    }
    Ok(Self(Arc::new(zeroize::Zeroizing::new(value.to_owned()))))
  }

  /// Generates a token from 32 bytes of operating-system randomness.
  pub fn generate() -> Result<Self, String> {
    let mut bytes = zeroize::Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|error| format!("failed to generate a launcher token: {error}"))?;
    let mut text = String::with_capacity(64);
    for byte in bytes.iter() {
      text.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble"));
      text.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble"));
    }
    Ok(Self(Arc::new(zeroize::Zeroizing::new(text))))
  }

  /// The secret text to hand to a client that should control this daemon.
  /// Never put it in argv or the environment: other processes of the same
  /// user can read both.
  pub fn expose(&self) -> &str {
    &self.0
  }

  fn matches(&self, presented: &str) -> bool {
    use subtle::ConstantTimeEq as _;
    let expected = self.0.as_bytes();
    presented.len() == expected.len() && bool::from(presented.as_bytes().ct_eq(expected))
  }
}

/// Authentication policy for one listener.
#[derive(Clone)]
pub(crate) enum Authenticator {
  /// Trust the local caller after optional Unix peer validation and one of
  /// the allowed local methods.
  Local {
    #[cfg(unix)]
    allowed_unix_uid: Option<u32>,
    methods: Arc<[LocalAuthMethod]>,
    pairing: Option<Arc<dyn Pairing>>,
  },
  /// Require an active paired Device bearer.
  PairedBearer { pairing: Arc<dyn Pairing> },
}

/// Proof that this request used a paired bearer or an owner-verified local
/// transport. A loopback address alone cannot authorize Device entry.
#[derive(Clone)]
pub(crate) struct DeviceEntryAdmission(());

impl Authenticator {
  pub(crate) fn local(
    #[cfg(unix)] allowed_unix_uid: Option<u32>,
    methods: Arc<[LocalAuthMethod]>,
    pairing: Option<Arc<dyn Pairing>>,
  ) -> Self {
    Self::Local {
      #[cfg(unix)]
      allowed_unix_uid,
      methods,
      pairing,
    }
  }

  pub(crate) fn paired_bearer(pairing: Arc<dyn Pairing>) -> Self {
    Self::PairedBearer { pairing }
  }

  pub(crate) fn authenticate_http<T>(&self, request: &axum::http::Request<T>) -> Result<CallerId, Status> {
    match self {
      Self::Local {
        methods, pairing, ..
      } => {
        self.authenticate_extensions(request.extensions())?;
        let presented =
          request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer "));
        authenticate_local(methods, pairing.as_deref(), presented).map(Principal::authority)
      }
      Self::PairedBearer { pairing } => {
        authenticate_bearer(pairing.as_ref(), request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()))
      }
    }
  }

  fn authenticate_extensions(&self, extensions: &Extensions) -> Result<CallerId, Status> {
    match self {
      Self::Local {
        #[cfg(unix)]
        allowed_unix_uid,
        ..
      } => {
        #[cfg(unix)]
        if let Some(allowed_uid) = allowed_unix_uid {
          let peer_uid = extensions
            .get::<tonic::transport::server::UdsConnectInfo>()
            .and_then(|info| info.peer_cred.as_ref())
            .map(tokio::net::unix::UCred::uid);

          if peer_uid != Some(*allowed_uid) {
            return Err(Status::permission_denied("Unix peer credentials do not match the API server owner"));
          }
        }
        Ok(CallerId::local_owner())
      }
      Self::PairedBearer { .. } => Err(Status::unauthenticated("paired Device bearer required")),
    }
  }

  pub(crate) fn authenticate_websocket(&self, credential: &str) -> Result<CallerId, Status> {
    match self {
      // Local upgrades are authenticated by the HTTP middleware, which passes
      // its caller to the session; never grant authority from here.
      Self::Local { .. } => Err(Status::internal("local WebSocket session is missing its authenticated caller")),
      Self::PairedBearer { pairing } => pairing.authenticate_bearer(credential).map_err(map_pairing_auth_error),
    }
  }

  /// Whether this listener authenticates a WebSocket from its Open message.
  pub(crate) fn authenticates_websocket_open(&self) -> bool {
    matches!(self, Self::PairedBearer { .. })
  }

  pub(crate) fn pairing(&self) -> Option<Arc<dyn Pairing>> {
    match self {
      Self::Local { pairing, .. } => pairing.clone(),
      Self::PairedBearer { pairing } => Some(pairing.clone()),
    }
  }

  pub(crate) fn device_entry_admission(&self) -> Option<DeviceEntryAdmission> {
    match self {
      Self::PairedBearer { .. } => Some(DeviceEntryAdmission(())),
      Self::Local {
        #[cfg(unix)]
        allowed_unix_uid,
        ..
      } => {
        #[cfg(unix)]
        {
          allowed_unix_uid.map(|_| DeviceEntryAdmission(()))
        }

        #[cfg(windows)]
        {
          // TODO(device-entry-windows-local): Admit named-pipe Device entry
          // after the listener verifies the peer SID for each request.
          None
        }
      }
    }
  }
}

/// Proves a local caller with the first allowed method its request satisfies,
/// most specific first, so a valid bearer never falls back to `OsUser`.
fn authenticate_local(methods: &[LocalAuthMethod], pairing: Option<&dyn Pairing>, presented: Option<&str>) -> Result<Principal, Status> {
  let mut pairing_failure = None;
  if let Some(bearer) = presented {
    for method in methods {
      match method {
        LocalAuthMethod::LauncherToken(token) if token.matches(bearer) => return Ok(Principal::Launcher),
        LocalAuthMethod::PairedDevice => match pairing.map(|pairing| pairing.authenticate_bearer(bearer)) {
          Some(Ok(caller)) => return Ok(Principal::PairedDevice(caller)),
          Some(Err(error)) => pairing_failure = Some(error),
          None => {}
        },
        _ => {}
      }
    }
  }
  if methods.iter().any(|method| matches!(method, LocalAuthMethod::OsUser)) {
    return Ok(Principal::OsUser);
  }
  match pairing_failure {
    // A broken pairing store is an internal error, not a wrong credential.
    Some(error) if !matches!(error, PairingError::Unauthenticated) => {
      tracing::warn!(%error, "local paired bearer check failed");
      Err(map_pairing_auth_error(error))
    }
    _ => Err(Status::unauthenticated(local_rejection(methods))),
  }
}

fn local_rejection(methods: &[LocalAuthMethod]) -> String {
  let accepted = methods
    .iter()
    .filter_map(|method| match method {
      LocalAuthMethod::LauncherToken(_) => Some("the process that launched it (launcher token)"),
      LocalAuthMethod::PairedDevice => Some("apps it paired (paired Device bearer)"),
      _ => None,
    })
    .collect::<Vec<_>>();
  format!("this daemon only accepts {}", accepted.join(" and "))
}

fn authenticate_bearer(pairing: &dyn Pairing, authorization: Option<&str>) -> Result<CallerId, Status> {
  let credential = authorization
    .and_then(|value| value.strip_prefix("Bearer "))
    .filter(|value| !value.is_empty())
    .ok_or_else(|| Status::unauthenticated("paired Device bearer required"))?;

  // Log the outcome only: the bearer value is a credential.
  pairing.authenticate_bearer(credential).map_err(|error| {
    tracing::warn!(%error, "paired Device bearer rejected");
    map_pairing_auth_error(error)
  })
}

fn map_pairing_auth_error(error: PairingError) -> Status {
  match error {
    PairingError::Unauthenticated => Status::unauthenticated("Device bearer is not an active paired credential"),
    _ => Status::internal("paired-device authentication store failed"),
  }
}

/// Returns the authenticated caller inserted by the listener middleware.
pub(crate) fn caller<T>(request: &tonic::Request<T>) -> Result<&CallerId, Status> {
  request.extensions().get::<CallerId>().ok_or_else(|| Status::internal("authenticated request is missing CallerId"))
}

/// Device entry also requires the transport proof issued after request
/// authentication; a `CallerId` by itself does not prove local OS ownership.
pub(crate) fn device_entry_caller<T>(request: &tonic::Request<T>) -> Result<&CallerId, Status> {
  if request.extensions().get::<DeviceEntryAdmission>().is_none() {
    return Err(Status::unauthenticated("Device entry requires a paired bearer or owner-verified local transport"));
  }

  caller(request)
}

/// Returns the authenticated caller inserted by the listener middleware.
pub(crate) fn http_caller<T>(request: &axum::http::Request<T>) -> Result<&CallerId, Status> {
  request.extensions().get::<CallerId>().ok_or_else(|| Status::internal("authenticated request is missing CallerId"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::control::{Enrollment, PairingToken};

  const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
  const DEVICE_BEARER: &str = "paired-device-bearer";

  /// Accepts exactly one Device bearer.
  struct OneDevice;

  impl Pairing for OneDevice {
    fn authenticate_bearer(&self, credential: &str) -> Result<CallerId, PairingError> {
      if credential == DEVICE_BEARER {
        Ok(CallerId::paired_device("other-app"))
      } else {
        Err(PairingError::Unauthenticated)
      }
    }
    fn is_active_caller(&self, _: &CallerId) -> bool {
      true
    }
    fn issue_token(&self, _: Option<std::time::Duration>) -> Result<PairingToken, PairingError> {
      unreachable!()
    }
    fn enroll(&self, _: &str, _: String, _: String) -> Result<Enrollment, PairingError> {
      unreachable!()
    }
    fn revoke_device_credentials(&self, _: &str) -> Result<bool, PairingError> {
      unreachable!()
    }
    fn set_enabled(&self, _: &str, _: bool) -> Result<bool, PairingError> {
      unreachable!()
    }
    fn unpair(&self, _: &str) -> Result<bool, PairingError> {
      unreachable!()
    }
  }

  fn local(methods: Vec<LocalAuthMethod>) -> Authenticator {
    Authenticator::local(
      #[cfg(unix)]
      None,
      methods.into(),
      Some(Arc::new(OneDevice)),
    )
  }

  fn launcher_token() -> LocalAuthMethod {
    LocalAuthMethod::LauncherToken(LauncherToken::parse(KEY).unwrap())
  }

  fn request(authorization: Option<&str>) -> axum::http::Request<()> {
    let mut builder = axum::http::Request::builder().uri("/");
    if let Some(authorization) = authorization {
      builder = builder.header(header::AUTHORIZATION, authorization);
    }
    builder.body(()).unwrap()
  }

  fn caller(authenticator: &Authenticator, authorization: Option<&str>) -> Result<CallerId, tonic::Code> {
    authenticator.authenticate_http(&request(authorization)).map_err(|status| status.code())
  }

  // ROOT CAUSE:
  //
  // Local IPC admitted every process of the daemon's OS user as the owner. A
  // process that only learned the socket path from `ps` could drive the
  // daemon's Runner with the permissions granted to the app that started it.
  // With only launcher-token allowed, callers must present the token.
  #[test]
  fn launcher_token_rejects_local_callers_without_the_token() {
    let authenticator = local(vec![launcher_token()]);
    assert_eq!(caller(&authenticator, None), Err(tonic::Code::Unauthenticated));
    assert_eq!(caller(&authenticator, Some(&format!("Bearer {}", KEY.replace('0', "1")))), Err(tonic::Code::Unauthenticated));
    assert_eq!(caller(&authenticator, Some(KEY)), Err(tonic::Code::Unauthenticated));
    assert!(caller(&authenticator, Some(&format!("Bearer {KEY}"))).unwrap().is_local_owner());
  }

  #[test]
  fn paired_device_callers_get_device_authority_not_the_owners() {
    let authenticator = local(vec![launcher_token(), LocalAuthMethod::PairedDevice]);
    let paired = caller(&authenticator, Some(&format!("Bearer {DEVICE_BEARER}"))).unwrap();
    assert!(!paired.is_local_owner());
    assert_eq!(paired.paired_device_id(), Some("other-app"));
    assert!(caller(&authenticator, Some(&format!("Bearer {KEY}"))).unwrap().is_local_owner());
    assert_eq!(caller(&authenticator, Some("Bearer unknown")), Err(tonic::Code::Unauthenticated));
  }

  #[test]
  fn os_user_admits_without_a_token_but_a_paired_bearer_stays_a_device() {
    let authenticator = local(vec![LocalAuthMethod::PairedDevice, LocalAuthMethod::OsUser]);
    assert!(caller(&authenticator, None).unwrap().is_local_owner());
    assert!(!caller(&authenticator, Some(&format!("Bearer {DEVICE_BEARER}"))).unwrap().is_local_owner());
  }

  #[test]
  fn local_websocket_sessions_never_take_authority_from_the_open_message() {
    let authenticator = local(vec![LocalAuthMethod::OsUser]);
    assert_eq!(authenticator.authenticate_websocket(KEY).unwrap_err().code(), tonic::Code::Internal);
  }

  #[test]
  fn local_methods_must_admit_someone_who_can_issue_pairing_tokens() {
    assert!(validate_local_auth_methods(&[]).is_err());
    assert!(validate_local_auth_methods(&[LocalAuthMethod::PairedDevice]).is_err());
    assert!(validate_local_auth_methods(&[launcher_token(), launcher_token()]).is_err());
    assert!(validate_local_auth_methods(&[launcher_token(), LocalAuthMethod::PairedDevice]).is_ok());
    assert!(validate_local_auth_methods(&[LocalAuthMethod::OsUser, LocalAuthMethod::PairedDevice]).is_ok());
    assert!(validate_remote_auth_methods(&[]).is_err());
  }

  #[test]
  fn generated_launcher_tokens_parse_and_differ() {
    let first = LauncherToken::generate().unwrap();
    let second = LauncherToken::generate().unwrap();
    assert!(LauncherToken::parse(first.expose()).is_ok());
    assert_ne!(first.expose(), second.expose());
  }

  #[test]
  fn launcher_token_requires_32_random_bytes_in_lowercase_hex() {
    assert!(LauncherToken::parse(KEY).is_ok());
    assert!(LauncherToken::parse(&KEY[1..]).is_err());
    assert!(LauncherToken::parse(&KEY.to_uppercase()).is_err());
    assert!(LauncherToken::parse(&format!("{}g", &KEY[1..])).is_err());
  }
}
