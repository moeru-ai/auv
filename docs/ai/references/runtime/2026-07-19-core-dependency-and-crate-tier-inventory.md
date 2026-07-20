# Core Dependency Map & Crate-Tier Inventory

Date: 2026-07-19
Responsibility: runtime (workspace architecture / maintenance tiers)
Type: inventory / matrix
Milestone: Workstream 1 / PR 6

## Purpose

Produce an accurate core dependency map and a maintenance-tier for every
workspace crate, so `auv-runtime`'s dependencies are no longer "nobody knows why
root depends on it." Doc-only: **no crate moved, no workspace member changed**
(milestone rule).

## Grounded baseline (verified via cargo)

- Workspace has **36 members**: root package `auv-runtime` (member `"."`) plus
  **35 crates** under `crates/`.
- `default-members = [".", "crates/auv-cli"]` — a bare `cargo build` / `cargo
  test` only compiles `auv-runtime` + `auv-cli` and their dependency trees
  (**27/36 crates** on macOS; 9 excluded: `auv-apple-{music,notes}`,
  `auv-driver-{linux,windows}`, `auv-gnome-control-center`, `auv-netease-music`,
  `auv-overlay-macos`, `auv-qqmusic`, `auv-steam`). Experimental crates (e.g.
  `auv-driver-linux`) are **not** built by default, which is why their failing
  tests can survive (see the OCR-stub test noted in the error-chain inventory).
- **18 internal crates depend on `auv-driver-common`** (the `DriverError` and
  `InputActionResult` owner). Changing a core contract there triggers **18 crate
  rebuilds (50% of the workspace)** — this is the concrete answer to "modify one
  core contract, how many crates react?". The 18: 4 platform drivers + aggregator
  `auv-driver`, 3 core (runtime / cli-invoke / scan), 1 product (cli), 3
  reference (apple-music / notes / textedit), 7 experimental (games / app donors).

## 1. Root's direct internal dependencies (why auv-runtime depends on each)

`auv-runtime` has **10 direct internal dependencies** (verified via
`Cargo.toml` + grep). For each: **why** root depends on it + whether the dep is
target-gated + whether it's actually used in code.

| Crate | Why root depends on it | Evidence (file:line) |
|---|---|---|
| `auv-driver` | Session recording uses `InputActionResult`, `InputDeliveryPath` | `src/run_write_loop.rs:9,27` |
| `auv-driver-macos` | Runtime directly uses macOS AX types in app analysis | `src/app/analysis.rs:5,76` (⚠️ unconditional dep at root `Cargo.toml:92`, not target-gated — forces macOS-specific code into auv-runtime) |
| `auv-cli-invoke` | Runtime re-exports invoke types, implements MCP `invoke_recorded_with_finalize` | `src/lib.rs:47`, `src/mcp.rs:11,239` |
| `auv-api-proto` | gRPC session service implementation | `src/session_api.rs:6` |
| `auv-tracing-driver` | Trace IDs (`SessionId`, `DeviceId`, `new_run_id`) and timestamps for run recording | `src/lib.rs:51`, `src/run_write_loop.rs:5` |
| `auv-inspect-server` | Runtime implements `InspectReadProjection` trait | `src/inspect_read_projection.rs:1,4` |
| `auv-inspect-model` | Artifact readers, `InspectComposer`, `InspectDocument` for run-read | `src/inspect/mod.rs:3`, `src/lib.rs:48` |
| `auv-scan` | Scene state reading via `ScanFrame` | `src/scene_state_read.rs:10,476` |
| `auv-view` | View parser memory reading (`ViewBounds`, view memory types) | `src/view_parser_read.rs:12`, `src/inspect/mod.rs:10` |
| `auv-media-macos` | ⚠️ **UNUSED** — declared at `Cargo.toml:112` (target-gated to macOS) but `grep auv_media_macos:: src/` returns **zero hits**. Only real consumer is product `auv-netease-music`. This is a **dead donor-leak** into core's default deps and should be removed. | root `Cargo.toml:112`; `auv-cli/Cargo.toml:58` (also unused) |

