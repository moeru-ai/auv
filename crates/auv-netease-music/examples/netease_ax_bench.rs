// Scratch bench: time the FULL AX playlist enumeration (scroll + position keying).
//
// NOTICE: exploration-only, deletable. Measures how long it takes to enumerate
// the whole created-playlist list via enable -> scroll -> capture -> reconstruct,
// and where that time goes. The sidebar is virtualized, so it scrolls and
// accumulates. Rows are keyed by their scroll-invariant offset from the section
// header (row.y - header.y), NOT by label: NetEase allows playlists with the same
// name (and AX truncates the visible name), so a label key drops the duplicates;
// the offset is the row's real position and keeps them. The dominant cost is
// scroll settle. NOT hermetic. Read-only except for scrolling the sidebar.
//
// Usage:
//   cargo run -p auv-netease-music --example netease_ax_bench -- \
//     [bundle-id] [max_rounds] [down_delta] [settle_ms]

#[cfg(target_os = "macos")]
fn main() {
  use std::collections::{BTreeMap, HashSet};
  use std::time::{Duration, Instant};

  use auv_driver::Driver;
  use auv_driver::geometry::WindowPoint;
  use auv_driver::input::{InputPolicy, Scroll, ScrollOptions};
  use auv_driver::selector::{App, Window as WindowSel};
  use auv_driver_macos::MacosDriver;
  use auv_driver_macos::native::ax_tree::{
    capture_ax_tree_snapshot, set_app_enhanced_user_interface,
  };
  use auv_netease_music::view_parsers::sidebar::ax_enumerate::{
    created_rows_with_offset, sidebar_projection_from_ax_nodes,
  };
  use auv_netease_music::views::sidebar::SidebarSectionKind;

  // Two observations are the same row if their header-relative offsets are within
  // this many px (rows are ~38-54px apart; cross-capture jitter is a px or two).
  const OFFSET_TOLERANCE: i64 = 15;

  let mut args = std::env::args().skip(1);
  let bundle = args
    .next()
    .unwrap_or_else(|| "com.netease.163music".to_string());
  let max_rounds: usize = args.next().and_then(|v| v.parse().ok()).unwrap_or(80);
  let down_delta: f64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(-350.0);
  let settle_ms: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(350);

  let session = MacosDriver::new().open_local().expect("open_local");
  let window = session
    .window()
    .resolve(WindowSel::main_visible().owned_by(App::bundle(bundle.clone())))
    .expect("resolve NetEase window");
  let anchor = WindowPoint::new(110.0, window.frame.size.height * 0.6);
  let scroll = |delta: f64| {
    let _ = session.window().scroll(
      &window,
      anchor,
      Scroll::new(0.0, delta),
      ScrollOptions {
        policy: InputPolicy::BackgroundPreferred,
        settle: Duration::from_millis(settle_ms),
        ..ScrollOptions::default()
      },
    );
  };

  let probe = capture_ax_tree_snapshot(&bundle, 2, 4).expect("probe (is NetEase running?)");
  let pid = probe.pid as i32;

  let started = Instant::now();
  set_app_enhanced_user_interface(pid, true).expect("enable");
  let enable = started.elapsed();
  std::thread::sleep(Duration::from_millis(900)); // one-time tree build (not counted below)

  // Seek to the top so the created list starts from item 1.
  let started = Instant::now();
  for _ in 0..25 {
    scroll(900.0);
  }
  let top_seek = started.elapsed();

  let trailing_count =
    |label: &str| -> Option<usize> { label.split_whitespace().last().and_then(|t| t.parse().ok()) };

  let mut rows: BTreeMap<i64, String> = BTreeMap::new();
  let mut created_total: Option<usize> = None;
  let mut capture_total = Duration::ZERO;
  let mut reconstruct_total = Duration::ZERO;
  let mut scroll_total = Duration::ZERO;
  let mut rounds = 0usize;
  let mut dry_rounds = 0;

  let enumerate_started = Instant::now();
  for _ in 0..max_rounds {
    rounds += 1;

    let started = Instant::now();
    let capture = capture_ax_tree_snapshot(&bundle, 12, 250).expect("capture");
    capture_total += started.elapsed();

    let started = Instant::now();
    if created_total.is_none() {
      created_total = sidebar_projection_from_ax_nodes(&capture.snapshot.nodes)
        .sections
        .iter()
        .find(|section| section.kind == SidebarSectionKind::MyPlaylists)
        .and_then(|section| section.label.as_deref())
        .and_then(trailing_count);
    }
    let captured = created_rows_with_offset(&capture.snapshot.nodes);
    reconstruct_total += started.elapsed();

    let before = rows.len();
    for (offset, label) in captured {
      let known = rows
        .range(offset - OFFSET_TOLERANCE..=offset + OFFSET_TOLERANCE)
        .next()
        .is_some();
      if !known {
        rows.insert(offset, label);
      }
    }
    dry_rounds = if rows.len() == before {
      dry_rounds + 1
    } else {
      0
    };
    if created_total.is_some_and(|total| rows.len() >= total) || dry_rounds >= 4 {
      break;
    }

    let started = Instant::now();
    scroll(down_delta);
    scroll_total += started.elapsed();
  }
  let enumerate = enumerate_started.elapsed();

  let _ = set_app_enhanced_user_interface(pid, false);

  let distinct = rows.values().collect::<HashSet<_>>().len();
  let per_round = |total: Duration| total / rounds.max(1) as u32;
  println!(
    "== AX playlist full-enumeration bench (down_delta={down_delta}, settle={settle_ms}ms) =="
  );
  println!(
    "collected {} rows ({distinct} distinct labels) / {} declared in {enumerate:?} over {rounds} rounds",
    rows.len(),
    created_total
      .map(|total| total.to_string())
      .unwrap_or("?".into()),
  );
  println!("enable (1x):    {enable:?}");
  println!("top-seek:       {top_seek:?}  (25 scrolls)");
  println!(
    "capture  sum:   {capture_total:?}  (avg {:?}/round)",
    per_round(capture_total)
  );
  println!(
    "reconstruct:    {reconstruct_total:?}  (avg {:?}/round)",
    per_round(reconstruct_total)
  );
  println!(
    "scroll+settle:  {scroll_total:?}  (avg {:?}/round)",
    per_round(scroll_total)
  );

  println!("\n== created playlists ({}) ==", rows.len());
  for (index, label) in rows.values().enumerate() {
    println!("{:>3}. {label}", index + 1);
  }
}

#[cfg(not(target_os = "macos"))]
fn main() {
  eprintln!("netease_ax_bench is macOS-only");
}
