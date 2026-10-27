# Click capture precondition

Classification: bug fix. Evidence: automated regressions, maintainer reports, and
[Linux native validation](2026-10-27-linux-click-capture-freshness.md).
The native results apply to the named environment and fixture boundaries.
No general platform support claim is made.

## Problem and contract

`ClickPointRequest` previously carried coordinates and delivery options only.
Refreshing a window reference corrected coordinates but could not reject stale pixels.
The [reported source](https://github.com/moeru-ai/auv/blob/739fe399311ae72b77ffeea530b231c3bb6c34ea/proto/auv/api/driver/v1/input.proto#L409)
documents that request shape.

`ClickPointWithCapture` requires a `click` and an AUV-owned `capture` reference.
Its response wraps the ordinary click result in `result`.
SDK capture options and CLI `--capture` select this dedicated method.
Older Runners return `UNIMPLEMENTED`; callers never downgrade to ordinary `ClickPoint`.
The unreleased `ClickPointRequest.capture_precondition` field 4 is reserved.
Full window and display captures retain provenance beside their stored reference.
Provenance includes the resolved resource, resolution, and visible window snapshots.
The baseline retains snapshots from before and after capture, including failure causes.
Identical pixel blobs can share storage without sharing provenance.
Store-specific UUIDs prevent references from aliasing across Runner restarts or hosts.

The Runner resolves coordinates, recaptures, checks, and calls delivery in one
blocking task. There is no asynchronous scheduling boundary between the check
and the input capability call. A refused click never calls that capability.
The input result remains the existing `InputActionResult`.

Guarded admission acquires the shared desktop input reservation before checking
the baseline or recapturing. It retains the reservation through delivery.
Nested platform calls reuse this reservation instead of waiting after the check.
If the recipient moved while admission waited, recapture rejects its old metadata.
Cancellation while waiting stops admission without input.
Admission also checks cancellation immediately before delivery, after recapture and comparison.

Per-reference provenance shares the capture store budget with pixels and derived caches.
The budget includes both window snapshots, target strings, and retained failure causes.
References with identical pixels still own separate provenance and can be evicted independently.
Replacing or expiring provenance releases its accounted bytes.
The existing exception for one oversized current reference remains.

On Windows, foreground window clicks now return activation errors before global
input. This applies to guarded and ordinary clicks. Previously, a failed activation
was recorded as an attempt, then global input continued at the target coordinates.
Call syntax, wire fields, and successful results stay unchanged.

## Detection policy

The first policy requires exact equality. It compares the resolved resource,
relevant window metadata and order, capture origin, bounds, scale, backend,
fallback metadata, dimensions, and all RGBA bytes at the original resolution.

Movement and resize invalidate window captures, including window-relative clicks.
Changed pixels invalidate scrolling, navigation, and dialogs rendered inside a window.
A window capture compares windows that intersect the captured window.
A display capture compares windows that contain the click point.
Changes within these groups invalidate separate dialogs even when pixels match.
Windows outside these groups do not affect the metadata comparison.
The click must also remain inside the stored capture extent and name the correct target.
Full-frame pixel comparison remains exact, so unrelated animation can still cause refusal.
Tolerance and region-only pixel comparison need a separate accepted detection policy.

Failure reasons are `CAPTURE_UNAVAILABLE`, `TARGET_MISMATCH`, `STATE_CHANGED`,
and `CHECK_UNAVAILABLE`. Every reason requests a new capture and caller decision.
There is no automatic retry or replanning.

Region captures, OCR-produced captures, and streamed frames lack the required
baseline in this slice. Their references refuse guarded clicks.
The code-site TODO records this producer boundary.

## Frontends and recording

The [method documentation](../../../../proto/auv/api/driver/v1/docs/input.click.md)
contains request, SDK, CLI, failure, and race details.
Rust exposes `click_with_capture` and `CapabilityError::CapturePrecondition`.
JavaScript exposes `capturePrecondition` and `ClickPreconditionError`.
Invoke exposes `--capture`, which also supplies MCP metadata and replay inputs.
Runner capture results expose `capture.capture_id` for this next operation.
CLI and MCP preserve the `recapture_required` failure code.
Local execution returns `invalid_target` with instructions to select a Run through `--run`.
Recapture alone cannot fix an absent Runner route.

The Runner returns `FAILED_PRECONDITION` with a standard `google.rpc.Status` envelope.
Its `details` packs `ClickPreconditionFailure` in `Any`.
Rust and JavaScript preserve the original RPC error for unknown or malformed details.
Traced refusals emit `auv.input.click_refused`, and RPC recording retains the
request and failed status. The event includes the reference, reason, cause, recovery,
and `input_delivered: false`. The status message also carries the cause, bounded to 512 characters.
Rust `CapabilityError::CapturePrecondition` retains the original `tonic::Status` as `source`.
Its displayed error preserves the diagnostic cause through CLI and MCP.
Guarded invoke errors identify `InputService/ClickPointWithCapture`.
No pixels travel through the caller.

Window-list comparison failures report counts for added, removed, and modified windows.
They also report changes to the relative order of shared window identities.
The first difference includes a window ID and app name, with application ID as fallback.
Each escaped label has an 80-character limit, plus an ellipsis when truncated.
Window titles are omitted. The complete cause retains its 512-character limit.

These diagnostics cover differences within a capture and between captures.
They do not change the comparison policy or refusal categories.
Automated admission and trace regressions cover the
[review request](https://github.com/moeru-ai/auv/pull/429#discussion_r4386560659).
This diagnostic follow-up has no new native macOS measurements.

Frontend target-resolution refusals remain in the command failure record.
They do not reach the Runner and currently emit no `auv.input.click_refused` event.
The inline TODO in `selected_click_point` records this admission-recording gap.
These frontend failures keep `recapture_required` and add `cause_code` for the original resolution category.
For example, permission failures retain `permission_denied`, while connection failures retain `backend`.

## Review evidence and limits

Guarded recapture requests `CaptureFreshness::RequireNewFrame` through the shared driver options.
Linux accepts only a buffer received while that request waits.
With an existing cached frame, the refresh wait remains 150 ms.
Without a new buffer, the guard returns `CHECK_UNAVAILABLE` before input.
It does not enter the interactive Screenshot fallback.

This policy deliberately refuses healthy static streams that send no new damage.
Ordinary captures retain `LatestAvailable`, including the newest idle frame from issue #244.
A freshness miss retains the connected receiver and its ordinary capture cache.
Terminal stream and format errors take precedence over cached pixels.
No timestamp enters `Capture` equality or pixel deduplication.

The guarantee concerns receipt during the request, not a compositor timestamp.
Source clocks and capture/input transactions require a separate accepted contract.
The [Linux evidence note](2026-10-27-linux-click-capture-freshness.md) separates the native fixture from complete RPC behavior.

The queue-order and Windows activation fixes have regression tests that send no
native input. Both tests failed before their respective fixes. Cancellation and
nested desktop reservations also have admission coverage.

The following measurements came from the maintainer's review of the original PR revision.
They are small samples, not general failure rates or new measurements of the fixes.

| Platform | Reported result | Review response |
| --- | --- | --- |
| macOS | 0/16 unchanged clicks delivered. Ignoring Window Server improved this to 7/10. | Scope window metadata to the target or click point. No process-name exceptions. |
| Windows display | 5/12 unchanged clicks refused, with 171–252 differing bytes and equal metadata. | Retain exact pixels and document the observed false refusals. |
| GNOME display | 1/10 unchanged clicks refused, possibly from caret blinking. | Retain exact pixels and document the observed false refusal. |
| GNOME Wayland window | GTK4 AT-SPI extents stayed at `(0,0)` after movement, so a moved window passed. | Refuse AT-SPI window crops and Linux window-targeted guards with `CHECK_UNAVAILABLE`. |

Sources: [macOS window churn](https://github.com/moeru-ai/auv/pull/429#discussion_r4382693014),
[pixel false refusals](https://github.com/moeru-ai/auv/pull/429#discussion_r4382693024),
[Wayland placement](https://github.com/moeru-ai/auv/pull/429#discussion_r4382693034).

The macOS recording indicator previously invalidated the full desktop window list.
The metadata fix ignores indicators outside the relevant area.
An indicator that intersects that area or changes captured pixels can still cause refusal.

The Linux placement restriction is conservative: the current driver exposes no reliable placement capability.
Display captures with screen or display coordinates remain available.
Window-targeted guards require a driver contract that establishes reliable placement before they can be enabled.

Recapture runs inside the input task, so other input RPCs can wait behind it.
Reported latency was 0.1–3.3 seconds on macOS and 1.6–2.7 seconds for a Windows 4K display.
GNOME guarded clicks took 4–5 times as long as unguarded clicks.
See the [latency report](https://github.com/moeru-ai/auv/pull/429#discussion_r4382693086).

## Remaining race and evidence limits

The OS does not transact capture and input. The UI can change after the final
capture or during native activation, fallback, and event delivery.
Repeated clicks retain their inter-event race.
Pixel-identical semantic changes cannot be detected.
Window enumeration does not prove that every platform occluder is represented.
An unchanged display capture does not prove that a named background recipient is visible.
For example, a window can remain covered while its display evidence stays unchanged.
The guard checks scene stability, not pixel ownership or recipient visibility.
Separate application verification remains necessary for semantic success.

Guarded callers use the dedicated method, so older Runners refuse before input.
Unguarded requests retain their existing behavior.

Automated evidence includes:

- [Admission and zero-delivery assertions](../../../../crates/auv-cli/src/runner/click_precondition_test.rs)
- [RPC refusal recording](../../../../crates/auv-cli/src/runner/local_driver_test.rs)
- [Rust SDK transport assertions](../../../../crates/auv/src/client/runner_test.rs)
- [CLI capture and replay assertions](../../../../crates/auv-cli-invoke/src/commands/input_test.rs)
- [MCP refusal assertions](../../../../crates/auv-cli/src/commands/mcp_test.rs)
- [JavaScript SDK assertions](../../../../js/packages/sdk/src/apis/auv/driver.test.ts)

The admission tests assert zero delivery calls on refusal and one call on valid evidence.
Other tests cover typed transport failures, reference isolation, and refusal recording.

## Validation

Original validation commands appear below. Current review-fix results are recorded
in the PR description and thread replies at the tested revision.

- The original unfiltered platform suite failed the live WGC/GDI pixel comparison.
  The remaining suites passed with that test explicitly excluded. This was not
  a fully green unfiltered run; see PR #429 validation history.
- `cargo test -p auv-cli -p auv-core -p auv-cli-invoke --lib`
- `cargo check`
- `cargo fmt --check`
- `cargo run --quiet -- invoke --help`
- `git diff --check`
- `pnpm --filter @auv-js/sdk typecheck`
- `pnpm --filter @auv-js/sdk test:run src/apis/auv/driver.test.ts`
- ESLint on the three changed SDK files
- Buf lint, generation, and breaking checks against `main`
- Buf formatting check on `input.proto`

The workspace still emits existing platform-specific warnings.
Full-repository Buf formatting reports checkout CRLF differences on Windows.
The changed schema passes its focused formatting check.
