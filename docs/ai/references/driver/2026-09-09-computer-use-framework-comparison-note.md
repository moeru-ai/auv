# AUV / Peekaboo / CUA / kwwk OS automation comparison

> NOTICE: Historical source review at the revisions recorded below. For the 2026-09-13 decision, subsequent implementation PRs, and Wayland evidence boundaries, read [Wayland background input research](2026-09-13-wayland-background-input-research.md). Candidate rows are not implementation approval.

Research date: 2026-09-09. Scope: docs-only capability inventory, not an implementation plan or approval to expand the driver surface.

Follow-up: [code and upstream review](2026-09-09-computer-use-code-and-upstream-review.md) qualifies this inventory with newer CUA commit `467c103`, concrete correctness risks, and `EYHN/kwwk-computer-use-core`. The kwwk column below covers **main only**, not that independent native component already cited by AUV. Direct local invoke/MCP and selected Runner dispatch also have different platform eligibility; the detailed follow-up records this distinction.

Evidence level: current source and first-party documentation inspection. No framework was built or exercised against live applications in this comparison. “Implemented” below means an implementation was found, not that arbitrary applications, desktop sessions, or OS releases are supported. Missing means no first-party public capability was found in the inspected surface; shell commands and user extensions are not counted as built-in APIs.

| Project | Inspected revision | Comparison boundary |
| --- | --- | --- |
| AUV | `bae42bf9905614b19347566d5d41b3a9998e8a35` (local checkout) | Driver libraries, first-party local Runner RPCs, invoke registry, MCP adapter, current reference contracts |
| Peekaboo | `6cd38c0d319ab8db52b05d22409072efc4a20021` | Native macOS automation services, CLI/MCP, command documentation |
| CUA | `00678fa8ec8f0f371716993ae4a207df812ef667` | Current native cua-driver, with VM/agent infrastructure treated separately |
| kwwk | `acc65213baa725b31153609e651f166fdaf79272` | Coding-agent built-in tools and provider implementation |

## 横向能力表

下表均为源码/文档级结论。`部分` 表示平台、入口或操作集合不完整；`未见` 限于检查过的第一方 API。kwwk 列的依据是其 [built-in tool registration][k-tools]，而非根据项目名称推测。

