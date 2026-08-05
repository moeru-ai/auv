use super::*;

#[tokio::test]
async fn selected_now_playing_rejects_application_target_before_daemon_resolution() {
  let error = invoke(
    crate::InvokeCommandInput {
      command_id: "mediaControl.nowPlaying".to_string(),
      target_application_id: Some("com.example.Player".to_string()),
      inputs: Default::default(),
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    },
    Default::default(),
  )
  .await
  .expect_err("now-playing target must fail before daemon resolution");
  assert_eq!(error, "mediaControl.nowPlaying cannot use --target; the macOS now-playing state is system-wide");
}

#[tokio::test]
async fn selected_media_commands_reject_application_target_before_daemon_resolution() {
  for command_id in [
    "mediaControl.play",
    "mediaControl.pause",
    "mediaControl.togglePlayPause",
    "mediaControl.next",
    "mediaControl.previous",
  ] {
    let error = invoke(
      crate::InvokeCommandInput {
        command_id: command_id.to_string(),
        target_application_id: Some("com.example.Player".to_string()),
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
      Default::default(),
    )
    .await
    .expect_err("target must fail before daemon resolution");
    assert_eq!(error, format!("{command_id} cannot use --target; macOS media controls are system-wide"));
  }
}

#[tokio::test]
async fn selected_disabled_overlay_validates_without_resolving_a_daemon() {
  let arguments = [
    "overlay.outline",
    "--x",
    "10",
    "--y",
    "20",
    "--width",
    "30",
    "--height",
    "40",
    "--no-overlay",
  ]
  .into_iter()
  .map(str::to_string)
  .collect::<Vec<_>>();
  let crate::InvokeCliParse::Invoke {
    command_id,
    target_application_id,
    inputs,
    typed_args,
    dry_run,
    ..
  } = crate::parse_invoke_args(&arguments).expect("parse overlay")
  else {
    panic!("expected overlay invocation");
  };
  let output = invoke(
    crate::InvokeCommandInput {
      command_id,
      target_application_id,
      inputs,
      typed_args: Some(typed_args),
      dry_run,
      cancellation: Default::default(),
    },
    Default::default(),
  )
  .await
  .expect("disabled overlay must not require daemon discovery");
  assert_eq!(output.report.expect("overlay report").fields.last().expect("overlay status").value, "disabled");
}

#[tokio::test]
async fn selected_text_wait_retries_until_the_first_matching_response() {
  let mut responses = std::collections::VecDeque::from([Vec::<u8>::new(), vec![1]]);
  let calls = std::cell::Cell::new(0);
  let response = wait_for_selected_text(
    "screen.waitForText",
    "Ready",
    auv_driver::WaitOptions {
      timeout: std::time::Duration::from_secs(1),
      poll_interval: std::time::Duration::ZERO,
    },
    &Default::default(),
    || {
      calls.set(calls.get() + 1);
      let response = responses.pop_front().expect("fixture response");
      async move { Ok(response) }
    },
    |response| !response.is_empty(),
  )
  .await
  .expect("second response matches");

  assert_eq!(response, vec![1]);
  assert_eq!(calls.get(), 2);
}

#[tokio::test]
async fn selected_text_wait_preserves_timeout_semantics_after_one_exact_call() {
  let calls = std::cell::Cell::new(0);
  let error = wait_for_selected_text(
    "window.waitForText",
    "Ready",
    auv_driver::WaitOptions {
      timeout: std::time::Duration::ZERO,
      poll_interval: std::time::Duration::ZERO,
    },
    &Default::default(),
    || {
      calls.set(calls.get() + 1);
      async { Ok(Vec::<u8>::new()) }
    },
    |response| !response.is_empty(),
  )
  .await
  .expect_err("empty response at the deadline times out");

  assert_eq!(error, "window.waitForText did not find text \"Ready\" before timeout");
  assert_eq!(calls.get(), 1);
}

#[test]
fn selected_window_selector_keeps_hierarchical_parent_context() {
  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("title".to_string(), "Preferences".to_string());
  let selector = selected_window_selector(&crate::InvokeCommandInput {
    command_id: "window.capture".to_string(),
    target_application_id: Some("com.example.app".to_string()),
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  });

  assert_eq!(selector.app, Some(auv_driver::App::bundle("com.example.app")));
  assert_eq!(selector.title, Some(auv_driver::TextMatcher::Contains("Preferences".to_string())));
}

#[test]
fn selected_window_point_projects_relative_coordinates_and_click_policy() {
  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("relative-x".to_string(), "0.25".to_string());
  inputs.insert("relative-y".to_string(), "0.5".to_string());
  inputs.insert("input-policy".to_string(), "background-only".to_string());
  inputs.insert("click-count".to_string(), "2".to_string());
  inputs.insert("click-interval-ms".to_string(), "80".to_string());
  let input = crate::InvokeCommandInput {
    command_id: "input.clickWindowPoint".to_string(),
    target_application_id: None,
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };
  let window = auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: "window_fixture".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: auv_driver::Rect::new(10.0, 20.0, 400.0, 200.0),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };

  assert_eq!(selected_window_point(&input, &window).unwrap(), auv_driver::WindowPoint::new(100.0, 100.0));
  let options = selected_click_options(&input).unwrap();
  assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundOnly);
  assert_eq!(options.click.count(), 2);
  assert_eq!(options.click.interval(), Some(std::time::Duration::from_millis(80)));
}

