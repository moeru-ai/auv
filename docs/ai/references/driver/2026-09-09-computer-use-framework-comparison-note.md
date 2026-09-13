# AUV / Peekaboo / CUA / kwwk OS automation comparison

> NOTICE: This source review records historical revisions. The [Wayland background input research](2026-09-13-wayland-background-input-research.md) records the 2026-09-13 decision, later implementation PRs, and evidence boundaries. Candidate rows do not authorize implementation.

Research date: 2026-09-09. Scope: docs-only capability inventory, not an implementation plan or approval to expand the driver surface.

The [code and upstream review](2026-09-09-computer-use-code-and-upstream-review.md) adds the newer CUA revision `467c103`, concrete correctness risks, and `EYHN/kwwk-computer-use-core`. The kwwk column covers **main only**. It excludes that independent native component, which AUV already cites. Direct local invoke/MCP and selected Runner dispatch have different platform eligibility. The detailed follow-up records this distinction.

Evidence level: source and first-party documentation review. This comparison includes no builds or live application tests. “Implemented” means that the review found an implementation. It does not establish support for arbitrary applications, desktop sessions, or OS releases. Missing means that the inspected first-party public API lacks the capability. Shell commands and user extensions do not count as built-in APIs.

| Project | Inspected revision | Comparison boundary |
| --- | --- | --- |
| AUV | `bae42bf9905614b19347566d5d41b3a9998e8a35` (local checkout) | Driver libraries, first-party local Runner RPCs, invoke registry, MCP adapter, current reference contracts |
| Peekaboo | `6cd38c0d319ab8db52b05d22409072efc4a20021` | Native macOS automation services, CLI/MCP, command documentation |
| CUA | `00678fa8ec8f0f371716993ae4a207df812ef667` | Current native cua-driver, with VM/agent infrastructure treated separately |
| kwwk | `acc65213baa725b31153609e651f166fdaf79272` | Coding-agent built-in tools and provider implementation |

## Capability comparison

This table records source/documentation evidence. `部分` (Partial) means incomplete platform, frontend, or operation coverage. `未见` (Not found) applies only to the inspected first-party APIs. The kwwk column uses its [built-in tool registration][k-tools], rather than assumptions about the project name.