| 能力 | AUV 当前 | Peekaboo 当前 | CUA 当前 | kwwk 当前 |
| --- | --- | --- | --- | --- |
| 定位 | Typed operations、drivers、run recording、Device/Run/Runner | macOS 原生自动化 CLI/MCP/Swift services/agent [来源][p-readme] | 三平台 native driver；另有 sandbox、fleet、agent/bench [来源][c-readme] | Coding-agent CLI/Swift SDK [来源][k-readme] |
| 桌面平台 | macOS / Windows / Linux Wayland 实现；能力不等价 | macOS 15+ [来源][p-readme] | macOS / Windows / Linux，X11 和多种 Wayland 路径有分项限制 [来源][c-support] | 没有第一方桌面 driver |
| 屏幕/窗口/区域截图 | 三平台 driver + Runner；Linux 窗口有裁剪限制 | screen/window/frontmost/area/multi [来源][p-see] | window state、desktop state、zoom [来源][c-tools] | 未见；computerUse/recordScreen provider 分支为空回复 [来源][k-placeholder] |
| 多显示器 | 三平台 display 列举、坐标与 capture；行为依平台 | 多屏与 Retina [来源][p-see] | portable desktop display_id 仅保证 primary [来源][c-tools] | 未见 |
| 原生 OCR | Vision / Windows.Media.Ocr / Tesseract | Apple Vision；OCR 行不冒充可操作 AX 元素 [来源][p-ocr] | cua-driver 未见独立 OCR API；用 accessibility + screenshot | 未见 |
| 基础点击/移动/键盘/滚动 | driver 已有；general scroll 未接通 Runner/invoke | 已有 [来源][p-automation] | 已有，按平台和 toolkit 限制 [来源][c-tools-windows] | 未见 |
| 右/中键、拖拽、按住释放 | 部分 native 基础存在；共享 button 参数、drag/hold 契约缺失 | 右/中键、drag、long press；move/drag/hold 要求 foreground [来源][p-automation] | right_click、drag；Linux 另有 button down/up，不能算三平台一致 [来源][c-tools-linux] | 未见 |
| 后台定向输入 | macOS 有；Windows classic-control best effort；Linux 原始输入为 foreground | AX/PID/window routes；不保证任意后台 drag/scroll [来源][p-automation] | 三平台语义/定向 routes；Electron、Wayland 等有明确 refusal [来源][c-support] | 未见 |
| Accessibility 观察 | AX / UIA / AT-SPI 已有，公开入口不统一 | snapshot-bound opaque IDs 与 AX tree [来源][p-see] | screenshot + AX/UIA/AT-SPI tree + snapshot-bound token [来源][c-tools] | 未见 |
| 元素动作/写值/选择 | 有 focus/select/native action 片段；缺统一可消费接口；Windows 写值 deferred | action、set-value [来源][p-automation] | click、set_value、text/selection 等语义路径 [来源][c-tools-windows] | 未见 |
| App 生命周期 | macOS/Windows activate；缺完整列举/launch/open/quit/relaunch API | list/launch/open/quit/relaunch/hide/unhide/focus [来源][p-app] | list_apps/launch_app/kill_app/bring_to_front [来源][c-tools] | Shell 启动进程，不是 typed app API |
| 窗口管理 | macOS/Windows move/resize/frame/minimize/restore/zoom，尚未统一暴露；Linux 缺 mutation | 加上 close、focus，完整度较高 [来源][p-window] | list、set_window_frame、bring_to_front；未见 portable minimize/maximize/close 专用工具 [来源][c-tools-windows] | 未见 |
| 应用菜单 | 缺 general menu-path API | menu list/click [来源][p-menu] | invoke_menu 原生菜单路径 [来源][c-tools-linux] | 未见 |
| 原生对话框/文件选择器 | 缺 general API | dialog/input/dismiss/open-save panel [来源][p-dialog] | 未见 general native dialog API；browser_dialog 仅浏览器 JS dialog [来源][c-browser] | 未见 |
| Dock/菜单栏/Spaces | 未见 general API | Dock、menubar、Spaces；Spaces 私有 API/best effort [Dock][p-dock] / [Spaces][p-spaces] | 不应从 invoke_menu 推导完整 Dock/Spaces 能力 | 未见 |
| 剪贴板 | macOS rich snapshot、Windows text/rich snapshot、Linux text；缺一致 typed image/file read/write 和独立工具 | text/image/file/raw UTI、read/write/clear/save/restore [来源][p-clipboard] | read formats/text；write text/image/file [来源][c-clipboard] | TUI 图片粘贴和 /copy，不是 desktop tool [来源][k-clipboard] |
| 等待/状态验证 | OCR wait、激活 readback、投递结果；缺 general predicate service | verify / verify_state，window/element predicates [来源][p-verify] | verify_state，window/element predicates，满足/不满足/unknown [来源][c-verify] | Agent/tool结果，不是 OS 状态验证 |
| 接入方式 | Rust、Runner gRPC、CLI、MCP；能力暴露不完全一致 | CLI/MCP/Swift service [来源][p-readme] | CLI/MCP/Rust/Python/TS，共享 runtime [来源][c-driver] | CLI/Swift SDK、自定义 agent tools [来源][k-readme] |
| 记录/复用 | 隐式 run/trace/artifact；不能推导通用 replay 已完成 | Agent session/trace/capture；v4 已移除旧脚本 runner [来源][p-changelog] | 显式 recording/video/trajectory replay；旧 token/handle 跨 session 不稳定 [来源][c-recording] | JSONL agent session/恢复 [来源][k-session] |
| 浏览器 DOM | inspected driver/API 未见专用 DOM/CDP service | Chrome DevTools MCP integration [来源][p-browser] | typed Chromium/Electron page routes，有限浏览器覆盖 [来源][c-browser] | 可接外部工具，不算内置 |
| VM/云隔离/评测 | 已有远程 device/runner routing，不等于 VM provisioning | 主要控制宿主 macOS | Sandbox/Fleets/Lume/Bench 是独立优势 [来源][c-readme] | 不属于其 OS API 能力 |

结论：AUV 已有基础桌面驱动，主要差距在完整操作集合、语义与应用级便利 API，以及已有能力对外接通。Peekaboo 适合作为 macOS 原生 API 覆盖参照；CUA 适合作为跨平台 driver 契约、语义操作及行为证据参照。kwwk main 属于不同产品层；其独立的 kwwk-computer-use-core 则有桌面 API，详见后续源码审查，不能由本表推导整个 KWWK 生态没有 computer use。

## AUV findings that change the comparison

