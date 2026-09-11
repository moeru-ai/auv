#![deny(clippy::all)]

use napi_derive::napi;

/// Return the version embedded into the native binding.
///
/// The JavaScript entrypoint compares this value with its package manifest so
/// a partially published release fails before it starts an AUV sidecar.
#[napi]
pub fn native_package_version() -> &'static str {
  env!("CARGO_PKG_VERSION")
}

// TODO(napi-operation-api): operation bindings are intentionally deferred;
// add them only when an owner-approved in-process consumer needs that surface.
