//! Conversion between transport protobufs and the shared AUV domain model.

use std::str::FromStr as _;

use auv_api_proto::auv::api::daemon::v1 as proto;

use crate::control::ControlError;

pub(crate) fn create_run(value: proto::CreateRunRequest) -> Result<auv::runs::CreateRun, ControlError> {
  Ok(auv::runs::CreateRun {
    devices: value
      .devices
      .into_iter()
      .map(|device| auv::resource::DeviceId::from_str(&device.device_id).map_err(|_| ControlError::InvalidArgument("invalid Device ID")))
      .collect::<Result<_, _>>()?,
    labels: value.labels,
  })
}

pub(crate) fn create_runner(value: proto::CreateRunnerRequest) -> Result<auv::runners::CreateRunner, ControlError> {
  let lifecycle = match proto::RunnerLifecycle::try_from(value.lifecycle).unwrap_or_default() {
    proto::RunnerLifecycle::Ephemeral => auv::runners::RunnerLifecycle::Ephemeral,
    proto::RunnerLifecycle::UnlessIdle => auv::runners::RunnerLifecycle::UnlessIdle,
    proto::RunnerLifecycle::UnlessShutdown => auv::runners::RunnerLifecycle::UnlessShutdown,
    proto::RunnerLifecycle::Unspecified => return Err(ControlError::InvalidArgument("Runner lifecycle is required")),
  };
  Ok(auv::runners::CreateRunner {
    device: value
      .device
      .map(|device| auv::resource::DeviceId::from_str(&device.device_id).map_err(|_| ControlError::InvalidArgument("invalid Device ID")))
      .transpose()?,
    class: auv::resource::RunnerClassId::from_str(
      &value.runner_class.ok_or(ControlError::InvalidArgument("runner_class is required"))?.runner_class,
    )
    .map_err(|_| ControlError::InvalidArgument("invalid RunnerClass ID"))?,
    labels: value.labels,
    lifecycle,
    idle_timeout: value.idle_timeout.map(duration_from_proto).transpose()?,
  })
}

pub(crate) fn run_outcome(value: proto::RunOutcome) -> Result<auv::runs::RunOutcome, ControlError> {
  match value {
    proto::RunOutcome::Succeeded => Ok(auv::runs::RunOutcome::Succeeded),
    proto::RunOutcome::Failed => Ok(auv::runs::RunOutcome::Failed),
    proto::RunOutcome::Canceled => Ok(auv::runs::RunOutcome::Canceled),
    proto::RunOutcome::Unspecified => Err(ControlError::InvalidArgument("Run outcome is required")),
  }
}

pub(crate) fn duration_from_proto(value: prost_types::Duration) -> Result<std::time::Duration, ControlError> {
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(ControlError::InvalidArgument("duration is outside its valid range"));
  }
  Ok(std::time::Duration::new(value.seconds as u64, value.nanos as u32))
}

fn timestamp(value: auv::time::Timestamp) -> prost_types::Timestamp {
  prost_types::Timestamp {
    seconds: value.seconds,
    nanos: value.nanos,
  }
}

pub(crate) fn device(value: auv::devices::Device) -> proto::Device {
  proto::Device {
    r#ref: Some(proto::DeviceRef {
      device_id: value.id.to_string(),
    }),
    name: value.name,
    platform: match value.platform {
      auv::devices::DevicePlatform::Unspecified => proto::DevicePlatform::Unspecified,
      auv::devices::DevicePlatform::Linux => proto::DevicePlatform::Linux,
      auv::devices::DevicePlatform::Macos => proto::DevicePlatform::Macos,
      auv::devices::DevicePlatform::Windows => proto::DevicePlatform::Windows,
    } as i32,
    local: value.local,
    labels: value.labels,
  }
}

pub(crate) fn user_session(value: auv::devices::UserSession) -> proto::UserSession {
  proto::UserSession {
    session_selector: value.selector,
    user: value.user,
    lock_state: match value.lock_state {
      auv::devices::UserSessionLockState::Locked => proto::UserSessionLockState::Locked,
      auv::devices::UserSessionLockState::Usable => proto::UserSessionLockState::Usable,
      auv::devices::UserSessionLockState::Unknown => proto::UserSessionLockState::Unknown,
    } as i32,
    connection_kind: match value.connection_kind {
      auv::devices::UserSessionConnectionKind::Unspecified => proto::UserSessionConnectionKind::Unspecified,
      auv::devices::UserSessionConnectionKind::Physical => proto::UserSessionConnectionKind::Physical,
      auv::devices::UserSessionConnectionKind::Remote => proto::UserSessionConnectionKind::Remote,
    } as i32,
    seat: value.seat.unwrap_or_default(),
  }
}

pub(crate) fn ensure_user_session_unlocked_effect(
  value: auv::devices::EnsureUserSessionUnlockedEffect,
) -> proto::EnsureUserSessionUnlockedEffect {
  proto::EnsureUserSessionUnlockedEffect {
    kind: match value.kind {
      auv::devices::DeviceEntryEffectKind::AlreadyUsable => proto::DeviceEntryEffectKind::AlreadyUsable,
      auv::devices::DeviceEntryEffectKind::UnlockedExistingSession => proto::DeviceEntryEffectKind::UnlockedExistingSession,
    } as i32,
    user: value.user,
    session_selector: value.session_selector.unwrap_or_default(),
  }
}

