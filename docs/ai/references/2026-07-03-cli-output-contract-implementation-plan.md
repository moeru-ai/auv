# CLI Output Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the CLI output contract from `2026-07-03-cli-output-contract-design.md` for `auv invoke` and `auv-netease-music playlist ls`.

**Scope classification:** approved feature, limited to CLI output parsing/rendering and the result data needed by those renderers.

**Current parser reality:** root `auv` uses a custom parser in `src/cli.rs`. App-local CLIs such as `auv-netease-music` use `clap`. `auv invoke` currently delegates to `auv_cli_invoke::parse_invoke_args`, which hand-parses help, command id, `--dry-run`, `--target`, `--label`, and arbitrary `--key value` command inputs. This slice should migrate that invoke parser to `clap` inside `auv-cli-invoke` instead of adding more root-level custom scanning. Root `src/cli.rs` may keep peeling existing inspect options for now, but it should not parse invoke output flags.

**Architecture:** Keep render helpers crate-local for this slice. Invoke-specific parsing and rendering belong in `auv-cli-invoke`, because that crate owns invoke commands, invoke result models, and the invoke CLI contract. The root binary should only detect the top-level `invoke` command, keep existing inspect option handling, delegate invoke argv to `auv-cli-invoke`, and call the invoke renderer. Do not introduce `auv-common-cli-renderer` until at least three consumers stabilize on the same API shape. Use a generic invoke report model first; avoid domain-specific `*Presentation` enums until a durable contract requires them.

**Tech stack:** Rust 2024, `serde`, `serde_json`, existing `clap` derive in app-local CLIs, `anstream`/`anstyle` for color-aware human output, focused unit tests for behavior boundaries rather than trivial parse aliases.

---

## File Structure

### Root CLI / Invoke

- Modify `src/cli.rs`
  - Stop owning invoke output option parsing.
  - Keep existing inspect option handling and delegate invoke argv to `auv_cli_invoke::parse_invoke_args`.
- Modify `src/main.rs`
  - Replace hard-coded invoke stdout with a call into `auv_cli_invoke::render`.
- Modify `crates/auv-cli-invoke/Cargo.toml`
  - Add `clap` for invoke parsing.
  - Add `anstream` and `anstyle` dependencies for invoke rendering.
- Modify `crates/auv-cli-invoke/src/lib.rs`
  - Replace the hand parser with a clap-backed parser.
  - Export invoke output option and renderer entrypoints.
- Modify `crates/auv-cli-invoke/src/model.rs`
  - Add invoke output option types and generic report types used by invoke output rendering.
- Modify `crates/auv-cli-invoke/src/command.rs`
  - Add optional report data to `InvokeCommandOutput`.
- Modify `crates/auv-cli-invoke/src/recorded.rs`
  - Propagate report data, command metadata, and existing detail evidence into `InvokeResult`.
- Create `crates/auv-cli-invoke/src/render.rs`
  - Human/detail/json renderers for `InvokeResult`.
- Modify representative invoke commands:
  - `crates/auv-cli-invoke/src/commands/display.rs`
  - `crates/auv-cli-invoke/src/commands/app.rs`
  - `crates/auv-cli-invoke/src/commands/input.rs`
  - `crates/auv-cli-invoke/src/commands/media_control.rs`

### NetEase

- Modify `crates/auv-netease-music/src/cli.rs`
  - Use existing `clap` parsing for `playlist ls` output options.
  - Add `--detail`, `--min-confidence <high|medium|low>`, and `--format json` where missing.
  - Preserve existing `--json` and `--json-out` behavior; `--json-out` remains the file-output mode.
- Modify `crates/auv-netease-music/src/output.rs`
  - Replace raw-scan JSON output for playlist listing with compact candidate JSON plus raw scan artifact refs.
- Create `crates/auv-netease-music/src/render/playlist.rs` only if it hides non-trivial human/detail/styling policy.
  - Keep confidence mapping private in that module unless another command reuses it.

### Docs

- Keep `docs/ai/references/INDEX.md` pointing to this plan and the design doc.
- Update `docs/TERMS_AND_CONCEPTS.md` only if implementation stabilizes a new project term beyond CLI-local output vocabulary.

---

## Task 1: Migrate Invoke Argument Parsing to Clap

**Files:**
- Modify: `crates/auv-cli-invoke/Cargo.toml`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Modify: `crates/auv-cli-invoke/src/model.rs`
- Modify: `src/cli.rs`

