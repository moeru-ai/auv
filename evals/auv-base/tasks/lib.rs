//! Shared receipt and process support for independent evaluation tasks.

#[path = "../platforms/desktop-macos/tasks/mod.rs"]
pub mod macos;

#[cfg(windows)]
#[path = "../platforms/desktop-windows/tasks/mod.rs"]
pub mod desktop_windows;

pub mod receiver;
pub mod support;
