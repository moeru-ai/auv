//! User session service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::user_session_service_server::UserSessionService;
use tonic::{Request, Response, Status};

use crate::authentication;
use crate::control::Control;
use crate::protocol::domain;

#[derive(Clone)]
pub(crate) struct UserSessionServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl UserSessionServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl UserSessionService for UserSessionServiceGrpc {
  async fn list_user_sessions(
    &self,
    request: Request<proto::ListUserSessionsRequest>,
  ) -> Result<Response<proto::ListUserSessionsResponse>, Status> {
    let caller = authentication::device_entry_caller(&request)?;
    let sessions = self.daemon.list_user_sessions(caller).map_err(entry_status)?;
    Ok(Response::new(proto::ListUserSessionsResponse {
      list: Some(proto::UserSessionList {
        sessions: sessions.into_iter().map(domain::user_session).collect(),
      }),
    }))
  }

  async fn get_user_session(
    &self,
    request: Request<proto::GetUserSessionRequest>,
  ) -> Result<Response<proto::GetUserSessionResponse>, Status> {
    let caller = authentication::device_entry_caller(&request)?;
    let session_selector = &request.get_ref().session_selector;

    if session_selector.trim().is_empty() {
      return Err(Status::invalid_argument("session_selector is required"));
    }

    let session = self.daemon.get_user_session(caller, session_selector).map_err(entry_status)?;
    Ok(Response::new(proto::GetUserSessionResponse {
      session: Some(domain::user_session(session)),
    }))
  }

  async fn ensure_user_session_unlocked(
    &self,
    request: Request<proto::EnsureUserSessionUnlockedRequest>,
  ) -> Result<Response<proto::EnsureUserSessionUnlockedResponse>, Status> {
    let caller = authentication::device_entry_caller(&request)?.clone();
    let target = match request.into_inner().target {
      Some(proto::ensure_user_session_unlocked_request::Target::User(user)) if !user.trim().is_empty() => {
        auv::devices::UserSessionTarget::User(user)
      }
      Some(proto::ensure_user_session_unlocked_request::Target::SessionSelector(selector)) if !selector.trim().is_empty() => {
        auv::devices::UserSessionTarget::SessionSelector(selector)
      }
      _ => return Err(Status::invalid_argument("exactly one non-empty user or session_selector is required")),
    };
    let effect = self.daemon.ensure_user_session_unlocked(&caller, target).await.map_err(entry_status)?;
    Ok(Response::new(proto::EnsureUserSessionUnlockedResponse {
      effect: Some(domain::ensure_user_session_unlocked_effect(effect)),
    }))
  }

  async fn ensure_user_session_locked(
    &self,
    request: Request<proto::EnsureUserSessionLockedRequest>,
  ) -> Result<Response<proto::EnsureUserSessionLockedResponse>, Status> {
    let caller = authentication::device_entry_caller(&request)?.clone();
    let target = match request.into_inner().target {
      Some(proto::ensure_user_session_locked_request::Target::User(user)) if !user.trim().is_empty() => {
        auv::devices::UserSessionTarget::User(user)
      }
      Some(proto::ensure_user_session_locked_request::Target::SessionSelector(selector)) if !selector.trim().is_empty() => {
        auv::devices::UserSessionTarget::SessionSelector(selector)
      }
      _ => return Err(Status::invalid_argument("exactly one non-empty user or session_selector is required")),
    };
    let effect = self.daemon.ensure_user_session_locked(&caller, target).await.map_err(entry_status)?;
    Ok(Response::new(proto::EnsureUserSessionLockedResponse {
      effect: Some(domain::ensure_user_session_locked_effect(effect)),
    }))
  }
}

/// Session failures leave as a gRPC status that carries the typed reason, so
/// interceptors, tracing and retry see a failure rather than an OK response.
fn entry_status(reason: auv::devices::DeviceEntryErrorReason) -> Status {
  auv_api_proto::device_entry::status(domain::device_entry_error_reason(reason))
}
