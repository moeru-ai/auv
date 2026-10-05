//! Target-local Windows PIN enrollment in the per-user daemon.
//!
//! The named-pipe transport supplies a verified client SID. A caller manages
//! only the enrollment of its own account; the requested user name is checked
//! against that SID and is never authority. Metadata stays here; the PIN goes
//! only to the Helper Host vault, which also accepts it only for the caller's
//! own account SID.

use std::sync::Arc;

use auv_api_server::device_local::{
  AuditPage as LocalAuditPage, CredentialKind, DeviceLocalControl, EnrollAccount, Enrollment as LocalEnrollment, LocalControlError,
  LocalOsPrincipal,
};
use auv_device_helper_windows::HostError;

use super::audit::Audit;
use super::local::{audit_page, complete_account_mutation, local_enrollment};
use super::metadata::MetadataStore;
use super::policy::AccountLocks;

pub(super) struct WindowsLocalEnrollment {
  metadata: Arc<MetadataStore>,
  account_locks: Arc<AccountLocks>,
  audit: Arc<Audit>,
  policy_gate: Arc<tokio::sync::RwLock<()>>,
}

impl WindowsLocalEnrollment {
  /// The installed host must pass the same owners to Device policy.
  pub(super) fn new(
    metadata: Arc<MetadataStore>,
    account_locks: Arc<AccountLocks>,
    audit: Arc<Audit>,
    policy_gate: Arc<tokio::sync::RwLock<()>>,
  ) -> Self {
    Self {
      metadata,
      account_locks,
      audit,
      policy_gate,
    }
  }
}

#[tonic::async_trait]
impl DeviceLocalControl for WindowsLocalEnrollment {
  async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<LocalEnrollment, LocalControlError> {
    let sid = verified_sid(principal)?;
    let stored = self.metadata.enrollment(sid).map_err(|_| LocalControlError::Persistence)?.ok_or(LocalControlError::NotFound)?;

    if stored.user != user {
      return Err(LocalControlError::NotFound);
    }

    Ok(local_enrollment(stored))
  }

  async fn list_enrollments(&self, principal: &LocalOsPrincipal) -> Result<Vec<LocalEnrollment>, LocalControlError> {
    let sid = verified_sid(principal)?;
    let records = self.metadata.list_enrollments().map_err(|_| LocalControlError::Persistence)?;
    Ok(records.into_iter().filter(|record| record.os_account_id == sid).map(local_enrollment).collect())
  }

