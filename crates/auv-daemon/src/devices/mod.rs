//! Shared policy and durable state for existing OS login-session access.
//!
//! Platform adapters are added independently; this module owns the common
//! admission, enrollment-state, audit, and same-session verification rules.

mod audit;
#[cfg(target_os = "linux")]
#[path = "platform/linux/enrollment.rs"]
mod enrollment_linux;
#[cfg(target_os = "linux")]
#[path = "platform/linux/host.rs"]
mod host_linux;
#[cfg(target_os = "linux")]
mod local;
mod metadata;
#[cfg(target_os = "linux")]
#[path = "platform/linux/pam.rs"]
mod pam_native;
mod policy;
#[cfg(target_os = "linux")]
#[path = "platform/unix/account.rs"]
mod unix_account;
#[cfg(target_os = "linux")]
#[path = "platform/linux/vault.rs"]
mod vault_linux;

#[cfg(target_os = "linux")]
pub(crate) use local::LocalState;
