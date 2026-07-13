# Projects Research and REPL API

Date: 2026-05-25

Status: research reference, API direction

## Purpose

This document records the comparison work around AUV, MaaFramework,
Appium Mac2/XCTest, WebDriver, Playwright, and Cua. It also captures the API
direction discussed for AUV: RPC-native runtime, first-class devices and
sessions, handle-based JavaScript APIs, and optional REPL globals.

The intent is not to choose one existing project to copy. Each reference solves
a different part of the problem:

- MaaFramework is strong at visual recognition, ROI-driven action, and
  pipeline execution.
- Appium Mac2 and XCTest are strong at accessibility-backed native UI
  automation.
- WebDriver is strong as a session-oriented remote automation protocol.
- Playwright is strong at ergonomic user APIs and fixture-managed runtime
  context.
- Cua is strong at RPC-native computer-use drivers, visual cursor overlays,
  and local/VM computer control.
- AUV should combine runtime recording, artifacts, replay/inspection,
  multi-source observation, and programmable orchestration without overfitting
  to a single UI stack.

## Current AUV Baseline

AUV already has several useful primitives:

- A runtime and artifact store under `.auv/runs/{run_id}/`.
- macOS driver capabilities for screenshots, OCR, AX-related observation, and
  input actions.
- Recipe manifests and case matrices.
- Scroll scan work that can detect list-like visual rows, crop list item
  candidates, OCR the crops, and write item context artifacts.
- Durable vocabulary in `docs/TERMS_AND_CONCEPTS.md`.

However, the current design also has structural gaps:

- Device and session are not yet first-class protocol concepts.
- The CLI is still too close to the capability surface.
- Recipes cannot elegantly express complex control flow, sub-recipes, typed
  context, or structured return values.
- OCR fragments and list item observations do not yet project into a stable
  UI semantic layer.
- Scroll scan has a partial loop, but top/bottom detection, section handling,
  segmentation, and until-match behavior remain provisional.
- Artifact capture is a strength, but artifact references are not yet a core
  RPC contract for every observation/action.

## Reference Projects

### MaaFramework

Repository: <https://github.com/MaaXYZ/MaaFramework>

Useful files and docs:

- <https://github.com/MaaXYZ/MaaFramework/blob/main/docs/en_us/3.1-PipelineProtocol.md>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/MaaFramework/Task/Component/Recognizer.cpp>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/MaaFramework/Task/Component/CustomRecognition.cpp>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/MaaFramework/Vision/OCRer.h>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/MaaFramework/Tasker/RuntimeCache.cpp>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/include/MaaAgent/Message.hpp>
- <https://github.com/MaaXYZ/MaaFramework/blob/main/source/include/MaaAgent/Transceiver.h>

MaaFramework models automation as a visual recognition and action pipeline.
The basic unit is a pipeline node with `recognition`, `action`, `next`,
`on_error`, `roi`, `target`, and timeout-like controls. Recognition runs inside
an ROI, returns a `box` and algorithm-specific `detail`, then the action uses
that box or an explicit target.

Maa's result model is useful for AUV:

```text
all results
filtered results
best result
box
detail
draw/debug output
```

Maa also has a custom logic boundary. `CustomRecognition` and `CustomAction`
can run in an Agent process. The Agent protocol uses message types such as
`CustomRecognitionRequest`, `CustomActionRequest`, and reverse requests like
`ContextRunTaskReverseRequest`. The transport in the current source uses
ZeroMQ (`zmq`), not just in-process bindings.

Maa does not define a normalized accessibility-like UI tree. Its core is closer
to:

```text
screenshot -> recognizer -> box/detail -> action target
```

That is a good pattern for visual automation, but it does not solve AUV's
need to merge AX, OCR, visual rows, and artifacts into a structured observation
model.

### Appium Mac2 Driver and XCTest

Repository: <https://github.com/appium/appium-mac2-driver>

Relevant docs:

- <https://appium.github.io/appium.io/docs/en/drivers/mac2/>
- <https://appium.io/docs/en/latest/intro/drivers/>
- <https://developer.apple.com/documentation/XCUIAutomation/XCUIApplication>
- <https://developer.apple.com/documentation/XCUIAutomation>

Appium Mac2 exposes macOS native app automation through Appium/WebDriver. The
outer driver is a Node.js Appium driver. The inner native automation layer is
based on Apple's XCTest/XCUIAutomation infrastructure.

