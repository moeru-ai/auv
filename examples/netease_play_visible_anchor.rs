use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_driver::capture::Capture;
use auv_driver::input::{Click, ClickOptions, TextSubmit, TypeTextOptions, WindowClickStrategy};
use auv_driver::selector::{App, Window};
use auv_driver::vision::{RecognizedText, TextRecognition};
use auv_driver::{Driver, RatioRect, ScreenPoint, WindowPoint};
use auv_driver_macos::MacosDriver;
use auv_tracing_driver::recorded_operation::RecordedOperationContext;
use auv_tracing_driver::run_builder::{Attributes, RunSpec};
use auv_tracing_driver::trace::{RunType, string_attr};

static TEMP_CAPTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Inputs {
  app_id: String,
  query: String,
  result_title: String,
  result_artist: String,
  result_index: usize,
  search_relative_x: f64,
  search_relative_y: f64,
  main_marker: String,
  collapse_relative_x: f64,
  collapse_relative_y: f64,
  click_interval_ms: u64,
  activation_settle_ms: u64,
  submit_settle_ms: u64,
  show_overlay: bool,
  overlay_pause_ms: u64,
  result_region: RatioRect,
  player_region: RatioRect,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let inputs = parse_inputs(std::env::args().skip(1).collect())?;
  run_recorded(inputs)
}

fn parse_inputs(args: Vec<String>) -> Result<Inputs, String> {
  let mut app_id = "com.netease.163music".to_string();
  let mut query = "AURORA Cure For Me".to_string();
  let mut result_title = "Cure For Me".to_string();
  let mut result_artist = "AURORA".to_string();
  let mut result_index = 0_usize;
  let mut search_relative_x = 0.31_f64;
  let mut search_relative_y = 0.06_f64;
  let mut main_marker = "网易云音乐".to_string();
  let mut collapse_relative_x = 0.05_f64;
  let mut collapse_relative_y = 0.015_f64;
  let mut click_interval_ms = 80_u64;
  let mut submit_settle_ms = 1200_u64;
  let mut activation_settle_ms = 500_u64;
  let mut show_overlay = true;
  let mut overlay_pause_ms = 450_u64;

  let mut iter = args.into_iter();
  while let Some(flag) = iter.next() {
    let value = iter
      .next()
      .ok_or_else(|| format!("{flag} requires a value"))?;
    match flag.as_str() {
      "--app-id" => app_id = value,
      "--query" => query = value,
      "--main-marker" => main_marker = value,
      "--result-title" => result_title = value,
      "--result-artist" => result_artist = value,
      "--result-index" => {
        result_index = value
          .parse()
          .map_err(|error| format!("invalid --result-index: {error}"))?;
      }
      "--click-interval-ms" => {
        click_interval_ms = value
          .parse()
          .map_err(|error| format!("invalid --click-interval-ms: {error}"))?;
      }
      "--search-relative-x" => {
        search_relative_x = value
          .parse()
          .map_err(|error| format!("invalid --search-relative-x: {error}"))?;
      }
      "--search-relative-y" => {
        search_relative_y = value
          .parse()
          .map_err(|error| format!("invalid --search-relative-y: {error}"))?;
      }
      "--collapse-relative-x" => {
        collapse_relative_x = value
          .parse()
          .map_err(|error| format!("invalid --collapse-relative-x: {error}"))?;
      }
      "--collapse-relative-y" => {
        collapse_relative_y = value
          .parse()
          .map_err(|error| format!("invalid --collapse-relative-y: {error}"))?;
      }
      "--submit-settle-ms" => {
        submit_settle_ms = value
          .parse()
          .map_err(|error| format!("invalid --submit-settle-ms: {error}"))?;
      }
      "--activation-settle-ms" => {
        activation_settle_ms = value
          .parse()
          .map_err(|error| format!("invalid --activation-settle-ms: {error}"))?;
      }
      "--show-overlay" => show_overlay = parse_bool(&value)?,
      "--overlay-pause-ms" => {
        overlay_pause_ms = value
          .parse()
          .map_err(|error| format!("invalid --overlay-pause-ms: {error}"))?;
      }
      other => return Err(format!("unknown argument {other}")),
    }
  }

  Ok(Inputs {
    app_id,
    query,
    result_title,
    result_artist,
    result_index,
    search_relative_x,
    search_relative_y,
    main_marker,
    collapse_relative_x,
    collapse_relative_y,
    click_interval_ms,
    activation_settle_ms,
    submit_settle_ms,
    show_overlay,
    overlay_pause_ms,
    result_region: RatioRect::new(0.12, 0.14, 0.74, 0.64),
    player_region: RatioRect::new(0.03, 0.86, 0.32, 0.14),
  })
}

