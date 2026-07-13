# AUV Driver Foreground Input API Design

Date: 2026-06-05

## Context

The macOS command adapter still sends active-control keyboard input through
legacy root helper functions for `debug.typeText` and `debug.pressKey`.
`auv-driver` already owns typed input result reporting through
`InputActionResult`, and `auv-driver-macos` already exposes typed window input,
paste, click, and scroll APIs. The remaining gap is foreground active-control
typing and key pressing.

This slice is an approved feature. It keeps the current seam:

```text
recognition / AX / candidates
  -> ActionResolver
  -> auv-driver InputActionResult
  -> OperationResult / VerificationResult / trace artifacts
```

## Goals

- Add typed foreground active-control input APIs for text typing and key
  pressing.
- Route `debug.typeText` and `debug.pressKey` through the typed macOS driver
  session bridge.
- Preserve existing CLI parameters and behavior for:
  - `text`
  - `replace_existing`
  - `submit_key`
  - `submit_settle_ms`
  - `key`
  - `settle_ms`
  - `activate`
- Emit bridge metadata from the legacy command response so traces can show that
  typed input was used.

## Non-Goals

- Do not change `ActionResolver`.
- Do not treat `auv-overlay-macos` as an input backend.
- Do not implement MCP invocation.
- Do not require a target window for these commands; they remain active-control
  foreground commands.
- Do not remove all legacy macOS input helpers in this slice. Helpers that
  remain reachable from other legacy commands should stay until their command
  path is migrated.

## API Shape

`auv-driver` gets a small key press options type:

```rust
pub struct KeyPressOptions {
  pub key: String,
  pub settle: Duration,
}
```

`auv-driver-macos::InputApi` gets:

```rust
pub fn type_text(&self, text: &str, options: TypeTextOptions)
  -> DriverResult<InputActionResult>;

pub fn press_key(&self, options: KeyPressOptions)
  -> DriverResult<InputActionResult>;
```

Both APIs return `InputActionResult` with
`InputDeliveryPath::ForegroundSystemEvents`. The text API reuses
`TypeTextOptions` so the same typed contract can represent background and
foreground typing; foreground callers should use `InputPolicy::ForegroundPreferred`.

## Data Flow

`debug.typeText`:

1. Parse existing CLI parameters.
2. Optionally activate the app through the existing activation helper.
3. Call the typed session bridge.
4. Build the same text artifact/report as before.
5. Add bridge signals:
   - `input.bridge=typed-session`
   - `input.bridge.selectedPath=foreground_system_events`
   - `input.bridge.policy=foreground_preferred`

`debug.pressKey` follows the same path with key press options.

## Error Handling

- Unsupported submit keys and key strings should fail through the typed API,
  not through a second result schema.
- Existing user-facing validation messages should stay compatible where
  practical.
- If opening the typed macOS session fails, the command returns the typed
  bridge error.

## Tests

- Unit test key parsing for supported special keys, shortcuts, single
  characters, and invalid multi-character keys.
- Unit test command response bridge signals without requiring live keyboard
  input where possible.
- Run repository validation for Rust behavior changes:
  - `cargo fmt --check`
  - `cargo check`
  - `cargo test`
  - `git diff --check`

## Deferred Work

- TODO(remove-legacy-driver-call-adapter): delete the temporary typed session
  bridge after `Runtime::invoke` can call typed driver sessions directly.
- TODO(foreground-input-target-lease): target-aware foreground typing is
  deferred until the owner approves a slice that connects active-control input
  to window preparation and restoration leases.