XCTest is best understood as an AX-backed automation layer, not a raw AX API.
It exposes `XCUIApplication`, `XCUIElementQuery`, and `XCUIElement`. Those
elements are largely built from accessibility metadata such as role, label,
identifier, value, frame, and children.

This means XCTest can operate Electron apps when the app exposes useful
Accessibility information, but it degrades on custom canvas/WebGL/self-drawn
interfaces where the accessibility tree is weak or incomplete.

The main lesson for AUV is the layering:

```text
client script
  -> stable protocol/API
    -> driver session
      -> platform helper
        -> native automation framework
```

Appium Mac2 is a strong example of API/protocol separation. It is not enough
for AUV by itself because AUV also needs OCR-first observation, artifact-first
inspection, replay, and run recording.

### WebDriver and WebDriver BiDi

Relevant specs and docs:

- <https://w3c.github.io/webdriver/>
- <https://www.w3.org/TR/webdriver-bidi/>
- <https://developer.mozilla.org/en-US/docs/Web/WebDriver>

WebDriver is a W3C remote-control protocol for browser automation. Classic
WebDriver is HTTP-based and centered around:

```text
POST /session
/session/{sessionId}/element
/session/{sessionId}/element/{elementId}/click
/session/{sessionId}/screenshot
DELETE /session/{sessionId}
```

WebDriver BiDi adds a bidirectional protocol model for event-driven browser
automation.

WebDriver is highly relevant as a compatibility target because it already has
sessions, capabilities, element handles, screenshots, and actions. It should
not be AUV's core protocol because it is browser-centric and does not have
first-class concepts for devices, runs, artifacts, OCR evidence, scroll scan
pages, or replay inspection.

The recommended stance is:

```text
AUV Native RPC = core protocol
WebDriver/Appium endpoint = optional compatibility facade
```

### Playwright

Repository/docs:

- <https://github.com/microsoft/playwright>
- <https://playwright.dev/docs/test-fixtures>
- <https://playwright.dev/docs/api/class-playwright>

Playwright is useful less for OS-level capability design and more for user API
ergonomics.

It has two API shapes:

```ts
// Library API: explicit handles.
import { chromium } from "playwright";

const browser = await chromium.launch();
const context = await browser.newContext();
const page = await context.newPage();
```

```ts
// Test runner API: fixture-managed context.
import { test, expect } from "@playwright/test";

test("example", async ({ page, context }) => {
  await page.goto("https://example.com");
});
```

The important pattern is:

```text
runtime context is managed implicitly
user-facing objects are explicit handles
```

Users do not pass a raw `ctx` everywhere. They operate on `page`, `locator`,
`browser`, and `context` handles. AUV should follow that ergonomic pattern:
keep a low-level session/context internally, but expose app/window/region/list
handles to scripts and REPL users.

### Cua

Repository: <https://github.com/trycua/cua>

Useful local paths inspected:

- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/cua-driver/src/main.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/cua-driver/src/serve.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/platform-macos/src/lib.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/platform-macos/src/cursor/overlay.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/platform-macos/src/tools/click.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs/crates/platform-macos/src/input/mouse.rs`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver/Sources/CuaDriverCore/Cursor/AgentCursor.swift`
- `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver/Sources/CuaDriverCore/Input/MouseInput.swift`

Cua's macOS driver is RPC-native. The Rust driver can run as an MCP JSON-RPC
server over stdio and also has a Unix-socket daemon mode. The daemon protocol
in `serve.rs` is line-delimited JSON with methods such as `call`, `list`,
`describe`, and `shutdown`.

On macOS, Cua's entrypoint treats AppKit as a main-thread requirement:

```text
main thread
  -> cursor overlay NSApplication loop

background thread
  -> tokio runtime
  -> MCP server / tool registry
```

Cua's "virtual pointer" is not a virtual HID device. It is a visual overlay:

```text
transparent click-through NSWindow
  -> tiny-skia/CALayer cursor rendering
  -> MoveTo / ClickPulse commands
  -> does not move the real mouse
```

Input is separate:

- Preferred path: AX actions such as `AXUIElementPerformAction`.
- Pixel fallback: `CGEvent`, `CGEvent::post_to_pid`, and SkyLight
  `SLEventPostToPid` for backgrounded or non-AX surfaces.