fn parse_bool(value: &str) -> Result<bool, String> {
  match value {
    "true" | "1" | "yes" | "on" => Ok(true),
    "false" | "0" | "no" | "off" => Ok(false),
    other => Err(format!("invalid boolean value {other:?}")),
  }
}

fn run_recorded(inputs: Inputs) -> Result<(), Box<dyn std::error::Error>> {
  let project_root = std::env::current_dir()?;
  let runtime = auv_cli::build_default_runtime(project_root.clone())?;
  let mut attributes = Attributes::new();
  attributes.insert(
    "auv.example.id".to_string(),
    string_attr("netease_play_visible_anchor"),
  );
  attributes.insert(
    "auv.target.application_id".to_string(),
    string_attr(inputs.app_id.clone()),
  );
  attributes.insert(
    "auv.example.query".to_string(),
    string_attr(inputs.query.clone()),
  );

  let recorded = runtime.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.example.netease_play_visible_anchor")
      .with_attributes(attributes),
    "NetEase visible anchor example",
    |context| run_steps(context, inputs),
  )?;
  println!("recorded run: {}", recorded.run_dir.display());
  Ok(())
}

fn run_steps(
  context: &mut RecordedOperationContext<'_>,
  inputs: Inputs,
) -> Result<(), Box<dyn std::error::Error>> {
  let driver = MacosDriver::new();
  let session = driver.open_local()?;
  let app = App::bundle(inputs.app_id.clone());
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(app))?;
  let full_window_region = RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let overlay = AnchorOverlay::new(inputs.show_overlay, inputs.overlay_pause_ms);

  context.in_span("auv.example.netease.prepare", |context| {
    let mut before = session.window().capture(&window)?;
    let cancel_matches =
      session
        .vision()
        .find_text_in_capture(&before, "取消", full_window_region)?;
    if let Some(cancel) = cancel_matches.best_match() {
      let cancel_point = session
        .window()
        .to_window_point(&window, ScreenPoint::from(cancel.action_point()))?;
      overlay.before_click(&session, &window, cancel_point, "cancel")?;
      session.window().click(
        &window,
        cancel_point,
        ClickOptions {
          click: Click::Single,
          window_strategy: WindowClickStrategy::ChromiumCompatible,
          ..ClickOptions::default()
        },
      )?;
      overlay.after_click(&session, &window, cancel_point, "cancel")?;
      std::thread::sleep(Duration::from_millis(500));
      before = session.window().capture(&window)?;
    }
    record_capture(context, "before-search", &before)?;

    let mut focus_capture = before;
    let marker_matches = session.vision().find_text_in_capture(
      &focus_capture,
      &inputs.main_marker,
      full_window_region,
    )?;
    if marker_matches.best_match().is_none() {
      let collapse_point = window_relative_window_point(
        &window,
        inputs.collapse_relative_x,
        inputs.collapse_relative_y,
      );
      overlay.before_click(&session, &window, collapse_point, "collapse")?;
      session.window().click(
        &window,
        collapse_point,
        ClickOptions {
          click: Click::Single,
          window_strategy: WindowClickStrategy::ChromiumCompatible,
          ..ClickOptions::default()
        },
      )?;
      overlay.after_click(&session, &window, collapse_point, "collapse")?;
      std::thread::sleep(Duration::from_millis(500));
      focus_capture = session.window().capture(&window)?;
      record_capture(context, "after-collapse-to-main", &focus_capture)?;
    }

    Ok::<_, Box<dyn std::error::Error>>(())
  })?;

  let selected_point = context.in_span("auv.example.netease.search", |context| {
    let search_point =
      window_relative_window_point(&window, inputs.search_relative_x, inputs.search_relative_y);

    // WORKAROUND:
    //
    // NetEase Cloud Music's CEF search entry can report a successful
    // pid-targeted mouse delivery without actually entering text-input state
    // after a single background click. A double click at the visible search
    // anchor did focus the field during live testing, and the subsequent
    // window-targeted keyboard path accepted text without foregrounding the app.
    //
    // Verification used the temporary `/private/tmp/auv-post-pid-probe` harness:
    // `AUV_CLICK_REL_X=0.31 AUV_CLICK_REL_Y=0.06 AUV_TYPE_TEXT=auv_probe_double_type cargo run --quiet -- com.netease.163music`
    // followed by this example with `--search-relative-y 0.045`. Remove this
    // workaround only after NetEase accepts a single Chromium-compatible click
    // plus background typing on the same visible anchor.
    overlay.before_click(&session, &window, search_point, "search")?;
    session.window().click(
      &window,
      search_point,
      ClickOptions {
        click: Click::Double {
          interval: Duration::from_millis(inputs.click_interval_ms),
        },
        window_strategy: WindowClickStrategy::ChromiumCompatible,
        ..ClickOptions::default()
      },
    )?;
    overlay.after_click(&session, &window, search_point, "search")?;
    std::thread::sleep(Duration::from_millis(500));
    let after_search_click = session.window().capture(&window)?;
    record_capture(context, "after-search-click", &after_search_click)?;

    overlay.mark_point(&session, &window, search_point, "type target")?;
    session.window().type_text(
      &window,
      &inputs.query,
      TypeTextOptions {
        replace_existing: true,
        submit: TextSubmit::Return,
        inter_char_delay: Duration::from_millis(8),
        allow_clipboard_fallback: false,
        settle: Duration::from_millis(inputs.submit_settle_ms),
        ..TypeTextOptions::default()
      },
    )?;

    let after_search = session.window().capture(&window)?;
    record_capture(context, "after-search", &after_search)?;
    let search_text = session
      .vision()
      .recognize_text_in_capture(&after_search, inputs.result_region)?;
    expect_text_visible(
      &search_text,
      &inputs.result_artist,
      "result artist on comprehensive results",
    )?;
    expect_text_visible(
      &search_text,
      &inputs.result_title,
      "result title on comprehensive results",
    )?;

    let selected = select_song_result(&search_text, &inputs.result_title, inputs.result_index)?;
    let selected_point = session
      .window()
      .to_window_point(&window, ScreenPoint::from(selected.action_point()))?;
    Ok::<_, Box<dyn std::error::Error>>(selected_point)
  })?;

  context.in_span("auv.example.netease.playback", |context| {
    overlay.before_click(&session, &window, selected_point, "play result")?;
    session.window().click(
      &window,
      selected_point,
      ClickOptions {
        click: Click::Double {
          interval: Duration::from_millis(inputs.click_interval_ms),
        },
        window_strategy: WindowClickStrategy::ChromiumCompatible,
        ..ClickOptions::default()
      },
    )?;
    overlay.after_click(&session, &window, selected_point, "play result")?;
    std::thread::sleep(Duration::from_millis(inputs.activation_settle_ms));

    let after_play = session.window().capture(&window)?;
    record_capture(context, "after-play", &after_play)?;
    let player_text = session
      .vision()
      .recognize_text_in_capture(&after_play, inputs.player_region)?;
    expect_text_visible(&player_text, &inputs.result_title, "player title")?;
    expect_text_visible(&player_text, &inputs.result_artist, "player artist")?;

    Ok::<_, Box<dyn std::error::Error>>(())
  })?;

  Ok(())
}

