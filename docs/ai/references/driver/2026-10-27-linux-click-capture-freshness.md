# Linux click capture freshness

Classification: bug fix for [PR #429](https://github.com/moeru-ai/auv/pull/429).
Evidence: automated regressions and native fixtures on the environment below.
The original revision is `6f2740e5d0ae812c13ceca47e02c732a81e293a6`.
The corrected implementation is in the same commit as this note.

## Environment

| Component | Recorded value |
| --- | --- |
| OS | Ubuntu 26.04.1 LTS, Linux 7.0.0-38-generic, x86_64 |
| Desktop | Existing local GNOME Wayland session, GNOME Shell 50.1, Mutter 50.1 |
| Portal | xdg-desktop-portal 1.21.1, GNOME backend 50.0, ScreenCast interface v5 |
| PipeWire | 1.6.2, WirePlumber 0.5.13 |
| Display | `eDP-1`, 2048×1152 logical points, 2560×1440 pixels, scale 1.25 |
| Renderer evidence | GNOME logged GBM renderers for two DRM devices and selected the built-in panel GPU. Exact renderer identity was not measured. |
| Frame transport | AUV read mapped PipeWire buffers. The allocation type was not separately recorded. |
| Receiver | A test-owned GTK4/GJS window, 640×420, with fixed title and logged pointer events |
| Remote viewing | None |

The first product capture used Screenshot fallback because the AUV desktop entry was absent.
`auv doctor --portal-setup` installed the application identity without granting permissions.
After Portal consent, later product captures reported `xdg-desktop-portal.screencast.pipewire` with no fallback.
The receiver used muted gray and blue content after an initial fullscreen fixture was closed by the user.

## Reproduction and native results

The fixture kept one driver session, capture receiver, and capture store per trial.
It identified its own PipeWire consumer by process ID and followed that consumer's `node.target`.
`pw-cli send-command NODE Pause '{}'` paused only that capture source.
The desktop compositor and GTK receiver continued to run.
A cleanup guard sent `Start` even after an assertion failed.

After the source paused, the receiver changed from gray to blue.
Two ordinary captures still returned identical pixels and metadata.
This established the cached-frame condition independently of the click result.

| Case | Result | Input evidence |
| --- | --- | --- |
| Original revision, paused source, changed receiver | Admission accepted identical cached pixels. | One native click succeeded. The blue receiver logged press and release at local `(317.8, 280.7)`. |
| Corrected code, same paused-source scenario | `CHECK_UNAVAILABLE`, with the no-new-buffer cause. | Zero delivery calls before the later control click. |
| Corrected code, resumed source and changed pixels | `STATE_CHANGED`. | Zero delivery calls. |
| Corrected code, healthy source and desktop animation | `STATE_CHANGED` from exact full-frame comparison. | The guard refused input. The separate ordinary control click reached the receiver. |

The native fixture called the real admission function, driver capture, Portal input, and GTK receiver.
It supplied fixed window metadata to isolate freshness from the terminal's animated accessible title.
It did not replace pixels or mock native delivery.
This is native fixture evidence, not a claim of successful unchanged clicks through every RPC layer.

Complete CLI/RPC trials used Run `8dda00bafe4b8312c307430bf4e3b354` on a private local daemon.
They established capture persistence, reference reuse, real input delivery, and identical artifacts with the source paused.
Their guarded clicks refused unstable window metadata before freshness admission.
The native fixture isolated that earlier boundary instead of adding production exceptions for terminal windows.

A corrected CLI/RPC trial used Run `eb8e74e89f830cf38c68887ef874c623`.
Its capture used PipeWire without fallback.
The guarded refusal named `InputService/ClickPointWithCapture` and preserved the window-metadata cause in `failure_details.message`.

Local evidence remains under `/tmp/auv-click-native/` on the validation host:

- `native-before.log` and `native-after.log`: admission results and delivery counts.
- `receiver-small.log`: GTK redraw, press, and release events.
- `native_fixture_before.rs`, `native_fixture.rs`, and `receiver.js`: temporary native fixture sources.
- `paused-baseline.json` and `paused-changed.json`: product capture references and equal artifact hashes.
- `before-store/runner-traces/`: recorded RPC refusals and their original causes.

Full desktop images and temporary absolute-path test includes are not committed.
The fixture used logical screen point `(1055, 700)` after a screenshot located the receiver.
That point is specific to this host and window position.

## Correction and limits

`CaptureOptions.freshness` now accepts `LatestAvailable` or `RequireNewFrame`.
Guarded recapture selects `RequireNewFrame`.
Linux requires a buffer received while the request waits and refuses cached responses.
The existing refresh wait remains 150 ms when a cache exists.
No first frame still follows the existing five-second initialization limit.
Freshness failure does not open Screenshot fallback or disconnect a healthy static stream.

Ordinary captures retain the newest idle frame, including issue #244 behavior.
Terminal stream and format errors clear the cache and take precedence over pending responses.
Freshness stays in the request policy, so capture equality and blob deduplication do not compare changing timestamps.

A static healthy stream can return `CHECK_UNAVAILABLE` for a guarded click.
Receipt during a request does not establish the source's capture timestamp.
Exact pixels can still reject unrelated animation.
The final capture and native input remain separate OS operations.
Linux window-placement restrictions remain unchanged.

## Additional review fixes

- Cancellation during recapture stops admission before native input.
- Per-reference window snapshots and failure strings participate in the capture budget and eviction.
- Rust refusal errors retain the original status and diagnostic cause through CLI and MCP.
- Guarded invoke errors name `InputService/ClickPointWithCapture`.
- Frontend resolution refusals retain the original category in optional `cause_code`.
- The contract distinguishes stable display pixels from visibility of a named background recipient.

The cancellation, budget, and Rust diagnostic regressions failed before their fixes and passed afterward.
Budget tests also cover replacement and expiry without double-counting shared pixels.

## Automated validation

- `cargo test -p auv-driver-common -p auv-driver-linux --lib`: 105 common and 119 Linux tests passed. Five existing Linux tests remained ignored.
- `cargo test -p auv-cli -p auv-core -p auv-cli-invoke -p auv-api-proto`: unit, integration, and documentation suites passed.
- `cargo check`, `cargo fmt --check`, `git diff --check`, and `cargo run --quiet -- invoke --help`: passed.

The tests include queue ordering, cancellation, capture deduplication, budget accounting, receiver reuse, static streams, and newest idle frames.
Socket tests required execution outside the filesystem/network sandbox.
Existing platform-specific compiler warnings remain.
No Protobuf schema or JavaScript SDK code changed in this follow-up.

The [contract](2026-10-26-click-capture-precondition-contract.md) records the shared behavior and remaining limits.
