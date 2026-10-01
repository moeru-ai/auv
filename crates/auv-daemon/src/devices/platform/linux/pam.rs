//! Password-only, target-local GDM PAM check for GNOME Device entry.
//!
//! This calls the installed `gdm-password` policy, not a bundled password
//! verifier. PAM messages and errors are never copied into AUV output.
//! NOTICE(device-entry-pam-side-effects): Installed PAM modules can act while
//! checking the password. The supervised GNOME gate logged `gkr-pam: unlocked
//! login keyring`; GNOME Keyring's PAM module performs that operation. A
//! dedicated service would require an owner-approved installation contract.
//! See `https://github.com/GNOME/gnome-keyring/blob/main/pam/gkr-pam-module.c`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use libloading::Library;
use zeroize::Zeroizing;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_AUTH_ERR: c_int = 7;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_MAXTRIES: c_int = 11;
const PAM_NEW_AUTHTOK_REQD: c_int = 12;
const PAM_ACCT_EXPIRED: c_int = 13;
const PAM_CONV_ERR: c_int = 19;
const PAM_AUTHTOK_EXPIRED: c_int = 27;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_USER: c_int = 2;
const PAM_SILENT: c_int = 0x8000;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 1;
const PAM_MAX_NUM_MSG: c_int = 32;
const PAM_MAX_RESP_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerifyError {
  Rejected,
  Unavailable,
}

#[repr(C)]
struct PamMessage {
  style: c_int,
  text: *const c_char,
}

#[repr(C)]
struct PamResponse {
  text: *mut c_char,
  code: c_int,
}

