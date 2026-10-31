use super::*;

#[tokio::test]
async fn selected_pointer_position_dry_run_never_resolves_a_daemon_or_observes_input() {
  let output = invoke(
    crate::InvokeCommandInput {
      command_id: "input.pointerPosition".to_string(),
      target: None,
      inputs: Default::default(),
      typed_args: None,
      dry_run: true,
      cancellation: Default::default(),
    },
    Default::default(),
  )
  .await
  .expect("dry-run does not require a Runner");
  assert!(output.result().is_none(), "no position was observed");
}

#[tokio::test]
async fn selected_now_playing_rejects_application_target_before_daemon_resolution() {
  let error = invoke(
    crate::InvokeCommandInput {
      command_id: "mediaControl.nowPlaying".to_string(),
      target: Some(crate::ExecutionTarget::Application {
        id: "com.example.Player".to_string(),
      }),
      inputs: Default::default(),
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    },
    Default::default(),
  )
  .await
  .expect_err("now-playing target must fail before daemon resolution");
  assert_eq!(error.code, crate::FailureCode::InvalidTarget);
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
        target: Some(crate::ExecutionTarget::Application {
          id: "com.example.Player".to_string(),
        }),
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
      Default::default(),
    )
    .await
    .expect_err("target must fail before daemon resolution");
    assert_eq!(error.code, crate::FailureCode::InvalidTarget);
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
    target,
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
      target,
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
async fn text_wait_rejects_cancellation_before_observation() {
  let cancellation = crate::InvokeCancellation::new();
  cancellation.cancel();
  let calls = std::cell::Cell::new(0);
  let error = wait_for_selected_text(
    "screen.waitForText",
    "Ready",
    Default::default(),
    &cancellation,
    || {
      calls.set(calls.get() + 1);
      async { Ok::<bool, crate::InvokeFailure>(false) }
    },
    |matched| *matched,
  )
  .await
  .expect_err("cancelled wait must not observe the desktop");

  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert_eq!(error.message, "invoke cancelled");
  assert_eq!(calls.get(), 0);
}

#[tokio::test]
async fn text_wait_cancels_during_the_poll_interval_without_another_observation() {
  use futures_util::FutureExt;

  let cancellation = crate::InvokeCancellation::new();
  let calls = std::cell::Cell::new(0);
  let waiting = wait_for_selected_text(
    "screen.waitForText",
    "Ready",
    auv_driver::WaitOptions {
      timeout: std::time::Duration::from_secs(30),
      poll_interval: std::time::Duration::from_secs(30),
    },
    &cancellation,
    || {
      calls.set(calls.get() + 1);
      async { Ok::<bool, crate::InvokeFailure>(false) }
    },
    |matched| *matched,
  );
  let mut waiting = std::pin::pin!(waiting);
  assert!(waiting.as_mut().now_or_never().is_none(), "empty observation must enter the poll interval");
  assert_eq!(calls.get(), 1);
  cancellation.cancel();
  let result = waiting.as_mut().now_or_never().expect("cancellation must finish without waiting for the poll timer");

  let error = result.expect_err("cancelled wait");
  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert_eq!(error.message, "invoke cancelled");
  assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn text_wait_preserves_observation_errors_without_retry() {
  let calls = std::cell::Cell::new(0);
  let error = wait_for_selected_text(
    "screen.waitForText",
    "Ready",
    Default::default(),
    &Default::default(),
    || {
      calls.set(calls.get() + 1);
      async { Err::<bool, crate::InvokeFailure>("fixture capture permission denied".to_string().into()) }
    },
    |matched| *matched,
  )
  .await
  .expect_err("observation failure");

  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert_eq!(error.message, "fixture capture permission denied");
  assert_eq!(calls.get(), 1);
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
      async move { Ok::<_, crate::InvokeFailure>(response) }
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
      async { Ok::<Vec<u8>, crate::InvokeFailure>(Vec::new()) }
    },
    |response| !response.is_empty(),
  )
  .await
  .expect_err("empty response at the deadline times out");

  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert_eq!(
    error.message,
    "window.waitForText did not find text \"Ready\" within 0 ms; capture the window to check what is shown, or raise --timeout-ms"
  );
  assert_eq!(calls.get(), 1);
}