- [ ] **Step 1: Add invoke-owned output option types**

Add `clap = { version = "4.5", features = ["derive"] }` to `crates/auv-cli-invoke/Cargo.toml`.

Add the output option types to `crates/auv-cli-invoke/src/model.rs` and export them from `crates/auv-cli-invoke/src/lib.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeOutputOptions {
  pub json: bool,
  pub detail: bool,
}
```

Default should be:

```rust
InvokeOutputOptions {
  json: false,
  detail: false,
}
```

Do not add `--color` or a color option type in this slice. Let the rendering library perform its normal auto-detection. Add `anstream` and `anstyle` to `auv-cli-invoke` because rendering lives there, not because root CLI owns color policy.

- [ ] **Step 2: Replace the hand parser with clap**

Replace the current hand-written `parse_invoke_args` loop in `crates/auv-cli-invoke/src/lib.rs` with a clap-backed parser. It should own:

- help handling;
- command id positional parsing;
- `--dry-run`;
- `--target <bundle-id>`;
- `--label <value>`;
- `--detail`;
- `--json`;
- `--format` as a hidden JSON alias;
- open-ended `--key value` command inputs.

The parser can use clap derive or builder APIs. The implementation must prove with tests that clap still allows open-ended invoke inputs while recognizing known invoke options. Prefer a single invoke parser boundary over a clap pre-parser plus custom output scanner.

Keep the public parser API compatible with current root usage: `parse_invoke_args(&["invoke", ...])` should remain valid, even if the internal clap parser normalizes away the leading command token before parsing.

Rules:

- The command id is the first non-option command token after `invoke`.
- Output flags may appear before or after the command id. Both `auv invoke --json display.list` and `auv invoke display.list --json` should parse as command `display.list` with JSON output.
- `--detail` sets `detail = true`.
- `--json` sets `json = true`.
- `--format` is a no-value hidden alias that sets `json = true`.
- `--format json` is not needed for `auv invoke`; if a value follows bare `--format`, it should be treated by the normal command-id/input parsing rules rather than as a format value.
- Missing values for known invoke options are clap parse errors.
- Unknown flags and their values remain command inputs and are forwarded to `auv-cli-invoke`.
- Known options should remain recognized regardless of whether they appear before or after the command id.

If clap cannot support arbitrary `--key value` inputs and known options in one clean derive shape, use clap builder APIs or a small clap-owned normalization layer inside `auv-cli-invoke`. Do not move output flag parsing back into root `src/cli.rs`.

Update `InvokeCliParse::Invoke` to include:

```rust
output: InvokeOutputOptions,
```

- [ ] **Step 3: Add parser behavior tests**

Do not add tests such as `parse_invoke_accepts_detail_flag` or `parse_invoke_accepts_json_aliases`; those mostly assert obvious string mapping.

Add focused `auv-cli-invoke` parser tests that protect the non-obvious behavior:

- `auv invoke --json display.list` and `auv invoke display.list --json` parse identically;
- `--detail` affects rendered field density, not the JSON/human choice;
- `--detail` does not imply JSON;
- `--format` without a value is accepted as JSON, matching `--json`;
- user-facing invoke help shows `--json`, not the hidden `--format` alias;
- unrelated command inputs such as `--label Foo` and `--key Cmd+L` still reach `InvokeRequest.inputs`;
- `--target` and `--dry-run` retain their existing behavior;
- `invoke help`, `invoke --help`, and `invoke <command> --help` preserve existing help behavior;
- unknown `--key value` pairs still become command inputs rather than clap errors.

- [ ] **Step 4: Wire options into `CliCommand::Invoke`**

Extend the root CLI command variant:

```rust
Invoke {
  request: InvokeRequest,
  inspect: InspectOptions,
  output: auv_cli_invoke::InvokeOutputOptions,
}
```

Update existing tests and pattern matches to use `..` where they do not care about output options.

