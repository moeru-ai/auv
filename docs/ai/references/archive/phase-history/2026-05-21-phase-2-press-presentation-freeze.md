# AUV Phase 2 Press / Presentation Freeze

Date: 2026-05-21

Status: accepted freeze decision

## 决策

Phase 2 的「按一下界面上的某个东西」语义被冻结。

这并不意味着所有应用、所有控件都能被按下。它意味着 macOS 驱动现在对
**「怎么按」**、**「按之前怎么呈现」**、**「按完留下什么证据」**
三件事有了显式契约，下一阶段应该基于这套契约去做更上层的 skill 抽象，而不
是再回到「我们要不要支持 AX」「我们要不要 warp 光标」这一类的讨论。

## What Phase 2 Actually Delivered

三个 macOS 驱动入口，外加一个底层捕获修复：

- `debug.axPressButton` — 直接给一个查询词，在目标 app 的 AX 树里挑出最匹配
  的可按节点，然后通过 `AXUIElementPerformAction(AXPress)` 触发。光标完全
  不动。
- `debug.axClickWindowText` — OCR 找到窗口里的可见文本，把锚点投影到 AX 树
  上的可按节点，再走 `AXUIElementPerformAction`。光标完全不动。
- `debug.smartPress` — 先尝试 `axClickWindowText`，失败时按需回退到
  `clickWindowText` (Quartz 点击 + cursor warp)。回退路径在六个独立的
  surface 上都被显式标记。

底层修复：

- `kCGWindowSharingState == 0` 的窗口（在本机就是绝大多数窗口）原本被 xcap
  在 enumeration 阶段就静默跳过，导致所有窗口级 OCR 命令报
  `capture.stale_window_ref`。
- 修复策略是绕开 xcap 的 enumerate→capture 路径，改用 `objc2-core-graphics`
  在进程内直接调用 `CGWindowListCreateImage(.optionIncludingWindow, …)`，
  以父进程已经持有的 Screen Recording TCC 权限完成捕获。
- 这一修复同时让 `findWindowText` / `clickWindowText` / `axClickWindowText`
  以及未来任何窗口级 OCR 命令都不再被 sharing state 卡住。

## 共享语义（被冻结）

以下字段名和取值是 Phase 2 三个命令的契约表面，下游 skill 应该围着这套字
段去判断 / 断言，而不是去解析 summary 文本。

### pressMechanism × cursorDisturbance

| 命令                      | pressMechanism | cursorDisturbance | 备注                                  |
| ------------------------- | -------------- | ----------------- | ------------------------------------- |
| `debug.axPressButton`     | `ax-action`    | `none`            | 仅 AX，不依赖 OCR                     |
| `debug.axClickWindowText` | `ax-action`    | `none`            | OCR 定位 + AX 触发                    |
| `debug.smartPress` (AX)   | `ax-action`    | `none`            | 走第一选择                            |
| `debug.smartPress` (fb)   | `pointer-click`| `warp`            | AX 失败后回退；`fallbackUsed=true`    |
| `debug.clickWindowText`   | `pointer-click`| `warp`            | 旧路径，仍然保留作为 smartPress 回退   |

`cursorDisturbance` 是 Phase 2 引入的新签名，下游 skill 可以用它来声明「我
要求 zero disturbance」或者「我接受 warp」。

### overlay 呈现生命周期

任何带 `--overlay true` 的命令产生：

- `overlayPresentation=visual-only`（光标不动，只在 target 位置画一个
  marker；不存在 `pointer-warp` 的 overlay 模式）
- `overlayShowEvent=<event>` / `overlayHideEvent=<event>` — overlay 守护
  进程的实际事件，不是 wishful "started"
- `daemonPid=<pid>` — 验证 overlay 守护实际起来过
- `previewMs` / `settleMs` — 用户控制 marker 可见时长和事后等待

`debug.axClickWindowText` 默认 `overlay=false`，`debug.smartPress` 默认
`overlay=true`。设计意图：`smartPress` 是 demo / 验证入口，需要肉眼看见；
`axClickWindowText` 是无侵入入口，默认不画任何东西。

### OCR → AX 投影

`debug.axClickWindowText` 的 artifact 上同时记录五个点 / 矩形：

- `ocrMatchBounds` — Vision 给出的像素矩形
- `ocrAnchorLogicalPoint` — 把 OCR 中心反投到全局逻辑坐标后的点
- `anchorOffset` — 用户传入的偏移
- `axResolvedLogicalPoint` — `ocrAnchor + offset`，也就是去 AX 树里查找的点
- `axNodeCenter` — 真正触发 `AXPress` 用的节点中心

并且 `matchedRole` / `matchedDescription` / `matchedBounds` 记录命中的 AX
节点。下游可以靠 `axResolvedLogicalPoint` 是否落在 `matchedBounds` 内来判
断 OCR 锚点是不是确实压在 AX 节点上，而不是命中了相邻控件。

### smartPress 的六面审计

smartPress 在以下 **六个独立 surface** 上都标记当前实际走的策略：

1. `signals["smartPress.strategy"]` — `ax-action` 或 `pointer-click`
2. `signals["smartPress.fallbackUsed"]` — `true` / `false`
3. `notes` 至少三行：`smartPress=true`、`smartPressStrategy=…`、
   `smartPressFallbackUsed=…`，回退时额外追加 `smartPressPrimaryError=…`
