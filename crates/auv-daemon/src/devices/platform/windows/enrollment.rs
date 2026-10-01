//! Candidate target-local Windows PIN enrollment for a LocalSystem service.
//!
//! The named-pipe transport supplies a verified client SID. This backend
//! never treats the requested user name as authority or lets a paired caller
//! choose a credential. Binding waits for restricted metadata and audit files.

use std::sync::Arc;

use auv_api_server::device_local::{
  AuditPage as LocalAuditPage, CredentialKind, DeviceLocalControl, EnrollAccount, Enrollment as LocalEnrollment, LocalControlError,
  LocalOsPrincipal,
};
use auv_driver_windows::device_session::observe_console;

use super::audit::Audit;
use super::local::{audit_page, complete_account_mutation, local_enrollment};
use super::metadata::MetadataStore;
use super::policy::{AccountLocks, Enrollment as StoredEnrollment};
use super::vault_windows::{VaultError, enroll as vault_enroll, remove as vault_remove};

const LOCAL_SYSTEM_SID: &str = "S-1-5-18";

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

  fn current_account(&self, user: &str) -> Result<(String, String), LocalControlError> {
    let console = observe_console().map_err(|_| LocalControlError::HostUnavailable)?.ok_or(LocalControlError::HostUnavailable)?;
    let name = console.account_name();

    if user != name || console.account_sid.is_empty() {
      return Err(LocalControlError::InvalidAccount);
    }

    Ok((name, console.account_sid))
  }

  fn stored_account(&self, principal: &LocalOsPrincipal, user: &str) -> Result<StoredEnrollment, LocalControlError> {
    let sid = verified_sid(principal)?;
    let stored = if sid == LOCAL_SYSTEM_SID {
      let mut matches =
        self.metadata.list_enrollments().map_err(|_| LocalControlError::Persistence)?.into_iter().filter(|record| record.user == user);

      let one = matches.next().ok_or(LocalControlError::NotFound)?;

      if matches.next().is_some() {
        return Err(LocalControlError::InvalidAccount);
      }

      one
    } else if is_administrator(principal) {
      // Administrator authority is limited to the selected live console
      // account. It does not turn an arbitrary stored name into a target.
      let (_, console_sid) = self.current_account(user)?;
      self.metadata.enrollment(&console_sid).map_err(|_| LocalControlError::Persistence)?.ok_or(LocalControlError::NotFound)?
    } else {
      self.metadata.enrollment(sid).map_err(|_| LocalControlError::Persistence)?.ok_or(LocalControlError::NotFound)?
    };

    if stored.user != user {
      return Err(LocalControlError::NotFound);
    }

    authorize(principal, &stored.os_account_id)?;
    Ok(stored)
  }
}

#[tonic::async_trait]
impl DeviceLocalControl for WindowsLocalEnrollment {
  async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<LocalEnrollment, LocalControlError> {
    self.stored_account(principal, user).map(local_enrollment)
  }

