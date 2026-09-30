//! Device service adapter.

use std::sync::Arc;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::device_service_server::DeviceService;
use tonic::{Request, Response, Status};

use crate::authentication;
use crate::control::Control;
use crate::protocol::domain;
use crate::protocol::grpc::status::map_control_error;

#[derive(Clone)]
pub(crate) struct DeviceServiceGrpc {
  daemon: Arc<dyn Control>,
}

impl DeviceServiceGrpc {
  pub(crate) fn new(daemon: Arc<dyn Control>) -> Self {
    Self { daemon }
  }
}

#[tonic::async_trait]
impl DeviceService for DeviceServiceGrpc {
  async fn list_devices(&self, _request: Request<proto::ListDevicesRequest>) -> Result<Response<proto::ListDevicesResponse>, Status> {
    let devices = self.daemon.list_devices().map_err(map_control_error)?.into_iter().map(domain::device).collect();
    Ok(Response::new(proto::ListDevicesResponse { devices }))
  }

  async fn get_device(&self, request: Request<proto::GetDeviceRequest>) -> Result<Response<proto::GetDeviceResponse>, Status> {
    let device_id = request
      .into_inner()
      .device
      .map(|device| device.device_id)
      .filter(|device_id| !device_id.is_empty())
      .ok_or_else(|| Status::invalid_argument("device is required"))?;
    let device = self
      .daemon
      .get_device(&device_id)
      .map_err(map_control_error)?
      .ok_or_else(|| Status::not_found(format!("unknown Device: {device_id}")))?;
    Ok(Response::new(proto::GetDeviceResponse {
      device: Some(domain::device(device)),
    }))
  }

  async fn list_user_sessions(
    &self,
    request: Request<proto::ListUserSessionsRequest>,
  ) -> Result<Response<proto::ListUserSessionsResponse>, Status> {
    let caller = authentication::device_entry_caller(&request)?;
    let result = match self.daemon.list_user_sessions(caller) {
      Ok(sessions) => proto::list_user_sessions_response::Result::List(proto::UserSessionList {
        sessions: sessions.into_iter().map(domain::user_session).collect(),
      }),
      Err(reason) => proto::list_user_sessions_response::Result::Error(proto::DeviceEntryError {
        reason: domain::device_entry_error_reason(reason) as i32,
      }),
    };

    Ok(Response::new(proto::ListUserSessionsResponse {
      result: Some(result),
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

    let result = match self.daemon.get_user_session(caller, session_selector) {
      Ok(session) => proto::get_user_session_response::Result::Session(domain::user_session(session)),
      Err(reason) => proto::get_user_session_response::Result::Error(proto::DeviceEntryError {
        reason: domain::device_entry_error_reason(reason) as i32,
      }),
    };

    Ok(Response::new(proto::GetUserSessionResponse {
      result: Some(result),
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
    let result = match self.daemon.ensure_user_session_unlocked(&caller, target).await {
      Ok(effect) => proto::ensure_user_session_unlocked_response::Result::Effect(domain::ensure_user_session_unlocked_effect(effect)),
      Err(reason) => proto::ensure_user_session_unlocked_response::Result::Error(proto::DeviceEntryError {
        reason: domain::device_entry_error_reason(reason) as i32,
      }),
    };

    Ok(Response::new(proto::EnsureUserSessionUnlockedResponse {
      result: Some(result),
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
    let result = match self.daemon.ensure_user_session_locked(&caller, target).await {
      Ok(effect) => proto::ensure_user_session_locked_response::Result::Effect(domain::ensure_user_session_locked_effect(effect)),
      Err(reason) => proto::ensure_user_session_locked_response::Result::Error(proto::DeviceEntryError {
        reason: domain::device_entry_error_reason(reason) as i32,
      }),
    };

    Ok(Response::new(proto::EnsureUserSessionLockedResponse {
      result: Some(result),
    }))
  }
}
