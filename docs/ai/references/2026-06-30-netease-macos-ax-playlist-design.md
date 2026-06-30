# NetEase Music (macOS) — AX-sourced sidebar/playlist design

> Status: **Provisional design.** Grounded in a read-only AX exploration spike run
> 2026-06-30; no production code changed. Names marked _(provisional)_ are not yet
> stabilized. This design overlaps live Collabi intents — read
> [Coordination](#coordination) before implementing.

## Context

The macOS `auv-netease-music` playlist surface (`playlist ls|select|play`) reads the
sidebar by screenshot + OCR + scroll-scan (`src/view_parsers/sidebar/`), and reads
now-playing by OCR + control classification (`src/commands/playback.rs`). OCR matching
is fuzzy, and the scan has "no page completion model" — it "stops at section landmarks
or scroll boundaries" (`src/view_parsers/sidebar/live.rs:387`).

NetEase Music macOS (`com.netease.163music`) is a **WebKit shell** (`AXWebArea`; no
Electron/CEF framework bundled). Its accessibility tree is **lazy/gated**: by default
`capture_ax_tree_snapshot` returns ~2 nodes; after `AXEnhancedUserInterface=true` is set
on the application element it exposes ~1000+ typed nodes. This is the macOS analog of the
**already-accepted Windows contract** where NetEase's CEF tree is empty to UIA "unless
Chromium renderer accessibility is enabled at process start"
(`src/commands/launch.rs:95`, `--force-renderer-accessibility`). Windows = CEF/Chromium;
macOS = WebKit. Two engines, same "force embedded-web a11y before automation" idea.

## Spike evidence (2026-06-30, verified)

- Flag off → 2 nodes (`AXWindow` → one opaque `AXGroup`). Flag on → 856 then 1092 nodes.
- Typed content: playlist names as `AXStaticText`, controls as `AXButton`, search box as
  `AXTextField`, plus exact section totals `创建的歌单 201` / `收藏的歌单 395`.
- Enumeration via window-targeted-wheel scroll + AX capture + dedup **by text value**:
  20 → **751 distinct names**, stable plateau after ~27 rounds.
- `AXManualAccessibility` (the Chromium flag) is rejected — wrong engine.
- **No `AXSlider`**; the bottom now-playing / transport bar is absent from the tree.
- Positional AX paths recycle under virtualization: a fixed path prefix froze at 124
  while value-dedup reached 751.
- Exploration-only tools (scratch, deletable): `crates/auv-driver-macos/examples/ax_dump.rs`,
  `crates/auv-driver-macos/examples/netease_ax_sidebar_enum.rs`.

## What we reuse (do not reinvent)

| Need | Reuse | Location |
|---|---|---|
| Playlist result shape + JSON + match filter | `PlaylistSidebarScan`, `MatchRef`, `PlaylistJsonOutput`, `collect_matches` | `lib.rs:327`, `output.rs:8/21/50` |
| Scroll + scan loop, query-stop, settle | sidebar observer + `scan_sidebar_with_observer_until_query` | `src/view_parsers/sidebar/live.rs` |
| AX capture | `capture_ax_tree_snapshot(app, depth, children)` | `auv-driver-macos/src/native/ax_tree.rs:36` |
| AX press (open/play a row) | `perform_ax_path_action(pid, path, role, "AXPress")` | `native/ax_tree.rs:60` |
| App-level set-attribute **pattern** | mirror `set_ax_focused` (FFI + Swift `AXUIElementSetAttributeValue` on `AXUIElementCreateApplication(pid)`) | `native/binding.rs:366`, `AxTree.swift:342/374/419` |
| Dead AX hook to revive or replace | `capture_ax_scrollbar_boundary` | `src/view_parsers/sidebar/live.rs:663` |

## Proposed design

1. **New driver capability** `set_app_enhanced_user_interface(pid, enabled)` — **IMPLEMENTED**
   on branch `feat/auv-driver-macos-ax-enhanced-ui` (sibling of `set_ax_focused_path` in
   `native/ax_tree.rs`; FFI in `binding.rs` reusing `NativeActionResponse`; Swift in
   `AxTree.swift`). **Verified gotcha:** `AXUIElementSetAttributeValue(…, "AXEnhancedUserInterface", …)`
   returns **-25208 `kAXErrorNotImplemented` on every app** (Finder/Music/NetEase) yet the
   write still applies — so the Swift confirms by **read-back**, not the return code (the
   Electron #38102 `AXManualAccessibility` pattern). This is the macOS half of the existing
   Windows force-a11y contract. Regenerate swift-bridge after FFI changes
   (`scripts/generate-swift-bridge`). Open: the flag latches once a11y is active, so a clean
   cold-start test (app restart) is still pending; restore-of-prior-value is deferred
   (`TODO(netease-ax-get-attr)`).
2. **AX sidebar reader** — inside the existing observer, when on macOS and the flag is
   enabled, extract sidebar rows from the AX snapshot (`AXStaticText` under the
   created/collected groups) instead of OCR, **keying on text value** (paths are not
   stable). Emit into the existing `PlaylistSidebarScan` so `collect_matches`, the JSON
   output, `playlist select`, and `playlist play` are unchanged.
3. **Completion oracle** — use the AX-reported totals (`创建的歌单 N` / `收藏的歌单 M`) to
   fill the "no page completion model" gap (`live.rs:387`): stop when distinct-collected ≥
   total (or on plateau). This is exactly what the current scan lacks.
4. **Actions** — `playlist play <query>` opens/plays by `AXPress` on the matched row
   element. AX bounds mix coordinate spaces (sidebar ≈ screen points, web content ≈ 2×
   document pixels), so element-targeted `AXPress` is preferred over coordinate clicks.

## Constraints & non-goals

- **Prerequisite**: the flag must be set before any AX read, or the tree is 2 nodes. The
  existing `capture_ax_scrollbar_boundary` (`live.rs:663`) is inert today because nothing
  sets it (works only when VoiceOver is already running).
  `// TODO(netease-ax-enable): AX read path is dead until the enable-capability lands; trigger = capability merged.`
- **Transport / seek / volume are out of scope** for the AX path: no `AXSlider`, no
  bottom-bar nodes.
  `// NOTICE(netease-ax-transport): AX evolves reads + playlist-nav only; transport stays on its current path until a play-state probe shows otherwise.`
- **Collapsed sections**: created/collected sidebar sections lazy-render; full
  enumeration needs scroll (and possibly the `更多` expander). The totals oracle makes the
  "are we done" decision exact instead of guessed.
- Keep `auv-overlay-macos` out of this — it is visual-only, not an input/read backend.

## Coordination

Live Collabi intents touch the same files (claims = 0, soft overlap):
`auv-netease-scroll-ax-corroboration` and `auv-netease-scroll-completion-boundary` on
`lib.rs`, plus sidebar `region.rs` clamp work. The completion oracle and the revived
`capture_ax_scrollbar_boundary` directly overlap that work, and no committed plan for
those intents exists in references as of 2026-06-30. **Dedupe with that owner before
editing `lib.rs` / `src/view_parsers/sidebar/`.**

## Deferred (intentional)

- `// TODO(netease-ax-ls-full): playlist ls 201/395 reconciliation — spike proved the mechanism (751 distinct, plateau) but did not classify captured names into created vs collected vs discover noise; value-keyed classifier is a follow-up, not a blocker for select/play.`
- `// TODO(netease-ax-get-attr): reading AXEnhancedUserInterface to skip redundant sets is omitted; trigger = if idempotent set proves costly.`
- `// TODO(netease-ax-transport): re-probe while a track is actively playing to confirm the bottom bar stays absent; trigger = owner approves a transport-evolution slice.`

## Validation done in the spike

- `cargo run -p auv-driver-macos --example ax_dump` and `--example netease_ax_sidebar_enum`
  → exit 0 on macOS.
- Live captures + enumeration curve recorded. App flag restored to `false` after each run.