#[repr(C)]
struct PamConversation {
  callback: Option<unsafe extern "C" fn(c_int, *const *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int>,
  context: *mut c_void,
}

struct ConversationContext {
  credential: Zeroizing<Vec<u8>>,
  prompted: bool,
}

type Start = unsafe extern "C" fn(*const c_char, *const c_char, *const PamConversation, *mut *mut c_void) -> c_int;
type Authenticate = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type Account = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type GetItem = unsafe extern "C" fn(*const c_void, c_int, *mut *const c_void) -> c_int;
type End = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

struct PamApi {
  _library: Library,
  start: Start,
  authenticate: Authenticate,
  account: Account,
  get_item: GetItem,
  end: End,
}

impl PamApi {
  fn load() -> Result<Self, VerifyError> {
    // NOTICE(linux-pam-runtime-load): Linking libpam requires development
    // files that are absent on other Linux build hosts. Load the installed
    // runtime library and fail enrollment closed if it is missing. Replace
    // this when every supported build host supplies the link-time PAM files.
    // SAFETY: This library is retained in the returned struct for as long as
    // any copied function pointer can be called.
    let library = unsafe { Library::new("libpam.so.0") }.map_err(|_| VerifyError::Unavailable)?;
    // SAFETY: These symbol names and signatures match Linux-PAM pam_appl.h;
    // pointers are copied while the library remains loaded in this struct.
    let start = unsafe { *library.get::<Start>(b"pam_start\0").map_err(|_| VerifyError::Unavailable)? };
    // SAFETY: Same Linux-PAM ABI and library lifetime as `pam_start` above.
    let authenticate = unsafe { *library.get::<Authenticate>(b"pam_authenticate\0").map_err(|_| VerifyError::Unavailable)? };
    // SAFETY: Same Linux-PAM ABI and library lifetime as `pam_start` above.
    let account = unsafe { *library.get::<Account>(b"pam_acct_mgmt\0").map_err(|_| VerifyError::Unavailable)? };
    // SAFETY: Same Linux-PAM ABI and library lifetime as `pam_start` above.
    let get_item = unsafe { *library.get::<GetItem>(b"pam_get_item\0").map_err(|_| VerifyError::Unavailable)? };
    // SAFETY: Same Linux-PAM ABI and library lifetime as `pam_start` above.
    let end = unsafe { *library.get::<End>(b"pam_end\0").map_err(|_| VerifyError::Unavailable)? };
    Ok(Self {
      _library: library,
      start,
      authenticate,
      account,
      get_item,
      end,
    })
  }
}

/// Verify that the installed GDM password service accepts this credential for
/// exactly the selected account. PAM must request the password once; a
/// passwordless or different-factor success does not satisfy enrollment.
pub(super) fn verify_password(user: &str, credential: &[u8]) -> Result<(), VerifyError> {
  if credential.is_empty() || credential.len() >= PAM_MAX_RESP_SIZE {
    return Err(VerifyError::Rejected);
  }

  let user = CString::new(user).map_err(|_| VerifyError::Rejected)?;
  let credential = Zeroizing::new(CString::new(credential).map_err(|_| VerifyError::Rejected)?.into_bytes_with_nul());

  if !Path::new("/etc/pam.d/gdm-password").is_file() {
    return Err(VerifyError::Unavailable);
  }

  let api = PamApi::load()?;
  let mut context = ConversationContext {
    credential,
    prompted: false,
  };
  let conversation = PamConversation {
    callback: Some(converse),
    context: (&mut context as *mut ConversationContext).cast(),
  };
  let mut handle = ptr::null_mut();
  // SAFETY: All C strings and the conversation context remain live through
  // pam_end. On success PAM initializes `handle` for this transaction.
  let start_status = unsafe { (api.start)(c"gdm-password".as_ptr(), user.as_ptr(), &conversation, &mut handle) };

  if start_status != PAM_SUCCESS || handle.is_null() {
    return Err(VerifyError::Unavailable);
  }

  let result = (|| {
    // SAFETY: `handle` came from successful pam_start and remains live.
    let auth_status = unsafe { (api.authenticate)(handle, PAM_SILENT | PAM_DISALLOW_NULL_AUTHTOK) };
    authentication_result(auth_status, context.prompted)?;
    let mut item = ptr::null();
    // SAFETY: The returned PAM_USER pointer is borrowed only while handle is
    // live; no PAM method is called before it has been copied and compared.
    let item_status = unsafe { (api.get_item)(handle, PAM_USER, &mut item) };

    if item_status != PAM_SUCCESS || item.is_null() {
      return Err(VerifyError::Unavailable);
    }

    // SAFETY: Successful pam_get_item(PAM_USER) returns a NUL-terminated
    // string owned by this live PAM transaction.
    let same_user = unsafe { CStr::from_ptr(item.cast()) } == user.as_c_str();

    if !same_user {
      return Err(VerifyError::Unavailable);
    }

    // SAFETY: The same live PAM handle is valid for account management.
    let account_status = unsafe { (api.account)(handle, PAM_SILENT) };

    if account_status == PAM_SUCCESS {
      Ok(())
    } else {
      Err(classify_status(account_status))
    }
  })();
  // SAFETY: The successful pam_start transaction must end once, after all
  // borrowed PAM data and callbacks are no longer in use.
  let end_status = unsafe {
    (api.end)(
      handle,
      if result.is_ok() {
        PAM_SUCCESS
      } else {
        PAM_AUTH_ERR
      },
    )
  };

  if end_status != PAM_SUCCESS {
    return Err(VerifyError::Unavailable);
  }

  result
}

fn classify_status(status: c_int) -> VerifyError {
  match status {
    PAM_AUTH_ERR | PAM_USER_UNKNOWN | PAM_MAXTRIES | PAM_NEW_AUTHTOK_REQD | PAM_ACCT_EXPIRED | PAM_AUTHTOK_EXPIRED => VerifyError::Rejected,
    _ => VerifyError::Unavailable,
  }
}

fn authentication_result(status: c_int, prompted: bool) -> Result<(), VerifyError> {
  if !prompted {
    return Err(VerifyError::Unavailable);
  }

  if status == PAM_SUCCESS {
    Ok(())
  } else {
    Err(classify_status(status))
  }
}

/// The only supplied value is the locally entered password for a single
/// hidden PAM prompt. No prompt or module text is read or logged.
unsafe extern "C" fn converse(
  count: c_int,
  messages: *const *const PamMessage,
  output: *mut *mut PamResponse,
  context: *mut c_void,
) -> c_int {
  if count <= 0 || count > PAM_MAX_NUM_MSG || messages.is_null() || output.is_null() || context.is_null() {
    return PAM_CONV_ERR;
  }

  // SAFETY: PAM supplies `count` message pointers and a live appdata context
  // for the synchronous callback. The bounded array is read before return.
  let messages = unsafe { std::slice::from_raw_parts(messages, count as usize) };
  // SAFETY: The appdata pointer was created from this live context in
  // verify_password and PAM invokes the callback synchronously.
  let context = unsafe { &mut *context.cast::<ConversationContext>() };
  // SAFETY: PAM frees the returned response array with libc free; calloc
  // creates a zeroed C-compatible array of the exact requested length.
  let responses = unsafe { libc::calloc(count as usize, std::mem::size_of::<PamResponse>()) }.cast::<PamResponse>();

  if responses.is_null() {
    return PAM_BUF_ERR;
  }

  let mut accepted = true;

  for (index, message) in messages.iter().enumerate() {
    if message.is_null() {
      accepted = false;
      break;
    }

    // SAFETY: PAM's documented callback contract supplies live pam_message
    // pointers. We read the style only, never the potentially sensitive text.
    let style = unsafe { (**message).style };

    match style {
      PAM_PROMPT_ECHO_OFF if !context.prompted => {
        context.prompted = true;
        // SAFETY: strdup returns libc-owned, NUL-terminated storage for PAM;
        // the source CString remains live during this callback.
        let copy = unsafe { libc::strdup(context.credential.as_ptr().cast()) };

        if copy.is_null() {
          accepted = false;
          break;
        }

        // SAFETY: `responses` is an allocated array of `count` elements, and
        // index comes from the bounded message slice.
        unsafe { (*responses.add(index)).text = copy };
      }
      PAM_ERROR_MSG | PAM_TEXT_INFO => {}
      _ => {
        accepted = false;
        break;
      }
    }
  }

  if !accepted {
    // SAFETY: Each nonnull entry was allocated with strdup. Clear it before
    // freeing; the array itself was allocated by calloc.
    unsafe {
      for index in 0..count as usize {
        let copy = (*responses.add(index)).text;

        if !copy.is_null() {
          libc::explicit_bzero(copy.cast(), context.credential.len());
          libc::free(copy.cast());
        }
      }

      libc::free(responses.cast());
    }

    return PAM_CONV_ERR;
  }

  // SAFETY: Output is a valid PAM-provided pointer. PAM owns and frees the
  // array and its responses after this successful callback.
  unsafe { *output = responses };
  PAM_SUCCESS
}

#[cfg(test)]
mod tests {
  use super::*;

