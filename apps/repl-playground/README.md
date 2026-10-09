# AUV REPL Playground

An interactive, steppable TypeScript REPL for [AUV](https://github.com/moeru-ai/auv).
Write automation scripts against `auv`, the `@auv-js/sdk` Runner client, run
them all at once or line by line,
pause on breakpoints, and see what every step saw and did on a canvas of the
controlled desktop.

> Status: experimental prototype. Names, APIs and layout are provisional.

## Features

- **REPL editor** (CodeMirror 6): `⌘↩` runs all or the selection, `⇧↩` runs the
  current line and moves down, `⌘⇧↩` runs and pauses at the first statement.
  Top-level declarations persist across runs until **Reset**.
- **Debug bar** floating over the editor (drag it anywhere): run, step in,
  offline replay, continue (`F8`), step (`F10`), stop (`⇧F5`). Click the gutter
  to toggle breakpoints.
- **Event log + time cursor**: every step, binding, call and log gets a `seq`.
  The thin time strip at the bottom shows step ticks (colored by line) and call
  spans (violet = read, rose = input); click or drag it to travel in time. The
  canvas, editor (purple line), variables and call list all show the state at
  the cursor. **Follow latest** returns to live.
- **History-aware hover**: for a name, one tick per binding (`#1 #2 #3` for
  loop iterations); for a window handle, its captures over time; for the line,
  each call. Hover a tick to preview, click it to move the time cursor.
- **Pins**: the pin button on any hover section (or the inspector) turns that
  moment into a floating, draggable card, so several moments can be compared
  side by side. Cards are fixed to their `seq`.
- **Per-line timing**: `Σ` inclusive time on multi-line statements, self time ×
  hits inside loops and functions.
- **Inspector** (right): the selected handle with its lineage (produced by,
  bound to, used by — each jumps the time cursor). Lineage follows window,
  display and capture IDs in SDK values and requests. Also an **AX tree** panel
  (DevTools-style; hover highlights the element on the canvas, clicking the
  canvas selects the element under the pointer).
- **Offline replay**: every live run records its `auv` calls as encoded
  messages. Replay re-runs the (possibly edited) script
  against that recording without touching the device, and stops with a
  divergence error at the first call that differs.
- **Mock desktop**: a deterministic in-browser desktop (todo app, counter, and
  a music app with a 40-song list that scrolls and plays on click) whose state
  changes on input, so everything works without a device. It is a mock Runner
  (`createMockTransport` from `@auv-js/sdk`): it answers the same Runner RPCs
  as a device, so scripts, the timeline and replay behave the same on it.

## Quick start

From the repository root:

```sh
pnpm install
pnpm generate:proto                  # generates the SDK's protobuf bindings
pnpm -F @auv-js/repl-playground dev  # http://localhost:5180/playground/
```

The REPL runs against the SDK source in this repository (`js/packages/sdk`,
aliased in `vite.config.ts` and `tsconfig.json`), so it needs no SDK build and
always matches the daemon built from the same checkout.

The REPL starts on the mock desktop. The device indicator in the status bar's
left corner shows the active Device; click it to open the device picker. Several
daemons can stay connected at once: pick any Device they report (or the mock
desktop) to make it the one scripts, the canvas and live view use. Switching is
disabled while a run is in progress. To control a real Device, choose
**Add a device…** and pick one of two modes:

- **Local · no pairing** (same machine, development): a loopback listener
  without a pairing store accepts local connections without a credential.

  ```sh
  auv serve --listen http://127.0.0.1:9847
  ```

  Then press **Connect**.

- **Paired**: start the daemon with a pairing store, so its HTTP listener
  requires a paired Device credential, and create a one-time token on the
  daemon host:

  ```sh
  auv serve --listen http://127.0.0.1:9847 --pairing-store ~/.auv/pairings.json
  auv devices pair create-token
  ```

  Paste the token and press **Pair**. The browser keeps the credential and
  reconnects with it on the next visit.

Daemons that were connected when the page closed reconnect on the next visit,
and the previously active Device is restored. **Disconnect** keeps a daemon in
the picker; **Forget** also drops its credential.

Without `--pairing-store`, `auv devices pair create-token` fails with
`pairing is not configured`; with it, unpaired HTTP requests are rejected.

## Script API

`auv` is the [`@auv-js/sdk`](../../js/packages/sdk/README.md) Runner client for
the selected device and the current Run, injected before every run. It is the
same object a Node script gets from
`createAuv(await connect(...)).runner(route)`, so every Runner RPC is
available and scripts move between the playground and Node unchanged. `sdk` is
the SDK module (`sdk.InputPolicy`, `sdk.Position`, `sdk.below`, …). The
playground adds editor helpers: `area()`, `draw()`, `focus()`, `show()`,
`centerOf()` and `sleep()`
([`src/script-api/api.ts`](src/script-api/api.ts)).

```ts
const counter = await auv.windows.resolve({ appName: 'Counter' })
const hits = await counter.findText('Increment') // match bounds are screen space
await counter.click(hits.matches[0]!) // a text match clicks at its center
const shot = await counter.capture() // a CaptureRef and metadata; pixels stay on the Runner
show((await auv.recognizeText(shot.capture!)).text, 'after')
```

On a window, `click({ x, y })` is window-local; a text match, an area or
anything with screen `bounds` clicks at its center; `sdk.Position.screen(x, y)`
names screen space. `auv.input.click(point)` is a global screen click.

Every call is recorded in the timeline under its API name (`windows.findText`)
with its ProtoJSON request and response. Results are drawn by message type:
captures as frames, text matches and recognized text as boxes, input results as
receipts at the delivered point, windows and displays as outlines. Lineage
follows window, display and capture IDs, so hovering or pinning a bound value
shows its resource. The editor types `auv` and `sdk` as `any`
(`TODO(playground-sdk-types)`); see
`docs/ai/references/inspect/2026-10-08-playground-sdk-transport-design.md`.

### Areas and overlays

`area()` builds screen-space rectangles relative to a window (an SDK window
client or `Window`), display, capture, text match or another area. Areas are
plain values computed in the script; they never call the device. Their geometry
is the SDK's (`sdk.region`, `sdk.below`, … with the same rules as Rust's
`Rect`); `area()` adds chaining and a canvas label. An area clicks and scrolls
at its center.

