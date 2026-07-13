# AUV NetEase Music CLI v0 Design

Date: 2026-05-29

Status: v0 design spec. Pins the product CLI that exposes existing
NetEase flows as agent-callable subcommands, by extracting the example
procedures into a reusable library crate.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing or reviewing the NetEase CLI.

## Purpose

The NetEase flows today exist only as `cargo` examples
(`examples/netease_playlist_ls.rs`, `examples/netease_play_visible_anchor.rs`).
Examples are not agent-callable products: they have no stable binary
name, no subcommand surface, and no structured-output contract an agent
loop can branch on.

The end goal is a loop where **agents call these binaries**. That
requires a compiled product CLI with:

- a stable binary name (`auv-netease-music`, alias `auv-wyy`),
- subcommands that map to flows that already exist,
- structured (JSON) output and stable exit codes for branching.

This spec is the smallest change that turns the existing flows into
that product without reimplementing them and without leaking NetEase
logic into core.

## Relationship to prior specs

```text
2026-05-29-view-parser-example-placement-v0.md   placed NetEase as an EXAMPLE crate
2026-05-29-view-parser-spec-vs-pr9-divergence-triage.md   PR #9 shipped single-file example
2026-05-28-view-parser-ir-netease-playlist-example-implementation-plan.md   the ls flow
```

This spec **supersedes** the placement-v0 classification of NetEase as
an example. NetEase is promoted from "example, invoked via `cargo run
-p`" to a **product CLI**. This is a deliberate owner decision, not a
silent boundary crossing. Placement-v0 is already in triage; this entry
records the promotion. The placement-v0 import-direction rules (app
code depends down the tree, never on `auv-cli`) still hold and are
reaffirmed below.

## Crate placement and layout

One new workspace member: a **product crate** (not an `auv-example-*`
crate).

```text
crates/auv-netease-music/
  Cargo.toml
  src/
    lib.rs            // public re-exports of the procedures
    scan/             // extracted from netease_playlist_ls.rs
      mod.rs          //   observer, region detect, parse, reconstruct, scan loop
    song.rs           // extracted from netease_play_visible_anchor.rs (search -> play -> verify)
    cli.rs            // subcommand dispatch + baked defaults + JSON/exit-code emission
    output.rs         // agent-facing JSON output types
  src/bin/
    auv-netease-music.rs  // fn main() -> auv_netease_music::cli::run()
    auv-wyy.rs            // fn main() -> auv_netease_music::cli::run()  (alias binary)
  tests/
    parser_fixtures.rs    // recorded-fixture parser tests (moved from the example)
    cli_output.rs         // JSON output + exit-code snapshots
