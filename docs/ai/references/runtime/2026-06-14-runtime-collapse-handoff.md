# C5 Runtime Collapse — In-Progress Handoff

Date: 2026-06-14
Status: **SUPERSEDED — see "Reviewer reality check + closure decision" below. The "blocked / main.rs broken" state was resolved after this note was written; recording extraction is now complete in the working tree and committable.**

## Reviewer reality check + closure decision (2026-06-14)

This in-progress note is stale. Verified working-tree state:

- `src/main.rs` syntax break is **repaired** (brace balance restored; `build_runtime_for_inspect` is well-formed).
- Recording callers **are migrated**: `src/osu.rs`, `src/app/mod.rs`, `src/app/infra.rs`, `src/scroll_scan/mod.rs` now call `recording.run_recorded_operation / start_run / finish_run`, not `runtime.*`.
- `Runtime`'s recording-facing methods (`inspect`, `read_run`, `run_recorded_operation`, `start_run`, `finish_run`, `stage_artifact_file*`, `run_dir`) are now **thin delegations** to `crate::inspect` / `crate::run_read` / `RecordingHandle` / `recorded_operation` (e.g. `runtime.rs:99` `inspect` → `crate::inspect::inspect_run(self.recording.store(), …)`). `Runtime` no longer **owns** recording semantics.

=> The recording-extraction substance is **DONE and committable**. It satisfies the C2 acceptance: "Runtime no longer owns core recording semantics; remaining methods are compatibility-only."

### Why the methods are NOT deleted (full Runtime/catalog delete is deferred)

`Runtime`'s recording methods are still called by **test consumers, including cross-crate**:

- `crates/auv-inference-ultralytics/tests/fixture_parity.rs` and `…/slay_the_spire_observe_only_boundary.rs` (`runtime.run_recorded_operation`, `runtime.read_run`, `runtime.inspect`)
- `src/app/tests.rs` (`runtime.read_run`)
- `src/runtime.rs` own unit tests