Root `src/cli.rs` should bind the new `output` field from `InvokeCliParse::Invoke`. It should not know how `--detail`, `--json`, or bare `--format` were parsed.

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p auv-cli parse_invoke
cargo test -p auv-cli-invoke parse_invoke
cargo check -p auv-cli
cargo check -p auv-cli-invoke
```

If the exact test filter finds no tests, run the closest existing root CLI parser test module and make sure the crate compiles.

---

## Task 2: Add Generic Invoke Report Data

**Files:**
- Modify: `crates/auv-cli-invoke/src/model.rs`
- Modify: `crates/auv-cli-invoke/src/command.rs`
- Modify: `crates/auv-cli-invoke/src/recorded.rs`

- [ ] **Step 1: Add generic report types**

Use renderer-oriented names rather than domain-specific `InvokePresentation`, `ActionPresentation`, `DisplayPresentation`, etc.

Add:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReport {
  pub fields: Vec<InvokeReportField>,
  pub sections: Vec<InvokeReportSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportField {
  pub label: String,
  pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct InvokeReportSection {
  pub title: String,
  pub fields: Vec<InvokeReportField>,
}
```

Expected examples:

- `input.key`: fields `Result`, `Key`, `Target`, `Backend`.
- `display.list`: one section per display, fields `Role`, `Kind`, `Size`, `Scale`, `Frame`.
- `app.permissions`: one section or field row per permission.
- `media.now-playing`: fields `Result`, `Title`, `Artist`, `Album`, `Source`.

The renderer composes the status-block heading from `InvokeResult.command_id` and `InvokeResult.command_summary`, not from handler-provided report strings. This avoids duplicating registry metadata in command handlers.

`InvokeReport` is a CLI presentation report derived from typed command results. It does not replace signals, artifacts, driver results, or other trace/evidence channels.

- [ ] **Step 2: Carry report data through invoke command output**

Add:

```rust
pub report: Option<InvokeReport>,
```

to `InvokeCommandOutput`, defaulting to `None`.

Add:

```rust
pub report: Option<InvokeReport>,
```

to `InvokeResult`, and propagate it in `recorded.rs`.

Also add and populate:

```rust
pub command_id: String,
pub command_summary: String,
pub backend: Option<String>,
pub notes: Vec<String>,
pub known_limits: Vec<String>,
pub verification: Option<String>,
```

`backend`, `notes`, `known_limits`, and `verification` already exist on `InvokeCommandOutput` but are currently recorded as events and dropped from the returned result. Detail rendering needs those values without reading the run back from storage, so propagate them into `InvokeResult` while keeping existing event recording and signals.

- [ ] **Step 3: Add behavior tests where shape matters**

Avoid tests that only instantiate a struct. Add tests for producer helpers:

- `InvokeCommandOutput::new()` and a macro-expanded fixture output default to `report: None`;
- recorded success propagates a fixture report into `InvokeResult`;
- report and detail evidence are not dropped on an artifact-recording failure path that still returns command output.

Test through helper functions or command outputs, not through arbitrary struct construction.

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p auv-cli-invoke report
cargo check -p auv-cli-invoke
```

---

## Task 3: Render `auv invoke` in `auv-cli-invoke`

**Files:**
- Modify: `crates/auv-cli-invoke/Cargo.toml`
- Modify: `crates/auv-cli-invoke/src/lib.rs`
- Create: `crates/auv-cli-invoke/src/render.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement color-aware writer helpers**

Use `anstream::AutoStream` and `anstyle` directly. Labels such as `Run`, `Status`, `Result`, `Target`, `Backend`, field names, and section field labels should render gray when color is enabled.

Do not create a project-wide color abstraction in this slice. Keep helpers private to `auv-cli-invoke::render` unless reuse pressure appears in a later slice.

- [ ] **Step 2: Implement human summary format**

Default `auv invoke` output shape:

```text
OK. Run: run_...

● input.key - Press keyboard shortcut
   Result: delivered
      Key: Cmd+L
   Target: com.apple.Safari
  Backend: macos
```

For `display.list`:

```text
OK. Run: run_...

● display.list - List displays
   Result: 2 displays

  display_0
     Role: primary
     Kind: built-in
     Size: 3008x1692 logical
    Scale: 2.000
    Frame: x=0,y=0,w=3008,h=1692

  display_1
     Role: external
     Size: 1920x1080 logical
    Scale: 1.000
    Frame: x=3008,y=0,w=1920,h=1080
```

Use aligned labels for scanability, but do not require table rendering.

- [ ] **Step 3: Implement detail mode**

`--detail` includes the summary output plus:

- notes;
- known limits;
- verification;
- artifacts;
- selected signals when useful.

The default summary must stay compact.

- [ ] **Step 4: Implement JSON format**

`--json` emits a renderer-owned DTO such as `InvokeJsonOutput`, not a blind serialization of internal `InvokeResult`.

Include stable fields:

