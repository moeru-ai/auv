# AUV REPL Playground

An interactive, steppable TypeScript REPL for [AUV](https://github.com/moeru-ai/auv).
Write automation scripts against `auv.*`, run them all at once or line by line,
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
  bound to, used by — each jumps the time cursor), and an **AX tree** panel
  (DevTools-style; hover highlights the element on the canvas, clicking the
  canvas selects the element under the pointer).
- **Offline replay**: every live run records its device calls, including direct
  SDK calls as encoded messages. Replay re-runs the (possibly edited) script
  against that recording without touching the device, and stops with a
  divergence error at the first call that differs.
- **Mock desktop**: a deterministic in-browser desktop (todo app, counter, and
  a music app with a 40-song list that scrolls and plays on click) whose state
  changes on input, so everything works without a device.

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

The full declaration is [`src/script-api/api.ts`](src/script-api/api.ts);
it is also what the editor uses for types and hover.

```ts
const counter = await auv.windows.resolve({ appName: 'Counter' })
const hits = await counter.findText('Increment') // TextHandle, bounds in screen space
await auv.input.click(centerOf(hits.matches[0]!.bounds))
const shot = await counter.capture() // FrameHandle (pixels stay on the host)
show((await auv.text.recognize(shot)).text, 'after')
```

### Areas and overlays

`area()` builds screen-space rectangles relative to a window, display, frame,
OCR match or another area. Areas are plain values computed in the script; they
never call the device. `auv.input.click` accepts an area (or any rectangle) and
clicks its center.

```ts
const music = await auv.windows.resolve({ bundleId: 'com.netease.163music' })
const search = area(music).region({ height: 36, left: 400, top: 32, width: 256 }).named('search')
const results = area(music).region({ bottom: 80, left: '22%', top: 120 }) // % follows resizes
draw(search) // editor only: outline it on the desktop canvas
await auv.input.click(search) // clicks the center
draw(search.below(40, 8), 'hint')
```

`region()` takes two of `left`/`right`/`width` per axis (and of
`top`/`bottom`/`height`); a missing start edge is 0 and a missing size fills
the rest. `draw()` marks are part of the run's event log, so time travel shows
only the marks drawn up to the cursor; they are not device calls and are not
recorded for replay.

OCR can be limited to an area with `within`. AUV reads only that part of the
image (`RelativeRect` region); match bounds stay in screen space, and the
area is drawn dashed on the result's preview.

```ts
const sidebar = area(music).region({ width: '30%' })
const hits = await music.findText('Remember', { within: sidebar })
const ocr = await auv.text.recognize(await music.capture(), { within: sidebar })
await auv.apps.activate('com.netease.163music') // macOS; receipt path reports verification
```

`focus(target, { zoom = true, autoZoomOut = true })` eases the desktop canvas
camera onto a window, area, OCR result or match, frame, click or point. Long
jumps follow a smooth zoom-and-pan arc (zoom out mid-way, descend onto the
target; van Wijk & Nuij), sized by distance and squeezed into the time until
the next focus on the timeline. Like `draw()`, focuses are editor-only events
in the run's log, so scrubbing the timeline replays the camera; dragging or
zooming the canvas yourself interrupts a flight.

```ts
focus(todos) // fly to a window
focus(await counter.findText('Increment')) // then to the matches, arcing over the gap
focus(search, { zoom: false }) // pan only, keep the zoom level
```

### Scrolling

`win.scroll(at, { dx?, dy? })` wheel-scrolls once and `win.scrollUntil(at,
options)` scrolls in steps, observing after each step on the Runner
(`InputService/ScrollWindowPoint` and `ScrollUntil`). `at` is a screen-space
point or the center of an area or rectangle (unlike `win.click`, which takes a
window-relative point). Positive `dy` scrolls toward later content (down).

```ts
const results = area(music).region({ bottom: 80, left: '22%', top: 120 })
await music.scroll(results, { dy: 600 })
const found = await music.scrollUntil(results, { dy: 400, text: 'Remember' })
// found.reason: 'text-visible' | 'until' | 'end' | 'budget'; found.match is screen-space
const page = await music.scrollUntil(results, {
  dy: 400,
  until: obs => obs.text.includes('Reply'), // runs in the script for every observation
})
```

`scrollUntil` uses the `auv invoke input.scroll-until` defaults (`maxSteps` 50,
`settle` 400 ms, `confirmations` 2) and scrolls along one axis. The result keeps
the last observation's capture (`frame`) and OCR (`text`) as handles. An `end`
stop means no visual motion was observed, not that no content is left. The
daemon must be AUV 0.0.29 or later. On the mock desktop, the Music window's
song list scrolls (other windows do not):

```ts
const music = await auv.windows.resolve({ appName: 'Music' })
const songs = area(music).region({ height: 420, left: 16, top: 82, width: 448 })
const found = await music.scrollUntil(songs, { dy: 240, text: 'Remember' })
await auv.input.click(area(found.match!), { count: 2 })
await music.findText('Now playing: Remember', { within: area(music).region({ bottom: 0, height: 70 }) })
```

