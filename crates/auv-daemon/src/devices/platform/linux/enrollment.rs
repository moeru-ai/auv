//! Target-local Linux enrollment for the same-UID GNOME host.
//!
//! The dedicated local listener persists enrollment metadata and stores the
//! credential in the target account's Secret Service. The installed gate is
//! specific to one GNOME host; other account and desktop layouts need gates.

use std::sync::Arc;

use auv_api_server::device_local::{
  AuditPage as LocalAuditPage, CredentialKind, DeviceLocalControl, EnrollAccount, Enrollment as LocalEnrollment, LocalControlError,
  LocalOsPrincipal,
};

use super::audit::Audit;
use super::local::{
  audit_page, authorize_unix as authorize, local_enrollment, require_unix_admin as require_os_admin, unix_account_id as account_id, unix_uid,
};
use super::metadata::MetadataStore;
use super::pam_native;
use super::policy::{AccountLocks, Enrollment as StoredEnrollment};
use super::unix_account::resolve_user;
use super::vault_linux::{GnomeSecretVault, VaultError};

pub(super) struct LinuxLocalEnrollment {
  metadata: Arc<MetadataStore>,
  account_locks: Arc<AccountLocks>,
  audit: Arc<Audit>,
  policy_gate: Arc<tokio::sync::RwLock<()>>,
}

impl LinuxLocalEnrollment {
  /// The caller must share these exact instances with Device policy. Opening
  /// a second metadata or audit owner would split synchronization.
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

  fn enrollment(&self, os_account_id: &str) -> Result<Option<StoredEnrollment>, LocalControlError> {
    self.metadata.enrollment(os_account_id).map_err(|_| LocalControlError::Persistence)
  }
}

#[tonic::async_trait]
impl DeviceLocalControl for LinuxLocalEnrollment {
  async fn get_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<LocalEnrollment, LocalControlError> {
    let account = resolve_user(user)?;
    authorize(principal, account.uid)?;
    let stored = self.enrollment(&account.id)?.ok_or(LocalControlError::NotFound)?;
    Ok(local_enrollment(stored))
  }

  async fn list_enrollments(&self, principal: &LocalOsPrincipal) -> Result<Vec<LocalEnrollment>, LocalControlError> {
    let caller_uid = unix_uid(principal)?;
    let own_id = account_id(caller_uid);
    let enrollments = self.metadata.list_enrollments().map_err(|_| LocalControlError::Persistence)?;
    Ok(enrollments.into_iter().filter(|record| caller_uid == 0 || record.os_account_id == own_id).map(local_enrollment).collect())
  }

  async fn enroll(&self, principal: &LocalOsPrincipal, request: EnrollAccount) -> Result<LocalEnrollment, LocalControlError> {
    if request.credential_kind != CredentialKind::OsPassword {
      return Err(LocalControlError::UnsupportedCredentialKind);
    }

    let account = resolve_user(&request.user)?;
    authorize(principal, account.uid)?;

    if current_euid() != account.uid {
      // An administrator may authorize another account, but the current host
      // cannot access that user's Secret Service. An authorized per-user host
      // is required before cross-account enrollment can be enabled.
      return Err(LocalControlError::HostUnavailable);
    }

    let _guard = self.account_locks.lock(&account.id).await.map_err(|_| LocalControlError::Persistence)?;
    let credential = request.credential;
    // The GNOME logind Unlock method does not consume this secret, but the
    // accepted Device policy still requires a usable OS login credential.
    // Authenticate against the target's GDM password PAM service before any
    // metadata or vault write. A PAM service that does not request this
    // password, changes the account, or cannot run fails closed.
    let account_name = account.name.clone();
    let credential =
      tokio::task::spawn_blocking(move || pam_native::verify_password(&account_name, credential.as_bytes()).map(|()| credential))
        .await
        .map_err(|_| LocalControlError::HostUnavailable)?
        .map_err(|error| match error {
          pam_native::VerifyError::Rejected => LocalControlError::InvalidCredential,
          pam_native::VerifyError::Unavailable => LocalControlError::HostUnavailable,
        })?;
    // Durable invalidation precedes replacement. A crash or vault error after
    // this point leaves the old generation ineligible for remote unlock.
    self.metadata.invalidate_for_enroll(&account.name, &account.id).map_err(|_| LocalControlError::Persistence)?;
    let vault = GnomeSecretVault::connect_for_current_user().await.map_err(vault_error)?;
    vault.store(account.uid, credential.as_bytes()).await.map_err(vault_error)?;
    let stored = self.metadata.publish_pending(&account.name, &account.id).map_err(|_| LocalControlError::Persistence)?;
    Ok(local_enrollment(stored))
  }

