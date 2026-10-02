//! Target-local macOS enrollment through the installed, signed Aqua helper.
//!
//! Metadata remains in the daemon. The helper alone writes or reads the
//! account's login Keychain item; no credential enters the remote Device API.
// Enrollment publishes PENDING. The remote policy promotes it only after the
// signed helper verifies locked retrieval for the same login instance.

use std::sync::Arc;

use auv_api_server::device_local::{
  AuditPage as LocalAuditPage, CredentialKind, DeviceLocalControl, EnrollAccount, Enrollment as LocalEnrollment, LocalControlError,
  LocalOsPrincipal,
};
use auv_device_helper_macos::HostError;

use super::audit::Audit;
use super::local::{
  audit_page, authorize_unix as authorize, complete_account_mutation, local_enrollment, require_unix_admin as require_os_admin,
  unix_account_id as account_id, unix_uid,
};
use super::metadata::MetadataStore;
use super::policy::AccountLocks;
use super::unix_account::resolve_user;

pub(super) struct MacosLocalEnrollment {
  metadata: Arc<MetadataStore>,
  account_locks: Arc<AccountLocks>,
  audit: Arc<Audit>,
  policy_gate: Arc<tokio::sync::RwLock<()>>,
}

impl MacosLocalEnrollment {
  /// These are the same owners used by remote policy, so invalidation and
  /// unlock cannot race through separate locks or metadata generations.
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
impl DeviceLocalControl for MacosLocalEnrollment {
  async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<LocalEnrollment, LocalControlError> {
    let account = resolve_user(user)?;
    authorize(principal, account.uid)?;
    self
      .metadata
      .enrollment(&account.id)
      .map_err(|_| LocalControlError::Persistence)?
      .map(local_enrollment)
      .ok_or(LocalControlError::NotFound)
  }

  async fn list_enrollments(&self, principal: &LocalOsPrincipal) -> Result<Vec<LocalEnrollment>, LocalControlError> {
    let uid = unix_uid(principal)?;
    let own_id = account_id(uid);
    let records = self.metadata.list_enrollments().map_err(|_| LocalControlError::Persistence)?;
    Ok(records.into_iter().filter(|record| uid == 0 || record.os_account_id == own_id).map(local_enrollment).collect())
  }

  async fn enroll(&self, principal: &LocalOsPrincipal, request: EnrollAccount) -> Result<LocalEnrollment, LocalControlError> {
    if request.credential_kind != CredentialKind::OsPassword {
      return Err(LocalControlError::UnsupportedCredentialKind);
    }

    if request.credential.as_bytes().is_empty() || request.credential.as_bytes().len() > 1024 {
      return Err(LocalControlError::InvalidCredential);
    }

    let account = resolve_user(&request.user)?;
    authorize(principal, account.uid)?;

    if account.uid == 0 {
      // The signed Aqua host is a per-user LaunchAgent, not a root login host.
      return Err(LocalControlError::InvalidAccount);
    }

    let guard = self.account_locks.lock(&account.id).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      // A failed replacement must not leave an old READY credential eligible.
      metadata.invalidate_for_enroll(&account.name, &account.id).map_err(|_| LocalControlError::Persistence)?;
      auv_device_helper_macos::enroll(&account.home, account.uid, request.credential.as_bytes()).map_err(enroll_host_error)?;
      let stored = metadata.publish_pending(&account.name, &account.id).map_err(|_| LocalControlError::Persistence)?;
      Ok(local_enrollment(stored))
    })
    .await
  }

  async fn remove_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<(), LocalControlError> {
    let account = resolve_user(user)?;
    authorize(principal, account.uid)?;
    let guard = self.account_locks.lock(&account.id).await.map_err(|_| LocalControlError::Persistence)?;
    let metadata = Arc::clone(&self.metadata);
    complete_account_mutation(guard, move || {
      // Revocation is durable before the helper is asked to delete the item.
      // A retry also removes an orphan after a previous partial failure.
      metadata.remove(&account.id).map_err(|_| LocalControlError::Persistence)?;
      auv_device_helper_macos::remove(&account.home, account.uid).map_err(|_| LocalControlError::HostUnavailable)
    })
    .await
  }

  async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
    unix_uid(principal)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError> {
    require_os_admin(principal)?;
    // Policy keeps the read side through accepted native input and audit.
    // Publishing a disable waits for those requests before returning.
    let _policy_guard = self.policy_gate.write().await;
    self.metadata.set_enabled(enabled).map_err(|_| LocalControlError::Persistence)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn list_audit(&self, principal: &LocalOsPrincipal, cursor: u64, limit: usize) -> Result<LocalAuditPage, LocalControlError> {
    unix_uid(principal)?;
    audit_page(&self.audit, principal, cursor, limit)
  }
}

fn enroll_host_error(error: HostError) -> LocalControlError {
  match error {
    HostError::InvalidRequest => LocalControlError::InvalidCredential,
    HostError::ProtocolUnsupported | HostError::Revoked => LocalControlError::HostIncompatible,
    _ => LocalControlError::HostUnavailable,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::os::unix::fs::PermissionsExt;

  #[test]
  fn rejects_cross_uid_caller() {
    assert_eq!(authorize(&LocalOsPrincipal::UnixUid(501), 502), Err(LocalControlError::PermissionDenied));
    assert!(authorize(&LocalOsPrincipal::UnixUid(501), 501).is_ok());
    assert!(authorize(&LocalOsPrincipal::UnixUid(0), 502).is_ok());
    assert_eq!(require_os_admin(&LocalOsPrincipal::UnixUid(501)), Err(LocalControlError::PermissionDenied));
  }

  #[tokio::test]
  async fn local_reads_filter_metadata_and_require_root_for_policy_mutation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let metadata = Arc::new(MetadataStore::open(root.path()).unwrap());
    let audit = Arc::new(Audit::open(root.path()).unwrap());

    for uid in [501_u32, 502] {
      let id = account_id(uid);
      metadata.invalidate_for_enroll("test", &id).unwrap();
      metadata.publish_pending("test", &id).unwrap();
    }

    let backend = MacosLocalEnrollment::new(metadata, Arc::new(AccountLocks::new()), audit, Arc::new(tokio::sync::RwLock::new(())));
    let own = backend.list_enrollments(&LocalOsPrincipal::UnixUid(501)).await.unwrap();

    assert_eq!(own.len(), 1);
    assert_eq!(own[0].os_account_id, "uid:501");
    assert_eq!(own[0].state, auv::devices::EnrollmentState::Pending);
    assert_eq!(backend.list_enrollments(&LocalOsPrincipal::UnixUid(0)).await.unwrap().len(), 2);
    assert_eq!(backend.set_policy(&LocalOsPrincipal::UnixUid(501), false).await, Err(LocalControlError::PermissionDenied));
    assert!(!backend.set_policy(&LocalOsPrincipal::UnixUid(0), false).await.unwrap());
    assert_eq!(backend.get_policy(&LocalOsPrincipal::UnixUid(501)).await.unwrap(), false);
  }
}
