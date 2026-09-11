//! Listener authentication and authenticated request context.

use std::sync::Arc;

use axum::http::{Extensions, header};
use tonic::Status;

use crate::control::{CallerId, Pairing, PairingError};

/// Authentication policy for one listener.
#[derive(Clone)]
pub(crate) enum Authenticator {
  /// Trust the local caller after optional Unix peer validation.
  Local {
    #[cfg(unix)]
    allowed_unix_uid: Option<u32>,
    pairing: Option<Arc<dyn Pairing>>,
  },
  /// Require an active paired Device bearer.
  PairedBearer { pairing: Arc<dyn Pairing> },
}

impl Authenticator {
  pub(crate) fn local(#[cfg(unix)] allowed_unix_uid: Option<u32>, pairing: Option<Arc<dyn Pairing>>) -> Self {
    Self::Local {
      #[cfg(unix)]
      allowed_unix_uid,
      pairing,
    }
  }

  pub(crate) fn paired_bearer(pairing: Arc<dyn Pairing>) -> Self {
    Self::PairedBearer { pairing }
  }

  pub(crate) fn authenticate_http<T>(&self, request: &axum::http::Request<T>) -> Result<CallerId, Status> {
    match self {
      Self::Local { .. } => self.authenticate_extensions(request.extensions()),
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
        pairing: _,
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
}

fn authenticate_bearer(pairing: &dyn Pairing, authorization: Option<&str>) -> Result<CallerId, Status> {
  let credential = authorization
    .and_then(|value| value.strip_prefix("Bearer "))
    .filter(|value| !value.is_empty())
    .ok_or_else(|| Status::unauthenticated("paired Device bearer required"))?;

  pairing.authenticate_bearer(credential).map_err(map_pairing_auth_error)
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

/// Returns the authenticated caller inserted by the listener middleware.
pub(crate) fn http_caller<T>(request: &axum::http::Request<T>) -> Result<&CallerId, Status> {
  request.extensions().get::<CallerId>().ok_or_else(|| Status::internal("authenticated request is missing CallerId"))
}