- Frontmost HID-tap path is used only for surfaces that require real HID-like
  input, with the known side effect that the real cursor can move.

This separation is important for AUV:

```text
visual cursor = user-visible trust signal
semantic action = AX/native action
pixel action = fallback event synthesis
```

Cua currently looks more single-daemon oriented by default socket path. AUV
should instead make device and session first-class in the protocol so one RPC
server can multiplex local, remote, container, and VM devices.

## Conceptual Comparison

| Concept | MaaFramework | Appium Mac2 / XCTest | WebDriver | Playwright | Cua | AUV target | AUV advantage? |
|---|---|---|---|---|---|---|---|
| Primary target | Screenshot/vision automation, often games and emulators | macOS apps through XCTest | Browsers | Browsers | Computer-use desktops, local/VM/cloud | Native apps and computer workflows across AX/OCR/vision | Yes, if AUV keeps multi-source observation and artifact inspection central |
| Core unit | Pipeline node | Appium/WebDriver session + XCTest element | Browser session | Browser/context/page/locator | Tool call / MCP session / daemon call | Device, session, run, artifact, observation, action | Yes, if these become protocol-native |
| Observation model | ROI recognition result | XCUIElement tree | DOM/web elements | Locator over DOM/accessibility | Screenshots, AX tree, tools | AX/OCR/vision evidence projected into observations | Partial today, stronger than others once normalized UI layer lands |
| Result detail | `box`, `detail`, `all/filtered/best` | Element metadata | Element IDs, attributes | Locator/action assertions | Tool JSON and recordings | RecognitionResult, candidates, artifact refs | Yes, AUV can combine Maa-like detail with artifact refs |
| Artifact-first inspection | Debug draws in debug mode | Limited | Limited | Traces/screenshots/videos in test runner | Trajectory recording | Core run/artifact store | Yes, this is one of AUV's strongest differentiators |
| Script ergonomics | JSON pipeline + language bindings + Agent | Appium clients | Selenium/WebDriver clients | Excellent handle/fixture API | Python/TS computer APIs and tools | JS SDK, REPL globals, handle APIs | Not yet; AUV should learn from Playwright and Cua |
| Multi-device model | Controllers/resources | Appium server sessions | Remote ends | Browser projects/devices | Local/cloud/VM computers | Device registry + sessions | Potential advantage, but not implemented yet |
| UI semantic layer | No generic UI layer | XCUIElement layer | Browser element layer | Locator abstraction | AX/screenshot tools, not unified UI layer | AX/ARIA-friendly Observed UI Layer | Potential advantage; currently missing |
| Scroll/list scan | Domain/project-specific pipelines | Depends on app accessibility | Browser DOM scroll | Locators and DOM | Visual/screenshot tools | Scroll scan with visual rows + OCR + artifacts | Partial advantage already, but boundary detection and segmentation are incomplete |
| Compatibility surface | C API, Python/Node, Agent | WebDriver/Appium | Standard | Playwright API | MCP/CLI/Python/TS | Native RPC plus adapters | Yes if AUV core stays protocol-clean |

## Capability Comparison