// ROOT CAUSE:
//
// If a Runner capability RPC failed, the arm stringified the CapabilityError
// into "RPC failed: {error}", and From<String> classified that message as
// command_failed. The transport category behind the error (not_found,
// unsupported, permission_denied, invalid_input, stale_ui_reference,
// backend) never reached CLI or MCP callers, so machine consumers could only
// grep the message text.
//
// Before the fix, every converted arm reported code command_failed, with the
// RPC name only inside the message. The fix routes the same arms through the
// shared From classifications while step_failure keeps the RPC name and the
// original error text in the message.
#[test]
fn runner_step_failures_keep_the_rpc_name_and_typed_code() {
  let not_found = step_failure("WindowService/ResolveWindow", auv::client::runner::CapabilityError::NotFound("window 777".to_string()));
  assert_eq!(not_found.code, crate::FailureCode::NotFound);
  assert_eq!(not_found.message, "WindowService/ResolveWindow failed: window 777 was not found");

  // Client-side request validation carries no transport category, so it
  // classifies no more specifically than backend, while the RPC name and the
  // error text stay in the message.
  let invalid = step_failure(
    "InputService/ScrollPoint",
    auv::client::runner::CapabilityError::InvalidArgument("hold duration must be within (0, 30s]".to_string()),
  );
  assert_eq!(invalid.code, crate::FailureCode::Backend);
  assert_eq!(invalid.message, "InputService/ScrollPoint failed: Runner request is invalid: hold duration must be within (0, 30s]");

  // Placement steps keep their context prefix through the same conversion.
  let routed = step_failure(
    "route core Runner for input.key",
    auv::client::PlacementError::Capability(auv::client::runner::CapabilityError::NotFound("hold id 777".to_string())),
  );
  assert_eq!(routed.code, crate::FailureCode::NotFound);
  assert_eq!(routed.message, "route core Runner for input.key failed: hold id 777 was not found");
}

#[tokio::test]
async fn text_wait_surfaces_typed_runner_observation_failures() {
  let calls = std::cell::Cell::new(0);
  let error = wait_for_selected_text(
    "window.waitForText",
    "Ready",
    Default::default(),
    &Default::default(),
    || {
      calls.set(calls.get() + 1);
      async {
        Err::<bool, _>(step_failure(
          "TextRecognitionService/FindWindowText",
          auv::client::runner::CapabilityError::NotFound("window 777".to_string()),
        ))
      }
    },
    |matched| *matched,
  )
  .await
  .expect_err("observation failure");

  assert_eq!(error.code, crate::FailureCode::NotFound);
  assert_eq!(error.message, "TextRecognitionService/FindWindowText failed: window 777 was not found");
  assert_eq!(calls.get(), 1);
}

#[test]
fn selected_window_selector_keeps_hierarchical_parent_context() {
  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("title".to_string(), "Preferences".to_string());
  let selector = selected_window_selector(&crate::InvokeCommandInput {
    command_id: "window.capture".to_string(),
    target: Some(crate::ExecutionTarget::Application {
      id: "com.example.app".to_string(),
    }),
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  });

  assert_eq!(selector.app, Some(auv_driver::App::id("com.example.app")));
  assert_eq!(selector.title, Some(auv_driver::TextMatcher::Contains("Preferences".to_string())));
}

#[test]
fn selected_click_options_preserve_policy_and_repeated_clicks() {
  let mut inputs = std::collections::BTreeMap::new();
  inputs.insert("input-policy".to_string(), "background-only".to_string());
  inputs.insert("click-count".to_string(), "2".to_string());
  inputs.insert("click-interval-ms".to_string(), "80".to_string());
  inputs.insert("modifiers".to_string(), "meta,shift".to_string());
  let input = crate::InvokeCommandInput {
    command_id: "input.clickPoint".to_string(),
    target: None,
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };
  let options = selected_click_options(&input).unwrap();
  assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundOnly);
  assert_eq!(options.click.count(), 2);
  assert_eq!(options.click.interval(), Some(std::time::Duration::from_millis(80)));
  assert!(options.modifiers.meta && options.modifiers.shift);
  assert!(!options.modifiers.control && !options.modifiers.alt);
}

