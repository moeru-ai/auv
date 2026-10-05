//! Platform observations used by existing-session Device lock and unlock.
//!
//! Supported host candidates route DeviceService through the same target-local
//! switch, enrollment, account locks, and audit as DeviceLocalService.

#[cfg(target_os = "macos")]
#[path = "platform/macos/enrollment.rs"]
mod enrollment_macos;

#[cfg(target_os = "macos")]
#[path = "platform/macos/host.rs"]
mod host_macos;

#[cfg(target_os = "windows")]
#[path = "platform/windows/enrollment.rs"]
mod enrollment_windows;
#[cfg(target_os = "windows")]
#[path = "platform/windows/host.rs"]
mod host_windows;

#[cfg(target_os = "linux")]
#[path = "platform/linux/host.rs"]
mod host_linux;

#[cfg(target_os = "linux")]
#[path = "platform/linux/vault.rs"]
mod vault_linux;

#[cfg(target_os = "linux")]
#[path = "platform/linux/enrollment.rs"]
mod enrollment_linux;

#[cfg(target_os = "linux")]
#[path = "platform/linux/pam.rs"]
mod pam_native;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "platform/unix/account.rs"]
mod unix_account;

mod audit;
mod metadata;
mod policy;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod local;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub(crate) use local::LocalState;