Deleting the methods requires migrating those test consumers to `RecordingHandle` first, then must be `cargo`-validated. That is a separate **runtime-delete endgame** slice (the design's `TODO(runtime-delete)`), gated on the test-consumer migration. It also still includes `src/catalog.rs` deletion + pulling command dispatch out of `Runtime`. The planner sandbox cannot build the macOS-targeted crates, so this slice must be done on the Mac, not blind.

### Closure decision

- **Commit the recording extraction now** as C5's committable substance.
- Re-scope the literal "delete `Runtime` recording methods + delete `src/catalog.rs` + extract command dispatch" as an explicit later **runtime-delete endgame** slice, gated on test-consumer migration.

### Pre-commit closeout (Mac-side)

1. Split the unrelated `crates/auv-driver/{capture,geometry,input}.rs` `#[derive(Default)]` changes into their own `chore(driver): derive Default` commit — unless `cargo` proves C5 needs them (it should not).
2. `cargo fmt --check && cargo check && cargo test && git diff --check`.
3. Commit the recording extraction (e.g. `refactor(runtime): C5 recording extraction — Runtime methods become thin delegations`) + this doc.
4. `git push`.

### Remaining after that commit (→ runtime-delete endgame slice)

Migrate the listed test consumers off `runtime.*` recording methods → delete those methods → delete `src/catalog.rs` → pull command dispatch out of `Runtime` → revalidate.

---

## Original in-progress notes (kept for history; now partly outdated)
Roadmap anchor: `docs/ai/references/runtime/2026-06-13-core-roadmap.md`
Design note anchor: `docs/ai/references/ops/2026-06-11-runtime-legacy-retirement-design.md`
Recording split anchor: `docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md`

## What this handoff covers

This handoff records the current **unfinished** state of the attempted C5 work so the next session can resume without re-auditing from scratch.

This is **not** a closure note.

Current intent of the attempted slice:
- move recording-only responsibilities off `Runtime`
- leave command dispatch behavior unchanged while shrinking the runtime surface
- set up the real deletion surface for later C5 completion

What is still **not** done:
- `Runtime` still owns command registry lookup and invoke dispatch
- recording-only callers are not migrated end-to-end
- standard validation block is not green
- no C5 closure evidence exists yet

## Current working-tree diff

Modified files:
- `src/recording/backend.rs`
- `src/recording/mod.rs`
- `src/main.rs`

### `src/recording/backend.rs`

Current in-progress change:
- introduced a new `RecordingHandle` wrapper around `RunRecordingBackend`
- copied recording-facing methods into that handle:
  - `read_run`
  - `inspect`
  - `list_candidate_action_execution_lineage`
  - `run_dir`
  - `start_run`
  - `finish_run`
  - `run_recorded_operation`
  - `record_candidate_action_decision`
  - `run_candidate_action_command`
  - `record_candidate_action_execution`
  - `stage_artifact_file`
  - `stage_artifact_file_with_ref`
- added `RunRecordingBackend::handle()` constructor

Important reality:
- this is a **mechanical extraction step only**
- nothing has been migrated to use `RecordingHandle` yet
- command dispatch still lives in `Runtime`

### `src/recording/mod.rs`

Current in-progress change:
- re-export now includes:
  - `RunRecordingBackend`
  - `RecordingHandle`

### `src/main.rs`

Current state:
- there is an accidental incomplete edit in `build_runtime_for_inspect(...)`
- diff currently shows the function closing `}` was removed after the `Ok(...)` block
- this leaves `src/main.rs` syntactically broken

Current broken diff fragment:
```rust
  Ok(
    build_runtime_with_store_root(project_root.to_path_buf(), store_root)?
      .with_recording(recording),
  )

fn should_write_local(...)
```

It should obviously still close the function before `fn should_write_local(...)`.

## Verified current blocker

The immediate blocker is **syntax breakage in `src/main.rs`**.

Observed command result:
- `cargo check` fails with:
  - `error: this file contains an unclosed delimiter`
  - points at `src/main.rs`
  - rooted at `build_runtime_for_inspect(...)`

This means the repo is currently not even back to a compilable intermediate state.

## Remaining direct `Runtime` couplings still blocking C5

These call sites still directly depend on recording-facing `Runtime` methods and were **not** migrated:

### `src/app/mod.rs`
Still uses:
- `runtime.start_run(...)`
- `runtime.finish_run(...)`

### `src/app/infra.rs`
Still uses:
- `runtime.stage_artifact_file(...)`
- `runtime.finish_run(...)`

### `src/scroll_scan/mod.rs`
Still uses:
- `runtime.start_run(...)`
- `runtime.finish_run(...)`
- `runtime.stage_artifact_file(...)`
- `runtime.read_run(...)`

### `src/osu.rs`
Still uses:
- `runtime.run_recorded_operation(...)`

### `src/runtime.rs`
Still still owns both sides at once:
- recording-facing APIs:
  - `inspect`
  - `read_run`
  - `run_recorded_operation`
  - `record_candidate_action_decision`
  - `list_candidate_action_execution_lineage`
  - `run_candidate_action_command`
  - `record_candidate_action_execution`
  - `run_dir`
  - `start_run`
  - `finish_run`
  - `stage_artifact_file`
  - `stage_artifact_file_with_ref`
- command-facing APIs:
  - `list_commands`
  - `invoke`
  - `invoke_in_span`
  - `invoke_direct_command_in_span`
  - `list_drivers`

That means C5 is still not actually at the deletion boundary.

## What the next session should do first

1. Repair `src/main.rs` so the tree compiles again.
2. Run `cargo check` immediately to confirm the tree is back to a valid intermediate state.
3. Migrate one recording-only surface at a time to `RecordingHandle`:
   - likely order:
     1. `src/osu.rs`
     2. `src/app/mod.rs` + `src/app/infra.rs`
     3. `src/scroll_scan/mod.rs`
4. Only after those callers stop depending on recording-facing `Runtime` methods, shrink `src/runtime.rs`.
5. Re-run the standard validation block.

## Recommended resume boundary

Resume as:
- `narrow refactor`
- goal: finish the recording-handle migration and restore a green intermediate build
- do **not** claim C5 closure until:
  - `Runtime` recording-facing methods are either removed or fully delegated through a reduced boundary
  - compile/test block passes
  - deletion surface is reported explicitly

## Current truth of repo state

The attempted C5 work is blocked in an incomplete middle state: `RecordingHandle` has been started in `src/recording/backend.rs`, but callers were not migrated, `Runtime` still owns both recording and command responsibilities, and `src/main.rs` is currently syntactically broken so `cargo check` fails before any C5 validation can proceed.
