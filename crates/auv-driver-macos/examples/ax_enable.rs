// Scratch smoke harness for the set_app_enhanced_user_interface capability.
//
// NOTICE: exploration-only, NOT a product surface. Toggles the app-level
// AXEnhancedUserInterface flag on a target pid so the NetEase AX-evolution
// investigation can confirm the new driver capability end-to-end (enable →
// capture shows the full tree). Delete once the investigation closes.
//
// Usage: cargo run -p auv-driver-macos --example ax_enable -- <pid> [true|false]

use auv_driver_macos::native::ax_tree::set_app_enhanced_user_interface;

fn main() {
  let mut args = std::env::args().skip(1);
  let pid: i32 = args
    .next()
    .and_then(|v| v.parse().ok())
    .expect("usage: ax_enable <pid> [true|false]");
  let enabled = args.next().map(|v| v == "true" || v == "1").unwrap_or(true);

  match set_app_enhanced_user_interface(pid, enabled) {
    Ok(()) => println!("ok: set AXEnhancedUserInterface={enabled} on pid {pid}"),
    Err(error) => {
      eprintln!("err: {error}");
      std::process::exit(1);
    }
  }
}