  async fn list_enrollments(&self, principal: &LocalOsPrincipal) -> Result<Vec<LocalEnrollment>, LocalControlError> {
    let sid = verified_sid(principal)?;
    let visible_sid = if sid == LOCAL_SYSTEM_SID {
      None
    } else if is_administrator(principal) {
      Some(observe_console().map_err(|_| LocalControlError::HostUnavailable)?.ok_or(LocalControlError::HostUnavailable)?.account_sid)
    } else {
      Some(sid.to_owned())
    };

    let records = self.metadata.list_enrollments().map_err(|_| LocalControlError::Persistence)?;
    Ok(
      records
        .into_iter()
        .filter(|record| visible_sid.as_ref().is_none_or(|visible| record.os_account_id == *visible))
        .map(local_enrollment)
        .collect(),
    )
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

    let (name, sid) = self.current_account(&request.user)?;
    authorize(principal, &sid)?;
    let guard = self.account_locks.lock(&sid).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      // Revoke the old generation before replacement. A failed DPAPI write
      // cannot leave the previously Ready PIN eligible for remote delivery.
      metadata.invalidate_for_enroll(&name, &sid).map_err(|_| LocalControlError::Persistence)?;
      let text = std::str::from_utf8(credential.as_bytes()).map_err(|_| vault_error(VaultError::RetrievalFailed))?;
      vault_enroll(&sid, text).map_err(vault_error)?;
      let stored = metadata.publish_pending(&name, &sid).map_err(|_| LocalControlError::Persistence)?;
      Ok(local_enrollment(stored))
    })
    .await
  }

  async fn remove_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<(), LocalControlError> {
    let principal_sid = verified_sid(principal)?;
    let sid = if principal_sid == LOCAL_SYSTEM_SID {
      // TODO(device-entry-windows-orphan): After a failed vault deletion and
      // metadata tombstone, a SYSTEM caller needs OS name-to-SID resolution
      // to retry cleanup without a live console. Add it with the installed
      // service account resolver.
      self.stored_account(principal, user)?.os_account_id
    } else if is_administrator(principal) {
      self.current_account(user)?.1
    } else {
      let stored = self.metadata.enrollment(principal_sid).map_err(|_| LocalControlError::Persistence)?;

      if stored.as_ref().is_some_and(|stored| stored.user != user) {
        return Err(LocalControlError::NotFound);
      }

      // A verified account owner may retry deletion of their own SID after
      // metadata was already tombstoned by a partial prior attempt.
      principal_sid.to_owned()
    };

    let guard = self.account_locks.lock(&sid).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      let existed = metadata.enrollment(&sid).map_err(|_| LocalControlError::Persistence)?.is_some();
      metadata.remove(&sid).map_err(|_| LocalControlError::Persistence)?;

      match vault_remove(&sid) {
        Ok(()) => Ok(()),
        Err(VaultError::NotEnrolled) if existed => Ok(()),
        Err(error) => Err(vault_error(error)),
      }
    })
    .await
  }

  async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
    verified_sid(principal)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError> {
    if verified_sid(principal)? != LOCAL_SYSTEM_SID && !is_administrator(principal) {
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

fn is_administrator(principal: &LocalOsPrincipal) -> bool {
  matches!(principal, LocalOsPrincipal::WindowsAdministratorSid(_))
}

fn authorize(principal: &LocalOsPrincipal, account_sid: &str) -> Result<(), LocalControlError> {
  let sid = verified_sid(principal)?;

  if sid == LOCAL_SYSTEM_SID || is_administrator(principal) || sid == account_sid {
    Ok(())
  } else {
    Err(LocalControlError::PermissionDenied)
  }
}

fn vault_error(error: VaultError) -> LocalControlError {
  match error {
    VaultError::InvalidAccount => LocalControlError::InvalidAccount,
    VaultError::NotEnrolled => LocalControlError::NotFound,
    VaultError::Unavailable | VaultError::Permissions | VaultError::RetrievalFailed | VaultError::NotLocked => {
      LocalControlError::HostUnavailable
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sid_authorization_accepts_owner_system_and_verified_administrator() {
    let owner = LocalOsPrincipal::WindowsSid("S-1-5-21-1-2-3-1001".into());
    let other = LocalOsPrincipal::WindowsSid("S-1-5-21-1-2-3-1002".into());
    let administrator = LocalOsPrincipal::WindowsAdministratorSid("S-1-5-21-1-2-3-1002".into());
    let system = LocalOsPrincipal::WindowsSid(LOCAL_SYSTEM_SID.into());

    assert_eq!(authorize(&owner, "S-1-5-21-1-2-3-1001"), Ok(()));
    assert_eq!(authorize(&system, "S-1-5-21-1-2-3-1001"), Ok(()));
    assert_eq!(authorize(&administrator, "S-1-5-21-1-2-3-1001"), Ok(()));
    assert_eq!(authorize(&other, "S-1-5-21-1-2-3-1001"), Err(LocalControlError::PermissionDenied));
    assert_eq!(authorize(&LocalOsPrincipal::UnixUid(0), "S-1-5-21-1-2-3-1001"), Err(LocalControlError::PermissionDenied));
  }
}