struct AnchorOverlay {
  enabled: bool,
  pause_ms: u64,
}

impl AnchorOverlay {
  const fn new(enabled: bool, pause_ms: u64) -> Self {
    Self { enabled, pause_ms }
  }

  fn mark_point(
    &self,
    session: &auv_driver_macos::MacosDriverSession,
    window: &auv_driver::Window,
    point: WindowPoint,
    label: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    if !self.enabled {
      return Ok(());
    }
    let screen = session.window().to_screen_point(window, point)?.point();
    auv_overlay_macos::set_cursor("netease-visible-anchor", screen.x, screen.y, label, "auv")?;
    auv_overlay_macos::pump_events(self.pause_ms)?;
    Ok(())
  }

  fn before_click(
    &self,
    session: &auv_driver_macos::MacosDriverSession,
    window: &auv_driver::Window,
    point: WindowPoint,
    label: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    self.mark_point(session, window, point, &format!("{label} before"))
  }

  fn after_click(
    &self,
    session: &auv_driver_macos::MacosDriverSession,
    window: &auv_driver::Window,
    point: WindowPoint,
    label: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    if !self.enabled {
      return Ok(());
    }
    let screen = session.window().to_screen_point(window, point)?.point();
    auv_overlay_macos::flash_cursor_id(
      "netease-visible-anchor",
      screen.x,
      screen.y,
      &format!("{label} fired"),
      350,
    )?;
    auv_overlay_macos::pump_events(450)?;
    Ok(())
  }
}

