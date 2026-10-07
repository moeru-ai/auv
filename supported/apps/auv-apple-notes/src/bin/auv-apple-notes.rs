#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
  auv_apple_notes::cli::run()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
  eprintln!("auv-apple-notes only runs on macOS");
  std::process::ExitCode::FAILURE
}
