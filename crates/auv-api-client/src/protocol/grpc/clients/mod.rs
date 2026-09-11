//! Package-shaped gRPC service clients.
//!
//! The module hierarchy mirrors the owning Protobuf packages. Business clients
//! compose these services into the Device → Run → Runner hierarchy; callers
//! should normally enter through [`crate::Client`].

pub mod daemon;