#[test]
fn selected_screen_point_preserves_logical_coordinates() {
  let input = crate::InvokeCommandInput {
    command_id: "input.clickPoint".to_string(),
    target: None,
    inputs: std::collections::BTreeMap::from([
      ("x".to_string(), "768".to_string()),
      ("y".to_string(), "1139.5".to_string()),
    ]),
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };

  assert_eq!(selected_screen_point(&input, "input.clickPoint").unwrap(), auv_driver::ScreenPoint::new(768.0, 1139.5));
}

#[test]
fn selected_screen_text_click_defaults_to_foreground_input() {
  let input = crate::InvokeCommandInput {
    command_id: "screen.clickText".to_string(),
    target: None,
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
    application_id: None,
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
    target: None,
    inputs,
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  };
  assert_eq!(
    selected_click_options(&input).unwrap(),
    auv_driver::ClickOptions {
      button: auv_driver::MouseButton::Left,
      policy: auv_driver::InputPolicy::ForegroundPreferred,
      click: auv_driver::Click::Repeated {
        count: 3,
        interval: std::time::Duration::from_millis(60),
      },
      window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
      modifiers: Default::default(),
    }
  );
}

// ROOT CAUSE:
//
// If a command without a Runner adapter ran with a selected Run or Device (the fixture commands
// `scan.frame` and `scan.coverage`, or an MCP adapter that is not a core command), the selected
// Runner route connected to the daemon and then reached `unreachable!` in its command match,
// which panicked the frontend.
//
// Before the fix, nothing refused those commands before the match. The fix refuses them as
// `unsupported` before any daemon resolution.
#[tokio::test]
async fn selected_commands_without_a_runner_route_are_refused_before_daemon_resolution() {
  for command_id in ["scan.frame", "scan.coverage", "example.notACoreCommand"] {
    let error = invoke(
      crate::InvokeCommandInput {
        command_id: command_id.to_string(),
        target: None,
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: Default::default(),
      },
      Default::default(),
    )
    .await
    .expect_err("a command without a Runner route must be refused");
    assert_eq!(error.code, crate::FailureCode::Unsupported, "{command_id}: {}", error.message);
  }
}

// ROOT CAUSE:
//
// If an MCP or Runner caller passed an input key that the command does not
// declare, serde dropped it and the command ran with its defaults. OSWorld
// agents sent `timeout` to screen.waitForText 49 times and got the fixed wait.
//
// Before the fix, the unknown key was ignored. The fix rejects it as
// invalid_input and names the accepted inputs; declared keys, the `target`
// fallback key on commands that take a target, and the protocol `overlay`
// key still pass.
#[tokio::test]
async fn undeclared_input_keys_fail_before_daemon_resolution() {
  let input = |command_id: &str, inputs: &[(&str, &str)]| crate::InvokeCommandInput {
    command_id: command_id.to_string(),
    target: None,
    inputs: inputs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  };

  let error = invoke(input("window.findText", &[("query", "Ready"), ("timeout", "30000")]), Default::default())
    .await
    .expect_err("timeout is not a declared input");
  assert_eq!(error.code, crate::FailureCode::InvalidInput);
  assert_eq!(error.message, "window.findText does not accept input timeout; accepted inputs: query, title, target");

  let registry = crate::default_registry();
  let validate = |command_id: &str, inputs: &[(&str, &str)]| registry.resolve(command_id).unwrap().validate(&input(command_id, inputs));
  validate(
    "window.findText",
    &[
      ("query", "Ready"),
      ("title", "Import"),
      ("target", "com.apple.TextEdit"),
      ("overlay", "false"),
    ],
  )
  .expect("declared inputs pass");
  let forbidden = validate("display.list", &[("target", "com.apple.TextEdit")]).expect_err("display.list takes no target");
  assert_eq!(forbidden.message, "display.list does not accept input target; accepted inputs: none");
  let id_spelling = validate("input.clickPoint", &[("x", "1"), ("y", "1"), ("click_count", "2")]).expect_err("ids are not input keys");
  assert!(id_spelling.message.contains("does not accept input click_count"), "{}", id_spelling.message);
}

fn gate_context(platform: Option<auv::devices::DevicePlatform>) -> SelectedDaemon {
  SelectedDaemon {
    context: auv::AuvContext {
      device_platform: platform,
      ..Default::default()
    },
    credential: None,
  }
}

