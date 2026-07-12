# Core Lane Short-Term Plan: C1d → C2 → C3 (sub-sliced for step-by-step execution)

Date: 2026-06-14

Status: proposed execution plan. This is the tier-2 plan under
`2026-06-13-auv-core-lane-roadmap.md`: the roadmap is the strategic map (phases),
this decomposes the next three phases into **sub-slices**, which are the real
commit / validate / review / stop unit. Written for Codex to execute one sub-slice
at a time on the Mac (the planning sandbox is Linux and cannot build the
macOS-targeted crates, so it cannot run `cargo`).

Why sub-slices: C1 proved a phase is not a slice. Its single set of exit criteria
bundled five concerns with different blast radii (CLI help, discovery decoupling,
mass id rename, catalog deletion, Runtime registry extraction) and had to be split
into C1a–c at execution time. So the rule below.

## Slice-Sizing Rule (apply to every sub-slice)

A sub-slice is **one risk-coherent change that is independently validatable and
committable**. If a candidate needs more than one validation profile (e.g. a CLI
smoke *and* a full runtime-test pass *and* a real-app smoke), or it bleeds into the
next phase's surface, split it. Each sub-slice ends with: state what changed + what
was validated, then stop for owner selection.

## Standing boundaries (inherited)

One execution model (CLI/MCP/lib share runtime/run/store/inspect; thin frontends
only); strongest-signal-wins with every signal landing in the same model; no new
core-wide contract or third action-result schema beside `ActionResolverDecision` /
`InputActionResult`; `candidate-action` frozen; no JSON recipe/bundle revival;
real-app smoke wherever runtime behavior changes.

## Current State Snapshot (verified 2026-06-14)

```text
C1a–d   DONE and pushed — list-commands tombstone; invoke --help metadata-only;
        InvokeDiscoveryCatalog decouples discovery from runtime resolve; full
        canonical rename/discovery/help completion landed; app-probe regression
        was fixed and validated in the C1 lane.
C1e     CLOSED by C2d — the deferred catalog/runtime-registry ownership split has
        now landed inside the C2 lane.
C2a     REJECTED as a standalone slice — exposing a hollow recorder handle without
        a real consumer boundary was judged non-shippable.
C2b     DONE locally, not pushed — recorded_operation no longer depends on
        `Runtime`; local commits:
        `e99b032` detach recorded-operation staging from runtime internals
        `4fd30ba` complete gate for recorded-operation detach via
        `RecordedOperationServices` + `RunRecordingBackend`.
C2c     DONE locally and validated — read-only inspect/read helpers were moved off
        `Runtime` into explicit read-side entry points in `inspect` / `run_read`.
C2d     DONE locally and validated — `Runtime` no longer directly owns
        `CommandCatalog`; registry ownership now flows through `RuntimeCommandRegistry`
        while invoke behavior stays unchanged.
C2e     NEXT — shrink Runtime toward a thinner facade and delete remaining dead
        recording/registry paths without changing behavior.
C3      DONE locally and validated — `steam.library.list.v0` now runs through the
        honest `steam.local` backend, the `auv-steam` bin and core command share
        `query_local_library_apps(...)`, and MCP/inspect regression coverage now
        pins structured evidence plus inspect shape.
```

## Sub-Slice Ladder + Dependencies

```text
C1d   done and pushed — canonical rename + consumers + discovery + guard tests

C2a   rejected as standalone recorder-handle slice                      superseded by C2b
C2b   recorded_operation detached from Runtime via recorder services    done locally
C2c   inspect/read helpers moved off Runtime                            done locally
C2d   command registry ownership detached from Runtime                  done locally
C2e   shrink Runtime to facade; delete dead recording/registry paths    next

C3a   rehome steam.library.list.v0 off fixture.observe driver           done locally
C3b   enforce thin-frontend: auv-steam bin reuses the core library fn   done locally
C3c   confirm structured evidence + inspect shape for the command       done locally
```

Default order: C2e, then C3a → C3b → C3c. C3 can run
in parallel with C2 if desired, but must land after C1d (avoid registry churn). C2a remains recorded as a
rejected intermediate idea, not an execution prerequisite.

---

## C1d — Finish the invoke id rename

Classification: narrow refactor (id rename + consumer updates), inherits C1 design.

Goal: no `debug.*` / `verify.*` ids remain anywhere; discovery shows the full
canonical capability set.