| Capability | MaaFramework | Appium Mac2 / XCTest | WebDriver | Playwright | Cua | AUV current/target | AUV advantage? |
|---|---|---|---|---|---|---|
| OCR | Strong OCR pipeline | Not primary | Not native | Not native | Screenshot-oriented, can integrate | Current macOS OCR exists | Yes versus AX-only tools; comparable to Maa for OCR-first work after contracts improve |
| Template/image recognition | Strong | Not primary | Not native | Screenshots only unless user code | Possible tool-level image work | Planned/template matching not fully mature | No today; Maa is stronger |
| AX/accessibility tree | Not primary | Strong through XCTest | Browser accessibility indirectly | Strong web locator/accessibility model | Has AX tree tools | macOS AX exists but needs unified model | Potential yes, because AUV can merge AX with OCR/vision |
| Pixel click | Yes | WebDriver/Appium action path | Yes for browsers | Yes for browsers | Strong CGEvent/SkyLight strategies | Existing macOS click paths, still evolving | Partial; Cua is more mature here |
| Background click/no focus steal | Some controller-specific support | XCTest/Appium behavior varies | Browser-scoped | Browser-scoped | Strong AX/pid-routed design | AUV needs clearer policy and locks | No today; Cua is ahead |
| Visual cursor / trust signal | Debug overlays/draws | No | No | Trace viewer cursor in recordings | Strong overlay cursor | AUV has dual cursor design notes, not core | Potential, not current |
| Run recording | Not central like AUV | Test logs | Limited protocol-level | Trace viewer, video, screenshots | Trajectories | Current run/artifact store | Yes |
| Replay/inspection | Debugger ecosystem | Test reports | Client-specific | Trace viewer | Trajectory viewer | AUV inspection is a project goal | Potential yes; current viewer/API still forming |
| Multi-session | Tasker/task model | WebDriver sessions | Standard browser sessions | Isolated contexts/pages | Tool/server sessions | Not first-class yet | Potential yes if device/session/run design lands |
| Remote/container computer use | Controllers/agents | Remote Appium server | Remote endpoints | Browser server/remote | Strong Cua sandbox/VM story | Target architecture | No today; target can be competitive |
| JS REPL/API | Node binding exists | Appium JS client | Selenium/WebDriver JS | Excellent | TS SDK exists | Not implemented yet | No today; should borrow Playwright ergonomics |
| WebDriver compatibility | No | Yes | Native | No, its own protocol | MCP/tools, not WebDriver | Optional facade recommended | Potential; not core |

## API Design Direction for AUV

### Avoid a `ctx.*`-First User API

A low-level context object is useful internally, but it is not the right
primary user API. A `ctx.observe.*`, `ctx.action.*`, `ctx.artifact.*` design
puts too much infrastructure in every script.

Prefer:

```ts
import { auv } from "@auv/sdk";

const device = await auv.device();
const session = await device.sessions.create({ name: "music" });
const app = await session.app("com.netease.163music");
const rows = await app.region("main").list().rows();

await rows[0].click();
```

Or in REPL:

```ts
await useDevice("local");
await useSession("music");

const rows = await app("com.netease.163music").region("main").list().rows();
```

The internal context still exists:

```text
AuvClient
  -> DeviceHandle
    -> SessionHandle
      -> AppHandle
        -> RegionHandle
          -> ListHandle
            -> ListItemHandle
```

Each handle carries the needed identifiers and scope:

```text
device_id
session_id
run_id
target app/window/region
artifact namespace
capability profile
```

### RPC-Native Core

AUV should treat RPC as the native execution model, not as a wrapper around the
CLI. CLI, JS SDK, REPL, MCP, WebDriver facade, and future UI surfaces should all
talk to the same runtime model.

Recommended request envelope:

```json
{
  "id": "req_123",
  "device_id": "local",
  "session_id": "music",
  "run_id": "run_abc",
  "method": "observe.windowRegion",
  "params": {
    "target": "com.netease.163music",
    "region": [0.2, 0.18, 0.78, 0.72]
  }
}
```

`run_id` can be omitted for convenience if the session has an active run, but
the runtime should always resolve it before recording artifacts or events.

### Device, Session, and Run

Preferred terms:

- **Device**: a controllable/observable computer target. Examples: local macOS,
  remote macOS, macOS VM, Windows VM, Android emulator, container desktop,
  browser-like sandbox.
- **Device Profile**: saved connection metadata, similar to Docker's context
  concept.
- **Device Connection / Endpoint**: the transport used to reach the device:
  local in-process, Unix socket, TCP, SSH, stdio, guest agent, VM proxy.
- **Session**: an automation context on a device. Holds target app/window
  defaults, cursor identity, run recording state, observation cache, and
  permission/capability state.
- **Run**: a concrete execution record under a session. It is the replay and
  artifact boundary.

`host` is not preferred because it implies the machine hosting the server,
while AUV's target may be a remote VM or container. `conn` is not preferred
because it describes transport, not the automation target.

### Device Registry API

The API can borrow the idea of Docker contexts: a client can have a current
default target, list configured targets, and switch the active target.

Recommended JS shape:

```ts
const device = await auv.device();          // current/default device
const local = await auv.device("local");    // named device shortcut

const devices = await auv.devices.list();
await auv.devices.use("local");
await auv.devices.register({
  id: "remote-mac",
  kind: "macos",
  endpoint: "tcp://192.0.2.10:7654"
});
```

Recommended CLI shape:

