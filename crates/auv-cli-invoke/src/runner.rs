//! Typed invoke execution through a selected daemon-owned Runner.

/// Converts a Runner-route failure into a typed failure through the shared
/// `From` classifications, keeping the failing RPC or placement step in the
/// message so the caller sees the same context the legacy stringify path
/// carried. The original error text is never dropped.
fn step_failure(step: &str, error: impl Into<crate::InvokeFailure>) -> crate::InvokeFailure {
  let mut failure = error.into();
  failure.message = format!("{step} failed: {}", failure.message);
  failure
}

/// Refuses a non-dry runner-route dispatch whose declared Runner support for
/// the selected Runner's platform is explicitly unsupported, before any
/// capability RPC. A missing or `Unspecified` platform never refuses: without
/// a Device fact the backend's UNIMPLEMENTED answer, typed by the shared
/// failure mapping, stays the only fallback. The decision input is the
/// daemon-reported platform of the selected device's own host, never the
/// local OS.
fn runner_platform_gate(
  command_id: &str,
  command: &crate::InvokeCommand,
  platform: Option<auv::devices::DevicePlatform>,
) -> Result<(), crate::InvokeFailure> {
  let Some(platform) = platform else {
    return Ok(());
  };
  let support_platform = match platform {
    auv::devices::DevicePlatform::Macos => crate::support::Platform::Macos,
    auv::devices::DevicePlatform::Linux => crate::support::Platform::Linux,
    auv::devices::DevicePlatform::Windows => crate::support::Platform::Windows,
    auv::devices::DevicePlatform::Unspecified => return Ok(()),
  };
  let Some(cells) = command.runner_cells() else {
    // Fixture commands have no Runner route; the earlier no-route refusal
    // owns that case, so the gate is not their business.
    return Ok(());
  };
  if cells.on(support_platform).state == crate::support::SupportState::Unsupported {
    return Err(crate::InvokeFailure::new(
      crate::FailureCode::Unsupported,
      format!(
        "{command_id} has no {} Runner path; the declared Runner support for {} is unsupported",
        support_platform.label(),
        support_platform.label()
      ),
    ));
  }
  Ok(())
}

/// Reuse the existing polling policy for local OCR and selected Runner calls.
/// Callers own capture and recognition, including cancellation between Driver calls.
///
/// NOTICE: The timeout bounds when a poll may start, not when it ends. Capture
/// and OCR are synchronous Driver calls that cannot be cut short, so a poll
/// that starts before the deadline runs to completion (one debug-build Wayland
/// OCR took about 19 s). No poll starts after the deadline, and the sleep
/// between polls never passes it.
///
/// The error type is caller-chosen: the local OCR path keeps its text errors,
/// while the Runner route folds them into typed [`crate::InvokeFailure`]s.
/// Cancellation and timeout have no dedicated failure code; they stay
/// command_failed by construction.
pub(crate) async fn wait_for_selected_text<R, E, Call, Future, HasMatches>(
  command_id: &str,
  query: &str,
  options: auv_driver::WaitOptions,
  cancellation: &crate::InvokeCancellation,
  mut call: Call,
  has_matches: HasMatches,
) -> Result<R, E>
where
  E: From<String>,
  Call: FnMut() -> Future,
  Future: std::future::Future<Output = Result<R, E>>,
  HasMatches: Fn(&R) -> bool,
{
  let started = std::time::Instant::now();
  loop {
    cancellation.check().map_err(|error| E::from(error.to_string()))?;
    let response = call().await?;
    if has_matches(&response) {
      return Ok(response);
    }
    let remaining = options.timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
      // TODO(text-wait-timeout-evidence): report the recognized lines or the
      // last capture with the failure. OcrMatches carries only matches, so
      // this needs the recognition result to keep all lines first.
      let scope = if command_id.starts_with("window.") {
        "window"
      } else {
        "screen"
      };
      return Err(E::from(format!(
        "{command_id} did not find text {query:?} within {} ms; capture the {scope} to check what is shown, or raise --timeout-ms",
        options.timeout.as_millis()
      )));
    }
    tokio::select! {
      _ = cancellation.cancelled() => return Err(E::from("invoke cancelled".to_string())),
      _ = tokio::time::sleep(options.poll_interval.min(remaining)) => {}
    }
  }
}

