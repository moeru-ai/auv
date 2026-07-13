# macOS osascript Backend Design

## Context

AUV currently uses `/usr/bin/osascript` for a small set of macOS automation
operations, mainly app activation and System Events keyboard input. The broader
macOS driver also uses Swift scripts for screen, window, AX, OCR, pointer, and
clipboard functionality. Swift-backed behavior is covered by the Swift bridge
migration design.

`osascript` remains useful for Apple Events, application-specific scripting
dictionaries, JavaScript for Automation, and quick automation surfaces exposed
by scriptable apps. It should not be the general macOS driver substrate. It is a
subprocess wrapper around Open Scripting Architecture scripts and should be
treated as a narrow, auditable backend.

## Goals

- Keep `osascript` usage small and explicit.
- Build an internal AUV wrapper instead of depending on a third-party
  osascript crate.
- Use `/usr/bin/osascript` directly, not PATH lookup.
- Avoid accepting arbitrary recipe-provided AppleScript or JXA.
- Separate osascript-backed application automation from screen, window, AX tree,
  OCR, pointer, clipboard, and native keyboard modules.
- Preserve structured errors and command artifacts where osascript-backed
  operations are user-visible.

## Non-Goals

- Do not expose a general AppleScript execution command.
- Do not use osascript for screen OCR, window listing, AX tree capture, pointer
  click/scroll, keyboard input, or clipboard operations once native Swift or
  Rust implementations cover those paths.
- Do not introduce `osascript-rs`, `osascript`, or another wrapper crate in this
  phase.
- Do not build a full Apple Events framework binding.

## Allowed Scope

The osascript backend may support:

```text
app activation by bundle id or app name
Automation permission probing
fixed application-specific AppleScript or JXA operations for scriptable apps
```

// TODO: Define the first scriptable-app allowlist after concrete browser or app
// automation needs appear. Do not add a generic "run script" operation.

These uses should live behind capability modules:

```text
src/driver/macos/osascript/
  mod.rs
  app.rs
  permission.rs
  scriptable_app.rs
```

The public command namespace should not include `osascript`. Commands should be
named by behavior:

```text
debug.activateApp
debug.probePermissions
```

Keyboard commands such as `debug.typeText` and `debug.pressKey` should move to
a native keyboard backend, either Swift bridge or a Rust-native implementation
using platform event APIs. Rust crates such as `rdev` can send and listen for
keyboard and mouse events on macOS, Windows, and Linux/X11, but AUV should
evaluate them as native input backends, not as part of the osascript backend.

## Crate Decision

Existing crates are not a good fit for AUV's current needs:

- `osascript-rs` is a very small wrapper and macro layer around AppleScript
  execution.
- `osascript` focuses on JavaScript for Automation and typed parameter/result
  handling.
- higher-level automation crates include extra abstraction that does not match
  AUV's driver and artifact model.

AUV needs tight control over binary path, allowed operations, error rendering,
disturbance classification, and inspectable artifacts. A local wrapper is
smaller and easier to audit.

## Execution Model

The wrapper should expose fixed operations, not raw script execution:

```rust
activate_app(target: &str) -> AuvResult<OsascriptOutput>
probe_system_events_automation() -> PermissionProbe
run_supported_app_automation(operation: ScriptableAppOperation) -> AuvResult<OsascriptOutput>
```

Internally, scripts should be assembled from fixed fragments and escaped values.
All execution should use:

```text
/usr/bin/osascript
```

with `-e` arguments. The wrapper should capture stdout, stderr, exit status,
operation name, and backend name.

## Security and Safety Rules

- No arbitrary AppleScript or JXA from recipes.
- No PATH-based `osascript` lookup.
- No temporary script files for osascript execution.
- String escaping must be centralized and tested.
- Backend errors should include stderr but avoid leaking large script bodies into
  user-facing summaries.
- System Events input should remain classified as foreground and keyboard
  disturbance.

`osascript` does not solve the runtime script injection concern by itself. It is
safe enough only when AUV exposes fixed, narrow operations.

## Relationship to Swift Bridge

Swift bridge or Rust-native platform code should become the preferred backend
for Apple framework-heavy driver functionality:

```text
screen capture and OCR
window listing and resolving
AX tree capture
pointer click and scroll
keyboard input
clipboard snapshot/set/restore
permission probes where direct framework checks are better
```

osascript should remain only where Apple Events, app scripting dictionaries, or
JXA are the right semantic layer. Examples include app activation and fixed
automation operations for scriptable applications such as browsers, media apps,
or productivity tools.

## Doctor and Permission Diagnostics

Permission diagnostics should not be scattered across Swift bridge, osascript,
and Rust-native input backends. Add a dedicated macOS permission diagnostics
module that can be called by both driver commands and a future CLI doctor
surface.

The CLI should grow a command such as:

```text
auv-cli doctor
auv-cli doctor macos
```

The doctor command should report Accessibility, Screen Recording, Automation,
input event, clipboard, and scriptable-app readiness where relevant. It should
also provide concrete repair hints instead of backend-specific raw errors.

## Artifact and Error Model

User-visible osascript-backed commands should produce a small text report when
useful:

```text
backend=macos.osascript.system-events
operation=press_key
targetApp=...
startedAt=...
finishedAt=...
status=...
stderr=...
```

Most reports should be concise because these operations usually produce no rich
state. The run trace should still record command invocation and disturbance
metadata.

## Testing

Unit tests should cover:

- AppleScript string escaping
- app activation script rendering for bundle ids and app names
- key and shortcut parsing
- fixed operation builders do not accept raw scripts
- error mapping from failed subprocess output

Live macOS validation should cover:

- activating an app by bundle id
- running one fixed scriptable-app automation operation when available
- probing Automation/System Events permission behavior