```bash
auv device list
auv device use local
auv device inspect local
auv device create remote-mac --endpoint tcp://192.0.2.10:7654

auv session list --device local
auv session create music --device local
```

Convenience defaults are fine at the SDK/CLI layer. The protocol should still
resolve explicit `device_id` and `session_id` for every request.

### Multi-Session Semantics

One AUV RPC server can manage many devices and many sessions:

```text
AUV RPC Server
  -> Device Registry
    -> Device
      -> Session
        -> Run / Artifact / Observation / Action
```

This supports:

- Local AUV device.
- Remote AUV device.
- Container or VM device.
- Multiple sessions on one device.
- Multiple app targets in one device.
- Per-session cursor identities and artifact namespaces.

However, local desktop actions are not safely parallel just because sessions
exist. On macOS, frontmost app, TCC permissions, global input, and screen
capture are device-level resources.

Recommended concurrency model:

```text
observe/capture/OCR/parse: can be parallel where platform allows
mutating actions: guarded by device-level action lock
session caches/artifacts: isolated by session/run
```

## Proposed AUV Native RPC Surface

This is a design direction, not an implementation commitment.

### Device Methods

```text
device.list
device.get
device.current
device.use
device.register
device.remove
device.capabilities
```

### Session Methods

```text
session.list
session.create
session.get
session.close
session.current
session.use
session.capabilities
```

### Run and Artifact Methods

```text
run.start
run.end
run.get
run.list
artifact.get
artifact.write
artifact.resolve
```

### Observation Methods

```text
observe.screenshot
observe.window
observe.windowRegion
observe.axTree
observe.ocr
observe.region
observe.uiLayer
```

### Scan Methods

```text
scan.region
scan.list
scan.scroll
scan.segment
```

### Action Methods

```text
action.click
action.type
action.key
action.scroll
action.drag
action.setValue
action.launchApp
```

### Compatibility Methods

```text
webdriver.newSession
webdriver.deleteSession
webdriver.findElement
webdriver.performActions
webdriver.screenshot
```

These should be adapters over AUV native methods, not the native model itself.

## Structured Observation Direction

AUV needs a normalized UI layer to solve the limitations of raw OCR fragments.
The goal is not to pretend OCR can always infer semantic business objects.
Instead, OCR fragments, AX nodes, visual bands, and image detections should be
evidence for a common UI observation shape.

Candidate shape:

```ts
type ObservedUiNode = {
  id: string;
  role?: "window" | "region" | "list" | "listitem" | "row" | "cell" |
    "button" | "text" | "image" | "unknown";
  name?: string;
  value?: string;
  bounds: Rect;
  source: "ax" | "ocr" | "vision" | "merged";
  evidence: ArtifactRef[];
  children?: ObservedUiNode[];
  attributes?: Record<string, unknown>;
};
```

This gives a place to put structure without forcing OCR fragments to become
domain objects. For example, scroll scan can emit:

```text
OCR fragments
  -> list row candidates
    -> list item candidates
      -> observed UI nodes with role=listitem
        -> recipe/parser interprets domain fields
```

The item is not a song, email, file, or table record until a recipe or parser
interprets it.

## WebDriver Compatibility Strategy

AUV can expose a WebDriver-compatible facade for ecosystem compatibility:

```text
POST /session
POST /session/{id}/screenshot
POST /session/{id}/actions
POST /session/{id}/element
POST /session/{id}/element/{elementId}/click
DELETE /session/{id}
```

Mapping:

| WebDriver concept | AUV concept | AUV advantage? |
|---|---|---|
| Remote end | AUV RPC server or adapter | Yes, because AUV can route to multiple device types |
| Session | AUV session | Partial; AUV session should also carry run/artifact context |
| Capabilities | Device/session capabilities | Yes if AUV exposes OCR/AX/vision/artifact capabilities explicitly |
| Element id | Observed UI node/list item/AX node handle | Potential; requires stable observed node model |
| Screenshot | Screenshot artifact | Yes, because AUV can persist and link artifacts |
| Actions | AUV action methods | Partial; WebDriver action model is browser-oriented |
| Execute script | JS orchestration / recipe hook | No direct equivalent yet; should be native JS SDK, not WebDriver-only |

This facade should not define AUV's internal protocol. WebDriver cannot model
AUV's artifact evidence, OCR crop context, visual row bands, run recording, or
multi-device routing cleanly.

