# PR #429 Linux review validation

Status: review fixes validated on Linux on 2026-10-27. The implementation accompanies this note.
This note replaces the pending Linux handoff. It records automated tests, native handler evidence, and measured limits.

## Revisions and scope

- PR: <https://github.com/moeru-ai/auv/pull/429>.
- Submitted head: `44c9ed4d4c0d5cbf73bfb9edb591d54406a6c64d`.
- PR merge base: `825a6330325848594796f2d6d599d3f751b4defb`.
- main at the start: `be0074beff9c4eabb0d0ab37272ee4a2b969a922`.
- Transferred patch SHA-256: `23a5573fe7e5aff37c31b63616250466bed32c96fb45cb9d7bd14f39fd7a866c`.

The patch applied without conflicts to the submitted head. The local branch first advanced from `f2357a89` to that head.
main advanced beyond the merge base. Its Runner and Linux Driver files did not differ from that base, so an extra base trial was unnecessary.
The measured patch contains the transferred production changes. Later edits only update comments and this evidence.

| Review | Result | Remaining boundary |
| --- | --- | --- |
| [4389303193: recapture error category](https://github.com/moeru-ai/auv/pull/429#discussion_r4389303193) | Original cause codes survive Runner, protocol, Rust/JS SDKs, CLI/MCP, and trace events. | Native permission revocation was not measured. |
| [4389756803: five enumerations](https://github.com/moeru-ai/auv/pull/429#discussion_r4389756803) | Successful window RPCs performed three enumerations after the patch, versus five in the submitted PR. | The two Driver identity/geometry checks remain. |
| [4389789663: unusable window baseline](https://github.com/moeru-ai/auv/pull/429#discussion_r4389789663) | Window captures store pixels and an unavailable cause, without Runner snapshots. Fetch and reference OCR worked. | Window placement and visibility remain unproven. |
| [4389877919: capture cost and blocking](https://github.com/moeru-ai/auv/pull/429#discussion_r4389877919) | Same-machine measurements appear below, including failed enumeration and health-request delay. | Display snapshots and synchronous handler blocking remain. |
| [4386560664: placement capability](https://github.com/moeru-ai/auv/pull/429#discussion_r4386560664) | The existing restriction and TODO remain. | A concrete reliable-placement contract needs a separately approved slice. |

The measurements preceded submission. The comparison uses the exact submitted head and transferred patch recorded above.

## Environment and method

| Item | Value |
| --- | --- |
| Host | Ubuntu 26.04.1, Linux 7.0.0-38-generic, x86_64; Intel Core i9-13900HX |
| Desktop | Existing GNOME/Mutter Wayland session; GNOME Shell 50.1 |
| Portal | xdg-desktop-portal 1.21.1; GNOME backend 50.0; ScreenCast v5 |
| Capture | PipeWire 1.6.2; WirePlumber 0.5.13; `xdg-desktop-portal.screencast.pipewire` |
| Accessibility | AT-SPI 2.60.4; eleven enumerated windows throughout measured requests |
| Output | eDP-1; 2048×1152 logical; 2560×1440 pixels; scale 1.25 |
| Test client | GTK3 3.24.52 under XWayland; fixed title; muted gray content; pointer-event logging |
| Window metadata | AT-SPI bounds (500, 250, 640, 494); returned crop 800×618 pixels |
| Build | Rust/Cargo 1.93.1; release profile, opt-level 2, debug 0; identical probe source and dependency versions |
| Renderer | Session logs report GBM renderers for card1/card2. Exact renderer and buffer allocation type were not measured. |
| Remote viewing | No remote viewer or compositor was created for these trials. |

The temporary harness served the real LocalCaptureService over loopback Tonic on a current-thread Tokio runtime.
A separate client thread measured each direct RPC with a monotonic clock. Window references resolved before each trial group.
The capture handlers, native Driver calls, capture store, previews, and response serialization all ran normally.
The harness omitted daemon routing and the production recording layer. These numbers describe direct handler RPCs, not complete daemon latency.
Generated CaptureService request/response schemas and the client code were identical across the three builds.

Each group used ten warm-up requests and 100 measured requests. Trials ran in order: main, submitted PR, local patch.
Each revision kept one Driver, Runner service, and PipeWire session. Every process stopped before the next trial started.
Builds completed before timing. Geometry, scale, resolution, and the eleven-window population remained fixed.
This was a live desktop: unrelated rendering and scheduling noise were not eliminated.

Instrumentation timed `auv-driver-linux/src/window.rs::list_windows`, including failed native calls.
Sequential RPC intervals assigned enumerations to reference resolution, Runner snapshots, and Driver checks by their known call order.
The probe logged intervals after each enumeration. Temporary instrumentation is absent from the working source changes.

For failure groups, a counter redirected only the selected AT-SPI connection to a nonexistent test socket.
The real zbus connection attempt failed. Portal connections and the desktop accessibility service continued normally.
This measures a controlled missing-socket failure, not a native service timeout, permission revocation, or typical failure latency.
The injection counter and file access sit outside the enumeration timer but inside total RPC time.

All percentiles use the nearest-rank method. Values below are p50 / p95 in milliseconds.

## Direct capture RPC latency

| Revision | RPC / condition | RPC result | Samples | Enumerations | Total p50 / p95 (ms) |
| --- | --- | --- | ---: | ---: | ---: |
| main | window / success | OK | 100 | 3 | 299.80 / 320.92 |
| main | window / lookup failure | UNAVAILABLE; early target lookup | 100 | 1 | 0.93 / 1.79 |
| main | display / success | OK | 100 | 0 | 100.38 / 106.93 |
| Submitted PR | window / success | OK | 100 | 5 | 400.89 / 433.05 |
| Submitted PR | window / lookup failure | UNAVAILABLE; early target lookup | 100 | 1 | 0.59 / 1.66 |
| Submitted PR | window / baseline failure | OK | 100 | 5 | 399.87 / 584.86 |
| Submitted PR | display / success | OK | 100 | 2 | 199.09 / 220.83 |
| Submitted PR | display / baseline failure | OK | 100 | 2 | 116.23 / 204.49 |
| Local patch | window / success | OK | 100 | 3 | 300.92 / 321.03 |
| Local patch | window / lookup failure | UNAVAILABLE; early target lookup | 100 | 1 | 0.42 / 1.71 |
| Local patch | display / success | OK | 100 | 2 | 201.26 / 225.64 |
| Local patch | display / baseline failure | OK | 100 | 2 | 116.07 / 204.84 |

A baseline failure still returned pixels. A target-lookup failure returned an error before capture.
main has no Runner baseline snapshots. Baseline-failure groups are therefore absent for main.
The patched Linux window path also has no Runner baseline, so that injection is not applicable.

Window capture returned to main-like timing after removal of the two unusable snapshots.
Display timing retained the added snapshots and their cost. The patch does not claim a display-capture speedup.

## Enumeration breakdown

Each cell is p50 / p95 in milliseconds. A dash means that the call is absent, not that a measured call took zero time.
Combined snapshot percentiles use each request's sum, rather than the sum of separate percentiles.

| Revision / RPC / condition | Reference lookup | Driver before | Driver after | Runner before | Runner after | Runner combined |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| main / window / success | 52.29 / 71.07 | 58.95 / 78.47 | 51.70 / 67.83 | — | — | — |
| main / window / lookup failure | 0.70 / 1.49 | — | — | — | — | — |
| main / display / success | — | — | — | — | — | — |
| Submitted PR / window / success | 59.55 / 75.92 | 57.06 / 73.32 | 52.98 / 70.62 | 50.96 / 71.16 | 60.53 / 72.56 | 113.41 / 135.94 |
| Submitted PR / window / lookup failure | 0.49 / 1.25 | — | — | — | — | — |
| Submitted PR / window / baseline failure | 65.75 / 119.40 | 58.34 / 127.70 | 59.39 / 115.22 | 2.24 / 2.92 | 65.17 / 121.60 | 67.84 / 124.77 |
| Submitted PR / display / success | — | — | — | 60.24 / 74.99 | 51.16 / 65.93 | 112.23 / 135.22 |
| Submitted PR / display / baseline failure | — | — | — | 2.43 / 3.16 | 52.28 / 68.84 | 54.41 / 72.27 |
| Local patch / window / success | 52.89 / 70.37 | 60.97 / 78.44 | 50.31 / 65.40 | — | — | — |
| Local patch / window / lookup failure | 0.34 / 1.39 | — | — | — | — | — |
| Local patch / display / success | — | — | — | 60.30 / 76.13 | 53.45 / 75.35 | 115.48 / 141.70 |
| Local patch / display / baseline failure | — | — | — | 2.39 / 3.03 | 51.77 / 70.97 | 54.51 / 72.79 |

## Independent RPC delay and scheduling decision

A separate group issued a lightweight Tonic health request ten milliseconds after starting each display capture.
The request shared the multiplexed channel but performed no capture or desktop input. Each revision used twenty samples.
This traffic was absent from the baseline timing groups.

| Revision | Health response p50 / p95 (ms) |
| --- | ---: |
| main | 89.56 / 98.77 |
| Submitted PR | 214.78 / 331.94 |
| Local patch | 214.66 / 282.57 |

The health delay confirms the review concern: synchronous capture and enumeration occupy the current-thread Runner.
main already blocks during capture. The PR adds snapshot work, and this patch only removes the unusable window snapshots.

A scheduling follow-up is warranted. It must preserve native thread ownership, tracing context, session serialization, cancellation, and capture/input ordering.
This change covers the baseline correction and measurements. Platform-wide scheduling remains a separate follow-up.
Keep display snapshots until a reviewed change preserves their admission guarantees.
`TODO(capture-baseline-latency)` now points to these measurements and the remaining scheduling decision.

## Native functional checks

The patched native fixture exercised real handlers, the Driver, stored captures, Tesseract, and the test receiver.

- Successful window capture performed exactly three native enumerations: one reference lookup and two Driver checks.
- The store retained an explicit placement-unavailable cause instead of a window baseline.
- `GetCaptureImage` returned a valid 800×618 PNG from that reference.
- `RecognizeText` accepted the same reference and returned nonempty text. Pixels were not uploaded by the client.
- A window baseline with a window recipient returned `CHECK_UNAVAILABLE`.
- A display baseline with a named window recipient returned `CHECK_UNAVAILABLE`.
- A window baseline with global screen coordinates returned `CHECK_UNAVAILABLE`.
- Display capture retained baseline snapshots. A failed first snapshot still allowed image fetch and caused guarded-click refusal.
- Native delivery counters stayed at zero. The receiver recorded no press or release.

The host lacked English Tesseract data. The test downloaded the distribution package and extracted it under the evidence directory.
`TESSDATA_PREFIX` selected that temporary model. No system package or global OCR setting changed.

The initial GTK4 Wayland receiver reported (0,0), which collided with desktop-window geometry. The Driver correctly refused that capture.
The final GTK3 XWayland receiver exposed nonzero coordinates, but visual inspection found other desktop content in its returned crop.
That result proves fetch/OCR availability, not correct window placement, visibility, or semantic target identity.
This limitation supports retaining the Linux placement guard. Correcting XWayland placement is a separate candidate slice.

The [earlier native freshness evidence](2026-10-27-linux-click-capture-freshness.md) remains applicable to the unchanged capture Driver.
This round reran static-cache, terminal-error, healthy-session, changed-pixel, and cancellation regressions.
It did not repeat the paused-source native trial or collect native permission-revocation evidence.

## Automated validation

All commands passed on Linux:

- `cargo test -p auv-cli -p auv-core -p auv-cli-invoke -p auv-api-proto`: 490 passed, one existing test ignored.
- `cargo test -p auv-driver-common -p auv-driver-linux --lib`: 232 passed, five existing Linux tests ignored.
- `cargo check`, `cargo fmt --check`, `git diff --check`, and `cargo run --quiet -- invoke --help`.
- `pnpm --filter @auv-js/sdk typecheck`.
- `pnpm --filter @auv-js/sdk test:run src/apis/auv/driver.test.ts`: 29 passed.
- Buf 1.72.0: lint, generation, changed-file format check, and breaking checks against local main and current remote main.

Cargo used `CARGO_HOME=/tmp/auv-cargo`. Socket tests ran outside the sandbox.
Generated TypeScript remains ignored. Existing platform warnings remain. No new macOS or Windows native result is claimed.

## Caller-visible changes

`ClickPreconditionFailure` adds optional `google.rpc.Code cause_code = 3`.
The outer status remains `FAILED_PRECONDITION`, with `CHECK_UNAVAILABLE` and zero input delivery when a native check fails.
Policy refusals and older Runners omit the field. Old wire readers ignore it.

Rust `CapabilityError::CapturePrecondition` adds `cause_code: Option<tonic::Code>` and `capture_cause_kind()`.
Explicit constructors need the new field. Exhaustive matches must bind it or use `..`.
JavaScript exposes `ClickPreconditionError.causeCode`; the existing three-argument constructor remains valid.
CLI/MCP retain `recapture_required` and expose the mapped category through `failure_details.cause_code`.
CLI syntax is unchanged. There is no automatic retry or unguarded fallback.

Permission and service failures require correction before a new capture. Recovery text alone does not select the recovery action.
The [contract](2026-10-26-click-capture-precondition-contract.md) describes the shared behavior.

## Evidence and review follow-up

Local raw logs, measured requests, probe sources, and binary hashes are preserved under `.tmp/pr429-review-evidence/`.
The original temporary directory is `/tmp/pr429-review/`. Neither directory is part of the proposed commit.
The sanitized [timing summary](2026-10-27-pr-429-linux-review-timing.json) contains aggregate results without images or window titles.

The evidence pack includes `timing-{main,submitted,patched}.log`, `timing-raw.json`, and `timing-summary.json`.
It also includes the native validation log, receiver log, build/test logs, and temporary probe sources.
The images and OCR text are local inspection evidence, not public API support claims.

The PR disclosure must distinguish these measurements from the earlier freshness and diagnostic fixes.
Keep the placement-capability deferral explicit. The timing request has measured evidence, but the blocking behavior remains unresolved in code.
