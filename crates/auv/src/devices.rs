//! Device inventory, selector resolution, and configured-profile probe.

use std::collections::HashMap;
use std::str::FromStr;

use auv_api_proto::auv::api::daemon::v1 as proto;
use futures_util::future::join_all;

use crate::client::Client;
use crate::error::{ClientError, ClientErrorKind};
use crate::profile::{self, ConfiguredDevice, ProfileStore};
use crate::resource::{DeviceId, DeviceSelector};
use crate::{AuvContext, ContextError};

/// Operating-system family reported by a Device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePlatform {
  /// The daemon did not report a platform.
  Unspecified,
  /// A Linux Device.
  Linux,
  /// A macOS Device.
  Macos,
  /// A Windows Device.
  Windows,
}

/// A Device visible through the selected daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
  /// Canonical Device identity.
  pub id: DeviceId,
  /// Human-facing Device name.
  pub name: String,
  /// Reported operating-system family.
  pub platform: DevicePlatform,
  /// Whether the Device is local to the selected daemon.
  pub local: bool,
  /// Operator-defined labels.
  pub labels: HashMap<String, String>,
}

/// Current lock state reported by the target OS for a login session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserSessionLockState {
  /// The session exists but requires authentication before use.
  Locked,
  /// The session is already usable.
  Usable,
  /// The host observed the session but cannot determine its lock state.
  Unknown,
}

impl UserSessionLockState {
  /// Stable name for command and API presentations.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Locked => "LOCKED",
      Self::Usable => "USABLE",
      Self::Unknown => "UNKNOWN",
    }
  }
}

/// Target-local credential readiness for one OS account.
///
/// A stored credential stays Pending until the installed unlock host can
/// retrieve it while the selected session is locked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrollmentState {
  /// Stored locally but not yet verified under the locked host identity.
  Pending,
  /// Verified and eligible for a remote unlock request.
  Ready,
  /// Ineligible until target-local re-enrollment.
  Suspended,
}

/// How an OS login session connects to the target Device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserSessionConnectionKind {
  /// The host did not report a connection kind.
  Unspecified,
  /// A session on the physical display or console.
  Physical,
  /// A remote graphical or terminal session.
  Remote,
}

impl UserSessionConnectionKind {
  /// Stable name for command and API presentations.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Unspecified => "UNSPECIFIED",
      Self::Physical => "PHYSICAL",
      Self::Remote => "REMOTE",
    }
  }
}

/// One current OS login instance, distinct from an AUV Session resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserSession {
  /// Opaque selector, revalidated against live OS state when used.
  pub selector: String,
  /// Resolved OS account name.
  pub user: String,
  /// Current lock state.
  pub lock_state: UserSessionLockState,
  /// Seat or connection type where known.
  pub connection_kind: UserSessionConnectionKind,
  /// Host-provided seat identifier where known.
  pub seat: Option<String>,
}

impl UserSession {
  /// Whether the target OS positively reports this session as locked.
  pub fn is_locked(&self) -> bool {
    self.lock_state == UserSessionLockState::Locked
  }

  /// Whether the target OS positively reports this session as usable.
  pub fn is_unlocked(&self) -> bool {
    self.lock_state == UserSessionLockState::Usable
  }
}

/// Account or current session selected for a Device lock or unlock request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserSessionTarget {
  /// Resolve one enrolled account to exactly one existing login session.
  User(String),
  /// Resolve one current OS login session and its actual account.
  SessionSelector(String),
}

/// Independently verified effect of a Device lock operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceLockEffectKind {
  /// The selected session was already locked; no input was sent.
  AlreadyLocked,
  /// An existing usable session became locked.
  LockedExistingSession,
}

impl DeviceLockEffectKind {
  /// Stable name for command and API presentations.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::AlreadyLocked => "ALREADY_LOCKED",
      Self::LockedExistingSession => "LOCKED_EXISTING_SESSION",
    }
  }
}

/// Result returned after verifying the target OS is locked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnsureUserSessionLockedEffect {
  /// Verified outcome.
  pub kind: DeviceLockEffectKind,
  /// OS account that owns the selected existing session.
  pub user: String,
  /// Selector independently observed after the lock request.
  pub session_selector: String,
}