**Actionable anomalies** (flagged for owner decision):
1. `auv-driver-macos` is an **unconditional** dependency at root `Cargo.toml:92`
   (not target-gated), which forces macOS-specific code (`app/analysis.rs`)
   into `auv-runtime`. Should either: (a) target-gate the root dep, (b)
   abstract the AX types behind a platform-agnostic trait in
   `auv-driver-common`, or (c) document why runtime is intentionally
   macOS-specific (contradicts the library-only positioning).
2. `auv-media-macos` is declared but **never imported** in root or `auv-cli`;
   only `auv-netease-music` uses it. Remove the unused deps from root
   `Cargo.toml:112` and `auv-cli/Cargo.toml:58`.

## 2. Forbidden-direction violations

**Zero violations found.** All dependency directions obey the milestone's
expected flow (`contracts/models → driver-common → platform drivers → runtime /
invoke → tracing / persistence → inspect model / server → product frontends`):

- ✅ Platform drivers (`auv-driver-{macos,linux,windows}`) only depend on
  `auv-driver-common`, not on product/app/game crates.
- ✅ Core runtime (`auv-runtime`, `auv-cli-invoke`, `auv-tracing-driver`) has
  zero dependencies on game (`auv-game-*`) or app donor (`auv-apple-*`,
  `auv-netease-music`, etc.) crates.
- ✅ `auv-inspect-model` doesn't depend on CLI (`auv-cli`).
- ✅ `auv-driver` aggregator only depends on `auv-driver-common` + the 3
  platform drivers.

The one "leak" (root depending on the unused `auv-media-macos`) is not a
*direction* violation (the dep is target-gated and forward), but a *dead
dependency* — the crate is declared but never used. See actionable anomalies
above.

## 3. Single-consumer crates (8)

Eight crates have exactly **one non-test consumer** — candidates for inlining,
archiving, or explicit single-donor justification:

| Crate | Sole consumer | Notes |
|---|---|---|
| `auv-runtime` | `auv-cli` | Expected — CLI is the sole product assembly frontend over the runtime library. |
| `auv-apple-textedit` | `auv-cli` | Reference vertical; single-product is the pattern for reference integrations. |
| `auv-game-balatro` | `auv-cli` | Experimental game donor; single-product expected. |
| `auv-game-minecraft` | `auv-cli` | Experimental game donor; single-product expected. |
| `auv-game-osu` | `auv-cli` | Experimental game donor; single-product expected. |
| `auv-godot` | `auv-cli` | Experimental; currently the AIRI Godot Stage dev observation client. |
| `auv-inference-ort` | `auv-game-balatro` | Experimental; `ort` backend only used by Balatro's detector. |
| `auv-compare` | `auv-game-minecraft` | **Recommend decoupling to experimental** (see ambiguous verdicts). Single training-result spatial-query consumer; broader abstraction deferred per `NOTICE(core-b2)`. |

No action needed for the first 7 (all are expected single-product or
single-donor relationships). `auv-compare` is flagged in the ambiguous-crate
analysis below.

## 4. Complete 36-member maintenance-tier table

Every workspace member classified into one of five maintenance tiers per the
milestone. Descriptions are from each crate's `src/lib.rs` top doc + module
structure (all `Cargo.toml` descriptions are `workspace = true` and the
workspace description is empty).