Detail: the full rename map, consumer inventory, and locked decisions are in
`2026-06-14-invoke-completion-plan.md`. Do not re-derive them. Summary:

- promote remaining ~28 ids to `screen.*` / `window.*` / `input.*` / `app.*` /
  `overlay.*` (+ the 5 locked ambiguous ones, e.g. `display.captureRegion`,
  `input.overlayClickPoint`, `fixture.observe` excluded from discovery)
- update consumers in lockstep: `src/scroll_scan/mod.rs:650,668`,
  `src/app/mod.rs` (its remaining 4 legacy ids) + the `APP_PROBE_COMMAND_IDS` list
  in `src/app/tests.rs`, `src/cli.rs` help_text (incl. the stale L250 note)
- expand `invoke_discovery_catalog()` to the full canonical set
- keep `music.*` / `recognition.read.ratio` resolvable but out of discovery

Validation / gate:

```bash
cargo fmt --check && cargo check && cargo test && git diff --check
cargo run -- invoke --help                 # full canonical index, no debug.*/verify.*
cargo run -- invoke debug.typeText         # a renamed id must now FAIL
auv-cli app probe <bundle-id>              # the smoke that hid the C1c regression
```

Add guard tests: scroll_scan command ids resolve in `default_command_catalog()`;
a catalog test asserting no production id starts with `debug.`/`verify.`. Gate:
report `invoke --help` + the app-probe smoke run, stop.

---

## C2a — Recorder handle usable without Runtime

Classification: approved feature (inherits the tracing-driver design note).

Goal: expose run/span/event/artifact recording as a standalone handle that typed
code can use **without constructing `Runtime`**. The recorder primitive already
lives behind `Runtime` (`self.recording.recorder()`, `run_builder::RecordingRun`);
this slice surfaces it.

Touches: a new recording boundary (start as a module, e.g. `src/recording/` /
`run_builder` graduating to its own surface, or a new `auv-tracing-driver` crate —
the design prefers a crate; a marked root module is acceptable for the first PR),
exposing `start_run / start_span / stage_artifact_file / finish_span / finish_run`
and recorder fan-out to local snapshots + inspect-server write mode.

Constraint: do not change persisted run wire shapes; the inspect server must not
become an execution dependency. No consumer migration in this slice.

Validation / gate: a unit test creates a recorded run + stages an artifact through
the handle **without building `Runtime`**; existing runtime recording still works.
Gate: report the standalone-recorder test, stop.

## C2b — recorded_operation.rs depends on the Recorder, not Runtime

