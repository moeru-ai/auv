//! GNOME Secret Service storage candidate for the same-UID locked-session host.
//!
//! This module is private and is not used to mark an enrollment READY. A
//! supervised gate must still prove retrieval by the installed host while the
//! selected GNOME session is locked. The D-Bus session uses Secret Service's
//! local `plain` transport; storage at rest belongs to the login collection.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use zbus::zvariant::{Dict, OwnedObjectPath, OwnedValue, Value};
use zeroize::{Zeroize, Zeroizing};

const SERVICE: &str = "org.freedesktop.secrets";
const SERVICE_PATH: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const ATTRIBUTE_APPLICATION: &str = "application";
const ATTRIBUTE_ACCOUNT: &str = "os-account-id";
const APPLICATION_VALUE: &str = "auv-device-entry-v1";
const LABEL: &str = "AUV Device unlock credential";
const DEADLINE: Duration = Duration::from_secs(5);

/// Failures intentionally contain no D-Bus error string or secret-derived text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultError {
  WrongIdentity,
  Unavailable,
  Locked,
  PromptRequired,
  Missing,
  Ambiguous,
  InvalidSecret,
  TimedOut,
}

/// A login-keyring vault reachable only from the intended user-host UID.
///
/// TODO(device-entry-linux-cross-uid): The installed same-UID host retrieved
/// an item while GNOME was locked; administrator management of another UID
/// still needs an authorized path into that account's host. Reopen this only
/// with an owner-approved cross-UID host design.
pub struct GnomeSecretVault {
  uid: u32,
}

impl GnomeSecretVault {
  /// Checks that this process can open the persistent default collection
  /// without invoking a Secret Service prompt.
  pub async fn connect_for_current_user() -> Result<Self, VaultError> {
    let uid = current_euid();
    bounded(Context::open()).await?;
    Ok(Self { uid })
  }

  /// Stores one credential for the current host user. A successful write is
  /// still PENDING until the locked-session retrieval gate passes.
  pub async fn store(&self, account_uid: u32, secret: &[u8]) -> Result<(), VaultError> {
    self.check_identity(account_uid)?;

    if secret.is_empty() || secret.len() > 1024 {
      return Err(VaultError::InvalidSecret);
    }

    bounded(async {
      let context = Context::open().await?;
      let collection = context.collection_proxy().await?;
      ensure_unlocked(&collection).await?;
      let account = account_id(account_uid);
      let attributes = attributes(&account);
      let existing: Vec<OwnedObjectPath> =
        collection.call("SearchItems", &(attributes.clone(),)).await.map_err(|_| VaultError::Unavailable)?;

      if existing.len() > 1 {
        return Err(VaultError::Ambiguous);
      }

      let mut properties = HashMap::new();
      properties.insert("org.freedesktop.Secret.Item.Label", Value::from(LABEL));
      let dict: Dict<'_, '_> = attributes.into();
      properties.insert("org.freedesktop.Secret.Item.Attributes", dict.into());
      let wire_secret = SecretValue::new(context.session.clone(), secret);
      // NOTICE(device-entry-secret-service-prompt): the high-level
      // secret-service crate invokes Prompt automatically for CreateItem.
      // Call the standard D-Bus method directly so a prompt path is rejected
      // without displaying UI. Remove this narrow call if the crate gains a
      // no-prompt API. Contract:
      // `https://specifications.freedesktop.org/secret-service/latest/org.freedesktop.Secret.Collection.html`.
      let (item, prompt): (OwnedObjectPath, OwnedObjectPath) =
        collection.call("CreateItem", &(&properties, &wire_secret, true)).await.map_err(|_| VaultError::Unavailable)?;

      if !no_prompt(&prompt) || item.as_str() == "/" {
        return Err(VaultError::PromptRequired);
      }

      Ok(())
    })
    .await
  }

  /// Retrieves a credential without attempting to unlock a collection or
  /// item. The caller must keep the returned buffer off logs and wire results.
  pub async fn retrieve(&self, account_uid: u32) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    self.check_identity(account_uid)?;
    bounded(async {
      let context = Context::open().await?;
      let item_path = context.find_one(account_uid).await?;
      let item = context.item_proxy(&item_path).await?;
      ensure_unlocked(&item).await?;
      let mut secret: SecretValue = item.call("GetSecret", &(context.session.clone(),)).await.map_err(|_| VaultError::Unavailable)?;

      if secret.content_type != "text/plain" || secret.value.is_empty() {
        return Err(VaultError::InvalidSecret);
      }

      Ok(Zeroizing::new(std::mem::take(&mut secret.value)))
    })
    .await
  }

  /// Deletes the selected item only if the service can do so immediately.
  pub async fn remove(&self, account_uid: u32) -> Result<(), VaultError> {
    self.check_identity(account_uid)?;
    bounded(async {
      let context = Context::open().await?;
      let item_path = context.find_one(account_uid).await?;
      let item = context.item_proxy(&item_path).await?;
      ensure_unlocked(&item).await?;
      let prompt: OwnedObjectPath = item.call("Delete", &()).await.map_err(|_| VaultError::Unavailable)?;

      if !no_prompt(&prompt) {
        return Err(VaultError::PromptRequired);
      }

      Ok(())
    })
    .await
  }

  fn check_identity(&self, account_uid: u32) -> Result<(), VaultError> {
    if self.uid != account_uid || current_euid() != self.uid {
      return Err(VaultError::WrongIdentity);
    }

    Ok(())
  }
}

