use std::sync::Arc;

use auv_netease_music::invoke::{build_select_result_from_fixture_dir, hermetic_select_proof_fixture_dir};
use auv_tracing::{AuthorityId, Context, Dispatch, MemoryRunStore, RunId, RunStore, configure, dispatcher};

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

  fn span_names(&self) -> Vec<String> {
    let snapshot = futures_executor::block_on(self.store.load_snapshot(self.run_id))
      .expect("snapshot read should succeed")
      .expect("instrumented run should exist");
    snapshot.spans().values().map(|span| span.started().name().as_str().to_owned()).collect()
  }
}

#[test]
fn direct_result_is_identical_with_and_without_active_dispatch() {
  let fixture = hermetic_select_proof_fixture_dir();
  let without = build_select_result_from_fixture_dir(&fixture).expect("fixture should parse without dispatch");
  let tracing = TestTracing::memory();
  let with = tracing.root.in_scope(|| build_select_result_from_fixture_dir(&fixture)).expect("fixture should parse with dispatch");

  assert_eq!(with, without);
  futures_executor::block_on(tracing.dispatch.flush()).expect("span writes should flush");
  assert_eq!(tracing.span_names(), vec!["auv.netease.playlist.select_proof"]);
}