/// Independently verified effect of a Device entry operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceEntryEffectKind {
  /// The selected session was already usable; no credential was retrieved.
  AlreadyUsable,
  /// An existing locked session became usable.
  UnlockedExistingSession,
  // NOTICE(device-entry-signed-out): The wire keeps SIGNED_IN_NEW_SESSION
  // reserved, but no locked-session adapter can produce it, so this domain
  // type omits it and the client rejects it. A later owner-approved signed-out
  // slice must prove login-window delivery and new-session readback first.
}

impl DeviceEntryEffectKind {
  /// Stable name for command and API presentations.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::AlreadyUsable => "ALREADY_USABLE",
      Self::UnlockedExistingSession => "UNLOCKED_EXISTING_SESSION",
    }
  }
}

/// Result returned after verifying the target OS state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnsureUserSessionUnlockedEffect {
  /// Verified outcome.
  pub kind: DeviceEntryEffectKind,
  /// OS account that owns the selected existing session.
  pub user: String,
  /// Current selector if the host can report it.
  pub session_selector: Option<String>,
}

/// Policy or OS-state reason a Device entry request could not proceed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeviceEntryErrorReason {
  #[error("the paired Device is no longer authorized")]
  Unauthorized,
  #[error("remote Device entry is disabled")]
  Disabled,
  #[error("the selected OS account is not enrolled")]
  Unenrolled,
  #[error("the selected OS account credential is suspended")]
  Suspended,
  #[error("the OS account has multiple eligible sessions")]
  AmbiguousUser,
  #[error("the selected OS login session is stale")]
  StaleSession,
  #[error("another user occupies the physical desktop")]
  OccupiedDesktop,
  #[error("the current OS state does not support Device entry")]
  UnsupportedOsState,
  #[error("the Device entry service or worker is unavailable")]
  ServiceUnavailable,
  #[error("the enrolled credential was rejected")]
  CredentialRejected,
  #[error("the Device entry outcome could not be verified")]
  OutcomeUnverified,
  #[error("the target-local audit could not be written")]
  AuditUnavailable,
  #[error("the installed Device entry host is incompatible with this daemon; update AUV on the target")]
  HostIncompatible,
}

impl DeviceEntryErrorReason {
  /// Stable name for a fixed, non-secret error reason.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Unauthorized => "UNAUTHORIZED",
      Self::Disabled => "DISABLED",
      Self::Unenrolled => "UNENROLLED",
      Self::Suspended => "SUSPENDED",
      Self::AmbiguousUser => "AMBIGUOUS_USER",
      Self::StaleSession => "STALE_SESSION",
      Self::OccupiedDesktop => "OCCUPIED_DESKTOP",
      Self::UnsupportedOsState => "UNSUPPORTED_OS_STATE",
      Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",
      Self::CredentialRejected => "CREDENTIAL_REJECTED",
      Self::OutcomeUnverified => "OUTCOME_UNVERIFIED",
      Self::AuditUnavailable => "AUDIT_UNAVAILABLE",
      Self::HostIncompatible => "HOST_INCOMPATIBLE",
    }
  }
}

/// Availability of a configured paired Device profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceAvailability {
  /// The configured Device responded successfully.
  Online,
  /// The configured Device could not be reached.
  Offline,
  /// The stored credential was rejected.
  Unauthorized,
  /// The configured profile or remote identity is inconsistent.
  Invalid,
  /// Probe failed for another reason.
  Error,
}

/// One configured profile together with its live probe, when reachable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredDeviceStatus {
  /// Stored non-secret profile metadata.
  pub profile: ConfiguredDevice,
  /// Classified live availability.
  pub availability: DeviceAvailability,
  /// Live Device returned by the remote daemon.
  pub remote: Option<Device>,
}

impl ConfiguredDeviceStatus {
  /// Returns whether this profile satisfies a validated Device selector.
  pub fn matches(&self, selector: &DeviceSelector) -> bool {
    self.profile.device_id().parse::<DeviceId>().is_ok_and(|id| selector.matches(&id, self.profile.device_name()))
  }
}

