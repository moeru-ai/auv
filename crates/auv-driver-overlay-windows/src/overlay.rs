use auv_driver_overlay_common::{Easing, Overlay, Removal, ShowOptions};

use crate::AuvResult;

/// Renders an ordered overlay through the native Win32 layered-window
/// adapter.
///
/// This is the one-shot path: every call draws the requested layers in their
/// final state at once and does not animate between calls. Animated cursor
/// motion that follows real operations is [`crate::Animator`], which calls the
/// same renderer once per frame with the layers a motion scene composes.
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
