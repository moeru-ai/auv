# AUV Core RI3-A — Runtime Invoke Surface Parity Audit Handoff

**Date:** 2026-07-06
**Status:** `audit_complete` — inventory only; **no close verdict**
**Slice:** docs-only evidence audit — no runtime, inspect, MCP, or packaging code changes

> **Owner summary（中文）：** L9-R2 已将 candidate-action seam 正式 `close`。RI3 问的是另一条线：registry invoke、product-crate invoke、session API、typed recorded operation 是否达到同等级别的 **recording → artifact → inspect/run_read** 可证明性。本 Slice A 只产出三张证据表与 1–2 条下一 reconnect 建议；**不开 ACP-C、不补 ACP-B2c、不碰 S/M/G**。

**Prerequisites:** [L9-R2 close](../runtime/2026-07-06-action-seam-closeout.md), [API-R2b freeze](../session-api/2026-06-30-session-api-closeout.md), [invoke direct handoff](2026-06-18-invoke-direct-command-implementations-handoff.md), [C-series roadmap](../runtime/2026-06-13-core-roadmap.md)

---

## 1. Problem statement

L8/L9 closed the **candidate-action** seam:

```text
L8a plan → L8b reconciled effective + input_action_result → ATL (seam) / CAEL (ledger) → inspect
```

RI3 opens the **general command** question:

```text
invoke / typed command → run recording → artifacts → run_read / inspect
```

Packaging (ACP) reuses this substrate but does not define it. Adding a third app pack proves another wrapper, not whether non-candidate-action commands share one execution model.

---

## 2. Parity rubric

Each path is scored per dimension: `full` | `partial` | `none` | `n/a` | `frozen_intentional`.

| Dimension | Evidence source |
|-----------|-----------------|
| **Run recording** | `auv_cli_invoke::invoke_recorded*` (`crates/auv-cli-invoke/src/recorded.rs`), `RunRecordingBackend` spans/events |
| **Typed artifacts** | `InvokeCommandOutput.artifacts`, span event roles, staged artifact roles |
| **Read projection** | `src/run_read.rs` (`read_operation_result`, verification aggregation) |
| **Inspect surfaces** | `src/inspect.rs` (`Command Boundary Claims`, lineage sections), `inspect_server`, viewer |
| **Verification boundary** | typed `VerificationResult` / `operation-result` vs `command.verification` string (`TODO(invoke-boundary-claims)`) |
| **Decision pair** | `ActionResolverDecision` + `InputActionResult` on artifact (candidate-action only; invoke `input.*` deferred per L9-R2) |
| **Hermetic proof** | fixture dir, `--dry-run`, unit/integration test |

### Path families in scope

| # | Family | Entry |
|---|--------|-------|
| 1 | Root `default_registry()` | `cargo run -- invoke --help`; `crates/auv-cli-invoke/src/commands/` |
| 2 | CLI / MCP | `src/main.rs` `CliCommand::Invoke` (~L1368–1370); `src/mcp.rs` `invoke` (~L79–91) |
| 3 | Session API | `src/api/session_service/handler.rs` `finish_invoke_response` (~L125–150) |
| 4 | Product-crate invoke (separate registry) | `crates/auv-netease-music/src/invoke/mod.rs`; `auv-qqmusic` has **no** invoke surface (CLI-only) |
| 5 | Reference tier | `candidate-action` / L8–L9 (not re-audited) |
| 6 | In-span invoke | `invoke_recorded_in_span` — `src/scroll_scan/mod.rs`, `src/app/infra.rs` |

### `frozen_intentional` rule (API-R2b)

Rows where CLI/MCP lack synthetic `operation-result` / `operation-summary` persist are **`frozen_intentional`**, not RI3 bugs. Session API Package A is owner-accepted per [API-R2b](../session-api/2026-06-30-session-api-closeout.md). RI3-B must not reopen R2b-impl without owner Package B dispatch.

---

## 3. Table 1 — Proof-grade paths

Paths with **run recording + inspectable evidence + hermetic or structural test**, suitable as parity reference tiers.

