#[tokio::main]
async fn main() {
  if let Err(error) = auv_netease_music::runner::serve_inherited().await {
    eprintln!("NetEase Music Runner failed: {error}");
    std::process::exit(1);
  }
}
