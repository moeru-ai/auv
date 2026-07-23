use std::sync::Arc;

use auv_tracing::{AuthorityId, Context, EventPayload, MemoryRunStore, RunId, RunStore, configure, dispatcher};
use serde::Serialize;

use super::{build_product_inspect_document, inspect_run};

#[derive(Serialize)]
struct FixtureEvent {
  value: u8,
}

impl EventPayload for FixtureEvent {
  const NAME: &'static str = "auv.test.product_inspect";
  const VERSION: u32 = 1;
}

#[tokio::test]
async fn canonical_product_inspect_has_locked_root_boundaries() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  root.in_scope(|| auv_tracing::emit_event!(FixtureEvent { value: 1 }));
  dispatch.flush().await.expect("flush fixture event");
  let snapshot = store.load_snapshot(run_id).await.expect("load fixture run").expect("fixture run");

  let document = build_product_inspect_document(store.as_ref(), &snapshot).await.expect("canonical product inspection");
  let ids = document.sections.iter().map(|section| section.id).collect::<Vec<_>>();
  assert_eq!(ids.first(), Some(&"core_prefix"));
  assert_eq!(ids.last(), Some(&"core_suffix"));
  assert!(document.render_text().contains(&run_id.to_string()));
}

#[tokio::test]
async fn inspect_entrypoint_renders_the_canonical_document() {
  let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
  let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  root.in_scope(|| auv_tracing::emit_event!(FixtureEvent { value: 2 }));
  dispatch.flush().await.expect("flush fixture event");
  let snapshot = store.load_snapshot(run_id).await.expect("load fixture run").expect("fixture run");

  let text = inspect_run(store.as_ref(), &snapshot).await.expect("inspect run");
  assert!(text.starts_with(&format!("Run {run_id}")));
}
