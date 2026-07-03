# CLI Output Contract Design

Status: proposed design, owner-selected approach C

Lanes: `core/invoke-cli`, `vertical/netease-music`

## Context

The current CLI output shape exposes runtime metadata before domain results.
For `auv invoke display.list`, the default stdout prints `runId`, `status`,
`output`, and `operatorSummary`, while the actual display inventory is only
available through returned `signals` or follow-up inspection. For
`auv-netease-music playlist ls`, default and JSON output can go the opposite
direction: too much high-entropy OCR, candidate, anchor, diagnostic, and scan
evidence is pushed to stdout.

These are related contract problems, but they are not the same renderer
problem:

- `auv invoke` is an atomic operation surface. It should replay the actual
  parameters used and show concise execution feedback.
- `auv-netease-music playlist ls` is a high-entropy discovery surface. It
  should compress observations into ranked, actionable refs and keep raw
  evidence out of default stdout.

This design defines one CLI output contract and two renderer families. It does
not approve broad CLI rewrites, game vertical changes, or a new public renderer
crate in the first implementation slice.

## Goals

- Make default human output result-first and readable in one CLI call.
- Stop requiring `inspect`, stdout cropping, or search through run records to
  understand normal command results.
- Preserve run and artifact traceability without making run metadata dominate
  default output.
- Make JSON output compact, stable, and machine-friendly.
- Keep raw scans, OCR evidence, trace events, diagnostics, and full artifacts
  available through `--detail`, artifact files, or inspect paths.
- Share output semantics across `auv invoke` and app-local CLIs without forcing
  every command into the same visual layout.

## Non-Goals

- Do not introduce `auv-common-cli-renderer` in the first slice.
- Do not redesign inspect output or inspect server UI.
- Do not change command execution behavior, driver behavior, OCR behavior, or
  playlist matching semantics as part of rendering work.
- Do not route app-local CLIs through `auv invoke` just to share formatting.
- Do not expand archived `candidate-action` or game vertical surfaces.

## Approach

Use approach C: define a shared output contract, implement crate-local renderer
helpers first, and extract a common crate only after reuse stabilizes across
multiple surfaces.

The first implementation should create internal renderer modules near the
owners of the output contract:

- `auv-cli-invoke`: an internal invoke renderer module used by the root binary.
  The root binary should parse top-level command shape and call the renderer,
  but it should not own display/input/media-specific invoke layout rules.
- NetEase crate: an internal result renderer module used by
  `crates/auv-netease-music/src/cli.rs`.

Shared concepts should be named consistently, but the Rust API does not need to
be public or cross-crate in the first slice. If the same rendering primitives
are later used by at least `auv invoke`, `auv-netease-music`, and a third CLI
such as `auv-steam` or `auv-media-macos`, extraction into a crate can be
reconsidered.

## Output Modes

Every modified surface should converge on these modes:

| Mode | Purpose | Required behavior |
|---|---|---|
| Default human | One-call operator reading | Domain result first; concise; no raw trace dumps |
| `--detail` | Human diagnostic detail | Adds metadata, signals, artifacts, known limits, verification, diagnostics |
| `--json` | Machine consumption | Stable object, no ANSI, compact result, artifact refs instead of raw scans |

`--json` is the preferred public spelling for new work. Existing CLIs that
already use `--format summary|json` may keep that spelling, but new or changed
commands should also accept `--json` unless doing so would break an existing
argument contract. For `auv invoke`, bare `--format` is a hidden compatibility
alias for JSON and does not take a value; user-facing help should prefer
`--json`.

## Color and Labels

Human output may use ANSI color when the output library's auto-detection says
it is appropriate.

- Field labels such as `Result:`, `Target:`, `Backend:`, `Key:`, and `Source:`
  should render gray in colored TTY output.
- Values should remain default foreground color unless the value itself is a
  status or warning.
- `OK` may be green and failures may be red, but color must not be the only
  carrier of meaning.
- JSON output must never include ANSI sequences.

Use an ANSI-aware Rust library such as `anstream` plus `anstyle`, or an
equivalent already accepted by the repository, instead of hand-writing escape
sequences or adding a CLI-level color policy in this slice. Table-heavy
libraries are not the default choice for this slice because `auv invoke` should
not become a table-first renderer.

## Confidence Display

High-entropy result lists such as playlist matches should use a confidence
marker:

