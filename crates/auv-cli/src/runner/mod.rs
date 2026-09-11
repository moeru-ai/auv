//! First-party Runner process entrypoints hosted by the `auv` executable.

mod local_driver;

pub(crate) const INTERNAL_SENTINEL: &str = "__auv-internal-runner";
pub(crate) const LOCAL_DRIVER_ROLE: &str = "local-driver";
pub(crate) const STATE_ROOT_ENV: &str = "AUV_RUNNER_STATE_ROOT";

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
  // NOTICE: The macOS overlay adapter owns a thread-local AppKit controller.
  // Use a current-thread runtime so the local Runner stays on the process main
  // thread from initialization through request handling.
  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("build local Runner runtime");
  match runtime.block_on(local_driver::serve_inherited()) {
    Ok(()) => 0,
    Err(error) => {
      eprintln!("AUV local driver Runner failed: {error}");
      1
    }
  }
}