```

Both `src/bin/*.rs` are thin: each is a `main` delegating to the same
`cli::run()`. `auv-wyy` is a real second `[[bin]]`, not a shell alias,
so `cargo build` produces both binaries.

`Cargo.toml` depends on:

- `auv-driver`, `auv-driver-macos` (typed capture / OCR / window / scroll / click)
- `serde` + `serde_json`
- a CLI arg parser (`clap`) for subcommand dispatch
- workspace standard dependencies

It does **not** depend on the root `auv-cli` crate. (Layering: a leaf
product crate depends down the tree, never up.)

## Library API (extracted procedures)

The example logic moves into the crate library so the binary, the
examples, and tests all consume the same procedures. No behavior change
during extraction.

From `netease_playlist_ls.rs` into `scan/`:

- the live scan orchestration (`run_live_scan` equivalent)
- `SidebarObserver` (session + window + ratio; capture / recognize / scroll)
- region/modal detection (`detect_sidebar_region`,
  `infer_visible_playlist_body_region`, `detect_blocking_modal`)
- parsing heuristics (`parse_sidebar_viewport`, `classify_sidebar_text`,
  section/label normalization)
- reconstruction (`reconstruct_playlist_sidebar`, node builders,
  anchor/landmark collection)
- scan loop + boundary (`scan_sidebar_with_observer`,
  `scroll_observer_to_top`, `boundary_summary_from_observations`)
- the IR/artifact types (`PlaylistSidebarScan`, `PlaylistSidebarProjection`,
  `ScrollBoundarySummary`, `ViewReconstructionRecord`, etc.)

From `netease_play_visible_anchor.rs` into `song.rs`:

- the search-focus-type-submit-select-play-verify procedure
- the result-selection heuristics (`select_song_result`, anchor logic)

Platform gating is preserved: live-driver procedures stay
`#[cfg(target_os = "macos")]`; non-macOS builds keep the existing
"only available on macOS" error path.

## CLI surface

```text
auv-netease-music <command>          (auv-wyy = identical binary)

playlist [<keyword>]                 list sidebar playlists; optional name filter
song play <query>                    search -> pick result -> play -> verify

global flags (all optional, baked defaults):
  --json                             emit JSON to stdout (default: human summary)
  --json-out <path>                  write JSON to a file
  --app-id <bundle>                  default com.netease.163music
  --max-pages <n>                    default 24
  --max-scrolls <n>                  default 48
  --scroll-amount <f>                default 6.0
```

Only flows that already exist are exposed. `playlist play` and a
standalone `song search` are **not** in v0 (see Non-goals).

### `playlist [<keyword>]`

Runs the full sidebar scan in-process via the shared `scan/`
procedures, then emits the reconstructed list. The optional keyword is
a **filter view over the same complete scan**, never a search with
early-exit.

- The scan always runs to its boundary or page/scroll cap regardless of
  keyword.
- Output always includes the full item list, sections, and
  `ScrollBoundarySummary` (whether `bottom` was reached).
- If `<keyword>` is present, output additionally includes a `matches`
  array: items whose normalized label contains the normalized keyword,
  using the same normalization the scan applies
  (`strip_leading_icon_noise` / `normalize_identity`, CJK + case
  folding).

### `song play <query>`

Runs the existing visible-anchor procedure from `song.rs`: focus
search, type the query, submit, select the result, double-click to
play, verify the player bar shows the expected title/artist.

## Output contract (agent-facing)

The CLI is for an agent loop, so output and exit codes are part of the
contract.

- `--json` / `--json-out` produce a stable JSON output object. The `playlist`
  output embeds the existing `PlaylistSidebarScan` JSON (which already
  carries `schema_version: "view-ir-v0"`) plus, when a keyword was
  given, a `matches` array.
- Exit codes:
  - `0` — the operation completed (a scan that found zero matches still
    exits `0`; match count is data, not an error; a playback that
    verified exits `0`).
  - non-zero — driver/scan/verify **failure** (window not resolved,
    capture/OCR error, playback not verified).
- An agent distinguishes "playlist not present" from "scan not
  exhaustive" by reading `matches` together with
  `ScrollBoundarySummary.bottom`; it does not infer absence from an
  empty list alone.

## Examples' fate

`examples/netease_playlist_ls.rs` and
`examples/netease_play_visible_anchor.rs` are **kept** but rewritten as
thin examples over the crate library (a `main` that calls the same
procedures), and updated as needed during extraction. They remain the
small-surface demos; the binary is the product.

## play.rs / music.rs (deferred decision, recorded)

`playlist play` is deferred. When it is built (a later slice), its
activation/verify logic lives in this crate on the typed
`auv-driver-macos` API, **not** by depending on the core
`src/driver/macos/control/music.rs` command layer:

- layering: this crate must not depend on the root `auv-cli` crate;
- `music.rs` is a song-search workflow with a different candidate source
  than a playlist scan; routing playlist-play through it risks bending
  result/candidate schemas (forbidden per the core contract seam).

If the liveness re-check in `music.rs` proves worth sharing, the
principled move is to lift that primitive **down** into `auv-driver`,
consumed by both. That is a separate, owner-gated refactor and is out
of scope here.

## v0 done criteria

1. `crates/auv-netease-music/` exists and is a workspace member.
2. The crate library exposes the scan and song procedures; the two
   examples compile as thin wrappers over it with no behavior change.
3. `cargo build -p auv-netease-music` produces both `auv-netease-music`
   and `auv-wyy` binaries.
4. `auv-wyy playlist` runs the sidebar scan and emits the same
   reconstruction the current example produces, with
   `ScrollBoundarySummary` present.
5. `auv-wyy playlist <keyword>` adds a normalized `matches` array
   without altering scan completeness or the full list.
6. `auv-wyy song play <query>` reproduces the visible-anchor flow and
   verifies the player bar.
7. `--json` output is stable and embeds the existing
   `schema_version`-tagged scan artifact; exit codes follow the output
   contract.
8. `cargo check --workspace` passes with the new crate in.
9. No NetEase logic is added to the root `auv-cli` crate; the crate does
   not depend on `auv-cli` or `music.rs`.

## Forbidden in v0

- A `playlist play` subcommand (no existing flow; net-new automation).
- A standalone `song search` subcommand (only the first half of `song
  play`; no separate flow).
- A keyword filter that early-exits the scan or hides scan completeness.
- Depending on the root `auv-cli` crate or `music.rs`.
- Adding NetEase commands to the `auv-cli` catalog, recipes, or bundles.
- Inventing a new candidate/result schema beside the existing contract
  types.
- Placing this under `crates/auv-example-*` (it is a product, not an
  example).

## Non-goals for this spec

- `playlist play` (the originally-requested flow). It is the obvious
  next slice once the select-and-activate procedure exists; deferred
  here so v0 is purely a CLI surface over existing flows.
- Inspect-viewer integration of CLI runs.
- Cross-app or generic music CLI. This crate is NetEase-only.
- A shared `auv-example-common` / multi-app CLI scaffold. Wait for a
  second consumer.

## How to use this spec

- Extraction first: move example logic into the crate library with no
  behavior change, prove the examples still pass, then add the CLI
  shell. The library boundary is where regressions are cheapest to
  catch.
- If a subcommand wants a flow that does not exist yet, stop and file it
  as a follow-up slice rather than inventing automation here.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