```text
● XH  pl_0   我喜欢的风格 | Trance 精选
● H   pl_1   我喜欢的风格 | Progressive Trance
● M   pl_2   我喜欢的音乐 | Uplifting Trance
● L   pl_3   两分钟不够爽来超长 Trance-like！
```

The dot may be colored in TTY output, but the text code is required so
colorless output remains readable. Levels are:

| Code | Meaning |
|---|---|
| `XH` | extremely high confidence |
| `H` | high confidence |
| `M` | medium confidence |
| `L` | low confidence |
| `XL` | extremely low confidence / unreliable |

`--threshold <number>` should filter results by numeric score only where a
command already has a score or this slice explicitly defines a score source.
Commands that only have coarse confidence levels should use a level threshold
such as `--min-confidence high|medium|low` instead of inventing a numeric
score in the renderer. Thresholds must not redefine the level names or color
mapping. If hidden results exist, default output should say how many were
hidden and how to reveal them.

## `auv invoke` Contract

`auv invoke` default output should use a systemctl-style status block optimized
for atomic operations.

Default successful shape:

```text
OK. Run: run_...

● input.key - Press keyboard shortcut
      Key: Cmd+L
   Target: com.apple.Safari
   Result: delivered
  Backend: System Events
```

Default failed shape:

```text
ERROR. Run: run_...

● display.list - List connected displays
   Result: failed
    Error: screen_recording permission was denied

Inspect: auv inspect run_...
```

The default block should show:

- command id and short command summary,
- actual important inputs used by the handler,
- domain result,
- target and backend when meaningful,
- concise artifact refs only when artifacts are primary outputs,
- known limits or verification only when they materially affect how to read
  the result.

It should not show `operatorSummary` by default. `runId` should be compressed to
the `OK. Run: ...` / `ERROR. Run: ...` header.

`--detail` should add the material that currently dominates or hides behind
inspect:

- full run id and status,
- backend,
- all signals,
- artifacts with kind, preferred name, and persisted path,
- notes,
- known limits,
- verification / boundary claim,
- inspect hint.

### Low-Entropy List Results

Small structured `invoke` results such as `display.list`, `window.list`, and
permission probes should still be readable in one call. They may use repeated
sub-blocks inside the invoke status block rather than NetEase-style compressed
search refs.

Example:

```text
OK. Run: run_...

● display.list - List connected displays
   Result: 2 displays observed
  Backend: auv-driver-macos.display

  display_0
      Role: primary
      Type: built-in
      Size: 3008x1692 logical
     Scale: 2.000
    Origin: 0,0

  display_1
      Role: external
      Type: external
      Size: 1920x1080 logical
     Scale: 1.000
    Origin: 3008,0
```

This keeps `invoke` visually consistent without forcing every list into a
fixed-width table.

### Required Model Change

`InvokeCommandOutput` currently has `summary`, flat `signals`, `known_limits`,
and a string `verification`. A rendering-only change can improve default output
for a few commands by reading known signal keys, but the durable direction
should add a typed presentation/result layer.

The first implementation slice may add a minimal internal enum or structured
payload, for example:

- action feedback payload for `input.key`, `input.clickPoint`,
  `input.clickWindowPoint`, `app.activate`,
- display inventory payload for `display.list`,
- permission payload for `app.probePermissions`,
- media state payload for `mediaControl.nowPlaying`.

Flat `signals` should remain as trace and diagnostic evidence, not the primary
human result API.

## NetEase Contract

`auv-netease-music` should follow the same output mode rules but use a
different default renderer for discovery commands.

`playlist ls` default output should be a compressed ranked result list, not a
status block for every observed item and not a raw scan dump.

With a query:

```text
85 playlists observed. 4 matches for "Trance".

● XH  pl_0   我喜欢的风格 | Trance 精选
● H   pl_1   我喜欢的风格 | Progressive Trance
● M   pl_2   我喜欢的音乐 | Uplifting Trance
● L   pl_3   两分钟不够爽来超长 Trance-like！

Use: auv-netease-music playlist play --candidate-id pl_0
More: --detail, --json
```

Without a query, default output should avoid printing every item. It should
summarize observed counts, sections, and the next useful command:

```text
85 playlists observed.

Sections:
  MyPlaylists: 72
  LibraryNav: 4
  Unknown: 9

More: use a keyword, --detail, or --json.
```

`--detail` should show section, confidence details, candidate id, anchor id,
diagnostics, known limits, and artifact/cache paths. It may show all items.

