//! Admission and same-session verification for remote Device lock and unlock.
//!
//! DeviceService and DeviceLocalService share this coordinator and its state
//! owner for supported hosts.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use auv::devices::{
  DeviceEntryEffectKind, DeviceEntryErrorReason, EnrollmentState, EnsureUserSessionUnlockedEffect, UserSession, UserSessionLockState,
  UserSessionTarget,
};
use auv_api_server::control::{CallerId, Pairing};

use super::audit::{Audit, Record, result_name};
use super::metadata::MetadataStore;

/// Stable OS account identity travels beside public session facts. A user
/// supplied account name is never substituted for this host observation.
#[derive(Clone)]
pub(super) struct ObservedSession {
  pub public: UserSession,
  pub os_account_id: String,
}

#[derive(Clone)]
pub(super) struct Enrollment {
  pub user: String,
  pub os_account_id: String,
  pub generation: u64,
  pub state: EnrollmentState,
}

/// The platform host owns exact-session observation, lock input, and
/// credential-bound unlock. This port passes no secret into remote policy,
/// a Run, trace, response, or audit record.
pub(super) trait SessionHost: Send + Sync {
  fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason>;
  /// First locked-host check before a Pending enrollment can become Ready.
  /// Call `authorize_effect` immediately before any effectful OS or PAM work,
  /// with no await between that check and the effect.
  fn verify_pending_credential<'a>(
    &'a self,
    selected: &'a ObservedSession,
    authorize_effect: &'a (dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> impl Future<Output = Result<(), DeviceEntryErrorReason>> + Send + 'a;
  /// Platform-specific check for each locked Ready attempt. Linux revalidates
  /// the saved OS password; macOS retrieves and submits it during delivery.
  /// Apply the same immediate admission rule as the Pending check.
  fn verify_ready_credential<'a>(
    &'a self,
    selected: &'a ObservedSession,
    authorize_effect: &'a (dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
  ) -> impl Future<Output = Result<(), DeviceEntryErrorReason>> + Send + 'a;
  /// Returns only after independent same-session OS readback.
  fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason>;
}

pub(super) struct AccountLocks {
  locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl AccountLocks {
  pub(super) fn new() -> Self {
    Self {
      locks: Mutex::new(HashMap::new()),
    }
  }

  pub(super) async fn lock(&self, os_account_id: &str) -> Result<tokio::sync::OwnedMutexGuard<()>, DeviceEntryErrorReason> {
    let account_lock = {
      let mut locks = self.locks.lock().map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
      Arc::clone(locks.entry(os_account_id.to_owned()).or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))))
    };
    Ok(account_lock.lock_owned().await)
  }
}

pub(super) struct Policy<H> {
  host: H,
  store: Arc<MetadataStore>,
  audit: Arc<Audit>,
  account_locks: Arc<AccountLocks>,
  policy_gate: Arc<tokio::sync::RwLock<()>>,
  pairing: Option<Arc<dyn Pairing>>,
}

impl<H: SessionHost> Policy<H> {
  pub(super) fn new(
    host: H,
    store: Arc<MetadataStore>,
    audit: Arc<Audit>,
    account_locks: Arc<AccountLocks>,
    policy_gate: Arc<tokio::sync::RwLock<()>>,
    pairing: Option<Arc<dyn Pairing>>,
  ) -> Self {
    Self {
      host,
      store,
      audit,
      account_locks,
      policy_gate,
      pairing,
    }
  }

  pub(super) fn list(&self) -> Result<Vec<UserSession>, DeviceEntryErrorReason> {
    self.require_enabled()?;
    self.host.sessions().map(|sessions| sessions.into_iter().map(|session| session.public).collect())
  }

  pub(super) fn get(&self, selector: &str) -> Result<UserSession, DeviceEntryErrorReason> {
    self.require_enabled()?;
    self
      .host
      .sessions()?
      .into_iter()
      .find(|session| session.public.selector == selector)
      .map(|session| session.public)
      .ok_or(DeviceEntryErrorReason::StaleSession)
  }

  pub(super) async fn ensure(
    &self,
    caller: &CallerId,
    target: UserSessionTarget,
  ) -> Result<EnsureUserSessionUnlockedEffect, DeviceEntryErrorReason> {
    // Audit admission first. If durable writing fails, no host read or native
    // input is allowed. Pre-resolution denials cannot name an OS account.
    let mut attempt = AuditAttempt::begin(Arc::clone(&self.audit), caller)?;
    // A completed disable must exclude native input from every later request.
    // An accepted request holds the read side through input and its outcome,
    // so SetPolicy(false) cannot finish in the final-check/input gap.
    let _policy_guard = self.policy_gate.read().await;
    let result = self.ensure_inner(caller, &target, &mut attempt.selected).await;
    attempt.finish(&result)?;

    match (attempt.selected.take(), result) {
      (Some(selected), Ok(kind)) => Ok(EnsureUserSessionUnlockedEffect {
        kind,
        user: selected.public.user,
        session_selector: Some(selected.public.selector),
      }),
      (_, Err(reason)) => Err(reason),
      (None, Ok(_)) => Err(DeviceEntryErrorReason::ServiceUnavailable),
    }
  }

