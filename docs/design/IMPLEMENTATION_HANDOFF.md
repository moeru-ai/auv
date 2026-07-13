# Implementation Handoff — AUV Design System

This document is a **cold-start spec** for the remaining design-system
implementation work. Any agent (Codex, Claude, human) should be able to
pick up an unfinished phase below without re-reading the prior session
context.

## Already shipped

| Phase | Commit | What landed |
|---|---|---|
| **A — overlay cursor** | `7f18b27` | `NativeOverlayCursorView.draw()` renders the pixel cyan+lime AUV cursor + cyan-strong (`#009ba6`) brand pill; default label `auv · replay`. Sprite ported from `assets/cursor-auv.svg`. |
| **B — vendor bundle** | `7b7061f` | Full upstream bundle vendored to `docs/design/` with `README.md` recording vendoring decisions + implementation status. |
| **C.1 — viewer shell + run list** | `3ae972b` | `GET /` on `auv-inspect-server` returns a vanilla HTML+CSS+JS viewer. Pixel logo top bar, 320px sidebar fetching `/runs`, status pills, run cards. |
| **C.2 — viewer span tree** | `4f0cbe0` | Run selection fetches `/runs/:id` + `/runs/:id/spans`; renders span sigils, statuses, durations, and timing bars. (Landed by Codex.) |
| **C.3a — viewer events rail** | `132ef3d` | 320px events rail below the span tree, fetching `/runs/:id/events`. Span-detail panel above the rail re-renders on row click. |
| **C.5 — viewer asset route (early)** | `e7726a4` | `GET /assets/:name` serves design-system SVGs from a compile-time map (path-traversal hardened, immutable cache). Inlined logo + sparkle migrated. |
| **C.3b — viewer artifact panel** | `a0b924a` | 340px right rail with artifact list + mime-routed preview (text/`<pre>`, image/`<img>`, else diagonal-stripe placeholder). Uses `/assets/icon-*.svg` + `/assets/sprite-inspector.svg`. |
| **C.4 — viewer WebSocket live stream** | `34eb07c` | When a `running` run is selected, the viewer opens `ws://host/runs/:id/stream` and handles `span_started` / `span_finished` / `event_appended` / `artifact_created` / `run_finished` frames; one 2 s reconnect on error, then `disconnected`. Streamed events get the `_live` tint reserved in C.3a. |

## Architecture decisions (do not relitigate)

These were settled in C.1. New phases should follow:

1. **Vite/Vue shell with a DOM-oriented viewer module.**
   The viewer source lives in `crates/auv-inspect-server/viewer/src/App.vue`,
   `src/viewer.ts`, and `src/styles/viewer.css`, then ships through
   the checked-in Vite build under `crates/auv-inspect-server/viewer/dist/`.
   The upstream JSX mocks in `ui_kits/viewer/*.jsx` are **prototypes
   to match visually**, not code to port directly. Preserve the existing
   DOM IDs/classes until the DOM module is deliberately split into typed
   Vue components.

2. **Design tokens are inlined.** The viewer's `:root` CSS block
   duplicates the relevant tokens from
   `docs/design/colors_and_type.css`. A regression test
   (`root_serves_inline_viewer_html` in `crates/auv-inspect-server/src/server.rs`)
   asserts `--brand: #00c4d2` is present so drift is caught.
   When adding tokens, copy from `colors_and_type.css` verbatim,
   keep the same names (`--brand`, `--validated`, etc.), and add a
   matching assertion if the token is new.

3. **Routes by purpose.** `GET /` returns the viewer payload;
   `GET /assets/:name` (added in C.5) serves design-system SVGs
   from a compile-time `include_bytes!` map keyed on
   `docs/design/assets/` filenames. To add a new asset, drop the
   SVG into that directory and add an entry to `DESIGN_ASSETS`
   in `crates/auv-inspect-server/src/server.rs`. The filename is the URL — keep
   them stable.

4. **The JSON contract is fixed.** Endpoints already exist:
   `/runs`, `/runs/:id`, `/runs/:id/spans`, `/runs/:id/events`,
   `/runs/:id/artifacts`, `/runs/:id/artifacts/:artifact_id`,
   `/runs/:id/stream` (WebSocket). JSON shapes are
   `RunRecordV1Alpha1` etc. in `src/trace.rs:118-173`. Do not
   change these to fit the UI; render against them as-is.

