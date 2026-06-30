// Read-only AX enumeration spike for the NetEase Music sidebar.
//
// NOTICE: exploration-only spike, NOT a product surface. It exists to answer
// one question for the netease AX-evolution investigation: can the playlist
// sidebar be enumerated reliably from the gated AX tree (AXEnhancedUserInterface)
// plus scrolling, instead of OCR? It performs no playback action; it only
// scrolls the sidebar and dumps AXStaticText so a shell can dedup/count. Delete
// once the investigation closes. The caller must enable AXEnhancedUserInterface
// on the app first (the tree is otherwise 2 nodes); this spike does not mutate
// app-level attributes because the auv-driver-macos binding only sets AXFocused
// today.
//
// Usage:
//   cargo run -p auv-driver-macos --example netease_ax_sidebar_enum -- \
//     [bundle-id] [rounds] [scroll_delta] [anchor_x] [max_depth] [max_children]

use std::time::Duration;

use auv_driver::Driver;
use auv_driver::geometry::WindowPoint;
use auv_driver::input::{InputPolicy, Scroll, ScrollOptions};
use auv_driver::selector::{App, Window as WindowSel};
use auv_driver_macos::MacosDriver;
use auv_driver_macos::native::ax_tree::capture_ax_tree_snapshot;

fn arg<T: std::str::FromStr>(args: &[String], idx: usize, default: T) -> T {
  args
    .get(idx)
    .and_then(|v| v.parse().ok())
    .unwrap_or(default)
}

fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let bundle = args
    .first()
    .cloned()
    .unwrap_or_else(|| "com.netease.163music".to_string());
  let rounds: usize = arg(&args, 1, 8);
  let scroll_delta: f64 = arg(&args, 2, -600.0);
  let anchor_x: f64 = arg(&args, 3, 110.0);
  let max_depth: i64 = arg(&args, 4, 16);
  let max_children: i64 = arg(&args, 5, 400);

  let driver = MacosDriver::new();
  let session = driver.open_local().expect("open_local");
  let window = session
    .window()
    .resolve(WindowSel::main_visible().owned_by(App::bundle(bundle.clone())))
    .expect("resolve NetEase main window");
  let anchor = WindowPoint::new(anchor_x, window.frame.size.height * 0.6);
  eprintln!(
    "window frame = {}x{} at ({},{}); scroll anchor = ({}, {})",
    window.frame.size.width,
    window.frame.size.height,
    window.frame.origin.x,
    window.frame.origin.y,
    anchor_x,
    window.frame.size.height * 0.6
  );

  for round in 0..rounds {
    match capture_ax_tree_snapshot(&bundle, max_depth, max_children) {
      Ok(capture) => {
        println!("ROUND {round} nodes={}", capture.snapshot.nodes.len());
        for node in &capture.snapshot.nodes {
          if node.role != "AXStaticText" {
            continue;
          }
          let text = if !node.value.is_empty() {
            &node.value
          } else {
            &node.title
          };
          if !text.is_empty() {
            // tab-separated so the shell can split on the path/value boundary.
            println!("txt\t{}\t{}\t{}", node.bounds.x, node.path, text);
          }
        }
      }
      Err(error) => println!("ROUND {round} capture_failed: {error}"),
    }

    if round + 1 < rounds {
      if let Err(error) = session.window().scroll(
        &window,
        anchor,
        Scroll::new(0.0, scroll_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: Duration::from_millis(600),
          ..ScrollOptions::default()
        },
      ) {
        eprintln!("scroll round {round} failed: {error}");
      }
    }
  }
}
