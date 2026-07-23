# AUV Core Lane Roadmap: Runtime Surfaces + osu Graduation

Date: 2026-06-13

Status: proposed forward plan for the AUV core lane. The C-series slices draw on
existing approved design notes and carry that approval. The G-series graduation
slices are scheduled and scoped here but each still requires its own named design
note and explicit owner approval before implementation, per the osu graduation
gate and `CLAUDE.md`. Starting one slice does not approve the next.

Predecessors:

- `docs/ai/references/apps/osu/2026-06-13-osu-benchmark-plus-roadmap.md` — osu lane; its
  Graduation Gate feeds the G-series here. P4–P7 fixture are merged and pushed
  (`origin/main` at `3de3f6c`); P8 bounded demo is local; P7 real-model smoke and
  the detector/YOLO side are handed to the detector owner (Neko).
- `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md` — invoke
  driver console (approved feature design).
- `docs/ai/references/inspect/2026-06-10-tracing-driver-runtime-recording-split.md` —
  recording split (approved feature slice).
- `docs/ai/references/ops/2026-06-11-frontend-convention-v0.md` — one execution model
  (M0 convention).

## What This Document Is

The osu benchmark lane is converging. The owner is returning to the AUV core lane
named in `CLAUDE.md`: invoke, run recording, artifacts, inspection, app-local Rust
commands, and distill/compile/run reuse across frontends. Several design notes
already specify pieces of this lane individually; nothing yet sequences them into
one ladder with explicit dependencies. This document is that ladder.

Rules for using it:

- Each C-series slice below is individually startable as written, because it
  inherits approval from a named predecessor design note. Starting one does not
  approve the next; finish, report, stop, and let the owner pick again.
- Each G-series slice is a graduation candidate. It needs a named design note and
  explicit owner approval before implementation. This document scopes it; it does
  not pre-authorize the code.
- Deviating from a slice's scope or acceptance section requires going back to the
  owner first.
- After each merged slice, update this doc's status lines and the codex handoff
  phase ladder.

Two tracks run here:

```text
C-series  Core Mainline    make "one execution model" a fact
G-series  osu Graduation   move proven reusable shapes into core, behind the gate
```

## Standing Boundaries (apply to every slice)

- one execution model: CLI, MCP, and library calls share runtime / run / store /
  inspect; frontends stay thin (parse args, map to typed request, format output)
  and never keep a parallel executor, store, planner, or retry loop
- strongest available signal wins, and every signal lands in the same
  runtime/run/store/inspect model: `API/file read -> AX -> OCR -> detector`
- dependency direction is `auv-cli / core -> product crate library`; core never
  copies product-crate logic and never consumes product-crate CLI text as a
  contract
- no new core-wide contract and no third action-result schema beside
  `ActionResolverDecision` (`src/action_resolver_decision.rs`) and
  `InputActionResult` (`crates/auv-driver/src/input.rs:322`); reuse
  `OperationResult` / `VerificationResult` (`src/contract.rs:125`, `:445`)
