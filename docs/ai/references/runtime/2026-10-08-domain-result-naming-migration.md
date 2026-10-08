# Domain result naming migration

Status: owner-approved naming refactor, 2026-10-08. API compatibility aliases
are intentionally omitted, including proto/SDK names. This changes naming and
serialized field/variant names, not capture, scroll, input delivery, or parsing
behavior. Pointer subscription remains a separate, unimplemented proposal.

## Naming decision

Prefer the concrete domain data: a captured frame, parsed viewport, frame item,
device status, read result, or stream update. Reserve **observation** for information
collected to inform the next action in a perception/decision/action loop. Merely
reading data, returning a response, recording an event or yielding a stream item
is not enough reason to call it an observation. Even in a decision loop, a more
specific domain name is preferable when it explains the payload better.

`Sample` means a timestamped measurement; `State` means maintained state;
`Snapshot` means a point-in-time collection; `Event` names something that happened;
`Update` carries progress/current data. These terms are not interchangeable.

Apple's `VNRecognizedTextObservation` and its native Vision result terminology
retain the platform's names. Historical evidence files retain emitted names.
Existing captures use `CaptureRef`/`CapturedFrame`; they were not renamed.
The glossary calls their coordinate/capture surface **Coordinate Scope**, replacing
the overly broad **Observation Scope**.

## Source and API mapping

| Previous | Current |
| --- | --- |
| `ScrollUntilObservation` | `ScrollUntilUpdate` |
| `ScrollObservation` | `ScrollUntilUpdate` |
| `ScrollUntilObserve` | `ScrollUntilOutputOptions` |
| `onObservation` | `onUpdate` |
| `FrameObservation` | `FrameItem` |
| `ObservationRequest` | `SceneRecommendation` |
| `ViewObservation` | `ParsedViewport` |
| `ViewObserver` | `ViewportReader` |
| `SidebarViewportObservation` | `SidebarViewport` |
| `SongListObservation` | `SongListPage` |
| `ReacquireObservation` | `ReacquireSnapshot` |
| `ConfiguredDeviceObservation` | `ConfiguredDeviceStatus` |
| `GodotDevObservationError` | `RenderInspectionError` |
| `RenderObservationArtifact` | `RenderInspectionArtifact` |
| `RenderObservationRequest` | `RenderInspectionRequest` |
| `RenderObservationContextFiles` | `RenderContextFiles` |
| `HoverReadObservation` | `HoverReadResult` |
| `ObservationError` | `ReadError` |
| `RestartObservation` | `RestartState` |
| `StaleObservation` | `StaleUiReference` |
| `ObservationFailedAtReacquisition` | `ReadFailedAtReacquisition` |
| `ObservationFailed` | `ReadFailed` |
| `InvalidObservationShape` | `InvalidFrameItems` |
| `observe_configured` | `probe_configured` |
| `export_current_render_observation` | `export_current_render_inspection` |

The playground's internal `NativeObservation` alias is `NativeScrollUpdate`.
The view parser associated type `Observation` is `Viewport`; scan results contain
`viewports` and use `viewport_index`. Reacquisition counters use
`read_count`; the existing `max_scroll_attempts` budget is unchanged. Device discovery uses `probe_configured`.
Balatro's `observation` module is `read`, with `read_image` / `read_image_via_api`.

## Protocol and persistence changes

- Scroll-until: `ScrollUntilResponse.event.update` replaces `observation`;
  `ScrollUntilBegin.output` replaces `observe`; SDK callbacks use `onUpdate`.
  Existing field numbers remain in place. Initial and terminal update ordering,
  decision waits, capture references, OCR opt-outs and cancellation are unchanged.
- Scan association: `items_by_frame`, `item_id`, `previous_item_id`,
  `current_item_id`, `candidate_item_ids`; diagnostic names follow `items`.
  Track coverage uses `sighting_count`; recommendations use `recommendations`.
- View/app records: `viewports`, `viewport_index` and associated telemetry keys
  replace generic observation fields. NetEase playlist proto count is `viewports`.
- Driver/invoke errors use `StaleUiReference` / `stale_ui_reference`; lifecycle
  failures use `ReadFailed` / `read_failed`, preserving their distinct meanings.

NOTICE: This is the owner's coordinated breaking rename. There are no legacy
Serde aliases or dual proto/SDK fields. Existing experimental schema labels are
retained; regenerate old test fixtures when using the new readers. Historical
run artifacts are not rewritten and are not promised read compatibility by this
migration. Versioned read compatibility is deferred unless an owner-approved
slice names a concrete historical artifact consumer.

## Guidance for older documents