| Crate | Tier | Responsibility (one line) |
|---|---|---|
| **auv-runtime** (root pkg, member `.`) | **Core-maintained** | Core runtime: implicit run execution, artifact persistence, contract/model/session, MCP bootstrap, inspect composition, `candidate_promotion`, scroll-scan/scene-state/view-parser read projections |
| auv-driver-common | **Core-maintained** | Shared driver capability types: capture, display, geometry, input, operation, permission, readiness, selector, traits, vision, window; `DriverError`/`DriverResult` |
| auv-driver | **Core-maintained** | Platform-dispatching `LocalDriver`; re-exports `auv-driver-common` and selects linux/macos/windows inner driver at compile time |
| auv-driver-macos | **Core-maintained** | macOS-native driver (accessibility/AX, application, capture, session, readiness); some modules `#[doc(hidden)]` pub during migration |
| auv-cli-invoke | **Core-maintained** | Registry-backed CLI invoke metadata, grouping, parsing, and help rendering for `auv invoke ...` |
| auv-cli-invoke-macros | **Core-maintained** | Proc-macro `#[invoke_command]` attribute (id/group/summary/args) for invoke command registration |
| auv-tracing-driver | **Core-maintained** | Durable driver-level run/span/event/artifact recording model + recorder fan-out; emits `tracing` events, installs no subscribers |
| auv-inspect-model | **Core-maintained** | Neutral inspect composition contract (`InspectSection`/`InspectDocument`/`InspectComposer`); composition shape only |
| auv-inspect-server | **Core-maintained** | Viewer-facing HTTP/WebSocket inspection server over run storage + artifacts; read-only, no execution |
| auv-api-proto | **Core-maintained** | gRPC/protobuf session API `auv.api.session.v1` + `FILE_DESCRIPTOR_SET` for reflection |
| **auv-cli** | **Product assembly** | Product bins, app integrations, inspect composition, MCP bootstrap; owns app-specific coupling so runtime stays library-only |
| auv-apple-music | **Reference-maintained** | Apple Music app integration: window resolution + launch |
| auv-apple-notes | **Reference-maintained** | Apple Notes product commands + macOS Notes driver (note new/write/focus/compare) |
| auv-apple-textedit | **Reference-maintained** | TextEdit product commands + macOS TextEdit driver (document write/focus/compare, marker verify) |
| auv-driver-linux | **Experimental compile-maintained** | Linux/Wayland desktop driver capabilities: portal readiness, XDG screenshot capture, wayland xdg-output geometry (input via libei reserved) |
| auv-driver-windows | **Experimental compile-maintained** | Windows-native driver capabilities mirroring macOS; first capability is system OCR via `Windows.Media.Ocr` |
| auv-game-minecraft | **Experimental compile-maintained** | Minecraft scene-packet ingest/projection, sample builder, evidence, measurement, inspect |
| auv-game-osu | **Experimental compile-maintained** | osu! visual-truth detection eval (witness/quality) + spatial-query action pipelines, benchmark |
| auv-game-balatro | **Experimental compile-maintained** | Balatro card detection (semantic/witness/quality/spatial-query), detector, inspect, run-read |
| auv-godot | **Experimental compile-maintained** | Godot integration; currently the AIRI Godot Stage dev observation client |
| auv-inference-common | **Experimental compile-maintained** | Shared inference types/errors: `BoundingBox`, `ImageFrame`, `ImageSize`, `ModelConfig`, `ModelId` |
| auv-inference-ort | **Experimental compile-maintained** | ONNX Runtime (`ort`) inference backend: `OrtModelConfig`, `ExecutionProvider` |
| auv-inference-ultralytics | **Experimental compile-maintained** | Ultralytics detector session backend: config, device selection, predictions/boxes |
| auv-task-object-detection | **Experimental compile-maintained** | Object-detection task abstraction (`Detection`/`DetectionOptions`/`DetectionResult`), annotated render, `ultralytics` feature |
| auv-qqmusic | **Experimental compile-maintained** | QQ Music product CLI lib: search command flow + macOS QQ Music driver |
| auv-netease-music | **Experimental compile-maintained** | NetEase Music product CLI lib: sidebar playlist scan + agent-callable output |
| auv-gnome-control-center | **Experimental compile-maintained** | GNOME Control Center product workflows (Settings labels/page flow) over `auv-driver-linux` |
| auv-steam | **Experimental compile-maintained** | Steam product CLI lib: local installed-library queries |
| auv-overlay-macos | **Experimental compile-maintained** | macOS visual overlay (cursor flash/move/dual-cursor); trust/debug layer, explicitly NOT the input backend |
| auv-scan | **Core-maintained** | Temporal scan wire contracts, frame artifacts, producers, read-side projections (S line); root-only public import path |
| auv-view | **Core-maintained** | Generic view-parser IR shared by app crates (extracted from netease); framework-level, wide pub-field v0 API |
| auv-file | **Core-adjacent** (experimental-friendly shared infra) | Narrow JSON artifact file-IO helpers (core-b1 graduation); broader file abstraction deferred |
| auv-media-macos | **Core-adjacent** (but **dead dep in core** — remove from root/cli) | macOS system now-playing read via vendored mediaremote-adapter through `/usr/bin/perl`; app-agnostic, reports owning bundle id |
| auv-query-readiness | **Core-adjacent** | Shared derived-action eligibility triad + optional refusal-reason for spatial-query probes (core-a graduation); NOT driver window-probe readiness |
| auv-stage-status | **Core-adjacent** | Shared status for persisted semantic/witness/quality stages (core-a3 triad); vertical policy stays in producing crate |
| auv-compare | **Experimental compile-maintained** (single-consumer game helper) | Narrow dual-backend compare policy helpers (core-b2 graduation); broader spatial compare deferred |