- `run_id`;
- `status`;
- `command_id`;
- `summary`;
- `report`;
- `failure`;
- `artifacts`;
- `signals` only when `detail = true` or when explicitly selected for the compact contract.

JSON mode must not include ANSI sequences. Do not make internal fields such as `producer_span_id`, local-only artifact paths, or future run-record implementation details part of the default CLI JSON contract accidentally.

- [ ] **Step 5: Replace root hard-coded stdout**

In `src/main.rs`, replace the current hard-coded invoke printing with a call into the `auv-cli-invoke` renderer, passing:

- the `InvokeResult`;
- the parsed `InvokeOutputOptions`;
- stdout/stderr handles as needed.

Root `main.rs` should not know display-specific, input-specific, or report-section formatting rules.

- [ ] **Step 6: Define failure handling**

The root invoke path should render exactly once. Avoid printing a human failure block in the renderer and then returning a second top-level error with the same text.

Preferred behavior:

- renderer prints success or failure output;
- root maps failed `InvokeResult` to a non-zero exit status without emitting duplicate human text;
- if the current top-level error path must remain in place, renderer output should not repeat the final failure line.

- [ ] **Step 7: Add renderer fixture tests**

Add fixture-based tests for:

- default success output omits `operatorSummary`, raw `signals`, artifacts, notes, and known limits;
- failed output renders `ERROR`, failure message, and inspect hint;
- `--detail` includes notes, known limits, verification, artifacts, and selected signals;
- JSON output parses as JSON and contains no ANSI escape sequences.

- [ ] **Step 8: Verify with real commands**

Run:

```bash
cargo run -p auv-cli -- invoke display.list
cargo run -p auv-cli -- invoke display.list --detail
cargo run -p auv-cli -- invoke display.list --json
```

Expected:

- default output names each display and exposes the important fields;
- detail output remains readable and includes extra evidence;
- JSON parses with `jq` or `serde_json`;
- no ANSI escape codes appear under `--json`.

---

## Task 4: Populate Reports for Representative Invoke Commands

**Files:**
- Modify: `crates/auv-cli-invoke/src/commands/display.rs`
- Modify: `crates/auv-cli-invoke/src/commands/app.rs`
- Modify: `crates/auv-cli-invoke/src/commands/input.rs`
- Modify: `crates/auv-cli-invoke/src/commands/media_control.rs`

- [ ] **Step 1: Add small private builder helpers**

Add private report builder helpers only where they hide formatting policy or non-trivial field selection. Do not extract one-call helpers that only rename a struct literal.

Examples:

- display report builder formats role, kind, size, scale, and frame consistently;
- input report builder formats key, target, result, and backend;
- permission report builder groups permissions into readable fields;
- media report builder omits absent metadata instead of printing empty values.

- [ ] **Step 2: Attach reports to command outputs**

Set `output.report = Some(...)` for the representative commands. Keep `output.summary` and `output.signals` as existing durable evidence channels.

- [ ] **Step 3: Verify command-level tests**

Add command-level report tests for the representative commands:

- display list report includes both display IDs and preserves role/kind/scale/frame values;
- `input.key` report includes the delivered key and target.

Run focused tests for touched commands and then:

```bash
cargo test -p auv-cli-invoke
```

---

## Task 5: Compact NetEase Playlist Listing Output

**Files:**
- Modify: `crates/auv-netease-music/src/cli.rs`
- Modify: `crates/auv-netease-music/src/output.rs`
- Create: `crates/auv-netease-music/src/render/playlist.rs` if the output policy is too large for `output.rs`

- [ ] **Step 1: Keep clap as the parsing boundary**

`auv-netease-music` already uses `clap`, so add playlist-local output options through existing derive structs and value parsers:

- `--detail`
- `--min-confidence <high|medium|low>`
- `--format json` as an alias for existing `--json`

Preserve existing behavior:

- `--json-out <path>` keeps current file-output precedence over stdout JSON;
- `--json` and `--format json` are equivalent stdout JSON requests;
- do not reuse `auv_media_macos::OutputFormat` for playlist output.

Use a local options type such as:

```rust
struct PlaylistOutputOptions {
  mode: OutputMode,
  detail: bool,
  min_confidence: Option<Confidence>,
}
```

Use `clap::ValueEnum` or an existing typed parser for enum-like values. Do not hand-roll parsing unless the existing CLI pattern already requires a custom validator.

- [ ] **Step 2: Implement confidence display from existing data**