| Path | Recording | Artifacts / signals | Read / inspect | Hermetic proof | Evidence |
|------|-----------|---------------------|----------------|----------------|----------|
| **candidate-action** (reference) | full | L8b pair + `operation-result` + ATL/CAEL | full (L9-R2 `close`) | extensive `cargo test` | L9-R2; not re-audited here |
| **scan.frame** | full | `scan-frame-v0` JSON + PNG staged into run | partial (artifact roles; no dedicated inspect panel) | `scan_frame_is_registered_in_default_registry`, fixture-dir tests (`scan.rs` ~L245–288) | `crates/auv-cli-invoke/src/commands/scan.rs` |
| **scan.coverage** | full | `scan-coverage-v0` artifact | partial | registry + arg tests (`scan.rs` ~L254–275) | same |
| **fixture.observe** | full | `command.verification` + `command.known_limit` span events | CLI `Command Boundary Claims` (`inspect.rs` ~L519–526) | registry/help tests (`lib.rs` ~L239–272) | `commands/fixture.rs` |
| **input.typeText / input.key** (macOS) | full | typed `input-action-result` artifact via `input_action_output` | partial (IAR in artifacts; no ATL) | `input_action_output` unit tests (`input.rs` ~L618+) | `input.rs` ~L508–542 |
| **netease.playlist.selectProof** | full (product store) | `netease-playlist-select-result` role | partial (product artifact; not in `run_read` generic join) | `select_proof_fixture_writes_run_and_artifact`, `select_proof_not_in_default_registry` | `invoke/select_proof.rs` |
| **netease.playlist.sidebarScanProof** | full (product store) | playlist sidebar scan artifact | partial | `sidebar_scan_proof_writes_scan_artifact`, `sidebar_scan_proof_no_view_memory_artifact` | `invoke/sidebar_scan_proof.rs` |
| **Session API invoke** (any catalog command) | full | trace + handler artifacts + **synthetic** `operation-summary` + `operation-result` | `GetOperation` two-source join | transport doc + handler tests | `handler.rs` ~L125–150; `operation_result_store.rs`; `transport.rs` L6 |

---

## 4. Table 2 — Partial / gap paths

Grouped by gap class. **Do not** treat API-R2b CLI/MCP persist gap as a defect (see §2).

### 4.1 `frozen_intentional` (owner boundary)

| Path | Gap | Evidence |
|------|-----|----------|
| **CLI `auv invoke`** | No synthetic `operation-result` / `operation-summary` write-through | `main.rs` ~L1368–1370 → `invoke_recorded` only; API-R2b durability matrix |
| **MCP `invoke` tool** | Same — trace + tool JSON + `run_inspect` read-back only | `mcp.rs` ~L79–91 |
| **In-span catalog invoke** (`scroll_scan`, `app/infra`) | Same — parent run context only | `invoke_recorded_in_span` callers |

### 4.2 Structural invoke deferrals (documented TODOs)

| Gap | Affected commands (sample) | Marker / evidence |
|-----|---------------------------|-------------------|
| No standalone **capture-contract** artifacts | `display.capture`, `screen.captureRegion`, `window.capture` | `TODO(invoke-capture-contract-artifacts)` — `display.rs` ~L167, `screen.rs` ~L154, `window.rs` ~L255 |
| No **RecognitionResult** artifacts | `screen.findText`, `window.findText`, `window.findIconMatch`, OCR click paths | `TODO(invoke-recognition-result-artifacts)` — `screen.rs`, `window.rs` |
| **Boundary claims** not first-class read model | all invoke handlers using `output.verification` | `TODO(invoke-boundary-claims)` — `command.rs` ~L31; rendered as span events → `inspect.rs` `Command Boundary Claims` |
| **pasteText** lacks `InputActionResult` artifact | `input.pasteText` only | `TODO(invoke-paste-input-action-result)` — `input.rs` ~L240–248 (contrast `typeText`/`key` at ~L508) |
| **input.*** root/runtime coupling | `input.smartPress`, AX focus/press, teach-click, overlay-dependent paths | `TODO(invoke-input-*)` cluster — `input.rs` |
| **overlay.*** session coupling | all `overlay.*` | `TODO(invoke-overlay-session)` — `overlay.rs` (12 handlers) |
| **mediaControl.*** typed API deferred | all `mediaControl.*` | `TODO(invoke-media-control-typed-api)` — `media_control.rs` |
| **window/screen row** helpers in root | `*.findRows`, `*.waitForRows`, `*.clickRow` | `TODO(invoke-window-rows)`, `TODO(invoke-screen-rows)` |

### 4.3 Packaging / registry isolation (not core seam blockers)

| Path | Gap | Evidence |
|------|-----|----------|
| **netease.playlist.*Proof** | Not in `default_registry()` — separate `netease_registry()` + `auv-netease-music invoke` CLI | `select_proof_not_in_default_registry` (`select_proof.rs` ~L175–179); `invoke/mod.rs` ~L42 |
| **auv-qqmusic** | No invoke registry; standalone CLI only | `crates/auv-qqmusic/src/cli.rs`; no `invoke/` module |
| **ACP-B2c unified proof hint** | Deferred packaging | L9-R2 deferrals table |

### 4.4 L9-R2 explicit deferrals (orthogonal lanes)

| Item | Lane | Reopen |
|------|------|--------|
| invoke `input.*` without decision pair | invoke | owner + L8 stable |
| `session.act_with_result` | session | owner |
| `g3-binding-fact` | MC-5 bridge | owner |

