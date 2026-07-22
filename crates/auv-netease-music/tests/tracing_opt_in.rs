use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use auv_netease_music::invoke::{build_select_result_from_fixture_dir, hermetic_select_proof_fixture_dir};
use auv_netease_music::recording::{PlaylistSelectInstrumentation, persist_playlist_select_proof};
use auv_tracing::{AuthorityId, Context, Dispatch, MemoryRunStore, RunId, RunSnapshot, RunStore, configure, dispatcher};

struct TestTracing {
  dispatch: Dispatch,
  store: Arc<MemoryRunStore>,
  run_id: RunId,
  root: Context,
}

impl TestTracing {
  fn memory() -> Self {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch should build");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    Self {
      dispatch,
      store,
      run_id,
      root,
    }
  }

  fn flush_snapshot(&self) -> RunSnapshot {
    futures_executor::block_on(self.dispatch.flush()).expect("span writes should flush");
    futures_executor::block_on(self.store.load_snapshot(self.run_id))
      .expect("snapshot read should succeed")
      .expect("instrumented run should exist")
  }
}

#[test]
fn tracing_feature_composes_netease_and_media_tracing_without_enabling_defaults() {
  let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
  let output = Command::new(env!("CARGO"))
    .args([
      "metadata",
      "--format-version=1",
      "--no-deps",
      "--manifest-path",
    ])
    .arg(&manifest_path)
    .output()
    .expect("cargo metadata should run");
  assert!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));

  let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).expect("cargo metadata should be JSON");
  let package = metadata["packages"]
    .as_array()
    .expect("metadata packages should be an array")
    .iter()
    .find(|package| package["name"] == "auv-netease-music")
    .expect("NetEase package should be present");
  let features = package["features"].as_object().expect("package features should be an object");
  let tracing = features["tracing"]
    .as_array()
    .expect("tracing feature should be an array")
    .iter()
    .map(|member| member.as_str().expect("feature member should be a string"))
    .collect::<BTreeSet<_>>();

  assert_eq!(tracing, BTreeSet::from(["auv-media-macos/tracing"]));
  assert!(features["default"].as_array().expect("default feature should be an array").is_empty());
}

#[test]
fn direct_result_is_identical_with_and_without_active_dispatch() {
  let fixture = hermetic_select_proof_fixture_dir();
  let without = build_select_result_from_fixture_dir(&fixture).expect("fixture should parse without dispatch");
  let expected = without.clone();
  let disabled = futures_executor::block_on(persist_playlist_select_proof(&without));
  assert!(matches!(disabled, PlaylistSelectInstrumentation::Disabled));
  assert_eq!(without, expected);

  let tracing = TestTracing::memory();
  let with = tracing.root.in_scope(|| build_select_result_from_fixture_dir(&fixture)).expect("fixture should parse with dispatch");
  let publication = tracing.root.in_scope(|| persist_playlist_select_proof(&with));
  let enabled = futures_executor::block_on(tracing.root.instrument(publication));
  assert!(matches!(enabled, PlaylistSelectInstrumentation::Published(_)));

  assert_eq!(with, expected);
  assert_eq!(with.run_id, without.run_id);
  assert_eq!(with.known_limits, without.known_limits);
  let snapshot = tracing.flush_snapshot();
  let spans = snapshot.spans().values().collect::<Vec<_>>();
  assert_eq!(spans.len(), 1);
  assert_eq!(spans[0].started().name().as_str(), "auv.netease.playlist.select_proof");
  assert!(spans[0].started().attributes().is_empty());
  assert!(spans[0].ended().is_some());
  assert!(snapshot.events().is_empty());
}