- `candidate-action` stays frozen; the `candidate_promotion` / `stability`
  promotion seam was retired on 2026-07-23 (zero production consumers after the
  #88 candidate-action removal) and must not be reintroduced without an
  owner-approved slice
- `auv-overlay-macos` is visual-only; `auv-driver` / `auv-driver-macos` own input
  delivery and disturbance reporting
- osu-specific logic stays in `crates/auv-game-osu`; it only reaches core through a
  G-series graduation slice with its own design note
- no JSON recipe, bundle, or case-matrix restoration as compatibility
- preserve the seam: `recognition / AX / candidates -> ActionResolver ->
  InputActionResult -> OperationResult / VerificationResult / trace artifacts`
- every slice ends with the standard validation block plus a real-app smoke where
  the slice changes runtime behavior

## The Code Facts Driving the Order

1. `src/runtime.rs` owns several unrelated responsibilities at once: run lifecycle,
   span/event recording, artifact staging, command-id catalog lookup, and legacy
   driver invocation. `src/recorded_operation.rs` already calls itself the bridge
   between typed Rust driver APIs and run recording, but its
   `RecordedOperationContext` (`src/recorded_operation.rs:20`) still depends on
   `Runtime`. So typed driver code cannot record inspectable evidence without
   dragging in the legacy command runtime. This is the recording-substrate gap.
2. `invoke` still resolves commands through `src/catalog.rs` and the `list-commands`
   surface, and the command ids are historical (`debug.*`, `verify.*`, `music.*`)
   rather than capability-oriented. The invoke-console design already specifies the
   replacement (`InvokeCommandSpec` metadata, `invoke --help`, capability
   namespaces, catalog removal) but it is unimplemented. This is the
   command-registry gap.
3. Product crates (`auv-steam`, `auv-apple-textedit`, `auv-apple-notes`,
   `auv-qqmusic`, ...) prove capability but do not automatically enter
   run/store/inspect, so the `AGENTS.md` "one execution model" claim is not yet
   true in product code. The frontend convention names `steam.library.list.v0` as
   the first API-grade proof. This is the convention-is-not-fact gap.
4. The osu lane proved four generically-shaped pieces that are currently
   osu-local: `LatencyReport` + `build_latency_report` + `percentile`
   (`crates/auv-game-osu/src/benchmark.rs:176`, `:957`, `:971`); `FrameKey`
   (`crates/auv-game-osu/src/visual_eval.rs:7`, fields `object_index` / `phase` /
   `capture_file_name`); the monotonic capture-timestamp semantics in
   `benchmark.rs` (`Instant` + `lead_in_ms` + `wait_until_due`, commit `7c63900`);
   and the `PlayfieldProjection` -> `projection.json` producer. These are the
   graduation candidates.
5. `auv-inference-common::DetectionCoordinateSpace`
   (`crates/auv-inference-common/src/types.rs:99`) is still v0 with only
   `SourceImagePixels`. A projected-space variant has exactly one consumer today
   (osu), so it stays parked until a second consumer appears.
6. P5 surfaced a real driver-side cost, not a benchmark-timing defect. After the
   warm-up fix the floor was a steady per-click `119–126 ms` on the successful
   `WindowTargetedMouse` path; switching the osu path from `ChromiumCompatible` to
   `WindowClickStrategy::PidTargeted` collapsed p95 from `125 ms` to `0 ms`
   (run `run_1781299108760_5250_0`). The cost lives in the native
   `ChromiumCompatible` strategy
   (`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift`),
   which performs fixed primer/move/sleep work even when the window-targeted path
   succeeds. That is a core driver finding, separate from osu.

These facts set the order: the recording substrate (C2) and the command registry
(C1) are the two pillars everything else rides on, so they come first; the
frontend-convention proof (C3) needs both; MCP reuse (C4) needs the proof; the
`Runtime` collapse (C5) needs both pillars. The G-series can be pulled in parallel
because each is a self-contained shape extraction, but G2/G4 are gated on a second
consumer existing.

## Phase Ladder

```text
C1  auv-cli-invoke metadata/registry boundary        (next, mainline)
C2  auv-tracing-driver recording extraction          (pillar; parallelizable with C1)
C3  steam.library.list.v0 through core invoke         needs C1 (+ C2 for clean recording)
C4  MCP frontend over the same core command           needs C3
C5  Runtime collapse to facade, then delete           needs C1 + C2

G1  latency/percentile reporting as a reusable shape  design-note-first; no second-consumer gate
G2  frame/action correlation keys as a reusable shape design-note-first; needs a 2nd capture consumer
G3  timestamped capture semantics hardening in core   design-note-first
G4  projected DetectionCoordinateSpace variant        parked until a 2nd consumer appears
GD  driver click-strategy cost reduction (from P5)    design-note-first; touches auv-driver-macos
```

Default order is C1 → C2 → C3 → C4 → C5 for the mainline. The G-series is opened on
owner demand and is not blocked by the C-series, except that G slices touching
`auv-driver*` or `auv-inference-common` contracts must land after their design note
is approved.

---

## C1: auv-cli-invoke Metadata/Registry Boundary

Classification: approved feature (inherits the invoke-console design note).

Status update (2026-06-14):
- C1a–d are now complete locally and validated.
- C1d finished the remaining production `debug.*` / `verify.*` canonical rename, expanded `invoke --help` / discovery to the canonical surface, preserved survivor commands as hidden-from-discovery resolvable entries, and updated the high-risk string-keyed runtime consumers.
- Validation passed for the required Rust block: `cargo fmt --check`, `cargo check`, `cargo test`, `git diff --check`.
- C1e (`src/catalog.rs` deletion / Runtime registry extraction`) remains explicitly deferred and still needs its own owner-approved slice.

Goal: stand up the `auv-cli-invoke` boundary with a metadata-first command model
and make `invoke --help` the discovery surface, so root runtime stops owning the
command registry.

Touches:

- new `auv-cli-invoke` boundary (workspace crate preferred; a root module with an
  explicit extraction marker is acceptable for the first PR)
- `src/cli.rs` (route discovery/help through the new boundary)
- `src/catalog.rs` (rebuilt as the capability registry, then deleted at exit)
- CLI/MCP/runtime tests using old ids

Design constraints:

- capability-oriented camelCase namespaces only: `display.*`, `screen.*`,
  `window.*`, `input.*`, `app.*`, `overlay.*`, `mediaControl.*`
- the macro/builder generates metadata only (`InvokeCommandSpec`, arg descriptors,
  help inputs, disturbance/artifact/verification notes); no execution logic moves
  in this slice
- this is a breaking rename: `debug.*`, `verify.*`, `music.*` ids disappear from
  the registry; an unknown-command error may hint the new family but must not
  execute through an alias table
- app-specific workflows stay in product crates (`auv-qqmusic` etc.), not in the
  invoke registry
- do not move run recording into `auv-cli-invoke`; that is C2

Deliverables:

- `InvokeCommandSpec` metadata model + `invoke --help` (grouped index) and
  `invoke <command> --help` (id, summary, backend driver/operation, arg schema,
  disturbance classes, artifacts/signals, verification semantics)
- `list-commands` removed as a first-class command (a failing parser tombstone
  pointing to `invoke --help` is the only allowed remnant)
- tests asserting old ids no longer resolve

Acceptance:

- `src/catalog.rs` is deleted and root runtime no longer owns `CommandCatalog` or
  command-id discovery
- `invoke --help` is the command index; `invoke <command> --help` renders
  command-specific metadata
- no `debug.*` / `verify.*` / `music.*` ids appear in the registry, help output, or
  positive tests; the negative checks fail without executing aliases
- standard validation block passes, plus:
  `cargo run -- invoke --help`, `invoke window.capture --help`,
  `invoke mediaControl.nowPlaying --help`, `invoke display.list`

Current progress note (2026-06-14):
- C1d satisfied the rename/discovery half of this acceptance except for the still-deferred `src/catalog.rs` deletion step.
- Do not treat this as C1 fully graduated yet; C1e remains a separate explicit follow-up slice.

Out of scope: typed dispatch migration, recording move, REPL design, command-family
rewrites.

Gate: report `invoke --help` output + the deleted-catalog diff, stop.

## C2: auv-tracing-driver Recording Extraction

Classification: approved feature (inherits the recording-split design note).

Goal: extract run/span/event/artifact recording out of `Runtime` into an
`auv-tracing-driver` boundary so typed driver code records inspectable evidence
without constructing `Runtime`.

Touches:

- new `auv-tracing-driver` recording boundary (run lifecycle, span lifecycle,
  event recording, artifact staging, artifact refs, local store persistence,
  recorder fan-out to inspect-server write mode)
- `src/recorded_operation.rs` (context depends on the recorder, not `Runtime`)
- `src/runtime.rs` (shrunk to a temporary facade with TODO markers)
- read-only inspect helpers moved toward store/read modules

Design constraints:

- prove the boundary with the smallest useful coverage: the existing
  `recorded_operation.rs` direct path; do not migrate command families here
- do not change persisted run wire shapes unless a compatibility boundary is
  documented and tested; the inspect server must not become an execution
  dependency
- `auv-tracing-interaction` (macro interactions like scroll scan) is explicitly
  deferred; this slice is driver-level recording only

Deliverables:

- typed Rust can `start_run` / `start_span` / `stage_artifact_file` / `finish_span`
  / `finish_run` without `Runtime`
- focused tests: successful recorded operation with artifacts; failed operation
  still persists a failed run; artifact refs include run/span/artifact/capture-event
  ids; inspect/read loads runs from the new recorder

Acceptance:

- `recorded_operation.rs` no longer imports `crate::runtime::Runtime`
- `Runtime` no longer owns core recording semantics; remaining methods are
  compatibility-only with clear TODOs
- existing inspect/read still loads persisted runs
- standard validation block passes

Out of scope: full `runtime.rs` deletion (C5), command migration, interaction-level
recording, viewer/inspect-protocol redesign.

Gate: report the recorded-operation test results + the `recorded_operation.rs`
import diff, stop.

## C3: steam.library.list.v0 Through Core Invoke

Classification: approved feature (inherits the frontend-convention note's named
first example).

Goal: make the frontend convention a fact for one real capability — an API/file
read signal that lands in the same runtime/run/store/inspect model.

Touches:

- core command implementation for `steam.library.list.v0` calling `auv-steam`
  library code (not reimplementing `steamlocate`)
- `auv-cli` invoke registration (rides on C1's boundary)
- `auv-steam` binary narrowed to a thin frontend over the same library call

Design constraints:

- `auv-cli` depends on `auv-steam` library APIs; the library function/result is the
  reusable unit, not a table; if the library shape is not reusable enough for core
  invoke, fix the library rather than duplicating logic in core
- the binary stays useful but only as a presentation shell over the same library
  function

Deliverables:

- `auv-cli invoke steam.library.list.v0` produces a standard run id, persists
  structured evidence through the existing store, and is inspectable via
  `auv-cli inspect <run-id>`
- a test proving the core command and the `auv-steam` binary call the same library
  function

Acceptance:

- the command is in the shared registry, invokable, recorded, and inspectable
- `auv-steam` binary no longer holds a parallel executor/store for this capability
- standard validation block passes, plus the invoke + inspect smoke on a real local
  Steam library (run id recorded)

Out of scope: MCP frontend (C4), additional product-crate commands, planner/NL
parsing in any frontend.

Gate: report the run id + inspect output for one real library list, stop.

## C4: MCP Frontend Over the Same Core Command

Classification: approved feature (inherits the frontend-convention + MCP-surface
notes). Resume only after C3 exists.

Goal: prove `steam.library.list.v0` is consumable by an external agent through MCP,
calling the same core command CLI calls — not a parallel path.

Touches: MCP frontend surface (`src/mcp.rs` and the MCP-frontend boundary), wiring
the existing core command through it.

Acceptance: an MCP client invokes `steam.library.list.v0`, gets the same structured
result and a run id that inspects identically to the CLI path; no capability logic
lives in the MCP layer. Standard validation block passes.

Out of scope: inventing capabilities in MCP, MCP-specific stores, mutation/replay
protocol redesign.

Gate: report the MCP-vs-CLI run-id parity evidence, stop.

## C5: Runtime Collapse to Facade, Then Delete

Classification: narrow refactor (inherits the `TODO(runtime-delete)` deferral).
Open only after C1 + C2 hold.

Goal: remove the legacy `runtime.rs` once command compatibility lives behind
`auv-cli-invoke` and recording lives behind `auv-tracing-driver`.

Acceptance: no caller constructs `Runtime` for recording or command lookup;
remaining references are deleted or moved behind the two boundaries; inspect/read
of historical runs still works; standard validation block passes.

Out of scope: behavior changes; this is a deletion/cleanup slice only.

Gate: report the deleted surface + green validation, stop.

---

## G-Series: osu → Core Graduation

Each G slice is design-note-first. Write the named design note, get owner approval,
then implement. The note must state the second consumer (where required) and the
exact reusable shape, and must not import osu-specific concepts (beatmap, playfield,
CS/AR/OD) into core.

## G1: Latency/Percentile Reporting as a Reusable Shape

Classification: approved feature only after its design note. Strongest first
graduation candidate — pure data shape, no behavior risk, and any timed driver
operation can reuse it.

Source: `LatencyReport` + `build_latency_report` + `percentile`
(`crates/auv-game-osu/src/benchmark.rs:176`, `:957`, `:971`), fields
`mean/p50/p95/p99/max_error_ms`, `jitter_ms`, `missed_schedule_count`.

Design note must answer: where the reusable shape lives (a small core crate or
`auv-tracing-driver` artifact helper), what the input sample type is so it is not
osu-specific, and that osu re-consumes the graduated shape instead of keeping a
private copy.

Acceptance (post-approval): osu's `LatencyReport` is produced by the core shape; at
least one non-osu timed operation (e.g. a driver input op) can emit the same report;
unit tests cover the percentile math at known distributions.

Out of scope: changing osu's dispatch semantics; new percentile algorithms.

## G2: Frame/Action Correlation Keys as a Reusable Shape

Classification: approved feature only after its design note. Gated on a second
capture-correlating consumer existing outside osu.

Source: `FrameKey` (`crates/auv-game-osu/src/visual_eval.rs:7`) and
`FrameDetections` / `FrameEvaluation`, keyed by `object_index` / `phase` /
`capture_file_name`.

Design note must name the second consumer (e.g. a non-osu capture-verify path) and
generalize `object_index` to an action/operation index without osu phase vocabulary
leaking into core.

Acceptance (post-approval): the core key correlates capture frames to actions for
two distinct consumers; osu maps onto it; round-trip tests pass.

Out of scope: opening this before a second consumer exists.

## G3: Timestamped Capture Semantics Hardening in Core

Classification: approved feature only after its design note.

Source: the monotonic capture-timestamp semantics in osu `benchmark.rs` (`Instant`
+ `lead_in_ms` + `wait_until_due` / `wait_until_instant`, commit `7c63900` "correct
capture timestamp semantics").

Design note must state which core capture path adopts the monotonic-clock rule and
how before/after capture timestamps relate to action delivery time, so the
correctness rule is shared rather than re-derived per lane.

Acceptance (post-approval): the core capture path carries the hardened timestamp
semantics with tests; osu reuses it.

Out of scope: capture-path latency optimization (separate question), capture backend
redesign.

## G4: Projected DetectionCoordinateSpace Variant (Parked)

Classification: parked. Do not open until a second consumer outside osu needs a
projected coordinate space.

`DetectionCoordinateSpace` (`crates/auv-inference-common/src/types.rs:99`) is v0
with only `SourceImagePixels`. osu's projection currently consumes capture-image
pixel space directly, so no enum change is justified yet. Recorded here so its
absence is not mistaken for an oversight.

## GD: Driver Click-Strategy Cost Reduction (from P5)

Classification: bug-fix/narrow-refactor candidate only after an owner-approved
design note; touches `auv-driver-macos`, so it goes through the graduation gate.

Finding: the native `ChromiumCompatible` strategy
(`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift`)
performs fixed primer/move/sleep work even when the window-targeted path succeeds,
adding a steady `~120 ms` per-click cost that `PidTargeted` does not
(run `run_1781299108760_5250_0`: p95 `125 ms` → `0 ms`).

Design note must decide whether to make click-strategy selection explicit per
operation, reduce the `ChromiumCompatible` fixed cost on the success path, or both —
and must preserve the `auv-driver` `InputActionResult` contract and disturbance
reporting unchanged.

Acceptance (post-approval): a measured before/after on a non-osu input op showing the
reduced cost or an evidence-backed explanation of why the strategy keeps the cost;
no new action-result schema.

Out of scope: input-device simulation fidelity; osu-specific tuning.

---

## Integration / Graduation Gate (osu lane -> core crates)

May graduate into core, each as its own owner-approved slice with a named design
note:

```text
timestamped capture semantics hardening            (G3)
latency histogram / percentile reporting shape      (G1)
frame/action correlation keys                        (G2)
driver click-strategy cost (P5 finding)             (GD)
a projected DetectionCoordinateSpace variant         (G4, only on a 2nd consumer)
```

Never graduates:

```text
beatmap parsing, playfield projection constants, CS/AR/OD rules
osu label map, dataset export format, play policy of any kind
```

## Per-Slice Process (same for every slice)

```bash
git status --short --branch   # verify clean start
cargo fmt --check
cargo check
cargo test
cargo build
git diff --check
```

Plus the slice's real-app smoke where runtime behavior changed, with run ids
recorded in the slice report. Docs-only follow-ups may skip Cargo validation and
must say so.

After each slice: state what changed and what was validated, list follow-up
observations without starting them, stop, and wait for the owner to choose the next
slice from this ladder.