  async fn ensure_inner(
    &self,
    caller: &CallerId,
    target: &UserSessionTarget,
    selected_for_audit: &mut Option<ObservedSession>,
  ) -> Result<DeviceEntryEffectKind, DeviceEntryErrorReason> {
    self.reauthorize(caller)?;
    self.require_enabled()?;
    let selected = select(self.host.sessions()?, target)?;
    *selected_for_audit = Some(selected.clone());
    let _account_guard = self.account_locks.lock(&selected.os_account_id).await?;
    // A paired bearer can be revoked while this request waits for its account.
    self.reauthorize(caller)?;
    self.require_enabled()?;
    let selected = self.reobserve(&selected)?;
    let mut enrollment = self.eligible_enrollment(&selected)?;
    self.ensure_selected(caller, &selected, &mut enrollment).await
  }

  async fn ensure_selected(
    &self,
    caller: &CallerId,
    selected: &ObservedSession,
    enrollment: &mut Enrollment,
  ) -> Result<DeviceEntryEffectKind, DeviceEntryErrorReason> {
    match selected.public.lock_state {
      UserSessionLockState::Usable => return Ok(DeviceEntryEffectKind::AlreadyUsable),
      UserSessionLockState::Unknown => return Err(DeviceEntryErrorReason::UnsupportedOsState),
      UserSessionLockState::Locked => {}
    }

    // The installed PAM stack may act on the login keyring. Its check is an
    // admitted host effect, so the host rechecks this bearer immediately
    // before invoking PAM, after asynchronous vault retrieval.
    let authorize_effect = || self.reauthorize(caller);
    let verified = match enrollment.state {
      EnrollmentState::Pending => self.host.verify_pending_credential(selected, &authorize_effect).await,
      EnrollmentState::Ready => self.host.verify_ready_credential(selected, &authorize_effect).await,
      EnrollmentState::Suspended => return Err(DeviceEntryErrorReason::Suspended),
    };

    if matches!(verified, Err(DeviceEntryErrorReason::CredentialRejected)) {
      // An invalid enrolled OS credential must not authorize this or a later
      // request, even when an earlier attempt promoted it to Ready.
      self.store.suspend(&enrollment.os_account_id, enrollment.generation)?;
    }

    verified?;

    if enrollment.state == EnrollmentState::Pending {
      self.recheck(selected, enrollment)?;
      self.store.promote_ready(&enrollment.os_account_id, enrollment.generation)?;
      enrollment.state = EnrollmentState::Ready;
    }

    self.recheck(selected, enrollment)?;
    // NOTICE(pairing-linearization): The host checks the bearer immediately
    // before effectful PAM work. A second exact-credential read here admits
    // the OS unlock; revocation after it affects later attempts, while
    // already admitted input may complete.
    self.reauthorize(caller)?;
    let delivered = self.host.unlock_locked(selected);

    if matches!(delivered, Err(DeviceEntryErrorReason::CredentialRejected)) {
      // A confirmed rejection suspends this generation. An input error or
      // unverified effect alone must not suspend the account.
      self.store.suspend(&enrollment.os_account_id, enrollment.generation)?;
    }

    delivered?;
    let after = self.reobserve(selected)?;

    if after.public.lock_state != UserSessionLockState::Usable {
      return Err(DeviceEntryErrorReason::OutcomeUnverified);
    }

    Ok(DeviceEntryEffectKind::UnlockedExistingSession)
  }

  fn eligible_enrollment(&self, selected: &ObservedSession) -> Result<Enrollment, DeviceEntryErrorReason> {
    let enrollment = self.store.enrollment(&selected.os_account_id)?.ok_or(DeviceEntryErrorReason::Unenrolled)?;

    if enrollment.os_account_id != selected.os_account_id || enrollment.user != selected.public.user || enrollment.generation == 0 {
      return Err(DeviceEntryErrorReason::StaleSession);
    }

    if enrollment.state == EnrollmentState::Suspended {
      return Err(DeviceEntryErrorReason::Suspended);
    }

    if enrollment.state == EnrollmentState::Pending && selected.public.lock_state == UserSessionLockState::Usable {
      // Pending credentials can become Ready only under the locked host.
      return Err(DeviceEntryErrorReason::Unenrolled);
    }

    Ok(enrollment)
  }