5. **Honest boundaries.** Match the AUV voice from
   `docs/ai/references/archive/phase-history/2026-05-18-phase-1-freeze.md`: when a feature
   isn't shipped yet, say so explicitly in the UI (e.g. C.1's
   placeholder reads "span tree, events, and artifact panel land in
   a follow-on commit"). Do not paper over.

## Source-of-truth mapping

When implementing any phase, the visual contract is the matching JSX
mock; the data contract is the matching Rust struct.

| Phase | Visual mock | Data shape | Endpoint |
|---|---|---|---|
| C.1 (done) | `docs/design/ui_kits/viewer/Sidebar.jsx` | `RunRecordV1Alpha1` | `GET /runs` |
| C.2 | `docs/design/ui_kits/viewer/SpanTree.jsx` + `Layout.jsx` (PaneHeader) | `RunRecordV1Alpha1` + `SpanRecordV1Alpha1` | `GET /runs/:id` + `GET /runs/:id/spans` |
| C.3a | `docs/design/ui_kits/viewer/EventsRail.jsx` | `EventRecordV1Alpha1` | `GET /runs/:id/events` |
| C.3b | `docs/design/ui_kits/viewer/ArtifactPanel.jsx` | `ArtifactRecordV1Alpha1` | `GET /runs/:id/artifacts` + `GET /runs/:id/artifacts/:id` for previews |
| C.4 | the pulsing `live` connection pill in the top bar (already styled in C.1) + the `live: true` event background tint in `EventsRail.jsx` | `RunStreamEvent` in `src/recording.rs` | `GET /runs/:id/stream` (WebSocket, text frames are serialized `RunStreamEvent`) |
| C.5 | — | n/a | new routes under `/assets/*` |

## Phase C.2 — span tree + run detail pane

> **Shipped in `4f0cbe0`** (by Codex). Section kept for historical
> context + future extension.

**Goal**: when the user clicks a run in the sidebar, replace the
placeholder with the span tree.

**Where to add code**:

1. Edit `crates/auv-inspect-server/viewer/src/App.vue`,
   `src/viewer.ts`, and `src/styles/viewer.css`. Rebuild the Vite
   output before validating the Rust server.
2. The `selectRun(runId)` function is the entry point. Today it
   updates the pane header and the placeholder. Replace the
   placeholder branch with `await loadRunDetail(runId)` that:
   - Fetches `/runs/:id` (run record — has `summary`, `status_code`,
     `state`, `started_at_millis`, `finished_at_millis`, `trace_id`,
     `run_type`).
   - Fetches `/runs/:id/spans` (array of `SpanRecordV1Alpha1`).
   - Renders into a new `<div class="main">` body region (currently
     a `.placeholder` div).

**Visual contract** (from `SpanTree.jsx`):

- Header row in `.pane-header` shows `Run · <run_id>` on the left,
  on the right a small mono crumb (`run_type · trace_id=<first 12>…`)
  and a status pill (reuse the same `.status-pill` CSS).
- Sticky table header inside the span tree: columns
  `span · name / step_id`, `status`, `dur`, `timing`.
  - Column widths: `0 0 300px`, `0 0 70px`, `0 0 70px`, `1 1 auto`.
  - 28px height, sticky `top: 0`, `background: var(--shell-2)`,
    border-bottom hairline.
- Each row: 7px vertical padding, 16px horizontal padding, mono
  12.5px text, hover/selected → `background: var(--shell-3)` and
  2px brand left border.
- Status sigil glyphs: `●` (ok / running), `×` (error), `○` (unset),
  `·` (none). Color = matching status token. Running sigil pulses via
  the existing `@keyframes auv-pulse` 1.2s linear.
- Indentation: `padding-left: depth * 16px` on the name column.
  Compute depth by walking `parent_span_id` chain to a root.
- Duration: `(finished_at_millis - started_at_millis) / 1000` to
  seconds with 2 decimals, or `—` when running.
- Timing bar: an 8px-tall track at `background: var(--shell-2)`,
  with a fill rect at `background: <status color>, opacity: 0.85`
  whose width = `(span_duration / max_duration) * 100%`. The
  upstream mock fakes the start offset (`indexOf * 5%`); compute
  the real offset as
  `(span.started - run.started) / (run.finished - run.started) * 100`
  if both ends are known, else stick to the cumulative-offset fake.

**Helper functions you'll want**:

- `depthOf(spans, span_id)` — recursive walk to root, memoized.
- `spanGlyph(span)` returning `{ glyph, color, pulse }`.
- `fmtSeconds(ms)` (2 decimals or `—`).

**Test to add** alongside `root_serves_inline_viewer_html`:

