//! Typed invoke execution through a selected daemon-owned Runner.

async fn wait_for_selected_text<R, Call, Future, HasMatches>(
  command_id: &str,
  query: &str,
  options: auv_driver::WaitOptions,
  cancellation: &crate::InvokeCancellation,
  mut call: Call,
  has_matches: HasMatches,
) -> Result<R, String>
where
  Call: FnMut() -> Future,
  Future: std::future::Future<Output = Result<R, String>>,
  HasMatches: Fn(&R) -> bool,
{
  let started = std::time::Instant::now();
  loop {
    cancellation.check().map_err(|error| error.to_string())?;
    let response = call().await?;
    if has_matches(&response) {
      return Ok(response);
    }
    if started.elapsed() >= options.timeout {
      return Err(format!("{command_id} did not find text {query:?} before timeout"));
    }
    tokio::select! {
      _ = cancellation.cancelled() => return Err("invoke cancelled".to_string()),
      _ = tokio::time::sleep(options.poll_interval) => {}
    }
  }
}

pub async fn invoke(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeCommandResult {
  let command_id = input.command_id.as_str();
  if command_id == "app.probePermissions" && input.target_application_id.is_some() {
    return Err("app.probePermissions cannot use --target".to_string());
  }
  if command_id == "app.activate" && input.target_application_id.as_ref().is_none_or(|target| target.trim().is_empty()) {
    return Err("app.activate requires --target".to_string());
  }
  if matches!(command_id, "input.focusText" | "input.axFocusText")
    && input.target_application_id.as_ref().is_none_or(|target| target.trim().is_empty())
  {
    return Err(format!("{command_id} requires --target"));
  }
  if command_id.starts_with("mediaControl.") && input.target_application_id.is_some() {
    return Err(if command_id == "mediaControl.nowPlaying" {
      "mediaControl.nowPlaying cannot use --target; the macOS now-playing state is system-wide".to_string()
    } else {
      format!("{command_id} cannot use --target; macOS media controls are system-wide")
    });
  }
  if command_id.starts_with("overlay.") && input.target_application_id.is_some() {
    return Err(format!("{command_id} cannot use --target; overlays use global screen coordinates"));
  }
  if command_id.starts_with("overlay.") {
    let plan = crate::commands::overlay::plan_overlay(&input)?;
    if input.dry_run || !input.overlay_enabled()? {
      return crate::commands::overlay::selected_overlay_output(&plan, false);
    }
  }
  if matches!(command_id, "input.typeText" | "input.pasteText" | "input.key") && input.target_application_id.is_some() {
    return Err(format!("{command_id} cannot use --target until typed input target activation is available"));
  }
  if matches!(command_id, "screen.findText" | "screen.waitForText" | "screen.clickText" | "screen.captureRegion")
    && input.target_application_id.is_some()
  {
    return Err(format!("{command_id} cannot use --target until typed target activation is available"));
  }
  let auv = auv::Client::from_context(context).await.map_err(|error| error.to_string())?;
  let run = auv.run(Default::default()).await.map_err(|error| format!("resolve selected Run failed: {error}"))?;
  let runner = run
    .runner(auv::client::RunnerOptions::default())
    .await
    .map_err(|error| format!("route core Runner for {command_id} failed: {error}"))?;

  let invoked = match command_id {
    "app.activate" => {
      let target = input.target_application_id.as_deref().expect("validated target").trim();
      runner
        .macos()
        .applications()
        .activate_bundle_id(target, std::time::Duration::from_millis(150))
        .await
        .map_err(|status| format!("ApplicationService/ActivateBundleId failed: {status}"))
        .and_then(|result| {
          if result.requested_bundle_id != target {
            return Err("ActivateBundleId response changed the requested bundle id".to_string());
          }
          crate::commands::app::activation_output(&result)
        })
    }
    "app.probePermissions" => runner
      .macos()
      .permissions()
      .probe()
      .await
      .map_err(|status| format!("PermissionService/ProbePermissions failed: {status}"))
      .and_then(|probe| crate::commands::app::permission_probe_output(&probe)),
    "input.focusText" | "input.axFocusText" => {
      let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
      let selector = if candidate.trim().is_empty() {
        auv_driver::AxTextSelector::Query(input.inputs.get("query").cloned().unwrap_or_default())
      } else {
        auv_driver::AxTextSelector::Path(candidate.clone())
      };
      runner
        .macos()
        .accessibility()
        .focus_text(auv_driver::FocusTextOptions {
          app: input.target_application_id.clone().expect("validated target"),
          selector,
          expected_role: None,
        })
        .await
        .map_err(|status| format!("AccessibilityService/FocusText failed: {status}"))
        .and_then(|result| crate::commands::input::focus_text_output(&result, &candidate))
    }
    "mediaControl.nowPlaying" => runner
      .macos()
      .media()
      .now_playing()
      .await
      .map_err(|status| format!("MediaControlService/GetNowPlaying failed: {status}"))
      .and_then(|state| crate::commands::media_control::now_playing_state_output(&state)),
    "mediaControl.play" => runner
      .macos()
      .media()
      .play()
      .await
      .map_err(|status| format!("MediaControlService/Play failed: {status}"))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome)),
    "mediaControl.pause" => runner
      .macos()
      .media()
      .pause()
      .await
      .map_err(|status| format!("MediaControlService/Pause failed: {status}"))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome)),
    "mediaControl.togglePlayPause" => runner
      .macos()
      .media()
      .toggle_play_pause()
      .await
      .map_err(|status| format!("MediaControlService/TogglePlayPause failed: {status}"))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome)),
    "mediaControl.next" => runner
      .macos()
      .media()
      .next_track()
      .await
      .map_err(|status| format!("MediaControlService/NextTrack failed: {status}"))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome)),
    "mediaControl.previous" => runner
      .macos()
      .media()
      .previous_track()
      .await
      .map_err(|status| format!("MediaControlService/PreviousTrack failed: {status}"))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome)),
    "overlay.outline" | "overlay.cursor" | "overlay.status" | "overlay.captureFrame" | "overlay.clickTarget" => {
      let plan = crate::commands::overlay::plan_overlay(&input)?;
      runner
        .overlay()
        .show(&plan.overlay, plan.options)
        .await
        .map_err(|status| format!("OverlayService/ShowOverlay failed: {status}"))
        .and_then(|()| crate::commands::overlay::selected_overlay_output(&plan, true))
    }
    "display.list" => runner
      .displays()
      .list()
      .await
      .map_err(|status| format!("DisplayService/ListDisplays failed: {status}"))
      .and_then(|displays| crate::commands::display::list_displays_output(&displays)),
    "display.capture" => match runner.displays().capture(None).await {
      Err(status) => Err(format!("CaptureService/CaptureDisplay failed: {status}")),
      Ok(capture) => crate::commands::display::recorded_display_capture_output(&capture).await,
    },
    "screen.captureRegion" => match selected_screen_region(&input) {
      Err(error) => Err(error),
      Ok(region) => match runner.displays().capture_region(region, None).await {
        Err(status) => Err(format!("CaptureService/CaptureRegion failed: {status}")),
        Ok(capture) => crate::commands::screen::recorded_region_capture_output(&capture).await,
      },
    },
    "window.list" => runner
      .windows()
      .list()
      .await
      .map_err(|status| format!("WindowService/ListWindows failed: {status}"))
      .and_then(|windows| crate::commands::window::list_windows_output(&windows)),
    "window.capture" => {
      let selector = selected_window_selector(&input);
      let response = match runner.windows().resolve(selector).await {
        Err(status) => Err(status),
        Ok(window) => window.capture().await,
      };
      match response {
        Err(status) => Err(format!("WindowService/ResolveWindow or CaptureService/CaptureWindow failed: {status}")),
        Ok(response) => {
          crate::commands::window::recorded_window_capture_output(&crate::commands::window::WindowCapture {
            window: response.window,
            capture: response.capture,
          })
          .await
        }
      }
    }
    "window.findText" => match input.inputs.get("query").cloned() {
      None => Err("window.findText omitted its typed query argument".to_string()),
      Some(query) => {
        let response = match runner.windows().resolve(selected_window_selector(&input)).await {
          Err(status) => Err(status),
          Ok(window) => window.find_text(query).await,
        };
        match response {
          Err(status) => Err(format!("WindowService/ResolveWindow or TextRecognitionService/FindWindowText failed: {status}")),
          Ok(response) => crate::commands::window::recorded_window_text_matches_output(
            &crate::commands::window::WindowTextRecognition {
              window: response.window,
              matches: response.matches,
            },
            &response.capture,
          ),
        }
      }
    },
    "window.waitForText" => match input.inputs.get("query").cloned() {
      None => Err("window.waitForText omitted its typed query argument".to_string()),
      Some(query) => match runner.windows().resolve(selected_window_selector(&input)).await {
        Err(status) => Err(format!("WindowService/ResolveWindow failed: {status}")),
        Ok(window) => {
          let response = wait_for_selected_text(
            command_id,
            &query,
            auv_driver::WaitOptions::default(),
            &input.cancellation,
            || {
              let window = window.clone();
              let query = query.clone();
              async move { window.find_text(query).await.map_err(|status| format!("TextRecognitionService/FindWindowText failed: {status}")) }
            },
            |response| !response.matches.matches.is_empty(),
          )
          .await;
          match response {
            Err(error) => Err(error),
            Ok(response) => crate::commands::window::recorded_window_text_matches_output(
              &crate::commands::window::WindowTextRecognition {
                window: response.window,
                matches: response.matches,
              },
              &response.capture,
            ),
          }
        }
      },
    },
    "screen.findText" => match input.inputs.get("query").cloned() {
      None => Err("screen.findText omitted its typed query argument".to_string()),
      Some(query) => {
        let response = runner.displays().find_text(None, query).await;
        match response {
          Err(status) => Err(format!("TextRecognitionService/FindDisplayText failed: {status}")),
          Ok(response) => crate::commands::screen::recorded_screen_text_matches_output(&response.matches, &response.capture),
        }
      }
    },
    "screen.clickText" => {
      async {
        let query = input.inputs.get("query").cloned().ok_or_else(|| "screen.clickText omitted its typed query argument".to_string())?;
        let recognized = runner
          .displays()
          .find_text(None, query.clone())
          .await
          .map_err(|status| format!("TextRecognitionService/FindDisplayText failed: {status}"))?;
        let matches = recognized.matches;
        let capture = recognized.capture;
        let point = matches.best_match().ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?.action_point();
        let click = selected_click_options(&input)?.click;
        let response = runner
          .input()
          .click_screen_point(point, click)
          .await
          .map_err(|status| format!("InputService/ClickScreenPoint failed: {status}"))?;
        let result = crate::commands::screen::ScreenTextClick {
          matches,
          point: response.point,
          action: response.action,
        };
        crate::commands::screen::recorded_screen_text_click_output(&result, &capture)
      }
      .await
    }
    "screen.waitForText" => match input.inputs.get("query").cloned() {
      None => Err("screen.waitForText omitted its typed query argument".to_string()),
      Some(query) => {
        let displays = runner.displays();
        let response = wait_for_selected_text(
          command_id,
          &query,
          auv_driver::WaitOptions::default(),
          &input.cancellation,
          || {
            let displays = displays.clone();
            let query = query.clone();
            async move {
              displays.find_text(None, query).await.map_err(|status| format!("TextRecognitionService/FindDisplayText failed: {status}"))
            }
          },
          |response| !response.matches.matches.is_empty(),
        )
        .await;
        match response {
          Err(error) => Err(error),
          Ok(response) => crate::commands::screen::recorded_screen_text_matches_output(&response.matches, &response.capture),
        }
      }
    },
    "window.clickText" => {
      async {
        let query = input.inputs.get("query").cloned().ok_or_else(|| "window.clickText omitted its typed query argument".to_string())?;
        let selected_index = input
          .inputs
          .get("index")
          .map(|value| value.parse::<usize>().map_err(|error| format!("window.clickText has invalid --index: {error}")))
          .transpose()?
          .unwrap_or(0);
        let resolved = runner
          .windows()
          .resolve(selected_window_selector(&input))
          .await
          .map_err(|status| format!("WindowService/ResolveWindow failed: {status}"))?;
        let resolved_window = resolved.resource().clone();
        let recognized =
          resolved.find_text(query.clone()).await.map_err(|status| format!("TextRecognitionService/FindWindowText failed: {status}"))?;
        let matches = recognized.matches;
        let capture = recognized.capture;
        let matched = crate::commands::window::selected_window_text_match(&matches, &query, selected_index)?;
        let point = matched_window_point(&resolved_window, matched)?;
        let options = selected_click_options(&input)?;
        let response =
          resolved.click(point, options.clone()).await.map_err(|status| format!("InputService/ClickWindowPoint failed: {status}"))?;
        let clicked_window = response.window;
        if clicked_window.reference != resolved_window.reference {
          return Err("ClickWindowPoint response changed the resolved WindowRef".to_string());
        }
        let result = crate::commands::window::WindowTextClick {
          window: clicked_window,
          matches,
          selected_index,
          point: response.point,
          options,
          action: response.action,
        };
        crate::commands::window::recorded_window_text_click_output(&result, &capture)
      }
      .await
    }
    "input.typeText" => match input.inputs.get("text").cloned() {
      None => Err("input.typeText omitted its typed text argument".to_string()),
      Some(text) => runner
        .input()
        .type_text(text, auv_driver::TypeTextOptions::default())
        .await
        .map_err(|status| format!("InputService/TypeText failed: {status}"))
        .and_then(|action| {
          crate::emit_input_action_result(&action);
          crate::commands::input::input_action_output(&action)
        }),
    },
    "input.pasteText" => match input.inputs.get("text").cloned() {
      None => Err("input.pasteText omitted its typed text argument".to_string()),
      Some(text) => runner
        .input()
        .paste_text(auv_driver::PasteTextOptions {
          text,
          ..Default::default()
        })
        .await
        .map_err(|status| format!("InputService/PasteText failed: {status}"))
        .and_then(|action| {
          crate::emit_input_action_result(&action);
          crate::commands::input::input_action_output(&action)
        }),
    },
    "input.key" => match input.inputs.get("key").cloned() {
      None => Err("input.key omitted its typed key argument".to_string()),
      Some(key) => runner
        .input()
        .press_key(auv_driver::KeyPressOptions {
          key: key.clone(),
          settle: std::time::Duration::ZERO,
        })
        .await
        .map_err(|status| format!("InputService/PressKey failed: {status}"))
        .and_then(|action| {
          crate::emit_input_action_result(&action);
          crate::commands::input::press_key_output(&action, &key)
        }),
    },
    "input.moveMouse" => {
      async {
        let point = selected_screen_point(&input, "input.moveMouse")?;
        let mut stream = runner
          .input()
          .move_mouse(auv_driver::MouseMotionPlan::direct(point.point()))
          .await
          .map_err(|status| format!("InputService/MoveMouse failed: {status}"))?;
        while let Some(event) = stream.next().await.map_err(|status| format!("InputService/MoveMouse failed: {status}"))? {
          if let auv::client::runner::MouseMotionEvent::Completed { point, action } = event {
            crate::emit_input_action_result(&action);
            return crate::commands::input::mouse_move_output(crate::commands::input::MouseMoveResult {
              point: auv_driver::ScreenPoint::new(point.x, point.y),
              action: Some(action),
            });
          }
        }
        Err("InputService/MoveMouse ended without completion evidence".to_string())
      }
      .await
    }
    "input.clickScreenPoint" => {
      async {
        let point = selected_screen_point(&input, "input.clickScreenPoint")?;
        let click = selected_click_options(&input)?.click;
        let response = runner
          .input()
          .click_screen_point(point.point(), click)
          .await
          .map_err(|status| format!("InputService/ClickScreenPoint failed: {status}"))?;
        crate::emit_input_action_result(&response.action);
        crate::commands::input::screen_point_click_output(crate::commands::input::ScreenPointClickResult {
          point: auv_driver::ScreenPoint::new(response.point.x, response.point.y),
          action: Some(response.action),
        })
      }
      .await
    }
    "input.clickWindowPoint" => match runner.windows().resolve(selected_window_selector(&input)).await {
      Err(status) => Err(format!("WindowService/ResolveWindow failed: {status}")),
      Ok(resolved) => {
        let window = resolved.resource().clone();
        match (selected_window_point(&input, &window), selected_click_options(&input)) {
          (Err(error), _) | (_, Err(error)) => Err(error),
          (Ok(point), Ok(options)) => match resolved.click(point, options).await {
            Err(status) => Err(format!("InputService/ClickWindowPoint failed: {status}")),
            Ok(response) => {
              crate::emit_input_action_result(&response.action);
              crate::commands::input::window_point_click_output_without_overlay(crate::commands::input::WindowPointClickResult {
                window: response.window,
                point: response.point,
                action: Some(response.action),
              })
            }
          },
        }
      }
    },
    _ => unreachable!("typed Runner adapter was selected above"),
  };
  invoked
}

