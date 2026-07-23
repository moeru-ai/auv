//! `auv-osu` donor product binary.

#[tokio::main]
async fn main() -> std::process::ExitCode {
  auv_cli::cli_frontend::exit_status(auv_cli::cli_frontend::run_donor_bin("osu").await)
}
