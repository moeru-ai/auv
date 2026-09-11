# Click modifiers

Date: 2026-09-11. Classification: approved feature, BG-1 standard modifier state.
The owner selected `ClickModifiers` after reviewing key identity and hold
semantics. This slice connects existing point-click consumers to that contract.

## Contract and execution

`auv-driver-common::ClickModifiers` contains four independent booleans:
`shift`, `control`, `alt`, and `meta`. All default to false. macOS maps Alt to
Option and Meta to Command. The type describes mouse-event flags, not a list
of physical keys or a keyboard layout. Unknown JSON fields are rejected.

Window clicks carry this type in `ClickOptions.modifiers`. Global Rust
`click_at` and Runner `click_screen_point` take it as an explicit argument.
The existing window strategy, click count, interval, delivery policy, and
`InputActionResult` retain their responsibilities. `verified` remains false.

macOS converts the shared type to Quartz flags behind `native::pointer`.
The foreground route and both existing window routes stamp flags on mouse
down/up events. Window compatibility move/primer events carry the same state.
No keyboard-down event is synthesized, and no keyboard hold survives the call.
Windows foreground clicks use one `SendInput` batch per click: modifier downs,
mouse down/up, and modifier ups in reverse order. Already-held modifiers are
not injected or released. If only a prefix is delivered, cleanup releases the
remaining injected keys and mouse button and reports delivery/release failure.
Windows background messages carry Shift/Control in `wParam` on down and up;
Alt/Meta are rejected for both background policies before target activation.
Use `ForegroundPreferred` for those modifiers; no implicit focus fallback is
introduced. Meta maps to the Windows key.

Linux RemoteDesktop Portal clicks scope modifier key transitions around the
click sequence, then attempt every release in reverse order even after a failed
press reply or click. Release failures are reported and the owning API drops
the failed portal session. Meta maps to Super. Background-only window delivery
remains unsupported. Portal input cannot query the user's existing key state;
this is not an isolation guarantee from concurrently held physical modifiers.

These platform mappings follow the native contracts:
[Windows mouse message flags](https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-lbuttondown),
[SendInput ordering and existing key state](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput),
and [RemoteDesktop keyboard/pointer events](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html).

`input.clickPoint --modifiers cmd,shift` and
`input.clickPoint --modifiers cmd --modifiers shift` are equivalent. Repeated
options can also contain comma-separated names. They serialize to one
comma-separated invoke argument for Runner dispatch and recording/replay.
Both forms use the same parser for local and
Runner dispatch. Accepted names are `shift`, `control`/`ctrl`, `alt`/`option`,
and `meta`/`cmd`/`command`. Unknown names, empty lists and duplicate aliases are
errors. The argument is retained in typed invoke input for recording/replay.
Library callers use the boolean fields directly.

The CLI regression test covers repeated flags, comma-separated names, and
mixed forms through typed arguments, protocol decoding, and replay decoding.
`cargo test -p auv-cli-invoke --lib --quiet`: 77 passed, 1 ignored after this
CLI extension.

## Wire and caller migration

Protobuf adds `ClickOptions.modifiers = 4` and `ScreenClickOptions.modifiers = 2`,
both containing a dedicated `ClickModifiers` message. Absence means no requested
modifiers. Existing serialized Rust click records without the new field also
decode with empty modifiers; a regression test defines this read boundary.

The JS screen helper now accepts `ScreenClickOptions`, matching the window
helper's options shape:

```ts
await runner.input.clickScreenPoint(
  { x: 100, y: 80 },
  { click: { count: 1 }, modifiers: { meta: true, shift: true } },
)
```

Migrate old `clickScreenPoint(point, { count: 1 })` calls to
`clickScreenPoint(point, { click: { count: 1 } })`. Rust screen callers add an
explicit modifier argument (`Default::default()` for existing ordinary clicks).
Generated SDK files remain ignored and are produced by the existing Buf template.

Clients and Runners must both include this contract to use modifiers. Old
Protobuf receivers can ignore the new fields, so mixed-version modifier delivery
is not a supported combination. Capability negotiation is intentionally deferred
until an owner-approved Runner version/capability slice; no claim of old-Runner
modifier support is made.

## Evidence and validation

Unit tests cover modifier preservation in serialized input, legacy absence,
unknown fields, CLI aliases and rejections, Runner mappings, and Quartz flag
mapping. The SDK test decodes the actual outgoing Protobuf request for both
window and screen clicks. Windows tests check event ordering, partial-delivery cleanup, and background
flag mapping. Linux tests check platform keys and modifier cleanup after press,
click, or release failure. Both retain background-policy rejection tests.

The opt-in `auv-driver-macos/tests/click_modifiers.rs` test compiles an independent
AppKit receiver from `tests/fixtures/click_modifiers.swift`. It observes target
window mouse down/up and flags for both background routes and the foreground
route, followed by an ordinary click. It does not use driver success as the
receipt and does not verify application semantics. Run it in a logged-in macOS
GUI with Accessibility permission and window-list access:

```sh
cargo test -p auv-driver-macos --test click_modifiers -- --ignored --nocapture
```

Live AppKit evidence on macOS 26.3 (25D2125), arm64: the opt-in test passed.
The same receiver window (196834 in this run) logged the following mouse events.
The mask for all four modifiers was `1966080` (`0x1e0000`). The subsequent
ordinary click logged `0`, confirming that request flags did not carry over.

