//! Root `auv` binary (product CLI).

fn main() {
  let arguments = std::env::args().skip(1).collect::<Vec<_>>();
  if auv_cli::cli::root_version_requested(&arguments) {
    print!("{}", auv_cli::cli::version_text());
    return;
  }

  run();
}

#[tokio::main]
async fn run() {
  auv_cli::cli_frontend::exit_on_error(auv_cli::cli_frontend::run_root().await);
}
