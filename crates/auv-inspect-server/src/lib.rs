//! Viewer-facing HTTP authority for canonical AUV run data.
//!
//! This crate composes an existing [`auv_tracing::RunStore`]. It does not
//! execute operations, own application control flow, or maintain a second run
//! recording model.

pub mod session;

mod run_api;
mod server;
mod viewer_assets;

pub use server::{DEFAULT_INSPECT_HOST, DEFAULT_INSPECT_PORT, InspectServeConfig, router, router_with_artifact_origin, serve};
pub use session::{InspectServerSession, default_session_path, read_inspect_session, write_inspect_session};

pub type InspectResult<T> = Result<T, String>;

#[cfg(test)]
mod tests {
  #[test]
  fn v1_has_no_generic_json_extension_protocol() {
    let library = include_str!("lib.rs").split("#[cfg(test)]").next().expect("library source should contain the test boundary");
    let server = include_str!("server.rs");
    let run_api = include_str!("run_api.rs");

    for source in [library, server, run_api] {
      assert!(!source.contains("InspectRunExtension"), "Inspect V1 still exposes a generic JSON extension trait");
      assert!(!source.contains("router_with_extension"), "Inspect V1 still exposes extension-specific router composition");
      assert!(!source.contains("/extensions/{extension}"), "Inspect V1 still registers a generic extension route");
    }
  }
}
