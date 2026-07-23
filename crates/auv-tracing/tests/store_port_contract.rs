use auv_tracing::RunStore;
use std::sync::Arc;

fn accepts_dyn_store(_store: Arc<dyn RunStore>) {}

#[test]
fn run_store_is_dyn_compatible() {
  let _: fn(Arc<dyn RunStore>) = accepts_dyn_store;
}