Classification: approved feature (the design's named proof slice). Needs C2a.

Goal: `RecordedOperationContext` takes the recorder handle instead of
`&Runtime`; drop `use crate::runtime::Runtime` from `recorded_operation.rs`.

Touches: `src/recorded_operation.rs` (context type + the `stage_artifact_*` /
`start_span` delegations now hit the recorder); the direct recorded-operation path;
update candidate-action / detector / AX recognition callers **only as needed to
compile and keep recording** — no command-family migration.

Validation / gate: `recorded_operation.rs` no longer imports
`crate::runtime::Runtime`; focused tests — successful recorded op with artifacts,
failed op still persists a failed run, artifact refs carry
run/span/artifact/capture-event ids. Gate: report the import diff + tests, stop.

## C2c — Move read-only inspect/read helpers off Runtime

Classification: narrow refactor. Needs C2a.

Goal: read/inspect helpers currently hanging on `Runtime` move toward `store` /
`run_read` modules so inspection does not require the command runtime.

Touches: `src/runtime.rs` (remove read-only helpers), `src/run_read.rs` /
`src/store.rs` / `src/inspect.rs` consumers.

Validation / gate: `auv-cli inspect <run-id>` still loads historical runs; inspect
tests pass without constructing a full command `Runtime`. Gate: report inspect
parity, stop.

## C2d — Extract the command registry from Runtime (folds old C1e)

Classification: approved feature (the deferred C1e). Needs C1d (stable canonical
registry) and benefits from C2a–c.

Goal: `Runtime` stops owning `CommandCatalog`; `invoke` receives an
already-resolved command descriptor (id → driver_id/operation/disturbance) from the
invoke boundary; `src/catalog.rs`'s role as Runtime's registry ends.

Touches: `src/runtime.rs` (`commands: CommandCatalog` field @ :32, `list_commands`
@ :61, resolve in `invoke_direct_command_in_span` @ :373, `new_with_catalogs`),
`src/lib.rs` (:47 builds `default_command_catalog()` for the default Runtime), the
invoke boundary (resolved-descriptor handoff), runtime tests building
`CommandCatalog`.

Constraint: behavior-preserving; the invoke execution result is unchanged. This is
where the design said execution-path change belongs — keep it isolated from C2b's
recording change.

Validation / gate: `invoke <canonical-id>` still dispatches identically; full
runtime test suite green; `Runtime` no longer references `CommandCatalog`. Gate:
report the Runtime diff + green suite, stop.

## C2e — Shrink Runtime to a facade; delete dead paths

Classification: narrow refactor. Needs C2b + C2c + C2d.

Goal: with recording, inspection, and registry all off `Runtime`, reduce
`runtime.rs` to whatever compatibility shim remains (or delete it if nothing real
is left), per `TODO(runtime-delete)`.

Validation / gate: no caller constructs `Runtime` for recording/registry; inspect
of historical runs still works; full suite green. Gate: report deleted surface,
stop.

---

## C3a — Rehome steam.library.list.v0 off the fixture.observe driver

Classification: narrow refactor (honest backend), inherits frontend-convention.
Needs C1d.

Problem: a real Steam library read is dispatched through `fixture.observe`
(`src/driver/fixture.rs:24-25`, `steam_library_list` @ :108). `fixture.observe` is
the deterministic no-real-UI test driver; a real API/file-read capability must not
live there.

Goal: give the command an honest backend. Register a first-class API/file-read
driver (e.g. `local.read` or `steam.local`) that owns `steam_library_list`, and
point the catalog entry (`catalog.rs:910`) at it. This realizes the convention's
"API/file read" rung as a first-class producer, not a fixture.

Touches: `src/driver/` (new driver registration), `src/catalog.rs` (driver_id of
the steam entry), `src/driver/fixture.rs` (remove the `steam_library_list`
special-case so `fixture.observe` is purely the test fixture again).

Constraint: still call `auv-steam` library code (`query_installed_apps` /
`SteamlocateSource`); do not reimplement `steamlocate`; keep the run id / artifact
shape so existing inspect/MCP behavior is preserved.

Validation / gate: `auv-cli invoke steam.library.list.v0` produces a run id +
inspectable evidence on a real local Steam library; the MCP test still passes;
`fixture.observe` no longer handles `steam_library_list`. Gate: report the run id +
inspect output, stop.

## C3b — Enforce the thin-frontend boundary on auv-steam

Classification: narrow refactor / test, inherits frontend-convention. Needs C3a.

Goal: the `auv-steam` binary is a presentation shell over the **same library
function** the core command calls; no parallel executor or store.

Touches: `crates/auv-steam/src/bin` + `cli.rs` (route through `library.rs`'s
`query_installed_apps` / `SteamLibraryStore`), a test asserting the core command
and the binary share that library entry point.

Validation / gate: a test proving both paths call the same `auv-steam` library fn;
binary output still works. Gate: report the shared-fn test, stop.

## C3c — Confirm structured evidence + inspect shape

Classification: test-only / docs-only (likely already satisfied). Needs C3a.

Goal: confirm `steam.library.list.v0` persists a structured `LibraryQueryResult`
(not just text) through the store, with grounding/diagnostics, and that
`auv-cli inspect <run-id>` surfaces it. If already true (the MCP test suggests so),
this collapses to a regression test + a one-line evidence note.

Validation / gate: inspect shows the structured library result + grounding; test
pins it. Gate: report, stop.

---

## Per-Sub-Slice Process (same for every one)

```bash
git status --short --branch
cargo fmt --check
cargo check
cargo test
cargo build
git diff --check
```

Plus the sub-slice's real-app smoke where runtime behavior changed (run ids
recorded). Do not `git add .` — the ~10 untracked `.tmp-*` run dirs are not
gitignored. After each sub-slice: state what changed + what was validated, list
follow-ups without starting them, stop for owner selection.

## Open Questions For The Owner (the "come back and ask" items)

1. C2 boundary form: new `auv-tracing-driver` crate (design's preference) vs a
   marked root module first? Crate is cleaner; module is less churn for the first PR.
2. C3a backend naming: `local.read` (generic API/file-read driver, reusable for
   future non-Steam reads) vs `steam.local` (Steam-specific)? The convention's
   signal ladder argues for a generic API/file-read driver.
3. Is rehoming the steam command (C3a) wanted now, or is the current fixture-driver
   wiring an acceptable v0 to leave until a second API-read consumer appears?