### Typing

`win.typeText(text)` and `win.pressKey('cmd+a')` deliver to that window
(`InputService/InputKeyboard`): the window is brought to the front and focused
first, so the text cannot land in another app. `auv.input.typeText` and
`auv.input.pressKey` go to whichever app has keyboard focus when they run, which
can be this playground's browser.

```ts
const music = await auv.windows.resolve({ bundleId: 'com.netease.163music' })
await music.click({ x: 528, y: 50 }) // the search box, window-relative
await music.typeText('Reply 超时空辉夜姬')
await music.pressKey('return')
await music.pressKey('cmd+a', { background: true }) // no activation; the box must already have focus
```

`{ background: true }` posts without activating the window. The control must
already have keyboard focus; a click does not give it focus in every app (it did
not in NetEase Cloud Music while another app was in front).

### Direct SDK (prototype)

Scripts can also call `@auv-js/sdk` directly. `sdk` is the SDK module and
`device` is the Runner client for the selected device and the current Run,
injected before every run: the same object a Node script gets from
`createAuv(await connect(...)).runner(route)`. Every
Runner RPC is available without a playground binding:

```ts
const music = (await device.windows.list()).find(w => w.window.applicationBundleId === 'com.netease.163music')
await music.click({ x: 528, y: 50 })
await music.typeText('Reply', { policy: sdk.InputPolicy.FOREGROUND_PREFERRED })
const playing = await device.macos.media.nowPlaying()
```

Calls cross to the page encoded, where the device credential is added; each one
is recorded in the timeline as `<Service>/<Method>` with ProtoJSON request and
response. Results are drawn by message type, like `auv.*` handles: captures as
frames, text matches and recognized text as boxes, input results as receipts at
the delivered point, windows and displays as outlines. Offline replay answers
SDK calls from the live run's recording too. The mock desktop does not serve SDK
calls yet, and the editor types `device` and `sdk` as `any`. See
`docs/ai/references/inspect/2026-10-08-playground-sdk-transport-design.md`.

Captures carry a ThumbHash (`CapturedFrame.thumbhash`): frames, and the first
live frame after connecting to a device, show a blurred preview at once and fade
the pixels in when they load.

Script API types (`Area`, `Rect`, `Point`, `WindowHandle`, `TextMatch`, …) can
be used by name in cells, e.g. `function toolbar(win: Area): Area`.

## Architecture

```
main thread                           language worker               exec worker
───────────                           ───────────────               ───────────
CodeMirror ── hover/lint/complete ──▶ TS 6 language service
           ── compile cell ─────────▶ ts-blank-space + acorn
                                      + magic-string (stepper) ──▶ instrumented JS
session ◀── onStep/onPause/onVars/onLog ─────────────────────────── await __step(id)
        ◀── call('windows.findText', …) ─────────────────────────── auv.* proxy
bindings ─▶ Backend (AUV via @auv-js/sdk | MockBackend)
store (zustand) ─▶ canvas, timeline, inspector, editor decorations
```

- `src/stepper` compiles one cell: strips erasable TypeScript while keeping
  offsets, inserts `await __step(id)` before statements (record-only
  `__stepSync` in sync functions), rewrites top-level declarations into
  prelude-declared globals for REPL persistence, and returns the last
  expression. `StepTimer` attributes time between hooks to lines, excluding
  pauses. Both are unit tested.
- The exec worker runs cells with indirect eval, pauses by awaiting a
  host-resolved promise, and proxies `auv.*` to the host with birpc.
- `runtime/bindings.ts` maps script calls onto a `Backend`, registers every
  result as a host-side resource, and returns plain handles to the worker.
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
- **Scroll** exposes `ScrollWindowPoint` and `ScrollUntil`; timed
  (`ScrollWindowPointMotion`) and live (`StreamScroll`) scrolling are not in the
  script API yet, and `scrollUntil` records only its last observation.
- **AX tree and object detection** have no AUV driver RPC yet; only the mock
  desktop provides an AX tree. The AX panel shows the current tree, not the
  tree at the time cursor (that needs AX snapshots recorded per step).
- **The event log is browser-only.** Its shape (seq, steps, binds, calls with
  effect, lineage) is meant to converge with AUV Run traces so recorded runs
  can be persisted and inspected by AUV; that read side does not exist yet.
- **Lineage stops at plain values**: `centerOf(match.bounds)` yields numbers,
  so a click point does not link back to the OCR match it came from.
- **Replay determinism**: `Date.now()`, `Math.random()` and `sleep()` are not
  recorded. Replay is order-based: arguments must match call by call.
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
pnpm -F @auv-js/repl-playground test:run  # stepper, area, camera, binding and store tests
pnpm -F @auv-js/repl-playground build
```

The root `pnpm lint`, `pnpm typecheck` and `pnpm test:run` include this package.
The root ESLint config ignores `devtools/**`, so this package keeps its own
config (React and UnoCSS rules).