Dated design notes may describe the previous spelling. Their migration notice
points here so historical reasoning is preserved without recommending old API
names. For new code, use the current domain name and update its consumers,
protocol fields, generated SDKs, current reference docs and tests together.
Do not expand archived app proofs or add new behavior as part of a naming change.

## Verification

Evidence level: source migration, generated bindings, compilation and automated
contract tests. This change does not claim new live platform capabilities.

Initial validation before rebasing (local base `a9edcfa6`):

- `cargo check --workspace --offline`, `cargo fmt --check`, `git diff --check`.
- `cargo run --quiet -- invoke --help`.
- `buf lint` and targeted formatting for `auv/api/driver/v1/input.proto`.
- Rust bindings through the normal Cargo build; Protobuf-ES bindings through
  the configured local plugin. The remote OpenAPI plugin was unavailable; its
  daemon-only input services are unchanged by this migration.
- 571 library tests across `auv-cli-invoke`, `auv-core`, `auv-driver-common`,
  `auv-driver-macos`, `auv-game-balatro`, `auv-godot`, `auv-netease-music`,
  `auv-scan` and `auv-view`. Six tests retain their existing ignored status.
  Two capture transport tests initially failed to bind sockets in the sandbox;
  both passed after the environment allowed local listeners. The focused
  capture test rerun passed all three selected tests.
- CLI Runner scroll-until Proto conversion tests: 2 passed via
  `cargo test --offline -p auv-cli --lib scroll_until`.
- SDK Driver contract tests: 14 passed, using the regenerated schemas and a
  temporary Vitest configuration without daemon/browser setup.
- Playground typecheck and tests: 38 tests passed.
- SDK typecheck with the workspace TypeScript 6.0.3 compiler:
  `node node_modules/typescript/bin/tsc -p js/packages/sdk/tsconfig.json --noEmit`.

Initial validation limits and follow-up:

- `buf breaking` against `main` reports the eight expected driver message/field
  changes. These are intentional; compatibility aliases are not added.
- Full `cargo test --workspace --offline` is blocked by the untouched Windows
  WGC test's platform imports on macOS. The first focused all-target run also
  found the untouched `auv-core/tests/backend_selection.rs` still importing
  `auv` instead of `auv_core`; library tests bypass that unrelated failure.
  That checkout predates the upstream fix, already merged in
  [PR #267](https://github.com/moeru-ai/auv/pull/267).
- The full SDK suite initially could not start its local listener (`EPERM`).
  After the sandbox restriction was lifted, `pnpm --filter @auv-js/sdk test:run`
  passed: 18 test files, 77 tests passed and one skipped, including real daemon
  and browser transport tests. Test daemon shutdown completed successfully.
- The SDK-local `node_modules/.bin/tsc` shim points to stale TypeScript 5.4.5
  and reports three `AbortSignal.any` typing errors. The installed workspace
  compiler is 6.0.3; invoking it directly passes. This is a local dependency
  installation issue, not a missing runtime API or a source change required
  by this migration.

Rust validation sets `CLANG_MODULE_CACHE_PATH` and `SWIFT_MODULECACHE_PATH` to
writable temporary directories because the sandbox cannot write the default
Swift module cache.

## PR validation after rebasing

The PR is based on `0dd585354fea67856a4c441ebf54fee6ad585758`. It retains
upstream capture resolution, screen-coordinate handling and daily-page
navigation fixes, and updates their new call sites to the renamed contracts.
The backend-selection test repair from PR #267 is already in this base.

- `cargo check --workspace --offline`: passed.
- `cargo test --offline --lib --no-fail-fast` for `auv-scan`, `auv-view`,
  `auv-core`, `auv-cli`, `auv-cli-invoke`, `auv-driver-common`,
  `auv-driver-macos`, `auv-netease-music`, `auv-godot` and `auv-game-balatro`:
  666 passed, seven ignored, no failures. Capture transport tests are included.
- `pnpm --filter @auv-js/sdk --filter @auv-js/repl-playground typecheck`: passed
  with a fresh lockfile installation, without the old TypeScript shim.
- Playground: 38 tests passed.
- Full `buf generate`, including OpenAPI, and `buf lint`: passed.
- `buf breaking` against the base Proto tree: eight expected driver naming
  incompatibilities, matching the approved migration.
- Full SDK suite: 18 files passed, 77 tests passed, one skipped; real daemon
  and browser tests completed, and the test daemon shut down.
- `cargo fmt --check`, `git diff --check` and
  `cargo run --quiet -- invoke --help`: passed.

The full workspace test suite is not claimed to pass on macOS: the untouched
Windows WGC unit-test platform imports remain outside this naming refactor.
These checks establish compilation and automated contract behavior, not new
live OS input capabilities.
