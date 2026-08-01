//! Root `auv` core command frontend and first-party Runner process host.

fn main() {
  let arguments = std::env::args().collect::<Vec<_>>();
  if arguments.get(1).is_some_and(|argument| argument == auv_cli::INTERNAL_RUNNER_SENTINEL) {
    let exit = match arguments.as_slice() {
      [_, _, role] if role == auv_cli::LOCAL_RUNNER_ROLE => run_local_driver(),
      [_, _, role] if role == auv_cli::INFERENCE_RUNNER_ROLE => run_inference(),
      _ => {
        eprintln!("invalid internal AUV Runner invocation");
        2
      }
    };
    std::process::exit(exit);
  }

  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("build AUV CLI runtime");
  let exit = runtime.block_on(auv_cli::cli_frontend::run_root());
  std::process::exit(auv_cli::cli_frontend::exit_status(exit));
}

fn run_local_driver() -> i32 {
  // NOTICE: The macOS overlay adapter owns a thread-local AppKit controller.
  // Dispatch before constructing the ordinary multi-thread runtime so the
  // first-party local Runner remains on the process main thread.
  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("build local Runner runtime");
  match runtime.block_on(auv_cli::runner_child::serve_inherited()) {
    Ok(()) => 0,
    Err(error) => {
      eprintln!("AUV local driver Runner failed: {error}");
      1
    }
  }
}

fn run_inference() -> i32 {
  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("build inference Runner runtime");
  match runtime.block_on(auv_runner_inference_ultralytics::serve_inherited()) {
    Ok(()) => 0,
    Err(error) => {
      eprintln!("AUV inference Runner failed: {error}");
      1
    }
  }
}
