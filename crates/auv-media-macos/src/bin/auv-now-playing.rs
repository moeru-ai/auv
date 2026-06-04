// Slice-1 temporary entry point: prove the harness returns live data.
// Replaced by `auv_media_macos::cli::run()` in the next slice.
fn main() {
  match auv_media_macos::now_playing() {
    Ok(state) => println!("{state:#?}"),
    Err(error) => {
      eprintln!("{error}");
      std::process::exit(1);
    }
  }
}
