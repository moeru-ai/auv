# Held input and independent observation: project evidence

Date: 2026-09-18. Classification: docs-only design research.

Evidence level: public API and source inspection, without builds or live receiver
probes. This note records precedents, not AUV support claims or implementation
approval. Revisions were resolved from upstream `main` during this review.

## Peekaboo: complete gestures and a separate owned hold lifecycle

Revision: `4d3c92eaf6e08ad3f17657daa2754b851bc754f4`.

- The CLI exposes complete foreground drag with endpoints, duration, steps,
  modifiers, and left/right button; `press --hold` describes a duration per key.
  These commands do not themselves establish an open-ended held-input session.
  [Drag contract](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/docs/commands/drag.md#L7-L33),
  [press contract](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/docs/commands/press.md#L7-L33).
- Separately, the public Bridge client exposes owner creation, begin, release,
  revoke, and disconnect for exact-window pointer holds. Begin returns a hold
  receipt. Capability checks require a protocol-1.30 host. This is actual
  cross-request API evidence, rather than an inference from internal pressed
  state. The inspected API has no held-receipt move operation; it does not
  establish arbitrary cross-call drag or keyboard holds.
  [Public Bridge methods and gates](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooCore/Sources/PeekabooBridge/PeekabooBridgeClient%2BHeldPointer.swift#L4-L95).
- The Bridge binds owners to peer process identity. The lifecycle accepts
  0.05–30 seconds, permits one active hold per owner, and checks receipt ownership.
  Its watchdog detects owner process exit/generation change, expiry, window
  changes, and target process generation changes. Begin cancellation signals
  termination. Cleanup attempts mouse-up, reports its outcome, and refuses to
  send it to a recycled PID. These are implementation mechanisms, not a promise
  that cleanup succeeds after host death or that the app consumed the input.
  [Peer binding](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooCore/Sources/PeekabooBridge/PeekabooBridgeServer%2BHeldPointer.swift#L21-L32),
  [admission and begin cancellation](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift#L277-L365),
  [receipt checks](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift#L400-L425),
  [watchdog](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift#L526-L558),
  [cleanup result](https://github.com/openclaw/Peekaboo/blob/4d3c92eaf6e08ad3f17657daa2754b851bc754f4/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift#L614-L642).

## CUA: primitive input calls alongside complete gestures

Revision: `9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb`.

The computer Python interface declares `mouse_down/up`, `key_down/up`, complete
`drag`/`drag_to`, and a separate `screenshot`. The sandbox mouse interface sends
separate down/up transport requests. The local `cua-auto` implementation calls
pynput press/release separately and implements complete drag as press, move,
release. These inspected methods contain no owner token, lease expiry, or
cancellation cleanup; this limited inspection does not prove their absence from
all CUA transports and runtimes.
[Computer interface](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/python/computer/computer/interface/base.py#L67-L210),
[screenshot](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/python/computer/computer/interface/base.py#L275-L280),
[sandbox requests](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/python/cua-sandbox/cua_sandbox/interfaces/mouse.py#L29-L43),
[local primitive and gesture implementations](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/python/cua-auto/cua_auto/mouse.py#L49-L101).

CUA Driver's macOS drag tool is a complete press-drag-release gesture. Its
ScreenCaptureKit video backend separately starts recording and finalizes MP4 on
stop. A recording backend alone is not evidence of a public live frame-analysis
subscription API.
[Driver drag](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/cua-driver/rust/crates/platform-macos/src/tools/drag.rs#L1-L75),
[video lifecycle](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/cua-driver/rust/crates/platform-macos/src/video_sckit.rs#L18-L23),
[encoding and stop](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/cua-driver/rust/crates/platform-macos/src/video_sckit.rs#L100-L160).

## Other inspected precedents

Anthropic's reference demo separately dispatches `left_mouse_down` and
`left_mouse_up` through xdotool, and implements bounded key hold as key-down,
sleep, key-up. This supports separating primitive input from bounded helpers;
it does not establish a durable ownership or watchdog contract.
[Demo, revision 4cfc16d](https://github.com/anthropics/claude-quickstarts/blob/4cfc16d8262f22b222f42e1c691e5d3bacc081fe/computer-use-demo/computer_use_demo/tools/computer.py#L334-L390).

Browser-use's actor mouse exposes separate down/up methods. These are
browser/CDP-scoped evidence, not native background window input evidence; full
cleanup behavior was not audited.
[Mouse API, revision d8110c5](https://github.com/browser-use/browser-use/blob/d8110c5ff87ccba887aaa726cdb780f2f84bef8d/browser_use/actor/mouse.py#L60-L85).

Playwright exposes separate mouse down/up and a complete click with optional
delay. Independently, its documented screencast API exposes start/stop and an
`onFrame` callback with JPEG data, timestamp, and viewport, with optional file
recording. This is a concrete browser precedent for an observation stream that
can feed consumers independently of individual input calls; it does not by
itself supply AUV's proposed analysis/verification pipeline.
[Mouse API](https://playwright.dev/docs/api/class-mouse),
[Screencast API](https://playwright.dev/docs/api/class-screencast#start).

## Design implications for BG-1 (inferences, not accepted decisions)

The examples support coexisting complete gestures and lower-level persistent
input. They do not force observation or semantic verification into the input
operation. Peekaboo supplies a particularly relevant lifecycle precedent for
exact-window background holds, but its inspected hold API must not be presented
as proof of interactive drag support.

If AUV accepts cross-call holds, owner identity, expiry, target continuity,
explicit release, cancellation/disconnection handling, and observable cleanup
failure need a contract. A video stream can remain an independent observation
producer, with feedback and verification as consumers. Whether BG-1 implements
only bounded gestures or also an owned cross-call hold is still an owner decision.


## Follow-up: agent-browser and automation libraries

Agent-browser revision `aff6125c023b810ea3f2e5deec5379e9a4270bdc`
provides `drag <src> <tgt>`, `mouse down/up [button]`, and top-level
`keydown/keyup <key>`. Its current backend is a persistent Rust daemon using
direct CDP, not a Playwright wrapper. The daemon shares `DaemonState` across
socket requests; mouse state records position/button bits, and later moves carry
those bits. Thus separate CLI invocations in the same live session can continue
a held mouse gesture. The keyboard methods dispatch separate CDP key events;
this is not evidence of a daemon-managed keyboard lease.
[Public commands](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/README.md#L129-L137),
[mouse syntax](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/README.md#L315-L324),
[daemon architecture](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/README.md#L1699-L1704),
[shared state](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/native/daemon.rs#L242-L270),
[held mouse movement and down/up](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/native/actions.rs#L13110-L13202),
[keyboard dispatch](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/native/actions.rs#L12928-L12961).

The complete drag handler resolves endpoints, approaches the source, posts down,
moves with the left-button bit, then posts up. A failed movement returns through
`?` before the release statement; the inspected handler has no cleanup guard.
Its daemon idle shutdown is a broader browser-lifetime mechanism, not a short
held-input timeout. This is a source-level failure-path finding, not a reproduced
stuck-input report or a complete cleanup audit.
[Drag implementation](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/cli/src/native/actions.rs#L10577-L10688),
[idle policy](https://github.com/vercel-labs/agent-browser/blob/aff6125c023b810ea3f2e5deec5379e9a4270bdc/README.md#L1704).

Agent-browser also documents WebSocket frames with sequence numbers and
timestamps, an input task that does not wait for frame delivery, and optional
latest-frame/ack handling. This provides a concrete precedent for independent
observation and input transport; it is not an AUV streaming implementation or a
general semantic verification guarantee.
[Streaming documentation](https://agent-browser.dev/streaming).

Additional official API precedents inspected in the same design review:

- Playwright offers locator `dragTo` with source/target positions alongside
  mouse down/move/up and keyboard down/up. `keyboard.press` delay is the interval
  between down and up; it should not be confused with a typing delay between
  characters.
  [Locator drag](https://playwright.dev/docs/api/class-locator#locator-drag-to),
  [mouse](https://playwright.dev/docs/api/class-mouse),
  [keyboard press](https://playwright.dev/docs/api/class-keyboard#keyboard-press).
- PyAutoGUI offers `mouseDown/mouseUp`, complete `dragTo` with duration/button,
  and a keyboard `hold` context manager that scopes key-down/key-up.
  [Mouse API](https://autogui.readthedocs.io/en/latest/mouse.html),
  [keyboard API](https://pyautogui.readthedocs.io/en/latest/keyboard.html).
- Selenium offers `click_and_hold/release` and `drag_and_drop`. The WebDriver
  actions API retains depressed input state and exposes release-all/reset input
  state explicitly.
  [Mouse actions](https://www.selenium.dev/documentation/webdriver/actions_api/mouse/),
  [release all actions](https://www.selenium.dev/documentation/webdriver/actions_api/#release-all-actions).

Design inference: these APIs distinguish held input state from complete gestures.
They do not require duplicate platform backends for the two public forms.

## Follow-up: locally bundled OpenAI Sky

Evidence level: shipped documentation, TypeScript declarations, and JavaScript
wrapper inspection, without native execution or native binary inspection.
The inspected local package is `@oai/sky` 0.7.1, under
`/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/sky`.
Public npm lookup returned 404; conclusions use the installed package, not
third-party reconstructions or a claim about all releases.

The full-desktop API provides complete `drag({path, key?})`, bounded mouse
presses through `click({duration, ...})`, and bounded keyboard presses through
`press_key({duration, ...})`. Its `drag_handle()` exposes `start(point)`,
`move_to(point)`, and `end()` for keeping the button pressed across calls and
observations. No separate hold method is needed in this surface.
Sources relative to the package root: `docs/sky-full-desktop-api.md:29`,
`:93`, `:103`, `:109`, `:128`; declarations under
`dist/project/cua/sky_js/src/types/full-desktop/{Click,PressKey,Drag,DragHandle}.d.ts`.

The Linux `targets/linux/drag_handle.js` wrapper tracks idle/dragging/ended and
rejects invalid operation order. Each method calls the native drag_handle
command with an action; the wrapper does not expose an owner token or establish
cross-client exclusion. Native conflict handling and failure cleanup are not
verified by these JavaScript checks. Complete `targets/linux/drag.js` dispatches
a separate native drag command, so public coexistence does not prove the complete
operation is implemented by calling the public handle.

This is platform-specific evidence: the inspected macOS window API instead
accepts app plus from/to coordinates for drag, and its PressKey declaration
has no duration. Do not project full-desktop options onto every Sky target.
Sources: `dist/project/cua/sky_js/src/types/window/{Drag,PressKey}.d.ts`.

The separate installed `@oai/cua` 0.2.5 package must not be confused with
trycua/cua. Its native App declarations expose complete `drag(from, to)`,
`pressKey(key)`, and click options, alongside separate screenshot/AX reads.
The inspected App surface does not expose independent down/up or hold methods.
Its shipped wrapper forwards native actions to embedded Sky implementations;
this is no evidence of caller-specific held-input ownership or conflict policy.
Local source: sibling package `@oai/cua`,
`dist/lib/js/oai_js_cua/src/tinysky_alt/types.d.ts:80` and
`create_tinysky_alt.js` in that directory. Browser surfaces are separate and
are not covered by this native-App statement.

CUA concurrency follow-up: the macOS Rust driver's background mutation helper
uses per-PID asynchronous locks spanning target proof, focus changes, dispatch,
restoration, and verification. It also carries task-local evidence for nested
calls. This serializes operations on a target process; it is distinct from
per-Run held-button ownership and from isolated virtual pointers.
[Fixed-revision source](https://github.com/trycua/cua/blob/9e60d90b8681d3ba7ccf2c7801dbaa21b0d6efbb/libs/cua-driver/rust/crates/platform-macos/src/background_mutation.rs#L1-L58).
