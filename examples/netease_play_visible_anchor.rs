use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use auv_cli::recorded_operation::RecordedOperationContext;
use auv_cli::run_builder::{Attributes, RunSpec};
use auv_cli::trace::{RunType, string_attr};
use auv_driver::capture::{Activation, Capture, CaptureOptions};
use auv_driver::input::{Click, PasteTextOptions, TextSubmit};
use auv_driver::selector::{App, Window};
use auv_driver::vision::TextRecognition;
use auv_driver::{Driver, Point, RatioRect};
use auv_driver_macos::MacosDriver;

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
    result_region: RatioRect::new(0.12, 0.14, 0.74, 0.64),
    player_region: RatioRect::new(0.03, 0.86, 0.32, 0.14),
  })
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

  context.in_span("auv.example.netease.prepare", |context| {
    let mut before = session.window().capture_with(
      &window,
      CaptureOptions {
        activation: Activation::ActivateFirst {
          settle: Duration::from_millis(200),
        },
        ..CaptureOptions::default()
      },
    )?;
    let cancel_matches =
      session
        .vision()
        .find_text_in_capture(&before, "取消", full_window_region)?;
    if let Some(cancel) = cancel_matches.best_match() {
      session
        .input()
        .click_at(cancel.action_point(), Click::Single)?;
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
      let collapse_point = window_relative_point(
        &window,
        inputs.collapse_relative_x,
        inputs.collapse_relative_y,
      );
      session.input().click_at(collapse_point, Click::Single)?;
      std::thread::sleep(Duration::from_millis(500));
      focus_capture = session.window().capture(&window)?;
      record_capture(context, "after-collapse-to-main", &focus_capture)?;
    }

    Ok::<_, Box<dyn std::error::Error>>(())
  })?;

  let selected_point = context.in_span("auv.example.netease.search", |context| {
    let search_point =
      window_relative_point(&window, inputs.search_relative_x, inputs.search_relative_y);

    session.input().click_at(search_point, Click::Single)?;
    std::thread::sleep(Duration::from_millis(500));
    let after_search_click = session.window().capture(&window)?;
    record_capture(context, "after-search-click", &after_search_click)?;

    session.input().paste_text(PasteTextOptions {
      text: inputs.query.clone(),
      replace_existing: true,
      submit: TextSubmit::Return,
      settle: Duration::from_millis(inputs.submit_settle_ms),
    })?;

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

    let title_matches = search_text.find_contains(&inputs.result_title);
    let selected = title_matches.get(inputs.result_index).ok_or_else(|| {
      format!(
        "result title was not visible for playback activation at index {}",
        inputs.result_index
      )
    })?;
    Ok::<_, Box<dyn std::error::Error>>(selected.action_point())
  })?;

  context.in_span("auv.example.netease.playback", |context| {
    session.input().click_at(
      selected_point,
      Click::Double {
        interval: Duration::from_millis(inputs.click_interval_ms),
      },
    )?;
    std::thread::sleep(Duration::from_millis(inputs.activation_settle_ms));

    let after_play = session.window().capture_with(
      &window,
      CaptureOptions {
        activation: Activation::ActivateFirst {
          settle: Duration::from_millis(200),
        },
        ..CaptureOptions::default()
      },
    )?;
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

fn window_relative_point(window: &auv_driver::Window, relative_x: f64, relative_y: f64) -> Point {
  Point::new(
    window.frame.origin.x + window.frame.size.width * relative_x,
    window.frame.origin.y + window.frame.size.height * relative_y,
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