fn selected_screen_point(input: &crate::InvokeCommandInput, command_id: &str) -> Result<auv_driver::ScreenPoint, String> {
  let number = |name: &str| {
    input
      .inputs
      .get(name)
      .ok_or_else(|| format!("{command_id} omitted its {name} coordinate"))?
      .parse::<f64>()
      .map_err(|error| format!("{command_id} has invalid {name} coordinate: {error}"))
  };
  let x = number("x")?;
  let y = number("y")?;
  if !x.is_finite() || !y.is_finite() {
    return Err(format!("{command_id} requires finite coordinates"));
  }
  Ok(auv_driver::ScreenPoint::new(x, y))
}

fn selected_screen_region(input: &crate::InvokeCommandInput) -> Result<auv_driver::Rect, String> {
  let number = |name: &str| {
    input
      .inputs
      .get(name)
      .ok_or_else(|| format!("screen.captureRegion omitted --{name}"))?
      .parse::<f64>()
      .map_err(|error| format!("screen.captureRegion has invalid --{name}: {error}"))
  };
  let x = number("x")?;
  let y = number("y")?;
  let width = number("width")?;
  let height = number("height")?;
  if !x.is_finite() || !y.is_finite() {
    return Err("screen.captureRegion requires finite --x and --y".to_string());
  }
  if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
    return Err("screen.captureRegion requires --width and --height greater than zero".to_string());
  }
  Ok(auv_driver::Rect::new(x, y, width, height))
}

