#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
  auv_qqmusic::cli::run()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
  eprintln!("auv-qqmusic only runs on macOS");
  std::process::ExitCode::FAILURE
}
