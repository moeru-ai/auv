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

The root legacy driver registry and command adapter were removed after direct
invoke routing became the only active command lane. `src/driver/**`,
`DriverCall`, `DriverResponse`, `DriverRegistry`, and `auv-cli list-drivers`
are gone; command discovery is `auv-cli invoke --help`.

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

## Remaining Evidence TODOs

The current direct invoke evidence lane is intentionally handler-owned: command
functions construct `InvokeCommandOutput { signals, artifacts, known_limits,
verification }`, and runtime only records what the handler returns. The
following gaps are known and intentionally left visible at the code site with
`TODO:` markers.

### Boundary Claim Model

- Code marker: `TODO(invoke-boundary-claims)`.
- Current state: `InvokeCommandOutput::verification` is a human-readable command
  boundary claim recorded as `command.verification` and rendered under
  `Command Boundary Claims`.
- Deferred gap: this is not yet a first-class read-side model separate from
  typed semantic `VerificationResult`.
- Reason: capture-only, recognition-only, read-only, and activation-only claims
  must not be misrepresented as semantic verification results.
- Reopen trigger: define an accepted boundary-claim schema for inspect CLI,
  inspect server, and future viewer APIs.

### Capture Contract Artifacts

- Code marker: `TODO(invoke-capture-contract-artifacts)`.
- Current state: `display.capture`, `screen.captureRegion`, and
  `window.capture` return screenshot artifacts plus scalar capture/display
  signals.
- Deferred gap: they do not yet emit standalone capture-contract artifacts.
- Reason: the old root adapter had capture-contract artifacts, but direct invoke
  needs an accepted JSON shape that is independent of the removed driver
  operation route.
- Reopen trigger: accept the direct-invoke capture-contract JSON shape, then add
  artifacts from each capture handler without reintroducing macro metadata.

### Recognition Result Artifacts

- Code marker: `TODO(invoke-recognition-result-artifacts)`.
- Current state: screen/window OCR commands record source screenshots when the
  handler has a capture and expose match count / best text as signals.
- Deferred gap: they do not yet emit a structured `recognition-result` artifact
  containing query, all matches, bounds, confidence, source capture reference,
  and coordinate-space metadata.
- Reason: this should be a durable artifact contract consumed by inspection and
  candidate promotion, not a one-off JSON dump local to `auv-cli-invoke`.
- Reopen trigger: accept the recognition-result artifact shape and wire the
  screen/window OCR handlers to emit it.

### Paste Input Action Result

- Code marker: `TODO(invoke-paste-input-action-result)`.
- Current state: `input.pasteText` records activation-only boundary text,
  clipboard disturbance signal, and known limits.
- Deferred gap: it does not persist a typed `InputActionResult` artifact.
- Reason: the typed paste API currently returns success/failure only, unlike
  `input.typeText` and `input.key`, which return `InputActionResult`.
- Reopen trigger: extend the typed paste API to return delivery evidence, then
  persist it as an `input-action-result` artifact.

### Window Capture Backend Stability

- Code marker: `TODO(invoke-window-capture-backend)`.
- Current state: window capture/OCR handlers are wired to produce boundary
  claims and artifacts when capture succeeds.
- Deferred gap: live verification on 2026-06-18 reproduced single-window capture
  failures for Chrome and NetEase Cloud Music:
  ScreenCaptureKit timed out after 10 seconds and xcap fallback failed to copy
  window data.
- Reason: this is a typed window capture backend reliability problem, not an
  invoke evidence-shape problem.
- Reopen trigger: stabilize or replace the typed window capture backend, then
  rerun the live window.capture/window.findText/window.clickText smoke checks.

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
- root `src/driver/**`
- root `DriverCall` / `DriverResponse` / `DriverRegistry`
- `auv-cli list-drivers`
- `steam.library.list.v0`

## Verification

- `cargo check`
- `cargo fmt --check`
- `cargo test -p auv-cli-invoke -p auv-cli-invoke-macros`
- `cargo test invoke_resolved -- --nocapture`
- `cargo test mcp -- --nocapture`