| Requested policy / route | Receiver active | Modified event types | Ordinary event types | Flags |
| --- | --- | --- | --- | --- |
| BackgroundOnly / PidTargeted | false | down, up | down, up | 1966080, then 0 |
| BackgroundOnly / ChromiumCompatible | false | down, down, up, up | down, down, up, up | 1966080, then 0 |
| ForegroundPreferred / global HID | true | down, up | down, up | 1966080, then 0 |

The duplicated AppKit events on ChromiumCompatible are a concrete BG-2
follow-up candidate: dual posting delivered duplicate target events on this
receiver, which matters for callers relying on single activation. This slice
does not change transport selection or claim exactly-one-pair semantics for
that route. AppKit receipt is not evidence that an Electron or input-forwarding
application consumes the events or performs the desired action.

Other checks on the same host:

- `cargo check --workspace`: passed.
- `cargo test -p auv-driver-common -p auv-driver-macos -p auv-cli-invoke -p auv-cli -p auv --lib --bins --quiet`: 293 passed, 7 ignored.
- `cargo test --quiet` (default CLI member, including integration tests): 79 passed, 1 ignored; overlaps the CLI unit tests above.
- `cargo test -p auv-driver-windows -p auv-driver-linux click_modifiers --quiet`: platform key mapping tests passed on the macOS host; native receipt is separate.
- `pnpm --filter @auv-js/sdk test:run src/apis/auv/driver.test.ts`: 3 passed.
- `scripts/generate-swift-bridge` and SwiftPM `swift build`: passed.
- `cargo fmt --check`, `git diff --check`, and ESLint on both touched SDK files: passed.
- From `proto/`: `buf generate --template buf.gen.yaml`, `buf breaking --against '../.git#branch=main,subdir=proto'`, and `buf format --diff auv/api/driver/v1/input.proto`: passed.
- CLI dry-run accepted `--modifiers cmd,shift` and rejected `--modifiers space` without delivery.

Existing validation failures outside this slice:

- `buf lint`: `InputService.MoveMouse` uses `MoveMouseStreamResponse`, which fails the existing RPC response naming rule at `input.proto:35`. The baseline already has this name.
- Workspace-wide `buf format --diff` reports ordering of Java options in `grpc/reflection/v1/reflection.proto`; the changed input schema is formatted.
- SDK `typecheck`: existing `AbortSignal.any` declarations fail in `client.ts:118`, `driver.ts:243`, and `node/daemon.ts:238`. The click schema and helper changes introduce no reported TypeScript errors.

## Linux and Windows follow-up validation

The Linux/Windows implementation extends the original macOS slice in the same
PR. Evidence levels remain separate:

- **Linux native tests:** `ssh neko-gpu-1` connected to Debian x86_64
  (`steam-deck-55d`, kernel `7.1.3+deb13-amd64`). In the isolated source directory
  `/tmp/auv-click-modifiers.6uouEh`, `cargo +1.95.0 test -p auv-driver-linux --lib`
  passed all 67 tests, including modifier release failure cases. The existing
  remote checkout and its uncommitted changes were not modified.
- **Linux live input blocked:** the existing Sway session has a live
  `wayland-1` socket, session D-Bus, PipeWire, and ScreenCast. The Portal's
  `Properties.Get(org.freedesktop.portal.RemoteDesktop, version)` returns
  `No such interface`. Its active `xdg-desktop-portal-wlr` backend does not
  expose RemoteDesktop in this session, so no modifier-click receipt is claimed.
  The driver probe `cargo +1.95.0 run -p auv-driver-linux --example validate
  -- click 100 100` independently failed at `RemoteDesktop.CreateSession`
  before pointer delivery with the same missing-interface error.
  Test with an existing desktop session whose Portal implements RemoteDesktop;
  no system packages, services, or Portal selection were changed here.
- **Windows compilation:** `cargo check -p auv-driver-windows --all-targets
  --target x86_64-pc-windows-msvc` passed, including native event-batch tests
  and the independent Win32 receiver. Compilation does not execute these tests.
- **Windows remote blocked:** three BatchMode SSH attempts to
  `luoling-windows-11` (`10.0.0.132:22`) timed out. No Windows receipt is claimed.
- Running the entire Windows test library on macOS also encounters two existing
  host-specific failures: `rejects_buffer_with_mismatched_length` expects the
  Windows OCR validator instead of the non-Windows stub, and
  `window_click_background_only_fails_for_invalid_window_handle` expects
  `ScreenToClient` instead of the non-Windows unsupported error.

On Windows, run the ordinary native tests and then the opt-in receiver test:

```sh
cargo test -p auv-driver-windows --lib
cargo test -p auv-driver-windows window_receives_modified_click_messages -- --ignored --nocapture
```

The receiver owns a hidden Win32 window, independently logs dispatched mouse
messages, and checks Shift/Control on both down/up followed by a plain click.
It does not claim foreground Alt/Meta receipt. Foreground input tests need the
logged-in interactive desktop; an SSH service session may not have that access.

## Intentional deferrals

- Arbitrary native keycodes, right/left modifiers, and keyboard layout semantics
  remain a separate keyboard identity contract, reopened with a concrete consumer.
- Physical modifier transitions for remote-desktop/input-forwarding consumers
  still need application-specific receipt evidence, including on Windows/Linux
  paths that use key transitions. Mouse flags alone do not claim that support.
- Hold/drag across calls requires ownership, cancellation, release and partial
  progress semantics; it is not added as a click option.
- Button selection remains the separate BG-1 parameter slice.
- BG-2 compatibility dual posting and count capping are unchanged. The receiver
  here checks modifier state, not exact click cardinality or semantic activation.
