//! The installed Aqua helper is the sole Keychain reader/writer for its UID.

use std::path::Path;

use security_framework::os::macos::keychain::SecKeychain;
use security_framework_sys::base::errSecItemNotFound;
use zeroize::Zeroizing;

use crate::HostError;

// NOTICE(device-entry-helper-identity-v1): The 2026-10-01 helper identity
// migration intentionally requires fresh enrollment; the experimental host
// has no released credential namespace that needs read compatibility.
const OFFICIAL_SERVICE: &str = "ai.moeru.auv.device-entry.v1";

/// Keychain service owned by the running helper identity.
fn service() -> Result<String, HostError> {
  let bundle_id = crate::service_management::bundle_identifier().ok_or(HostError::VaultUnavailable)?;
  Ok(service_for(&bundle_id))
}

/// Every helper identity keeps its own item: a Keychain ACL trusts the app
/// that created the item, so another helper for the same user would prompt
/// instead of reading it. The official helper keeps its released service.
fn service_for(bundle_id: &str) -> String {
  if bundle_id == crate::OFFICIAL_BUNDLE_ID {
    OFFICIAL_SERVICE.to_string()
  } else {
    format!("{bundle_id}.device-entry.v1")
  }
}

fn keychain(home: &Path) -> Result<SecKeychain, HostError> {
  // NOTICE: The first release targets a logged-in, then locked session. The
  // explicit login.keychain-db is accessed by this same signed Aqua helper.
  // Its locked-state readability and item ACL remain an installed-host gate.
  SecKeychain::open(home.join("Library").join("Keychains").join("login.keychain-db")).map_err(|_| HostError::VaultUnavailable)
}

fn account(uid: u32) -> String {
  format!("uid:{uid}")
}

pub(super) fn enroll(home: &Path, uid: u32, credential: &[u8]) -> Result<(), HostError> {
  let keychain = keychain(home)?;
  let service = service()?;
  let account = account(uid);

  match keychain.find_generic_password(&service, &account) {
    Ok((_, mut item)) => item.set_password(credential).map_err(|_| HostError::VaultUnavailable)?,
    Err(error) if error.code() == errSecItemNotFound => {
      keychain.add_generic_password(&service, &account, credential).map_err(|_| HostError::VaultUnavailable)?;
    }
    Err(_) => return Err(HostError::VaultUnavailable),
  }

  let readback = read(home, uid)?;

  if readback.as_slice() != credential {
    return Err(HostError::VaultUnavailable);
  }

  Ok(())
}

pub(super) fn read(home: &Path, uid: u32) -> Result<Zeroizing<Vec<u8>>, HostError> {
  // Never allow a hidden Keychain authorization dialog to turn a remote
  // unlock request into an interactive, unattended prompt.
  let _no_ui = SecKeychain::disable_user_interaction().map_err(|_| HostError::VaultUnavailable)?;
  let keychain = keychain(home)?;
  let (secret, _) = keychain.find_generic_password(&service()?, &account(uid)).map_err(|_| HostError::VaultUnavailable)?;
  Ok(Zeroizing::new(secret.as_ref().to_vec()))
}

pub(super) fn remove(home: &Path, uid: u32) -> Result<(), HostError> {
  let _no_ui = SecKeychain::disable_user_interaction().map_err(|_| HostError::VaultUnavailable)?;
  let keychain = keychain(home)?;
  let service = service()?;

  match keychain.find_generic_password(&service, &account(uid)) {
    Ok((_, item)) => item.delete(),
    Err(error) if error.code() == errSecItemNotFound => return Ok(()),
    Err(_) => return Err(HostError::VaultUnavailable),
  }

  match keychain.find_generic_password(&service, &account(uid)) {
    Err(error) if error.code() == errSecItemNotFound => Ok(()),
    _ => Err(HostError::VaultUnavailable),
  }
}

#[cfg(test)]
mod tests {
  #[test]
  fn each_helper_identity_owns_a_separate_keychain_service() {
    assert_eq!(super::service_for(crate::OFFICIAL_BUNDLE_ID), "ai.moeru.auv.device-entry.v1");
    assert_eq!(super::service_for("com.example.computer-use.helper"), "com.example.computer-use.helper.device-entry.v1");
  }
}