// An endpoint that cannot parse fails placement deterministically and without
// any network traffic, so a placement error from it proves the dispatch got
// past the gate instead of connecting anywhere.
fn gate_context_past_the_gate(platform: Option<auv::devices::DevicePlatform>) -> SelectedDaemon {
  SelectedDaemon {
    context: auv::AuvContext {
      device_platform: platform,
      daemon_endpoint: Some("not a valid endpoint".to_string()),
      ..Default::default()
    },
    credential: None,
  }
}

fn gate_input(command_id: &str, dry_run: bool) -> crate::InvokeCommandInput {
  crate::InvokeCommandInput {
    command_id: command_id.to_string(),
    target: None,
    inputs: Default::default(),
    typed_args: None,
    dry_run,
    cancellation: Default::default(),
  }
}

// focusText validates an application target before dispatch, so its gate
// tests must satisfy that validation to reach the gate itself.
fn gate_input_targeted(command_id: &str, dry_run: bool) -> crate::InvokeCommandInput {
  crate::InvokeCommandInput {
    target: Some(crate::ExecutionTarget::Application {
      id: "com.example.TextEdit".to_string(),
    }),
    ..gate_input(command_id, dry_run)
  }
}

#[test]
fn runner_platform_gate_refuses_only_explicitly_unsupported_cells() {
  let registry = crate::default_registry();
  use auv::devices::DevicePlatform;

  // Linux has no backend; Windows query focus is partial; macOS is supported.
  let focus_text = registry.resolve("input.focusText").unwrap();
  for (platform, label) in [(DevicePlatform::Linux, "Linux")] {
    let error = runner_platform_gate("input.focusText", focus_text, Some(platform)).unwrap_err();
    assert_eq!(error.code, crate::FailureCode::Unsupported);
    assert_eq!(error.message, format!("input.focusText has no {label} Runner path; the declared Runner support for {label} is unsupported"));
  }
  assert!(runner_platform_gate("input.focusText", focus_text, Some(DevicePlatform::Macos)).is_ok());
  assert!(runner_platform_gate("input.focusText", focus_text, Some(DevicePlatform::Windows)).is_ok());

  // Partial proceeds: nowPlaying is Partial on Windows.
  let now_playing = registry.resolve("mediaControl.nowPlaying").unwrap();
  assert!(runner_platform_gate("mediaControl.nowPlaying", now_playing, Some(DevicePlatform::Windows)).is_ok());

  // Direct shorthand: pointerPosition is Unsupported on Linux only.
  let pointer = registry.resolve("input.pointerPosition").unwrap();
  assert!(runner_platform_gate("input.pointerPosition", pointer, Some(DevicePlatform::Linux)).is_err());
  assert!(runner_platform_gate("input.pointerPosition", pointer, Some(DevicePlatform::Windows)).is_ok());

  // A missing or unreported platform never refuses.
  assert!(runner_platform_gate("input.focusText", focus_text, None).is_ok());
  assert!(runner_platform_gate("input.focusText", focus_text, Some(DevicePlatform::Unspecified)).is_ok());

  // Fixtures have no Runner route; their upstream refusal owns that case.
  let fixture = registry.resolve("scan.frame").unwrap();
  assert!(runner_platform_gate("scan.frame", fixture, Some(DevicePlatform::Linux)).is_ok());
}