pub(crate) fn ensure_user_session_locked_effect(value: auv::devices::EnsureUserSessionLockedEffect) -> proto::EnsureUserSessionLockedEffect {
  proto::EnsureUserSessionLockedEffect {
    kind: match value.kind {
      auv::devices::DeviceLockEffectKind::AlreadyLocked => proto::DeviceLockEffectKind::AlreadyLocked,
      auv::devices::DeviceLockEffectKind::LockedExistingSession => proto::DeviceLockEffectKind::LockedExistingSession,
    } as i32,
    user: value.user,
    session_selector: value.session_selector,
  }
}

pub(crate) fn device_entry_error_reason(value: auv::devices::DeviceEntryErrorReason) -> proto::DeviceEntryErrorReason {
  match value {
    auv::devices::DeviceEntryErrorReason::Unauthorized => proto::DeviceEntryErrorReason::Unauthorized,
    auv::devices::DeviceEntryErrorReason::Disabled => proto::DeviceEntryErrorReason::Disabled,
    auv::devices::DeviceEntryErrorReason::Unenrolled => proto::DeviceEntryErrorReason::Unenrolled,
    auv::devices::DeviceEntryErrorReason::Suspended => proto::DeviceEntryErrorReason::Suspended,
    auv::devices::DeviceEntryErrorReason::AmbiguousUser => proto::DeviceEntryErrorReason::AmbiguousUser,
    auv::devices::DeviceEntryErrorReason::StaleSession => proto::DeviceEntryErrorReason::StaleSession,
    auv::devices::DeviceEntryErrorReason::OccupiedDesktop => proto::DeviceEntryErrorReason::OccupiedDesktop,
    auv::devices::DeviceEntryErrorReason::UnsupportedOsState => proto::DeviceEntryErrorReason::UnsupportedOsState,
    auv::devices::DeviceEntryErrorReason::ServiceUnavailable => proto::DeviceEntryErrorReason::ServiceUnavailable,
    auv::devices::DeviceEntryErrorReason::CredentialRejected => proto::DeviceEntryErrorReason::CredentialRejected,
    auv::devices::DeviceEntryErrorReason::OutcomeUnverified => proto::DeviceEntryErrorReason::OutcomeUnverified,
    auv::devices::DeviceEntryErrorReason::AuditUnavailable => proto::DeviceEntryErrorReason::AuditUnavailable,
  }
}

pub(crate) fn run(value: auv::runs::Run) -> proto::Run {
  proto::Run {
    r#ref: Some(proto::RunRef {
      run_id: value.id.to_string(),
    }),
    phase: match value.phase {
      auv::runs::RunPhase::Unspecified => proto::RunPhase::Unspecified,
      auv::runs::RunPhase::Pending => proto::RunPhase::Pending,
      auv::runs::RunPhase::Running => proto::RunPhase::Running,
      auv::runs::RunPhase::Succeeded => proto::RunPhase::Succeeded,
      auv::runs::RunPhase::Failed => proto::RunPhase::Failed,
      auv::runs::RunPhase::Canceled => proto::RunPhase::Canceled,
    } as i32,
    devices: value
      .devices
      .into_iter()
      .map(|id| proto::DeviceRef {
        device_id: id.to_string(),
      })
      .collect(),
    labels: value.labels,
    created_at: value.created_at.map(timestamp),
    completed_at: value.completed_at.map(timestamp),
  }
}

fn lifecycle(value: auv::runners::RunnerLifecycle) -> proto::RunnerLifecycle {
  match value {
    auv::runners::RunnerLifecycle::Ephemeral => proto::RunnerLifecycle::Ephemeral,
    auv::runners::RunnerLifecycle::UnlessIdle => proto::RunnerLifecycle::UnlessIdle,
    auv::runners::RunnerLifecycle::UnlessShutdown => proto::RunnerLifecycle::UnlessShutdown,
  }
}

pub(crate) fn runner(value: auv::runners::Runner) -> proto::Runner {
  proto::Runner {
    r#ref: Some(proto::RunnerRef {
      runner_id: value.id.to_string(),
    }),
    device: Some(proto::DeviceRef {
      device_id: value.device.to_string(),
    }),
    runner_class: Some(proto::RunnerClassRef {
      runner_class: value.class.to_string(),
    }),
    labels: value.labels,
    lifecycle: lifecycle(value.lifecycle) as i32,
    idle_timeout: value.idle_timeout.map(|value| prost_types::Duration {
      seconds: value.as_secs() as i64,
      nanos: value.subsec_nanos() as i32,
    }),
    phase: match value.phase {
      auv::runners::RunnerPhase::Unspecified => proto::RunnerPhase::Unspecified,
      auv::runners::RunnerPhase::Starting => proto::RunnerPhase::Starting,
      auv::runners::RunnerPhase::Ready => proto::RunnerPhase::Ready,
      auv::runners::RunnerPhase::Draining => proto::RunnerPhase::Draining,
      auv::runners::RunnerPhase::Stopped => proto::RunnerPhase::Stopped,
      auv::runners::RunnerPhase::Failed => proto::RunnerPhase::Failed,
    } as i32,
    created_at: value.created_at.map(timestamp),
    process_id: value.process_id.unwrap_or_default(),
    active_operations: value.active_operations,
    idle_deadline: value.idle_deadline.map(timestamp),
  }
}

pub(crate) fn runner_class(value: auv::runners::RunnerClass) -> proto::RunnerClass {
  proto::RunnerClass {
    r#ref: Some(proto::RunnerClassRef {
      runner_class: value.id.to_string(),
    }),
    device: value.device.map(|id| proto::DeviceRef {
      device_id: id.to_string(),
    }),
    display_name: value.display_name,
    supported_lifecycles: value.supported_lifecycles.into_iter().map(|value| lifecycle(value) as i32).collect(),
    available: value.available,
  }
}