  async fn remove_enrollment(&self, principal: &LocalOsPrincipal, user: &str) -> Result<(), LocalControlError> {
    let account = resolve_user(user)?;
    authorize(principal, account.uid)?;
    let _guard = self.account_locks.lock(&account.id).await.map_err(|_| LocalControlError::Persistence)?;
    let existed = self.enrollment(&account.id)?.is_some();
    // Eligibility is revoked durably before touching the vault. If cleanup
    // fails, the caller sees an error but no later remote request can unlock.
    // Retrying this method can still remove an orphan after the tombstone.
    self.metadata.remove(&account.id).map_err(|_| LocalControlError::Persistence)?;

    if current_euid() != account.uid {
      return Err(LocalControlError::HostUnavailable);
    }

    let vault = GnomeSecretVault::connect_for_current_user().await.map_err(vault_error)?;

    match vault.remove(account.uid).await {
      Ok(()) => Ok(()),
      Err(VaultError::Missing) if existed => Ok(()),
      Err(error) => Err(vault_error(error)),
    }
  }

  async fn get_policy(&self, principal: &LocalOsPrincipal) -> Result<bool, LocalControlError> {
    unix_uid(principal)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn set_policy(&self, principal: &LocalOsPrincipal, enabled: bool) -> Result<bool, LocalControlError> {
    require_os_admin(principal)?;
    // Keep a completed disable from racing an admitted native attempt once
    // the installed Linux host is bound to DeviceService.
    let _policy_guard = self.policy_gate.write().await;
    self.metadata.set_enabled(enabled).map_err(|_| LocalControlError::Persistence)?;
    self.metadata.enabled().map_err(|_| LocalControlError::Persistence)
  }

  async fn list_audit(&self, principal: &LocalOsPrincipal, cursor: u64, limit: usize) -> Result<LocalAuditPage, LocalControlError> {
    unix_uid(principal)?;
    audit_page(&self.audit, principal, cursor, limit)
  }
}

fn vault_error(error: VaultError) -> LocalControlError {
  match error {
    VaultError::Missing => LocalControlError::NotFound,
    VaultError::InvalidSecret => LocalControlError::InvalidCredential,
    VaultError::WrongIdentity | VaultError::Unavailable | VaultError::Locked | VaultError::PromptRequired | VaultError::TimedOut => {
      LocalControlError::HostUnavailable
    }
    VaultError::Ambiguous => LocalControlError::Persistence,
  }
}

fn current_euid() -> u32 {
  // SAFETY: geteuid has no arguments or pointers.
  unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
  use auv::devices::EnrollmentState;

  use super::super::audit::Record;
  use super::*;
  use std::os::unix::fs::PermissionsExt;

  fn test_audit(root: &std::path::Path) -> Arc<Audit> {
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    Arc::new(Audit::open(root).unwrap())
  }

  #[test]
  fn root_or_account_uid_may_manage_account() {
    assert_eq!(authorize(&LocalOsPrincipal::UnixUid(1000), 1000), Ok(()));
    assert_eq!(authorize(&LocalOsPrincipal::UnixUid(0), 1000), Ok(()));
    assert_eq!(authorize(&LocalOsPrincipal::UnixUid(1001), 1000), Err(LocalControlError::PermissionDenied));
    assert_eq!(authorize(&LocalOsPrincipal::WindowsSid("S-1".into()), 1000), Err(LocalControlError::PermissionDenied));
  }

  #[test]
  fn only_verified_root_may_change_target_policy() {
    assert_eq!(require_os_admin(&LocalOsPrincipal::UnixUid(0)), Ok(()));
    assert_eq!(require_os_admin(&LocalOsPrincipal::UnixUid(1000)), Err(LocalControlError::PermissionDenied));
    assert_eq!(require_os_admin(&LocalOsPrincipal::WindowsSid("S-1".into())), Err(LocalControlError::PermissionDenied));
  }

  #[tokio::test]
  async fn local_listing_filters_by_verified_uid_without_reading_vault() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let metadata = Arc::new(MetadataStore::open(directory.path()).unwrap());
    metadata.invalidate_for_enroll("one", "uid:1000").unwrap();
    metadata.publish_pending("one", "uid:1000").unwrap();
    metadata.invalidate_for_enroll("two", "uid:1001").unwrap();
    metadata.publish_pending("two", "uid:1001").unwrap();
    let backend = LinuxLocalEnrollment::new(
      Arc::clone(&metadata),
      Arc::new(AccountLocks::new()),
      test_audit(directory.path()),
      Arc::new(tokio::sync::RwLock::new(())),
    );
    let own = backend.list_enrollments(&LocalOsPrincipal::UnixUid(1000)).await.unwrap();

    assert_eq!(own.len(), 1);
    assert_eq!(own[0].os_account_id, "uid:1000");
    assert_eq!(own[0].state, EnrollmentState::Pending);

    let admin = backend.list_enrollments(&LocalOsPrincipal::UnixUid(0)).await.unwrap();

    assert_eq!(admin.len(), 2);
    assert_eq!(backend.list_enrollments(&LocalOsPrincipal::WindowsSid("S-1".into())).await, Err(LocalControlError::PermissionDenied));

    metadata.remove("uid:1000").unwrap();

    assert!(backend.list_enrollments(&LocalOsPrincipal::UnixUid(1000)).await.unwrap().is_empty());
  }