**Coverage:** All 36 members (root + 35 crates) classified. Milestone lists
every crate that exists, and every crate is covered by the milestone. No members
are missing from the milestone, and no milestone name is absent from `members`.

**Tier counts:** Core-maintained **12** (10 milestone-listed + `auv-scan` +
`auv-view`), Product **1**, Reference **3**, Experimental **15**,
Core-adjacent **5** (`auv-file`, `auv-media-macos`, `auv-query-readiness`,
`auv-stage-status`, `auv-compare` — see verdicts below). Total = **36**.

## 5. Ambiguous-crate verdicts (7 crates, 6 questions each, final labels)

The milestone flagged 7 crates as "归属不明确" and asked 6 questions for each.
Evidence-backed verdicts:

### 5.1. auv-scan → **Core-maintained**
1. **In the 0.1 core execution chain?** Yes — it is the temporal-scan producer
   + read/inspect contract feeding the observation → scene-state → inspect leg.
2. **Why root depends:** `src/scene_state_read.rs:10,476` builds scene-state
   inspect from scan frames; `auv-cli-invoke/src/commands/scan.rs:10,130`
   (frame/coverage artifact read).
3. **Reverse/donor leak?** None. Deps = `auv-driver` (core), `image`, `serde`,
   `tempfile`.
4. **Stable public contract?** Yes — crate-root-only import discipline, typed
   errors (`ScanArtifactError`, `TimelineError`, `SceneStateError` — no bare
   `Result<_,String>`), versioned wire schemas (`SCAN_*_SCHEMA_VERSION`).
   Multi-consumer (root + invoke).
5. **Can decouple from core?** No — scene-state inspect and the `invoke scan`
   command would lose their contract.
6. **What breaks if removed:** Scene-state observation read-side + `invoke scan`
   artifact production/inspection.

**Verdict:** `保留为 core` — direct root inspect-chain dep with stable contract.

### 5.2. auv-view → **Core-maintained**
1. **In chain?** Yes, on the inspect/read-side — framework-level view-parser IR
   + view-memory used to project inspect output.
2. **Why root depends:** `src/view_parser_read.rs:12` imports `auv_view::memory::*`;
   `src/lib.rs:56-57` calls `view_parser_read::build_view_parser_inspect` then
   `auv_view::memory::summarize_view_parser_inspect`; `src/inspect/mod.rs:10`
   uses `ViewMemory, ViewParserInspect`.
3. **Reverse/donor leak?** None. Deps = `image`, `serde`, `serde_json` only
   (zero workspace deps).
4. **Stable contract?** Yes — framework IR with `VIEW_IR_SCHEMA_VERSION` guard;
   wide `pub` fields are a documented v0 choice (`NOTICE(pub-fields-v0)`).
   Multi-consumer: root, `auv-inspect-server`, `auv-tracing-driver`, product
   `auv-netease-music`.
5. **Can decouple?** No — inspect view-memory projection depends on it directly.
6. **What breaks:** view-parser/view-memory inspect projection +
   inspect-server read projection.

**Verdict:** `保留为 core` — direct root inspect-chain dep, multi-consumer
framework IR.

### 5.3. auv-file → **Core-adjacent** (experimental-friendly shared infra)
1. **In chain?** No, not in the root execution/inspect chain today.
2. **Why root depends:** Root does **not** depend on it. Consumers are only the
   game crates (balatro/minecraft/osu).
