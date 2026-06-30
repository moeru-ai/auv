// Scratch end-to-end PoC: AX-sourced NetEase playlist enumeration.
//
// NOTICE: exploration-only, deletable. Chains the new driver enable-capability,
// AX capture, and the `ax_enumerate` reconstructor against the live app — the
// minimal proof that playlist reading via AX works with no OCR. It performs no
// playback action; it only toggles the app's accessibility flag and reads.
//
// Usage: cargo run -p auv-netease-music --example netease_ax_poc -- [query] [bundle-id]

#[cfg(target_os = "macos")]
fn main() {
  use std::time::Duration;

  use auv_driver_macos::native::ax_tree::{
    capture_ax_tree_snapshot, set_app_enhanced_user_interface,
  };
  use auv_netease_music::output::collect_matches;
  use auv_netease_music::view_parsers::sidebar::ax_enumerate::sidebar_projection_from_ax_nodes;

  let mut args = std::env::args().skip(1);
  let query = args.next();
  let bundle = args
    .next()
    .unwrap_or_else(|| "com.netease.163music".to_string());

  // A shallow capture works even with the flag off; we use it only to learn the
  // target pid (the enable-capability takes a pid, not a bundle id).
  let probe = capture_ax_tree_snapshot(&bundle, 2, 4).expect("probe capture (is NetEase running?)");
  let pid = probe.pid as i32;
  eprintln!("pid={pid}, probe nodeCount={}", probe.snapshot.nodes.len());

  set_app_enhanced_user_interface(pid, true).expect("enable AXEnhancedUserInterface");
  std::thread::sleep(Duration::from_millis(900));

  let capture = capture_ax_tree_snapshot(&bundle, 20, 600).expect("full capture");
  eprintln!("woken nodeCount={}", capture.snapshot.nodes.len());

  let projection = sidebar_projection_from_ax_nodes(&capture.snapshot.nodes);
  println!("== AX playlist sidebar projection ==");
  for section in &projection.sections {
    println!(
      "[{:?}] {} — {} items",
      section.kind,
      section.label.as_deref().unwrap_or(""),
      section.items.len()
    );
    for item in section.items.iter().take(8) {
      println!("    - {}", item.label);
    }
    if section.items.len() > 8 {
      println!("    … (+{} more)", section.items.len() - 8);
    }
  }

  if let Some(query) = query {
    let matches = collect_matches(&projection, Some(&query));
    println!(
      "\n== collect_matches({query:?}) -> {} match(es) ==",
      matches.len()
    );
    for hit in &matches {
      println!("    {} [{:?}]", hit.label, hit.section_kind);
    }
  }

  // Leave the app as found; latching means a woken tree stays usable regardless.
  let _ = set_app_enhanced_user_interface(pid, false);
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("netease_ax_poc is macOS-only");
}
