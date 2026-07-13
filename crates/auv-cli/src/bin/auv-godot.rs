//! `auv-godot` donor product binary.

#[tokio::main]
async fn main() {
  auv_cli::cli_frontend::exit_on_error(auv_cli::cli_frontend::run_donor_bin("godot").await);
}
