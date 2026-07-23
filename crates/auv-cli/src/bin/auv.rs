//! Root `auv` binary (product CLI).

fn main() -> std::process::ExitCode {
  let arguments = std::env::args().skip(1).collect::<Vec<_>>();
  if auv_cli::cli::root_version_requested(&arguments) {
    print!("{}", auv_cli::cli::version_text());
    return std::process::ExitCode::SUCCESS;
  }

  run()
}

#[tokio::main]
async fn run() -> std::process::ExitCode {
  auv_cli::cli_frontend::exit_status(auv_cli::cli_frontend::run_root().await)
}
