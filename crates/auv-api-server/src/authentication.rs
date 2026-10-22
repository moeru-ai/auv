//! Listener authentication and authenticated request context.

use std::sync::Arc;

use axum::http::{Extensions, header};
use tonic::Status;

use crate::control::{CallerId, Pairing, PairingError};

/// Which local processes may call the daemon over its Unix socket or named
/// pipe. Paired TCP listeners always require a Device bearer instead.
#[derive(Clone)]
#[non_exhaustive]
pub enum LocalClientPolicy {
  /// Only callers that present the token of the process that launched the
  /// daemon, so other processes of the same OS user cannot borrow its desktop
  /// and OS permissions.
  LauncherOnly(LauncherToken),
  /// The launcher, plus callers presenting an active paired Device bearer.
  /// The launcher issues pairing tokens to the apps it admits and can revoke
  /// each one; paired callers cannot issue further tokens.
  LauncherAndPaired(LauncherToken),
  /// Any process running as the daemon's OS user.
  AnyProcessOfOsUser,
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
  /// Trust the local caller after optional Unix peer validation and the
  /// configured local client policy.
  Local {
    #[cfg(unix)]
    allowed_unix_uid: Option<u32>,
    policy: LocalClientPolicy,
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
  pub(crate) fn local(#[cfg(unix)] allowed_unix_uid: Option<u32>, policy: LocalClientPolicy, pairing: Option<Arc<dyn Pairing>>) -> Self {
    Self::Local {
      #[cfg(unix)]
      allowed_unix_uid,
      policy,
      pairing,
    }
  }

  pub(crate) fn paired_bearer(pairing: Arc<dyn Pairing>) -> Self {
    Self::PairedBearer { pairing }
  }

  pub(crate) fn authenticate_http<T>(&self, request: &axum::http::Request<T>) -> Result<CallerId, Status> {
    match self {
      Self::Local {
        policy, pairing, ..
      } => {
        let caller = self.authenticate_extensions(request.extensions())?;
        let presented =
          request.headers().get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer "));
        match policy {
          LocalClientPolicy::AnyProcessOfOsUser => Ok(caller),
          LocalClientPolicy::LauncherOnly(token) if presented.is_some_and(|presented| token.matches(presented)) => Ok(caller),
          LocalClientPolicy::LauncherOnly(_) => Err(Status::unauthenticated(LAUNCHER_ONLY_REJECTION)),
          LocalClientPolicy::LauncherAndPaired(token) if presented.is_some_and(|presented| token.matches(presented)) => Ok(caller),
          LocalClientPolicy::LauncherAndPaired(_) => match (pairing, presented) {
            // A paired app gets the authority of a paired Device, not the owner's.
            (Some(pairing), Some(bearer)) => pairing.authenticate_bearer(bearer).map_err(|error| {
              tracing::warn!(%error, "local paired bearer rejected");
              map_pairing_auth_error(error)
            }),
            _ => Err(Status::unauthenticated(LAUNCHER_OR_PAIRED_REJECTION)),
          },
        }
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
      Self::Local { .. } => Ok(CallerId::local_owner()),
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

const LAUNCHER_ONLY_REJECTION: &str = "this daemon only accepts the process that launched it; present its launcher token";
const LAUNCHER_OR_PAIRED_REJECTION: &str =
  "this daemon only accepts the process that launched it and apps it paired; present the launcher token or a paired Device bearer";

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

  const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

  fn local(policy: LocalClientPolicy) -> Authenticator {
    Authenticator::local(
      #[cfg(unix)]
      None,
      policy,
      None,
    )
  }

  fn request(authorization: Option<&str>) -> axum::http::Request<()> {
    let mut builder = axum::http::Request::builder().uri("/");
    if let Some(authorization) = authorization {
      builder = builder.header(header::AUTHORIZATION, authorization);
    }
    builder.body(()).unwrap()
  }

  // ROOT CAUSE:
  //
  // Local IPC admitted every process of the daemon's OS user as the owner. A
  // process that only learned the socket path from `ps` could drive the
  // daemon's Runner with the permissions granted to the app that started it.
  // Under LauncherOnly, local callers must also present the launcher token.
  #[test]
  fn launcher_only_rejects_local_callers_without_the_token() {
    let authenticator = local(LocalClientPolicy::LauncherOnly(LauncherToken::parse(KEY).unwrap()));
    let missing = authenticator.authenticate_http(&request(None)).err().unwrap();
    assert_eq!(missing.code(), tonic::Code::Unauthenticated);
    let wrong = format!("Bearer {}", KEY.replace('0', "1"));
    assert_eq!(authenticator.authenticate_http(&request(Some(&wrong))).err().unwrap().code(), tonic::Code::Unauthenticated);
    assert_eq!(authenticator.authenticate_http(&request(Some(KEY))).err().unwrap().code(), tonic::Code::Unauthenticated);
    assert!(authenticator.authenticate_http(&request(Some(&format!("Bearer {KEY}")))).is_ok());
  }

  #[test]
  fn any_process_of_os_user_admits_local_callers_without_a_token() {
    assert!(local(LocalClientPolicy::AnyProcessOfOsUser).authenticate_http(&request(None)).is_ok());
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
