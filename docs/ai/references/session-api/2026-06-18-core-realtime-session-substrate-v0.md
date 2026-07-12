# 2026-06-18 Core Realtime Session Substrate V0

Date: 2026-06-18

## Scope classification

`approved feature`

This slice implements the post-MC-4 runtime-lifecycle step recorded in the
Minecraft closure note: a core-resident stateful session substrate. It is not a
Minecraft slice, not MC-6/MC-7, and not a perception representation decision.

## Decision

Build the first in-process core substrate before transport:

- warm provider instances are held by a session across repeated observations
- observations persist as session resources and can answer lookup queries
- actions reuse existing `auv-driver::InputActionResult` and invalidate affected
  observations
- the session does not run a perceive-decide-act loop

This realizes the narrow part of
`2026-06-10-stateful-session-daemon-js-repl-v0.md` needed before any daemon,
JS REPL, hot/cold lane split, or spatial-memory representation choice.

## Exploration ledger

- Read `2026-06-16-minecraft-live-mc2-mc4-closure-plan.md`: MC-2/3/4 are closed
  and the next axis is runtime lifecycle, not 2.5D or 3DGS.
- Read `2026-06-10-stateful-session-daemon-js-repl-v0.md`: first slice should
  be in-process Rust API first, resource table, versions, invalidation events,
  one provider-cache proof, and no full transport.
- Read `docs/TERMS_AND_CONCEPTS.md`: `Device` and `Session` already exist as
  core vocabulary, but multi-session semantics were planned, not implemented.
- Read `src/model.rs`, `src/runtime.rs`, and `crates/auv-tracing-driver`: run
  recording already has `device_id` / `session_id`, so this slice should not
  change the run schema.
- Read `src/contract.rs`: `ObservationSnapshot` / `SurfaceNode` are the existing
  observation projection; the session should reuse them instead of inventing a
  second node/candidate contract.
- Read `crates/auv-driver/src/input.rs`: `InputActionResult` is explicitly one
  of the two action-result schemas; session action responses must carry that
  type rather than add a third result schema.
- Opened Collabi session `auv-core-realtime-session-substrate-v0` before edits.
- Second slice: kept Minecraft-specific projection outside `src/session.rs`.
  `auv-game-minecraft` now exposes an app-local spatial-frame observation, and
  root `minecraft_session` converts it into the shared `ObservationSnapshot`
  contract before registering it with `SessionRuntime`.

Rejected options:

- No daemon transport in this slice. HTTP/WebSocket/JSON-RPC shape is deferred
  until the in-process resource semantics are tested.
- No JS client or REPL in this slice. The API is shaped so transport can wrap it
  later, but no JS package lands here.
- No autonomous loop. The substrate exposes `observe`, `act`, `verify`-ready
  resources; it never chooses goals or actions.
- No Minecraft nouns in core. Minecraft remains a consumer, not the substrate.
- No 2.5D or 3DGS representation work. Those depend on measured needs above
  this lifecycle layer.

## V0 acceptance

- A provider is initialized once and reused across repeated `observe` calls.
- A session keeps observed nodes across calls and answers a label lookup.
- `act` returns `InputActionResult` and marks existing observations stale.
- A lightweight vertical consumer projects osu fixture detections into the same
  session observation substrate.
- A second vertical consumer projects Minecraft spatial telemetry frames into
  the same session observation substrate while preserving the app-local
  telemetry limits.
- Focused tests cover the above without requiring a live app.

## Current validation

The in-process Slice B gate is now covered by focused tests:

- warm provider proof:
  `session::tests::warm_provider_loads_once_across_repeated_observe_calls`
  records `load_count=1` and `observe_count=3`, with exactly one
  `ProviderInitialized` event.
- retained lookup proof:
  `session::tests::session_reuses_provider_and_answers_lookup` observes twice
  through one registered provider and resolves `hit_circle` from session state.
- action/verification seam proof:
  `session::tests::action_result_invalidates_observations_without_new_result_schema`
  returns the existing `InputActionResult` and marks retained observations
  stale; `session::tests::verify_records_existing_verification_result_contract`
  stores the existing `VerificationResult`.
- second-consumer proof:
  `osu::tests::osu_detection_provider_projects_into_session_observation` and
  `minecraft_session::tests::minecraft_spatial_frame_session_provider_feeds_session_runtime`
  both drive `SessionRuntime` through the same `ObservationSnapshot` /
  `SurfaceNode` substrate.

No transport endpoint landed in this slice. The inspect-server-hosted
HTTP/WebSocket surface remains deferred until the in-process resource semantics
need a remote caller.

## Deferrals

- TODO(session-daemon-transport): HTTP/WebSocket/stdio transport is deferred
  until this in-process resource table has a real second consumer and owner
  approval for a daemon slice.
- TODO(session-js-repl): JS handles, browser preview, and reactive REPL are
  deferred until transport exists. Rust only owns resource state and neutral IR.
- TODO(session-action-lock): device-level mutating action locks are deferred
  until multiple live sessions can contend for one device.
- TODO(session-recording-link): per-session operation recording is deferred
  until the first transport or vertical command needs session-originated runs.
- TODO(session-spatial-memory): keyframe/2.5D/3DGS memory is deferred until the
  MC-6 measurement slice produces numbers that justify a representation choice.
- TODO(session-minecraft-action-loop): Minecraft spatial observations are
  observe-only in this slice; action selection, world-diff verification loops,
  and live client scheduling require an owner-approved follow-up.
