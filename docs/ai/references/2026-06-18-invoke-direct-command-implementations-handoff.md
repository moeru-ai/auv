# Invoke Direct Command Implementations Handoff

Date: 2026-06-18

## Summary

`auv-cli-invoke` no longer routes invoke commands through
`driver_id` / `operation` strings. `#[invoke_command]` registers command
metadata plus the implementation function pointer. The function returns
`InvokeCommandOutput` directly.

Root runtime records invoke command output on the `auv.command.invoke` span.
It no longer creates an invoke-only `auv.driver.invoke` span, and
`InvokeResult.producer_span_id` now points at the command span for invoke
commands.

Steam invoke support was removed. No compatibility command or fallback route is
kept.

## Direct Implementations

These commands now call typed APIs or produce deterministic direct output:

- `fixture.observe`
- `app.probePermissions`
- `display.capture`
- `display.list`
- `screen.captureRegion`
- `screen.findText`
- `screen.waitForText`
- `screen.clickText`
- `window.list`
- `window.capture`
- `window.findText`
- `window.waitForText`
- `window.clickText`
- `input.typeText`
- `input.pasteText`
- `input.key`

## Explicit Typed-API Gaps

These commands intentionally return explicit errors instead of routing through
root driver operation strings:

- `app.activate`
- `display.identifyPoint`
- `display.probeCoordinateReadiness`
- `display.projectScreenshotPoint`
- `screen.findRows`
- `screen.waitForRows`
- `screen.findImageText`
- `screen.clickRow`
- `window.captureAxTree`
- `window.findRows`
- `window.waitForRows`
- `window.observeRegion`
- `window.findIconMatch`
- `window.scrollRegion`
- `window.verifyText`
- `window.clickRow`
- `input.focusText`
- `input.pressButton`
- `input.axPressButton`
- `input.axFocusText`
- `input.axClickWindowText`
- `input.smartPress`
- `input.clickPoint`
- `input.clickWindowPoint`
- `input.teachClick`
- `input.scrollPoint`
- `overlay.*`
- `mediaControl.*`

The deferral trigger is the same for each gap: move or expose the owning typed
capability in `auv-driver-macos`, an overlay crate, `auv-media-macos`, or a
future interaction crate, then replace the command-local error with a direct
call.

`scroll_scan::scan_window_region` currently fails fast at
`window.observeRegion` for the same reason: scroll scan still needs a typed
region-observation capability plus artifact/evidence recording that does not
belong in `auv-cli-invoke`.

## Removed Adapter Code

- `InvokeCommandExecution`
- `InvokeCommandExecution::driver_operation`
- root runtime `DriverOperationRoute`
- root runtime `invoke_driver_operation_in_span`
- `steam.library.list.v0`

## Verification

- `cargo check`
- `cargo fmt --check`
- `cargo test -p auv-cli-invoke -p auv-cli-invoke-macros`
- `cargo test invoke_resolved -- --nocapture`
- `cargo test mcp -- --nocapture`
