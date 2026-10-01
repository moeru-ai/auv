//! Shared policy and durable state for existing OS login-session access.
//!
//! Platform adapters are added independently; this module owns the common
//! admission, enrollment-state, audit, and same-session verification rules.

mod audit;
mod metadata;
mod policy;