```ts
const music = await auv.windows.resolve({ bundleId: 'com.netease.163music' })
const search = area(music).region({ height: 36, left: 400, top: 32, width: 256 }).named('search')
const results = area(music).region({ bottom: 80, left: '22%', top: 120 }) // % follows resizes
draw(search) // editor only: outline it on the desktop canvas
await music.click(search) // clicks the center
draw(search.below(40, 8), 'hint')
```

`region()` takes two of `left`/`right`/`width` per axis (and of
`top`/`bottom`/`height`); a missing start edge is 0 and a missing size fills
the rest. `draw()` marks are part of the run's event log, so time travel shows
only the marks drawn up to the cursor; they are not device calls and are not
recorded for replay.

OCR can be limited to an area with `screenRegion`. AUV reads only that part of
the image; match bounds stay in screen space.

```ts
const sidebar = area(music).region({ width: '30%' })
const hits = await music.findText('Remember', { screenRegion: sidebar })
const ocr = await auv.recognizeText((await music.capture()).capture!, { screenRegion: sidebar })
await auv.macos.applications.activateBundleId({ bundleId: 'com.netease.163music' })
```

`focus(target, { zoom = true, autoZoomOut = true })` eases the desktop canvas
camera onto a window, area, text match, capture or point. Long jumps follow a
smooth zoom-and-pan arc (zoom out mid-way, descend onto the target; van Wijk &
Nuij), sized by distance and squeezed into the time until the next focus on the
timeline. Like `draw()`, focuses are editor-only events in the run's log, so
scrubbing the timeline replays the camera; dragging or zooming the canvas
yourself interrupts a flight.

