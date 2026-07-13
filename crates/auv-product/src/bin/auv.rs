//! Root `auv` binary (product CLI).

#[tokio::main]
async fn main() {
  auv_product::cli_frontend::exit_on_error(auv_product::cli_frontend::run_root().await);
}