3. **Reverse/donor leak?** None. Deps = `serde` only; games depend on it
   (forward, fine).
4. **Stable contract?** Yes — narrow, clean generic JSON file IO
   (`read_json_file`/`write_json_file`, typed
   `JsonFileReadError`/`JsonFileWriteError`, no `String` errors). Deliberately
   narrow per `NOTICE(core-b1)`.
5. **Can decouple from core?** Already decoupled from root; nothing in core breaks.
6. **What breaks:** Only game-crate artifact JSON IO. It is generic reusable
   infra (not product-specific), so it belongs adjacent to core as a shared
   helper rather than in experimental/archive.

**Verdict:** `保留为 core-adjacent` — not a root dep, but a clean shared helper
with graduation record (`core-b1`). Games depend on it forward (fine). Not core
execution seam, but stable shared infra.

### 5.4. auv-media-macos → **Core-adjacent** (BUT: dead dep in core — remove from root/cli)
1. **In chain?** No — macOS system now-playing capability (mediaremote-adapter
   wrapper), outside the input-delivery/verification seam.
2. **Why root depends:** It **shouldn't** — **dead dependency**. Declared at
   root `Cargo.toml:112` (macOS) and `auv-cli/Cargo.toml:58`, but
   `rg auv_media_macos::` finds **zero** usage in `src/` or `crates/auv-cli/src/`.
   This is a donor-leak into core's default deps and should be removed. Only
   real code consumer is product `auv-netease-music`
   (`output.rs:411`, `cli.rs:8,200,553`).
3. **Reverse/donor leak?** Crate deps clean (`serde`, `clap`). The leak is the
   **unused declaration** in root/cli.
4. **Stable contract?** Yes — typed `NowPlayingState`, `MediaError`,
   `MediaCommand`, `OutputFormat`; well-documented limitations.
5. **Can decouple from core?** Yes, and it should be: removing the unused
   root/cli deps changes nothing in core.
6. **What breaks:** Nothing in core; only the product `auv-netease-music`
   now-playing feature would need to keep its own (forward) dep — which it
   already has.

**Verdict:** `保留为 core-adjacent` as a platform capability with a clean
contract, **but remove the unused root `Cargo.toml:112` and
`auv-cli/Cargo.toml:58` deps immediately** — they are dead donor-leaks.

### 5.5. auv-query-readiness → **Core-adjacent**
1. **In chain?** Adjacent — feeds the CLI run-read projection (readiness_class),
   not the input/verification seam itself.
2. **Why root depends:** Root does not. Consumed by `auv-cli` run-read
   (`crates/auv-cli/src/run_read/query_wired_projection.rs:7`,
   `map_action_eligibility_to_readiness_class`) plus games (minecraft/osu).
3. **Reverse/donor leak?** None — the crate has **no dependencies at all**.
   NOTICE explicitly keeps manifest → input mapping and geometry donor-local.
4. **Stable contract?** Yes — clean typed triad (`DerivedActionEligibility`,
   `DerivedActionReadiness`), pure mapping fns; multi-consumer (cli + 2 games).
5. **Can decouple?** Root already doesn't depend; the CLI read-side projection
   would lose its readiness_class mapping.
6. **What breaks:** CLI `query_wired_projection` readiness-class labeling + game
   readiness reads.

**Verdict:** `保留为 core-adjacent` — not a root dep but a multi-consumer shared
helper with graduation record (`core-a`). Clean contract, zero deps.

### 5.6. auv-stage-status → **Core-adjacent**
1. **In chain?** Adjacent — a shared status vocabulary for persisted
   semantic/witness/quality stages; not the execution seam.
2. **Why root depends:** Root does not. Used by `auv-cli` minecraft integration
   (`crates/auv-cli/src/integrations/minecraft/mod.rs:1023`) and games
   (balatro/minecraft/osu).
