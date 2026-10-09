//! Process-level COM apartment contract for the WGC backend.
//!
//! This lives in its own integration-test binary on purpose: the assertion is about the
//! whole process, so no other test may create or hold a multithreaded apartment (MTA).
#![cfg(target_os = "windows")]

use auv_driver_windows::prewarm_wgc;
use windows::Win32::System::Com::{
  APTTYPE, APTTYPE_MTA, APTTYPEQUALIFIER, COINIT_APARTMENTTHREADED, CoGetApartmentType, CoInitializeEx, CoUninitialize,
};

// Observed as the intermittent `wgc_capture` crash on the `rust checks windows-2025` job
// (exit code 0xc0000005, STATUS_ACCESS_VIOLATION), for example `Check` run 37918189761.
//
// ROOT CAUSE:
//
// If the first WGC activation in a process ran on a thread that already had a COM
// apartment, the process was left without any MTA of its own. A thread that owns a window
// is such a thread: user32/TSF makes it the main STA, and the windows-rs `factory()`
// fallback that adds an MTA reference only fires on a thread with no apartment. The MTA
// then existed only while a thread inside the capture stack happened to hold it. When the
// last such thread released it, combase tore the MTA down and unloaded unused DLLs on that
// thread, which could be one started by `GraphicsCapture.dll`. It returned into the
// unmapped image and faulted (execute access violation inside the unloaded
// `GraphicsCapture.dll`; a second thread then faulted in RPCRT4 on the unloaded proxy stub).
//
// Before the fix, `prewarm_wgc()` called from an STA thread left the process with no MTA
// at all, so a fresh thread got `CO_E_NOTINITIALIZED` immediately and forever.
// The fix keeps a process-lifetime MTA reference taken before any WGC WinRT object is
// created, so a fresh thread always finds an MTA after WGC has been used.
#[test]
fn wgc_use_from_an_sta_thread_leaves_a_process_mta() {
  std::thread::spawn(|| {
    // Reproduce the window-owning thread: it already has an apartment, so the windows-rs
    // fallback does not run and cannot be what keeps the MTA alive.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().expect("failed to initialize an STA on the capture thread");
    prewarm_wgc().expect("prewarm_wgc should succeed");
    unsafe { CoUninitialize() };
  })
  .join()
  .expect("capture thread panicked");

  // A thread that never touched COM sees the process-wide MTA, if there is one.
  let apartment = std::thread::spawn(|| {
    let mut apartment_type = APTTYPE(0);
    let mut qualifier = APTTYPEQUALIFIER(0);
    unsafe { CoGetApartmentType(&mut apartment_type, &mut qualifier) }.map(|()| apartment_type)
  })
  .join()
  .expect("probe thread panicked");

  assert_eq!(
    apartment.as_ref().map(|apartment_type| *apartment_type).map_err(|error| error.code()),
    Ok(APTTYPE_MTA),
    "WGC must leave a process-lifetime MTA reference behind, otherwise combase can unload GraphicsCapture.dll under a live thread"
  );
}
