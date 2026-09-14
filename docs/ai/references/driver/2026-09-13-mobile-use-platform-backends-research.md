# Mobile-use frameworks: platform backends and execution modes

Research date: 2026-09-13. Change classification: docs-only external-source research. This document does not approve or implement any AUV feature.

## Evidence boundary

This survey traces frontend entrypoints and factories to observation and input adapters.
The evidence consists of local source checkouts, published source packages, and first-party documentation.
The review did not install dependencies or run build scripts, model services, simulators, phones, benchmarks, or cloud sessions.
It did not sign apps or grant permissions.

“Implemented” means that a connected source path exists. It does not mean that a device action succeeded.
“No adapter found” applies only to the inspected paths and revisions.

The review cloned accessible top-level repositories with full history.
It reused the existing Minitap and ARTEMIS revisions without changes.
It initialized MadeAgents submodules recursively at their recorded commits.
Two Git repositories were unavailable. The review used public package releases for those sources.

## Main conclusions


1. Agent strategy, frontend, deployment, and platform driver are separate concerns. A portable action schema does not establish platform support.

2. Android implementations use ADB commands, UIAutomator instrumentation, persistent DEX services, or AccessibilityService/IME apps. Their permissions, startup, text input, and observations differ.

3. IOS implementations use idb simulators, XCTest services, or Mac iPhone Mirroring. Simulator support does not establish real-device discovery or signing.

4. Open-AutoGLM, MobiAgent, and Mobile-Agent v3 contain Harmony adapters. They use HDC/UITest commands or hmdriver2 RPC. Generic OpenHarmony compatibility remains untested.

5. Source and documentation can differ. Minitap contains a real-iPhone WDA path despite its README restriction. Phone Use CLI 0.5.3 contains Android and Mirroring backends.

6. Model completion and independent task-state checks are different. AndroidWorld evaluates task state, ARTEMIS separates Checker results from release policy, and AppAgent accepts model FINISH.

## Platform and main-mode overview

The detailed source notes provide evidence for this table.
“No adapter found” describes the inspected revision. It does not mean that an implementation is impossible.
The Phone Use evidence comes from published packages.

| Project | Primary modes | Android implementation | iOS simulator | Physical iOS | Harmony/OpenHarmony evidence |
|---|---|---|---|---|---|
| Minitap mobile-use | Python SDK/CLI. LangGraph task planning/execution. Local/BrowserStack/Limrun | adbutils + UIAutomator2 | idb companion + simctl | WDA, plus BrowserStack XCUITest | No HDC adapter found |
| ARTEMIS | Flash/Pro. CLI/SDK/MCP/web console. Diagnostics and replay | ADB input, helper-first hierarchy with UIAutomator2 fallback | No driver found | No driver found | No HDC adapter found |
| Mobilerun | Python/CLI/TUI. FastAgent or Manager+Executor. Local/cloud | ADB + Portal a11y/IME. ADB-only fallback | No separate local simulator backend established | WDA + local HTTP/USB bridge | No HDC adapter found |
| Mobile Next mobile-mcp | Device MCP for external agents, local/cloud | Delegates to mobilecli by default | mobilecli | mobilecli | No driver found |
| Mobile Next mobilecli | Go CLI + daemon + JSON-RPC. Local/cloud | ADB + persistent DEX UiAutomation | DeviceKit XCTest + simctl | Signed DeviceKit + go-ios/testmanagerd | No driver found |
| Phone Use CLI 0.5.3 / SDK 0.4.1 | CLI/agent/MCP/SDK. Maps/skills/replay/cloud | CLI ADB + UIAutomator dump | SDK simctl + agent-device XCTest | CLI iPhone Mirroring + Vision OCR + CGEvent | No HDC path in inspected package release |
| Open-AutoGLM | CLI/interactive/Python run or step. Screenshot model loop | ADB + ADBKeyboard | No project-specific setup/discovery established | Separate IOSPhoneAgent + WDA/XCTest | HarmonyOS NEXT HDC/UITest. Generic OpenHarmony unverified |
| MobiAgent | Python task/workflow. Planner/decider/grounder. Memory. Native App. Termux | uiautomator2, or AccessibilityService/MediaProjection, or raw ADB | No adapter found | No adapter found | HarmonyDevice via hmdriver2. Generic OpenHarmony unverified |
| MobileAgent | Versioned model/agent runners and evaluation | v3/v3.5 ADB. AndroidWorld evaluation | Explicitly excluded in v3/v3.5 | Explicitly excluded in v3/v3.5 | v3 HDC adapter. V3.5 mobile entry Android-only |
| AppAgent | Autonomous exploration, human demonstration, learned UI docs, task execution | ADB + UIAutomator XML. Physical/emulator | No adapter found | No adapter found | No adapter found |
| MadeAgents MobileUse | WebUI/Python. ReAct/multi-agent/hierarchical. Exploration/RAG. Benchmark adapters | adbutils screenshots/actions. Benchmark-only optional a11y | No adapter found | No adapter found | No adapter found |
| AndroidWorld | Emulator environment. Task initialization, baseline agents, evaluation, HTTP/Docker | AndroidEnv emulator/gRPC, a11y forwarding app, ADB input | No adapter found | No adapter found | No adapter found |

## Mechanisms to compare

### Android


- **ADB commands:** adapters use `screencap`, `uiautomator dump`, `input tap/swipe/keyevent`, and package or activity commands. Unicode input commonly requires ADBKeyboard or another IME.

- **UIAutomator2:** a Python client controls Android test automation and obtains screenshots and XML. Minitap and MobiAgent use this route.

- **Persistent UiAutomation:** mobilecli sends a DEX file to the device and starts `app_process`. The service retains a connection across calls. It needs device-side code but no installed APK.

- **Accessibility/IME services:** Mobilerun Portal, ARTEMIS Helper, and the MobiAgent app use different control paths. An accessibility tree does not establish AccessibilityService gesture delivery.

- **Benchmark environment:** AndroidWorld requires emulator lifecycle services, gRPC, and task-state evaluators. Separate ADB helpers do not establish a benchmark for physical phones.

### iOS


- **Simulator idb:** Minitap controls an idb companion. Apple tools manage simulator lifecycle and app discovery.

- **XCTest services:** Minitap, Open-AutoGLM, and Mobilerun use WDA. Mobile Next uses DeviceKit. These services expose test-runner functions through HTTP or JSON-RPC. Physical devices also require signing, trust, Developer Mode, and connection management.

- **Mirroring:** Phone Use captures the Mac iPhone Mirroring window and recognizes text through OCR. It activates the window and sends Mac input. Its synthetic nodes do not contain native iOS roles or states.

The [iPhone review](2026-09-14-iphone-phone-use-source-review.md) describes additional Mirroring implementations and their background-input limits.
An AX tree is optional for a visual control loop.

### OpenHarmony and HarmonyOS

HDC provides a transport. The device also needs compatible screenshots, UITest commands, app lifecycle operations, and text input.
Device helpers add ABI and service-version dependencies.
The official OpenHarmony UITest documentation lists `screenCap`, `dumpLayout`, and `uiInput`.
These primitives do not establish compatibility for every agent or device.

## Clone inventory