3. **Reverse/donor leak?** None. Deps = `serde` only.
4. **Stable contract?** Yes — tiny stable enum
   `StageStatus{Ready,Blocked,Failed}` with serde wire labels + `Display`,
   roundtrip-tested; shared across cli-integration + 3 game crates. This is
   exactly the kind of shared vocabulary that belongs near core.
5. **Can decouple?** Root already doesn't depend; multiple product/integration
   consumers rely on the shared labels.
6. **What breaks:** Stage-status wire labels across game readers + the minecraft
   CLI integration diverge into per-crate copies.

**Verdict:** `保留为 core-adjacent` — not a root dep but a shared stable enum
with graduation record (`core-a3`). Multi-consumer shared vocabulary.

### 5.7. auv-compare → **Experimental compile-maintained** (single-consumer game helper)
1. **In chain?** No — dual-backend spatial-compare policy helpers,
   product/game-scoped.
2. **Why root depends:** Root does not.
3. **Reverse/donor leak?** None — the crate has **no dependencies**. But its
   only consumer is a single game file.
4. **Stable contract?** Clean typed API (`DualBackendCompareVerdict`,
   `DualBackendAnswer` trait, `screen_points_match_with_tolerance`), but the
   broader abstraction is explicitly deferred (`NOTICE(core-b2)`) and it is
   exercised by exactly **one** consumer —
   `auv-game-minecraft/src/training_result_spatial_query.rs` (sole entry in both
   Cargo.toml and code grep).
5. **Can decouple?** Already decoupled from root; only `auv-game-minecraft` uses it.
6. **What breaks:** Only minecraft's dual-backend compare witness.
   Single-consumer + deferred abstraction + game-lane scope = experimental, not
   a shared core-adjacent contract (and not archive, since it is live/compiling).

**Verdict:** `解耦为 experimental` — single-consumer (minecraft), deferred
broader abstraction, game-scoped. Not a shared core-adjacent helper. Keep it
experimental compile-maintained; if a second consumer appears, reconsider
core-adjacent.

### Verdict summary table
| Crate | Root dep? | Real consumers | Contract quality | Graduation record | Final label |
|---|---|---|---|---|---|
| auv-scan | yes (used) | root inspect + invoke | typed errors, versioned wire | none (direct core use) | **Core-maintained** |
| auv-view | yes (used) | root inspect + inspect-server + tracing + netease | IR w/ schema guard | none (direct core use) | **Core-maintained** |
| auv-file | no | games only | clean generic JSON IO | `core-b1` | **Core-adjacent** |
| auv-media-macos | yes but **unused (dead)** | netease (product) only | clean capability API | none | **Core-adjacent** (remove dead root+cli deps) |
| auv-query-readiness | no | cli run-read + 2 games | clean typed triad, no deps | `core-a` | **Core-adjacent** |
| auv-stage-status | no | cli-integration + 3 games | tiny stable shared enum | `core-a3` | **Core-adjacent** |
| auv-compare | no | 1 game file only | clean but single-consumer, deferred | `core-b2` (deferred) | **Experimental compile-maintained** |

## Actionable follow-up (owner decisions required)

1. **Remove the dead `auv-media-macos` dependency** from root `Cargo.toml:112`
   and `crates/auv-cli/Cargo.toml:58` — it is declared but never imported in
   either codebase. Only `auv-netease-music` uses it (product forward-dep,
   fine). This is the only genuine donor-leak into core's default dependency set.
2. **Decide on `auv-driver-macos` target-gating**: the root dep at
   `Cargo.toml:92` is unconditional (not `[target.'cfg(...)'.dependencies]`),
   which forces macOS-specific code (`app/analysis.rs` using AX types) into
   `auv-runtime`. Options: (a) target-gate the root dep, (b) abstract the AX
   types behind a platform-agnostic trait in `auv-driver-common`, or (c)
   document why runtime is intentionally macOS-specific and accept the coupling
   (contradicts the library-only positioning).
3. **Confirm `auv-media-macos` tier**: I classified it as **core-adjacent** (a
   platform capability with a clean contract, not in the exec seam but not
   experimental either). The owner may prefer **driver-tier / core platform
   capability** instead. Clarify the distinction between "driver-tier" and
   "core-adjacent platform capability" for future classification.
