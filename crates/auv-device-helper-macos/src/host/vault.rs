//! The installed Aqua helper is the sole Keychain reader/writer for its UID.

use std::path::Path;

use security_framework::os::macos::keychain::SecKeychain;
use security_framework_sys::base::errSecItemNotFound;
use zeroize::Zeroizing;

use crate::HostError;

const SERVICE: &str = "dev.auv.device-entry.v1";

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
  let account = account(uid);

  match keychain.find_generic_password(SERVICE, &account) {
    Ok((_, mut item)) => item.set_password(credential).map_err(|_| HostError::VaultUnavailable)?,
    Err(error) if error.code() == errSecItemNotFound => {
      keychain.add_generic_password(SERVICE, &account, credential).map_err(|_| HostError::VaultUnavailable)?;
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
  let (secret, _) = keychain.find_generic_password(SERVICE, &account(uid)).map_err(|_| HostError::VaultUnavailable)?;
  Ok(Zeroizing::new(secret.as_ref().to_vec()))
}

pub(super) fn remove(home: &Path, uid: u32) -> Result<(), HostError> {
  let _no_ui = SecKeychain::disable_user_interaction().map_err(|_| HostError::VaultUnavailable)?;
  let keychain = keychain(home)?;

  match keychain.find_generic_password(SERVICE, &account(uid)) {
    Ok((_, item)) => item.delete(),
    Err(error) if error.code() == errSecItemNotFound => return Ok(()),
    Err(_) => return Err(HostError::VaultUnavailable),
  }

  match keychain.find_generic_password(SERVICE, &account(uid)) {
    Err(error) if error.code() == errSecItemNotFound => Ok(()),
    _ => Err(HostError::VaultUnavailable),
  }
}
