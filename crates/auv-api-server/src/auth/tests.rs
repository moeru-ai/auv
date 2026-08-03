use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::{PairingError, PairingStore};

#[test]
fn bootstrap_token_is_one_time_and_plaintexts_are_never_persisted() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("pairs.json");
  let store = PairingStore::open(path.clone()).unwrap();
  let token = store.issue_token(None).unwrap().expose_once();
  assert_eq!(token.len(), 32);
  assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
  let persisted = fs::read_to_string(&path).unwrap();
  assert!(!persisted.contains(&token));
  assert_eq!(serde_json::from_str::<serde_json::Value>(&persisted).unwrap()["version"], 1);

  let enrollment = store.consume_token(&token, "tablet".to_string(), "Tablet".to_string()).unwrap();
  let bearer = enrollment.expose_credential_once();
  assert_eq!(bearer.len(), 64);
  assert!(bearer.bytes().all(|byte| byte.is_ascii_hexdigit()));
  assert!(!fs::read_to_string(&path).unwrap().contains(&bearer));
  assert_eq!(store.authenticate_bearer(&bearer).unwrap().as_str(), "paired-device:tablet");
  assert!(matches!(store.consume_token(&token, "other".to_string(), "Other".to_string()), Err(PairingError::InvalidPairingToken)));

  assert!(PairingStore::open(path.clone()).is_err(), "one process owns the pairing store lock");
  store.revoke_device_credentials("tablet").unwrap();
  assert!(matches!(store.authenticate_bearer(&bearer), Err(PairingError::Unauthenticated)));
  drop(store);
  let reopened = PairingStore::open(path).unwrap();
  assert!(matches!(reopened.authenticate_bearer(&bearer), Err(PairingError::Unauthenticated)));
}

#[test]
fn bootstrap_token_expires_only_when_a_ttl_was_requested() {
  let directory = tempfile::tempdir().unwrap();
  let store = PairingStore::open(directory.path().join("pairs.json")).unwrap();
  let issued_at = UNIX_EPOCH + Duration::from_secs(100);
  let expiring = store.issue_token_at(Some(Duration::from_secs(30)), issued_at).unwrap().expose_once();
  assert!(matches!(
    store.consume_token_at(&expiring, "late".to_string(), "Late".to_string(), UNIX_EPOCH + Duration::from_secs(130)),
    Err(PairingError::InvalidPairingToken)
  ));

  let persistent = store.issue_token_at(None, issued_at).unwrap().expose_once();
  store
    .consume_token_at(&persistent, "later".to_string(), "Later".to_string(), UNIX_EPOCH + Duration::from_secs(10_000))
    .expect("token without explicit TTL remains valid");
}

#[test]
fn disable_and_remove_apply_to_the_next_bearer_lookup() {
  let directory = tempfile::tempdir().unwrap();
  let store = PairingStore::open(directory.path().join("pairs.json")).unwrap();
  let token = store.issue_token(None).unwrap().expose_once();
  let bearer = store.consume_token(&token, "tablet".to_string(), "Tablet".to_string()).unwrap().expose_credential_once();

  store.set_enabled("tablet", false).unwrap();
  assert!(matches!(store.authenticate_bearer(&bearer), Err(PairingError::Unauthenticated)));
  store.set_enabled("tablet", true).unwrap();
  assert!(store.authenticate_bearer(&bearer).is_ok());
  store.remove_pair("tablet").unwrap();
  assert!(matches!(store.authenticate_bearer(&bearer), Err(PairingError::Unauthenticated)));
}
