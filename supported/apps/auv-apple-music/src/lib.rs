//! Apple Music app integration.

pub mod cli;
mod platforms;

#[cfg(feature = "tracing")]
mod tracing {
  use auv_tracing::{Attributes, ByteLength, EmitBytesOptions};
  use serde::Serialize;

  const JSON_ARTIFACT_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

  pub(super) fn window_open<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.window.open", operation)
  }

  pub(super) fn ax_probe<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.ax.probe", operation)
  }

  pub(super) fn search<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.search", operation)
  }

  pub(super) fn search_result_select<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.search_result.select", operation)
  }

  pub(super) fn playback_status<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.playback.status", operation)
  }

  pub(super) fn transport<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::in_span!("auv.apple_music.transport", operation)
  }

  #[derive(Serialize)]
  struct ArtifactPreparationFailed {
    purpose: &'static str,
    error: String,
  }

  impl auv_tracing::EventPayload for ArtifactPreparationFailed {
    const NAME: &'static str = "auv.apple_music.artifact_preparation_failed";
    const VERSION: u32 = 1;
  }

  pub(super) fn json_artifact<T: Serialize>(purpose: &'static str, value: &T) {
    if !auv_tracing::Context::current().can_publish_artifacts() {
      return;
    }
    match auv_tracing::emit_json_artifact(
      purpose,
      Attributes::empty(),
      ByteLength::new(JSON_ARTIFACT_BYTE_LIMIT).expect("static Apple Music JSON limit is valid"),
      value,
    )
    .map_err(|error| format!("encode JSON artifact failed: {error}"))
    {
      Ok(emission) => drop(emission),
      Err(error) => preparation_failed(purpose, error),
    }
  }

  pub(super) fn image_artifact(purpose: &'static str, image: &image::RgbaImage) {
    if !auv_tracing::Context::current().can_publish_artifacts() {
      return;
    }
    let options = EmitBytesOptions::new().with_purpose(purpose).with_attributes(Attributes::empty());
    match auv_tracing::image_artifact(options, image) {
      Ok(artifact) => drop(auv_tracing::emit_artifact!(artifact)),
      Err(error) => preparation_failed(purpose, error.to_string()),
    }
  }

  pub(super) fn image_artifact_with(purpose: &'static str, capture: impl FnOnce() -> Result<image::RgbaImage, String>) {
    if !auv_tracing::Context::current().can_publish_artifacts() {
      return;
    }
    match capture() {
      Ok(image) => image_artifact(purpose, &image),
      Err(error) => preparation_failed(purpose, error),
    }
  }

  fn preparation_failed(purpose: &'static str, error: String) {
    auv_tracing::emit_event!(ArtifactPreparationFailed { purpose, error });
  }
}

#[cfg(not(feature = "tracing"))]
mod tracing {
  use serde::Serialize;

  pub(super) fn window_open<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn ax_probe<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn search<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn search_result_select<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn playback_status<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn transport<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn json_artifact<T: Serialize>(_purpose: &'static str, _value: &T) {}

  pub(super) fn image_artifact<T>(_purpose: &'static str, _image: &T) {}

  pub(super) fn image_artifact_with<T>(_purpose: &'static str, _capture: impl FnOnce() -> Result<T, String>) {}
}

pub use platforms::*;
