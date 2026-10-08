// The tool drives Windows-only APIs; other platforms get a stub `main` so
// `cargo build --workspace` and `cargo publish` verification succeed there.
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
fn main() {
  windows::main();
}

#[cfg(not(target_os = "windows"))]
fn main() {
  eprintln!("replay-qqmusic-operation only runs on Windows");
  std::process::exit(1);
}