fn selected_window_point(input: &crate::InvokeCommandInput, window: &auv_driver::Window) -> Result<auv_driver::WindowPoint, String> {
  let number = |name: &str| {
    input
      .inputs
      .get(name)
      .map(|value| value.parse::<f64>().map_err(|error| format!("input.clickWindowPoint has invalid --{name}: {error}")))
      .transpose()
  };
  let offset_x = number("offset-x")?;
  let offset_y = number("offset-y")?;
  let relative_x = number("relative-x")?;
  let relative_y = number("relative-y")?;
  let point = match (offset_x, offset_y, relative_x, relative_y) {
    (Some(x), Some(y), None, None) if x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 => auv_driver::WindowPoint::new(x, y),
    (None, None, Some(x), Some(y)) if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => {
      auv_driver::WindowPoint::new(window.frame.size.width * x, window.frame.size.height * y)
    }
    (Some(_), Some(_), None, None) => return Err("input.clickWindowPoint requires finite non-negative window offsets".to_string()),
    (None, None, Some(_), Some(_)) => return Err("input.clickWindowPoint requires relative coordinates within 0..=1".to_string()),
    _ => return Err("input.clickWindowPoint requires --offset-x/--offset-y or --relative-x/--relative-y".to_string()),
  };
  let point_value = point.point();
  if !(0.0..=window.frame.size.width).contains(&point_value.x) || !(0.0..=window.frame.size.height).contains(&point_value.y) {
    return Err(format!(
      "input.clickWindowPoint point {},{} is outside target window bounds 0..={},0..={}",
      point_value.x, point_value.y, window.frame.size.width, window.frame.size.height
    ));
  }
  Ok(point)
}

