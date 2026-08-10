# Orca Computer Use 与 AUV：能力与实现对照

> 研究快照：2026-08-09。这里的 **Orca** 专指
> [`stablyai/orca`](https://github.com/stablyai/orca)，其自述为 AI coding-agent
> ADE；不是同名的工作流或终端 agent 项目。外部源码引用固定在提交
> [`04f7123`](https://github.com/stablyai/orca/tree/04f7123d26921795a3e582a2e0713bcb0f2b1076)，
> 避免 `main` 漂移。能力状态区分为 *已实现并公开*、*AUV 当前 macOS
> 证据* 与 *未证明/刻意未做*，不是路线图承诺。

## 结论

Orca 的 Computer Use 是一个面向通用 coding agent 的产品化桌面控制面：统一
`orca computer` CLI 先读 accessibility tree + screenshot，按短生命周期 element
index 执行语义动作，并返回新的 snapshot；它在 macOS/Linux/Windows 均带有提供者。
AUV 当前的核心更窄也更可审计：macOS-first typed driver/operation、输入路径和
扰动元数据、run tracing 与 durable artifacts，且明确把输入投递和语义验证分离。

因此两者并非简单的“谁功能多”：Orca 领先于**跨平台、可直接被任意 CLI agent
消费的完整桌面操作表面**；AUV 领先于**对投递事实、回退、扰动、artifact/run
recording 和 app-owned verification 的契约约束**。AUV 目前不应把 Orca 的宽 CLI
直接当作自己的共享 runtime 或重复建立另一套 action-result schema。

## 完整能力矩阵（当前可核验表面）

| 能力维度 | Orca Computer Use（公开实现） | AUV 当前可核验表面 | 关键差异 / 证据等级 |
|---|---|---|---|
| 产品边界 | ADE 中给 coding agents 使用的通用本机桌面 CLI；也以 skill/MCP 分发。 | 面向可检查、可回放的 typed operation/driver/run-artifact 核心，不是 agent IDE。 | 两者前端目标不同；不要把 Orca 的 worktree/agent orchestration 误归为 Computer Use 或 AUV 缺项。Orca [README](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/README.md#L188-L242)，AUV [术语](../../../TERMS_AND_CONCEPTS.md#runtime-responsibility)。 |
| 受支持桌面平台 | macOS 原生 Swift helper；Linux AT-SPI/Python；Windows UI Automation/PowerShell；统一 provider capability response。 | live invoke 命令明确为 macOS-only；`auv-driver` 的本地选择器含 Linux/Windows adapter，但本表不把枚举当作各项功能证据。 | Orca 在平台覆盖上已产品化；AUV 的当前已验证核心 lane 是 macOS。Orca [provider source](https://github.com/stablyai/orca/tree/04f7123d26921795a3e582a2e0713bcb0f2b1076/native)，AUV [LocalDriver](../../../../crates/auv-driver/src/lib.rs)。 |
| 运行 app / 窗口枚举 | `list-apps`、`list-windows`，支持 bundle ID、名称、pid，窗口可用 ID/index 选择。 | `window.list` 与规范化 `WindowSelector`；`app.activate`，但没有公开的 app-list invoke 命令。 | Orca 更适合 agent 从零发现目标；AUV 更偏调用方已带 target 的 typed operation。Orca [docs](https://www.onorca.dev/docs/cli/computer-use#selecting-an-app)，AUV [window command](../../../../crates/auv-cli-invoke/src/commands/window.rs)。 |
| 无障碍树观察 | 每次 `get-app-state` 返回 tree text、element frame、焦点、窗口和 truncation；element index 只对最新 snapshot 有效，macOS action 前还会用 element signature 对 fresh snapshot 复核。 | macOS driver 有 AX tree/native tree 模块；当前 invoke 金路径主要是窗口/屏幕 capture + OCR，未见同等统一的“取树再按 index 操作”公开命令。 | Orca 的 agent-consumption contract 更完整；AUV 的 AX 是 driver capability 而非已统一暴露的 Computer Use ABI。Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L38-L49)，[snapshot/signature gate](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L422-L445)。 |
| 图像观察 | 默认返回目标窗口 screenshot；JSON CLI 将图片写到受限临时路径并返回 path；可 `--no-screenshot`。 | `display.capture`、`screen.captureRegion`、`window.capture` 会发出 PNG artifact，capture 也有坐标/后端元数据。 | Orca 优化单次 agent loop payload；AUV 优化 run 内可检查的 durable evidence。Orca [docs](https://www.onorca.dev/docs/cli/computer-use#screenshots)，AUV [capture frame 术语](../../../TERMS_AND_CONCEPTS.md#capture-frame)。 |
| OCR / 视觉定位 | provider capability 在 macOS/Linux/Windows 均声明 `ocr: false`；依赖 agent 看 screenshot 或读 AX tree。 | macOS invoke 有 `screen/window.findText`、`waitForText`、`clickText`，用原生 OCR 结果投影到点击点。 | AUV 在已证明的 macOS OCR anchor 上更强；Orca 不将 OCR 声称为 Computer Use 能力。Orca [macOS capabilities](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L478-L520)，AUV [screen commands](../../../../crates/auv-cli-invoke/src/commands/screen.rs)。 |
| 语义 click / secondary action | `click --element-index` 尝试 `AXPress`/`AXConfirm`/`AXOpen`（右键 `AXShowMenu`）；显式 `perform-secondary-action` 只接受该 element 公布的 action。 | 输入路径模型有 `AxPress`/`AxFocus`/`AxSetValue`/`AxScroll`；现有公开文本点击走 OCR point + typed input。 | 两者均优先 semantic action；Orca 已将其统一成 index CLI，AUV 更强调 driver-level path facts。Orca [implementation](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L728-L800)，AUV [InputDeliveryPath](../../../../crates/auv-driver-common/src/input.rs)。 |
| 原始指针 / 拖拽 / 滚动 | click 坐标回退、drag、scroll；坐标为 window-local，依据 screenshot scale 转换。 | click/scroll 的 window-relative typed trait，且还有 mouse motion、key/text/paste invoke 命令。 | AUV 对坐标类型与投递路径更强；Orca 有统一跨平台 command 可立刻调用。Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L90-L117)，AUV [WindowInput](../../../../crates/auv-driver-common/src/input.rs)。 |
| 文本和值输入 | `set-value` 直接 AX write 后 read-back；否则 `type-text` / `paste-text` / key/hotkey 走焦点依赖的合成输入。 | `input.focusText`、`axFocusText`、`typeText`、`pasteText`、`key`；input action 的 semantic `verified` 只允许明确 read-back 后为真。 | 语义一致；AUV 将“未验证投递”提升为项目级不变量，Orca 在该 API 里把 verified/unverified metadata 随 action 返回。Orca [set-value/type](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L803-L834)，AUV [result invariant](../../../../crates/auv-driver-common/src/input.rs)。 |
| 回退与前台影响 | semantic AX click 不可用时，用 element frame 的 synthetic click；有 `restore-window`，并提示焦点/遮挡限制。 | `InputActionResult` 固化 selected path、所有 attempts、鼠标/焦点/剪贴板 disturbance；input mode 明确 background-only/preferred 与 foreground-preferred。 | AUV 的事实模型更细；Orca 以 `path`、`fallbackReason` 和使用指南表达，未见等价的 disturbance 三元字段。Orca [fallback](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L739-L774)，AUV [InputActionResult](../../../../crates/auv-driver-common/src/input.rs)。 |
| 行为后的验证 | 每项 action 都返回当前 snapshot，文档要求 snapshot → act → snapshot；`set-value` 可值 read-back，其余 synthetic 是 unverified。 | 规定 input delivery 与 application semantic verification 分离，后者为 app-owned typed result/event/artifact；不从成功 dispatch 推断成功。 | 两者都拒绝“点击成功=任务成功”；AUV 的边界更严格、更适合 durable inspection。Orca [loop](https://www.onorca.dev/docs/cli/computer-use#snapshot--act--snapshot)，AUV [semantic verification](../../../TERMS_AND_CONCEPTS.md#semantic-verification)。 |
| 权限与敏感数据 | 有 capabilities/permissions status；macOS 要 Accessibility + Screen Recording；安全阻止若干密码管理 app；stdin 避开 shell history（但 Linux/Windows action payload 仍短暂落操作文件）。 | macOS probe 屏幕录制、ScreenCaptureKit、Accessibility、Automation；运行记录与 artifact 走 tracing store。 | Orca 对 agent 操作的风险提示/拒绝名单更产品化；AUV 可借鉴其 secret transport 警示，但不能把临时文件视为安全边界。Orca [guide](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L20-L31)，[Linux/Windows caveat](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/skill-guides/computer-use.md#L80-L88)，AUV [permission command](../../../../crates/auv-cli-invoke/src/commands/app.rs)。 |
| Agent 接入方式 | 分发很短的 discovery stub，agent 再用 `orca skills get <topic>` 从运行时取版本匹配的完整 guide；命令建议 `--json`；也可注册 MCP。 | CLI 与内建 MCP 解码到同一 command-local typed input；frontend 自己拥有 run context。 | Orca 的“thin stub + live versioned guide”是可借鉴的分发思路；AUV 的 typed command routing/run ownership 已更贴合其架构。Orca [skills docs](https://www.onorca.dev/docs/cli/skills#hybrid-stubs-vs-the-live-guide)，AUV [CLI invoke boundary](../../../TERMS_AND_CONCEPTS.md#cli-invoke-boundary)。 |
| tracing / artifact / inspection | screenshot 是临时 CLI 输出；本次调查未找到 Computer Use 作为 append-only run record/artifact/reader 的公开 contract。 | `auv-tracing` 持久化 span/event/artifact metadata + bodies，inspect 是读侧；capture/operation 可发 artifact receipt。 | 不能由“Orca 有 screenshot”推断它有 AUV 的可检查 run 记录。AUV [artifact/inspect terms](../../../TERMS_AND_CONCEPTS.md#artifact)。 |
| 回放 | 本次资料仅证明 agent 可重复执行 CLI loop；未证明 Computer Use 有 durable UI replay contract。 | 项目使命与 artifacts/run 为未来 replay 提供基础；不能将此表写成“已经实现通用 UI replay”。 | 两方均不可在当前证据下宣称完整通用 desktop replay。 |

## 实现结构（源码可见）

```text
Orca agent / `orca computer` CLI
  -> TypeScript RPC methods + single-flight Node sidecar
  -> provider capability handshake
  -> macOS: authenticated Unix-socket Swift helper
     Linux: per-operation JSON file -> Python AT-SPI/GDK bridge
     Windows: per-operation JSON file -> PowerShell UIAutomation/Win32 bridge
  -> snapshot (AX tree + optional PNG) / semantic action -> synthetic fallback
  -> next snapshot returned to the agent

AUV typed command or app operation
  -> `auv-driver` capability (macOS Swift via `swift-bridge`)
  -> `InputActionResult` + direct app-owned result
  -> tracing event/artifact receipt (frontend owns run context)
  -> separate app-owned semantic verification when required
```

直接证据：Orca 的 RPC 命令表在
[`computer.ts`](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/runtime/rpc/methods/computer.ts)，
sidecar 将调用排队、超时后销毁并在下一请求重启（[client](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/sidecar-client.ts#L118-L313)）；macOS helper 在 handshake 中公开各 capability，并使用 authenticated socket
([client](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/macos-native-provider-client.ts#L28-L219))。
Linux 使用 PyGObject 的 `Atspi`/`Gdk`，Windows 使用 .NET `UIAutomationClient` 与 Win32 `SendInput`，而非一套跨平台 GUI library（[Linux](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-linux/runtime.py#L1-L43)，[Windows](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-windows/runtime.ps1#L1-L136)）。

macOS action handler 会把 action 后的 current snapshot 带回（允许一个有界的
window-change 情形），因此其“重新观察”不只是 guide 中的建议；Linux/Windows 则让
TypeScript client 缓存短生命周期 snapshot，以 index/action request 建立对应关系。
前者见 [response construction](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/native/computer-use-macos/Sources/OrcaComputerUseMacOS/main.swift#L245-L258)，后者见 [script-provider cache](https://github.com/stablyai/orca/blob/04f7123d26921795a3e582a2e0713bcb0f2b1076/src/main/computer/desktop-script-provider-client.ts)。

这意味着“Orca 使用某视觉模型/视觉 agent loop”并不成立：公开 provider 自报
`ocr: false`，源码显示的是 OS accessibility + screenshot 基元；由上层任意 CLI
agent 决定如何推理截图和何时循环。这是源码推断，不是 Orca 对视觉模型的产品声明。

## 可借鉴的最小切片（不是本次批准的实现）

1. **能力协商 + 显式降级原因。** Orca 的 provider handshake 将 apps/windows/
   observation/actions 分开，前端先拒绝 unsupported action。AUV 可以在现有 driver
   descriptor/protobuf boundary 增补 *已实现的* capability query，并让 direct result
   复用 `InputActionResult` 的 attempts/fallback facts；不新建并行 action schema。

2. **“fresh observation token” 约束。** Orca 将 element index 限于最近 snapshot，
   强制 agent 在导航、滚动、重绘后重读。AUV 若批准 AX/recognition candidate-to-action
   slice，可把 source artifact / surface identity / freshness 作为候选消费前校验，避免
   OCR 坐标或 AX node 静默过期；这应接到现有 evidence/artifact contract，而不是进 CLI
   本地缓存。

3. **版本匹配的 agent guide。** Orca 的 stub 不复制 flags，运行时提供完整 guide，降低
   skill 文档与二进制漂移。AUV 可在 owner 批准 agent-facing distribution 后采用该模式，
   但 guide 必须把“delivery 不等于 semantic success”和 artifact/run evidence 写成强制
   规则。

4. **不宜直接照搬的部分。** Linux/Windows 每操作 JSON 临时文件与 PowerShell/Python
   bridge 有交付价值，但对 secret 与 durable evidence 都不是合适的 AUV core boundary；
   AUV 的 Rust typed driver + `swift-bridge` 已是 macOS 主线。跨平台扩展必须先有
   owner-approved producer/consumer 和对应证据，不能因 Orca 已覆盖三个系统而扩面。

## 待验证边界

- 本笔记没有实际在三种 OS 上运行 Orca，故“可用”表示源码与官方文档声明，并非独立
  live probe。
- Orca 的截图临时导出不等同于未记录：这里只说明本次审阅未找到与 AUV 同等的公开
  append-only run/artifact reader contract。
- AUV 的 Linux/Windows crate 存在不构成此表每项功能已支持的证据；若需要平台矩阵，
  应另做每个 driver 的 live evidence pack。