```ts
focus(todos) // fly to a window
focus((await counter.findText('Increment')).matches[0]!) // then to a match, arcing over the gap
focus(search, { zoom: false }) // pan only, keep the zoom level
```

### Scrolling

`window.scroll(target, { deltaX?, deltaY? })` wheel-scrolls once and
`window.scrollUntil(target, options, { until })` scrolls in steps, observing
after each step on the Runner (`InputService/ScrollWindowPoint` and
`ScrollUntil`). Positive `deltaY` scrolls toward later content (down).

```ts
const results = area(music).region({ bottom: 80, left: '22%', top: 120 })
await music.scroll(results, { deltaY: 600 })
const found = await music.scrollUntil(results, {
  condition: { case: 'textVisible', value: { query: 'Remember' } },
  step: { case: 'instant', value: { deltaY: 400 } },
}, {
  until: update => update.text?.text.includes('Reply'), // runs in the script for every update
})
// found.reason is a ScrollUntilStopReason; found.textMatch is screen space
```

Omitted `maxSteps`, `noMotionConfirmations` and `settle` take the
`auv invoke input.scrollUntil` defaults (50 steps, 2 confirmations, 400 ms;
`sdk.SCROLL_UNTIL_DEFAULTS`).

An `END_BY_NO_VISUAL_PROGRESS` stop means no visual motion was observed, not
that no content is left. On the mock desktop, the Music window's song list
scrolls (other windows do not):

```ts
const music = await auv.windows.resolve({ appName: 'Music' })
const songs = area(music).region({ height: 420, left: 16, top: 82, width: 448 })
const found = await music.scrollUntil(songs, {
  condition: { case: 'textVisible', value: { query: 'Remember' } },
  step: { case: 'instant', value: { deltaY: 240 } },
})
await music.click(found.textMatch!, { click: { count: 2, interval: { nanos: 80_000_000 } } })
await music.findText('Now playing: Remember', { screenRegion: area(music).region({ bottom: 0, height: 70 }) })
```

### Typing

`window.typeText(text)` and `window.pressKeys('cmd+a')` deliver to that window
(`InputService/InputKeyboard`): by default the window is brought to the front
and focused first, so the text cannot land in another app.
`auv.input.typeText` and `auv.input.pressKey` go to whichever app has keyboard
focus when they run, which can be this playground's browser.

```ts
const music = await auv.windows.resolve({ bundleId: 'com.netease.163music' })
await music.click({ x: 528, y: 50 }) // the search box, window-relative
await music.typeText('Reply 超时空辉夜姬')
await music.pressKeys('return')
await music.pressKeys('cmd+a', { policy: sdk.InputPolicy.BACKGROUND_ONLY }) // the box must already have focus
```

`BACKGROUND_ONLY` posts without activating the window. The control must
already have keyboard focus; a click does not give it focus in every app (it did
not in NetEase Cloud Music while another app was in front).

### Previews and types

Captures carry a ThumbHash (`CapturedFrame.thumbhash`): frames, and the first
live frame after connecting to a device, show a blurred preview at once and fade
the pixels in when they load.

Playground helper types (`Area`, `AreaEdges`, `Rect`, `Point`, …) can be used
by name in cells, e.g. `function toolbar(win: Area): Area`.

## Architecture

```
main thread                           language worker               exec worker
───────────                           ───────────────               ───────────
CodeMirror ── hover/lint/complete ──▶ TS 6 language service
           ── compile cell ─────────▶ ts-blank-space + acorn
                                      + magic-string (stepper) ──▶ instrumented JS
session ◀── onStep/onPause/onVars/onLog ─────────────────────────── await __step(id)
sdk-bridge ◀── encoded RPCs (auv = @auv-js/sdk Runner client) ──── createBridgeTransport
           ─▶ Backend transport (a daemon, the mock Runner, or a replay)
store (zustand) ─▶ canvas, timeline, inspector, editor decorations
```

