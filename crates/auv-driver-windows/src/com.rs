//! COM apartment setup shared by the modules that call COM APIs (UI Automation, audio endpoints,
//! the virtual desktop manager).

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// Balances a successful `CoInitializeEx` with `CoUninitialize` on the same
/// thread. When COM was already initialized in a different apartment model
/// (`RPC_E_CHANGED_MODE`), `uninit` stays false so we do not tear down an
/// initialization we did not perform.
pub(crate) struct ComGuard {
  uninit: bool,
}

impl Drop for ComGuard {
  fn drop(&mut self) {
    if self.uninit {
      unsafe { CoUninitialize() };
    }
  }
}

/// Joins the multithreaded apartment. UIA is happiest in an MTA; if the thread is already STA
/// this returns RPC_E_CHANGED_MODE and we proceed against the existing apartment.
pub(crate) fn init_com() -> ComGuard {
  let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
  ComGuard { uninit: hr.is_ok() }
}