impl Device {
  /// Validates that this resource is the Device independently selected by a
  /// frontend root context.
  pub fn validate_selection(&self, selected: Option<&Device>) -> Result<(), DeviceError> {
    if let Some(selected) = selected
      && selected.id != self.id
    {
      return Err(DeviceError::SelectionConflict {
        actual: self.id.to_string(),
        selected: selected.id.to_string(),
      });
    }
    Ok(())
  }
}

/// Failure from Device inventory or selector resolution.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
  /// The daemon client request failed.
  #[error(transparent)]
  Client(#[from] ClientError),
  /// The target rejected a Device entry request with a typed reason.
  #[error(transparent)]
  Entry(#[from] DeviceEntryErrorReason),
  /// A Device identity or selector is malformed.
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
  /// The daemon omitted the canonical Device identity.
  #[error("Device response omitted its canonical ID")]
  MissingIdentity,
  /// No Device satisfies the selector.
  #[error("Device selector matched no Device")]
  NotFound,
  /// More than one Device satisfies the selector.
  #[error("Device selector is ambiguous; candidate IDs: {candidate_ids}")]
  Ambiguous {
    /// Canonical IDs of the matching Devices.
    candidate_ids: String,
  },
  /// The operation target differs from the root-selected Device.
  #[error("Device {actual:?} conflicts with selected Device {selected:?}")]
  SelectionConflict {
    /// Device selected by the operation.
    actual: String,
    /// Device selected by the frontend root.
    selected: String,
  },
  /// Reading or validating a configured profile failed.
  #[error(transparent)]
  Profile(#[from] profile::ProfileError),
  /// A Device entry response omitted or gave an unknown result value.
  #[error("Device entry response is invalid")]
  InvalidEntryResponse,
}

/// Device inventory operations bound to one selected daemon.
#[derive(Clone, Debug)]
pub struct Devices {
  client: Client,
}

impl Devices {
  pub(crate) fn new(client: Client) -> Self {
    Self { client }
  }

  /// Lists Devices exposed by the selected daemon.
  pub async fn list(&self) -> Result<Vec<Device>, DeviceError> {
    self
      .client
      .grpc_client()
      .devices()
      .list_devices()
      .await
      .map_err(|status| ClientError::from_status("ListDevices", status))?
      .into_iter()
      .map(Device::try_from)
      .collect()
  }

  /// Resolves exactly one Device using the shared selector policy.
  pub async fn get(&self, selector: &DeviceSelector) -> Result<Device, DeviceError> {
    let devices = self.list().await?;
    let matches = devices.iter().filter(|device| selector.matches(&device.id, &device.name)).collect::<Vec<_>>();
    match matches.as_slice() {
      [] => Err(DeviceError::NotFound),
      [device] => Ok((*device).clone()),
      _ => Err(DeviceError::Ambiguous {
        candidate_ids: matches.iter().map(|device| device.id.to_string()).collect::<Vec<_>>().join(", "),
      }),
    }
  }

  /// Lists current OS login instances on this Device. A selector may become
  /// stale immediately and is revalidated by `get_user_session` or unlock.
  pub async fn list_user_sessions(&self) -> Result<Vec<UserSession>, DeviceError> {
    let response = self
      .client
      .grpc_client()
      .devices()
      .list_user_sessions()
      .await
      .map_err(|status| ClientError::from_status("ListUserSessions", status))?;

    match response.result.ok_or(DeviceError::InvalidEntryResponse)? {
      proto::list_user_sessions_response::Result::List(list) => list.sessions.into_iter().map(UserSession::try_from).collect(),
      proto::list_user_sessions_response::Result::Error(error) => Err(entry_error(error)?.into()),
    }
  }

  /// Resolves one current OS login instance by its opaque selector.
  pub async fn get_user_session(&self, session_selector: &str) -> Result<UserSession, DeviceError> {
    let response = self
      .client
      .grpc_client()
      .devices()
      .get_user_session(session_selector)
      .await
      .map_err(|status| ClientError::from_status("GetUserSession", status))?;

    match response.result.ok_or(DeviceError::InvalidEntryResponse)? {
      proto::get_user_session_response::Result::Session(session) => UserSession::try_from(session),
      proto::get_user_session_response::Result::Error(error) => Err(entry_error(error)?.into()),
    }
  }

  /// Asks the target to make one existing OS session usable. The remote request
  /// carries no credential or input sequence; the target verifies OS state.
  pub async fn ensure_user_session_unlocked(&self, target: UserSessionTarget) -> Result<EnsureUserSessionUnlockedEffect, DeviceError> {
    let target = match target {
      UserSessionTarget::User(user) => proto::ensure_user_session_unlocked_request::Target::User(user),
      UserSessionTarget::SessionSelector(selector) => proto::ensure_user_session_unlocked_request::Target::SessionSelector(selector),
    };
    let response = self
      .client
      .grpc_client()
      .devices()
      .ensure_user_session_unlocked(proto::EnsureUserSessionUnlockedRequest {
        target: Some(target),
      })
      .await
      .map_err(|status| ClientError::from_status("EnsureUserSessionUnlocked", status))?;

    match response.result.ok_or(DeviceError::InvalidEntryResponse)? {
      proto::ensure_user_session_unlocked_response::Result::Effect(effect) => effect.try_into(),
      proto::ensure_user_session_unlocked_response::Result::Error(error) => Err(entry_error(error)?.into()),
    }
  }

  /// Asks the target to lock one existing usable OS session. The target
  /// revalidates the selected login instance and verifies its final state.
  pub async fn ensure_user_session_locked(&self, target: UserSessionTarget) -> Result<EnsureUserSessionLockedEffect, DeviceError> {
    let target = match target {
      UserSessionTarget::User(user) => proto::ensure_user_session_locked_request::Target::User(user),
      UserSessionTarget::SessionSelector(selector) => proto::ensure_user_session_locked_request::Target::SessionSelector(selector),
    };
    let response = self
      .client
      .grpc_client()
      .devices()
      .ensure_user_session_locked(proto::EnsureUserSessionLockedRequest {
        target: Some(target),
      })
      .await
      .map_err(|status| ClientError::from_status("EnsureUserSessionLocked", status))?;

    match response.result.ok_or(DeviceError::InvalidEntryResponse)? {
      proto::ensure_user_session_locked_response::Result::Effect(effect) => effect.try_into(),
      proto::ensure_user_session_locked_response::Result::Error(error) => Err(entry_error(error)?.into()),
    }
  }

  /// Observes all configured paired profiles without failing the whole list
  /// when an individual remote is offline or unauthorized.
  pub async fn probe_configured(store: &ProfileStore) -> Result<Vec<ConfiguredDeviceStatus>, DeviceError> {
    let configured = match store.list_devices() {
      Ok(configured) => configured,
      Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(error) => return Err(error.into()),
    };
    Ok(join_all(configured.into_iter().map(|profile| probe_profile(store, profile))).await)
  }
}

async fn probe_profile(store: &ProfileStore, profile: ConfiguredDevice) -> ConfiguredDeviceStatus {
  let context = AuvContext {
    config_profile: Some(profile.config_profile().to_string()),
    ..AuvContext::default()
  };
  match Client::from_context_with_profiles(context, store).await {
    Ok(client) => match client.devices().list().await {
      Ok(devices) => {
        let remote = devices.into_iter().find(|device| device.id.as_str() == profile.device_id());
        ConfiguredDeviceStatus {
          availability: if remote.is_some() {
            DeviceAvailability::Online
          } else {
            DeviceAvailability::Invalid
          },
          profile,
          remote,
        }
      }
      Err(error) => ConfiguredDeviceStatus {
        availability: availability_from_device_error(&error),
        profile,
        remote: None,
      },
    },
    Err(error) => ConfiguredDeviceStatus {
      availability: availability_from_context_error(&error),
      profile,
      remote: None,
    },
  }
}

fn availability_from_device_error(error: &DeviceError) -> DeviceAvailability {
  match error {
    DeviceError::Client(error) if error.kind() == ClientErrorKind::Unauthorized => DeviceAvailability::Unauthorized,
    DeviceError::Client(error) if error.kind() == ClientErrorKind::Unavailable => DeviceAvailability::Offline,
    DeviceError::Identity(_) | DeviceError::MissingIdentity => DeviceAvailability::Invalid,
    _ => DeviceAvailability::Error,
  }
}

fn availability_from_context_error(error: &ContextError) -> DeviceAvailability {
  match error {
    ContextError::Connect(_) | ContextError::PairedConnect(_) => DeviceAvailability::Offline,
    ContextError::RemoteDeviceList(error) if error.kind() == ClientErrorKind::Unauthorized => DeviceAvailability::Unauthorized,
    ContextError::RemoteDeviceList(error) if error.kind() == ClientErrorKind::Unavailable => DeviceAvailability::Offline,
    ContextError::Profile(_) | ContextError::ProfileEndpointMismatch { .. } | ContextError::CanonicalDeviceMissing(_) => {
      DeviceAvailability::Invalid
    }
    _ => DeviceAvailability::Error,
  }
}

impl TryFrom<proto::Device> for Device {
  type Error = DeviceError;

  fn try_from(device: proto::Device) -> Result<Self, Self::Error> {
    let id = device.r#ref.ok_or(DeviceError::MissingIdentity)?.device_id;
    let platform = match proto::DevicePlatform::try_from(device.platform).unwrap_or(proto::DevicePlatform::Unspecified) {
      proto::DevicePlatform::Unspecified => DevicePlatform::Unspecified,
      proto::DevicePlatform::Linux => DevicePlatform::Linux,
      proto::DevicePlatform::Macos => DevicePlatform::Macos,
      proto::DevicePlatform::Windows => DevicePlatform::Windows,
    };
    Ok(Self {
      id: DeviceId::from_str(&id)?,
      name: device.name,
      platform,
      local: device.local,
      labels: device.labels,
    })
  }
}

impl TryFrom<proto::UserSession> for UserSession {
  type Error = DeviceError;

  fn try_from(session: proto::UserSession) -> Result<Self, Self::Error> {
    if session.session_selector.is_empty() || session.user.is_empty() {
      return Err(DeviceError::InvalidEntryResponse);
    }

    let lock_state = match proto::UserSessionLockState::try_from(session.lock_state).map_err(|_| DeviceError::InvalidEntryResponse)? {
      proto::UserSessionLockState::Unspecified => return Err(DeviceError::InvalidEntryResponse),
      proto::UserSessionLockState::Locked => UserSessionLockState::Locked,
      proto::UserSessionLockState::Usable => UserSessionLockState::Usable,
      proto::UserSessionLockState::Unknown => UserSessionLockState::Unknown,
    };
    let connection_kind =
      match proto::UserSessionConnectionKind::try_from(session.connection_kind).map_err(|_| DeviceError::InvalidEntryResponse)? {
        proto::UserSessionConnectionKind::Unspecified => UserSessionConnectionKind::Unspecified,
        proto::UserSessionConnectionKind::Physical => UserSessionConnectionKind::Physical,
        proto::UserSessionConnectionKind::Remote => UserSessionConnectionKind::Remote,
      };
    Ok(Self {
      selector: session.session_selector,
      user: session.user,
      lock_state,
      connection_kind,
      seat: (!session.seat.is_empty()).then_some(session.seat),
    })
  }
}

impl TryFrom<proto::EnsureUserSessionUnlockedEffect> for EnsureUserSessionUnlockedEffect {
  type Error = DeviceError;

  fn try_from(effect: proto::EnsureUserSessionUnlockedEffect) -> Result<Self, Self::Error> {
    if effect.user.is_empty() {
      return Err(DeviceError::InvalidEntryResponse);
    }

    let kind = match proto::DeviceEntryEffectKind::try_from(effect.kind).map_err(|_| DeviceError::InvalidEntryResponse)? {
      proto::DeviceEntryEffectKind::Unspecified => return Err(DeviceError::InvalidEntryResponse),
      proto::DeviceEntryEffectKind::AlreadyUsable => DeviceEntryEffectKind::AlreadyUsable,
      proto::DeviceEntryEffectKind::UnlockedExistingSession => DeviceEntryEffectKind::UnlockedExistingSession,
      // NOTICE(device-entry-signed-out): The wire value remains reserved, but
      // this locked-session facade must never report it as a verified effect.
      proto::DeviceEntryEffectKind::SignedInNewSession => return Err(DeviceError::InvalidEntryResponse),
    };

    Ok(Self {
      kind,
      user: effect.user,
      session_selector: (!effect.session_selector.is_empty()).then_some(effect.session_selector),
    })
  }
}

impl TryFrom<proto::EnsureUserSessionLockedEffect> for EnsureUserSessionLockedEffect {
  type Error = DeviceError;

  fn try_from(effect: proto::EnsureUserSessionLockedEffect) -> Result<Self, Self::Error> {
    if effect.user.is_empty() || effect.session_selector.is_empty() {
      return Err(DeviceError::InvalidEntryResponse);
    }

    let kind = match proto::DeviceLockEffectKind::try_from(effect.kind).map_err(|_| DeviceError::InvalidEntryResponse)? {
      proto::DeviceLockEffectKind::Unspecified => return Err(DeviceError::InvalidEntryResponse),
      proto::DeviceLockEffectKind::AlreadyLocked => DeviceLockEffectKind::AlreadyLocked,
      proto::DeviceLockEffectKind::LockedExistingSession => DeviceLockEffectKind::LockedExistingSession,
    };

    Ok(Self {
      kind,
      user: effect.user,
      session_selector: effect.session_selector,
    })
  }
}

fn entry_error(error: proto::DeviceEntryError) -> Result<DeviceEntryErrorReason, DeviceError> {
  Ok(match proto::DeviceEntryErrorReason::try_from(error.reason).map_err(|_| DeviceError::InvalidEntryResponse)? {
    proto::DeviceEntryErrorReason::Unspecified => return Err(DeviceError::InvalidEntryResponse),
    proto::DeviceEntryErrorReason::Unauthorized => DeviceEntryErrorReason::Unauthorized,
    proto::DeviceEntryErrorReason::Disabled => DeviceEntryErrorReason::Disabled,
    proto::DeviceEntryErrorReason::Unenrolled => DeviceEntryErrorReason::Unenrolled,
    proto::DeviceEntryErrorReason::Suspended => DeviceEntryErrorReason::Suspended,
    proto::DeviceEntryErrorReason::AmbiguousUser => DeviceEntryErrorReason::AmbiguousUser,
    proto::DeviceEntryErrorReason::StaleSession => DeviceEntryErrorReason::StaleSession,
    proto::DeviceEntryErrorReason::OccupiedDesktop => DeviceEntryErrorReason::OccupiedDesktop,
    proto::DeviceEntryErrorReason::UnsupportedOsState => DeviceEntryErrorReason::UnsupportedOsState,
    proto::DeviceEntryErrorReason::ServiceUnavailable => DeviceEntryErrorReason::ServiceUnavailable,
    proto::DeviceEntryErrorReason::CredentialRejected => DeviceEntryErrorReason::CredentialRejected,
    proto::DeviceEntryErrorReason::OutcomeUnverified => DeviceEntryErrorReason::OutcomeUnverified,
    proto::DeviceEntryErrorReason::AuditUnavailable => DeviceEntryErrorReason::AuditUnavailable,
    proto::DeviceEntryErrorReason::HostIncompatible => DeviceEntryErrorReason::HostIncompatible,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn user_session_decodes_current_os_state_and_opaque_selector() {
    let session = UserSession::try_from(proto::UserSession {
      session_selector: "seat0:42".into(),
      user: "neko".into(),
      lock_state: proto::UserSessionLockState::Locked as i32,
      connection_kind: proto::UserSessionConnectionKind::Physical as i32,
      seat: "seat0".into(),
    })
    .unwrap();

    assert_eq!(session.selector, "seat0:42");
    assert_eq!(session.user, "neko");
    assert_eq!(session.lock_state, UserSessionLockState::Locked);
    assert!(session.is_locked());
    assert!(!session.is_unlocked());
    assert_eq!(session.connection_kind, UserSessionConnectionKind::Physical);
    assert_eq!(session.seat.as_deref(), Some("seat0"));

    let usable = UserSession {
      lock_state: UserSessionLockState::Usable,
      ..session
    };

    assert!(!usable.is_locked());
    assert!(usable.is_unlocked());
  }

  #[test]
  fn unknown_lock_state_does_not_claim_locked_or_unlocked() {
    let session = UserSession::try_from(proto::UserSession {
      session_selector: "seat0:42".into(),
      user: "neko".into(),
      lock_state: proto::UserSessionLockState::Unknown as i32,
      connection_kind: proto::UserSessionConnectionKind::Physical as i32,
      seat: "seat0".into(),
    })
    .unwrap();

    assert_eq!(session.lock_state.as_str(), "UNKNOWN");
    assert!(!session.is_locked());
    assert!(!session.is_unlocked());

    assert!(matches!(
      UserSession::try_from(proto::UserSession {
        lock_state: proto::UserSessionLockState::Unspecified as i32,
        ..proto::UserSession::default()
      }),
      Err(DeviceError::InvalidEntryResponse)
    ));
  }

  #[test]
  fn entry_effect_requires_a_verified_kind_and_account() {
    let effect = proto::EnsureUserSessionUnlockedEffect {
      kind: proto::DeviceEntryEffectKind::UnlockedExistingSession as i32,
      user: "neko".into(),
      session_selector: "seat0:42".into(),
    };

    assert_eq!(
      EnsureUserSessionUnlockedEffect::try_from(effect.clone()).unwrap(),
      EnsureUserSessionUnlockedEffect {
        kind: DeviceEntryEffectKind::UnlockedExistingSession,
        user: "neko".into(),
        session_selector: Some("seat0:42".into()),
      }
    );
    assert!(matches!(
      EnsureUserSessionUnlockedEffect::try_from(proto::EnsureUserSessionUnlockedEffect {
        kind: proto::DeviceEntryEffectKind::Unspecified as i32,
        ..effect.clone()
      }),
      Err(DeviceError::InvalidEntryResponse)
    ));

    assert!(matches!(
      EnsureUserSessionUnlockedEffect::try_from(proto::EnsureUserSessionUnlockedEffect {
        kind: proto::DeviceEntryEffectKind::SignedInNewSession as i32,
        user: "neko".into(),
        session_selector: "seat0:42".into(),
      }),
      Err(DeviceError::InvalidEntryResponse)
    ));

    assert!(matches!(
      EnsureUserSessionUnlockedEffect::try_from(proto::EnsureUserSessionUnlockedEffect {
        user: String::new(),
        ..effect
      }),
      Err(DeviceError::InvalidEntryResponse)
    ));
  }

  #[test]
  fn lock_effect_requires_verified_kind_and_exact_session() {
    let effect = proto::EnsureUserSessionLockedEffect {
      kind: proto::DeviceLockEffectKind::LockedExistingSession as i32,
      user: "neko".into(),
      session_selector: "macos:login".into(),
    };

    assert_eq!(
      EnsureUserSessionLockedEffect::try_from(effect.clone()).unwrap(),
      EnsureUserSessionLockedEffect {
        kind: DeviceLockEffectKind::LockedExistingSession,
        user: "neko".into(),
        session_selector: "macos:login".into(),
      }
    );
    assert!(
      EnsureUserSessionLockedEffect::try_from(proto::EnsureUserSessionLockedEffect {
        kind: 0,
        ..effect.clone()
      })
      .is_err()
    );
    assert!(
      EnsureUserSessionLockedEffect::try_from(proto::EnsureUserSessionLockedEffect {
        session_selector: String::new(),
        ..effect
      })
      .is_err()
    );
  }

  #[test]
  fn fixed_wire_error_reason_remains_typed() {
    let reason = entry_error(proto::DeviceEntryError {
      reason: proto::DeviceEntryErrorReason::Suspended as i32,
    })
    .unwrap();

    assert_eq!(reason, DeviceEntryErrorReason::Suspended);
    assert_eq!(
      entry_error(proto::DeviceEntryError {
        reason: proto::DeviceEntryErrorReason::Unauthorized as i32,
      })
      .unwrap(),
      DeviceEntryErrorReason::Unauthorized
    );
    assert_eq!(
      entry_error(proto::DeviceEntryError {
        reason: proto::DeviceEntryErrorReason::HostIncompatible as i32,
      })
      .unwrap(),
      DeviceEntryErrorReason::HostIncompatible
    );
    assert!(matches!(
      entry_error(proto::DeviceEntryError {
        reason: proto::DeviceEntryErrorReason::Unspecified as i32,
      }),
      Err(DeviceError::InvalidEntryResponse)
    ));
  }
}