AUV already has native desktop capability implementations on macOS, Windows, and Linux. The important distinction is driver implementation versus Runner RPC exposure versus invoke/MCP exposure. A count of CLI commands alone would understate the implementation; a count of driver methods alone would overstate the ready-to-use automation surface. [Local platform dispatch](../../../../crates/auv-driver/src/lib.rs), [Runner implementations](../../../../crates/auv-cli/src/runner/local_driver.rs), [invoke registry](../../../../crates/auv-cli-invoke/src/registry.rs), [MCP adapters](../../../../crates/auv-cli/src/commands/mcp.rs).

| Capability | Driver implementation | Public frontend boundary / remaining gap | Evidence |
| --- | --- | --- | --- |
| Displays and capture | All three drivers list displays and capture displays/regions/windows, with platform-specific limits | Direct local invoke/MCP paths for window/screen operations remain largely macOS-specific; selected Runner dispatch can use portable capture/list services; display operations have their own platform gates | [dispatch](../../../../crates/auv-cli/src/commands/invoke.rs), [selected invoke](../../../../crates/auv-cli-invoke/src/runner.rs), [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [capture proto](../../../../proto/auv/api/driver/v1/capture.proto) |
| Native OCR | macOS Apple Vision, Windows `Windows.Media.Ocr`, Linux Tesseract; window text search/polling | Runner exposes recognition and window/display text search; invoke OCR-oriented commands are mostly macOS paths | [macOS OCR](../../../../crates/auv-driver-macos/src/native/ocr.rs), [Windows OCR](../../../../crates/auv-driver-windows/src/ocr.rs), [Linux OCR](../../../../crates/auv-driver-linux/src/ocr.rs), [RPC](../../../../proto/auv/api/driver/v1/text_recognition.proto) |
| Left click / multiple clicks / movement | Shared click/movement implementations; Runner also has timed curve movement | Shared click options do not expose mouse button selection; macOS session hardcodes button `0` even though its native wrapper accepts a button code | [shared input](../../../../crates/auv-driver-common/src/input.rs), [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [native pointer](../../../../crates/auv-driver-macos/src/native/pointer.rs), [input RPC](../../../../proto/auv/api/driver/v1/input.proto) |
| Drag / held input | No complete shared drag or independent button/key down/up contract found | `PressKeysOptions` explicitly defers key hold until cancellation/release semantics are approved; pointer movement is not drag | [shared input](../../../../crates/auv-driver-common/src/input.rs), [movement model](../../../../crates/auv-driver-common/src/mouse.rs), [input RPC](../../../../proto/auv/api/driver/v1/input.proto) |
| Scrolling | Window and global scrolling exist in driver APIs | No standalone Scroll RPC or registered general scroll invoke command; scan's internal scrolling does not fill this API gap | [shared WindowInput](../../../../crates/auv-driver-common/src/input.rs), [RPC](../../../../proto/auv/api/driver/v1/input.proto), [invoke input group](../../../../crates/auv-cli-invoke/src/commands/input.rs) |
| Keyboard | All three drivers have basic foreground text/key delivery; macOS has target-bound ordered keyboard requests | Runner basic `TypeText`/`PressKey` are portable; `InputKeyboard`/`PressKeys` target-bound sequence implementation is macOS-only; invoke typing/key operations also have platform restrictions | [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [invoke input](../../../../crates/auv-cli-invoke/src/commands/input.rs) |
| Background delivery | macOS PID/window-targeted mouse, wheel and keyboard; Windows posted-message click/scroll | Windows background delivery is best effort for classic controls, not Chromium/Electron/WinUI; Linux portal input is foreground and rejects BackgroundOnly | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [Windows boundary](../../../../crates/auv-driver-windows/README.md), [Linux delivery](../../../../crates/auv-driver-linux/src/session.rs) |
| Accessibility tree and actions | macOS AX capture/focus/read, native action wrapper; Windows UIA capture/focus/select with Invoke fallback; Linux AT-SPI capture/focus/select/action | No uniform public element observation/action contract; Runner AccessibilityService only exposes macOS FocusText; Windows ValuePattern writes explicitly deferred | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [native AX wrapper](../../../../crates/auv-driver-macos/src/native/tree.rs), [Windows UIA](../../../../crates/auv-driver-windows/src/accessibility.rs), [Linux AT-SPI](../../../../crates/auv-driver-linux/src/atspi.rs), [AX RPC](../../../../proto/auv/api/driver/macos/v1/accessibility.proto) |
| Window management | macOS/Windows move/resize/set-frame/minimize/restore/zoom | Runner window service only lists/resolves; invoke window group does not expose mutations; Linux mutation and common close-window operation were not found | [macOS session](../../../../crates/auv-driver-macos/src/session.rs), [Windows session](../../../../crates/auv-driver-windows/src/session.rs), [Linux session](../../../../crates/auv-driver-linux/src/session.rs), [window RPC](../../../../proto/auv/api/driver/v1/window.proto) |
| App lifecycle | macOS bundle activation, Windows process-name activation | No complete shared installed/running-app list, explicit launch/open/quit/relaunch/hide API; macOS activate may implicitly launch through AppleScript, which is not a complete lifecycle contract; Windows activation is not wired to app.activate | [macOS application](../../../../crates/auv-driver-macos/src/application.rs), [Windows application](../../../../crates/auv-driver-windows/src/application.rs), [invoke app](../../../../crates/auv-cli-invoke/src/commands/app.rs) |
| Menus / system dialogs / Dock / Spaces | Low-level AX could be used by application code | No reusable general menu-path, native file-panel, Dock, or Spaces service found in the inspected session/Runner/invoke APIs | [session](../../../../crates/auv-driver-macos/src/session.rs), [Runner](../../../../crates/auv-cli/src/runner/local_driver.rs), [registry](../../../../crates/auv-cli-invoke/src/registry.rs) |
| Clipboard | macOS snapshots preserve data for pasteboard item types; Windows has text and memory-backed rich-format snapshots; Linux portal clipboard is text-only | Text set and preserve/restore are present; no uniform image/file/raw-format read/write API or standalone Clipboard RPC/invoke group. Do not misread the macOS opaque String snapshot as plain text | [Swift clipboard](../../../../crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Clipboard.swift), [Windows clipboard](../../../../crates/auv-driver-windows/src/clipboard.rs), [Linux clipboard](../../../../crates/auv-driver-linux/src/clipboard.rs), [registry](../../../../crates/auv-cli-invoke/src/registry.rs) |
| Verification and evidence | `InputActionResult` records delivery path, attempts, disturbance and verification flag; explicit app activation readback; OCR waits | Delivery success defaults to unverified; no general shared element/window predicate verification service found. Tracing is valuable but does not itself prove semantic success | [input contract](../../../../crates/auv-driver-common/src/input.rs), [app activation](../../../../crates/auv-driver-macos/src/application.rs), [terms](../../../TERMS_AND_CONCEPTS.md) |
| Linux environment coverage | Wayland + portal capture/input, AT-SPI observation; repository explicitly prioritizes GNOME/PaperWM | X11 is deliberately outside current scope. Window capture currently uses a display crop because portal window source cannot be bound to the requested AT-SPI window; current pointer position is unsupported | [Linux README](../../../../crates/auv-driver-linux/README.md), [window capture](../../../../crates/auv-driver-linux/src/window.rs), [pointer position](../../../../crates/auv-driver-linux/src/input.rs) |

## Gap interpretation

For general OS automation, the inspected gaps form six candidate work groups. This grouping is an analytical inventory, not a completion percentage, effort estimate, or approved roadmap.

| Candidate group | What is missing | Nature of work |
| --- | --- | --- |
| Complete pointer/keyboard operations | Public right/middle button selection, drag, held buttons/keys with reliable release | Shared contract plus platform implementation; some native building blocks already exist |
| App lifecycle | App enumeration, explicit launch/open/quit/relaunch/hide, consistent app targeting | New reusable OS capability APIs and platform integration |
| Semantic element operations | Public tree observation, fresh target identity, common action/value/selection operations and separate predicate verification | Reconnect existing AX/UIA/AT-SPI implementations, then fill actual action gaps |
| Menus and native dialogs | Menu paths, context/system menus, native open/save panels; macOS Dock/Spaces are additional platform-specific scope | New convenience APIs grounded in native capabilities; require explicit owner-selected slices |
| Expose existing capabilities consistently | Scroll, window mutations, clipboard and existing tree APIs across Runner/invoke/MCP | Primarily producer-to-consumer integration; preserve typed direct results and recording |
| Platform behavior coverage | Windows background toolkit limits and target keyboard parity; Linux compositor, exact-window capture and foreground limits | Backend-specific implementations and reproducible behavior tests; X11 remains intentional scope exclusion |

Browser DOM/CDP integration and VM/cloud fleet management are separate comparisons. They should not inflate an estimate of missing native OS primitives. AUV's Device/Run/Runner routing already exists, but does not imply VM provisioning or a general autonomous computer-use agent. Likewise, implicit run recording does not establish completed replay/distill/compile behavior. [Current vocabulary](../../../TERMS_AND_CONCEPTS.md).

No production code was changed, and no runtime tests were run for this research note.

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