  fn conversation(styles: &[c_int]) -> (c_int, bool) {
    let mut context = ConversationContext {
      credential: Zeroizing::new(CString::new("non-secret-test-only").unwrap().into_bytes_with_nul()),
      prompted: false,
    };
    let messages: Vec<_> = styles
      .iter()
      .map(|style| PamMessage {
        style: *style,
        text: ptr::null(),
      })
      .collect();

    let pointers: Vec<_> = messages.iter().map(|message| message as *const PamMessage).collect();
    let mut response = ptr::null_mut();
    // SAFETY: The messages, pointer array, output, and context are all live
    // for this synchronous test callback.
    let status =
      unsafe { converse(styles.len() as c_int, pointers.as_ptr(), &mut response, (&mut context as *mut ConversationContext).cast()) };

    if !response.is_null() {
      // SAFETY: The successful callback allocated one response per style
      // with libc, and the test releases all of them.
      unsafe {
        for index in 0..styles.len() {
          let copy = (*response.add(index)).text;

          if !copy.is_null() {
            libc::explicit_bzero(copy.cast(), context.credential.len());
            libc::free(copy.cast());
          }
        }

        libc::free(response.cast());
      }
    }

    (status, context.prompted)
  }

  #[test]
  fn only_one_hidden_password_prompt_is_accepted() {
    assert_eq!(conversation(&[PAM_TEXT_INFO, PAM_PROMPT_ECHO_OFF]), (PAM_SUCCESS, true));
    assert_eq!(conversation(&[PAM_PROMPT_ECHO_OFF, PAM_PROMPT_ECHO_OFF]), (PAM_CONV_ERR, true));
    assert_eq!(conversation(&[2]), (PAM_CONV_ERR, false));
    assert_eq!(conversation(&[PAM_TEXT_INFO]), (PAM_SUCCESS, false));
  }

  #[test]
  fn only_authentication_rejections_are_classified_as_invalid_credential() {
    assert_eq!(classify_status(PAM_AUTH_ERR), VerifyError::Rejected);
    assert_eq!(classify_status(PAM_ACCT_EXPIRED), VerifyError::Rejected);
    assert_eq!(classify_status(PAM_CONV_ERR), VerifyError::Unavailable);
    assert_eq!(classify_status(PAM_BUF_ERR), VerifyError::Unavailable);
    assert_eq!(authentication_result(PAM_SUCCESS, false), Err(VerifyError::Unavailable));
    assert_eq!(authentication_result(PAM_AUTH_ERR, false), Err(VerifyError::Unavailable));
    assert_eq!(authentication_result(PAM_AUTH_ERR, true), Err(VerifyError::Rejected));
    assert_eq!(authentication_result(PAM_SUCCESS, true), Ok(()));
  }

  #[test]
  fn malformed_credential_is_rejected_before_opening_pam() {
    assert_eq!(verify_password("neko", b""), Err(VerifyError::Rejected));
    assert_eq!(verify_password("neko", b"a\0b"), Err(VerifyError::Rejected));
    assert_eq!(verify_password("neko", &vec![b'x'; PAM_MAX_RESP_SIZE]), Err(VerifyError::Rejected));
  }
}
