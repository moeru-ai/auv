# Click capture precondition

Classification: bug fix. Evidence: source inspection and automated fixtures.
No live desktop reproduction or platform support claim accompanies this change.

## Problem and contract

`ClickPointRequest` previously carried coordinates and delivery options only.
Refreshing a window reference corrected coordinates but could not reject stale pixels.
The [reported source](https://github.com/moeru-ai/auv/blob/739fe399311ae72b77ffeea530b231c3bb6c34ea/proto/auv/api/driver/v1/input.proto#L409)
documents that request shape.

The optional `capture_precondition` now names an AUV-owned `CaptureRef`.
Full window and display captures retain provenance beside their stored reference.
Provenance includes the resolved resource, resolution, and a hash of visible windows.
The baseline requires equal window lists before and after capture.
Identical pixel blobs can share storage without sharing provenance.
Store-specific UUIDs prevent references from aliasing across Runner restarts or hosts.

The Runner resolves coordinates, recaptures, checks, and calls delivery in one
blocking task. There is no asynchronous scheduling boundary between the check
and the input capability call. A refused click never calls that capability.
The input result remains the existing `InputActionResult`.

## Detection policy

The first policy requires exact equality. It compares the resolved resource,
visible window metadata and order, capture origin, bounds, scale, backend,
fallback metadata, dimensions, and all RGBA bytes at the original resolution.

Movement and resize invalidate window captures, including window-relative clicks.
Changed pixels invalidate scrolling, navigation, and dialogs rendered inside a window.
A changed visible window list invalidates separate dialogs even when window pixels match.
The click must also remain inside the stored capture extent and name the correct target.
Unrelated visible window changes and animation can cause conservative refusals.

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
Local execution cannot access a Runner capture store and refuses guarded clicks.

The Runner returns `FAILED_PRECONDITION` with method-specific Protobuf details.
Traced refusals emit `auv.input.click_refused`, and RPC recording retains the
request and failed status. The event includes the reference, reason, recovery,
and `input_delivered: false`. No pixels travel through the caller.

## Remaining race and evidence limits

The OS does not transact capture and input. The UI can change after the final
capture or during native activation, fallback, and event delivery.
Repeated clicks retain their inter-event race.
Pixel-identical semantic changes cannot be detected.
Window enumeration does not prove that every platform occluder is represented.
Separate application verification remains necessary for semantic success.

Callers must use a Runner that implements the new field. An older Protobuf
server can ignore unknown fields. Unguarded requests retain their existing behavior.

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

Validated on Windows with automated fixtures:

- `cargo test`
- `cargo test -p auv-cli -p auv-core -p auv-cli-invoke --lib`
- `cargo check`
- `cargo fmt --check`
- `cargo run --quiet -- invoke --help`
- `git diff --check`
- `pnpm --filter @auv-js/sdk typecheck`
- `pnpm --filter @auv-js/sdk test:run src/apis/auv/driver.test.ts` (21 tests)
- ESLint on the three changed SDK files
- Buf lint, generation, and breaking checks against `main`
- Buf formatting check on `input.proto`

The workspace still emits existing platform-specific warnings.
Full-repository Buf formatting reports checkout CRLF differences on Windows.
The changed schema passes its focused formatting check.