impl Drop for AnchorOverlay {
  fn drop(&mut self) {
    if self.enabled {
      let _ = auv_overlay_macos::hide_cursor_id("netease-visible-anchor");
      let _ = auv_overlay_macos::pump_events(100);
    }
  }
}

fn record_capture(
  context: &mut RecordedOperationContext<'_>,
  label: &str,
  capture: &Capture,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
  let path = temp_png_path(label);
  capture.image.save(&path)?;
  let staged = context.stage_artifact_file(
    "screenshot",
    &path,
    format!("{label}.png"),
    Some(format!("NetEase example {label} window capture")),
  )?;
  let _ = std::fs::remove_file(path);
  Ok(staged)
}

fn window_relative_window_point(
  window: &auv_driver::Window,
  relative_x: f64,
  relative_y: f64,
) -> WindowPoint {
  WindowPoint::new(
    window.frame.size.width * relative_x,
    window.frame.size.height * relative_y,
  )
}

fn temp_png_path(label: &str) -> PathBuf {
  let millis = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis();
  let sequence = TEMP_CAPTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
  std::env::temp_dir().join(format!(
    "auv-netease-example-{label}-{}-{millis}-{sequence}.png",
    std::process::id()
  ))
}

fn expect_text_visible(
  recognition: &TextRecognition,
  query: &str,
  label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  if recognition.best_contains(query).is_none() {
    Err(format!("{label} was not visible").into())
  } else {
    Ok(())
  }
}

fn select_song_result<'a>(
  recognition: &'a TextRecognition,
  title: &str,
  index: usize,
) -> Result<&'a RecognizedText, Box<dyn std::error::Error>> {
  let song_section_y = recognition
    .best_contains("单曲")
    .map(|section| section.bounds.origin.y)
    .unwrap_or(f64::NEG_INFINITY);
  let title_matches = recognition
    .find_contains(title)
    .into_iter()
    .filter(|candidate| candidate.bounds.origin.y > song_section_y)
    .collect::<Vec<_>>();
  title_matches.get(index).copied().ok_or_else(|| {
    format!("song result title was not visible for playback activation at index {index}").into()
  })
}
