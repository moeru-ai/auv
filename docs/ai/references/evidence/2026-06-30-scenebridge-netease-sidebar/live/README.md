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
- Current live status (2026-07-01 @ A6 closeout):
  - **Closed:** default-window geometry (`case-ls-a6c3-default-probe.json`: `height≈286`, `item_count≥1`).
  - **Closed:** dedup-only ViewMemory write (`case-ls-a6c3-resized-probe.json` + default probe).
  - **Closed (hermetic):** `query_match` exact-first + `query_resolution` JSON (`A6c-10a`).
  - **Closed (hermetic @ A6c-9):** ViewMemory write gate for paired `sidebar_region_fallback_used`.
  - **Closed (live @ A6c-10b):** `playlist ls '3'` → `query_resolution=unique_exact` @ 2338
    (`/tmp/auv-a6c13-case-b-20260701-2338/case-ls.json`); skip top rewind when query visible;
    audit via `query_scan_skipped_top_rewind` / `query_scan_top_rewind_applied`.
  - **Closed (live @ A6c-12):** single-digit `select` verification via OCR ladder + sidebar row
    echo (`sidebar_row_echo_detail_chrome_v1`); confirmed on Case B replay path @ 2338.
  - **Closed (live @ A6c-13):** `playlist select <query>` uses scan-cache target resolve when
    gated; Case B **PASS** @ 2338 (`not_found` + rescan replay + verification passed) —
    [`case-b-miss-select.json`](case-b-miss-select.json). `playlist play <query>` shares the
    same target-resolve fast path, but the matrix command is **`playlist select <query>`**, not
    `play --candidate-id`.
  - **PASS (scoped):** Cases A–E live matrix complete @ `AUV_NETEASE_VIEW_MEMORY=1`. See
    [`SIGNOFF.md`](SIGNOFF.md). Gate default-on and NOTICE removal remain explicit non-goals.

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
| **B Miss** | gate=1; `ls` then **A6c-13+** manually scroll target off-screen; **`playlist select <query>`** (not `play --candidate-id`) | `reacquire.outcome=not_found`; `skipped_rescan_replay=false`; `known_limits` has `playlist_select_target_from_scan_cache_v1` | miss fallback text; **has** `scroll-sidebar-top-*` + `reobserve-playlist-after-rescan-replay` |
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

### A6c-13 prerequisite (Case B only)

### Validated @ 2338 (Case B PASS)

Prerequisite: A6c-13 scan-cache target resolve (audit marker below). Historical note:
before A6c-13, pre-select live scan erased manual scroll-off (2309 bisect).

```bash
jq -e '
  .reacquire.outcome == "not_found" and
  .reacquire.skipped_rescan_replay == false and
  (.known_limits | index("playlist_select_target_from_scan_cache_v1")) and
  (.known_limits[] | select(test("reacquire missed target"))) and
  ([.steps[].name] | any(test("^scroll-sidebar-top-"))) and
  ([.steps[].name] | index("reobserve-playlist-after-rescan-replay")) and
  .verification.status == "passed" and
  (([.diagnostics[].code] | index("playlist_select_rescan_reobserve_missed_target")) | not)
' case-b-miss-select.json
```

`play --candidate-id` skips pre-scan by design but is **not** the Case B acceptance command.

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

Single-digit labels (`"3"`) required A6c-10b top-rewind skip + probe OCR path before
Case B; validated @ 2338. If `ls` regresses to `ambiguous`, check `obs-*-recognition.json`
and `known_limits` for `query_scan_*` before re-running Case B.

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
jq -e '.query_resolution == "unique_exact"' "$ARTIFACT_DIR/case-ls.json"
jq -e '.match_count == 1' "$ARTIFACT_DIR/case-ls.json"
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

1. When `AUV_NETEASE_VIEW_MEMORY=1` and `playlist-scan-cache.json` has a unique
   `select_target`, `playlist select <query>` skips pre-resolve live scan (A6c-13);
   otherwise `resolve_playlist_target_for_query` still runs `run_live_scan_until_query`.
   Reacquire optimizes **scroll replay** after target resolve.
2. `reacquire.outcome` wire values are `reacquired` / `stale` / `not_found`.
3. Hermetic FakeAdapter JSON ≠ `proof_class: live` (A5 anti-misread #6).
4. Live evidence is CLI JSON + artifact-dir files only — no `view.reacquire.*` spans,
   no run-storage `view-memory` role (A5 Tier II–III).
5. **A6c-13:** target resolve from scan cache does **not** require view-memory file;
   memory is consumed at reacquire only. `playlist play <query>` shares this fast path, but the
   Case B matrix uses `playlist select <query>`, not `play --candidate-id`.
6. **PASS (scoped) != default-on** — gate remains off unless owner approves a rollout slice.
7. **PASS (scoped) != NOTICE removed** — A3e deferral unchanged.

## Sign-off checklist

- [x] Hermetic matrix green (`cargo test -p auv-view memory`, `playlist_select`, `region`, `write_from_scan_when_enabled`)
- [x] A6c-3 baseline + Cases A–E live matrix on owner Mac (see `SIGNOFF.md`)
- [x] Case B miss live PASS @ 2338
- [ ] Owner approval to remove NOTICE / default-on feature gate (**not** in A6 scope)
