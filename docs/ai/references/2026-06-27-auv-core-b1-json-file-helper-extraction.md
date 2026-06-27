# 2026-06-27 AUV Core-B1 JSON file helper extraction

Date: 2026-06-27

Status: implemented helper-only extraction. This note records a narrow code move.
It does **not** graduate any MC-10 through MC-17 donor contract into core.

## Why this slice exists

Core-A already froze two constraints:

1. MC-10 through MC-17 donor symbols are still mostly `keep app-specific` or at
   most `candidate core contract`.
2. Core-B must not start by inventing a generic runtime or shared provider API
   just because the Minecraft vertical looks internally coherent.

That left one honest first move: extract a helper that is already repeated
across more than one owned vertical and does not freeze donor semantics.

JSON artifact file IO passed that bar.

## What changed

Added a new crate:

- `crates/auv-file`

Current scope of that crate is intentionally narrow:

- `read_json_file`
- `write_json_file`
- `JsonWriteOptions`
- `JsonFileReadError`
- `JsonFileWriteError`

The helper owns only low-level concerns:

- open / parse split for JSON reads
- pretty JSON serialization
- optional parent directory creation
- optional trailing newline append
- file write error split

It does **not** own caller-facing wording, artifact labels, stage semantics, or
vertical-specific error policy.

## Why this helper is admissible now

This extraction satisfies the earlier-helper bar from Core-A D4:

- repeated in more than one owned vertical
- extraction removes glue duplication without creating a new domain contract
- helper names are file-IO names, not Minecraft donor names

The concrete donors were:

- `crates/auv-game-minecraft/src/training_result_semantic.rs`
- `crates/auv-game-minecraft/src/training_result_spatial_query.rs`
- `crates/auv-game-minecraft/src/training_result_holdout_preview.rs`
- `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs`
- `crates/auv-game-osu/src/benchmark.rs`
- `crates/auv-game-osu/src/dataset.rs`

This is enough repetition to justify helper extraction, but not enough to claim
"shared artifact contract" or "shared stage runtime".

## Deliberate non-goals

This slice intentionally does **not**:

- extract MC-10 through MC-17 status enums
- extract provider/reference compare verdicts
- extract action-readiness contracts
- invent a generic artifact store API
- unify viewer or inspect pairing
- centralize vertical error-message text

Those remain blocked by the Core-A proof matrix. Dual-backend compare helper
design (still no enum contract extraction): see
[`2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md`](2026-06-27-auv-core-b2-dual-backend-query-compare-helper-extraction.md).

## Error-policy decision

The shared helper returns structured low-level errors.

Per-vertical wrappers still map those low-level errors back into the current
local wording, for example:

- Minecraft keeps `failed to open {label} ...` and `failed to parse {label} ...`
- osu keeps `failed to read ...`, `failed to parse ...`, and `failed to encode ...`

That matters because caller-facing wording is part of the current vertical
behavior and tests. Centralizing those strings now would couple unrelated
surfaces and create fake shared semantics.

## Current wrapper sites

### Minecraft wrappers kept local

- `training_result_semantic.rs`
- `training_result_spatial_query.rs`
- `training_result_holdout_preview.rs`
- `training_result_holdout_render_quality.rs`

These still expose local `read_json_file` / `write_json_file` helpers, but they
are now thin adapters over `auv-file`.

### osu wrappers kept local

- `benchmark.rs`
- `dataset.rs`

These still expose local `read_json` / `write_json` helpers, but they now map to
`auv-file` while preserving the old read/write wording and newline behavior.

## Behavior preserved on purpose

This extraction preserves the existing local differences:

- Minecraft writes pretty JSON **without** a trailing newline in these four MC
  modules.
- osu benchmark writes pretty JSON **with** a trailing newline.
- parent directory creation remains opt-in and is not silently enabled for all
  callers.
- caller-facing labels stay local instead of moving into the shared crate.

## Validation

Implemented and validated with:

- `cargo fmt --check`
- `cargo check -p auv-file -p auv-game-minecraft -p auv-game-osu`
- `cargo test -p auv-file`
- `cargo test -p auv-game-minecraft training_result_semantic`
- `cargo test -p auv-game-minecraft training_result_spatial_query`
- `cargo test -p auv-game-minecraft training_result_holdout_preview`
- `cargo test -p auv-game-minecraft holdout_render`
- `cargo test -p auv-game-osu --lib`
- `git diff --check`

Note: `cargo test -p auv-game-osu` still includes an existing integration test
that depends on local fixture state under `.tmp-osu-dispatch-p4ab-closeout/`.
That failure is outside this helper-only slice and was not changed here.

## Honest conclusion

Core-B1 is a **shared helper extraction**, nothing more, and explicitly **not**
a contract graduation.

The repository now has a reusable JSON artifact file helper that serves more
than one vertical. The MC-10 through MC-17 contracts remain donor-local, and
Core-A's graduation bars are unchanged.