fn selected_click_options(input: &crate::InvokeCommandInput) -> Result<auv_driver::ClickOptions, String> {
  let command_id = input.command_id.as_str();

  let policy = match input.inputs.get("input-policy").map(String::as_str) {
    None if command_id == "screen.clickText" => auv_driver::InputPolicy::ForegroundPreferred,
    None | Some("background-preferred") => auv_driver::InputPolicy::BackgroundPreferred,
    Some("background-only") => auv_driver::InputPolicy::BackgroundOnly,
    Some("foreground-preferred") => auv_driver::InputPolicy::ForegroundPreferred,
    Some(value) => return Err(format!("{command_id} has unknown --input-policy {value:?}")),
  };
  let count = input
    .inputs
    .get("click-count")
    .map(|value| value.parse::<u32>().map_err(|error| format!("{command_id} has invalid --click-count: {error}")))
    .transpose()?
    .unwrap_or(1);
  if !(1..=u32::from(u8::MAX)).contains(&count) {
    return Err(format!("{command_id} requires --click-count within 1..=255"));
  }
  let interval_ms = input
    .inputs
    .get("click-interval-ms")
    .map(|value| value.parse::<u64>().map_err(|error| format!("{command_id} has invalid --click-interval-ms: {error}")))
    .transpose()?
    .unwrap_or(75);
  if count > 1 && interval_ms == 0 {
    return Err(format!("{command_id} requires a positive --click-interval-ms for repeated clicks"));
  }
  let interval = std::time::Duration::from_millis(interval_ms);
  let click = match count {
    1 => auv_driver::Click::Single,
    2 => auv_driver::Click::Double { interval },
    count => auv_driver::Click::Repeated {
      count: u8::try_from(count).expect("validated click count fits u8"),
      interval,
    },
  };
  Ok(auv_driver::ClickOptions {
    policy,
    click,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
  })
}

