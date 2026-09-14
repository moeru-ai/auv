# iPhone phone-use backend source review

Date: 2026-09-14. Change classification: docs-only external-project research.

## Scope and evidence

This review covers eight iPhone projects and the DeviceKit implementation that mobilecli uses.
The review added seven clones under `~/Git/github.com/<owner>/<repo>`.
The existing mobilecli and devicekit-ios checkouts were clean.
For both repositories, `git pull --ff-only` reported no new commits.

Evidence level: **source inspected**. This note separates source findings from hardware reports in upstream comments and READMEs.
The review did not run builds, phone installation, signing, hardware automation, performance measurements, or end-to-end tests.
This note records external research. It does not establish AUV platform support or approve implementation.

An AX tree is optional. The baseline consists of screenshots, coordinate input, text input, and observation after each action.

The [platform survey](2026-09-13-mobile-use-platform-backends-research.md) covers Android, Harmony, and the wider agent frameworks.

## Revision inventory

| Repository / local relative path | Inspected commit |
|---|---|
| `ShawnPana/phone-harness` | `6e47f24cd1d6110e9c72dc2bfe559d2ec709e56d` |
| `yev-yev-yev/iphone-mcp` | `e56e78e281a4c564a28c82e3cfa481411c647380` |
| `jfarcand/mirroir-mcp` | `e1cd98aa872f9a7d61623920cb8b9e6806b2de2e` |
| `mobile-next/mobilecli` | `f7148582b01aff489f006ad6e62c5fb21dd0785d` |
| `mobile-next/devicekit-ios` | `510d10e5e376221397cef7f8d7acb74603c61888` |
| `leeguooooo/iphone-use` | `8188abd3ebf693c278d1d8239c0e4bdae3658076` |
| `ucsandman/sidetap` | `0c75c53f672d07cfc8634969d246ece9194af37d` |
| `appium/appium-mcp` | `541dbd62cfbed9a86ea283a8f2ecd8edabecb497` |
| `ikatkov/iphone-safari-mcp` | `093df46ddb7fa4124195ff07dffa9b1d1d4f2d05` |

## Comparison

| Project | Execution surface | Capture | Input / text | Setup and runtime boundary |
|---|---|---|---|---|
| phone-harness | Real iPhone through Mac iPhone Mirroring. Python/CLI/skill | Window capture + Vision OCR | SkyLight process/window mouse events. Targeted keyboard. Clipboard paste | Mirroring and Mac permissions. Background scroll still raises window and borrows cursor |
| iphone-mcp | Real iPhone through Mirroring. TypeScript MCP | screencapture + sharp | CGEvent through JXA, keystrokes and clipboard segments | Activates Mirroring for input. No phone test runner |
| mirroir-mcp | Real iPhone through Mirroring. Swift MCP and exploration/replay tools | Window-ID capture with region fallback. Vision OCR | CGEvent. Key mapping and whole-text paste for unmappable characters | Capture activates target. Unicode paste depends on Universal Clipboard |
| mobilecli + devicekit-ios | Real iPhone and Simulator. CLI/HTTP JSON-RPC | XCUIScreen screenshot. Separate streaming paths | XCTest synthesized pointer/text events | Signed XCTest runner and developer connection. Go-ios testmanagerd launch |
| iphone-use | Real iPhone. Rust daemon, HTTP/MCP/browser | WDA PNG/MJPEG | W3C pointer actions. WDA Unicode keys | Default direct WDA backend. Xcode runner lifecycle and USB forwarding |
| SideTap | Real iPhone from Windows. Python/MCP/browser | WDA screenshot/MJPEG | W3C pointer actions. WDA keys | go-ios tunnel/runwda/forward. Signing including nested XCTest bundle |
| appium-mcp | Real iPhone and Simulator through Appium. MCP | WebDriver screenshot, resize/artifact output | Element or coordinate actions. Focused W3C keyboard option | Appium/XCUITest/WDA. Real-device preparation signs prebuilt WDA |
| iphone-safari-mcp | Real iPhone Safari web content. Python MCP | WebDriver viewport screenshot | DOM click/send_keys. Touch actions. JS scroll | safaridriver with platformName=ios, trusted device and Safari automation configuration |

