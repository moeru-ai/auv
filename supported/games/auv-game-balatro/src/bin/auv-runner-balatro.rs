#[tokio::main]
async fn main() {
  if let Err(error) = auv_game_balatro::runner::serve_inherited().await {
    eprintln!("Balatro Runner failed: {error}");
    std::process::exit(1);
  }
}
