pub use auv_cli_invoke::{ExecutionTarget, InvokeRequest, InvokeResult, RunStatus};
pub type AuvResult<T> = Result<T, String>;

pub fn now_millis() -> u64 {
  u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX)
}
