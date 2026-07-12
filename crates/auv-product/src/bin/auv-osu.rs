//! `auv-osu` donor product binary.

#[tokio::main]
async fn main() {
  auv_product::cli_frontend::exit_on_error(auv_product::cli_frontend::run_donor_bin("osu").await);
}
