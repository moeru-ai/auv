use auv_driver_overlay_common::{Easing, Overlay, Removal, ShowOptions};

use crate::AuvResult;

/// Renders an ordered overlay through the native Win32 layered-window
/// adapter.
///
/// TODO(driver-overlay-windows-motion): per-layer position easing is
/// deferred for this slice; every render draws the requested layers in
/// their final state immediately instead of animating between renders.
/// Revisit once a consumer needs animated cursor motion rather than the
/// current one-shot visual evidence (see `Overlay::with_layer`'s deferral
/// note in `auv-driver-overlay-common`).
pub fn render(overlay: &Overlay, options: ShowOptions) -> AuvResult<()> {
  // NOTICE: the native renderer currently implements ease-in-out-expo as the
  // sole shared easing contract (there is only one `Easing` variant today).
  // Extend the match when `Easing` gains another variant.
  match options.motion().easing() {
    Easing::EaseInOutExpo => {}
  }

  crate::window::present(overlay.layers())?;

  match options.lifecycle().removal() {
    Removal::Manual => {}
    Removal::AutoAfter(duration) => {
      if !duration.is_zero() {
        std::thread::sleep(duration);
      }
      remove()?;
    }
  }

  Ok(())
}

/// Hides the native overlay window, removing all previously shown layers.
pub fn remove() -> AuvResult<()> {
  crate::window::hide_all()
}
