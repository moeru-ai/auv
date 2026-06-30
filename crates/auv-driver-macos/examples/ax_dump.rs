// Scratch AX-tree dumper for live exploration.
//
// NOTICE: exploration-only utility, NOT the product CLI. The
// `window.captureAxTree` frontend in auv-cli-invoke is intentionally still a
// stub; this example only exists to inspect a live application's AX tree during
// the NetEase Music AX-evolution investigation. It is not part of any approved
// feature slice and can be deleted once the investigation closes.
//
// Usage:
//   cargo run -p auv-driver-macos --example ax_dump -- <bundle-id> [max_depth] [max_children]

use auv_driver_macos::native::ax_tree::{capture_ax_tree_snapshot, render_ax_tree_report};

fn main() {
  let mut args = std::env::args().skip(1);
  let app = args
    .next()
    .unwrap_or_else(|| "com.netease.163music".to_string());
  let max_depth = args.next().and_then(|v| v.parse().ok()).unwrap_or(20);
  let max_children = args.next().and_then(|v| v.parse().ok()).unwrap_or(200);

  match capture_ax_tree_snapshot(&app, max_depth, max_children) {
    Ok(capture) => print!("{}", render_ax_tree_report(&capture)),
    Err(error) => {
      eprintln!("capture failed: {error}");
      std::process::exit(1);
    }
  }
}
