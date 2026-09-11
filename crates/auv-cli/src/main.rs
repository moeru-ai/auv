//! Root `auv` core command frontend and first-party Runner process host.

fn main() {
  let arguments = std::env::args().collect::<Vec<_>>();
  if let Some(exit) = auv_cli::runner::run_if_internal(&arguments) {
    std::process::exit(exit);
  }

  let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("build AUV CLI runtime");
  let exit = runtime.block_on(auv_cli::cli::run_root());
  std::process::exit(auv_cli::cli::exit_status(exit));
}