  fn recheck(&self, selected: &ObservedSession, enrollment: &Enrollment) -> Result<(), DeviceEntryErrorReason> {
    self.require_enabled()?;
    let now = self.reobserve(selected)?;

    if now.public.lock_state != UserSessionLockState::Locked {
      return Err(DeviceEntryErrorReason::StaleSession);
    }

    let current = self.eligible_enrollment(&now)?;

    if current.generation != enrollment.generation || current.state != enrollment.state {
      return Err(DeviceEntryErrorReason::Unenrolled);
    }

    Ok(())
  }

  fn reobserve(&self, selected: &ObservedSession) -> Result<ObservedSession, DeviceEntryErrorReason> {
    let current = self
      .host
      .sessions()?
      .into_iter()
      .find(|session| session.public.selector == selected.public.selector)
      .ok_or(DeviceEntryErrorReason::StaleSession)?;

    if current.os_account_id != selected.os_account_id || current.public.user != selected.public.user {
      return Err(DeviceEntryErrorReason::StaleSession);
    }

    Ok(current)
  }

  fn require_enabled(&self) -> Result<(), DeviceEntryErrorReason> {
    if self.store.enabled()? {
      Ok(())
    } else {
      Err(DeviceEntryErrorReason::Disabled)
    }
  }

  fn reauthorize(&self, caller: &CallerId) -> Result<(), DeviceEntryErrorReason> {
    if caller == &CallerId::local_owner() {
      return Ok(());
    }

    if self.pairing.as_ref().is_some_and(|pairing| pairing.is_active_caller(caller)) {
      Ok(())
    } else {
      Err(DeviceEntryErrorReason::Unauthorized)
    }
  }
}

/// Owns the interval between a durable admission record and its terminal
/// record. Tokio drops this guard when a queued request future is canceled.
struct AuditAttempt {
  audit: Arc<Audit>,
  id: String,
  caller: String,
  selected: Option<ObservedSession>,
  finished: bool,
}

impl AuditAttempt {
  fn begin(audit: Arc<Audit>, caller: &CallerId) -> Result<Self, DeviceEntryErrorReason> {
    let id = uuid::Uuid::now_v7().to_string();
    let caller = caller.as_str().to_owned();
    audit.append(record("attempt", &id, &caller, None, None))?;
    Ok(Self {
      audit,
      id,
      caller,
      selected: None,
      finished: false,
    })
  }

  fn finish(&mut self, result: &Result<DeviceEntryEffectKind, DeviceEntryErrorReason>) -> Result<(), DeviceEntryErrorReason> {
    // Once native input might have occurred, a failed terminal write must not
    // be replaced by a misleading CANCELED record during Drop.
    self.finished = true;
    let written = self.audit.append(record("outcome", &self.id, &self.caller, self.selected.as_ref(), Some(result_name(result))));

    if written.is_err() {
      self.audit.poison();
    }

    written
  }
}

impl Drop for AuditAttempt {
  fn drop(&mut self) {
    if !self.finished {
      // Cancellation can occur at the policy/account locks or the host's
      // read-only credential check, always before native delivery.
      // The audit append is synchronous and durable; a failure poisons Audit
      // so later attempts fail closed.
      if self.audit.append(record("outcome", &self.id, &self.caller, self.selected.as_ref(), Some("CANCELED"))).is_err() {
        self.audit.poison();
      }
    }
  }
}

fn select(sessions: Vec<ObservedSession>, target: &UserSessionTarget) -> Result<ObservedSession, DeviceEntryErrorReason> {
  let mut matches = sessions.into_iter().filter(|session| match target {
    UserSessionTarget::User(user) => &session.public.user == user,
    UserSessionTarget::SessionSelector(selector) => &session.public.selector == selector,
  });
  let selected = matches.next().ok_or(match target {
    UserSessionTarget::User(_) => DeviceEntryErrorReason::UnsupportedOsState,
    UserSessionTarget::SessionSelector(_) => DeviceEntryErrorReason::StaleSession,
  })?;

  if matches.next().is_some() {
    return Err(DeviceEntryErrorReason::AmbiguousUser);
  }

  Ok(selected)
}

fn record<'a>(
  event: &'static str,
  attempt_id: &'a str,
  caller: &'a str,
  selected: Option<&'a ObservedSession>,
  result: Option<&'static str>,
) -> Record<'a> {
  Record {
    event,
    attempt_id,
    caller,
    os_account_id: selected.map(|selected| selected.os_account_id.as_str()),
    user: selected.map(|selected| selected.public.user.as_str()),
    session_selector: selected.map(|selected| selected.public.selector.as_str()),
    result,
    at_unix_millis: 0,
  }
}