```rust
#[tokio::test]
async fn root_payload_includes_span_tree_markers() {
  // Smoke that the new SpanTree HTML hooks exist in the payload.
  // E.g. assert html.contains("span · name / step_id")
  // and html.contains("@keyframes auv-pulse") (already present).
}
```

Pure HTML assertion — no need to round-trip data.

**Out of scope for C.2**: events rail (C.3a), artifact panel
(C.3b), WebSocket (C.4).

## Phase C.3a — events rail

> **Shipped in `132ef3d`.** Section kept for historical context.

**Goal**: a 320px-tall horizontal rail below the span tree, showing
`events.jsonl` tail.

**Where**:

- Historical implementation note: this originally changed the single-file
  viewer. Current work should edit the Vite viewer source under
  `crates/auv-inspect-server/viewer/src/`, then rebuild `viewer/dist`.
  The `<main class="main">` is a vertical flex column with two children:
  the span tree (flex 1) and the events rail (flex `0 0 320px`).
- New `loadEvents(runId)` function fetching `/runs/:id/events`.

**Visual contract** (from `EventsRail.jsx`):

- Top sub-section: `SpanDetail` — when a span is selected,
  render its `name`, `span_id`, and a `key/value` grid of
  `attributes`. When nothing selected, the empty state pairs a
  `sparkle.svg` (24×24) with the line "Select a span to inspect its
  attributes." Current code references `/assets/sparkle.svg`; do not
  re-inline design SVGs.
- Pane header reading `Events · events.jsonl` with right-side count
  `<n> · tail`.
- Each event row: `grid-template-columns: 70px 160px 60px 1fr`,
  4px/20px padding, mono 12px, line-height 1.45.
  - Col 1: relative timestamp (compute `event.timestamp_millis -
    run.started_at_millis`, format as `+12.34s`).
  - Col 2: `event.name` — color by name substring:
    `failed` → `--failed`, `started`/`invoke` → `--brand-soft`,
    everything else → `--fg`.
  - Col 3: `span_id` (truncated to 8 chars).
  - Col 4: `event.message` or join of `attributes` if no message.
- Tint live events at `background: rgba(31, 125, 140, 0.08)` —
  C.4 will set the `live` flag from the WebSocket stream; for C.3a
  leave it always false (no tint).

**Span selection wiring**: span clicks in C.2's tree should set a
shared `state.activeSpanId`. When set, find the matching span and
pass it to `renderSpanDetail`. When unset, show the empty state.

## Phase C.3b — artifact panel

> **Shipped in `a0b924a`.** Uses `/assets/icon-*.svg` +
> `/assets/sprite-inspector.svg` (C.5 was pulled forward; see
> below). Note: the mock's `bytes` field is omitted from the
> metadata grid because the v1alpha1 `ArtifactRecord` doesn't
> carry it.

**Goal**: a 340px-wide right rail with artifact list + preview pane.

**Visual contract** (from `ArtifactPanel.jsx`):

- Same dark shell-2 column with hairline left border.
- Pane header `Artifacts · /artifacts` + count.
- Artifact rows: 10/12 padding, 28×28 mime-typed icon
  (`icon-png.svg` for `image/*`, `icon-json.svg` for
  `application/json`, `icon-bin.svg` otherwise — these three SVGs
  are in `docs/design/assets/`). Below the icon: role + filename
  basename in mono. Selected row gets the cyan brand left border
  and `--shell-3` background, same as run rows.
- Preview pane (bottom half):
  - Empty state: `sprite-inspector.svg` 96×112 + "Select an
    artifact to preview." + "<n> artifacts on this run".
  - Selected: 6-row metadata grid (`role`, `mime`, `path`, `sha256`,
    `bytes`, `span_id`).
  - Below metadata: a 220px content surface.
    - `application/json` → `<pre>` in mono with the actual artifact
      bytes (fetch via `/runs/:id/artifacts/:artifact_id`).
    - `image/*` → `<img>` tag with the same URL.
    - Anything else → a diagonal-stripe placeholder background
      (`repeating-linear-gradient(45deg, var(--shell-2) 0 12px,
      var(--shell-3) 12px 24px)`) with center caption
      `binary · <bytes>`.

**Historical asset note**: the original C.3b design considered inlining
`icon-png`, `icon-json`, `icon-bin`, and `sprite-inspector`. C.5 has since
landed, so current viewer work should reference `/assets/<name>.svg` and keep
the design asset map in `crates/auv-inspect-server/src/server.rs` in sync.

## Phase C.4 — WebSocket live streaming

