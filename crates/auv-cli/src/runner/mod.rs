//! First-party Runner process entrypoints hosted by the `auv` executable.

mod capture_store;
mod local_driver;
mod recent_frames;
mod recording;

pub(crate) const INTERNAL_SENTINEL: &str = "__auv-internal-runner";
pub(crate) const LOCAL_DRIVER_ROLE: &str = "local-driver";
pub(crate) const STATE_ROOT_ENV: &str = "AUV_RUNNER_STATE_ROOT";
/// Parent of the per-Runner trace stores: `<root>/<runner_id>/records.jsonl`.
/// Each Runner process appends to its own store, so concurrent Runners never
/// interleave writes in one file.
pub(crate) const TRACE_ROOT_ENV: &str = "AUV_RUNNER_TRACE_ROOT";

/// Runs an internal Runner role before the ordinary CLI runtime is created.
///
/// The local Driver owns thread-local platform state, so this dispatch must
/// remain ahead of the root multi-thread Tokio runtime in `main`.
pub fn run_if_internal(arguments: &[String]) -> Option<i32> {
  if arguments.get(1).is_none_or(|argument| argument != INTERNAL_SENTINEL) {
    return None;
  }

  Some(match arguments {
    [_, _, role] if role == LOCAL_DRIVER_ROLE => run_local_driver(),
    _ => {
      eprintln!("invalid internal AUV Runner invocation");
      2
    }
  })
}

fn run_local_driver() -> i32 {
  // Runner stderr is the daemon's stderr; the file names the Runner the daemon
  // reports so a failed Runner in `auv runner list` has a log to read.
  let runner_id = std::env::var(auv_daemon::runner_provider::RUNNER_ID_ENV).unwrap_or_else(|_| format!("pid-{}", std::process::id()));
  crate::logging::init(0, Some(&format!("runner-{runner_id}")));
  tracing::info!(runner_id = %runner_id, role = LOCAL_DRIVER_ROLE, version = env!("CARGO_PKG_VERSION"), "Runner starting");
  // NOTICE: The macOS overlay adapter owns a thread-local AppKit controller.
  // Use a current-thread runtime so the local Runner stays on the process main
  // thread from initialization through request handling.
  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("build local Runner runtime");
  match runtime.block_on(local_driver::serve_inherited()) {
    Ok(()) => {
      tracing::info!(runner_id = %runner_id, "Runner stopped");
      0
    }
    Err(error) => {
      tracing::error!(runner_id = %runner_id, %error, "Runner failed");
      eprintln!("AUV local driver Runner failed: {error}");
      1
    }
  }
}
