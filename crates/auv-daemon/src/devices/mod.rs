//! Shared policy and durable state for existing OS login-session access.
//!
//! Platform adapters are added independently; this module owns the common
//! admission, enrollment-state, audit, and same-session verification rules.

mod audit;
#[cfg(target_os = "linux")]
#[path = "platform/linux/enrollment.rs"]
mod enrollment_linux;
#[cfg(target_os = "macos")]
#[path = "platform/macos/enrollment.rs"]
mod enrollment_macos;
#[cfg(target_os = "linux")]
#[path = "platform/linux/host.rs"]
mod host_linux;
#[cfg(target_os = "macos")]
#[path = "platform/macos/host.rs"]
mod host_macos;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod local;
mod metadata;
#[cfg(target_os = "linux")]
#[path = "platform/linux/pam.rs"]
mod pam_native;
mod policy;
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "platform/unix/account.rs"]
mod unix_account;
#[cfg(target_os = "linux")]
#[path = "platform/linux/vault.rs"]
mod vault_linux;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use local::LocalState;
