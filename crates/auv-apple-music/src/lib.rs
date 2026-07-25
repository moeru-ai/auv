//! Apple Music app integration.

pub mod cli;
mod platforms;

#[cfg(feature = "tracing")]
mod tracing {
  use auv_tracing::{ArtifactPurpose, Attributes, ByteLength, ContentType, NewArtifact};
  use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
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
    match ArtifactPurpose::parse(purpose).map_err(|error| error.to_string()).and_then(|purpose| {
      auv_tracing::emit_json_artifact(
        purpose,
        Attributes::empty(),
        ByteLength::new(JSON_ARTIFACT_BYTE_LIMIT).expect("static Apple Music JSON limit is valid"),
        value,
      )
      .map_err(|error| format!("encode JSON artifact failed: {error}"))
    }) {
      Ok(emission) => drop(emission),
      Err(error) => preparation_failed(purpose, error),
    }
  }

  pub(super) fn image_artifact(purpose: &'static str, image: &image::RgbaImage) {
    if !auv_tracing::Context::current().can_publish_artifacts() {
      return;
    }
    let mut body = Vec::new();
    let prepared = PngEncoder::new(&mut body)
      .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
      .map_err(|error| format!("encode PNG artifact failed: {error}"))
      .and_then(|()| emit_bytes(purpose, "image/png", body));
    match prepared {
      Ok(()) => {}
      Err(error) => preparation_failed(purpose, error),
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

  fn emit_bytes(purpose: &'static str, content_type: &'static str, body: Vec<u8>) -> Result<(), String> {
    let artifact = NewArtifact::from_bytes(
      ArtifactPurpose::parse(purpose).map_err(|error| error.to_string())?,
      ContentType::parse(content_type).map_err(|error| error.to_string())?,
      Attributes::empty(),
      body,
    )
    .map_err(|error| error.to_string())?;
    drop(auv_tracing::emit_artifact!(artifact));
    Ok(())
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

#[cfg(all(test, feature = "tracing"))]
mod tracing_tests {
  use std::cell::Cell;
  use std::sync::Arc;

  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;

  #[test]
  fn command_uses_the_caller_context_without_owning_a_run() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let inputs = SearchResultSelectInputs {
      search: SearchInputs::with_query("fixture"),
      anchor: String::new(),
      selection_timeout_ms: 0,
    };

    let result = root.in_scope(|| run_search_result_select(&inputs));

    assert!(result.is_err());
    futures_executor::block_on(dispatch.flush()).expect("flush");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("run snapshot");
    let span = snapshot.spans().values().next().expect("command span");
    assert_eq!(span.started().name().as_str(), "auv.apple_music.search_result.select");
    assert!(span.started().attributes().is_empty());
    assert!(span.ended().is_some());
  }

  #[test]
  fn artifact_helper_publishes_bytes_to_the_caller_store() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    root.in_scope(|| tracing::json_artifact("auv.apple_music.test_snapshot", &serde_json::json!({ "node_count": 3 })));

    futures_executor::block_on(dispatch.flush()).expect("flush");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("run snapshot");
    let artifact = snapshot.artifacts().values().next().expect("published artifact");
    assert_eq!(artifact.metadata().purpose().as_str(), "auv.apple_music.test_snapshot");
    assert_eq!(artifact.metadata().content_type().to_string(), "application/json");
    assert_eq!(snapshot.artifacts().len(), 1);
  }

  #[test]
  fn disabled_runtime_does_not_invoke_artifact_capture() {
    let invoked = Cell::new(false);

    tracing::image_artifact_with("auv.apple_music.test_capture", || {
      invoked.set(true);
      Err::<image::RgbaImage, _>("capture should not run".to_string())
    });

    assert!(!invoked.get());
  }

  #[test]
  fn artifact_capture_failure_is_recorded_without_changing_the_direct_value() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    let direct_value = root.in_scope(|| {
      let _ = tracing::image_artifact_with("auv.apple_music.test_capture", || Err::<image::RgbaImage, _>("capture unavailable".to_string()));
      42
    });

    assert_eq!(direct_value, 42);
    futures_executor::block_on(dispatch.flush()).expect("flush preparation event");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("run snapshot");
    assert_eq!(snapshot.events().len(), 1);
    assert_eq!(snapshot.events()[0].schema().name().as_str(), "auv.apple_music.artifact_preparation_failed");
  }
}
