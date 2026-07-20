# Core Contract Ownership Review

Date: 2026-07-19
Responsibility: runtime (core result contracts)
Type: review
Milestone: Workstream 3 / PR 7

## Purpose

Review the six core result contracts for ownership, stable-vs-platform fields,
duplicate schemas, unreachable states, and — the milestone's central concern —
whether a delivered action is ever mistaken for semantic success. Per the
milestone, this PR **fixes one clearest inconsistency**, not rewrites six types.

## Scope answered per contract

For each of the six the milestone named:
1. owner crate, 2. stable fields vs platform detail, 3. do CLI/MCP/library read
the same result, 4. is the same fact persisted in run/artifact, 5. duplicate
schema, 6. unreachable state, 7. is a successful action mistaken for semantic
success.

## Findings — 5 of 6 are cleanly owned

### 1. `InputActionResult` — owner `auv-driver-common` (`input.rs:325`)
- Single owner; re-exported by `auv-driver`. CLI/MCP/library all consume the
  same type via the driver session APIs.
- Persisted as an `input-action-result` artifact (read-side projection added in
  #115).
- **No duplicate**: grep for a parallel `struct InputActionResult` in donor
  crates returns nothing.
- **Action vs semantic**: correct by construction — the struct carries only
  delivery evidence (`selected_path`, `attempts`, `fallback_reason`,
  disturbance levels) and has **no** semantic-success field. Semantic success
  is never claimed here. ✅

### 2. `VerificationResult` — owner `src/contract.rs:446` (root `auv-runtime`)
- Single owner. Exemplary separation of axes: `executed` (did the check run),
  `state_changed` (did the world move), `semantic_matched: Option<bool>` (did it
  reach the expected state), `failure_layer` (where it failed). No duplicate in
  any donor crate.
- **Action vs semantic**: this is the type that *carries* the semantic claim,
  explicitly separate from execution. ✅

### 3. `OperationResult` / `OperationStatus` — owner `src/contract.rs:126` / `:100`
- Core struct is well-owned and documented; `OperationStatus` **was not** (see
  the fix below).
- **Duplicate-name collision** (follow-up, not fixed here): three crates define
  `pub type OperationResult<T> = Result<T, String>` — `auv-apple-notes`,
  `auv-apple-textedit`, `auv-qqmusic` — shadowing the core contract name (~75
  usages total). And `auv-game-balatro` has a placeholder
  `struct OperationResult { _private: () }` (intentional, `TODO(balatro-operations-v1)`).
  These are naming smells in feature-frozen reference/experimental crates;
  renaming ~75 sites across 3 crates is out of scope for PR 7's "one narrow fix"
  and touches crates whose feature work is paused. Recorded as a follow-up.
- **Unreachable state**: `OperationStatus { Completed, Failed }` — both
  reachable. ✅

### 4. `ArtifactRef` — owner `auv-tracing-driver` (`artifact.rs:6`)
- Single owner; `src/contract.rs:39` re-exports it (`pub use
  auv_tracing_driver::ArtifactRef`). Not redefined anywhere.
- `ArtifactRefView` (`auv-inspect-model:41`) is a **read-side enrichment
  projection** (adds `role`/`path`/`summary`/`resolved`), documented with an
  explicit anti-fork guard ("Owned here so game crates can parse artifacts
  without depending on `auv-cli`. Do not fork a same-shape copy."). This is a
  justified projection, **not** a duplicate. ✅

### 5. `CandidateRef` — owner `src/contract.rs:92`
- Cross-run pointer (`source_run_id`, `source_span_id`, `source_operation_id`,
  `source_artifact_id`, `candidate_local_id`), distinct from the inline
  `Candidate` struct (`:269`). Not a duplicate — different concept (reference vs
  inline value). ✅

### 6. Failure layer (`FailureLayer`) — owner `src/contract.rs:523`
- Single owner; consumed by inspect rendering (`src/inspect/mod.rs`). Not
  redefined in any donor crate. ✅

## The one fix landed here: document + lock the OperationStatus / semantic axis

**The clearest inconsistency**: `OperationStatus { Completed, Failed }` had
**zero documentation**. I verified all four producers derive status from the
**execution/dispatch** path and never from `semantic_matched`:

- `src/api/session_service/operation_result_store.rs:27` — from `RunStatus`.
- `crates/auv-cli/src/integrations/query_wired_live_action_status.rs:59`
  (`operation_status_and_message`) — `Completed` when a click summary exists,
  `Failed` when refused. Used by both minecraft and osu `query_live_action`.
- `crates/auv-cli/src/cli_frontend.rs:1107` (`build_minecraft_operation_result`)
  — `Completed` because it is only reached *after* the click was dispatched
  (refusal returns `Err` earlier); the semantic verdict is carried separately in
  `verifications[0]`.

So the codebase is **consistent** — `OperationStatus` = execution outcome,
semantic success = `verifications[].semantic_matched` + `failure_layer` — but
that load-bearing separation (the milestone's #1 principle, "action delivered ≠
semantic success") was protected **only by convention**. A future producer
could set `Failed` on a semantic mismatch, or a consumer could read `Completed`
as "succeeded", silently collapsing the two axes.

**Fix** (narrow, core-only, `src/contract.rs`):
1. Documented `OperationStatus`: `Completed`/`Failed` are the execution/dispatch
   outcome, **not** semantic success; semantic success is a separate axis in
   `VerificationResult::semantic_matched` + `failure_layer`. Cross-referenced
   from `OperationResult::status`.
2. Added a contract regression test
   `operation_status_completed_is_independent_of_semantic_outcome`: an
   `OperationResult` with `status == Completed` and
   `verifications[0].semantic_matched == Some(false)` (+ `failure_layer =
   StateChangedNoMatch`) is valid and round-trips — locking the two axes as
   independent so no future change can collapse them.

Chosen over the `OperationResult` alias rename because it is **core, narrow, and
directly answers the milestone review question** ("是否存在成功 action 被误当成
semantic success?"), whereas the rename is a large mechanical sweep across
feature-frozen crates.

## Stable vs provisional fields (summary)

- **Stable**: all six contracts' core identity fields (run/span/operation IDs,
  `status`, `executed`/`state_changed`/`semantic_matched`, artifact IDs).
- **Provisional / wire-versioned**: `api_version` on `OperationResult`,
  `VerificationResult`, `ObservationSnapshot` — stamped on write, but readers do
  **not** reject unknown values yet (`NOTICE(contract-api-version-reader-check)`
  in `contract.rs`). Graduation trigger for reader-side rejection: a real
  non-additive `v1alpha2` of the same record. Deferred, marked at the code site.

## Follow-ups (NOT done in this PR)

1. **`OperationResult` name collision**: rename the three
   `type OperationResult<T> = Result<T, String>` aliases
   (apple-notes/textedit/qqmusic) to a non-shadowing, non-`Result<_,String>`
   name. ~75 sites, 3 feature-frozen crates. Also ties into the error-chain
   inventory's goal of retiring `Result<T,String>` boundaries. Needs owner
   sign-off given the frozen-crate rule.
2. **`api_version` reader rejection** (`NOTICE(contract-api-version-reader-check)`):
   deferred until a non-additive record version lands.
3. From PR 6: remove the dead `auv-media-macos` dependency and decide the
   `auv-driver-macos` root target-gating — dependency-graph concerns, tracked in
   the crate-tier inventory.