  #[tokio::test]
  async fn policy_read_is_local_and_non_admin_write_does_not_change_it() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let metadata = Arc::new(MetadataStore::open(directory.path()).unwrap());
    let backend = LinuxLocalEnrollment::new(
      metadata,
      Arc::new(AccountLocks::new()),
      test_audit(directory.path()),
      Arc::new(tokio::sync::RwLock::new(())),
    );
    let ordinary = LocalOsPrincipal::UnixUid(1000);

    assert_eq!(backend.get_policy(&ordinary).await, Ok(true));
    assert_eq!(backend.set_policy(&ordinary, false).await, Err(LocalControlError::PermissionDenied));
    assert_eq!(backend.get_policy(&ordinary).await, Ok(true));
    assert_eq!(backend.set_policy(&LocalOsPrincipal::UnixUid(0), false).await, Ok(false));
    assert_eq!(backend.get_policy(&ordinary).await, Ok(false));
  }

  #[tokio::test]
  async fn audit_listing_uses_verified_uid_and_bounded_paging() {
    let directory = tempfile::tempdir().unwrap();
    let audit = test_audit(directory.path());

    for id in ["uid:1000", "uid:1001"] {
      audit
        .append(Record {
          event: "outcome",
          attempt_id: "fixture",
          caller: "paired-device:test",
          os_account_id: Some(id),
          user: None,
          session_selector: None,
          result: Some("ALREADY_UNLOCKED"),
          at_unix_millis: 0,
        })
        .unwrap();
    }

    let metadata = Arc::new(MetadataStore::open(directory.path()).unwrap());
    let backend = LinuxLocalEnrollment::new(metadata, Arc::new(AccountLocks::new()), audit, Arc::new(tokio::sync::RwLock::new(())));
    let own = backend.list_audit(&LocalOsPrincipal::UnixUid(1000), 0, 10).await.unwrap();

    assert_eq!(own.entries.len(), 1);
    assert_eq!(own.entries[0].os_account_id.as_deref(), Some("uid:1000"));

    let admin = backend.list_audit(&LocalOsPrincipal::UnixUid(0), 0, 1).await.unwrap();

    assert_eq!(admin.entries.len(), 1);
    assert!(admin.next_cursor.is_some());

    let second = backend.list_audit(&LocalOsPrincipal::UnixUid(0), admin.next_cursor.unwrap(), 1).await.unwrap();

    assert_eq!(second.entries.len(), 1);
    assert!(second.next_cursor.is_none());
    assert!(matches!(
      backend.list_audit(&LocalOsPrincipal::WindowsSid("S-1".into()), 0, 10).await,
      Err(LocalControlError::PermissionDenied)
    ));
  }
}
