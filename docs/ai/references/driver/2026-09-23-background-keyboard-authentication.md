# Background keyboard event authentication

Date: 2026-09-23. Classification: approved feature. Status: authentication
implemented; receiver acceptance varies by action and application.

The owner approved adding the keyboard authentication mechanism discussed in
the [BG-2 comparison](2026-09-23-background-delivery-project-comparison.md).
It changes process-targeted keyboard submission for text, keys, and
combinations in the macOS native boundary. Mouse posting and multi-click
counts are separate concerns.

## Delivery contract

The native code stamps each process-targeted keyboard event, prepares an
authentication message, and submits the event once through `SLEventPostToPid`.
Missing symbols, a missing factory selector, or preparation failure select
the existing public `CGEvent.postToPid` route. There is no second submission
after a SkyLight post. Foreground input still uses the session event tap.

The fallback preserves behavior on hosts without the complete private
capability. The public API does not expose a toolkit heuristic or an
authentication-status field. `InputActionResult` describes submission and
keeps `verified: false` until a separate receiver check establishes an effect.

## Native boundary

CUA supplies the authentication factory and setter reference, including the
missing factory selector on macOS 14: [pinned source](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L308-L351).

On macOS 26.3 arm64, a standalone no-post probe resolved the factory,
`SLEventGetEventRecord`, and setter. The native getter copies into a
caller-owned buffer with signature `(CGEventRef, void *, uint32_t) -> CGError`.
The observed record length was 248 bytes. A wrong buffer length caused an
assertion in the native getter, so the implementation rejects other lengths
before calling it. It does not parse private CGEvent offsets.

`EventPosting.swift` owns symbol resolution, Objective-C signature checks,
record copying, message lifetime, and single submission. `Keyboard.swift`
calls it after stamping the recipient. An autorelease pool bounds the
message lifetime; the record and message remain alive through attachment and
posting. No Rust/Swift wire declarations or public operation parameters
changed for authentication.

## Validation boundary

- `scripts/generate-swift-bridge`, native macOS `swift build`, `cargo check`,
  default `cargo test`, and focused driver library tests passed.
- `cargo test -p auv-driver-macos --test keyboard_authentication` passed on
  macOS 26.3 arm64. Its injected boundary asserts copy, factory, attachment,
  and exactly one private post in order. Missing API, ABI mismatch, copy
  failure, and factory failure cannot post. It also exercises native
  preparation while intercepting the final post.
- Earlier local AppKit and Chrome/Electron probes observed accepted Unicode
  input and several physical-key combinations, as well as missing events.
  The separate evaluation PR owns the reproducible GUI harness. Raw receipts
  and temporary probes are not part of this PR.

The historical Chrome/Electron background matrix had 30/45 and 26/45
complete cases respectively on one macOS 26.3 host. Both applications
failed all five background emoji insertions and Cmd+A replacement trials.
Some foreground controls also lost events. This does not establish a
universal Chromium keyboard capability or isolate authentication as the
cause of the failures. No timing, Unicode, or menu-routing fix is included
in this slice.

NOTICE: Background command-target resolution and Unicode IME focus remain
open. A future owner-approved input contract must define those behaviors
before production delivery can claim semantic text-editing success.