struct Context {
  connection: zbus::Connection,
  session: OwnedObjectPath,
  collection: OwnedObjectPath,
}

impl Context {
  async fn open() -> Result<Self, VaultError> {
    let connection = zbus::Connection::session().await.map_err(|_| VaultError::Unavailable)?;
    let service = zbus::Proxy::new(&connection, SERVICE, SERVICE_PATH, SERVICE_IFACE).await.map_err(|_| VaultError::Unavailable)?;
    let (_, session): (OwnedValue, OwnedObjectPath) =
      service.call("OpenSession", &("plain", Value::from(""))).await.map_err(|_| VaultError::Unavailable)?;

    let collection: OwnedObjectPath = service.call("ReadAlias", &("default",)).await.map_err(|_| VaultError::Unavailable)?;

    if collection.as_str() == "/" || collection.as_str().ends_with("/session") {
      return Err(VaultError::Unavailable);
    }

    let context = Self {
      connection,
      session,
      collection,
    };
    let collection = context.collection_proxy().await?;
    ensure_unlocked(&collection).await?;
    Ok(context)
  }

  async fn collection_proxy(&self) -> Result<zbus::Proxy<'_>, VaultError> {
    zbus::Proxy::new(&self.connection, SERVICE, self.collection.as_str(), COLLECTION_IFACE).await.map_err(|_| VaultError::Unavailable)
  }

  async fn item_proxy<'a>(&'a self, path: &'a OwnedObjectPath) -> Result<zbus::Proxy<'a>, VaultError> {
    zbus::Proxy::new(&self.connection, SERVICE, path.as_str(), ITEM_IFACE).await.map_err(|_| VaultError::Unavailable)
  }

  async fn find_one(&self, account_uid: u32) -> Result<OwnedObjectPath, VaultError> {
    let collection = self.collection_proxy().await?;
    ensure_unlocked(&collection).await?;
    let account = account_id(account_uid);
    let found: Vec<OwnedObjectPath> = collection.call("SearchItems", &(attributes(&account),)).await.map_err(|_| VaultError::Unavailable)?;

    match found.as_slice() {
      [] => Err(VaultError::Missing),
      [path] => Ok(path.clone()),
      _ => Err(VaultError::Ambiguous),
    }
  }
}

#[derive(serde::Serialize, serde::Deserialize, zbus::zvariant::Type)]
struct SecretValue {
  session: OwnedObjectPath,
  parameters: Vec<u8>,
  value: Vec<u8>,
  content_type: String,
}

impl SecretValue {
  fn new(session: OwnedObjectPath, value: &[u8]) -> Self {
    Self {
      session,
      parameters: Vec::new(),
      value: value.to_vec(),
      content_type: "text/plain".into(),
    }
  }
}

impl Drop for SecretValue {
  fn drop(&mut self) {
    self.value.zeroize();
  }
}

async fn ensure_unlocked(proxy: &zbus::Proxy<'_>) -> Result<(), VaultError> {
  if proxy.get_property::<bool>("Locked").await.map_err(|_| VaultError::Unavailable)? {
    return Err(VaultError::Locked);
  }

  Ok(())
}

fn account_id(uid: u32) -> String {
  format!("uid:{uid}")
}

fn attributes(account: &str) -> HashMap<&str, &str> {
  HashMap::from([
    (ATTRIBUTE_APPLICATION, APPLICATION_VALUE),
    (ATTRIBUTE_ACCOUNT, account),
  ])
}

fn no_prompt(path: &OwnedObjectPath) -> bool {
  path.as_str() == "/"
}

async fn bounded<T>(future: impl Future<Output = Result<T, VaultError>>) -> Result<T, VaultError> {
  tokio::time::timeout(DEADLINE, future).await.map_err(|_| VaultError::TimedOut)?
}

fn current_euid() -> u32 {
  // SAFETY: geteuid has no pointer arguments or preconditions.
  unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn attributes_bind_only_auv_and_stable_uid() {
    let account = account_id(1000);
    let fields = attributes(&account);

    assert_eq!(fields.get(ATTRIBUTE_APPLICATION), Some(&APPLICATION_VALUE));
    assert_eq!(fields.get(ATTRIBUTE_ACCOUNT), Some(&"uid:1000"));
  }

  #[test]
  fn vault_rejects_other_host_identity_before_dbus() {
    let vault = GnomeSecretVault {
      uid: current_euid(),
    };

    assert_eq!(vault.check_identity(current_euid().saturating_add(1)), Err(VaultError::WrongIdentity));
  }

  #[test]
  fn prompt_path_is_never_accepted_as_completion() {
    let no_prompt_path = OwnedObjectPath::try_from("/").unwrap();
    let prompt_path = OwnedObjectPath::try_from("/org/freedesktop/secrets/prompt/p1").unwrap();

    assert!(no_prompt(&no_prompt_path));
    assert!(!no_prompt(&prompt_path));
  }
}
