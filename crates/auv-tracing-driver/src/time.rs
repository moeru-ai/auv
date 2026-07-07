use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}
