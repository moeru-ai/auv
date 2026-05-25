use std::time::Duration;

use auv_driver::capture::{Activation, CaptureOptions};
use auv_driver::input::{Click, PasteTextOptions, Submit, WaitOptions};
use auv_driver::selector::{App, Window};
use auv_driver::{Driver, RatioRect};
use auv_driver_macos::MacosDriver;

struct Inputs {
  app_id: String,
  query: String,
  search_anchor: String,
  result_title: String,
  result_artist: String,
  result_index: usize,
  search_region: RatioRect,
  result_region: RatioRect,
  player_region: RatioRect,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let inputs = parse_inputs(std::env::args().skip(1).collect())?;
  run(inputs)
}

fn parse_inputs(args: Vec<String>) -> Result<Inputs, String> {
  let mut app_id = "com.netease.163music".to_string();
  let mut query = "AURORA Cure For Me".to_string();
  let mut search_anchor = "搜索".to_string();
  let mut result_title = "Cure For Me".to_string();
  let mut result_artist = "AURORA".to_string();
  let mut result_index = 0_usize;

  let mut iter = args.into_iter();
  while let Some(flag) = iter.next() {
    let value = iter
      .next()
      .ok_or_else(|| format!("{flag} requires a value"))?;
    match flag.as_str() {
      "--app-id" => app_id = value,
      "--query" => query = value,
      "--search-anchor" => search_anchor = value,
      "--result-title" => result_title = value,
      "--result-artist" => result_artist = value,
      "--result-index" => {
        result_index = value
          .parse()
          .map_err(|error| format!("invalid --result-index: {error}"))?;
      }
      other => return Err(format!("unknown argument {other}")),
    }
  }

  Ok(Inputs {
    app_id,
    query,
    search_anchor,
    result_title,
    result_artist,
    result_index,
    search_region: RatioRect::new(0.20, 0.0, 0.42, 0.25),
    result_region: RatioRect::new(0.12, 0.14, 0.74, 0.64),
    player_region: RatioRect::new(0.03, 0.86, 0.32, 0.14),
  })
}

fn run(inputs: Inputs) -> Result<(), Box<dyn std::error::Error>> {
  let driver = MacosDriver::new();
  let session = driver.open_local()?;
  let app = App::bundle(inputs.app_id.clone());
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(app))?;
  let wait = WaitOptions {
    timeout: Duration::from_millis(3500),
    poll_interval: Duration::from_millis(250),
  };

  let _before = session.window().capture_with(
    &window,
    CaptureOptions {
      activation: Activation::ActivateFirst {
        settle: Duration::from_millis(200),
      },
      ..CaptureOptions::default()
    },
  )?;

  let search_matches =
    session
      .window()
      .find_text(&window, &inputs.search_anchor, inputs.search_region, wait)?;
  let search_box = search_matches
    .best_match()
    .ok_or("search anchor not found")?;
  session
    .input()
    .click_at(search_box.action_point(), Click::Single)?;

  session.clipboard().paste_text(PasteTextOptions {
    text: inputs.query.clone(),
    submit: Submit::Enter,
  })?;

  session
    .window()
    .wait_text(&window, &inputs.query, inputs.search_region, wait)?;

  let after_search = session.window().capture(&window)?;
  let result_matches = session.vision().find_text_in_capture(
    &after_search,
    &inputs.result_title,
    inputs.result_region,
  )?;
  let selected = result_matches
    .matches
    .get(inputs.result_index)
    .ok_or("requested result index was not visible")?;
  expect_visible(
    session.vision().find_text_in_capture(
      &after_search,
      &inputs.result_artist,
      inputs.result_region,
    )?,
    "result artist",
  )?;
  session.input().click_at(
    selected.action_point(),
    Click::Double {
      interval: Duration::from_millis(80),
    },
  )?;

  std::thread::sleep(Duration::from_millis(1200));
  let after_play = session.window().capture_with(
    &window,
    CaptureOptions {
      activation: Activation::ActivateFirst {
        settle: Duration::from_millis(200),
      },
      ..CaptureOptions::default()
    },
  )?;
  expect_visible(
    session.vision().find_text_in_capture(
      &after_play,
      &inputs.result_title,
      inputs.player_region,
    )?,
    "player title",
  )?;
  expect_visible(
    session.vision().find_text_in_capture(
      &after_play,
      &inputs.result_artist,
      inputs.player_region,
    )?,
    "player artist",
  )?;

  Ok(())
}

fn expect_visible(
  matches: auv_driver_macos::OcrMatches,
  label: &str,
) -> Result<(), Box<dyn std::error::Error>> {
  if matches.matches.is_empty() {
    Err(format!("{label} was not visible").into())
  } else {
    Ok(())
  }
}