## Recommended Near-Term Phases

### Phase 1: Device and Session Skeleton

Add `device_id` and `session_id` to runtime-facing request/recording paths,
with automatic default values:

```text
device_id = "local"
session_id = "default"
```

Do not implement remote devices yet. The value is to make the protocol and
recording model grow in the right direction.

### Phase 2: Session-Scoped Namespaces

Ensure run/artifact/cache paths can be grouped by session while preserving
existing `.auv/runs/{run_id}` compatibility.

### Phase 3: Native RPC Server

Expose a single local RPC endpoint that routes by `device_id`, `session_id`,
and method. Start with local macOS only.

### Phase 4: JS SDK and REPL

Implement handle-based JS APIs and optional REPL globals:

```ts
const device = await auv.device();
const session = await device.sessions.create({ name: "scan" });
const rows = await session.app("com.netease.163music").region("main").list().rows();
```

The REPL can expose globals such as `app`, `screen`, `device`, `session`,
`useDevice`, and `useSession`. Script files should prefer imports from
`@auv/sdk`.

### Phase 5: UI Layer and Recognition Contracts

Project AX nodes, OCR fragments, visual row candidates, and segmented regions
into a normalized observation shape. Keep raw evidence and artifact references.

### Phase 6: Compatibility Adapters

Add optional WebDriver/Appium-like endpoints only after the native protocol is
stable enough to map cleanly.

## Design Principles

- Keep CLI, JS SDK, REPL, MCP, and future UI surfaces on the same runtime
  model.
- Keep manifest/recipe files thin. Avoid turning JSON into a full programming
  language with fragile reference paths.
- Put complex orchestration in JS or another programmable API surface.
- Preserve artifacts and evidence for every observation/action.
- Treat OCR, AX, visual detection, and future model outputs as evidence sources,
  not isolated product features.
- Make device/session/run explicit in protocol and storage.
- Allow convenience defaults in SDK/CLI, but do not let the core protocol depend
  on hidden global state.
- Do not promise true parallel desktop mutation on platforms where input and
  focus are global resources.

## Open Questions

1. Should `Device Profile` be the persisted name and `Device` be the runtime
   resolved target, or is `Device` enough for both?
2. Should AUV use a Unix socket, stdio, HTTP, or all three for the first native
   RPC server?
3. Should the first JS implementation use Node/Bun subprocess JSON-RPC, or an
   embedded runtime later?
4. How should session-scoped action locks be represented in traces?
5. Should `ObservedUiNode` be committed as a public contract now, or remain a
   provisional design term until scroll scan and AX observation both project
   into it?

## Sources

- AUV local docs:
  - `docs/TERMS_AND_CONCEPTS.md`
  - `docs/ai/references/view-memory/2026-05-21-scroll-scan-design.md`
  - `docs/ai/references/ops/2026-05-24-structured-observation-roadmap.md`
  - `docs/ai/references/recognition/2026-05-24-maa-recognition-pipeline-research.md`
- MaaFramework:
  - <https://github.com/MaaXYZ/MaaFramework>
  - <https://github.com/MaaXYZ/MaaFramework/blob/main/docs/en_us/3.1-PipelineProtocol.md>
  - <https://github.com/MaaXYZ/MaaFramework/blob/main/source/MaaFramework/Task/Component/Recognizer.cpp>
  - <https://github.com/MaaXYZ/MaaFramework/blob/main/source/include/MaaAgent/Message.hpp>
- Appium/XCTest:
  - <https://github.com/appium/appium-mac2-driver>
  - <https://appium.github.io/appium.io/docs/en/drivers/mac2/>
  - <https://developer.apple.com/documentation/XCUIAutomation>
- WebDriver:
  - <https://w3c.github.io/webdriver/>
  - <https://www.w3.org/TR/webdriver-bidi/>
  - <https://developer.mozilla.org/en-US/docs/Web/WebDriver>
- Playwright:
  - <https://github.com/microsoft/playwright>
  - <https://playwright.dev/docs/test-fixtures>
  - <https://playwright.dev/docs/api/class-playwright>
- Cua:
  - <https://github.com/trycua/cua>
  - Local inspected path: `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver-rs`
  - Local inspected path: `/Users/neko/Git/github.com/trycua/cua/libs/cua-driver`