---

## 5. Table 3 — Recommended next reconnect (owner pick; not approved)

Two candidates ranked by **core surface impact**, not packaging sample count.

| Rank | Path | Why | Slice B shape (if owner picks) |
|------|------|-----|--------------------------------|
| **1** | **`input.pasteText` → typed `InputActionResult` artifact** | Same handler family as `input.typeText`/`input.key` (`input_action_output` at `input.rs` ~L508); paste is the only macOS typed-input path without IAR; closes a concrete parity hole without new schema | owner-approved feature: extend `PasteTextOptions` / driver paste API to return `InputActionResult`, wire handler, hermetic test mirroring `input_action_output` tests |
| **2** | **`TODO(invoke-boundary-claims)` read-side model** | Every catalog invoke uses string `verification` today; inspect flattens to `Command Boundary Claims` (`inspect.rs` ~L519–526). Unblocks honest activation-only vs semantic boundaries across **all** invoke commands | docs-first schema slice OR narrow read projection; **not** mixed with pasteText in one PR |

**Explicitly not recommended as RI3-B default:**

- **ACP-C / third app pack** — packaging lane; no new structural proof ([ACP gate](../runtime/2026-07-05-core-app-command-pack-gate.md))
- **Register netease proofs in `default_registry()`** — registry/packaging wiring; defer until owner wants convention proof (C3) or explicit ACP slice
- **API-R2b Package B** (CLI/MCP `operation-result` persist) — frozen unless owner reopens

---

## 6. Falsifiers (would block RI3 progression)

| # | Falsifier | Effect |
|---|-----------|--------|
| F1 | **C2 recording substrate** cannot stage invoke artifacts without `Runtime` drag-in | Pause RI3 feature slices; land C2 first ([roadmap](../runtime/2026-06-13-core-roadmap.md) §Code Facts #1) |
| F2 | New invoke work introduces a **third action-result schema** | Violates standing boundaries; block |
| F3 | Read-side invents policy/method from `InputActionResult` alone on invoke paths | Same hard rule as L8 ATL projection |
| F4 | RI3-B mixes packaging (ACP), S-lane, or L8b producer reopen | CONTRIBUTING veto — split slice |

---

## 7. Explicit deferrals

| Item | Lane | Notes |
|------|------|-------|
| ACP-C | packaging | Owner-named third app only |
| ACP-B2c / qqmusic unified hint | packaging | L9-R2 deferral |
| S / Surface Memory | observation | [lane discipline](../runtime/2026-07-05-core-surface-memory-lane-discipline.md) |
| M / G | model / graduation | Not RI3 scope |
| L8b / L9 viewer | action seam | `close` — reopen only on new failing evidence |
| API-R2b-impl | session API | Package B requires owner dispatch |

---

## 8. default_registry command inventory (2026-07-06)

Enumerated from the command list rendered by `cargo run -- invoke --help`,
excluding usage/help headings. **61** command ids across namespaces:

| Namespace | Count | Parity tier (summary) |
|-----------|------:|------------------------|
| `display.*` | 4 | partial — capture signals; no capture-contract artifact |
| `screen.*` | 8 | partial — screenshots + OCR signals; recognition/capture-contract gaps |
| `window.*` | 13 | partial — mix of capture, OCR, AX stubs (`TODO(invoke-window-*)`) |
| `input.*` | 13 | mixed — `typeText`/`key` stronger; `pasteText` IAR gap; AX/smartPress root-coupled |
| `app.*` | 2 | partial — permissions probe / activate |
| `overlay.*` | 12 | partial — visual-only; root session coupling |
| `mediaControl.*` | 6 | partial — root media backend coupling |
| `fixture.*` | 1 | proof-grade (deterministic) |
| `scan.*` | 2 | proof-grade (hermetic fixture → typed scan artifacts) |

---

## 9. Validation commands (audit time)

```sh
cargo run --quiet -- invoke --help
rg 'TODO\(invoke' crates/auv-cli-invoke/src
rg 'invoke_recorded' src/main.rs src/mcp.rs src/api
cargo test -p auv-cli-invoke scan_frame
cargo test -p auv-netease-music select_proof
cargo test -p auv-netease-music sidebar_scan_proof
git diff --check -- docs/ai/references/
```

---

## 10. Next gate

- **RI3-B:** Owner selects **one** row from Table 3 → named owner-approved feature slice with regression test.
- **Do not** open ACP-C or ACP-B2c as default follow-on.
- **Cross-lane:** C2 recording extraction remains parallel pillar if F1 triggers; C3 convention proof (`steam.library.list.v0`) is separate from RI3-B unless owner links them.

**Supersedes as active core question:** L9-R2 “continue core lane work without adding pack sample count.”
