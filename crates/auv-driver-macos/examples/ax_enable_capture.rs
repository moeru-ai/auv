// Scratch: set AXEnhancedUserInterface via our capability THEN capture in the
// SAME process. Distinguishes "set is ineffective" from "set only persists
// while the setting process keeps its AX connection alive" (System Events stays
// resident; our one-shot ax_enable exits immediately).
//
// NOTICE: exploration-only, deletable. Part of the NetEase AX-evolution probe.
//
// Usage: cargo run -p auv-driver-macos --example ax_enable_capture -- \
//   <pid> [bundle-id] [true|false] [depth] [children]

use std::time::Duration;

use auv_driver_macos::native::ax_tree::{
  capture_ax_tree_snapshot, set_app_enhanced_user_interface,
};

fn main() {
  let mut args = std::env::args().skip(1);
  let pid: i32 = args
    .next()
    .and_then(|v| v.parse().ok())
    .expect("usage: ax_enable_capture <pid> [bundle] [true|false] [depth] [children]");
  let bundle = args
    .next()
    .unwrap_or_else(|| "com.netease.163music".to_string());
  let enabled = args.next().map(|v| v == "true" || v == "1").unwrap_or(true);
  let depth: i64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(14);
  let children: i64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(250);

  let set = set_app_enhanced_user_interface(pid, enabled);
  println!("set(enabled={enabled}) -> {set:?}");
  std::thread::sleep(Duration::from_millis(800));

  match capture_ax_tree_snapshot(&bundle, depth, children) {
    Ok(capture) => println!("nodeCount={}", capture.snapshot.nodes.len()),
    Err(error) => println!("capture_err: {error}"),
  }
}
