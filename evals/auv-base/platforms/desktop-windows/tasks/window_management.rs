//! `window-management`: `set_frame`, minimize, maximize and focus on a throwaway receiver,
//! through the public `WindowsDriverSession`. The scenario itself is shared with macOS
//! (`crate::window_management`); its oracle is a fresh `WindowApi::get` after each step.
//!
//! Unlike the background scenarios, this one changes the receiver's state and takes the
//! foreground for it, so there is no desktop watch.

use anyhow::Result;
use auv_driver_common::Driver;
use auv_driver_windows::WindowsDriver;

use super::background::{discover_by_hwnd, receiver_spec};
use super::receiver_process::{Receiver, receiver_binary};

pub fn window_management() -> Result<bool> {
  let session = WindowsDriver::new().open_local().map_err(|error| anyhow::anyhow!("open driver session: {error}"))?;
  let receiver = Receiver::spawn_detached(&receiver_binary()?, &receiver_spec(0, false)?)?;
  let (window, _) = discover_by_hwnd(&session, receiver.hwnd())?;
  Ok(match crate::window_management::run(&session.window(), &window.reference.id) {
    Ok(()) => {
      eprintln!("window-management: pass");
      true
    }
    Err(error) => {
      eprintln!("window-management: fail: {error:#}");
      false
    }
  })
}