#[cfg(test)]
mod tests {
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv::devices::UserSessionConnectionKind;

  use super::*;

  struct Host {
    sessions: Mutex<Vec<ObservedSession>>,
    observations: AtomicUsize,
    probes: AtomicUsize,
    ready_probes: AtomicUsize,
    probe_error: Mutex<Option<DeviceEntryErrorReason>>,
    deliveries: AtomicUsize,
    delivery_error: Mutex<Option<DeviceEntryErrorReason>>,
  }

  impl Host {
    fn new(sessions: Vec<ObservedSession>) -> Self {
      Self {
        sessions: Mutex::new(sessions),
        observations: AtomicUsize::new(0),
        probes: AtomicUsize::new(0),
        ready_probes: AtomicUsize::new(0),
        probe_error: Mutex::new(None),
        deliveries: AtomicUsize::new(0),
        delivery_error: Mutex::new(None),
      }
    }
  }

  impl SessionHost for Arc<Host> {
    fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
      self.observations.fetch_add(1, Ordering::SeqCst);
      Ok(self.sessions.lock().unwrap().clone())
    }

    async fn verify_pending_credential(
      &self,
      _selected: &ObservedSession,
      authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
    ) -> Result<(), DeviceEntryErrorReason> {
      authorize_effect()?;
      self.probes.fetch_add(1, Ordering::SeqCst);

      match *self.probe_error.lock().unwrap() {
        Some(error) => Err(error),
        None => Ok(()),
      }
    }

    async fn verify_ready_credential(
      &self,
      selected: &ObservedSession,
      authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
    ) -> Result<(), DeviceEntryErrorReason> {
      self.ready_probes.fetch_add(1, Ordering::SeqCst);
      self.verify_pending_credential(selected, authorize_effect).await
    }

    fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
      self.deliveries.fetch_add(1, Ordering::SeqCst);

      if let Some(error) = *self.delivery_error.lock().unwrap() {
        return Err(error);
      }

