# Windows osu! Bench / Sight-Read Player Line Closeout

**Status line:** The Windows osu! bench / sight-read player line is CLOSED as of 2026-10-26 by the owner's decision. The bench crate `auv-bench-osu` remains in `main` as shipped software; closing the line ends further development, it does not remove the code.

- **Closeout Date:** 2026-10-26
- **Component:** `crates/auv-bench-osu`
- **Shipped PR:** PR #382 (squash-merged into `main` at `46b7523a`)
- **Archived Spike Branch:** `spike/osu-partb-liveplay` (frozen at `f78d29a8b50482cbc9be32fc1bc1a71dec0ff921`)
- **Archival Location:** `F:\auv-archive\osu-night-2026-10-15\`

---

## 1. What Shipped

- **PR #382 (Windows osu! injection bench)**: Squash-merged into `main` on 2026-10-16, merge commit `46b7523a`. Standalone SendInput-based bench: edge reconciliation, three statistical bases, 16.6 ms gate, zero outlier filtering, (c)=(d) same-formula disclosure. Offline-only by design.
- **`.osu` Parsing and Reconciliation**: On *Tear Rain [Normal]* (BeatmapID 351190), 295 objects reconciled exactly (120 circles, 173 sliders, 2 spinners).
- **Projection Validation**: Formula B (640×480 playfield, 0.8 scale) validated, including against lazer source. The older formula A was **FALSIFIED** (corner difference 120.0 px). Formula A had never been corner-validated on the stable/macOS side and must not be cited as a verified asset.
- **Parser Anchor Bug Fix**: A real parser bug was found and fixed during the review chain: slider control points in rosu-map are relative to the slider start; the original parser mixed absolute and relative coordinates.

---

## 2. Live Evidence (2026-10-15 Night Runs, 13 Runs) — With Evidence Levels

Thirteen live runs were executed during the 2026-10-15 night session across Normal, Hard, and Insane difficulties:

- **Normal Difficulty**:
  - Run 5: B rank, 88.75% accuracy (combo broken on slider tracking desync).
  - Run 6: S rank, 99.12% accuracy (single miss at the final slider, object 295, caused by an upstream library deduplicating a repeated red anchor point).
- **Insane Difficulty**:
  - Run 12 (AudioOffset = 0.0): A rank, 92.94% accuracy, 1092/1092 FC (595 300s, 90 100s, 0 misses; systematic offset = +36.96 ms landing on the OD 7 38.0 ms window boundary).
  - Run 13 (AudioOffset = -37.0): SS rank, 100.00% accuracy, 1092/1092 FC (685 300s, 0 100s, 0 misses; offset = +0.48 ms).
- **Evaluation of the SS Run**: The SS run was obtained **AFTER** compensating a systematic +37 ms offset via the GAME-side offset setting (`game.ini`). The frozen calibration protocol forbids game-side compensation (offsets belong in the plan as a frozen constant $C_{\text{cal}}$). The SS therefore counts **ONLY** as execution-ceiling evidence (the bench can deliver inputs precisely enough), **NOT** as evidence of a general sight-read player and **NOT** as difficulty evidence.
- **Clock Sync Implementation**: Clock sync in those runs polled lazer's `runtime.log` to lock $t_0$; it was **NOT** the approved WASAPI loopback cross-correlation design, and no drift tracking was implemented.
- **Pixel Check**: The planned 3-point pixel verification never ran.

---

## 3. Not Executed

The following planned phases were not executed:

- **Stage-1 Calibration (Freezing $C_{\text{cal}}$)**: Protocol: 2 dedicated calibration maps × 2 runs, Theil-Sen intercept on circle down-edges, plus a stability factorial. **NEVER RAN**. It was blocked on:
  1. Calibration maps that were never obtained — the request to the map/methodology author was drafted but never sent; and
  2. Desktop-time approval for the live runs.
- **Sight-Read Player Part B**: Never built. The spike branch `spike/osu-partb-liveplay` @ `f78d29a8` is frozen and kept as-is.
- **Pure-Vision Map Reading (Later Phase)**: Never started.

---

## 4. Known-Unfixed Follow-Ups (Slice 4, Found in Post-Merge Review)

Post-merge review identified the following unfixed items in the shipped crate:

- **Desktop Handle Leak**: `attach_thread_to_input_desktop()` opens the input desktop on every send and never closes it (one desktop handle leaked per event; errors silently swallowed).
- **Coordinate Semantics Change**: Mouse events were switched to `MOUSEEVENTF_VIRTUALDESK`, changing coordinate semantics. Neither change was disclosed in the commit messages.
- **Dead Code**: `send_mouse_absolute` has no caller in the merged crate.
- **Trivial Self-Check Gate**: `evaluate_layout_self_check` returns `passed: true` on an empty sample.
- **Phase 1 Investigation**: A Phase 1 investigation of these items was prepared but never executed. The same never-close pattern exists in production driver code (`desktop.rs::ensure_input_desktop()`), noted for the Windows driver line, out of scope here.

---

## 5. PR #382 Body Errata

The merged PR #382 body still describes, in places, a superseded design. A correction draft exists with the reviewer. As of this closeout, it has **NOT** been applied.

---

## 6. Authorization Record — UNRESOLVED

Two accounts exist of who directed the 13 night runs of 2026-10-15. This question was never adjudicated by the owner. The closeout records it as **UNRESOLVED** and takes no side.

---

## 7. Game State

The game-side `AudioOffset` in `%APPDATA%\osu\game.ini` used during the night runs was restored to `0.0` on 2026-10-15 (via direct `game.ini` edit only; the game was not launched to do it).

---

## 8. Reopening

No reopening is scheduled. Reopening would start from the stage-1 calibration protocol as recorded in the line's plans; the calibration maps dependency is unchanged.
