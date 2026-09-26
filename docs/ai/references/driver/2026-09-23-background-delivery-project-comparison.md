# BG-2: background delivery in CUA, Sky, KWWK, and Maa

Date: 2026-09-23. Classification: docs-only research.

This note follows [BG-2 in the background-input review](2026-09-09-background-ax-and-media-gap-review.md). It separates transport selection, generated click count, target consumption, and semantic verification. The research examined source and test definitions; it did not run GUI receivers or establish OS/toolkit support.

Subsequent owner-approved AUV implementation and live probes are recorded separately
in [background keyboard authentication](2026-09-23-background-keyboard-authentication.md).

## Revisions and scope

- CUA: remote `main` resolved through the GitHub API to `d1a01f8580d5963702427b9e110fbcd98c39fac3` (2026-09-22). The implementation examined below is its Rust macOS driver. The historical September 9 review used `467c103be28384502cdd77b9edd5ea46da0b8ded`; do not substitute a stale local checkout for either revision.
- KWWK: `EYHN/kwwk-computer-use-core` remote `main` remains `5201e300ceb58f2aaf501b20477aff5a912efb9a` (2026-08-05). This is the extracted native core, not a claim about all releases of the KWWK application.
- Sky and Maa: see their sections below for inspected versions and boundaries.

## CUA: single delivery for the window-left-click path, other dual-post paths remain

For a left click with window-local coordinates, `WindowClickDelivery::Background` selects the Chromium-compatible sequence. `Foreground` selects a public-PID-only sequence. The background recipe calls SkyLight once, and calls the public API only when the SkyLight symbol is absent. The low-level success boolean means “post attempted,” not receipt, and missing authentication support does not trigger a second public delivery. [Window route selection](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L259-L289), [background post](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L491-L496), [SPI return contract](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L308-L351).

This is **not a project-wide elimination of dual posting**. `post_mouse_event` still selects `MousePostMode::Both`; that branch calls SkyLight and public PID posting unconditionally. The right-click path calls this helper, including when a window ID is supplied. The middle-click path also calls this helper ([middle click](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L936-L973)). The PID-only left-click entry also selects `Both`; scroll still has explicit dual posting. Therefore “CUA uses one transport” is too broad. [Default helper and transport branch](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L1094-L1161), [right click](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L978-L1064), [PID-only left click](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L47-L54), [scroll](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L1292-L1298).

The Chromium-compatible left-click recipe still clamps `count` to 1–2, then emits that many target down/up pairs with click-state values 1 then 2. It also emits a move and an off-screen primer down/up before the target pairs. The ordinary left-click loop uses the requested count. Consequently CUA supplies a narrower example of choosing a single transport, **not a complete solution for AUV's count-cap issue**. [Cap and primer](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L450-L457), [sequence](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L498-L564), [ordinary loop](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L334-L389).

