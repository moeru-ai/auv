#![forbid(unsafe_code)]

//! Versioned, HTTP-frontend-neutral DTOs for the Inspect run protocol.

// TODO(inspect-store-client-v1): The HTTP RunStore client and binary transfer
// remain deferred to Task 12; Task 11 exports protocol DTOs only.
pub mod protocol;
