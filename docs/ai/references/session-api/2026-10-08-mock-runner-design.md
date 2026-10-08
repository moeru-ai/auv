# Mock Runner: synthetic desktops behind the Runner API

Date: 2026-10-08

Status: **accepted direction**; the first slice is implemented (2026-10-09,
see "Implemented"). Names marked *provisional* are open.

## Why

The playground's mock desktop (`apps/repl-playground/src/backend/mock.ts`) is
a deterministic in-browser desktop. It has two displays and three apps
(Todos, Counter, and Music with a scrolling song list). Clicks and keys
mutate its state, and OCR returns exact text geometry from the scene graph.
Today it does two jobs:

- **Demonstration.** Scripts run without a device or permissions.
- **Debugging.** The same scene answers every run, so a failing script fails
  the same way twice.

The owner sees a third job, and it shapes the design: **synthetic benchmark
datasets.** A scene that AUV renders and owns knows its own ground truth. That
includes every widget, its text and its bounds, and whether a task has been
completed. A generated episode is therefore a labeled example, with no human
annotation.

Today the mock can do none of the benchmark job, for three reasons:

- It implements the playground's per-method `Backend` interface, which is
  being retired (see the
  [playground SDK transport design](../inspect/2026-10-08-playground-sdk-transport-design.md)).
  Only playground scripts can reach it, not Node, Rust, the CLI or MCP.
- Its scene is hard-coded in one class: three apps with fixed layout, data
  and theme.
- It has no notion of a task, a goal check, or an exported episode.

## Decision already made

The owner decided on 2026-10-08 that the mock desktop stays and moves to the
SDK layer. It becomes a mock Runner that answers SDK RPCs, so that Node tests
and the playground use the same mock.

## Proposed shape

Three concepts. All names are *provisional*.

```text
Scene      state + layout + render + hit-test + text truth      (pure, seeded)
  ▲
MockRunner Runner RPCs over a Scene: Capture, FindText, ClickPoint,
  │        Scroll*, InputKeyboard, ListWindows, ...             (an SDK Transport)
  ▼
Task       setup(seed) → Scene, goal check, instruction text    (benchmark unit)
```

### Scene

A scene is plain data plus pure functions:

- **State.** Apps, windows, widgets and app data, such as the song list.
- **`render(bounds, scale)`** returns pixels, drawn with canvas in the browser
  and in Node.
- **`hit(point)`** returns the widget at a point.
- **`text(bounds)`** returns ground-truth text, including clipping by scroll
  viewports.
- **`apply(event)`** applies a click, key, text or wheel event and returns the
  next state.

Two properties matter for benchmarks:

- **Determinism.** The same seed and the same event sequence give the same
  pixels and state, with no wall clock and no global `Math.random`.
- **Variation.** Generators vary layout, data, theme, font, scale and window
  placement from the seed. This gives many distinct episodes that share one
  checker.

### MockRunner

`MockRunner` implements the SDK `Transport` and answers Runner RPCs with the
same protobuf messages and rules as the real Runner. Examples:

- `ClickPoint` follows the position rules from #284.
- `GetCaptureImage` serves captures from a bounded capture store.
- An inside-window check runs before delivery.

A script therefore cannot tell the mock from a device, except through
`DiscoverRunner`, which names the Runner class. A script that works on the
mock should run on a device unchanged.

Every RPC that the mock does not implement returns `UNIMPLEMENTED`, as an old
Runner does. A missing behavior is never approximated silently.

Fidelity knobs are off by default and seeded when on. They make the mock
harder than a perfect oracle:

- OCR noise: confidence, dropped or merged words, and character errors;
- delivery outcomes: background delivery refused and foreground fallback;
- latency.

### Task

A task is a seeded scene setup plus a goal check over scene state, for
example "the song titled *Reply* is playing". The check reads state, not
pixels. This is the same split as AUV's verification-is-separate-from-delivery
rule: a delivered click proves nothing until the check passes.

An episode is the task, the seed, the ordered RPC requests and responses
(the RPC-level recording that playground replay also needs), and the
verdict. Episodes export as a dataset row:

- the instruction;
- each step's frames, as capture references whose pixels the exporter
  renders;
- the actions taken;
- ground-truth boxes;
- the verdict.

## Placement

- `@auv-js/sdk` gets the `Transport` seam (it exists) and a `mock` entry point
  (*provisional*: `@auv-js/sdk/mock`) that holds `MockRunner` and the scene
  model.
- The playground's mock desktop becomes one scene definition on top of it.
  The per-method `MockBackend` is deleted.
- Scenes and tasks live outside the SDK core, for example
  `js/packages/scenes` (*provisional*). The SDK then does not ship app
  fixtures.

## Decisions (owner, 2026-10-08)

- **JS only.** `MockRunner` is an SDK `Transport`. A Runner class served by the
  daemon, so that Rust, the CLI and MCP could reach the mock, is not planned.
  It stays RPC-level so it could become one later.
- **Keep it simple.** Benchmark tasks, episode export and fidelity knobs
  (above) are the long-term direction, not current work.

## Implemented (2026-10-09)

The first slice follows the plan below, with these choices:

- **Registration** follows Connect-ES's `createRouterTransport`
  (`connectrpc/connect-es` `packages/connect/src/router-transport.ts`), as the
  owner approved. The mock registers typed implementations per generated
  service, and methods left out answer `UNIMPLEMENTED`:

  ```ts
  createMockTransport(({ service }) => {
    service(WindowService, { listWindows: () => ({ windows }) })
  })
  ```

- **`@auv-js/sdk` exports** (from the main entry point, not a `/mock` subpath):
  - `createMockTransport`, which also answers gRPC Reflection for the
    registered services, so `discoverRunner` and method `presentation` work
    as on a device;
  - `serveMockDaemon`: one local Device and the Run lifecycle;
  - the Runner's generated service descriptors, such as `WindowService` and
    `InputService`.
- **Scene and Runner** stay in the playground:
  - `backend/mock-desktop.ts` (`MockDesktop`) is the scene: state, painting,
    hit testing and OCR ground truth. It was already deterministic, so no
    seed was needed.
  - `backend/mock-runner.ts` serves the scene with the Runner's rules:
    positions in any space, window-only input inside the window, global clicks
    without window options, captures by reference with a ThumbHash, and
    `GetCaptureImage` crop, fit and encode.
- **Backend.** The playground's mock backend is `AuvBackend` over the mock
  transport, so `auv.*`, direct SDK calls, the timeline and replay use one
  path. The per-method `MockBackend` is deleted.

Deferred, marked in code:

- **`TODO(mock-runner-tasks)`:** scene variation, task goal checks and
  episode export.
- **`TODO(mock-runner-image-ocr)`:** `RecognizeText` on caller-owned images
  answers `UNIMPLEMENTED`.
- **Method docs:** `MethodDocsService` is not served, so docs resolve
  `undefined`; the presentation still comes through reflection.
- **AX tree:** the scene supplies the tree directly (`TODO(auv-ax-tree)`).
- **Node tests:** Node has no `OffscreenCanvas`
  (`NOTICE(mock-runner-node-tests)`). Node tests cover Runner rules and scene
  state. Captures, OCR, scroll-until and image encoding were checked in a
  headless Chromium.

## First slice plan (as proposed)

This slice is the minimum that replaces today's mock and leaves the benchmark
path open. It does not build the benchmark itself.

1. `MockRunner` as an SDK `Transport`, over a `Scene` interface. The RPCs it
   answers are the ones `MockBackend` answers today:
   - list and resolve windows and displays;
   - capture plus `GetCaptureImage`;
   - find, recognize text;
   - `ClickPoint`;
   - window scroll and scroll-until;
   - keyboard input.
2. Port the current three-app desktop to a seeded scene, and seed its
   randomness.
3. Point the playground's mock backend at it, and delete `MockBackend`.
4. Node tests that drive the SDK against it.

Not in this slice: tasks, episode export, fidelity knobs and the daemon
Runner class. Each would be marked with a `TODO(mock-runner-…)` marker where
it plugs in.