Keyboard transport uses a different rule: background key events prefer SkyLight with an `SLSEventAuthenticationMessage`, and fall back to public PID posting only when the post symbol is unavailable. The helper does not choose a transport by toolkit. The authentication factory selector is guarded: when absent on macOS 14, CUA skips the envelope but still posts through SkyLight; its comment explicitly warns that Chromium-class targets may not receive the event. This prevents a crash, not silent input loss. [Keyboard helper](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs#L591-L598), [availability guard](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L321-L351).

CUA also has unauthenticated key helpers for NSMenu handling and a foreground key route that activates the exact window and sends HID transitions. Its `type_text` result logic treats AXValue-only readback under an AXWebArea as insufficient renderer evidence, retaining `verified:false`. These are distinct action/surface policies; they must not be described as a universal authenticated background keyboard guarantee. [Unauthenticated helper](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/keyboard.rs#L614-L630), [foreground/background key selection](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/tools/press_key.rs#L414-L449), [web-content verification boundary](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/tools/type_text.rs#L424-L445).

CUA's E2E definitions require external oracles and structured refusal codes, and evaluate observed evidence against those declarations. This is useful acceptance machinery, but the presence of the machinery alone does not prove that each BG-2 route, every click count, or every toolkit passed. [Oracle requirements](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/cua-driver-testkit/src/e2e.rs#L573-L611), [result evaluation](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/cua-driver-testkit/src/e2e.rs#L785-L816).

## KWWK: public PID posting, plus an activation primer

KWWK's mouse dispatcher stamps the target PID, window routing fields, and window-local coordinates, then calls public `CGEvent.postToPid` once per event. Its click API emits one down/up pair with click-state 1; it does not expose a multi-click count parameter. This avoids the particular dual-transport construction, but does not demonstrate an arbitrary multi-click solution. [Mouse construction and post](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L137-L171), [single-click implementation](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L60-L66), [public action signature](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L96-L103).

Keyboard Unicode and key-combination events also use public `postToPid` after PID/window stamping. The examined dispatcher has no SkyLight authentication envelope or toolkit-selecting transport branch. This is an implementation boundary, not evidence that plain PID keyboard delivery works in every target. [Text and key construction](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L273-L335), [keyboard post](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundInputDispatcher.swift#L338-L342).

There is an important qualification to “one requested click”: KWWK conditionally prepares background activation by sending an AppKit-defined activation event **and a real left down/up primer at the window center**. It does this when its session decides activation is needed; it is not a duplicate post of the requested event. A receiver test must count and distinguish this extra pair, since single transport alone does not imply only one generated click or no side effects. [Activation decision](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseSession.swift#L267-L305), [activation and center primer](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundActivationSession.swift#L74-L95), [primer down/up](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/BackgroundActivationSession.swift#L129-L171).

Element clicks can instead use an AX action; coordinate fallback is conditional on element policy or the AX action return. That semantic route is separate from raw input transport. [Click dispatch](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L144-L164).

KWWK includes an opt-in AppKit probe suite. It checks coordinate click consumption, typed text, logged mouse-down coordinates, click-count deltas, cursor position, and foreground state. It requires a GUI-test environment variable, Accessibility permission, and prebuilt probe apps. This research read those assertions but did not execute them; their existence is not a cross-toolkit or cross-version pass claim. [Probe prerequisites and cases](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Tests/KWWKComputerUseCoreTests/InProcessComputerUseBehaviorTests.swift#L146-L185), [coordinate click count](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Tests/KWWKComputerUseCoreTests/InProcessComputerUseBehaviorTests.swift#L215-L249), [receiver/focus assertions](https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Tests/KWWKComputerUseCoreTests/InProcessComputerUseBehaviorTests.swift#L953-L973).

## Locally bundled OpenAI Sky

Evidence is limited to the installed `@oai/sky` 0.7.1 documentation and JavaScript
wrapper. This is not `trycua/cua`. No native receiver experiment or native
implementation inspection was performed. No Swift/Objective-C/Rust implementation
source was found in the inspected installed package.

Package root:
`/Applications/ChatGPT.app/Contents/Resources/cua_node/lib/node_modules/@oai/sky`.
Sources relative to that root:

- `docs/sky-window-api.md:46`: app-targeted click accepts `click_count`, button,
  and coordinates or an element index; key and text actions are app-targeted.
- `dist/project/cua/sky_js/src/targets/mac/click.js:1`: passes `click_count` to
  `MacComputerUseClient.click` as `clickCount` without a two-click cap.
- `dist/project/cua/sky_js/src/targets/mac/client.js:1`: defaults an omitted
  count to 1, preserves a supplied count, and serializes click, pressKey, and
  type through `ComputerUseIPCAppPerformActionRequest`.
- `dist/project/cua/sky_js/src/targets/mac/native-pipe.js:1`: transfers requests
  to the native service. JavaScript request count is not native CGEvent count.

Therefore native single versus dual posting, a native click-count cap, toolkit
selection, authentication envelopes, and exactly-once receipt remain **unknown**.
The wrapper cannot establish that Sky solves BG-2. Full-desktop/Linux declarations
must not be substituted for the macOS app/window implementation.

For reproducibility, SHA-256 of the inspected files:

```text
package.json
 d52f502c135eae23994c0ca295640cd7a8f01b18a85e307c968fecae8d3a42d6
src/targets/mac/client.js (under dist/project/cua/sky_js/)
 b5addc3c85ff0124095042832bca0fe1a271d119925459df7bd284dcab1931ab
src/targets/mac/click.js (under dist/project/cua/sky_js/)
 020901282209d67583ba6316f0a09bc00a694fd394d705105821d047b22da0a4
```

## MaaFramework

Inspected remote HEAD: `b96ab05ace1c14533144fedfcbb7358dad785579`, resolved through
the GitHub API on 2026-09-23. The macOS source files below matched the existing
local checkout byte-for-byte; Windows and ControllerAgent files were read from
that fixed remote revision. This is source evidence, without native execution.

MaaFramework now has a macOS controller. Its manager selects **one** configured
input implementation: `GlobalEvent` or `PostToPid`. It does not attempt both
backends per click. This is explicit backend configuration, not observed
consumption-based fallback or toolkit detection.
[Manager selection](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Manager/MacOSControlUnitMgr.cpp#L42-L55).

The PostToPid mouse path creates an NSEvent with the window number and local
coordinates, converts it to CGEvent, and calls `CGEventPostToPid` once. It stamps
`clickCount:1`. The backend advertises down/up composition; ControllerAgent
implements a click with touch-down, a 50 ms pause, and release. Consequently this
is a single-click primitive, not a reference for preserving native double/triple
click state. Repeated single clicks must not be described as proven multi-click
semantics.
[Mouse event construction](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Input/PostToPidInput.mm#L202-L227),
[feature flags](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Input/PostToPidInput.mm#L15-L27),
[controller click composition](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaFramework/Controller/ControllerAgent.cpp#L447-L467).

Keyboard down/up and Unicode text also use plain `CGEventPostToPid`, with no
SkyLight authentication or toolkit branch in these inspected paths. Text attaches
UTF-16 data to a down and an up event. Return success follows posting; this code
contains no independent receiver acknowledgement.
[Text](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Input/PostToPidInput.mm#L106-L129),
[key events](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Input/PostToPidInput.mm#L230-L239).

The single-mouse-event statement must not be generalized to every Maa action:
its scroll method constructs and posts two distinct events through the same
public API. This is not AUV's same-event/SkyLight-plus-public mouse double post.
[Scroll sequence](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaMacOSControlUnit/Input/PostToPidInput.mm#L154-L189).

Windows follows the configurable-backend approach too. Mouse and keyboard can
select different methods. `MessageInput::send_or_post_w` chooses PostMessageW
**or** SendMessageW according to configuration; it does not send the same message
through both. Other configured methods include Seize and LegacyEvent. This is
useful for backend ownership and explicit selection, but does not establish
macOS Chromium compatibility or control consumption.
[Separate selection](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaWin32ControlUnit/Manager/Win32ControlUnitMgr.cpp#L76-L83),
[backend construction](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaWin32ControlUnit/Manager/Win32ControlUnitMgr.cpp#L139-L149),
[exclusive send/post branch](https://github.com/MaaXYZ/MaaFramework/blob/b96ab05ace1c14533144fedfcbb7358dad785579/source/MaaWin32ControlUnit/Input/MessageInput.cpp#L96-L119).

## Implications for AUV

CUA's background window-left-click path is a concrete example of selecting one transport per event while retaining the compatibility sequence. It still has a two-click cap, and other paths retain dual posting. KWWK shows plain PID posting with window metadata and independent probe assertions, but its activation primer can add another physical click pair. Maa shows explicit backend selection with single-click primitives; its plain PID keyboard path does not add the CUA authentication mechanism. Sky exposes the requested count at the JavaScript boundary, but its native delivery remains unknown in this inspection. None establishes that AUV can remove compatibility input without testing its existing consumers.

The smallest useful BG-2 comparison remains: use an independent receiver to distinguish primer events from target events; count down/up and click-state for one, two, and three requested clicks; compare ordinary PID and compatibility delivery; verify the target result separately. Keyboard needs its own target/toolkit evidence. These are candidate validation criteria, not approval to introduce another delivery route.
