# AUV Driver Permission Probe API Design

Date: 2026-06-05

## Context

`debug.probePermissions` currently builds a permission report in the root
macOS command adapter by calling `auv-driver-macos` native permission helpers
directly and by probing System Events automation from root support code.

The native macOS probe exists, but there is no shared `auv-driver` typed
contract and no `MacosDriverSession` permission API.

## Goals

- Add a small typed permission probe contract to `auv-driver`.
- Expose `session.permission().probe()` from `auv-driver-macos`.
- Keep `debug.probePermissions` as the CLI/report/artifact surface while moving
  platform probing behind the typed driver session.
- Emit stable permission signals from the command response.

## Non-Goals

- Do not change the command catalog ID or CLI arguments.
- Do not add permission prompting or remediation flows.
- Do not change overlay behavior.
- Do not move unrelated OCR, AX, or app analysis commands in this slice.

## API Shape

`auv-driver` owns:

```rust
pub enum PermissionStatus {
  Granted,
  Missing,
  Unknown,
}

pub struct PermissionProbe {
  pub screen_recording: PermissionStatus,
  pub screen_capture_kit: PermissionStatus,
  pub accessibility: PermissionStatus,
  pub automation_to_system_events: PermissionStatus,
}
```

`auv-driver-macos` exposes:

```rust
session.permission().probe() -> DriverResult<PermissionProbe>
```

## Data Flow

```text
debug.probePermissions
  -> typed session bridge
  -> MacosDriverSession::permission().probe()
  -> native permission probe + System Events automation probe
  -> PermissionProbe
  -> root report / artifact / signals
```

## Signals

The command response emits:

- `permission.screen_recording`
- `permission.screen_capture_kit`
- `permission.accessibility`
- `permission.automation_to_system_events`

## Deferred Work

`TODO(remove-legacy-driver-call-adapter)` still applies to the typed session
bridge. Once `Runtime::invoke` can open typed driver sessions directly, the
root bridge should be removed and `debug.probePermissions` should call the typed
runtime path instead of the legacy `DriverCall` handler.