  async fn enroll(&self, principal: &LocalOsPrincipal, request: EnrollAccount) -> Result<LocalEnrollment, LocalControlError> {
    if request.credential_kind != CredentialKind::WindowsPin {
      // TODO(device-entry-windows-password): OS-password provider delivery
      // needs its own locked Winlogon gate before it can be enrolled here.
      return Err(LocalControlError::UnsupportedCredentialKind);
    }

    let credential = request.credential;
    let text = std::str::from_utf8(credential.as_bytes()).map_err(|_| LocalControlError::InvalidCredential)?;

    if text.is_empty() || text.chars().any(char::is_control) || text.encode_utf16().count() > 128 {
      return Err(LocalControlError::InvalidCredential);
    }

    // Enrollment targets the caller's own live console login. The Helper Host
    // applies the same rule with the daemon's token SID.
    let console = auv_device_helper_windows::observe().map_err(helper_error)?.ok_or(LocalControlError::HostUnavailable)?;
    let name = console.account_name();
    let sid = console.account_sid;

    if request.user != name || sid.is_empty() {
      return Err(LocalControlError::InvalidAccount);
    }

    if sid != verified_sid(principal)? {
      return Err(LocalControlError::PermissionDenied);
    }

    let guard = self.account_locks.lock(&sid).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      // Revoke the old generation before replacement. A failed DPAPI write
      // cannot leave the previously Ready PIN eligible for remote delivery.
      metadata.invalidate_for_enroll(&name, &sid).map_err(|_| LocalControlError::Persistence)?;
      let text = std::str::from_utf8(credential.as_bytes()).map_err(|_| LocalControlError::InvalidCredential)?;
      auv_device_helper_windows::enroll(&sid, text).map_err(helper_error)?;
      let stored = metadata.publish_pending(&name, &sid).map_err(|_| LocalControlError::Persistence)?;
      Ok(local_enrollment(stored))
    })
    .await
  }

  async fn remove_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<(), LocalControlError> {
    let sid = verified_sid(principal)?.to_owned();
    let stored = self.metadata.enrollment(&sid).map_err(|_| LocalControlError::Persistence)?;

    if stored.as_ref().is_some_and(|stored| stored.user != user) {
      return Err(LocalControlError::NotFound);
    }

    // A verified account owner may retry deletion of their own SID after
    // metadata was already tombstoned by a partial prior attempt.
    let guard = self.account_locks.lock(&sid).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      let existed = metadata.enrollment(&sid).map_err(|_| LocalControlError::Persistence)?.is_some();
      metadata.remove(&sid).map_err(|_| LocalControlError::Persistence)?;

      match auv_device_helper_windows::remove(&sid) {
        Ok(()) => Ok(()),
        Err(HostError::NotEnrolled) if existed => Ok(()),
        Err(error) => Err(helper_error(error)),
      }
    })
    .await
  }

  async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
    verified_sid(principal)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError> {
    // The remote-entry switch needs an elevated caller, like the Unix root
    // check. The policy file stays writable by the daemon's own user.
    if !matches!(principal, LocalOsPrincipal::WindowsAdministratorSid(_)) || verified_sid(principal).is_err() {
      return Err(LocalControlError::PermissionDenied);
    }

    let _guard = self.policy_gate.write().await;
    self.metadata.set_enabled(enabled).map_err(|_| LocalControlError::Persistence)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn list_audit(&self, principal: &LocalOsPrincipal, cursor: u64, limit: usize) -> Result<LocalAuditPage, LocalControlError> {
    verified_sid(principal)?;
    audit_page(&self.audit, principal, cursor, limit)
  }
}

fn verified_sid(principal: &LocalOsPrincipal) -> Result<&str, LocalControlError> {
  match principal {
    LocalOsPrincipal::WindowsSid(sid) | LocalOsPrincipal::WindowsAdministratorSid(sid) if sid.starts_with("S-1-") && sid.len() <= 128 => {
      Ok(sid)
    }
    _ => Err(LocalControlError::PermissionDenied),
  }
}

fn helper_error(error: HostError) -> LocalControlError {
  match error {
    HostError::InvalidRequest => LocalControlError::InvalidAccount,
    HostError::InvalidCredential => LocalControlError::InvalidCredential,
    HostError::NotEnrolled => LocalControlError::NotFound,
    // The Helper Host stores PINs only for this daemon's own account.
    HostError::Unauthorized => LocalControlError::PermissionDenied,
    HostError::ProtocolUnsupported => LocalControlError::HostIncompatible,
    HostError::Unavailable
    | HostError::Untrusted
    | HostError::StaleSession
    | HostError::NotLocked
    | HostError::VaultUnavailable
    | HostError::Unverified => LocalControlError::HostUnavailable,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn only_a_windows_sid_identifies_the_caller() {
    assert_eq!(verified_sid(&LocalOsPrincipal::WindowsSid("S-1-5-21-1-2-3-1001".into())), Ok("S-1-5-21-1-2-3-1001"));
    assert_eq!(verified_sid(&LocalOsPrincipal::WindowsAdministratorSid("S-1-5-21-1-2-3-1001".into())), Ok("S-1-5-21-1-2-3-1001"));
    assert_eq!(verified_sid(&LocalOsPrincipal::WindowsSid("not-a-sid".into())), Err(LocalControlError::PermissionDenied));
    assert_eq!(verified_sid(&LocalOsPrincipal::UnixUid(0)), Err(LocalControlError::PermissionDenied));
  }
}