fn matched_window_point(window: &auv_driver::Window, matched: &auv_driver::OcrMatch) -> Result<auv_driver::WindowPoint, String> {
  let screen_point = matched.action_point();
  let point = auv_driver::WindowPoint::new(screen_point.x - window.frame.origin.x, screen_point.y - window.frame.origin.y);
  let point_value = point.point();
  if !(0.0..=window.frame.size.width).contains(&point_value.x) || !(0.0..=window.frame.size.height).contains(&point_value.y) {
    return Err(format!("recognized text point {},{} is outside resolved window bounds", screen_point.x, screen_point.y));
  }
  Ok(point)
}

fn selected_window_selector(input: &crate::InvokeCommandInput) -> auv_driver::WindowSelector {
  let app = input
    .target_application_id
    .as_ref()
    // TODO(cross-platform-application-selector): `--target` currently carries
    // an application id and therefore maps to bundle/accessibility id. Add an
    // explicit application-name selector when the CLI contract can distinguish
    // ids from names; do not guess from punctuation or silently retry.
    .map(|bundle_id| auv_driver::App::bundle_id(bundle_id.clone()))
    .unwrap_or_else(auv_driver::App::frontmost);
  let title =
    input.inputs.get("title").filter(|title| !title.trim().is_empty()).map(|title| auv_driver::TextMatcher::Contains(title.clone()));
  auv_driver::WindowSelector {
    app: Some(app),
    main_visible: title.is_none(),
    title,
  }
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod tests;
