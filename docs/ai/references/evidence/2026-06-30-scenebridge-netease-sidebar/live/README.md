# SceneBridge A6 live evidence

`proof_class: live`

Owner-labeled macOS evidence for **A6** NetEase ViewMemory reacquire sign-off.
Hermetic tests remain the required CI gate; this folder holds the **live** acceptance
matrix (Cases A–E) and redacted attachments.

**Closure:** [A6 live evidence closure](../../2026-06-30-auv-scenebridge-a6-live-evidence-closure.md)

## Prerequisites

- NetEase Music (`com.netease.163music`) installed and foreground-visible
- macOS with driver permissions (Accessibility, Screen Recording as required)
- `AUV_NETEASE_VIEW_MEMORY=1` for reacquire path (Cases A–D)
- Sidebar must produce a writable `ViewMemory` on `playlist ls` (non-empty scan)
- **Logged-in account** with at least one **named playlist** in the sidebar
  (`创建的歌单` / `收藏的歌单` items). Guest / `创建的歌单 0` yields `item_count=0`
  and blocks Cases A–E (A6b probe: `case-ls-probe.json`).
- Current live status (2026-07-01 @ A6c-9):
  - **Closed:** default-window geometry (`case-ls-a6c3-default-probe.json`: `height≈286`, `item_count≥1`).
  - **Closed:** dedup-only ViewMemory write (`case-ls-a6c3-resized-probe.json` + default probe).
  - **Closed (hermetic):** `query_match` exact-first resolution (`playlist_query_resolution_is_unique_exact`)
    is unit-tested and wired into `scan.rs`'s `observation_satisfies_query`.
  - **Closed (live @ A6c-8b):** `playlist ls '3'` pre-gate — `/tmp/auv-case-b-live-20260701-1725`:
    `item_count=42`, `match_count=1`, `matches[0].label="3"`.
  - **Open:** Case B miss (`not_found`) — see [`SIGNOFF.md`](SIGNOFF.md). 1725 live showed `select`
    fell through to **Case D** (`reacquire=null`, `view-memory file missing`) because `playlist ls`
    skipped ViewMemory write when `sidebar_region_fallback_used` diagnostics were present.
    **A6c-9** relaxes the write gate for the paired fallback path and adds `view_memory.written` to
    `ls --json`. Owner must re-run Case B after A6c-9 with `jq -e '.view_memory.written == true'`
    (or `test -f view-memory-playlist_sidebar.json`) before manual scroll-off + `select`.

## Hermetic pre-gate (run before live)

```bash
cargo fmt --check
cargo check -p auv-view -p auv-netease-music
cargo test -p auv-view memory
cargo test -p auv-netease-music playlist_select
git diff --check
```

## Acceptance matrix (Cases A–E)

| Case | Preconditions | Expected `PlaylistSelectResult` signals | Expected `steps[]` signals |
| --- | --- | --- | --- |
| **A Hit** | gate=1; `ls` then **no** large scroll; `select` same label | `reacquire.outcome=reacquired`; `skipped_rescan_replay=true`; `strategy_used` non-empty | contains `reacquire-target`; **no** `scroll-sidebar-top-*` |
| **B Miss** | gate=1; after `ls` manually scroll target off-screen; `select` same label | `reacquire.outcome=not_found`; `skipped_rescan_replay=false` | `known_limits` miss fallback text; **has** `scroll-sidebar-top-*` + `scroll-sidebar-target-page-*` |
| **C Stale** | gate=1; after `ls` edit `view-memory-playlist_sidebar.json` (recipe below) | `reacquire.outcome=stale`; `stale_reason` is a wire value | same as B: honest rescan replay |
| **D Memory missing** | gate=1; after `ls` delete view-memory file | `reacquire=null`; `known_limits` missing-memory text | rescan replay |
| **E Gate off** | unset/`0` env; otherwise same as A | `reacquire=null` | rescan replay (pre-A3 path) |

Wire values for `reacquire.outcome`: **`reacquired` / `stale` / `not_found`** (not `hit`).
See [`reacquire_adapter.rs`](../../../../../../crates/auv-view/src/memory/reacquire_adapter.rs).

## Stale recipes (pick one)

### Freshness TTL

Edit `view-memory-playlist_sidebar.json` after `ls`:

```json
"last_reconstructed_at_millis": <now_millis - 25h>
```

Default TTL is 24h (`DEFAULT_MEMORY_TTL_MILLIS`). Expect
`stale_reason=memory_rejected_at_freshness`.

### Baseline drift

Change `scope_snapshot.baseline_width` by ±50 from the live scan value. Expect
`stale_reason=baseline_mismatch_hard`.

## Miss recipe

Before scrolling for a miss, `playlist ls '<query>'` must first return
`match_count == 1` with `matches[0].label` exactly equal to the query. Only
then scroll the NetEase sidebar down 10+ pages so the target playlist is no
longer in the viewport, and run `playlist select` with the same label.

`QUERY` must be a label that `playlist ls` resolves **uniquely** in its JSON
output (`match_count == 1`). Short numeric labels (for example `"3"`) use
exact-first matching: substring-only hits such as `"43"` or `"13"` no longer
match. If `match_count > 1`, refine the query or use `playlist play
--candidate-id` with the `candidate_id` from `playlist ls --json` (`playlist
select` does not expose `--candidate-id` yet).