`--json` should be compact:

```json
{
  "ok": true,
  "command": "playlist.ls",
  "query": "Trance",
  "summary": "85 playlists observed, 4 matches",
  "result": {
    "item_count": 85,
    "match_count": 4,
    "matches": [
      {
        "ref": "pl_0",
        "label": "我喜欢的风格 | Trance 精选",
        "candidate_id": "obs...",
        "anchor_id": "anchor...",
        "confidence": {
          "level": "H",
          "reason": "existing scan confidence and query match"
        }
      }
    ]
  },
  "artifacts": {
    "scan_cache_path": "/tmp/.../playlist-scan-cache.json"
  },
  "known_limits": []
}
```

The current `scan` field in `PlaylistJsonOutput` should not remain in default
stdout JSON. Full scan data should be written to a file and referenced from
JSON. This keeps JSON machine-friendly without turning it into a token-heavy
evidence dump. Playlist candidate refs are scan-local follow-up tokens, not
globally stable playlist identities.

## Library and Module Plan

First slice:

- Do not create `auv-common-cli-renderer`.
- Add crate-local renderer helpers.
- Use `anstream` / `anstyle` or an equivalent approved ANSI abstraction.
- Keep helpers private or `pub(crate)` until at least three consumers stabilize.

Candidate internal module shapes:

```text
crates/auv-cli-invoke/src/render.rs

crates/auv-netease-music/src/render/
  playlist.rs
```

The exact paths can change during implementation if a nearby existing module
owns the responsibility more clearly. The boundary should be:

- render modules format already-computed result data,
- command handlers compute result data,
- storage/artifact modules persist raw evidence,
- JSON builders build compact public JSON, not human strings.

Extraction trigger for a future `auv-common-cli-renderer` crate:

- at least three non-test call sites use the same block/label/color/confidence
  primitives,
- the primitives no longer encode NetEase-specific or invoke-specific terms,
- tests prove colorless output, colored output, and JSON output remain stable.

## Migration Slices

1. Migrate the `auv-cli-invoke` parser to clap and add output mode parsing for
   `auv invoke`: `--detail`, `--json`, and bare `--format` as a JSON alias.
2. Add a minimal invoke renderer and convert `display.list`,
   `app.probePermissions`, `mediaControl.nowPlaying`, and one input command.
3. Add typed result/presentation payloads for converted invoke commands.
4. Move `operatorSummary`, full signals, artifacts, known limits, and
   verification to `--detail`.
5. Refactor NetEase `playlist ls` human output to compressed ranked refs.
6. Replace NetEase `playlist ls --json` raw scan embedding with compact JSON
   plus artifact refs.
7. Add confidence marker support for playlist matches using existing confidence
   data first; defer numeric score thresholds unless a score source is approved.
8. Evaluate whether Steam/media helpers should adopt the same output mode
   spelling. Do not change their behavior in the first slice unless needed for
   shared helper validation.

## Testing

Use focused unit tests beside renderer code:

- colorless invoke status block output,
- optional colored label rendering through deterministic renderer fixtures,
- no ANSI in JSON,
- `display.list` renders both displays without inspect,
- `input.key` renders actual key and target,
- NetEase playlist query renders ranked refs and confidence codes,
- NetEase no-query output summarizes without dumping every item,
- NetEase JSON omits raw `scan` and includes scan artifact ref,
- confidence filtering hides lower-confidence results and reports hidden count.

Add integration or snapshot-style tests only for stable CLI outputs. Avoid
testing terminal color through fragile raw escape strings unless the helper
provides a narrow deterministic rendering mode.

## Open Decisions

- Whether `--json` should be accepted everywhere immediately or introduced only
  where output-mode aliases already exist.
- The exact score thresholds for `XH/H/M/L/XL`. The first NetEase slice may map
  existing enum confidence into levels and add numeric scores later if no
  stable score exists today.

## Acceptance Criteria

- A user can run `auv invoke display.list` once and see both displays in
  default stdout.
- A user can run an input invoke command once and see the actual requested
  action parameters and result.
- `operatorSummary` is no longer part of default `auv invoke` stdout.
- `--detail` exposes run metadata and diagnostics that default output omits.
- `auv-netease-music playlist ls <query>` default output is bounded and
  actionable.
- `auv-netease-music playlist ls <query> --json` is compact and does not embed
  raw scan observations.
- Colorless output remains readable through labels and confidence codes.