Confidence display:

- `H`: high confidence
- `M`: medium confidence
- `L`: low confidence

Human output uses colored dot plus code:

```text
● H   pl_0  Trance Classics       82 songs
● H   pl_1  Progressive Trance    44 songs
● M   pl_2  Vocal Trance          31 songs
```

If color is disabled, the `H/M/L` code must still make the confidence readable.

First slice maps the existing `Confidence::{High, Medium, Low}` data into display codes. Defer `XH`, `XL`, numeric scores, and numeric `--threshold <number>` unless this slice also propagates raw OCR/source scores into playlist match refs and documents that score derivation with a `NOTICE:`.

- [ ] **Step 3: Implement compact default output**

Default `playlist ls <query>` output should show:

- observed playlist count;
- match count after `--min-confidence` filtering;
- compact candidate rows with scan-local refs;
- one short hint for detail or JSON only when helpful.

Do not print raw OCR/scan dumps by default.

- [ ] **Step 4: Implement detail output**

`--detail` adds OCR/source evidence and ambiguity notes for selected candidates, but still avoids dumping the full scan record unless explicitly requested by a future flag.

- [ ] **Step 5: Implement compact JSON**

JSON output should contain:

- query;
- min confidence filter;
- candidate list with scan-local `candidate_id` / `anchor_id` refs, label, confidence level, and source evidence;
- `artifacts.scan_cache_path`;
- optional `run_id`;
- `known_limits`;
- query resolution;
- existing `view_memory` fields when present.

Do not embed the full raw scan object in `playlist ls --json`. Compact JSON must not break the existing cache handoff used by follow-up commands such as `playlist play --candidate-id` with the same `--artifact-dir`.

- [ ] **Step 6: Add focused tests**

Tests should cover:

- `--min-confidence` filtering;
- confidence level mapping from existing data;
- colorless marker includes code;
- serialized compact JSON omits raw `scan`, includes scan/cache refs, and preserves query resolution, known limits, and view memory;
- no-query human output summarizes sections instead of dumping every item;
- query human output renders ranked scan-local refs and confidence codes;
- hidden filtered count is reported;
- detail mode includes extra evidence.

Use existing `clap_*` test style for parser coverage, but avoid duplicating every obvious alias unless it protects an existing regression.

---

## Task 6: Validation

- [ ] Run required automated checks:

```bash
cargo fmt --check
cargo test -p auv-cli-invoke
cargo test -p auv-cli parse_invoke
cargo test -p auv-netease-music playlist
cargo check
git diff --check
```

- [ ] Run manual live smoke checks when the local machine has the required macOS permissions, display access, app state, and OCR/automation setup:

```bash
cargo run -p auv-cli -- invoke display.list
cargo run -p auv-cli -- invoke display.list --detail
cargo run -p auv-cli -- invoke display.list --json
cargo run -p auv-netease-music -- playlist ls Trance
cargo run -p auv-netease-music -- playlist ls Trance --detail
cargo run -p auv-netease-music -- playlist ls Trance --format json
```

For JSON smoke checks, parse with `serde_json` or `jq` if available.

---

## Explicit Deferrals

- `auv-common-cli-renderer` is deferred until at least three CLI surfaces need the same API and the call sites prove the shape. Trigger: root invoke, NetEase, and one more app-local CLI all duplicate the same renderer responsibilities.
- Full root CLI migration to `clap` is deferred. Trigger: owner approves a parser migration slice that preserves open-ended invoke command input behavior.
- Domain-specific invoke report variants are deferred. Trigger: JSON consumers or inspector APIs need stable typed display/media/permission schemas rather than generic report sections.
- Raw NetEase scan export flags are deferred. Trigger: owner asks for a debugging/export path separate from default and compact JSON output.

---

## Acceptance Criteria

- `auv invoke display.list` default output names each display and shows role, kind, size, scale, and frame in a readable status-block format.
- `auv invoke input.key` default output highlights result, key, target, and backend without dumping low-value trace data.
- Labels/field names may render gray when the output library auto-enables color.
- `--json` emits parseable JSON with no ANSI escapes.
- `auv-netease-music playlist ls` defaults to compact ranked refs with confidence marker codes.
- `auv-netease-music playlist ls --min-confidence <level>` filters low-confidence candidates using existing confidence data.
- NetEase JSON output references raw scan artifacts instead of embedding the full raw scan.
- No shared renderer crate is introduced in this slice.