| Repository | Local checkout | Commit | Clone | Worktree |
|---|---|---|---|---|
| [minitap-ai/mobile-use](https://github.com/minitap-ai/mobile-use) | `/Users/neko/Git/github.com/minitap-ai/mobile-use` | `12a1dbd3774e96fbc6029ba4d2a7801aeb527764` | Full | Clean |
| [google/artemis](https://github.com/google/artemis) | `/Users/neko/Git/github.com/google/artemis` | `371aa6df56880643da57b30da936e9812fb0ec66` | Full | Clean |
| [droidrun/mobilerun](https://github.com/droidrun/mobilerun) | `/Users/neko/Git/github.com/droidrun/mobilerun` | `9a95ad435fc627c18a8bf511a83c02260bf519fb` | Full | Clean |
| [mobile-next/mobile-mcp](https://github.com/mobile-next/mobile-mcp) | `/Users/neko/Git/github.com/mobile-next/mobile-mcp` | `5bc7402713dc2ad4b9a7b7a97b319f8b37a99c25` | Full | Clean |
| [mobile-next/mobilecli](https://github.com/mobile-next/mobilecli) | `/Users/neko/Git/github.com/mobile-next/mobilecli` | `f7148582b01aff489f006ad6e62c5fb21dd0785d` | Full | Clean |
| [zai-org/Open-AutoGLM](https://github.com/zai-org/Open-AutoGLM) | `/Users/neko/Git/github.com/zai-org/Open-AutoGLM` | `86f55382982fb054e8fc98ca80609dff8a2cdc3c` | Full | Clean |
| [X-PLUG/MobileAgent](https://github.com/X-PLUG/MobileAgent) | `/Users/neko/Git/github.com/X-PLUG/MobileAgent` | `11cea575561fb7800b5fb6b6cafa56f7a91de11f` | Full | Clean |
| [IPADS-SAI/MobiAgent](https://github.com/IPADS-SAI/MobiAgent) | `/Users/neko/Git/github.com/IPADS-SAI/MobiAgent` | `4ee794021bb34a8de4d00c52ca8fa6f0065a45ff` | Full | Clean |
| [TencentQQGYLab/AppAgent](https://github.com/TencentQQGYLab/AppAgent) | `/Users/neko/Git/github.com/TencentQQGYLab/AppAgent` | `2c1900422caf6f9e94e96d5dd984b530e5a5fbf8` | Full | Clean |
| [MadeAgents/mobile-use](https://github.com/MadeAgents/mobile-use) | `/Users/neko/Git/github.com/MadeAgents/mobile-use` | `babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347` | Full | Clean |
| [google-research/android_world](https://github.com/google-research/android_world) | `/Users/neko/Git/github.com/google-research/android_world` | `e3fea3ccc69787570e282c99573298f1c3019a34` | Full | Clean |
| [droidrun/mobilerun-portal](https://github.com/droidrun/mobilerun-portal) | `/Users/neko/Git/github.com/droidrun/mobilerun-portal` | `d4cb7d6657385488239812e776df584f890e32fd` | Full | Clean |
| [mobile-next/devicekit-ios](https://github.com/mobile-next/devicekit-ios) | `/Users/neko/Git/github.com/mobile-next/devicekit-ios` | `510d10e5e376221397cef7f8d7acb74603c61888` | Full | Clean |
| [mobile-next/devicekit-android](https://github.com/mobile-next/devicekit-android) | `/Users/neko/Git/github.com/mobile-next/devicekit-android` | `73b8d59da7b419bf8105d9a16d9d58082d6cdcde` | Full | Clean |
| [callstack/agent-device](https://github.com/callstack/agent-device) | `/Users/neko/Git/github.com/callstack/agent-device` | `3394d5b89cb2db428c838f77bec78e3ad5adadb5` | Full | Clean |
| [codematrixer/hmdriver2](https://github.com/codematrixer/hmdriver2) | `/Users/neko/Git/github.com/codematrixer/hmdriver2` | `3a7d6c43016274c607975bcc9f92a31d0c3248f8` | Full | Clean |

The inventory contains 16 top-level repositories: 11 main projects and 5 companion implementations.
It also contains 3 initialized MadeAgents submodules.
The Phone Use Git repository was unavailable. Its public package artifacts remain separate from Git checkouts.

## Detailed source notes

## Minitap mobile-use

Evidence: source inspected at `12a1dbd3774e96fbc6029ba4d2a7801aeb527764`. No devices or agent tasks run.


- **Modes:** Minitap provides a Python `Agent` SDK and Typer CLI. It supports local devices, model-provider configuration, and a LangGraph task loop. The Docker launcher targets Android. The source also connects BrowserStack and Limrun controllers.

  The README links a hosted MCP page. Neither the inspected entrypoints nor the `langchain-mcp-adapters` dependency establish an MCP server in this repository. [Entry/dependencies](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/pyproject.toml#L16), [SDK](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/sdk/agent.py#L129), [graph](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/graph/graph.py#L107).


- **Android:** the factory requires adbutils and UIAutomator clients. UIAutomator2 provides PNG screenshots and XML hierarchy. The controller converts the hierarchy to element records. Gestures use ADB, and text uses UIAutomator with an ADB fallback. Local devices require USB debugging and an authorized ADB connection. Real devices and emulators share this path. [Factory](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/controllers/controller_factory.py#L10), [UIAutomator client](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/ui_automator_client.py#L1), [Android controller](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/controllers/android_controller.py#L52).


- **iOS simulator:** `get_ios_client` selects `IdbClientWrapper`. `idb_companion --udid ... --grpc-port ...` manages the device connection. fb-idb gRPC provides input and capture. `idb ui describe-all` and `simctl listapps` provide observations and app discovery. This path requires a Mac, an Xcode simulator, and idb companion. The companion host and port accept configuration. [Client factory](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/ios_client.py#L281), [idb lifecycle](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/idb_client.py#L121), [UI description](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/idb_client.py#L338).


- **Physical iOS:** the same factory selects `WdaClientWrapper`. `facebook-wda` provides clicks, swipes, screenshots, XML, and keyboard input through a WDA session. `iproxy` forwards the device HTTP port. Lifecycle code can run `xcodebuild test`. This requires a trusted device and a WDA Xcode project with signing configuration. The review did not run this device path. [WDA wrapper](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/wda_client.py#L71), [WDA startup/signing](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/wda_lifecycle.py#L203).


- **Cloud iOS:** BrowserStack Appium requests `automationName=XCUITest`. Limrun has separate Android and iOS adapters. These integrations do not establish identical local and cloud capabilities. [BrowserStack](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/browserstack_client.py#L45), [Limrun factory](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/clients/cloud_device_factory.py#L1).


- **Documentation difference:** the README excludes physical iOS. The factory and WDA lifecycle contain a connected real-device path. This note records the source implementation separately from the README statement. [README](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/README.md#L177).


- **Harmony:** the factory accepts Android/iOS and rejects other values. The inspection found no HDC backend. [Factory](https://github.com/minitap-ai/mobile-use/blob/12a1dbd3774e96fbc6029ba4d2a7801aeb527764/minitap/mobile_use/controllers/controller_factory.py#L33).

## Google ARTEMIS

Evidence: source inspected at `371aa6df56880643da57b30da936e9812fb0ec66`. No devices, startup scripts, or tasks run.


- **Modes:** ARTEMIS provides CLI, MCP, a web console, replay, a Python SDK, and a remote client. Flash uses a reactive runner with summaries. Pro uses a graph for planning, operation, and task checks. [README](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/README.md), [SDK profile dispatch](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/sdk/agent.py#L507), [Flash](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/agents/flash/runner.py), [graph](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/graph/graph.py).


- **Driver:** `create_driver` selects a mock or Android ADB driver. Base classes and cloud classes do not establish iOS or Harmony implementations. [Driver factory](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/drivers/factory.py#L34).


- **Observation:** `auto` tries Accessibility Helper, with UIAutomator2 fallback for each call. `helper` requires helper success. `uiautomator` uses the previous backend. The helper uses HTTP, ADB forwarding, and a session token. The helper manager owns its lifecycle. [Screen factory](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/clients/screen_client_factory.py#L15), [helper client](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/clients/accessibility_client.py#L15), [helper manager](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/runtime/helper_manager.py).


- **Input:** gestures use ADB `input tap/swipe/keyevent`, independently of hierarchy capture. Text tries the clipboard, ADBKeyboard, and native ADB. `ScreenData` combines screenshots and XML. [ADB driver](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/drivers/android/adb_driver.py#L121), [input](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/drivers/android/adb_driver.py#L229).


- **Records and checks:** the data engine records screenshots, hierarchy, and actions. MCP exposes trace inspection. The read-only Checker reports passed, failed, or inconclusive results. Release policy is separate. An `inconclusive` result can permit release. Task completion does not mean that every assertion passed. [Checker](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/agents/checker/checker.py#L15), [trace inspector](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/mcp_server/tools/inspect_trace.py).


- **Cloud:** `ARTEMIS_CLOUD_MODE` imports `cloud_service.virtualization`. This implementation is absent from the clone. The branch alone does not provide a complete cloud backend. [Factory](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/artemis/drivers/factory.py#L36).


- **Provenance:** the README credits Minitap for source code. [Attribution](https://github.com/google/artemis/blob/371aa6df56880643da57b30da936e9812fb0ec66/README.md#L314).

## Phone Use: published source fallback, not a successful Git clone

The npm metadata for `phone-use@0.5.3` and `@phone-use/sdk@0.4.1` identifies `https://github.com/Rajmeet/phone-use.git`.
During this survey, `git clone` returned `Repository not found`.
This response does not establish whether the repository is private, removed, or temporarily unavailable.

The review downloaded and extracted the public tarballs without install scripts.
It checked each SHA-1 against npm metadata.
The files are under `/Users/neko/Git/npmjs.org/phone-use-reference/`, in `phone-use-0.5.3/` and `sdk-0.4.1/`.
The metadata and original archives remain with the extracted files.

The CLI 0.5.3 and SDK 0.4.1 tarballs contain the inspected release sources.
The paths in this section refer to those archives. The review did not run the packages.

Sources: [CLI 0.5.3 tarball](https://registry.npmjs.org/phone-use/-/phone-use-0.5.3.tgz), [SDK 0.4.1 tarball](https://registry.npmjs.org/@phone-use/sdk/-/sdk-0.4.1.tgz).


- **Modes:** CLI 0.5.3 provides commands, task agents, MCP, app maps, skill recording, replay, a cloud client, and a self-hosted runner. SDK 0.4.1 provides typed device APIs and local iOS lifecycle. The inspected SDK exports no Android lifecycle engine. Relevant files are `src/index.ts`, `src/driver.ts`, `src/mcp.ts`, `src/sdk.ts`, and `src/skills.ts`. The skill implementation contains preconditions and result checks.


- **iOS simulator:** SDK `src/backends/ios.ts` starts dedicated simulators through `xcrun simctl` and retains their UDIDs. `src/backends/agent-device.ts` sends observations and input to `agent-device@0.19.3`. That dependency uses an XCTest runner. Its current main branch does not describe this pinned version.


- **Physical iPhone:** `src/driver.ts:36` routes `PHONE_USE_DEVICE=mirror` to `src/mirror/backend.ts`. The Swift helper captures the Mac iPhone Mirroring window through `screencapture`. Vision OCR produces text-only nodes. The helper activates the window and sends CGEvents. It requires Mirroring, Mac screen-recording/accessibility permissions, and Swift tools. It cannot obtain native iOS roles, toggle state, or icon semantics from OCR alone.

  Source locations: `src/mirror/helper.swift:70`, `:104`, `:260`, and `src/mirror/preflight.ts`.


- **Android:** `src/driver.ts:38` routes `PHONE_USE_DEVICE=android` to `src/android/backend.ts`. Hierarchy capture uses `uiautomator dump` and XML parsing. Screenshots use `exec-out screencap -p`. Gestures use `input tap/swipe`. ASCII text uses `input text`. Non-ASCII text requires ADBKeyboard and a base64 broadcast, followed by IME restoration.

  `src/android/adb.ts` selects the device serial. CLI 0.5.3 contains a connected implementation.


- **Cloud:** `src/cloud-cli.ts:12` rejects Android cloud provisioning. Android requires a self-hosted device in this release. Local Android code does not establish Android cloud availability.


- **Documentation difference:** the website describes simulator-only support and no Android engine. CLI 0.5.3 contains Android and real-iPhone Mirroring backends. This finding applies to that CLI release. It does not extend to every SDK or cloud interface. [website docs](https://phoneuse.dev/docs).


- **Harmony:** these release sources contain no HDC factory path. Later changes in the agent-device main branch do not extend this pinned release.

Evidence date: 2026-09-13. The review inspected source and first-party documentation at the recorded commits.
It did not install dependencies, compile code, or run device tests.

## Clone inventory


- `droidrun/mobilerun`: `/Users/neko/Git/github.com/droidrun/mobilerun`, HEAD `9a95ad435fc627c18a8bf511a83c02260bf519fb`, full clone, clean worktree.

- `mobile-next/mobile-mcp`: `/Users/neko/Git/github.com/mobile-next/mobile-mcp`, HEAD `5bc7402713dc2ad4b9a7b7a97b319f8b37a99c25`, full clone, clean worktree.

- `mobile-next/mobilecli`: `/Users/neko/Git/github.com/mobile-next/mobilecli`, HEAD `f7148582b01aff489f006ad6e62c5fb21dd0785d`, full clone, clean worktree.

- `droidrun/mobilerun-portal`: `/Users/neko/Git/github.com/droidrun/mobilerun-portal`, HEAD `d4cb7d6657385488239812e776df584f890e32fd`, full clone, clean worktree.

- `mobile-next/devicekit-ios`: `/Users/neko/Git/github.com/mobile-next/devicekit-ios`, HEAD `510d10e5e376221397cef7f8d7acb74603c61888`, full clone, clean worktree.

- `mobile-next/devicekit-android`: `/Users/neko/Git/github.com/mobile-next/devicekit-android`, HEAD `73b8d59da7b419bf8105d9a16d9d58082d6cdcde`, full clone, clean worktree.

The Portal and DeviceKit repositories contain companion implementations.
The PyPI metadata for `droidrun/mobilerun-core-local` identifies an unavailable GitHub repository.
GitHub returned `Repository not found`.

The review downloaded the published `mobilerun-core-local 0.6.0` wheel without installation.
It extracted the wheel to `/Users/neko/Git/pypi.org/mobilerun-core-local-reference/0.6.0/extracted`.
The published SHA-256 is `fb67820a44dacef84b6a1baa96817b69e4de13a99ba80338cff85eda41e70cf1`.

Sources: [Immutable wheel](https://files.pythonhosted.org/packages/d5/7f/be52c36593c6350bda1f1e78b959b915682790e54c6c28467f57386a246d/mobilerun_core_local-0.6.0-py3-none-any.whl).

## Mode and backend matrix

| Project | Main modes | Android | iOS simulator | iOS real device | OpenHarmony/Harmony |
|---|---|---|---|---|---|
| Mobilerun (former DroidRun) | Python LLM agent SDK, CLI/TUI/Docker. Direct FastAgent or Manager+Executor reasoning. Screenshots+accessibility or vision-only. Recording/macros. Separate managed cloud | ADB real/emulator. Portal APK accessibility/IME by default, ADB-only option in local driver | No distinct simctl/idb backend found in inspected framework or 0.6.0 dependency. The generic iOS HTTP interface does not establish simulator support | Documented local flow: macOS/Xcode + signed WDA + Developer Mode iPhone + mobilerun-ios USB bridge exposing HTTP | No explicit backend found. Default non-iOS branch is Android, not an OpenHarmony adapter |
| Mobile Next mobile-mcp | MCP tool server for external coding/LLM agent. Local and cloud physical fleet | Default delegates to mobilecli | Default mobilecli. Installs missing DeviceKit agent | Default mobilecli. Signed DeviceKit/WDA prerequisite | No backend found |
| Mobile Next mobilecli | Go CLI + persistent per-user daemon + HTTP JSON-RPC. Local device lifecycle/input/inspection/logs/files, webview/Flutter tooling. Cloud reserve/release | ADB real/emulator, persistent headless DEX DeviceServer via app_process/UiAutomation | simctl lifecycle + DeviceKit XCTest runner and JSON-RPC | go-ios USB/services/tunnel + testmanagerd launches signed DeviceKit XCTest/WDA runner | No backend found |

## Mobilerun details

Mobilerun uses Python and LlamaIndex Workflows.
`reasoning=False` runs FastAgent. `reasoning=True` separates Manager planning from Executor actions.
The driver, state provider, tool registry, trajectory writer, and model policy have separate responsibilities.

Sources: [pyproject.toml:6-28](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/pyproject.toml#L6-L28), [mobilerun/agent/droid/droid_agent.py:255-270](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/mobilerun/agent/droid/droid_agent.py#L255-L270).

Driver selection tries an injected driver, VisualRemoteDriver, the iOS HTTP factory, and AndroidDriver in that order.
RecordingDriver wraps the selected driver to record actions.
The vision-only and visual-remote modes use screenshots without a hierarchy.

Sources: [mobilerun/agent/droid/droid_agent.py:614-703](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/mobilerun/agent/droid/droid_agent.py#L614-L703).

The normal Android configuration uses Portal accessibility and a custom IME.
The transport supports TCP through ADB forwarding and an Android content provider.
The published AndroidDriver uses `async_adbutils` click/swipe calls for gestures. Text input prefers the Portal IME.
`portal_mode=auto|required|disabled` controls the Portal requirement. The text fallback accepts printable ASCII only.

Screenshots prefer Portal, then ADB screencap. The ADB-only hierarchy path uses `uiautomator dump`.
The wheel contains these paths in `mobilerun_core_local/driver/android/adb.py:91-180,281-299,309-420`.
The default ADB driver and Portal HTTP backend use different gesture paths.

Sources: [docs/guides/device-setup.mdx:61-112](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/docs/guides/device-setup.mdx#L61-L112).

The Portal companion contains an AccessibilityService, an InputMethodService, a UI root resolver, and gesture dispatch.
It exposes TCP, HTTP, and content-provider interfaces.
This device service can support more than one agent loop.

Sources: [app/src/main/java/com/mobilerun/portal/input/MobilerunKeyboardIME.kt:1-30](https://github.com/droidrun/mobilerun-portal/blob/d4cb7d6657385488239812e776df584f890e32fd/app/src/main/java/com/mobilerun/portal/input/MobilerunKeyboardIME.kt#L1-L30), [app/src/main/java/com/mobilerun/portal/streaming/ScrcpyControlChannel.kt:263-300](https://github.com/droidrun/mobilerun-portal/blob/d4cb7d6657385488239812e776df584f890e32fd/app/src/main/java/com/mobilerun/portal/streaming/ScrcpyControlChannel.kt#L263-L300).

The iPhone instructions require macOS, Xcode, an Apple signing account, Developer Mode, USB trust, and a signed Mobilerun WDA.
`mobilerun-ios --local <udid>` serves localhost:8080. Discovery scans ports 8080–8089.
WDA/XCTest provides accessibility, text, and gestures.

The wheel supports new Portal HTTP and legacy iOS HTTP contracts.
Ping/version detection selects the contract in `driver/ios/local.py:166-224`.
The inspected framework contains no separate simulator controller. README references to another simulator harness do not establish this capability.

Sources: [docs/guides/device-setup.mdx:328-435](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/docs/guides/device-setup.mdx#L328-L435).

The MCP code is a client for external tools.
It does not establish that the repository provides a device MCP server.

Sources: [mobilerun/mcp/client.py:1-63](https://github.com/droidrun/mobilerun/blob/9a95ad435fc627c18a8bf511a83c02260bf519fb/mobilerun/mcp/client.py#L1-L63).

## Mobile Next details

`mobile-mcp` registers TypeScript MCP tools through a Robot interface.
By default, `createRobotFromDevice` returns `MobileDevice`, which runs mobilecli commands.
`MOBILEMCP_LEGACY_ROBOT=1` selects older AndroidRobot/IosRobot implementations for physical Android and iOS devices.
Simulators still use mobilecli.

Sources: [src/server.ts:195-260](https://github.com/mobile-next/mobile-mcp/blob/5bc7402713dc2ad4b9a7b7a97b319f8b37a99c25/src/server.ts#L195-L260), [src/mobile-device.ts:105-123](https://github.com/mobile-next/mobile-mcp/blob/5bc7402713dc2ad4b9a7b7a97b319f8b37a99c25/src/mobile-device.ts#L105-L123).

mobilecli serves CLI and HTTP requests through a per-user daemon.
The daemon retains discovered devices, iOS tunnels, and agents across calls.
Unix-socket JSON-RPC avoids repeated preparation.
Remote devices implement the same device interface and forward calls to the cloud.

Sources: [README.md:449-466](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/README.md#L449-L466), [devices/remote.go:1-80](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/remote.go#L1-L80).

mobilecli sends its embedded DEX to Android and starts `com.mobilenext.mobilecli.DeviceServer` through `CLASSPATH=... nohup app_process`.
ADB forwards a localabstract socket. DeviceServer retains a UiAutomation connection.
Tap, swipe, gesture, and key calls use this service.

If the fast path fails, UI dumps use `uiautomator dump` and screenshots use `screencap`.
The README phrase “no agent needed” means that no APK installation is necessary.
The implementation still runs code on the device.

Sources: [devices/android_device_server.go:23-112](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/android_device_server.go#L23-L112), [devices/android.go:523-581](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/android.go#L523-L581), [devices/android.go:1609-1678](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/android.go#L1609-L1678).

iOS simulator lifecycle and app operations use `xcrun simctl`.
Input and hierarchy calls use a DeviceKit XCTest runner through JSON-RPC.
Physical iOS uses go-ios installationproxy, testmanagerd, tunnels, and signed agent provisioning.

Sources: [devices/simulator.go:450-595](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/simulator.go#L450-L595), [devices/ios.go:599-677](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/ios.go#L599-L677), [README.md:498-500](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/README.md#L498-L500).

DeviceKit is a Swift/Objective-C XCTest service.
Its tap handler creates a synthesized pointer event and calls the runner daemon.
UI dumps inspect the foreground app through AX/XCTest snapshots, with SpringBoard as a fallback.
The service uses private XCTest interfaces.
Simulator and real-device builds have different host and signing requirements.

Sources: [DeviceKitTests/JSONRPC/Handlers/IOTap.swift:17-41](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/JSONRPC/Handlers/IOTap.swift#L17-L41), [DeviceKitTests/JSONRPC/Handlers/DumpUI.swift:34-75](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/DeviceKitTests/JSONRPC/Handlers/DumpUI.swift#L34-L75), [README.md:35-68](https://github.com/mobile-next/devicekit-ios/blob/510d10e5e376221397cef7f8d7acb74603c61888/README.md#L35-L68).

The optional LLDB agent provides in-app webview and Flutter inspection.
It first requires the DeviceKit/WDA agent to run.
This injection path is separate from the default device-wide input backend.

Sources: [devices/ios_device_agent.go:137-161](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/devices/ios_device_agent.go#L137-L161).

devicekit-android publishes headless DEX tools for clipboard access, UI hierarchy, package lists, and video.
mobilecli also contains its own Java DeviceServer.
Not all mobilecli Android calls use the external DeviceKit package.

Sources: [README.md:1-53](https://github.com/mobile-next/devicekit-android/blob/73b8d59da7b419bf8105d9a16d9d58082d6cdcde/README.md#L1-L53), [agents/android/java/DeviceServer.java:19-60](https://github.com/mobile-next/mobilecli/blob/f7148582b01aff489f006ad6e62c5fb21dd0785d/agents/android/java/DeviceServer.java#L19-L60).

## OpenHarmony boundary

The search found no `Harmony`, `OpenHarmony`, `harmony`, or `hdc` matches in `mobilerun/mobilerun`, `mobile-mcp/src`, or `mobilecli/devices`.
Their inspected factories expose Android and iOS paths, without an HDC driver.
This finding applies only to those source trees.
An Android APK on an Android-compatible Huawei phone does not establish native OpenHarmony or HarmonyOS NEXT support.

The review retained the original wheel, version-specific PyPI metadata, and extracted source.
It checked the SHA-256 against the metadata. It did not install the package.

Sources: [0.6.0 source and metadata](/Users/neko/Git/pypi.org/mobilerun-core-local-reference/0.6.0/).

## Supplemental: Callstack agent-device and Phone Use release boundaries

The review cloned `/Users/neko/Git/github.com/callstack/agent-device`.
It inspected main commit `3394d5b89cb2db428c838f77bec78e3ad5adadb5` and tag `v0.19.3` at `fb7dbfe6fe280ee820b5f47286b6a4548620dfa4` through read-only Git commands.
It did not change the selected revision or install dependencies.

At the inspected main commit, agent-device dispatches HarmonyOS operations to `@agent-device/platform-harmonyos`.
HDC selects the device through `-t device.id`.
Input uses `hdc shell uitest uiInput click/swipe/text/inputText/keyEvent`.

Observation uses `uitest dumpLayout`, `hdc file recv`, and JSON normalization.
Screenshot capture uses `snapshot_display -f`, file transfer, and JPEG checks.
The tool search recognizes DevEco/OpenHarmony paths.
These paths establish a native HDC/ArkUI implementation. They do not establish success on every OpenHarmony distribution.

Sources: [src/core/interactors/harmonyos.ts:25](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/src/core/interactors/harmonyos.ts#L25), [packages/platform-harmonyos/src/hdc.ts:18](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-harmonyos/src/hdc.ts#L18), [packages/platform-harmonyos/src/input-actions.ts:14](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-harmonyos/src/input-actions.ts#L14), [packages/platform-harmonyos/src/snapshot.ts:42](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-harmonyos/src/snapshot.ts#L42), [packages/platform-harmonyos/src/screenshot.ts:7](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-harmonyos/src/screenshot.ts#L7).

The Harmony adapter reports unsupported operations for clipboard, TV remote, and alert read/accept/dismiss calls.
Its gesture implementation supports one contact.

Sources: [src/core/interactors/harmonyos.ts:54](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/src/core/interactors/harmonyos.ts#L54), [packages/platform-harmonyos/src/runtime.ts:170](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-harmonyos/src/runtime.ts#L170).

The inspected main commit differs from v0.19.3 for iOS observation.
Eligible local simulators use host AX observation first. A typed fallback uses XCTest.
After fallback, a circuit retains that choice for the current generation.
Physical iOS observations, provider targets, and interactions still use XCTest.

Sources: [docs/adr/0004-ios-snapshot-backend-strategy.md:15](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/docs/adr/0004-ios-snapshot-backend-strategy.md#L15), [docs/adr/0004-ios-snapshot-backend-strategy.md:38](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/docs/adr/0004-ios-snapshot-backend-strategy.md#L38), [packages/platform-apple/src/snapshot-route.ts:65](https://github.com/callstack/agent-device/blob/3394d5b89cb2db428c838f77bec78e3ad5adadb5/packages/platform-apple/src/snapshot-route.ts#L65).

In v0.19.3, iOS snapshots call `runAppleRunnerCommand` with the `xctest` backend.
That revision contains no Harmony platform implementation.
Both phone-use 0.5.3 and @phone-use/sdk 0.4.1 pin agent-device to 0.19.3 in package.json.
The later Harmony backend and simulator AX path do not belong to these Phone Use releases.

Sources: [src/platforms/apple/interactor.ts:47](https://github.com/callstack/agent-device/blob/fb7dbfe6fe280ee820b5f47286b6a4548620dfa4/src/platforms/apple/interactor.ts#L47), [src/platforms/apple/core/runner/runner-provider.ts:12](https://github.com/callstack/agent-device/blob/fb7dbfe6fe280ee820b5f47286b6a4548620dfa4/src/platforms/apple/core/runner/runner-provider.ts#L12).

Research date: 2026-09-13. The review inspected default-branch source and first-party documentation.
It did not install dependencies, connect devices, or run models.
It cloned all four repositories with full history, without `--depth`.
Each repository had empty `git status --porcelain` output after inspection.

| Repository | Local checkout | HEAD |
|---|---|---|
| zai-org/Open-AutoGLM | `/Users/neko/Git/github.com/zai-org/Open-AutoGLM` | `86f55382982fb054e8fc98ca80609dff8a2cdc3c` |
| IPADS-SAI/MobiAgent | `/Users/neko/Git/github.com/IPADS-SAI/MobiAgent` | `4ee794021bb34a8de4d00c52ca8fa6f0065a45ff` |
| X-PLUG/MobileAgent | `/Users/neko/Git/github.com/X-PLUG/MobileAgent` | `11cea575561fb7800b5fb6b6cafa56f7a91de11f` |
| codematrixer/hmdriver2 | `/Users/neko/Git/github.com/codematrixer/hmdriver2` | `3a7d6c43016274c607975bcc9f92a31d0c3248f8` |

## Platform and mode matrix

| Project | Entry/execution modes | Android | iOS | HarmonyOS / OpenHarmony |
|---|---|---|---|---|
| Open-AutoGLM | Python CLI, interactive task input, Python `PhoneAgent.run/step`, configurable OpenAI-compatible model endpoint | ADB real device/emulator, USB/network. Screenshot-driven actions | Separate `IOSPhoneAgent`, WDA HTTP/XCTest, documented real iPhone/iPad setup. No simulator setup/discovery path found | Explicit HarmonyOS NEXT adapter using HDC shell commands. No OpenHarmony hardware matrix established |
| MobiAgent | Python task-list CLI, multi-task/workflow runner, model-serving HTTP service, data collection tools, native Android app, Termux standalone | Main runner uses uiautomator2 over ADB. Native app uses AccessibilityService and MediaProjection. Standalone uses raw ADB | No iOS device branch found in audited runners | Main runner has `HarmonyDevice` using hmdriver2. Upstream hmdriver2 targets HarmonyOS NEXT. Not evidence of generic OpenHarmony compatibility |
| MobileAgent | Versioned research implementations, CLI mobile runners, notebooks, AndroidWorld/OSWorld evaluation | v3.5 current mobile runner uses ADB screenshot/input. Separate emulator benchmark integration | v3 and v3.5 explicitly exclude iOS | v3 has HDC HarmonyOS controller. V3.5 mobile runner only exposes Android ADB. No OpenHarmony hardware matrix established |

The inspected mobile paths contain no first-party mobile MCP server entrypoint.
MobileAgent announcements include model, tool, and MCP benchmarks.
Those announcements do not establish an exported phone MCP server.

Sources: [MobileAgent root announcement](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/README.md#L46-L54).

## Open-AutoGLM

The agent captures a screenshot and the current app.
It sends multimodal history to the configured model, parses the model action, and calls an action handler.
`run` manages task iteration. `step` exposes one iteration.
The inspected observation path uses screenshots rather than UI-tree selectors.

Sources: [Agent](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/agent.py#L84-L235).

`main.py --device-type adb|hdc|ios` selects the platform.
Android and Harmony share `PhoneAgent` and a global `DeviceFactory`.
iOS uses a separate agent and action handler.
`DeviceType` includes IOS, but `DeviceFactory.module` implements only ADB/HDC.

Sources: [CLI branching](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/main.py#L690-L780), [factory](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/device_factory.py#L7-L45).

Android capture runs `adb shell screencap -p /sdcard/tmp.png` and transfers the image to the host.
Tap, swipe, Back, and Home use shell `input`.
App state uses `dumpsys window`. App startup uses Android commands.
Text temporarily selects ADBKeyboard and sends base64 broadcasts.

The documented requirements include Android 7+, developer/USB debugging, and ADBKeyboard.
The implementation also enumerates emulators.

Sources: [Screenshot](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/adb/screenshot.py#L25-L85), [input](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/adb/input.py#L7-L101), [device](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/adb/device.py#L24-L59), [requirements](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/README.md#L81-L123).

Harmony capture tries `hdc shell screenshot`, then `snapshot_display -f`.
It transfers the image through `hdc file recv`.
Actions use `uitest uiInput click/doubleClick/longClick/swipe/keyEvent`.
App state uses `aa dump -l`, and app startup uses `aa start`.
Text uses `uitest uiInput text` without an Android keyboard APK.
The documentation describes USB and TCP connections.

Sources: [Screenshot](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/hdc/screenshot.py#L41-L81), [device commands](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/hdc/device.py#L21-L301), [text](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/hdc/input.py#L9-L86).

iOS requires macOS, Xcode, developer signing, an iPhone/iPad, and a deployed WebDriverAgentRunner.
The documentation permits a free Apple account and a USB or same-Wi-Fi connection.
Other requirements include device trust, UI automation configuration, and libimobiledevice.

Discovery uses `idevice_id -ln`. Sessions use WDA `/session`.
Capture uses WDA `/screenshot`, with `idevicescreenshot` as a fallback.
Input uses W3C `/actions`, WDA drag/home/app endpoints, and `/wda/keys`.
Back uses an edge swipe rather than an Android-style global key.

Sources: [Setup](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/docs/ios_setup/ios_setup.md#L5-L95), [discovery](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/xctest/connection.py#L57-L108), [screenshots](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/xctest/screenshot.py#L24-L95), [actions](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/xctest/device.py#L75-L388), [text](https://github.com/zai-org/Open-AutoGLM/blob/86f55382982fb054e8fc98ca80609dff8a2cdc3c/phone_agent/xctest/input.py#L26-L61).

Some text transport errors only print warnings. A screenshot failure can return a black fallback image.
Thus action return and loop completion do not establish app-state success.
The documented discovery and configuration target real devices.
Upstream WDA simulator support alone does not establish a simulator path in this project.

## MobiAgent

The Python runner combines planning, decider, and optional grounding models.
An end-to-end mode omits the separate grounding model.
The runner accepts task lists and records screenshots, actions, and history for each step.

Optional profile memory uses Mem0/Milvus. Graph retrieval uses Neo4j.
Experience memory changes the planning context, and action memory includes AgentRR.
The README marks ActChain integration as experimental.

Sources: [Modes and memory](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/README.md#L170-L300).

The CLI `--device Android|Harmony` branch selects the device.
Android wraps `u2.connect` for screenshots, gestures, app lifecycle, and hierarchy.
Text uses ADBKeyboard base64 broadcasts.
Harmony wraps `hmdriver2.Driver` for corresponding operations.
It records hierarchy as JSON rather than Android XML.

Sources: [Android adapter](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/runner/mobiagent/mobiagent.py#L119-L226), [Harmony adapter](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/runner/mobiagent/mobiagent.py#L228-L337), [hierarchy persistence](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/runner/mobiagent/mobiagent.py#L1191-L1223), [factory](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/runner/mobiagent/mobiagent.py#L1832-L1840).

MobiAgent does not pin `hmdriver2` in this checkout.
The review inspected its upstream source at `3a7d6c43016274c607975bcc9f92a31d0c3248f8` without installation or execution.
The worktree was clean.

hmdriver2 forwards a socket through HDC to UITest port 8012 and calls Hypium Driver APIs.
It sends `agent.so` to the device and starts `uitest start-daemon singleness`.
No preinstalled testRunner APK does not mean no device helper.
Screenshots use `snapshot_display` or `uitest screenCap`. Trees use `uitest dumpLayout`.

The README targets HarmonyOS NEXT.
This snapshot explains the dependency mechanism. It does not establish the resolved version of every MobiAgent installation.

Sources: [Requirements](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/requirements.txt#L44), [client](https://github.com/codematrixer/hmdriver2/blob/3a7d6c43016274c607975bcc9f92a31d0c3248f8/hmdriver2/_client.py#L18-L43), [helper setup](https://github.com/codematrixer/hmdriver2/blob/3a7d6c43016274c607975bcc9f92a31d0c3248f8/hmdriver2/_client.py#L172-L252), [capture](https://github.com/codematrixer/hmdriver2/blob/3a7d6c43016274c607975bcc9f92a31d0c3248f8/hmdriver2/hdc.py#L317-L360).

Two other Android execution modes matter:


- **Native Java app:** the documentation requires Android 8+. AccessibilityService sends gestures and retrieves nodes. MediaProjection captures the screen for the model service. This path differs from the PC uiautomator2 runner. It does not establish local model execution. [App usage](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/app/README.md#L3-L97), [gesture delivery](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/app/app/src/main/java/com/mobi/agent/MyAccessibilityService.java#L110-L180), [MediaProjection](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/app/app/src/main/java/com/mobi/agent/ScreenCaptureService.java#L142-L175).


- **Termux:** Python runs raw ADB subprocesses and connects to local wireless debugging. This path needs no uiautomator2 backend APK. Chinese input still requires ADBKeyboard. Android 11+ wireless pairing can avoid a PC. Another documented route requires one initial USB connection to a PC. [Deployment](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/mobile_mobi/DEPLOY.md#L12-L69), [adapter](https://github.com/IPADS-SAI/MobiAgent/blob/4ee794021bb34a8de4d00c52ca8fa6f0065a45ff/mobile_mobi/mobiagent_mobile_standalone.py#L173-L290).

A generic uiautomator2 connection does not establish a tested device or emulator matrix.
The inspected device factories contain no iOS adapter.
Harmony package names and `ohos` identifiers do not establish compatibility with arbitrary OpenHarmony distributions.

## X-PLUG/MobileAgent

MobileAgent contains separate research versions rather than one uniform platform SDK.
The v3 mobile runner selects Android or Harmony through mutually exclusive `--adb_path` and `--hdc_path` arguments.
It uses Manager planning, Operator actions, reflection after actions, optional notes, and per-step records.

Sources: [Factory](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/mobile_v3/run_mobileagentv3.py#L21-L29), [planning/operator](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/mobile_v3/run_mobileagentv3.py#L100-L155).

v3 Android uses ADB image transfer, shell input, and keyboard broadcasts.
Harmony uses HDC `uitest screenCap`, file transfer, and `uitest uiInput`.
The README lists Android and HarmonyOS, excludes iOS, and provides commands for both supported platforms.

Sources: [v3 support](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/README.md#L28-L76), [Android](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/mobile_v3/utils/android_controller.py#L7-L59), [Harmony](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/mobile_v3/utils/harmonyos_controller.py#L6-L59).

The v3 Harmony text branch for spaces references `self.adb_path`.
The constructor initializes `self.hdc_path` instead.
This is a source finding, not a reproduced device failure.
The review did not change code or run a test for this branch.

Sources: [Exact branch](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3/mobile_v3/utils/harmonyos_controller.py#L30-L35).

The v3.5 mobile runner requires ADB and uses `AdbTools`.
It parses VLM tool calls, converts normalized coordinates, resolves packages, and retains screenshot history.
Capture uses `adb exec-out screencap -p`.
Gestures use shell input, and text uses ADBKeyboard.
The README lists Android only and excludes iOS.
The v3 Harmony implementation does not establish v3.5 Harmony support.

Sources: [v3.5 support and setup](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3.5/README.md#L44-L81), [CLI and parser](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3.5/mobile_use/run_gui_owl_1_5_for_mobile.py#L33-L87), [driver](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3.5/mobile_use/utils.py#L73-L190).

AndroidWorld evaluation and desktop OSWorld/GUI-Owl examples use separate environments.
The cloud-phone announcement describes deployment at the documentation level.
This review did not establish a separate local adapter for that service.

Sources: [Evaluation](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/Mobile-Agent-v3.5/README.md#L140-L150), [cloud announcement](https://github.com/X-PLUG/MobileAgent/blob/11cea575561fb7800b5fb6b6cafa56f7a91de11f/README.md#L46).

## Cross-project conclusions

These three projects separate visual model decisions from device actions to different degrees.
Platform differences remain in capture, input delivery, app lifecycle, discovery, and permissions.
A visual model does not make its device backend portable.

MobiAgent provides PC control, native Android accessibility, and Termux control modes.
Open-AutoGLM includes real-iOS WDA code alongside Android and Harmony code.
MobileAgent requires separate support statements for each version.

The inspected paths do not establish a tested OpenHarmony distribution/version/device matrix.
Compatibility depends on commands, test-service permissions, helper ABI, hierarchy schema, image format, and app bundle/ability conventions.
HDC and UITest provide useful porting references. Device compatibility remains an open hardware question.

## OpenHarmony primitives and third-party compatibility

The official OpenHarmony arkxtest repository contains JsUnit, UITest, and PerfTest.
UITest provides control lookup and interaction. Its initial APIs start at API version 8.
This establishes native OpenHarmony automation primitives independently of HarmonyOS NEXT product claims.

Sources: [Official arkxtest introduction](https://github.com/openharmony/testfwk_arkxtest/blob/46945da5b1d0376772a6fc73f833e821ca2497f3/README_zh.md#L1-L24).

Official UITest guidance describes ArkTS APIs and command-line tools.
A test-app client communicates through IPC with a server for control trees, window actions, input, and screenshots.
HDC provides the device shell.

`uitest screenCap` captures images. `uitest dumpLayout` writes a JSON tree.
`uitest uiInput` sends clicks, swipes, keys, and text.
The focused-field `text` subcommand starts at API 18.
Display-specific configuration starts at API 20. Agent compatibility depends on these version differences.

Sources: [Architecture](https://github.com/openharmony/docs/blob/f41b9345badd47c7ab0c263344cd7f4b5a549afb/zh-cn/application-dev/application-test/uitest-guidelines.md#L14-L35), [shell and capture/tree commands](https://github.com/openharmony/docs/blob/f41b9345badd47c7ab0c263344cd7f4b5a549afb/zh-cn/application-dev/application-test/uitest-guidelines.md#L609-L654), [input and API 18 text](https://github.com/openharmony/docs/blob/f41b9345badd47c7ab0c263344cd7f4b5a549afb/zh-cn/application-dev/application-test/uitest-guidelines.md#L719-L738).

Inference: Open-AutoGLM and MobileAgent v3 use command families that OpenHarmony also documents.
MobiAgent/hmdriver2 adds a helper and RPC protocol over UITest.
These implementations provide porting references.
They do not establish compatible helper ABI, commands, permissions, or app identifiers on a specific board.

The review did not run board tests or clone the large OpenHarmony repositories.
It read the cited official documents.

Inspection date: 2026-09-13. Evidence consists of source and first-party documentation.
The review did not run device actions, models, emulators, or paid API calls.
It preserved existing workspaces and cloned these three repositories with full history.
All three worktrees were clean after clone and submodule initialization.

| Repository | Local path | HEAD |
|---|---|---|
| TencentQQGYLab/AppAgent | `/Users/neko/Git/github.com/TencentQQGYLab/AppAgent` | `2c1900422caf6f9e94e96d5dd984b530e5a5fbf8` |
| MadeAgents/mobile-use | `/Users/neko/Git/github.com/MadeAgents/mobile-use` | `babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347` |
| google-research/android_world | `/Users/neko/Git/github.com/google-research/android_world` | `e3fea3ccc69787570e282c99573298f1c3019a34` |

The review initialized MadeAgents submodules recursively at the recorded commits:


- `third_party/android_env` = `c16c209fb09b4082b0d28b2f84de8b1693a870cb` (google-deepmind/android_env).

- `third_party/android_lab` = `373fc1d86617a5958a0ad768b1fc388e8722ba91` (MadeAgents/Android-Lab).

- `third_party/android_world` = `9a207d1a378bacbef0dbf3b81b79c63369e11f7e` (MadeAgents/android_world).

These nested clones are separate from the upstream AndroidWorld checkout.

## Platform matrix

| Project | Android physical | Android emulator | iOS physical/simulator | OpenHarmony/HarmonyOS |
|---|---|---|---|---|
| AppAgent | Explicit USB/ADB instructions and implementation | Explicit Android Studio emulator instructions. Same ADB controller | No adapter found | No HDC/native Harmony adapter found |
| MadeAgents MobileUse | Explicit real Android/ADB setup and implementation | AndroidWorld and AndroidLab integration paths | No adapter found | No HDC/native Harmony adapter found |
| AndroidWorld | Generic action helpers use ADB, but shipped environment factory is emulator-specific. Do not claim supported phone benchmark | Main supported environment: live Pixel 6/API 33 AVD. Local or Docker | No adapter found | No HDC/native Harmony adapter found |

Absence findings apply to the inspected commits.
The search covered Python, Markdown, YAML, and TOML. It included the initialized MadeAgents submodules.
The terms were `ios`, `iOS`, `OpenHarmony`, `HarmonyOS`, word-boundary `hdc`, `WebDriverAgent`, and `simctl`.
The search returned no matches. The linked factories provide positive evidence of Android-only paths.

An ADB-accessible Huawei phone does not establish native OpenHarmony/HarmonyOS NEXT support.
A macOS host does not establish an iOS target.

## AppAgent

**Modes:** AppAgent supports autonomous exploration, human demonstrations, and task execution.
Tasks can omit learned documents. `learn.py` selects self-exploration or a step recorder followed by document generation.
This process records UI knowledge. It does not train model weights.

Sources: [Mode dispatcher](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/learn.py#L18-L44), [deployment document selection](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/task_executor.py#L49-L87).

**Observation:** the controller runs `adb shell screencap -p` and transfers the PNG.
It separately runs `adb shell uiautomator dump` and transfers XML.
Clickable and focusable XML elements become numbered screenshot labels.

Resource IDs or class/geometry and ancestor context determine element IDs.
These IDs select learned documentation for each element.
If an element lacks a label, the model can request a grid and select a cell or subarea.

Sources: [Device observations](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/and_controller.py#L41-L130), [observation and prompt assembly](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/task_executor.py#L142-L205).

**Actions:** model output calls `AndroidController` for taps, long presses, text, swipes, and Back.
A long press uses a swipe at one point. Text uses `adb shell input text`.
The controller removes apostrophes and maps spaces to `%s`.
It contains no separate Unicode keyboard transport.
These actions control the foreground UI rather than an app RPC interface.

Sources: [ADB action implementation](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/and_controller.py#L132-L179).

**Knowledge:** self-exploration compares screenshots before and after actions, then writes `auto_docs`.
Human demonstrations produce `demo_docs`.
Task execution adds matching element documentation to each action prompt.

**Outcome:** the loop reports success after model output `FINISH`.
This path has no independent evaluator of app state.
Task directories retain screenshots, XML, prompts, and responses.

Sources: [Reflection and documentation](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/self_explorer.py#L187-L255), [model FINISH boundary](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/task_executor.py#L205-L221), [reported completion](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/scripts/task_executor.py#L286-L291).

**Platform evidence:** the README describes physical Android devices with USB debugging and an Android Studio emulator configuration.
The only device controller uses Android/ADB.

Sources: [Setup](https://github.com/TencentQQGYLab/AppAgent/blob/2c1900422caf6f9e94e96d5dd984b530e5a5fbf8/README.md#L68-L81).

## MadeAgents/mobile-use

MadeAgents/mobile-use has separate ownership from minitap-ai/mobile-use.
It targets real Android GUI agents.
Its interfaces include WebUI, a Python API, AndroidWorld evaluation, and AndroidLab evaluation.

Sources: [README overview](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/README.md#L1-L5).

**Core execution:** `Environment` connects through `adbutils.AdbClient(host, port)` and selects a device serial.
It obtains screenshots, the foreground package, and the device date.
The ordinary environment does not retrieve an accessibility tree.

`Action` dispatch supports app startup, coordinate gestures, key events, text, Home/Back, wait, answer, clear text, and notes.
ASCII text uses `input text`.
Non-ASCII text enables ADBKeyboard and broadcasts base64 UTF-8.
This path requires Android app installation and keyboard configuration.

Sources: [Environment and observations](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/environment/mobile_environ.py#L15-L60), [actions and keyboard](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/environment/mobile_environ.py#L78-L167).

**Agent modes:** the repository includes ReAct, Qwen, MultiAgent, HierarchicalAgent, and ColorMobileAgent implementations.
Configuration selects model roles.
MultiAgent uses Planner, Operator, AnswerAgent, Reflector, TrajectoryReflector, GlobalReflector, Progressor, and NoteTaker.
HierarchicalAgent adds a higher task layer.
Reflection at action, trajectory, and completion levels uses model calls.
These calls do not independently establish app-state success.

Sources: [Role assembly](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/agents/multi_agent.py#L24-L74), [hierarchical reflection flow](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/agents/hierarchical_agent.py#L297-L386).

**Exploration:** the script prepares AndroidWorld tasks and opens an app.
It requests VLM actions, summarizes visited screens, and periodically requests critic advice.
It saves screenshots, action/summary/critic JSON, and explored knowledge.
A separate RAG utility creates a vector database.
This is inference-time exploration. It does not establish a complete pipeline for model-weight training.

Sources: [Exploration entry and loop](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/utils/proactive_exploration/main.py#L98-L217), [outputs](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/mobile_use/utils/proactive_exploration/README.md#L40-L54).

**AndroidWorld mode:** a subclass adds optional accessibility descriptions to adbutils screenshots.
An adapter implements `EnvironmentInteractingAgent` and stores text answers in the AndroidWorld interaction cache.
It translates `FINISHED` into the agent done signal.
AndroidWorld task evaluators determine benchmark success.
The benchmark uses a MadeAgents fork with app-reset changes.
Scores need the corresponding benchmark configuration.

Sources: [Observation adapter](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/benchmark/android_world/mobile_use_environment.py#L18-L61), [agent adapter](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/benchmark/android_world/mobile_use_agent.py#L28-L62), [fork/reset documentation](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/benchmark/android_world/README.md#L3-L16).

**AndroidLab mode:** an executor translates `Action` into AndroidLab controller calls and operation records.
`AndroidLabEnvironment` reads executor screenshots and sends actions to the executor.
The documentation requires an AVD and recommends Linux x86_64 Docker.
A separate command runs evaluation and generates metrics.

Sources: [Action adapter and environment](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/benchmark/android_lab/mobile_use_executor.py#L27-L121), [setup and evaluation modes](https://github.com/MadeAgents/mobile-use/blob/babec07fd0e5faa7e7bcc7d3d0ee2320f6b83347/benchmark/android_lab/README.md#L21-L62).

## AndroidWorld

**Role:** AndroidWorld provides an environment and benchmark.
It initializes tasks, runs agent episodes, evaluates task state, and randomizes task parameters.
It also provides suite execution, checkpoints, and MiniWoB++ tasks on Android.
Baseline agents include random, M3A, T3A, and SeeAct selections.
A FastAPI server and Docker configuration provide remote environment control.

Sources: [Baseline selection](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/run.py#L153-L193), [HTTP environment controls](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/server/android_server.py#L89-L128).

**Platform:** `get_controller` creates AndroidEnv `EmulatorLauncherConfig` and `EmulatorConfig`.
The configuration uses console, ADB, and emulator gRPC ports.
The README requires a live Pixel 6 Android Virtual Device with API 33.
Some ADB helpers can operate independently of this factory.
They do not establish a complete benchmark for physical phones.

Sources: [Emulator factory](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/env/android_world_controller.py#L307-L333), [AVD setup](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/README.md#L39-L58).

**Observation:** `State` contains pixels, an accessibility forest, and UI elements.
The default wrapper installs and starts an accessibility forwarding app.
It obtains the latest tree through gRPC.
The controller also provides a UIAutomator tree mode.
AndroidEnv supplies screenshots. This path is broader than `uiautomator dump` alone.

Sources: [Observation contract](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/env/interface.py#L41-L71), [a11y implementation](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/env/android_world_controller.py#L127-L174), [tree-mode handling](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/env/android_world_controller.py#L227-L255).

**Actions:** `JSONAction` maps indexed elements or coordinates to ADB taps, double taps, and long presses.
Text can focus the target, clear it, type, and press Enter.
Separate branches provide navigation, swipes, scroll, drag-and-drop, and app startup.
The agent selects an action. Actuation converts an element index to coordinates.

Sources: [Actuation](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/env/actuation.py#L28-L198).

**Task-state checks:** the suite runs `initialize_task`, runs the agent, and calls `task.is_successful(env)`.
Successful completion requires both a successful task evaluator and the agent done signal.

For SQLite tasks, evaluators record initial rows and read rows after the actions.
They compare requested additions against reference rows.
These task-specific checks provide evidence beyond model FINISH.
They are not a universal evaluator for arbitrary apps.

Sources: [Episode evaluation boundary](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/suite_utils.py#L223-L275), [SQLite verification](https://github.com/google-research/android_world/blob/e3fea3ccc69787570e282c99573298f1c3019a34/android_world/task_evals/common_validators/sqlite_validators.py#L271-L310).

## Cross-project interpretation for AUV

AppAgent, MadeAgents, and AndroidWorld use observation/action loops over foreground Android state.
AppAgent records reusable UI knowledge. MadeAgents combines model planning, reflection, and benchmark adapters.
AndroidWorld provides repeatable initialization and independent task-state checks.

AndroidWorld separates the agent done signal from `is_successful`.
This distinction illustrates the AUV separation between input delivery and semantic verification.
Model reflection or ADB completion alone does not establish app-state success.
Portable Python schemas do not establish iOS or OpenHarmony drivers.

## Reuse questions for AUV (research candidates only)


- mobilecli and agent-device provide references for separate device and frontend APIs.

- Physical iOS and simulators need different lifecycle and signing models.

- Observation records need to distinguish native hierarchy, OCR text, and screenshots.

- Input delivery, semantic verification, and run completion describe different results.

- A future OpenHarmony investigation needs a specific distribution, API level, and device before hardware checks.

These are reference questions, not an implementation plan or authorization to add new AUV platforms.
