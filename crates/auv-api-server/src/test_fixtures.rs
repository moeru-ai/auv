use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn api_temp_store_root(label: &str) -> PathBuf {
  let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
  let path = std::env::temp_dir().join(format!("auv-api-{label}-{}-{unique}", uuid::Uuid::now_v7()));
  let _ = fs::remove_dir_all(&path);
  fs::create_dir_all(&path).expect("API fixture directory");
  path
}