/// Runs one core command through the selected Runner inside one recorded
/// command span; see [`crate::trace`].
pub async fn invoke(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeExecutionResult {
  // The selected adapter is a large state machine; keep it boxed so the span
  // wrapper does not move it onto the caller's stack (Windows main thread).
  crate::trace::record_command(input, crate::trace::Execution::Runner, move |input| Box::pin(invoke_selected(input, context))).await
}

async fn invoke_selected(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeExecutionResult {
  {
    let registry = crate::default_registry();
    // Every adapter arm below is a core command, so anything else has no Runner route.
    let Some(command) = registry.resolve(&input.command_id) else {
      return Err(crate::InvokeFailure::new(
        crate::FailureCode::Unsupported,
        format!("{} is not a core command, so a selected Runner cannot run it", input.command_id),
      ));
    };
    command.validate(&input)?;
    // TODO(invoke-support-runner-route): a fixture command processes data the caller supplies and
    // touches no desktop, so it could run locally inside the selected Run instead of being refused.
    // Whether it should is part of the Runner route's support, which needs an owner decision.
    if command.role == crate::CommandRole::Fixture {
      return Err(crate::InvokeFailure::new(
        crate::FailureCode::Unsupported,
        format!("{} has no Runner route; run it without a selected Run or Device", input.command_id),
      ));
    }
    // Dry runs keep their current remote-free behavior, so only a real
    // dispatch refuses on a declared-unsupported Runner platform.
    if !input.dry_run {
      runner_platform_gate(&input.command_id, command, context.device_platform)?;
    }
  }

  match input.command_id.as_str() {
    "input.pointerPosition" if input.dry_run => return Ok(crate::InvokeCommandOutput::completed()),
    "input.key" | "input.keys" | "input.pressKeys" | "input.keyboard" | "input.typeText" | "input.pasteText" => {
      let keyboard = crate::commands::input::decode_keyboard_input(&input)
        .map_err(|message| crate::InvokeFailure::new(crate::FailureCode::InvalidInput, message))?;
      crate::commands::input::validate_keyboard_policy(&input, &keyboard)?;
      return execute_keyboard(input, keyboard, context).await;
    }
    "input.holdKeys" => {
      let (keys, policy, duration) = crate::commands::input::decode_hold_keys(&input)?;
      return execute_hold_keys(input, keys, policy, duration, context).await;
    }
    "input.drag" => return execute_drag(input, context).await,
    "input.scroll" => return execute_scroll(input, context).await,
    "input.scrollUntil" => return execute_scroll_until(input, context).await,
    _ => {}
  }

  let command_id = input.command_id.as_str();
  if command_id.starts_with("overlay.") {
    let plan = crate::commands::overlay::plan_overlay(&input)?;
    if input.dry_run || !input.overlay_enabled()? {
      return crate::commands::overlay::selected_overlay_output(&plan, false).map_err(Into::into);
    }
  }

  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await.map_err(|error| step_failure("resolve selected Run", error))?;
  let runner = run
    .runner(auv::client::RunnerOptions::default())
    .await
    .map_err(|error| step_failure(&format!("route core Runner for {command_id}"), error))?;

  if command_id == "app.activate" {
    let target = input.application_target()?.expect("validated target");
    // Keep the typed failure: a Runner without activation (Linux) answers
    // UNIMPLEMENTED, which must reach the caller as `unsupported`.
    let result = runner
      .applications()
      .activate(target, crate::commands::app::ACTIVATION_SETTLE)
      .await
      .map_err(|error| step_failure("ApplicationService/ActivateApplication", error))?;
    return crate::commands::app::activation_output(&result).map_err(Into::into);
  }

  let result = match command_id {
    "input.pointerPosition" => runner
      .input()
      .pointer_position()
      .await
      .map_err(|error| step_failure("InputService/GetPointerPosition", error))
      .and_then(|point| crate::commands::input::pointer_position_output(point.point()).map_err(Into::into)),
    "app.probePermissions" => runner
      .macos()
      .permissions()
      .probe()
      .await
      .map_err(|error| step_failure("PermissionService/ProbePermissions", error))
      .and_then(|probe| crate::commands::app::permission_probe_output(&probe).map_err(Into::into)),
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
          app: input.application_target()?.expect("validated target").to_string(),
          selector,
          expected_role: None,
        })
        .await
        .map_err(|error| step_failure("AccessibilityService/FocusText", error))
        .and_then(|result| crate::commands::input::focus_text_output(&result, &candidate).map_err(Into::into))
    }
    "mediaControl.nowPlaying" => runner
      .macos()
      .media()
      .now_playing()
      .await
      .map_err(|error| step_failure("MediaControlService/GetNowPlaying", error))
      .and_then(|state| crate::commands::media_control::now_playing_state_output(&state).map_err(Into::into)),
    "mediaControl.play" => runner
      .macos()
      .media()
      .play()
      .await
      .map_err(|error| step_failure("MediaControlService/Play", error))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome).map_err(Into::into)),
    "mediaControl.pause" => runner
      .macos()
      .media()
      .pause()
      .await
      .map_err(|error| step_failure("MediaControlService/Pause", error))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome).map_err(Into::into)),
    "mediaControl.togglePlayPause" => runner
      .macos()
      .media()
      .toggle_play_pause()
      .await
      .map_err(|error| step_failure("MediaControlService/TogglePlayPause", error))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome).map_err(Into::into)),
    "mediaControl.next" => runner
      .macos()
      .media()
      .next_track()
      .await
      .map_err(|error| step_failure("MediaControlService/NextTrack", error))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome).map_err(Into::into)),
    "mediaControl.previous" => runner
      .macos()
      .media()
      .previous_track()
      .await
      .map_err(|error| step_failure("MediaControlService/PreviousTrack", error))
      .and_then(|outcome| crate::commands::media_control::media_control_output(&outcome).map_err(Into::into)),
    "overlay.outline" | "overlay.cursor" | "overlay.status" | "overlay.captureFrame" | "overlay.clickTarget" => {
      let plan = crate::commands::overlay::plan_overlay(&input)?;
      runner
        .overlay()
        .show(&plan.overlay, plan.options)
        .await
        .map_err(|error| step_failure("OverlayService/ShowOverlay", error))
        .and_then(|()| crate::commands::overlay::selected_overlay_output(&plan, true).map_err(Into::into))
    }
    "display.list" => runner
      .displays()
      .list()
      .await
      .map_err(|error| step_failure("DisplayService/ListDisplays", error))
      .and_then(|displays| crate::commands::display::list_displays_output(&displays).map_err(Into::into)),
    "display.capture" => match runner.displays().capture(None).await {
      Err(error) => Err(step_failure("CaptureService/CaptureDisplay", error)),
      Ok(capture) => match evidence_pixels(&runner, &capture.capture).await {
        Err(error) => Err(error),
        Ok(pixels) => crate::commands::display::recorded_display_capture_output(
          &capture.display,
          crate::commands::runner_capture_result(&capture.capture),
          pixels.as_ref(),
        )
        .await
        .map_err(Into::into),
      },
    },
    "screen.captureRegion" => match selected_screen_region(&input) {
      Err(error) => Err(error.into()),
      Ok(region) => match runner.displays().capture_region(region, None).await {
        Err(error) => Err(step_failure("CaptureService/CaptureRegion", error)),
        Ok(capture) => match evidence_pixels(&runner, &capture.capture).await {
          Err(error) => Err(error),
          Ok(pixels) => crate::commands::screen::recorded_region_capture_output(
            &capture.display,
            crate::commands::runner_capture_result(&capture.capture),
            pixels.as_ref(),
          )
          .await
          .map_err(Into::into),
        },
      },
    },
    "window.list" => runner.windows().list().await.map_err(|error| step_failure("WindowService/ListWindows", error)).and_then(|windows| {
      crate::commands::window::list_windows_output(&windows.iter().map(|window| window.resource().clone()).collect::<Vec<_>>())
        .map_err(Into::into)
    }),
    "window.capture" => {
      let selector = selected_window_selector(&input);
      let response = match runner.windows().resolve(selector).await {
        Err(error) => Err(error),
        Ok(window) => window.capture().await,
      };
      match response {
        Err(error) => Err(step_failure("WindowService/ResolveWindow or CaptureService/CaptureWindow", error)),
        Ok(response) => match evidence_pixels(&runner, &response.capture).await {
          Err(error) => Err(error),
          Ok(pixels) => crate::commands::window::recorded_window_capture_output(
            &response.window,
            crate::commands::runner_capture_result(&response.capture),
            pixels.as_ref(),
          )
          .await
          .map_err(Into::into),
        },
      }
    }
    "window.findText" => match input.inputs.get("query").cloned() {
      None => Err("window.findText omitted its typed query argument".to_string().into()),
      Some(query) => {
        let response = match runner.windows().resolve(selected_window_selector(&input)).await {
          Err(error) => Err(error),
          Ok(window) => window.find_text(query).await,
        };
        match response {
          Err(error) => Err(step_failure("WindowService/ResolveWindow or TextRecognitionService/FindWindowText", error)),
          Ok(response) => evidence_pixels(&runner, &response.capture).await.and_then(|pixels| {
            crate::commands::window::recorded_window_text_matches_output(
              &crate::commands::window::WindowTextRecognition {
                window: response.window,
                matches: response.matches,
              },
              pixels.as_ref(),
            )
            .map_err(Into::into)
          }),
        }
      }
    },
    "window.waitForText" => match crate::commands::window::WaitForWindowTextArgs::decode(&input) {
      Err(error) => Err(error.into()),
      Ok((query, options)) => match runner.windows().resolve(selected_window_selector(&input)).await {
        Err(error) => Err(step_failure("WindowService/ResolveWindow", error)),
        Ok(window) => {
          let response = wait_for_selected_text(
            command_id,
            &query,
            options,
            &input.cancellation,
            || {
              let window = window.clone();
              let query = query.clone();
              async move { window.find_text(query).await.map_err(|error| step_failure("TextRecognitionService/FindWindowText", error)) }
            },
            |response| !response.matches.matches.is_empty(),
          )
          .await;
          match response {
            Err(error) => Err(error),
            Ok(response) => evidence_pixels(&runner, &response.capture).await.and_then(|pixels| {
              crate::commands::window::recorded_window_text_matches_output(
                &crate::commands::window::WindowTextRecognition {
                  window: response.window,
                  matches: response.matches,
                },
                pixels.as_ref(),
              )
              .map_err(Into::into)
            }),
          }
        }
      },
    },
    "screen.findText" => match input.inputs.get("query").cloned() {
      None => Err("screen.findText omitted its typed query argument".to_string().into()),
      Some(query) => {
        let response = runner.displays().find_text(None, query).await;
        match response {
          Err(error) => Err(step_failure("TextRecognitionService/FindDisplayText", error)),
          Ok(response) => evidence_pixels(&runner, &response.capture).await.and_then(|pixels| {
            crate::commands::screen::recorded_screen_text_matches_output(&response.matches, pixels.as_ref()).map_err(Into::into)
          }),
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
          .map_err(|error| step_failure("TextRecognitionService/FindDisplayText", error))?;
        let matches = recognized.matches;
        let capture = recognized.capture;
        let point = matches.best_match().ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?.action_point();
        let click = selected_click_options(&input)?.click;
        let options = auv_driver::ClickOptions {
          click,
          ..Default::default()
        };
        let response = runner
          .input()
          .click(&auv_driver::ScreenPoint::from(point), options)
          .await
          .map_err(|error| step_failure("InputService/ClickPoint", error))?;
        let result = crate::commands::screen::ScreenTextClick {
          matches,
          point: response.screen_point.point(),
          action: response.action,
        };
        crate::commands::screen::recorded_screen_text_click_output(&result, evidence_pixels(&runner, &capture).await?.as_ref())
          .map_err(Into::into)
      }
      .await
    }
    "screen.waitForText" => match crate::commands::screen::WaitForScreenTextArgs::decode(&input) {
      Err(error) => Err(error.into()),
      Ok((query, options)) => {
        let displays = runner.displays();
        let response = wait_for_selected_text(
          command_id,
          &query,
          options,
          &input.cancellation,
          || {
            let displays = displays.clone();
            let query = query.clone();
            async move { displays.find_text(None, query).await.map_err(|error| step_failure("TextRecognitionService/FindDisplayText", error)) }
          },
          |response| !response.matches.matches.is_empty(),
        )
        .await;
        match response {
          Err(error) => Err(error),
          Ok(response) => evidence_pixels(&runner, &response.capture).await.and_then(|pixels| {
            crate::commands::screen::recorded_screen_text_matches_output(&response.matches, pixels.as_ref()).map_err(Into::into)
          }),
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
          .map_err(|error| step_failure("WindowService/ResolveWindow", error))?;
        let resolved_window = resolved.resource().clone();
        let recognized =
          resolved.find_text(query.clone()).await.map_err(|error| step_failure("TextRecognitionService/FindWindowText", error))?;
        let matches = recognized.matches;
        let capture = recognized.capture;
        let matched = crate::commands::window::selected_window_text_match(&matches, &query, selected_index)?;
        let window_point = matched_window_point(&resolved_window, matched)?;
        let options = selected_click_options(&input)?;
        let response =
          resolved.click(window_point, options.clone()).await.map_err(|error| step_failure("InputService/ClickPoint", error))?;
        let clicked_window = response.window;
        if clicked_window.reference != resolved_window.reference {
          return Err("ClickPoint response changed the resolved WindowRef".to_string().into());
        }
        let result = crate::commands::window::WindowTextClick {
          window: clicked_window,
          matches,
          selected_index,
          // The Runner echoes the delivered pair, so report both spaces from
          // the response rather than re-deriving the screen point locally.
          point: response.screen_point,
          window_point: response.point,
          options,
          action: response.action,
        };
        crate::commands::window::recorded_window_text_click_output(&result, evidence_pixels(&runner, &capture).await?.as_ref())
          .map_err(Into::into)
      }
      .await
    }
    "input.moveMouse" => {
      async {
        let point = selected_screen_point(&input, "input.moveMouse")?;
        let mut stream = runner
          .input()
          .move_mouse(auv_driver::MoveMouseRequest::direct(point.point()))
          .await
          .map_err(|error| step_failure("InputService/MoveMouse", error))?;
        while let Some(event) = stream.next().await.map_err(|error| step_failure("InputService/MoveMouse", error))? {
          if let auv::client::runner::MouseMotionEvent::Completed { point, action } = event {
            crate::emit_input_action_result(&action);
            return crate::commands::input::mouse_move_output(crate::commands::input::MouseMoveResult {
              point: auv_driver::ScreenPoint::new(point.x, point.y),
              action: Some(action),
            })
            .map_err(Into::into);
          }
        }
        Err("InputService/MoveMouse ended without completion evidence".to_string().into())
      }
      .await
    }
    "input.clickPoint" => selected_click_point(&input, &runner).await,
    "input.scrollPoint" => {
      async {
        let (point, scroll, settle) = crate::commands::input::decode_screen_scroll(&input)?;
        let response = runner
          .input()
          .scroll(&auv_driver::ScreenPoint::new(point.x, point.y), scroll, settle)
          .await
          .map_err(|error| step_failure("InputService/ScrollPoint", error))?;
        crate::emit_input_action_result(&response.action);
        crate::commands::input::scroll_point_output(crate::commands::input::ScrollPointResult {
          point: response.screen_point,
          scroll,
          action: Some(response.action),
        })
        .map_err(Into::into)
      }
      .await
    }
    // A core command added without an adapter arm lands here; refuse it rather than panic.
    _ => {
      return Err(crate::InvokeFailure::new(
        crate::FailureCode::Unsupported,
        format!("{command_id} has no Runner adapter; run it without a selected Run or Device"),
      ));
    }
  };
  result
}