## Implementation findings

### phone-harness

`ios._load_transport` selects the background module by default. If the module fails to load, it selects the foreground module.
The background mouse path creates a SkyLight event record with process identifiers, window identifiers, and coordinates.
It sends this record through `SLPSPostEventRecordTo`.

Keyboard delivery makes the target window key and uses `CGEventPostToPid`.
Mirroring forwards these Mac events to iOS. The file header describes an older keyboard fallback.
The current function bodies contain the implementation described here.

Background input can still disturb the desktop. `scroll_wheel` activates and raises Mirroring, then checks the window at the target point.
It moves the real cursor and sends global wheel events. Then it restores the cursor and the previous application.

The text helper uses the Mac clipboard and Cmd+V by default.
Its fixed delay does not establish that the text reached the phone.
The helper leaves the new clipboard contents in place by default. An early restore can cause the phone to paste previous contents.

[ShawnPana/phone-harness: src/phone_harness/ios.py](https://github.com/ShawnPana/phone-harness/blob/6e47f24cd1d6110e9c72dc2bfe559d2ec709e56d/src/phone_harness/ios.py#L39). [ShawnPana/phone-harness: src/phone_harness/background.py](https://github.com/ShawnPana/phone-harness/blob/6e47f24cd1d6110e9c72dc2bfe559d2ec709e56d/src/phone_harness/background.py#L165). [ShawnPana/phone-harness: src/phone_harness/mirror.py](https://github.com/ShawnPana/phone-harness/blob/6e47f24cd1d6110e9c72dc2bfe559d2ec709e56d/src/phone_harness/mirror.py#L615)

### iphone-mcp

The capture path runs `screencapture -l` and processes the image with sharp.
JXA creates pointer events through CoreGraphics.
`typeText` activates Mirroring and divides the text into keystroke, physical-keycode, and paste segments.
Each paste segment sets the clipboard, waits 0.1 seconds, presses Cmd+V, and waits again.

Repeated Unicode segments need a synchronization test on a real device.
Source inspection alone does not establish a failure. This project provides a small implementation of visual control.

[yev-yev-yev/iphone-mcp: src/lib/mirroring/mirroring-client.ts](https://github.com/yev-yev-yev/iphone-mcp/blob/e56e78e281a4c564a28c82e3cfa481411c647380/src/lib/mirroring/mirroring-client.ts#L161)

### mirroir-mcp

The Swift implementation includes OCR, navigation graphs, exploration, and compiled scenarios.
Its capture function activates the target and tries capture by window ID.
If this capture fails, it captures a screen region. Region capture depends on the visible content at those coordinates.

If text contains characters without keyboard mappings, the implementation pastes the entire text once.
This avoids repeated clipboard changes.
The result warns about the Universal Clipboard dependency.
The implementation cannot read the phone clipboard to check synchronization.
A successful Cmd+V dispatch does not establish successful text entry.

[jfarcand/mirroir-mcp: Sources/mirroir-mcp/ScreenCapture.swift](https://github.com/jfarcand/mirroir-mcp/blob/e1cd98aa872f9a7d61623920cb8b9e6806b2de2e/Sources/mirroir-mcp/ScreenCapture.swift#L28). [jfarcand/mirroir-mcp: Sources/mirroir-mcp/InputSimulationKeyboard.swift](https://github.com/jfarcand/mirroir-mcp/blob/e1cd98aa872f9a7d61623920cb8b9e6806b2de2e/Sources/mirroir-mcp/InputSimulationKeyboard.swift#L106). [jfarcand/mirroir-mcp: Sources/mirroir-mcp/AppleVisionTextRecognizer.swift](https://github.com/jfarcand/mirroir-mcp/blob/e1cd98aa872f9a7d61623920cb8b9e6806b2de2e/Sources/mirroir-mcp/AppleVisionTextRecognizer.swift#L14)

### mobilecli and devicekit-ios

DeviceKit replaces WDA but retains the XCTest permissions and test-session requirements.
`IOSDevice.LaunchTestRunner` starts the developer tunnel.
Then it runs go-ios `testmanagerd.RunTestWithConfig` with `devicekit-iosUITests.xctest`.
Some host comments and log messages retain the WDA name.

DeviceKit exposes HTTP/WebSocket JSON-RPC.
Its tap handler creates an EventRecord and calls RunnerDaemonProxy.
The proxy calls `_XCT_synthesizeEvent:completion:`.

Text input uses synthesized text paths. It sends the first character slowly, pauses for 0.5 seconds, and sends the remaining text faster.
The screenshot RPC uses `XCUIScreen.main.screenshot()` and returns PNG/JPEG data.
Coordinate taps do not require AX element lookup.

The separate mobilecli `ios_device_agent.go` path uses LLDB to inject an agent into an app for additional inspection.
This optional path differs from the standard DeviceKit driver for touch and text.
Its presence does not establish access to arbitrary apps without debug permissions.

[mobile-next/mobilecli: devices/ios.go](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/ios.go#L623). [mobile-next/devicekit-ios: DeviceKitTests/JSONRPC/Handlers/IOTap.swift](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/JSONRPC/Handlers/IOTap.swift#L17). [mobile-next/devicekit-ios: DeviceKitTests/XCTest/RunnerDaemonProxy.swift](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/XCTest/RunnerDaemonProxy.swift#L48). [mobile-next/devicekit-ios: DeviceKitTests/JSONRPC/Handlers/IOText.swift](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/JSONRPC/Handlers/IOText.swift#L43). [mobile-next/devicekit-ios: DeviceKitTests/JSONRPC/Handlers/Screenshot.swift](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/JSONRPC/Handlers/Screenshot.swift#L30). [mobile-next/mobilecli: devices/ios_device_agent.go](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/ios_device_agent.go#L136)

### iphone-use

The default backend is Direct/WDA. The mirror backend remains an explicit compatibility mode.
Coordinate taps, long presses, drags, and swipes use W3C pointer actions.
Text uses `/wda/keys`, and PNG capture uses `/screenshot`.

These direct operations do not require a focused Mirroring window.
The configuration scripts manage the Xcode test runner and USB iproxy relays.
The device still needs a usable developer connection and test session.

The implementation includes session health, backend selection, action results, and lifecycle management.
The upstream README reports hardware checks for individual functions.
It also records incomplete acceptance tests for the browser interface.
This review does not establish an end-to-end hardware result.

[leeguooooo/iphone-use: crates/server/src/wda.rs](https://github.com/leeguooooo/iphone-use/blob/8188abd3ebf693c278d1d8239c0e4bdae3658076/crates/server/src/wda.rs#L629). [leeguooooo/iphone-use: crates/server/src/wda.rs](https://github.com/leeguooooo/iphone-use/blob/8188abd3ebf693c278d1d8239c0e4bdae3658076/crates/server/src/wda.rs#L969). [leeguooooo/iphone-use: crates/server/src/config.rs](https://github.com/leeguooooo/iphone-use/blob/8188abd3ebf693c278d1d8239c0e4bdae3658076/crates/server/src/config.rs#L60). [leeguooooo/iphone-use: scripts/setup-wda.sh](https://github.com/leeguooooo/iphone-use/blob/8188abd3ebf693c278d1d8239c0e4bdae3658076/scripts/setup-wda.sh#L164)

### SideTap

SideTap implements device configuration and lifecycle management on Windows.
Its go-ios wrapper manages the tunnel, runwda, and port forwards.
The signing module processes the nested XCTest bundle.
A signature on only the outer app can leave this bundle unusable.

A free account still requires an Apple signing workflow with user participation.
The provisioning profile still needs renewal.

The WDA client uses pointer actions for gestures and `/wda/keys` for text.
Clipboard operations use separate calls. These calls temporarily activate the runner to access the clipboard.
Screenshot and text calls do not require AX tree lookup.

[ucsandman/sidetap: src/phone_harness/device.py](https://github.com/ucsandman/sidetap/blob/0c75c53f672d07cfc8634969d246ece9194af37d/src/phone_harness/device.py#L476). [ucsandman/sidetap: src/phone_harness/signing.py](https://github.com/ucsandman/sidetap/blob/0c75c53f672d07cfc8634969d246ece9194af37d/src/phone_harness/signing.py#L1). [ucsandman/sidetap: src/phone_harness/wda_client.py](https://github.com/ucsandman/sidetap/blob/0c75c53f672d07cfc8634969d246ece9194af37d/src/phone_harness/wda_client.py#L547)

### appium-mcp

The coordinate tap branch calls W3C performActions without an element ID.
Text can target the focused field through the w3cActions configuration.
The locator tools do not prevent a visual agent loop.
The screenshot tool saves or resizes images, or returns image content.
Image dimensions and action coordinates can use different scales.

This tool prepares real iOS devices only on macOS.
It locates provisioning profiles, downloads WDA, creates a package, and signs the IPA.
Then it returns the capabilities for the Appium session.
This automates preparation but retains the Appium/XCUITest dependency.

[appium/appium-mcp: src/tools/gestures/handlers/tap.ts](https://github.com/appium/appium-mcp/blob/541dbd62cfbed9a86ea283a8f2ecd8edabecb497/src/tools/gestures/handlers/tap.ts#L12). [appium/appium-mcp: src/tools/interactions/set-value.ts](https://github.com/appium/appium-mcp/blob/541dbd62cfbed9a86ea283a8f2ecd8edabecb497/src/tools/interactions/set-value.ts#L22). [appium/appium-mcp: src/tools/ios/prepare-ios-real-device.ts](https://github.com/appium/appium-mcp/blob/541dbd62cfbed9a86ea283a8f2ecd8edabecb497/src/tools/ios/prepare-ios-real-device.ts#L389)

### iphone-safari-mcp

The Selenium Remote session sets `platformName=ios` in the Safari configuration.
It provides screenshots, DOM operations, JavaScript evaluation, and touch actions at CSS viewport coordinates.
Scroll uses JavaScript. This interface controls Safari web content, not the whole phone.
The screenshot tool can resize the viewport image. Its pixels can differ from device screen pixels.

[ikatkov/iphone-safari-mcp: src/iphone_safari_mcp/server.py](https://github.com/ikatkov/iphone-safari-mcp/blob/093df46ddb7fa4124195ff07dffa9b1d1d4f2d05/src/iphone_safari_mcp/server.py#L197). [ikatkov/iphone-safari-mcp: src/iphone_safari_mcp/server.py](https://github.com/ikatkov/iphone-safari-mcp/blob/093df46ddb7fa4124195ff07dffa9b1d1d4f2d05/src/iphone_safari_mcp/server.py#L334)

## Conclusions and follow-up probes


- phone-harness and iphone-mcp require no test runner on the phone. Both require Mac iPhone Mirroring.

- DeviceKit and iphone-use control the device without a desktop window. Both retain signing and XCTest session requirements.

- SideTap provides a Windows implementation of device configuration and control.

- SafariDriver provides a separate route for web content.

- These projects do not establish unrestricted control of other apps through an ordinary App Store app.

The following hardware probes remain open:


- Mixed Chinese, ASCII, and emoji input after a clipboard change.

- Screenshot coordinates after image resize or device rotation.

- phone-harness scroll with another window over Mirroring.

- Runner recovery after USB disconnect.

- A fresh screenshot after each action.

These probes address the visual control baseline. They do not require an AX tree.