- `src/stepper` compiles one cell: strips erasable TypeScript while keeping
  offsets, inserts `await __step(id)` before statements (record-only
  `__stepSync` in sync functions), rewrites top-level declarations into
  prelude-declared globals for REPL persistence, and returns the last
  expression. `StepTimer` attributes time between hooks to lines, excluding
  pauses. Both are unit tested.
- The exec worker runs cells with indirect eval and pauses by awaiting a
  host-resolved promise (birpc). Its `auv` client sends encoded RPCs to the
  page on their own port (`runtime/sdk-bridge.ts`); the page adds the device
  credential, forwards them to the backend's transport and records them.
- `runtime/rpc-resources.ts` turns recorded SDK messages into host-side
  resources (frames, text, receipts, windows, displays) by protobuf type.
- `src/components` holds store-agnostic UI building blocks (icon buttons, tab
  bars, panel frames). `src/features/*` groups the workbench by area:
  `device` (connection registry and picker), `editor`, `desktop` (canvas and
  camera), `inspect` (values, handles, previews, pins), `run` (calls, console,
  time strip) and `workbench` (activity and status bars).
- Icons are UnoCSS `presetIcons` classes from Iconify Phosphor
  (`i-ph-*`, regular weight).

## Deliberate gaps

These are known and intentionally deferred; each is marked in code.

- **Captures are fetched as logical-resolution JPEGs.** Frames are AUV
  capture references; OCR reads the full-resolution capture in the Runner,
  but the canvas and magnifiers show only the logical-resolution image. A
  reference expires in the Runner (memory budget, ten idle minutes); OCR on an
  expired frame fails with NOT_FOUND and needs a new capture. Replay keeps the
  images it loaded during the recorded run.
- **Streams** such as `scrollUntil` show their resources when they end, not
  step by step (`TODO(playground-sdk-stream-progress)`).
- **AX tree and object detection** have no AUV driver RPC yet; only the mock
  desktop provides an AX tree. The AX panel shows the current tree, not the
  tree at the time cursor (that needs AX snapshots recorded per step).
- **The event log is browser-only.** Its shape (seq, steps, binds, calls with
  effect, lineage) is meant to converge with AUV Run traces so recorded runs
  can be persisted and inspected by AUV; that read side does not exist yet.
- **Lineage follows IDs only**: text results and input receipts have no Runner
  ID, and `centerOf(match.bounds)` yields numbers, so a click point does not
  link back to the OCR match it came from.
- **Replay determinism**: `Date.now()`, `Math.random()` and `sleep()` are not
  recorded. Replay matches each call by method and request bytes.
- **Stepping** goes into called async functions (no step-over), sync functions
  are record-only, and a sync infinite loop can only be escaped with **Reset**
  (which clears REPL state). A user `try/catch` can swallow a stop request.
- **Breakpoints are line numbers**; they do not move with edits.
- **Not a sandbox.** The exec worker shares the page origin, and the paired
  Device credentials are stored in `localStorage`. Run only your own scripts and
  keep the REPL on a local origin.
- `const` at the top level becomes reassignable across cells (same as
  `node:repl`), and a line starting with `(` or `[` continues the previous
  statement unless it ends with `;`.

## Development

```sh
pnpm -F @auv-js/repl-playground lint      # oxlint + eslint via moeru-lint
pnpm -F @auv-js/repl-playground typecheck
pnpm -F @auv-js/repl-playground test:run  # stepper, area, camera, replay, resource and store tests
pnpm -F @auv-js/repl-playground build
```

The root `pnpm lint`, `pnpm typecheck` and `pnpm test:run` include this package.
The root ESLint config ignores `devtools/**`, so this package keeps its own
config (React and UnoCSS rules).