As of 2026-07-01 (A6c-8), single-character numeric playlist labels (`"3"`,
`"9"`) can still fail to reach `match_count == 1` on `ls`. This is a
**parse/OCR** blocker, not an exact-first query bug — see the A6c-8 status
row above and [`SIGNOFF.md`](SIGNOFF.md) Case B for the live evidence chain.
If `ls` still reports zero matches for a single digit after this fix, check
`obs-*-recognition.json` in the artifact dir for whether OCR produced the
digit at all before escalating.

## Redaction rules

- Do **not** commit real playlist names if sensitive — use placeholder labels that
  remain self-consistent across `ls` / `select` JSON and the view-memory file.
- Redact usernames from paths in `transcript.txt` (use `$HOME` or `/tmp/...`).
- Do **not** commit verification screenshot paths containing home directory names.
- Structure exemplars under `examples/` must carry `"proof_class": "structure_exemplar"`
  and must **not** be cited as live PASS evidence.

## Full bash protocol

```bash
export AUV_NETEASE_VIEW_MEMORY=1
ARTIFACT_DIR=/tmp/auv-scenebridge-a6-live-$(date +%Y%m%d)
mkdir -p "$ARTIFACT_DIR"
QUERY="<playlist-label-from-ls>"

# Shared: scan + write view-memory
cargo run -p auv-netease-music -- playlist ls "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-ls.json"

# Verify view-memory exists before Cases A–D
jq -e '.view_memory.written == true' "$ARTIFACT_DIR/case-ls.json"
test -f "$ARTIFACT_DIR/view-memory-playlist_sidebar.json"

# Case A — Hit (no scroll between ls and select)
cargo run -p auv-netease-music -- playlist select "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-a-select.json"

# Case B — Miss (scroll target away first, then select)
# ... manual scroll in NetEase UI ...
cargo run -p auv-netease-music -- playlist select "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-b-select.json"

# Case C — Stale (re-run ls, edit view-memory per recipe, then select)
# ... edit "$ARTIFACT_DIR/view-memory-playlist_sidebar.json" ...
cargo run -p auv-netease-music -- playlist select "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-c-select.json"

# Case D — Memory missing (re-run ls, delete view-memory, then select)
rm -f "$ARTIFACT_DIR/view-memory-playlist_sidebar.json"
cargo run -p auv-netease-music -- playlist select "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-d-select.json"

# Case E — Gate off
unset AUV_NETEASE_VIEW_MEMORY
cargo run -p auv-netease-music -- playlist ls "$QUERY" --json --artifact-dir "$ARTIFACT_DIR"
cargo run -p auv-netease-music -- playlist select "$QUERY" --json --artifact-dir "$ARTIFACT_DIR" \
  | tee "$ARTIFACT_DIR/case-e-select.json"
```

Copy redacted artifacts into this folder after owner review:

| File | Purpose |
| --- | --- |
| `transcript.txt` | Redacted commands and key stdout |
| `case-ls-a6c3-default-probe.json` | A6c-3 default-window post-fix probe |
| `case-ls-a6c3-resized-probe.json` | A6c-3 resized dedup write confirmation |
| `case-a-hit-select.json` | Case A full `PlaylistSelectResult` |
| `case-b-miss-select.json` | Case B (recommended) |
| `case-c-stale-select.json` | Case C |
| `case-d-missing-select.json` | Case D |
| `case-e-gate-off-select.json` | Case E |
| `case-ls-probe.json` | A6b blocker probe (`item_count=0`) |
| `case-ls-window-resized-probe.json` | A6c blocker probe (`match_count=1`, dirty scan, no ViewMemory file) |
| `view-memory-playlist_sidebar.json` | Post-`ls` snapshot |
| `view-memory-playlist_sidebar-probe.json` | A6b probe snapshot |
| `SIGNOFF.md` | Matrix checkboxes + environment |

`examples/` — optional **structure exemplars** only (`proof_class: structure_exemplar`).

## Anti-misread (live sign-off)

1. Reacquire optimizes **scroll replay only** — `resolve_playlist_target_for_query` still
   runs a live scan before select ([`playlist.rs`](../../../../../../crates/auv-netease-music/src/commands/playlist.rs) L362–418).
2. `reacquire.outcome` wire values are `reacquired` / `stale` / `not_found`.
3. Hermetic FakeAdapter JSON ≠ `proof_class: live` (A5 anti-misread #6).
4. Live evidence is CLI JSON + artifact-dir files only — no `view.reacquire.*` spans,
   no run-storage `view-memory` role (A5 Tier II–III).

## Sign-off checklist

- [x] Hermetic matrix green (`cargo test -p auv-view memory`, `playlist_select`, `region`, `write_from_scan_when_enabled`)
- [x] A6c-3 baseline + A/C/D/E live matrix on owner Mac (see `SIGNOFF.md`)
- [ ] Case B miss live PASS
- [ ] Owner approval to remove NOTICE / default-on feature gate (**not** in A6 scope)