#[test]
fn selected_screen_point_preserves_logical_coordinates() {
  let input = crate::InvokeCommandInput {
    command_id: "input.clickScreenPoint".to_string(),
    target_application_id: None,
    inputs: std::collections::BTreeMap::from([
      ("x".to_string(), "768".to_string()),
      ("y".to_string(), "1139.5".to_string()),
    ]),
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };

  assert_eq!(selected_screen_point(&input, "input.clickScreenPoint").unwrap(), auv_driver::ScreenPoint::new(768.0, 1139.5));
}

#[test]
fn selected_screen_text_click_defaults_to_foreground_input() {
  let input = crate::InvokeCommandInput {
    command_id: "screen.clickText".to_string(),
    target_application_id: None,
    inputs: std::collections::BTreeMap::new(),
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };

  let options = selected_click_options(&input).expect("screen click options");
  assert_eq!(options.policy, auv_driver::InputPolicy::ForegroundPreferred);
  assert_eq!(options.click, auv_driver::Click::Single);
}

#[test]
fn selected_window_text_click_projects_screen_match_and_reuses_click_options() {
  let window = auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: "window_fixture".to_string(),
    },
    title: None,
    app_name: None,
    app_bundle_id: None,
    process_id: None,
    frame: auv_driver::Rect::new(100.0, 200.0, 400.0, 300.0),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: true,
    is_visible: true,
  };
  let matched = auv_driver::OcrMatch {
    text: "Play".to_string(),
    confidence: 0.9,
    bounds: auv_driver::Rect::new(140.0, 250.0, 80.0, 20.0),
  };
  assert_eq!(matched_window_point(&window, &matched).unwrap(), auv_driver::WindowPoint::new(80.0, 60.0));

  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("input-policy".to_string(), "foreground-preferred".to_string());
  inputs.insert("click-count".to_string(), "3".to_string());
  inputs.insert("click-interval-ms".to_string(), "60".to_string());
  inputs.insert("index".to_string(), "1".to_string());
  let input = crate::InvokeCommandInput {
    command_id: "window.clickText".to_string(),
    target_application_id: None,
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };
  assert_eq!(
    selected_click_options(&input).unwrap(),
    auv_driver::ClickOptions {
      policy: auv_driver::InputPolicy::ForegroundPreferred,
      click: auv_driver::Click::Repeated {
        count: 3,
        interval: std::time::Duration::from_millis(60),
      },
      window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
    }
  );
}