| Capability | AUV | Peekaboo | CUA | kwwk |
| --- | --- | --- | --- | --- |
| Product role | Typed operations, drivers, run recording, Device/Run/Runner | Native macOS automation CLI/MCP/Swift services/agent [source][p-readme] | Native drivers for three platforms. Separate sandbox, fleet, agent/bench infrastructure. [source][c-readme] | Coding-agent CLI/Swift SDK [source][k-readme] |
| Desktop platforms | macOS / Windows / Linux Wayland implementations. Capabilities differ. | macOS 15+ [source][p-readme] | macOS / Windows / Linux. X11 and different Wayland paths have individual limits. [source][c-support] | No first-party desktop driver |
| Screen/window/region screenshots | Drivers and Runner on three platforms. Linux window capture has crop limits. | screen/window/frontmost/area/multi [source][p-see] | Window state, desktop state, zoom [source][c-tools] | Not found. computerUse/recordScreen provider branches return empty replies. [source][k-placeholder] |
| Multiple displays | Display enumeration, coordinates, and capture on three platforms. Behavior depends on the platform. | Multiple displays and Retina [source][p-see] | Portable desktop display_id guarantees only the primary display. [source][c-tools] | Not found |
| Native OCR | Vision / Windows.Media.Ocr / Tesseract | Apple Vision. OCR rows do not claim actionable AX identity. [source][p-ocr] | No separate OCR API found in cua-driver. It uses accessibility and screenshots. | Not found |
| Basic click/move/keyboard/scroll | Driver implementations exist. General scroll lacks Runner/invoke integration. | Implemented [source][p-automation] | Implemented with platform/toolkit limits [source][c-tools-windows] | Not found |
| Right/middle buttons, drag, hold/release | Some native foundations exist. Shared button parameters and drag/hold contracts are absent. | Right/middle buttons, drag, long press. Move/drag/hold requires foreground input. [source][p-automation] | right_click and drag. Linux also has button down/up. This does not establish parity across three platforms. [source][c-tools-linux] | Not found |
| Targeted background input | macOS implementation. Windows best effort for classic controls. Linux raw input uses the foreground. | AX/PID/window routes. No guarantee of arbitrary background drag/scroll. [source][p-automation] | Semantic/targeted routes on three platforms. Explicit refusals include Electron and Wayland cases. [source][c-support] | Not found |
| Accessibility observation | AX / UIA / AT-SPI implementations. Public entrypoints differ. | Snapshot-bound opaque IDs and AX trees [source][p-see] | Screenshots, AX/UIA/AT-SPI trees, and snapshot-bound tokens [source][c-tools] | Not found |
| Element actions, value writes, selection | Some focus/select/native actions exist. No uniform consumer interface. Windows value writes remain deferred. | action, set-value [source][p-automation] | Semantic click, set_value, text/selection, and related routes [source][c-tools-windows] | Not found |
| App lifecycle | macOS/Windows activation. No complete enumeration/launch/open/quit/relaunch API. | list/launch/open/quit/relaunch/hide/unhide/focus [source][p-app] | list_apps/launch_app/kill_app/bring_to_front [source][c-tools] | Shell process creation, not a typed app API |
| Window management | macOS/Windows move/resize/frame/minimize/restore/zoom. Public access differs. Linux lacks mutations. | Also close and focus, with broader coverage. [source][p-window] | list, set_window_frame, bring_to_front. No dedicated portable minimize/maximize/close tools found. [source][c-tools-windows] | Not found |
| Application menus | No general menu-path API | menu list/click [source][p-menu] | invoke_menu for native menu paths [source][c-tools-linux] | Not found |
| Native dialogs/file pickers | No general API | dialog/input/dismiss/open-save panel [source][p-dialog] | No general native dialog API found. browser_dialog handles only browser JS dialogs. [source][c-browser] | Not found |
| Dock/menu bar/Spaces | No general API found | Dock, menu bar, Spaces. Spaces uses private APIs with best-effort behavior. [Dock][p-dock] / [Spaces][p-spaces] | invoke_menu does not establish complete Dock/Spaces capabilities. | Not found |
| Clipboard | macOS rich snapshots, Windows text/rich snapshots, Linux text. No consistent typed image/file read/write API or separate tools. | text/image/file/raw UTI, read/write/clear/save/restore [source][p-clipboard] | Format/text reads. Text/image/file writes. [source][c-clipboard] | TUI image paste and /copy, not desktop tools [source][k-clipboard] |
| Wait/state verification | OCR wait, activation readback, delivery results. No general predicate service. | `verify` / verify_state with window/element predicates [source][p-verify] | verify_state with window/element predicates and satisfied/unsatisfied/unknown results [source][c-verify] | Agent/tool results, not OS state verification |
| Access methods | Rust, Runner gRPC, CLI, MCP. Capability access differs. | CLI/MCP/Swift services [source][p-readme] | CLI/MCP/Rust/Python/TS with shared runtime [source][c-driver] | CLI/Swift SDK and custom agent tools [source][k-readme] |
| Recording/reuse | Implicit runs/traces/artifacts. These do not establish complete general replay. | Agent sessions/traces/capture. v4 removed the legacy script runner. [source][p-changelog] | Explicit recording/video/trajectory replay. Old tokens/handles are unstable across sessions. [source][c-recording] | JSONL agent sessions and resumption [source][k-session] |
| Browser DOM | No dedicated DOM/CDP service found in inspected driver/APIs | Chrome DevTools MCP integration [source][p-browser] | Typed Chromium/Electron page routes with limited browser coverage [source][c-browser] | External tools are possible, but are not built-in capabilities. |
| VM/cloud isolation/evaluation | Remote device/runner routing exists. This does not constitute VM provisioning. | Mainly host macOS control | Sandbox/Fleets/Lume/Bench are separate strengths. [source][c-readme] | Outside its OS API capabilities |