/// Fetches a Runner-held capture's pixels for the PNG evidence artifacts these
/// commands record, or `None` when this call records no artifacts: results
/// themselves need only the capture's metadata.
// TODO(runner-side-capture-artifacts): the Runner now records display, region,
// and window captures itself (`auv-cli` `runner::recording`), so those three
// commands are recorded twice on this path: once here under the command span
// and once in the Runner's store under its RPC span. Drop this pull for them
// once frontends can present Runner-side artifact references, and for text
// searches once the Runner records their captures; see
// docs/ai/references/driver/2026-10-06-capture-references-and-positions-design.md.
async fn evidence_pixels(
  runner: &auv::client::runner::RunnerClient,
  capture: &auv::client::runner::RunnerCapture,
) -> Result<Option<auv_driver::Capture>, crate::InvokeFailure> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return Ok(None);
  }
  let pixels = runner.captures().pixels(capture).await.map_err(|error| step_failure("CaptureService/GetCaptureImage", error))?;
  Ok(Some(pixels))
}

async fn selected_click_point(
  input: &crate::InvokeCommandInput,
  runner: &auv::client::runner::RunnerClient,
) -> crate::InvokeExecutionResult {
  let requested = selected_screen_point(input, "input.clickPoint")?;
  let normalized = input
    .inputs
    .get("normalized")
    .map(|value| value.parse::<bool>().map_err(|error| format!("input.clickPoint has invalid --normalized value: {error}")))
    .transpose()?
    .unwrap_or(false);
  if normalized && (!(0.0..=1.0).contains(&requested.point().x) || !(0.0..=1.0).contains(&requested.point().y)) {
    return Err("input.clickPoint --normalized coordinates must be within 0..=1".to_string().into());
  }
  let basis = crate::commands::input::point_basis(
    "input.clickPoint",
    input.target.as_ref(),
    input.inputs.get("relative-to").map(String::as_str),
    normalized,
    input.inputs.contains_key("input-policy"),
    input.inputs.contains_key("title"),
  )?;
  let requested_point = requested.point();
  let click_options = selected_click_options(input)?;
  match basis {
    crate::commands::input::RelativeToArg::Screen if !crate::commands::input::targets_window(input.target.as_ref()) => {
      let response =
        runner.input().click(&requested, click_options).await.map_err(|error| step_failure("InputService/ClickPoint", error))?;
      crate::emit_input_action_result(&response.action);
      crate::commands::input::click_point_output(crate::commands::input::ClickPointResult {
        relative_to: basis.as_str().to_string(),
        requested_point,
        normalized,
        screen_point: response.screen_point,
        window: None,
        display: None,
        action: Some(response.action),
      })
      .map_err(Into::into)
    }
    crate::commands::input::RelativeToArg::Window | crate::commands::input::RelativeToArg::Screen => {
      let windows = runner.windows();
      let resolved = match input.target.as_ref().expect("window-relative target validated") {
        crate::ExecutionTarget::Application { .. } => {
          windows.resolve(selected_window_selector(input)).await.map_err(|error| step_failure("WindowService/ResolveWindow", error))?
        }
        crate::ExecutionTarget::Window { id } => windows.get(id).await.map_err(|error| step_failure("WindowService/ListWindows", error))?,
        crate::ExecutionTarget::Display { .. } => unreachable!("target/basis validated"),
      };
      let window = resolved.resource();
      let target: auv::client::runner::PointerTarget = if basis == crate::commands::input::RelativeToArg::Screen {
        crate::commands::input::screen_point_in_window("input.clickPoint", requested_point, window.frame)?;
        // Send the screen point itself: the Runner converts it with the frame
        // it reads at delivery, so a window that moved since ResolveWindow
        // still gets the requested screen point.
        auv_driver::ScreenPoint::new(requested_point.x, requested_point.y).into()
      } else {
        let point = crate::commands::input::resolve_local_point(
          "input.clickPoint",
          requested_point.x,
          requested_point.y,
          normalized,
          window.frame.size,
          "window",
        )?;
        auv_driver::WindowPoint::new(point.x, point.y).into()
      };
      let response = resolved.click(target, click_options).await.map_err(|error| step_failure("InputService/ClickPoint", error))?;
      crate::emit_input_action_result(&response.action);
      crate::commands::input::click_point_output(crate::commands::input::ClickPointResult {
        relative_to: basis.as_str().to_string(),
        requested_point,
        normalized,
        screen_point: response.screen_point,
        window: Some(response.window),
        display: None,
        action: Some(response.action),
      })
      .map_err(Into::into)
    }
    crate::commands::input::RelativeToArg::Display => {
      let crate::ExecutionTarget::Display { id } = input.target.as_ref().expect("display-relative target validated") else {
        unreachable!("target/basis validated")
      };
      let display = runner
        .displays()
        .list()
        .await
        .map_err(|error| step_failure("DisplayService/ListDisplays", error))?
        .displays
        .into_iter()
        .find(|display| display.id == *id)
        .ok_or_else(|| format!("input.clickPoint could not find display target {id:?}"))?;
      let point = crate::commands::input::resolve_local_point(
        "input.clickPoint",
        requested_point.x,
        requested_point.y,
        normalized,
        display.frame.size,
        "display",
      )?;
      let position = auv_driver::Position {
        point,
        coordinate_space: auv_driver::CoordinateSpace::Display(display.id.clone()),
      };
      let response = runner.input().click(&position, click_options).await.map_err(|error| step_failure("InputService/ClickPoint", error))?;
      crate::emit_input_action_result(&response.action);
      crate::commands::input::click_point_output(crate::commands::input::ClickPointResult {
        relative_to: basis.as_str().to_string(),
        requested_point,
        normalized,
        screen_point: response.screen_point,
        window: None,
        display: Some(display),
        action: Some(response.action),
      })
      .map_err(Into::into)
    }
  }
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
  let interval = input
    .inputs
    .get("click-interval-ms")
    .map(|value| value.parse::<u64>().map_err(|error| format!("{command_id} has invalid --click-interval-ms: {error}")))
    .transpose()?
    .map_or(auv_driver::Click::DEFAULT_INTERVAL, std::time::Duration::from_millis);
  if count > 1 && interval.is_zero() {
    return Err(format!("{command_id} requires a positive --click-interval-ms for repeated clicks"));
  }
  let click = match count {
    1 => auv_driver::Click::Single,
    2 => auv_driver::Click::Double { interval },
    count => auv_driver::Click::Repeated {
      count: u8::try_from(count).expect("validated click count fits u8"),
      interval,
    },
  };
  Ok(auv_driver::ClickOptions {
    button: crate::commands::input::parse_click_button(input.inputs.get("button").map(String::as_str))?,
    policy,
    click,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
    modifiers: crate::commands::input::parse_click_modifiers(input.inputs.get("modifiers").map(String::as_str))?,
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
    .target
    .as_ref()
    .and_then(crate::ExecutionTarget::application_id)
    // TODO(application-name-target): application targets carry platform
    // application ids, never application names. Add an explicit
    // application-name target only through an owner-approved target contract
    // extension.
    .map(|application_id| auv_driver::App::id(application_id.to_string()))
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

// Selection happens on the chosen Runner; a target-bound RPC never falls back
// to a local driver or an unqualified global keyboard RPC.
async fn execute_keyboard(
  input: crate::InvokeCommandInput,
  keyboard: Vec<auv_driver::KeyboardInput>,
  context: auv::AuvContext,
) -> crate::InvokeExecutionResult {
  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await?;
  let runner = run.runner(auv::client::RunnerOptions::default()).await?;

  let target = runner_keyboard_target(&input, &runner).await?;
  input.cancellation.check().map_err(|error| error.to_string())?;

  let result = runner.input().input_keyboard(&target, keyboard, input.dry_run).await.map_err(Into::into);
  crate::commands::input::keyboard_output(&input, result)
}

async fn runner_keyboard_target(
  input: &crate::InvokeCommandInput,
  runner: &auv::client::runner::RunnerClient,
) -> Result<auv_driver::InputTarget, crate::InvokeFailure> {
  let target = match input.target.as_ref() {
    None => auv_driver::InputTarget::Foreground,
    Some(crate::ExecutionTarget::Application { id }) => auv_driver::InputTarget::Application { id: id.clone() },
    Some(crate::ExecutionTarget::Window { id }) => auv_driver::InputTarget::Window(runner.windows().get(id).await?.resource().clone()),
    Some(crate::ExecutionTarget::Display { .. }) => unreachable!("target policy validated"),
  };

  Ok(target)
}

async fn execute_hold_keys(
  input: crate::InvokeCommandInput,
  keys: Vec<String>,
  policy: auv_driver::InputPolicy,
  duration: std::time::Duration,
  context: auv::AuvContext,
) -> crate::InvokeExecutionResult {
  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await?;
  let runner = run.runner(auv::client::RunnerOptions::default()).await?;
  let target = runner_keyboard_target(&input, &runner).await?;
  input.cancellation.check().map_err(|error| error.to_string())?;

  if input.dry_run {
    let options = auv_driver::PressKeysOptions {
      keys,
      ..Default::default()
    };
    runner.input().input_keyboard(&target, vec![auv_driver::KeyboardInput::PressKeys { options, policy }], true).await?;
    return crate::commands::input::targeted_keyboard_output(None).map_err(Into::into);
  }

  let client = runner.input();
  let action = tokio::select! {
    _ = input.cancellation.cancelled() => return Err("invoke cancelled".to_string().into()),
    result = client.hold_keys(&target, keys, policy, duration) => result?,
  };

  crate::commands::input::targeted_keyboard_output(Some(&action)).map_err(Into::into)
}

/// Runner route for `input.drag`: the same plan and path as local invoke,
/// resolved through Runner window/display services and one DragMouse call.
async fn execute_scroll_until(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeExecutionResult {
  let plan = crate::commands::input::decode_scroll_until(&input)?;
  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await?;
  let runner = run.runner(auv::client::RunnerOptions::default()).await?;
  let resolved = resolve_runner_window(&runner, &input).await?;
  let point = plan.window_point(resolved.resource())?;
  let mut output = plan.output(resolved.resource().clone(), point);
  if input.dry_run {
    return crate::commands::input::scroll_until_output(output).map_err(Into::into);
  }
  input.cancellation.check().map_err(|error| error.to_string())?;
  let delivery = async {
    let mut stream = resolved.scroll_until(point, plan.request.clone(), plan.options.clone(), false).await?;
    while let Some(event) = stream.next().await? {
      if let auv::client::runner::ScrollUntilEvent::Completed(result) = event {
        return Ok(result);
      }
    }
    Err(crate::InvokeFailure::from("ScrollUntil ended without completion evidence".to_string()))
  };
  let result = tokio::select! {
    _ = input.cancellation.cancelled() => return Err("invoke cancelled".to_string().into()),
    result = delivery => result?,
  };
  if let Some(action) = &result.action {
    crate::emit_input_action_result(action);
  }
  output.result = Some(result);
  crate::commands::input::scroll_until_output(output).map_err(Into::into)
}

/// Resolves an `app:` (optionally by title) or `window:` target through the
/// Runner for window-bound scroll commands.
async fn resolve_runner_window(
  runner: &auv::client::runner::RunnerClient,
  input: &crate::InvokeCommandInput,
) -> Result<auv::client::runner::WindowClient, crate::InvokeFailure> {
  let windows = runner.windows();
  match input.target.as_ref().expect("window target validated") {
    crate::ExecutionTarget::Application { .. } => Ok(windows.resolve(selected_window_selector(input)).await?),
    crate::ExecutionTarget::Window { id } => Ok(windows.get(id).await?),
    crate::ExecutionTarget::Display { .. } => unreachable!("target validated"),
  }
}

async fn execute_scroll(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeExecutionResult {
  let plan = crate::commands::input::decode_scroll(&input)?;
  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await?;
  let runner = run.runner(auv::client::RunnerOptions::default()).await?;

  let resolved = resolve_runner_window(&runner, &input).await?;
  let point = plan.window_point(resolved.resource())?;
  let mut result = plan.result(resolved.resource().clone(), point);
  if input.dry_run {
    return crate::commands::input::scroll_output(result).map_err(Into::into);
  }

  input.cancellation.check().map_err(|error| error.to_string())?;
  if let Some(motion) = plan.motion {
    let delivery = async {
      let mut stream = resolved.scroll_motion(point, motion, plan.options.clone()).await?;
      while let Some(event) = stream.next().await? {
        match event {
          auv::client::runner::ScrollMotionEvent::Started { window, .. } if window.reference != result.window.reference => {
            return Err(crate::InvokeFailure::from("ScrollWindowPointMotion changed the resolved WindowRef".to_string()));
          }
          auv::client::runner::ScrollMotionEvent::Completed { delivered, action } => return Ok((delivered, action)),
          _ => {}
        }
      }
      Err(crate::InvokeFailure::from("ScrollWindowPointMotion ended without completion evidence".to_string()))
    };
    let (delivered, action) = tokio::select! {
      _ = input.cancellation.cancelled() => return Err("invoke cancelled".to_string().into()),
      response = delivery => response?,
    };
    crate::emit_input_action_result(&action);
    result.delivered = Some(delivered);
    result.action = Some(action);
    return crate::commands::input::scroll_output(result).map_err(Into::into);
  }
  let response = tokio::select! {
    _ = input.cancellation.cancelled() => return Err("invoke cancelled".to_string().into()),
    response = resolved.scroll(point, plan.scroll, plan.options.clone()) => response?,
  };
  if response.window.reference != result.window.reference {
    return Err("ScrollWindowPoint response changed the resolved WindowRef".to_string().into());
  }
  crate::emit_input_action_result(&response.action);
  result.window = response.window;
  result.action = Some(response.action);
  crate::commands::input::scroll_output(result).map_err(Into::into)
}

async fn execute_drag(input: crate::InvokeCommandInput, context: auv::AuvContext) -> crate::InvokeExecutionResult {
  use crate::commands::input::{DragResult, RelativeToArg};

  let plan = crate::commands::input::decode_drag(&input)?;
  // TODO(drag-runner-window-policy): DragMouse carries no window input policy,
  // so the Runner cannot foreground the window first. Add a window drag RPC
  // when a Device or Run consumer needs foreground window drags.
  if plan.policy == Some(auv_driver::InputPolicy::ForegroundPreferred) {
    return Err(crate::InvokeFailure::new(
      crate::FailureCode::Unsupported,
      "input.drag --input-policy foreground-preferred is available only for local invoke",
    ));
  }
  let auv = auv::Client::from_context(context).await?;
  let run = auv.run(Default::default()).await?;
  let runner = run.runner(auv::client::RunnerOptions::default()).await?;

  let (mut result, target) = match plan.basis {
    RelativeToArg::Screen if !crate::commands::input::targets_window(input.target.as_ref()) => {
      let start = auv_driver::ScreenPoint::new(plan.start.x, plan.start.y);
      let end = auv_driver::ScreenPoint::new(plan.end.x, plan.end.y);
      (DragResult::planned(&plan, start, end), auv_driver::InputTarget::Foreground)
    }
    RelativeToArg::Window | RelativeToArg::Screen => {
      let windows = runner.windows();
      let window = match input.target.as_ref().expect("window-relative target validated") {
        crate::ExecutionTarget::Application { .. } => windows.resolve(selected_window_selector(&input)).await?.resource().clone(),
        crate::ExecutionTarget::Window { id } => windows.get(id).await?.resource().clone(),
        crate::ExecutionTarget::Display { .. } => unreachable!("target/basis validated"),
      };
      let (start, end) = plan.window_screen_points(window.frame)?;
      let mut result = DragResult::planned(&plan, start, end);
      result.window = Some(window.clone());
      (result, auv_driver::InputTarget::Window(window))
    }
    RelativeToArg::Display => {
      let crate::ExecutionTarget::Display { id } = input.target.as_ref().expect("display-relative target validated") else {
        unreachable!("target/basis validated")
      };
      let display = runner.displays().list().await?.displays.into_iter().find(|display| display.id == *id).ok_or_else(|| {
        crate::InvokeFailure::new(crate::FailureCode::NotFound, format!("input.drag could not find display target {id:?}"))
      })?;
      let (start, end) = plan.screen_points(display.frame, "display")?;
      let mut result = DragResult::planned(&plan, start, end);
      result.display = Some(display);
      (result, auv_driver::InputTarget::Foreground)
    }
  };
  if input.dry_run {
    return crate::commands::input::drag_output(result).map_err(Into::into);
  }

  input.cancellation.check().map_err(|error| error.to_string())?;
  let movement = plan.movement(target, result.screen_start, result.screen_end);
  let client = runner.input();
  let (point, action) = tokio::select! {
    _ = input.cancellation.cancelled() => return Err("invoke cancelled".to_string().into()),
    response = client.drag_mouse(movement, plan.button) => response?,
  };
  crate::emit_input_action_result(&action);
  result.final_point = Some(auv_driver::ScreenPoint::new(point.x, point.y));
  result.action = Some(action);
  crate::commands::input::drag_output(result).map_err(Into::into)
}