> **Shipped in _this commit_.** Section kept for historical context.
> Handles `span_started` / `span_finished` / `event_appended` /
> `artifact_created` / `run_finished` per `RunStreamEvent` in
> `src/recording.rs:215`. Single 2 s retry on error, then
> `disconnected`. Streamed events get the `_live` tint reserved
> in C.3a.

**Goal**: when the selected run is `state == "running"`, open the
`/runs/:id/stream` WebSocket and append events live; pulse the
connection pill cyan.

**Where**:

- Same viewer source. Add a `connectStream(runId)` function that opens
  `new WebSocket(\`ws://\${location.host}/runs/\${runId}/stream\`)`.
- On `message`, parse JSON (each frame is a serialized
  `RunStreamEvent` — see `src/recording.rs`). Push the event into
  `state.events`, re-render the events rail, mark the row as live
  (`live: true`) so it gets the 8% running-cyan tint, mark the
  artifact row if `RunStreamEvent::ArtifactPersisted` arrives.
- Tear down the socket when selecting a different run.

**Connection pill**: it's already wired in C.1 — `setConnection(ok,
endpoint)` flips classes. C.4 should update the endpoint readout to
`ws://.../stream` while a stream is open, and back to `/runs` when
no stream is open.

**Reconnect policy**: on `onerror` / `onclose` of an active stream,
schedule a single retry after 2s; on the second failure flip to
`disconnected` and stop. Don't infinite-loop.

## Phase C.5 — extract assets to /assets/* route

> **Shipped in `e7726a4` (pulled forward to land before C.3b).**
> Hand-rolled `GET /assets/:name` against a compile-time
> `DESIGN_ASSETS` map (option 2 from the original recommendation).
> Path traversal hardened. Cache-Control: immutable, 1 year.

Only do this when total inlined-SVG bytes start to feel bloated, or
when the same asset is needed from multiple HTML payloads.

Two options:

1. **`tower-http::services::ServeDir`** mounted at `/assets` →
   serves files from `docs/design/assets/` directly. Adds the
   `tower-http` dependency. Smallest code.
2. **Hand-rolled** `GET /assets/:name` that matches a small
   compile-time map of `(filename, bytes, mime)` produced by
   `include_bytes!`. Zero new dependencies. More boilerplate.

When you do this, also remove inline SVG copies from
the viewer source and reference `/assets/<name>.svg`
instead.

## Common pitfalls

- **Run record duration**: a `running` run has
  `finished_at_millis: null`. Always guard against `null` before
  arithmetic.
- **Span ordering**: `/runs/:id/spans` returns spans in the order
  they were recorded, not topologically. Render in tree order
  (parent-first) by sorting by `started_at_millis` or by walking
  from the run's `root_span_id` and emitting children in
  start-time order. The mock `SpanTree.jsx` doesn't sort; the real
  data may have multiple roots if `parent_span_id` is `None` for
  more than one span.
- **WebSocket origin**: `location.host` (not `location.hostname`)
  carries the port, so `ws://${location.host}/...` is correct even
  on non-default ports.
- **Status name mismatch**: the Rust `TraceState` is `ended` /
  `running`; the JSX mocks use `running` / `unset`. Map both to
  the right pill class.

## How to verify a phase landed

For each new phase:

1. `cargo test -p auv-inspect-server` — every new viewer feature should have at least
   one assertion in `crates/auv-inspect-server/src/server.rs` that the payload
   contains a stable marker string (e.g. `"events.jsonl"` for C.3a).
2. End-to-end smoke:
   ```
   auv-cli inspect serve --port 18765 &
   curl -s http://127.0.0.1:18765/ | grep -F "<expected marker>"
   curl -s http://127.0.0.1:18765/runs | jq 'length'
   kill %1
   ```
3. Visual: open `http://127.0.0.1:18765/` in a real browser. The
   user has live runs in `.auv/runs/`; the sidebar should fill
   with them. Document any visual deltas in the commit body.

## Don't do

- Don't switch to React or another framework. The viewer is currently
  Vue/Vite; keep changes inside that toolchain unless the owner approves a
  migration.
- Don't edit generated `viewer/dist` by hand. Change `viewer/src/*`, run the
  Vite build, and commit the generated asset changes with the source change.
- Don't add emoji or icons from Lucide or any icon font. Use the
  `assets/` SVG sprites or status sigil glyphs only.
- Don't rename frozen contract fields (`cursorDisturbance`,
  `pressMechanism`, run/span/event v1alpha1 fields). The Phase 2
  freeze doc lists what's locked.