// ROOT CAUSE:
//
// If the selected Runner's platform was known and a command's Runner support
// cell for that platform was Unsupported, dispatch still issued the capability
// RPC and surfaced the backend's refusal (before the typed-failure phase,
// flattened to `command_failed`).
//
// Before the fix, input.focusText against a Linux Runner reached the Runner
// and failed remotely. The fix refuses the dispatch pre-RPC as a typed
// `unsupported` failure that names the command and the Runner platform.
#[tokio::test]
async fn selected_dispatch_refuses_a_declared_unsupported_runner_platform_before_resolution() {
  let error = invoke(gate_input_targeted("input.focusText", false), gate_context(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect_err("a Linux Runner has no focusText path");
  assert_eq!(error.code, crate::FailureCode::Unsupported);
  assert_eq!(error.message, "input.focusText has no Linux Runner path; the declared Runner support for Linux is unsupported");
}

#[tokio::test]
async fn selected_dispatch_without_a_platform_fact_reaches_placement() {
  // No platform fact in the context: the gate stays open and the dispatch
  // proceeds until the (deliberately invalid) endpoint fails placement.
  let error = invoke(gate_input_targeted("input.focusText", false), gate_context_past_the_gate(None))
    .await
    .expect_err("the invalid endpoint must fail placement");
  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert!(!error.message.contains("Runner path"), "{}", error.message);
}

#[tokio::test]
async fn selected_dispatch_decides_on_the_runner_platform_not_the_local_os() {
  // Runs on a Windows host: the Linux cell refuses while the Partial Windows
  // cell proceeds, so only the Runner's reported platform drives the gate.
  let refused = invoke(gate_input("mediaControl.nowPlaying", false), gate_context(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect_err("a Linux Runner has no now-playing path");
  assert_eq!(refused.code, crate::FailureCode::Unsupported);
  assert!(refused.message.contains("has no Linux Runner path"));

  let proceeded =
    invoke(gate_input("mediaControl.nowPlaying", false), gate_context_past_the_gate(Some(auv::devices::DevicePlatform::Windows)))
      .await
      .expect_err("the Partial Windows cell must proceed to placement");
  assert_eq!(proceeded.code, crate::FailureCode::CommandFailed);
  assert!(!proceeded.message.contains("Runner path"), "{}", proceeded.message);
}

#[tokio::test]
async fn selected_dry_runs_bypass_the_platform_gate() {
  let error = invoke(gate_input_targeted("input.focusText", true), gate_context_past_the_gate(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect_err("a dry run proceeds past the gate and fails at the invalid endpoint");
  assert_eq!(error.code, crate::FailureCode::CommandFailed);
  assert!(!error.message.contains("Runner path"), "{}", error.message);
}

#[tokio::test]
async fn fixture_refusal_stays_identical_with_a_known_platform() {
  let error = invoke(gate_input("scan.frame", false), gate_context(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect_err("fixtures have no Runner route regardless of platform");
  assert_eq!(error.code, crate::FailureCode::Unsupported);
  assert_eq!(error.message, "scan.frame has no Runner route; run it without a selected Run or Device");
}

// Builds a valid typed overlay invocation; `extra` appends flags such as
// `--no-overlay`, mirroring how the CLI frontend produces overlay inputs.
fn overlay_gate_input(extra: &[&str]) -> crate::InvokeCommandInput {
  let mut arguments = vec![
    "overlay.outline",
    "--x",
    "10",
    "--y",
    "20",
    "--width",
    "30",
    "--height",
    "40",
  ];
  arguments.extend_from_slice(extra);
  let crate::InvokeCliParse::Invoke {
    command_id,
    target,
    inputs,
    typed_args,
    dry_run,
    ..
  } = crate::parse_invoke_args(&arguments.into_iter().map(str::to_string).collect::<Vec<_>>()).expect("parse overlay")
  else {
    panic!("expected overlay invocation");
  };
  crate::InvokeCommandInput {
    command_id,
    target,
    inputs,
    typed_args: Some(typed_args),
    dry_run,
    cancellation: Default::default(),
  }
}

// ROOT CAUSE:
//
// If the selected Runner's platform was known and the command's Runner cell
// for it was Unsupported, the platform gate refused before the disabled-
// overlay early return, so `overlay.outline --no-overlay` — a call that
// validates and answers locally without any capability RPC — failed as
// `unsupported` on Linux and Windows Runners.
//
// Before the fix, the gate sat ahead of the overlay block. The fix keeps the
// local disabled-overlay answer ahead of the gate; only an overlay that will
// really issue its Runner RPC can be refused by the platform cell.
#[tokio::test]
async fn selected_disabled_overlay_stays_local_on_an_unsupported_runner_platform() {
  let output = invoke(overlay_gate_input(&["--no-overlay"]), gate_context(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect("a disabled overlay answers locally regardless of the Runner platform");
  assert_eq!(output.report.expect("overlay report").fields.last().expect("overlay status").value, "disabled");
}

#[tokio::test]
async fn selected_enabled_overlay_is_still_gated_on_an_unsupported_runner_platform() {
  let error = invoke(overlay_gate_input(&[]), gate_context(Some(auv::devices::DevicePlatform::Linux)))
    .await
    .expect_err("an overlay that would issue its Runner RPC is refused pre-RPC");
  assert_eq!(error.code, crate::FailureCode::Unsupported);
  assert_eq!(error.message, "overlay.outline has no Linux Runner path; the declared Runner support for Linux is unsupported");
}
