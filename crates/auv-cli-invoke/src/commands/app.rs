use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{NO_ARGS, TARGET_ARGS},
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("app", "APP")
    .command(probe_permissions_invoke_command())
    .command(activate_app_invoke_command())
}

#[invoke_command(
  id = "app.probePermissions",
  group = "app",
  summary = "Probe macOS screen recording, accessibility, and automation permissions.",
  args = NO_ARGS,
)]
fn probe_permissions(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new(
      "dry run: app.probePermissions would probe macOS permissions",
    ));
  }
  probe_permissions_impl()
}

#[invoke_command(
  id = "app.activate",
  group = "app",
  summary = "Bring a target macOS app to the foreground before a foreground-dependent step.",
  args = TARGET_ARGS,
)]
fn activate_app(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-app-activation): app activation still lives behind the root
  // macOS command adapter; migrate it to `auv-driver-macos` before enabling
  // this direct invoke command.
  Err("app.activate requires a typed app activation API in auv-driver-macos".to_string())
}

#[cfg(target_os = "macos")]
fn probe_permissions_impl() -> InvokeCommandResult {
  use auv_driver::Driver;

  let driver = auv_driver_macos::MacosDriver::new();
  let session = driver.open_local().map_err(|error| error.to_string())?;
  let permissions = session
    .permission()
    .probe()
    .map_err(|error| error.to_string())?;
  let mut output = InvokeCommandOutput::new("macOS permissions probed");
  output.backend = Some("auv-driver-macos.permission".to_string());
  output.signals.insert(
    "permission.screen_recording".to_string(),
    permissions.screen_recording.as_str().to_string(),
  );
  output.signals.insert(
    "permission.screen_capture_kit".to_string(),
    permissions.screen_capture_kit.as_str().to_string(),
  );
  output.signals.insert(
    "permission.accessibility".to_string(),
    permissions.accessibility.as_str().to_string(),
  );
  output.signals.insert(
    "permission.automation_to_system_events".to_string(),
    permissions.automation_to_system_events.as_str().to_string(),
  );
  output.verification = Some("read-only; no semantic success claim".to_string());
  output
    .known_limits
    .push("app.probePermissions records current permission status only; it does not verify an application workflow.".to_string());
  Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn probe_permissions_impl() -> InvokeCommandResult {
  Err("app.probePermissions is only available on macOS".to_string())
}