AUV already has basic desktop drivers. The main gaps concern complete operation sets, semantic/app convenience APIs, and public access to existing capabilities. Peekaboo provides a reference for native macOS API coverage. CUA provides a reference for cross-platform driver contracts, semantic operations, and behavior evidence.

kwwk main occupies a different product layer. Its independent kwwk-computer-use-core component has desktop APIs, as the later source review describes. This table does not imply that the whole KWWK ecosystem lacks computer use.

## AUV findings that change the comparison

AUV has native desktop implementations on macOS, Windows, and Linux. Driver implementation, Runner RPC access, and invoke/MCP access are distinct. CLI command counts understate the implementation. Driver method counts overstate the available automation interfaces. [Local platform dispatch](../../../../crates/auv-driver/src/lib.rs), [Runner implementations](../../../../crates/auv-cli/src/runner/local_driver.rs), [invoke registry](../../../../crates/auv-cli-invoke/src/registry.rs), [MCP adapters](../../../../crates/auv-cli/src/commands/mcp.rs).

| Capability | Driver implementation | Public frontend boundary / remaining gap | Evidence |
| --- | --- | --- | --- |
| Displays and capture | All three drivers list displays and capture displays/regions/windows, with platform-specific limits. | Direct local window/screen invoke/MCP paths remain largely macOS-specific. Selected Runner dispatch supports portable capture/list services. Display operations have separate platform gates. | [dispatch](../../../../crates/auv-cli/src/commands/invoke.rs), [selected invoke](../../../../crates/auv-cli-invoke/src/runner.rs), [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [capture proto](../../../../proto/auv/api/driver/v1/capture.proto) |
| Native OCR | macOS Apple Vision, Windows `Windows.Media.Ocr`, Linux Tesseract. Window text search/polling. | Runner exposes recognition and window/display text search. OCR-oriented invoke commands mainly use macOS paths. | [macOS OCR](../../../../crates/auv-driver-macos/src/native/ocr.rs), [Windows OCR](../../../../crates/auv-driver-windows/src/ocr.rs), [Linux OCR](../../../../crates/auv-driver-linux/src/ocr.rs), [RPC](../../../../proto/auv/api/driver/v1/text_recognition.proto) |
| Left click / multiple clicks / movement | Shared click/movement implementations. Runner also provides timed curve movement. | Shared click parameters lack mouse button selection. The macOS session hardcodes button `0`, although its native wrapper accepts a button code. | [shared input](../../../../crates/auv-driver-common/src/input.rs), [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [native pointer](../../../../crates/auv-driver-macos/src/native/pointer.rs), [input RPC](../../../../proto/auv/api/driver/v1/input.proto) |
| Drag / held input | No complete shared drag or independent button/key down/up contract found. | `PressKeysOptions` explicitly defers key hold until the owner approves cancellation/release semantics. Pointer movement is not drag. | [shared input](../../../../crates/auv-driver-common/src/input.rs), [movement model](../../../../crates/auv-driver-common/src/mouse.rs), [input RPC](../../../../proto/auv/api/driver/v1/input.proto) |
| Scrolling | Window and global scrolling exist in driver APIs. | No standalone Scroll RPC or registered general scroll invoke command. Internal scan scrolling does not fill this API gap. | [shared WindowInput](../../../../crates/auv-driver-common/src/input.rs), [RPC](../../../../proto/auv/api/driver/v1/input.proto), [invoke input group](../../../../crates/auv-cli-invoke/src/commands/input.rs) |
| Keyboard | All three drivers have basic foreground text/key delivery. macOS has ordered keyboard requests with bound targets. | Basic Runner `TypeText`/`PressKey` are portable. Target-bound `InputKeyboard`/`PressKeys` sequences have implementations only on macOS. Invoke typing/key operations also have platform restrictions. | [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [invoke input](../../../../crates/auv-cli-invoke/src/commands/input.rs) |
| Background delivery | macOS PID/window-targeted mouse, wheel, and keyboard. Windows click/scroll through posted messages. | Windows background delivery is best effort for classic controls, without Chromium/Electron/WinUI guarantees. Linux Portal input uses the foreground and rejects BackgroundOnly. | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [Windows boundary](../../../../crates/auv-driver-windows/README.md), [Linux delivery](../../../../crates/auv-driver-linux/src/session.rs) |
| Accessibility tree and actions | macOS AX capture/focus/read and native action wrapper. Windows UIA capture/focus/select with Invoke fallback. Linux AT-SPI capture/focus/select/action. | No uniform public element observation/action contract. Runner AccessibilityService exposes only macOS FocusText. Windows ValuePattern writes remain explicitly deferred. | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [native AX wrapper](../../../../crates/auv-driver-macos/src/native/tree.rs), [Windows UIA](../../../../crates/auv-driver-windows/src/accessibility.rs), [Linux AT-SPI](../../../../crates/auv-driver-linux/src/atspi.rs), [AX RPC](../../../../proto/auv/api/driver/macos/v1/accessibility.proto) |
| Window management | macOS/Windows move/resize/set-frame/minimize/restore/zoom | Runner window service only lists/resolves. The invoke window group lacks mutations. No Linux mutation or common close-window operation found. | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [Windows session](../../../../crates/auv-driver-windows/src/session.rs), [Linux session](../../../../crates/auv-driver-linux/src/session.rs), [window RPC](../../../../proto/auv/api/driver/v1/window.proto) |
| App lifecycle | macOS bundle activation and Windows process-name activation | No complete shared installed/running-app list or explicit launch/open/quit/relaunch/hide API. macOS activation can implicitly launch through AppleScript. This is not a complete lifecycle contract. Windows activation lacks app.activate integration. | [macOS application](../../../../crates/auv-driver-macos/src/application.rs), [Windows application](../../../../crates/auv-driver-windows/src/application.rs), [invoke app](../../../../crates/auv-cli-invoke/src/commands/app.rs) |
| Menus / system dialogs / Dock / Spaces | Application code can use low-level AX. | No reusable general menu-path, native file-panel, Dock, or Spaces service found in the inspected session/Runner/invoke APIs. | [session](../../../../crates/auv-driver-macos/src/session.rs), [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [registry](../../../../crates/auv-cli-invoke/src/registry.rs) |
| Clipboard | macOS snapshots preserve pasteboard item types. Windows has text and memory-backed rich-format snapshots. Linux Portal clipboard supports text only. | Text set and preserve/restore exist. No uniform image/file/raw-format read/write API or standalone Clipboard RPC/invoke group. The opaque macOS String snapshot is not plain text. | [Swift clipboard](../../../../crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Clipboard.swift), [Windows clipboard](../../../../crates/auv-driver-windows/src/clipboard.rs), [Linux clipboard](../../../../crates/auv-driver-linux/src/clipboard.rs), [registry](../../../../crates/auv-cli-invoke/src/registry.rs) |
| Verification and evidence | `InputActionResult` records delivery path, attempts, disturbance, and a verification flag. Explicit app activation readback and OCR waits exist. | Delivery success defaults to unverified. No general shared element/window predicate verification service found. Tracing alone does not prove semantic success. | [input contract](../../../../crates/auv-driver-common/src/input.rs), [app activation](../../../../crates/auv-driver-macos/src/application.rs), [terms](../../../TERMS_AND_CONCEPTS.md) |
| Linux environment coverage | Wayland/Portal capture/input and AT-SPI observation. The repository explicitly prioritizes GNOME/PaperWM. | X11 is deliberately outside this scope. Window capture uses a display crop because Portal window sources lack binding to the requested AT-SPI window. Current pointer position is unsupported. | [Linux README](../../../../crates/auv-driver-linux/README.md), [window capture](../../../../crates/auv-driver-linux/src/window.rs), [pointer position](../../../../crates/auv-driver-linux/src/input.rs) |

## Gap interpretation

The inspected general OS automation gaps form six candidate groups. This analytical inventory gives no completion percentage, effort estimate, or approved roadmap.

| Candidate group | What is missing | Nature of work |
| --- | --- | --- |
| Complete pointer/keyboard operations | Public right/middle button selection, drag, held buttons/keys with reliable release | Shared contract and platform implementation. Some native foundations already exist. |
| App lifecycle | App enumeration, explicit launch/open/quit/relaunch/hide, consistent app targeting | New reusable OS capability APIs and platform integration |
| Semantic element operations | Public tree observation, fresh target identity, common action/value/selection operations, and separate predicate verification | Integration of existing AX/UIA/AT-SPI implementations, then missing actions |
| Menus and native dialogs | Menu paths, context/system menus, native open/save panels. macOS Dock/Spaces are separate platform-specific scope. | New convenience APIs based on native capabilities. Explicit owner-selected scope is required. |
| Consistent access to existing capabilities | Scroll, window mutations, clipboard, and existing tree APIs across Runner/invoke/MCP | Mainly producer-to-consumer integration, with typed direct results and recording |
| Platform behavior coverage | Windows background toolkit limits and targeted keyboard parity. Linux compositor, exact-window capture, and foreground limits. | Backend-specific implementations and reproducible behavior tests. X11 remains an intentional scope exclusion. |

Browser DOM/CDP integration and VM/cloud fleet management are separate comparisons. They do not increase the count of missing native OS primitives. AUV has Device/Run/Runner routing. This does not establish VM provisioning or a general autonomous computer-use agent. Implicit run recording does not establish complete replay/distill/compile behavior. [Current vocabulary](../../../TERMS_AND_CONCEPTS.md).

This research changed no production code and included no runtime tests.

[p-readme]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/README.md
[p-see]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/see.md
[p-ocr]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/Observation/ObservationOCRService.swift
[p-automation]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/automation.md
[p-app]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/app.md
[p-window]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/window.md
[p-menu]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/menu.md
[p-dialog]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/dialog.md
[p-dock]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/dock.md
[p-spaces]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/space.md
[p-clipboard]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/clipboard.md
[p-verify]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/verify.md
[p-changelog]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/CHANGELOG.md
[p-browser]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/docs/commands/browser.md
[c-readme]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/README.md
[c-driver]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/README.md
[c-tools]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/docs/content/docs/reference/cua-driver/mcp-tools.mdx
[c-tools-windows]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/docs/content/docs/reference/cua-driver/mcp-tools-windows.mdx
[c-tools-linux]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/docs/content/docs/reference/cua-driver/mcp-tools-linux.mdx
[c-support]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/docs/action-support.md
[c-clipboard]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/rust/crates/cua-driver-core/src/clipboard.rs
[c-verify]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/rust/crates/cua-driver-contract/src/verification.rs
[c-browser]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/rust/Skills/cua-driver/BROWSER.md
[c-recording]: https://github.com/trycua/cua/blob/00678fa8ec8f0f371716993ae4a207df812ef667/libs/cua-driver/rust/Skills/cua-driver/RECORDING.md
[k-readme]: https://github.com/EYHN/kwwk/blob/acc65213baa725b31153609e651f166fdaf79272/README.md
[k-tools]: https://github.com/EYHN/kwwk/blob/acc65213baa725b31153609e651f166fdaf79272/Sources/KWWKAgent/CodingAgentBuilder.swift
[k-placeholder]: https://github.com/EYHN/kwwk/blob/acc65213baa725b31153609e651f166fdaf79272/Sources/KWWKAI/Cursor/CursorAgentProvider.swift#L585-L592
[k-clipboard]: https://github.com/EYHN/kwwk/blob/acc65213baa725b31153609e651f166fdaf79272/Sources/KWWKCli/Clipboard.swift
[k-session]: https://github.com/EYHN/kwwk/blob/acc65213baa725b31153609e651f166fdaf79272/Sources/KWWKAgent/SessionStore.swift
