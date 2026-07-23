use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn session_api_temp_store_root(label: &str) -> PathBuf {
  let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
  let path = std::env::temp_dir().join(format!("auv-session-api-{label}-{}-{unique}", crate::model::now_millis()));
  let _ = fs::remove_dir_all(&path);
  fs::create_dir_all(&path).expect("session API fixture directory");
  path
}