4. `summary` 前缀：`"Smart press used <strategy>: …"`
5. `backend` 后缀：`"macos.smart-press.<strategy>"`
6. 专用 artifact：`smart-press-{query}.txt`

并且底层操作自己的 artifact / signals（例如 pointer-click 的
`cursorDisturbance=warp` 和它自己的 click 报告）会一并保留在 response 里，
不会被 smartPress 这一层吞掉。下游做 disturbance 审计时，仍然能拿到底层操
作的原始证据。

## Freeze 范围

被冻结意味着以下名字进入「不要改」的稳定集合，要改需要走 v2 / new
command id：

- 命令 id：`debug.axPressButton`、`debug.axClickWindowText`、
  `debug.smartPress`
- 字段名：`pressMechanism`、`cursorDisturbance`、`performedAction`、
  `availableActions`、`overlayPresentation`、`overlayShowEvent`、
  `overlayHideEvent`、`daemonPid`、`previewMs`、`settleMs`、
  `ocrMatchBounds`、`ocrAnchorLogicalPoint`、`anchorOffset`、
  `axResolvedLogicalPoint`、`axNodeCenter`、`matchedPath`、
  `matchedRole`、`matchedDescription`、`matchedBounds`、
  `smartPress.strategy`、`smartPress.fallbackUsed`、
  `smartPressPrimaryError`
- artifact kind：`ax-press-button`、`ax-click-window-text`、`smart-press`
- backend id：`macos.ax.perform-action`、
  `macos.ax.perform-action+overlay-ffi`、
  `macos.ax.click-window-text`、
  `macos.ax.click-window-text+overlay-ffi`、
  `macos.smart-press.ax-action`、`macos.smart-press.pointer-click`

## Accepted Unresolved Boundary

Phase 2 接受以下边界，**不在这一阶段强行解决**：

- **Canvas / WebView 渲染文本** — `axClickWindowText` 找不到对应 AX 节点
  时显式报错并提示走 `debug.clickWindowText`。`smartPress` 在这种情况下
  会自动回退到 pointer-click，记录 `fallbackUsed=true`。这是「降级到
  warp」，不是「假装无干扰」。
- **Cursor warp 视觉抖动** — `clickWindowText` 路径仍然走
  `CGWarpMouseCursorPosition`，会有可观察的瞬时光标跳跃
  （见 `2026-05-20-cursor-warp-jitter-smoke.md`）。Phase 2 没有引入
  virtual cursor / save-restore wrapper；smartPress 的 fallback 路径继承
  这个抖动。
- **Vision OCR 误识** — 例如本次验收里 RustRover 的 ▶ Run ⌃ 被识别为
  `"® Run v"`。匹配仍然成功是因为 substring `Run` 命中；OCR 原文保留在
  `ocrMatchText` 中以便人工核对，但驱动层不做 OCR 校正。
- **多窗口同应用** — 当前 OCR 在「resolved window」上工作，由
  `capture_resolved_window_observation` 选窗。同 bundle 多窗口（如多个
  浏览器窗口）需要 caller 自己用更具体的 selector 锁定。
- **smartPress 默认值** — `allow_pointer_fallback` 默认 `true`、
  `overlay` 默认 `true`。这是 debug 入口的「让它能跑」默认；产品级 skill
  应该显式传 `allow_pointer_fallback=false`，并依赖 `cursorDisturbance`
  signal 来断言。

## What This Freeze Does Not Mean

不要把这次 freeze 翻译成更大的产品承诺。它 **不** 意味着：

- macOS 上任意 app 的任意按钮都能被 AX press（AX 树覆盖度取决于 app 实现）
- smartPress 是一个「always succeeds」的通用按钮（它会失败，且失败时不会
  伪装成功）
- cursor warp 已经被解决（pointer-click 路径仍然有抖动；目前只是把它移到
  smartPress 的回退位置）
- OCR → AX 的投影对所有应用都准确（缩放、Retina、子窗口、popover 仍然可能
  把锚点投到错误位置；artifact 已经把所有中间坐标都记录了，便于诊断，但驱
  动层不自动重投）

## What Phase 3 Should Focus On

按优先级：

1. **把 `cursorDisturbance` 提到 skill contract 一级字段**。下游 skill 应
   该能声明 "I require `cursorDisturbance=none`"，case matrix 应该能在不
   满足时直接 fail，而不是依赖人工读 artifact。
2. **smartPress 的 narrow 化**。今天的 smartPress 是 debug 级别。Phase 3
   应该把 smartPress 的策略选择封装进 skill 层，让一个 narrow skill 显式
   声明 "ax only" 或 "ax with named fallback"，而不是依赖 debug 命令的
   默认值。
3. **canvas / web view 路径**。`axClickWindowText` 在 canvas 上失败是显
   式的；Phase 3 可以考虑引入 chrome MCP / web app MCP 作为这一类目标的
   专用通道，而不是继续推 OCR + warp。

如果 Phase 3 立刻又退回到「再支持一种 AX 角色」「再加一个 OCR 后处理规
则」，那不是进展，那是回到 Phase 1 / Phase 2 的探索循环。