      let mut sessions = self.sessions.lock().unwrap();
      let current = sessions.iter_mut().find(|current| current.public.selector == selected.public.selector).unwrap();
      current.public.lock_state = UserSessionLockState::Usable;
      Ok(())
    }
  }

  fn session(selector: &str, state: UserSessionLockState) -> ObservedSession {
    ObservedSession {
      public: UserSession {
        selector: selector.to_owned(),
        user: "neko".into(),
        lock_state: state,
        connection_kind: UserSessionConnectionKind::Physical,
        seat: Some("seat0".into()),
      },
      os_account_id: "uid:1000".into(),
    }
  }

  fn fixture<H: SessionHost>(host: H) -> (tempfile::TempDir, Policy<H>) {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let store = Arc::new(MetadataStore::open(root.path()).unwrap());
    let audit = Arc::new(Audit::open(root.path()).unwrap());
    (root, Policy::new(host, store, audit, Arc::new(AccountLocks::new()), Arc::new(tokio::sync::RwLock::new(())), None))
  }

  fn caller() -> CallerId {
    CallerId::local_owner()
  }

  fn paired_policy(host: Arc<Host>) -> (tempfile::TempDir, Arc<Policy<Arc<Host>>>, super::super::super::pairing::PairingStore, CallerId) {
    use super::super::super::pairing::PairingStore;

    let (root, mut policy) = fixture(host);
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let enrollment = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", enrollment.generation).unwrap();
    let pairing = PairingStore::open(root.path().join("pairing.json")).unwrap();
    let token = pairing.issue_token(None).unwrap().expose_once();
    let enrolled = pairing.consume_token(&token, "paired-a".into(), "Paired A".into()).unwrap();
    let caller = pairing.authenticate_bearer(&enrolled.expose_credential_once()).unwrap();
    policy.pairing = Some(Arc::new(pairing.clone()));
    (root, Arc::new(policy), pairing, caller)
  }

  // ROOT CAUSE:
  //
  // If a queued request was canceled after its attempt append, the ordinary
  // outcome path never ran. The request guard now closes that audit interval
  // before the account lock can be released or native input delivered.
  #[tokio::test]
  async fn canceled_request_waiting_for_account_lock_has_terminal_audit_outcome() {
    use auv_api_server::device_local::LocalOsPrincipal;

    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    let policy = Arc::new(policy);
    let account_guard = policy.account_locks.lock("uid:1000").await.unwrap();
    let request = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      while host.observations.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();

    request.abort();

    assert!(request.await.unwrap_err().is_cancelled());

    drop(account_guard);

    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);

    let entries = policy.audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), 0, 10).unwrap().entries;

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].event, "attempt");
    assert_eq!(entries[1].event, "outcome");
    assert_eq!(entries[0].attempt_id, entries[1].attempt_id);
    assert_eq!(entries[1].result.as_deref(), Some("CANCELED"));
    assert_eq!(entries[1].os_account_id.as_deref(), Some("uid:1000"));
    assert_eq!(entries[1].session_selector.as_deref(), Some("s"));
  }

  #[tokio::test]
  async fn canceled_request_waiting_for_policy_gate_has_terminal_audit_outcome() {
    use auv_api_server::device_local::LocalOsPrincipal;

    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    let policy = Arc::new(policy);
    let policy_guard = policy.policy_gate.write().await;
    let request = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      while policy.audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), 0, 10).unwrap().entries.is_empty() {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();

    request.abort();

    assert!(request.await.unwrap_err().is_cancelled());

    drop(policy_guard);

    assert_eq!(host.observations.load(Ordering::SeqCst), 0);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);

    let entries = policy.audit.read_for_principal(&LocalOsPrincipal::UnixUid(0), 0, 10).unwrap().entries;

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].event, "attempt");
    assert_eq!(entries[1].event, "outcome");
    assert_eq!(entries[0].attempt_id, entries[1].attempt_id);
    assert_eq!(entries[1].result.as_deref(), Some("CANCELED"));
    assert!(entries[1].os_account_id.is_none());
    assert!(entries[1].session_selector.is_none());
  }

  #[tokio::test]
  async fn revoked_bearer_waiting_for_account_lock_never_reaches_native_input() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (root, policy, pairing, caller) = paired_policy(Arc::clone(&host));
    let account_guard = policy.account_locks.lock("uid:1000").await.unwrap();
    let attempt = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller, UserSessionTarget::User("neko".into())).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      while host.observations.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    pairing.revoke_device_credentials("paired-a").unwrap();
    drop(account_guard);

    assert_eq!(attempt.await.unwrap().unwrap_err(), DeviceEntryErrorReason::Unauthorized);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);

    let audit = std::fs::read_to_string(root.path().join("device-entry-audit.jsonl")).unwrap();

    assert!(audit.contains("UNAUTHORIZED"));
    assert!(!audit.contains("credential_sha256"));
  }

  // ROOT CAUSE:
  //
  // A Linux vault read can await after the initial bearer check. If the
  // bearer is revoked during that read, PAM must not run its effectful modules
  // before reauthorization at the native admission point.
  #[tokio::test]
  async fn revoked_bearer_during_credential_read_never_reaches_pam_effect() {
    use super::super::super::pairing::PairingStore;

    struct AdmissionHost {
      inner: Arc<Host>,
      entered: Arc<tokio::sync::Notify>,
      release: Arc<tokio::sync::Notify>,
      effects: Arc<AtomicUsize>,
    }

    impl SessionHost for AdmissionHost {
      fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
        self.inner.sessions()
      }

      async fn verify_pending_credential(
        &self,
        selected: &ObservedSession,
        authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.verify_ready_credential(selected, authorize_effect).await
      }

      async fn verify_ready_credential(
        &self,
        _selected: &ObservedSession,
        authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.entered.notify_one();
        self.release.notified().await;
        authorize_effect()?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(())
      }

      fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
        self.inner.unlock_locked(selected)
      }
    }

    let inner = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let effects = Arc::new(AtomicUsize::new(0));
    let (root, mut policy) = fixture(AdmissionHost {
      inner: Arc::clone(&inner),
      entered: Arc::clone(&entered),
      release: Arc::clone(&release),
      effects: Arc::clone(&effects),
    });
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    let pairing = PairingStore::open(root.path().join("pairing.json")).unwrap();
    let token = pairing.issue_token(None).unwrap().expose_once();
    let enrolled = pairing.consume_token(&token, "paired-a".into(), "Paired A".into()).unwrap();
    let caller = pairing.authenticate_bearer(&enrolled.expose_credential_once()).unwrap();
    policy.pairing = Some(Arc::new(pairing.clone()));
    let policy = Arc::new(policy);
    let attempt = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller, UserSessionTarget::User("neko".into())).await }
    });
    entered.notified().await;
    pairing.revoke_device_credentials("paired-a").unwrap();
    release.notify_one();

    assert_eq!(attempt.await.unwrap().unwrap_err(), DeviceEntryErrorReason::Unauthorized);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(inner.deliveries.load(Ordering::SeqCst), 0);

    let audit = std::fs::read_to_string(root.path().join("device-entry-audit.jsonl")).unwrap();

    assert!(audit.contains("UNAUTHORIZED"));
  }

  #[tokio::test]
  async fn old_bearer_stays_unauthorized_if_pair_id_is_reused_while_queued() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy, pairing, caller) = paired_policy(Arc::clone(&host));
    let account_guard = policy.account_locks.lock("uid:1000").await.unwrap();
    let attempt = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller, UserSessionTarget::User("neko".into())).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
      while host.observations.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();
    pairing.remove_pair("paired-a").unwrap();
    let token = pairing.issue_token(None).unwrap().expose_once();
    let enrolled = pairing.consume_token(&token, "paired-a".into(), "Paired A again".into()).unwrap();
    let new_caller = pairing.authenticate_bearer(&enrolled.expose_credential_once()).unwrap();

    assert!(pairing.is_active_caller(&new_caller));

    drop(account_guard);

    assert_eq!(attempt.await.unwrap().unwrap_err(), DeviceEntryErrorReason::Unauthorized);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn pending_locked_session_promotes_only_after_host_probe_and_verified_readback() {
    let host = Arc::new(Host::new(vec![session(
      "linux-logind:52:1000:42",
      UserSessionLockState::Locked,
    )]));
    let (root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    policy.store.publish_pending("neko", "uid:1000").unwrap();
    let result = policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap();

    assert_eq!(result.kind, DeviceEntryEffectKind::UnlockedExistingSession);
    assert_eq!(host.probes.load(Ordering::SeqCst), 1);
    assert_eq!(host.ready_probes.load(Ordering::SeqCst), 0);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 1);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Ready);

    let audit = std::fs::read_to_string(root.path().join("device-entry-audit.jsonl")).unwrap();

    assert_eq!(audit.lines().count(), 2);
    assert!(audit.contains("UNLOCKED_EXISTING_SESSION"));
    assert!(!audit.contains("credential"));
  }

  // ROOT CAUSE:
  //
  // A Ready Linux enrollment previously skipped host PAM revalidation, so a
  // rotated OS password could continue authorizing remote unlock. Each locked
  // request must run its host's Ready check before native delivery.
  #[tokio::test]
  async fn ready_locked_session_verifies_credential_on_every_attempt() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();

    let first = policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap();

    assert_eq!(first.kind, DeviceEntryEffectKind::UnlockedExistingSession);

    host.sessions.lock().unwrap()[0].public.lock_state = UserSessionLockState::Locked;
    let second = policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap();

    assert_eq!(second.kind, DeviceEntryEffectKind::UnlockedExistingSession);
    assert_eq!(host.probes.load(Ordering::SeqCst), 2);
    assert_eq!(host.ready_probes.load(Ordering::SeqCst), 2);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 2);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Ready);
  }

  // ROOT CAUSE:
  //
  // A Ready credential can become invalid after an OS password rotation.
  // Rejecting it before native delivery must suspend this generation so a
  // later request cannot bypass the failed check.
  #[tokio::test]
  async fn ready_preflight_credential_rejection_suspends_without_unlock() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    *host.probe_error.lock().unwrap() = Some(DeviceEntryErrorReason::CredentialRejected);

    assert_eq!(
      policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap_err(),
      DeviceEntryErrorReason::CredentialRejected
    );
    assert_eq!(host.probes.load(Ordering::SeqCst), 1);
    assert_eq!(host.ready_probes.load(Ordering::SeqCst), 1);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Suspended);
    assert_eq!(policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap_err(), DeviceEntryErrorReason::Suspended);
    assert_eq!(host.probes.load(Ordering::SeqCst), 1);
    assert_eq!(host.ready_probes.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn pending_preflight_credential_rejection_suspends_without_promotion_or_unlock() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    policy.store.publish_pending("neko", "uid:1000").unwrap();
    *host.probe_error.lock().unwrap() = Some(DeviceEntryErrorReason::CredentialRejected);

    assert_eq!(
      policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap_err(),
      DeviceEntryErrorReason::CredentialRejected
    );
    assert_eq!(host.probes.load(Ordering::SeqCst), 1);
    assert_eq!(host.ready_probes.load(Ordering::SeqCst), 0);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Suspended);
  }

  #[tokio::test]
  async fn transient_ready_preflight_error_preserves_enrollment_without_unlock() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    *host.probe_error.lock().unwrap() = Some(DeviceEntryErrorReason::ServiceUnavailable);

    assert_eq!(
      policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await.unwrap_err(),
      DeviceEntryErrorReason::ServiceUnavailable
    );
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Ready);
  }

  // ROOT CAUSE:
  //
  // A Pending credential probe now awaits Secret Service. If its request is
  // canceled during that read, native input must not run and the audit needs
  // one terminal CANCELED record while enrollment stays Pending.
  #[tokio::test]
  async fn canceling_async_pending_probe_preserves_pending_and_records_canceled() {
    struct SuspendedProbe {
      inner: Arc<Host>,
      entered: Arc<tokio::sync::Notify>,
    }

    impl SessionHost for SuspendedProbe {
      fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
        self.inner.sessions()
      }

      async fn verify_pending_credential(
        &self,
        _selected: &ObservedSession,
        _authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.entered.notify_one();
        std::future::pending().await
      }

      async fn verify_ready_credential(
        &self,
        _selected: &ObservedSession,
        _authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.entered.notify_one();
        std::future::pending().await
      }

      fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
        self.inner.unlock_locked(selected)
      }
    }

    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let entered = Arc::new(tokio::sync::Notify::new());
    let (root, policy) = fixture(SuspendedProbe {
      inner: Arc::clone(&host),
      entered: Arc::clone(&entered),
    });
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    policy.store.publish_pending("neko", "uid:1000").unwrap();
    let policy = Arc::new(policy);
    let attempt = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await }
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified()).await.unwrap();
    attempt.abort();

    assert!(attempt.await.unwrap_err().is_cancelled());
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Pending);

    let audit = std::fs::read_to_string(root.path().join("device-entry-audit.jsonl")).unwrap();

    assert_eq!(audit.lines().count(), 2);
    assert!(audit.contains("CANCELED"));
  }

  #[tokio::test]
  async fn disabled_switch_denies_inventory_and_unlock_before_host_access() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.set_enabled(false).unwrap();

    assert!(matches!(policy.list(), Err(DeviceEntryErrorReason::Disabled)));
    assert!(matches!(policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await, Err(DeviceEntryErrorReason::Disabled)));
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
  }

  #[cfg(target_os = "macos")]
  // ROOT CAUSE:
  //
  // If an administrator disabled remote unlock after its final enabled read,
  // SetPolicy(false) could return while accepted native input was still in flight.
  // The shared gate orders that input before the durable disable response.
  #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
  async fn disabling_waits_for_accepted_native_input_and_denies_later_unlock() {
    use std::sync::mpsc;
    use std::time::Duration;

    use auv_api_server::device_local::{DeviceLocalControl, LocalOsPrincipal};

    use super::super::enrollment_macos::MacosLocalEnrollment;

    struct BlockingHost {
      inner: Arc<Host>,
      entered: Mutex<mpsc::Sender<()>>,
      release: Mutex<mpsc::Receiver<()>>,
    }

    impl SessionHost for BlockingHost {
      fn sessions(&self) -> Result<Vec<ObservedSession>, DeviceEntryErrorReason> {
        self.inner.sessions()
      }

      async fn verify_pending_credential(
        &self,
        selected: &ObservedSession,
        authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.inner.verify_pending_credential(selected, authorize_effect).await
      }

      async fn verify_ready_credential(
        &self,
        selected: &ObservedSession,
        authorize_effect: &(dyn Fn() -> Result<(), DeviceEntryErrorReason> + Send + Sync),
      ) -> Result<(), DeviceEntryErrorReason> {
        self.inner.verify_ready_credential(selected, authorize_effect).await
      }

      fn unlock_locked(&self, selected: &ObservedSession) -> Result<(), DeviceEntryErrorReason> {
        self.entered.lock().unwrap().send(()).unwrap();
        self.release.lock().unwrap().recv_timeout(Duration::from_secs(5)).map_err(|_| DeviceEntryErrorReason::ServiceUnavailable)?;
        self.inner.unlock_locked(selected)
      }
    }

    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (_root, policy) = fixture(BlockingHost {
      inner: Arc::clone(&host),
      entered: Mutex::new(entered_tx),
      release: Mutex::new(release_rx),
    });
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    let local = MacosLocalEnrollment::new(
      Arc::clone(&policy.store),
      Arc::clone(&policy.account_locks),
      Arc::clone(&policy.audit),
      Arc::clone(&policy.policy_gate),
    );
    let policy = Arc::new(policy);
    let unlock = tokio::spawn({
      let policy = Arc::clone(&policy);
      async move { policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await }
    });
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)).unwrap()).await.unwrap();

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let disable = tokio::spawn(async move {
      started_tx.send(()).unwrap();
      local.set_policy(&LocalOsPrincipal::UnixUid(0), false).await
    });
    started_rx.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
      // Tokio's fair lock stops admitting new readers after the writer queues.
      while policy.policy_gate.try_read().is_ok() {
        tokio::task::yield_now().await;
      }
    })
    .await
    .unwrap();

    assert!(!disable.is_finished());

    release_tx.send(()).unwrap();

    assert_eq!(unlock.await.unwrap().unwrap().kind, DeviceEntryEffectKind::UnlockedExistingSession);
    assert_eq!(disable.await.unwrap().unwrap(), false);
    assert!(matches!(policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await, Err(DeviceEntryErrorReason::Disabled)));
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn usable_ready_session_is_noop_without_vault_probe_or_input() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Usable)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    let result = policy.ensure(&caller(), UserSessionTarget::SessionSelector("s".into())).await.unwrap();

    assert_eq!(result.kind, DeviceEntryEffectKind::AlreadyUsable);
    assert_eq!(host.probes.load(Ordering::SeqCst), 0);
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn ambiguity_and_unenrolled_account_never_deliver() {
    let host = Arc::new(Host::new(vec![
      session("s1", UserSessionLockState::Locked),
      session("s2", UserSessionLockState::Locked),
    ]));
    let (root, policy) = fixture(Arc::clone(&host));

    assert!(matches!(policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await, Err(DeviceEntryErrorReason::AmbiguousUser)));
    assert!(matches!(
      policy.ensure(&caller(), UserSessionTarget::SessionSelector("s1".into())).await,
      Err(DeviceEntryErrorReason::Unenrolled)
    ));

    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);

    let audit = std::fs::read_to_string(root.path().join("device-entry-audit.jsonl")).unwrap();

    assert_eq!(audit.lines().count(), 4);
    assert!(audit.contains("AMBIGUOUS_USER"));
    assert!(audit.contains("UNENROLLED"));
  }

  #[tokio::test]
  async fn confirmed_rejection_suspends_only_that_generation() {
    let host = Arc::new(Host::new(vec![session("s", UserSessionLockState::Locked)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();
    *host.delivery_error.lock().unwrap() = Some(DeviceEntryErrorReason::CredentialRejected);
    assert!(matches!(
      policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await,
      Err(DeviceEntryErrorReason::CredentialRejected)
    ));

    assert_eq!(policy.store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Suspended);
    assert!(matches!(policy.ensure(&caller(), UserSessionTarget::User("neko".into())).await, Err(DeviceEntryErrorReason::Suspended)));
    assert_eq!(host.deliveries.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn stale_selector_and_unknown_lock_state_never_deliver() {
    let host = Arc::new(Host::new(vec![session("current", UserSessionLockState::Unknown)]));
    let (_root, policy) = fixture(Arc::clone(&host));
    policy.store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let record = policy.store.publish_pending("neko", "uid:1000").unwrap();
    policy.store.promote_ready("uid:1000", record.generation).unwrap();

    assert!(matches!(
      policy.ensure(&caller(), UserSessionTarget::SessionSelector("previous".into())).await,
      Err(DeviceEntryErrorReason::StaleSession)
    ));

    assert!(matches!(
      policy.ensure(&caller(), UserSessionTarget::SessionSelector("current".into())).await,
      Err(DeviceEntryErrorReason::UnsupportedOsState)
    ));

    assert_eq!(host.deliveries.load(Ordering::SeqCst), 0);
  }

  #[test]
  fn removal_preserves_generation_tombstone_across_restart() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let store = MetadataStore::open(root.path()).unwrap();
    store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let old = store.publish_pending("neko", "uid:1000").unwrap();
    store.remove("uid:1000").unwrap();

    assert!(store.enrollment("uid:1000").unwrap().is_none());

    drop(store);
    let reopened = MetadataStore::open(root.path()).unwrap();
    reopened.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let new = reopened.publish_pending("neko", "uid:1000").unwrap();

    assert!(new.generation > old.generation);
  }

  #[test]
  fn enrollment_update_invalidates_ready_before_vault_overwrite() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let store = MetadataStore::open(root.path()).unwrap();
    store.invalidate_for_enroll("neko", "uid:1000").unwrap();
    let old = store.publish_pending("neko", "uid:1000").unwrap();
    store.promote_ready("uid:1000", old.generation).unwrap();

    // This durable write precedes the vault overwrite. If that later write
    // fails, an old READY enrollment cannot authorize remote input.
    store.invalidate_for_enroll("neko", "uid:1000").unwrap();

    assert_eq!(store.enrollment("uid:1000").unwrap().unwrap().state, EnrollmentState::Suspended);

    drop(store);
    let reopened = MetadataStore::open(root.path()).unwrap();
    let suspended = reopened.enrollment("uid:1000").unwrap().unwrap();

    assert_eq!(suspended.state, EnrollmentState::Suspended);
    assert!(suspended.generation > old.generation);
  }

  #[test]
  fn second_metadata_owner_cannot_open_same_root() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let first = MetadataStore::open(root.path()).unwrap();

    assert!(matches!(MetadataStore::open(root.path()), Err(DeviceEntryErrorReason::ServiceUnavailable)));

    drop(first);
    MetadataStore::open(root.path()).unwrap();
  }
}
