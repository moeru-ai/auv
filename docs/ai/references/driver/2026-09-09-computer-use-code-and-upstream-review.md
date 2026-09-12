# AUV 与 computer-use 组件：源码差异、近期上游与路线判断

> NOTICE: Historical source review at the revisions recorded below. For the 2026-09-13 decision, subsequent implementation PRs, and Wayland evidence boundaries, read [Wayland background input research](2026-09-13-wayland-background-input-research.md). Candidate rows are not implementation approval.

Research date: 2026-09-09. Classification: docs-only research. This is a source-level review and a set of candidate follow-ups, not an approved implementation plan.

Follow-up: [改进点清单](2026-09-09-computer-use-improvement-candidates.md) maps reference implementations to individual AUV gaps, trigger flows, exact source lines, and acceptance criteria, with additional scripting, Windows UIA, and multi-device research.

## 结论与证据边界

AUV 的 typed operations → driver → InputActionResult → run/artifact，以及将输入投递与语义验证分开的方向有价值。此次没有发现足以要求推翻 Rust/Swift、gRPC 或 driver/tracing 分层的证据。但在通用 OS automation 上，“非常 frontier”目前不能等同于实现领先：Peekaboo 和 CUA 在目标身份、部分投递、并发资源生命周期和独立接收端验证上，已经有比 AUV 更完整的实现。[AUV 结果契约][a-input]、[Peekaboo outcome][p-outcome]、[CUA 行为证据][c-tests]。

最确定的发现是：**一处点击次数契约不一致、一处 CI 测试选择缺口，以及同一操作在不同入口的能力差异**。另外发现三组应优先复现的正确性风险：投递与重试、目标身份、截图几何。这些不等于已复现的生产事故，也不能换算为“走歪了几条路线”或完成百分比。

证据标签：

- **源码确认**：当前代码的分支、数据流或测试配置直接成立；不等于已运行 GUI 复现。
- **风险推断**：从源码推导出可发生的竞态或失败条件，需要独立接收端/故障注入验证。
- **能力缺口/主动延后**：缺公共契约或生产消费者，不自动视作 bug。
- **上游测试证据**：审阅了上游测试/记录；本次没有重跑，不是 AUV 的行为证明。

三名研究 agent 分别审阅 Peekaboo、CUA、KWWK；主审阅 AUV 与补充项目，并让另一 agent 复核 CI 和入口差异的反证。未修改生产代码、运行构建或操作桌面应用；未做跨框架成功率、延迟或吞吐基准。

## 修订与研究范围

| 项目 | 固定版本 | 范围 |
| --- | --- | --- |
| AUV | `bae42bf9905614b19347566d5d41b3a9998e8a35` | 当前本地代码；driver、invoke、Runner、MCP、tracing、CI 与已接受契约 |
| Peekaboo | `6cd38c0d319ab8db52b05d22409072efc4a20021` | macOS 原生 services、CLI/MCP、测试、近期提交 |
| CUA | `467c103be28384502cdd77b9edd5ea46da0b8ded` | 最新 native cua-driver；9 月 9 日新提交补审；云和 agent 层单列 |
| KWWK main | `acc65213baa725b31153609e651f166fdaf79272` | Coding-agent host/SDK；不能代表整个 KWWK desktop 生态 |
| KWWKComputerUseCore | `5201e300ceb58f2aaf501b20477aff5a912efb9a` | 真正的独立 macOS automation core；AUV 已引用其代码 |

“最近”主窗口为 **2026-08-10 至 2026-09-09**；7 月、8 月初的相关实现明确标为前置进展。CUA 用户原有 checkout 保持 `02fdd98`，最新审阅在临时研究 checkout；没有切换用户分支。

本报告补充并修正[先前能力表](2026-09-09-computer-use-framework-comparison-note.md)的两处边界：KWWK main 与独立 native core 必须分开；AUV 默认本地 invoke/MCP 与 selected Runner 的平台可用性必须分开。

## 代码层横向比较

| 技术边界 | AUV 当前实现 | Peekaboo 优点 | CUA 优点 | KWWK native core 优点与限制 |
| --- | --- | --- | --- | --- |
| 原生输入路径 | typed path/attempts/disturbance；macOS 默认 compatibility click；keyboard 已有目标预校验和部分进度 | 单事件选择一个 transport，区分拒绝、部分投递、效果未知 | 新点击路径避免双投递；typed effect/evidence；后台按目标可证明性拒绝 | Swift 定向事件与 AX 动作较紧凑；部分机制来自 CUA，不能视为独立验证 |
| 观察后再操作 | AX 路径含 child index 与 expected role；尚缺一致的公开 snapshot/action 契约 | snapshot + process generation + window receipt；操作时重验证 | runtime/PID/window scoped token、fresh AX ancestry 与失效拒绝 | action 消费 session 最近快照，再取树匹配；打分匹配没有完整歧义拒绝 |
| 坐标与截图 | 已有 ScreenPoint/WindowPoint；macOS 截图结果可能带调用方旧 frame | 检查 raster extent、密度和目标；拒绝无法证明的几何 | capture 后复核 owner/layer/frame，变化后重建一次 | 坐标动作检查窗口 frame 改变；不是通用跨平台几何层 |
| 操作生命周期 | 有 mouse-motion mutex、移动断流停止、剪贴板锁；同步 native call 取消与 hold 尚有限制 | held input 的 owner/watchdog/释放，原子组合操作与 capture ownership | 每 PID mutation lease；SDK shutdown 等待调用、清理 token、停止 recording | session.finish/deinit 恢复 monitor；drag 显式 release，不是完整取消模板 |
| 语义效果 | 默认 verified=false，允许 app-owned 独立验证；缺通用 predicate service | 对有限非安全文本控件做精确 readback，其余保持 unverified | confirmed/partial/refused/unknown 有约束，独立 verifier 不把不完整 AX 树当缺失证明 | action 后返回 settled snapshot；稳定画面不等于完成用户意图 |
| 入口复用 | registry、typed inputs、记录已共享；直接 invoke/MCP 与 selected Runner 仍有行为分叉 | Swift service 接 CLI/MCP/Bridge | 多语言 SDK、daemon 共用 typed native runtime | 简洁 Swift client 与 session facade |
| 平台与验收 | 三平台代码已在；平台/入口不等价；CI 未选中许多 owning unit suites | 深耕 macOS 原生窗口、菜单、对话框与输入边界 | toolkit fixture + 外部 oracle + 明确 refusal；Wayland 按 compositor 逐项适配 | macOS 专项；不提供 CUA 式跨平台行为矩阵 |

依据：[AUV input][a-input]、[AUV keyboard][a-keyboard-session]、[AUV geometry][a-geometry]、[Peekaboo pointer][p-pointer]、[Peekaboo actions][p-actions]、[CUA token][c-token]、[CUA output][c-output]、[CUA testkit][c-tests]、[KWWK client][k-client]、[KWWK core][k-core]。各项的具体限制见以下审查。

## 应优先处理的具体差异

### 1. 默认 compatibility click：旧 workaround 需要重新验证

**源码确认：** AUV `postCompatibilityMouseEvent` 对同一事件先调用 SkyLight，再调用 public `postToPid`；默认路由采用 ChromiumCompatible。兼容函数把请求的 click count 限制到最多 2 对目标点击，而 invoke 接受 1–255。三击请求在这一 native 分支无法保持所声明的次数。[投递][a-pointer]、[次数限制][a-count]、[入口验证与路由][a-runner-click]。

**上游差异：** Peekaboo 8 月 9 日 [f2edb8be](https://github.com/openclaw/Peekaboo/commit/f2edb8be4fa8fc6d1cc66f6ce1fbbcda506b5c53) 已选择单 transport；CUA 9 月 9 日 [467c103](https://github.com/trycua/cua/commit/467c103be28384502cdd77b9edd5ea46da0b8ded) 修复相关双投递，以独立 AppKit 接收端核对事件次数。当前 CUA foreground 用 public，特定 background route 用 SkyLight，仅在 symbol 不存在时回退 public。[当前 CUA pointer][c-pointer]。

**风险推断：** AUV 在某些 toolkit 中可能重复投递；本次没有现场复现。CUA 仍有其他使用 Both 的 helper，不能说全仓所有路径都已解决。AUV 的 offscreen primer 又有特定应用的既有需求，不能根据这个发现直接删除全部兼容逻辑。

**路线判断：** 把 toolkit-specific workaround 当通用默认值值得纠正；不是 CGEvent 或原生 API 方向错误。先用 AppKit/Electron 独立计数器验证 count=1/2/3、实际收件窗口与 transport，再决定条件化策略。

### 2. 部分文字已发送后，fallback 可能重放全文

**源码确认：** Swift TypeText 逐字符创建并发送事件；后续分配失败返回普通错误，没有携带已投递字符数。Rust 在 foreground 且显式允许 clipboard fallback 时，仍可能粘贴整段原文。[逐字符发送][a-keyboard]、[fallback][a-fallback]。

**风险推断：** 如果错误发生在已有前缀投递之后，特别是 `replace_existing=false`，可能重复文本。是否发生、概率多高需要故障注入；不能从代码推导常态失败。

Peekaboo 8 月 18 日 [86b7d102](https://github.com/openclaw/Peekaboo/commit/86b7d10298d4f903b9122e252ec4a4c88b0b3de3) 显式处理 input prefix，部分投递后阻止普通 fallback replay，并区分 retry safety。[pointer 错误分类][p-pointer]、[outcome][p-outcome]。

AUV 的[已接受 keyboard 契约][a-keyboard-contract]本来就允许 foreground opt-in clipboard fallback，并记录 TypeText 内部进度不可测；它已经报告完整 action/repetition 的进度，background 也拒绝这种 fallback。应补足“未开始 / 已部分发送 / 无法确定”的失败边界，而不是否定全部 fallback 或声称完全没有部分进度。

### 3. “PID/窗口存在”与“仍然是观察到的接收目标”有差距

AUV keyboard 会固定 recipient、重复前复核 PID/window owner，不能称为盲目全局输入。但 PID 投递键盘事件仍是进程范围，后台 `require_window_focus=false`；缺少同进程多窗口竞争检查与贯穿 proof→dispatch 的每 PID 协调。[keyboard][a-keyboard-session]、[native 检查][a-window-check]。

AUV AX path 的另一具体边界是：从 `axFirstWindow` 开始按 child index 遍历，末端主要检查 expected role。同 role 节点替换或首窗口变化可能绕过这个检查。[AX resolution][a-ax]。这些是风险推断，不是已复现误点。

CUA 8 月 5 日 [1b2cb5a7](https://github.com/trycua/cua/commit/1b2cb5a706c3e5d636b683ab15336dbf35e579e0) 对 GenericKey/InsertText 拒绝竞争的同 PID 窗口，并使用[每 PID mutation lease][c-lease]；它的[token registry][c-token]让新快照使同窗口旧引用失效。Peekaboo 在 8 月强化 process generation 和 mutation receipt。KWWK core 会重新捕获并匹配签名，但其“最高分匹配”本身不保证无歧义。[CUA 检查][c-background]、[KWWK 重解析][k-core]。

应借鉴目标身份、过期拒绝和投递期间的有效性边界；**不要恢复已退休的 candidate_promotion/stability，也不要先造无消费者的通用 token 系统。**

### 4. 新截图像素可能搭配旧窗口 frame

AUV Swift 重新找到 SCWindow、用它的尺寸截图；Rust 返回值却采用调用者旧 `window.frame`，并用旧宽度推导 scale。窗口在 resolve 与 capture 之间移动/缩放时，像素和坐标元数据可能来自不同观察。[Swift capture][a-capture]、[Rust result][a-capture-result]。

CUA 的[post-capture validation][c-capture]检查 owner/layer/frame，变化后重建一次，再变则拒绝。Peekaboo 9 月 5 日 [620563ac](https://github.com/openclaw/Peekaboo/commit/620563ac2f98a391405af681bc6db085c8b2f2d3) 加强 popup/sheet 的 raster extent 与密度验证。[几何代码][p-capture]。

这不是相同 backend 的同一个 bug：Peekaboo 该修复针对 classic capture，AUV 此处用 ScreenCaptureKit。共同要学习的是“图像、坐标、窗口身份来自一致观察”的契约。

### 5. 多客户端、按住输入、取消：缺的是生命周期契约

AUV 已有移动序列锁、断流停止后续样本、跨进程剪贴板锁；不能说没有并发治理。[移动生命周期][a-motion]。但 MCP 取消 future 不能中断同步 native call；独立 key/button down/up 明确延后到可靠释放语义存在。[取消边界][a-cancel]、[hold 延后][a-hold]。

截图也有具体边界：Swift semaphore 等待超时不等于底层 callback 已结束；上层随后选择另一 capture backend，存在操作重叠风险。[capture][a-capture]。这不是测得的卡死或吞吐问题。

Peekaboo 8 月新增/加强 held-input watchdog、原子 focus+typing/modifier click，以及 SCK owner 协调；CUA 9 月 4 日加强 shutdown drain/worker join/recording cleanup。[CUA runtime][c-runtime]。借鉴时要按资源作用域协调，而非加一把全局锁。KWWK 的 session finish 值得看，但它的 drag handle 是显式 release，不能拿来证明取消安全。[KWWK session][k-session]、[drag][k-drag]。

### 6. 已有驱动能力尚未一致接到各入口

源码调用关系：

```text
invoke（未选择 runner） → InvokeCommand.invoke
MCP command_adapter   → InvokeCommand.invoke
invoke（选择 runner）   → auv_cli_invoke::runner::invoke → selected services
```

dry-run 有额外例外，以上概括正常执行路径。[invoke dispatch][a-invoke]、[MCP adapter][a-mcp]。

具体例子：直接 `screen.captureRegion` 在非 macOS 返回平台限制；selected Runner 分支调用所选平台 CaptureService。[direct][a-screen]、[selected][a-runner-screen]。不能笼统说“invoke 全部 macOS-only”，也不能因底层有 Windows/Linux capture 就称默认 MCP 同样可用。

这是已确认的能力暴露差异。当前[ownership exception][a-ownership]允许保留 invoke crate，并未证明每个行为分叉都被明确接受。应该选定真实命令接通共享执行路径；没有理由删除 gRPC、强迫所有本地调用经过 daemon，或重建 auv-runtime。

### 7. CI 编译依赖不等于运行依赖 crate 自己的单元测试

`cargo metadata --no-deps --format-version 1` 显示 41 个 workspace members，default member 仅 auv-cli；CI 三平台执行裸 `cargo test`。[Cargo.toml][a-cargo]、[workflow][a-ci]。按 [Cargo package selection](https://doc.rust-lang.org/cargo/commands/cargo-test.html#package-selection)，它不会自动选中 driver、tracing、invoke 各自的单元测试集。

限定：依赖仍编译；[CLI integration suite][a-integrated]会间接覆盖 invoke/recording/MCP，不能说其余 40 个 crate 完全没测。default-members 作为本地 CLI 使用便利本身也合理；缺口在 CI 未显式选择应测的 owning packages。

CUA 的优势是[外部接收端 oracle 和证据约束][c-tests]：记录 SHA、fixture、focus/cursor/occlusion、允许的 refusal、截图/视频。其“122/122”之类矩阵可能同时计入按预期拒绝的案例，不等于 122 个功能全部可执行，更不等于本次重跑全部通过。AUV 的下一步应建立可以证明“真的投到谁、几次、何种状态”的少量关键行为测试。

## 上游最近更新，AUV 该追哪些

以下按技术主题合并，避免把每个 commit 当一项缺失能力。优先级是研究建议，不构成实施授权。

| 上游时间 | 变化与提交 | 对 AUV 的具体意义 | 判断 |
| --- | --- | --- | --- |
| CUA 09-09 | [467c103](https://github.com/trycua/cua/commit/467c103be28384502cdd77b9edd5ea46da0b8ded)：修复特定 mouse route 双投递，独立接收端核验 | AUV 还保留双 post；同时查明 count clamp | 最高优先复现 |
| Peekaboo 08-18 | [86b7d102](https://github.com/openclaw/Peekaboo/commit/86b7d10298d4f903b9122e252ec4a4c88b0b3de3)：partial dispatch 后禁止普通重放 | AUV TypeText 内部 progress 与全文 paste fallback 的边界 | 高优先故障注入 |
| Peekaboo 08-20、08-26 | [a146c035](https://github.com/openclaw/Peekaboo/commit/a146c035)、[cc4c714c](https://github.com/openclaw/Peekaboo/commit/cc4c714c)：generation-bound observation/mutation | process/window/snapshot 失效与动态目标复核 | 高优先正确性边界 |
| Peekaboo 08-16、08-22 | [aef709fe](https://github.com/openclaw/Peekaboo/commit/aef709fea3d18f7ca989c3ab6c1a60428f2f3e51)、[7a1d13aa](https://github.com/openclaw/Peekaboo/commit/7a1d13aab2bfa05a99c0896632aeb6fa08023c1d)：held input、原子组合操作 | AUV drag/hold 需要 owner、取消、保证释放后再开放 | 有真实消费者后做 |
| Peekaboo 08-11、08-27 | [f2773c36](https://github.com/openclaw/Peekaboo/commit/f2773c3628feffadc559eeb3667dbb04f0ff98d4)、[715caa24](https://github.com/openclaw/Peekaboo/commit/715caa24bbe1d9accaeff283d3e8b1a96a7338b4)：SCK ownership，同时保留 classic capture 并发 | timeout 不能误当 native 工作结束；按 backend 协调 | 生命周期补强 |
| Peekaboo 09-05；CUA 09-02 | [620563ac](https://github.com/openclaw/Peekaboo/commit/620563ac2f98a391405af681bc6db085c8b2f2d3)、[808c014](https://github.com/trycua/cua/commit/808c0142dc7c8c84cde3a0d1fc5118194898a7a3)：捕获几何/metadata 与 capture-only | AUV fresh pixels + old frame 风险；不必照抄新增工具 | 高优先契约验证 |
| Peekaboo 08-26 | [1d2b6614](https://github.com/openclaw/Peekaboo/commit/1d2b6614bfeeef9c3a38d3b80448db664f06a144)：有限后台文字 readback 确认 | AUV 保留投递/语义分离，在具体 app result 接 verification | 逐个语义操作接通 |
| CUA 08-30 | [99f27ee](https://github.com/trycua/cua/commit/99f27eeb96481a155fe10f6dee6a131cc0de8b9e)：跨平台行为证据强化 | 补 CI owning suites 与独立接收端；拒绝也必须可核验 | 基础优先 |
| CUA 08-27、09-07 | [9596fb3](https://github.com/trycua/cua/commit/9596fb334f3eeec541979ccf5f0ef9ef360da0c6)、[c5a15f3](https://github.com/trycua/cua/commit/c5a15f3df3b29ffbe774de9f33d632fe75afec75)：KWin identity、受条件限制的 Hyprland input | AUV portal/GNOME 路线更窄；compositor 身份桥接需要单独维护与测试 | 平台扩展，非错误路线 |
| CUA 09-04 | [aabb208](https://github.com/trycua/cua/commit/aabb2082c170289256f0c8d9db4cce094c778578)：SDK shutdown 清理 | 对齐 frontend-owned lifecycle 与 native termination | 可借鉴，不照搬 runtime crate |
| CUA 08-17、08-25 | [3f791b2c](https://github.com/trycua/cua/commit/3f791b2cfec23d690cd34e6d275b6cbe1a8acc05)、[85d77792](https://github.com/trycua/cua/commit/85d77792e2f400a88f4b77c1218e388945c4b01c)：浏览器 profile 接入与调试清理 | native window ↔ CDP page 的精确绑定可补 browser/app operations | 相邻能力，非 OS 必须项 |
| KWWK main 08-19、08-22 | [9bfed81](https://github.com/EYHN/kwwk/commit/9bfed818295a0203b79d3ae5a80f0ec424d82d21)、[562f1e0](https://github.com/EYHN/kwwk/commit/562f1e0f46567f41e76ba36c6279ead083c1758e)：caller wait 与任务生命周期分开；截断预览保留完整输出 | 长任务和 artifact producer/consumer 的参考；AUV 已有持久 artifact | host 层借鉴，不是 native 落后 |
| KWWK main 09-08 | [052cd0c](https://github.com/EYHN/kwwk/commit/052cd0cc5f30a23784451f280b0e1475f0c2d726)：beforeRunEnd 与取消复核 | embedding host 生命周期设计 | 不需要为追平添加 agent loop |
| KWWK native core | 最近 30 天 0 commits；AUV 引用版本之后 0 runtime 实现变更 | 没有待追的“近期 native 更新” | 不制造缺口 |

前置实现（**不属于最近 30 天**）：CUA [07-31 typed effects](https://github.com/trycua/cua/commit/8e0a92e3dbf20134be9922f9e0dc847addcc92fa)、[08-02 snapshot refs](https://github.com/trycua/cua/commit/d8ae6df643df5049505a327b88abc2644a25b209)、[08-05 exact target](https://github.com/trycua/cua/commit/1b2cb5a706c3e5d636b683ab15336dbf35e579e0)、[08-05 capture validation](https://github.com/trycua/cua/commit/bc90373362cb7c521b1ff03f94457d5de618095c)；Peekaboo [08-09 single transport](https://github.com/openclaw/Peekaboo/commit/f2edb8be4fa8fc6d1cc66f6ce1fbbcda506b5c53)。

## KWWK 的比较对象与技术来源需要纠正

KWWK main 是 coding-agent。computer-use 的 [87e87e7](https://github.com/EYHN/kwwk/commit/87e87e7e627bc07385b6e14ba6b10b7c80c134ba) 在 `origin/eyhn/feat/background-computer-use`，不是 main 的 ancestor，不能说从 main 删除了。

独立的 [kwwk-computer-use-core](https://github.com/EYHN/kwwk-computer-use-core) 才是桌面核心。AUV 已在 native source 引用其 5 月 22 日 `eddd9e5`。[来源注释][a-source]。从该版本到最新，只有 [2b8da82](https://github.com/EYHN/kwwk-computer-use-core/commit/2b8da82e1232e15191b1d3d838378e0806cdf22a) MIT attribution 和随后 merge，没有运行时代码更新。

因此，它优秀的地方是已有的 client/session 组合、观察后动作、drag 与前台恢复边界；不是过去一个月出现了 AUV 没跟上的新底层机制。该 core 部分机制也来自 CUA，[attribution][k-license]明确记录了来源。AUV、KWWK core、旧 CUA 不是三个互不相关的可靠性实验。

KWWK 主仓库的 skills index、LLM image provider、agent loop 改进不构成 AUV OS API 缺口，也不应成为恢复 SkillBundle 的理由。其 8 月 22 日输出相关后续提交还删除了 task_read search 和 artifact GC，不能把中间提交出现过的能力当成当前能力。

## 还有哪些组件值得看

这些项目按“可学习的模块”选取，不按 stars、宣传成功率或工具数量排名。补充项目仅检查下列源文件/官方文档，深度小于三份主要审查。

| 项目 / 固定版本 | 与 AUV 最相关的组件 | 优秀之处 | 适用边界 |
| --- | --- | --- | --- |
| [oh-my-pi][omp-doc] / `a33cc268`，09-08 | Rust pi-natives desktop + JS computer worker | [frame-bound 坐标][omp-frame]拒绝无截图/越界/窗口尺寸变化；[AX registry][omp-ax]有 generation；[worker supervisor][omp-worker]负责重启和状态失效 | 值得看可脚本调用的 native API；当前文档明确预编译 Wayland 不启用 PipeWire capture，不能把四后端声明当完全等价 |
| [terminator][terminator-readme] / `73a381c0`，06-02 | Rust Windows UIA Locator/selector | [Locator][terminator]将作用域、查找、Exists/Visible/Enabled/Focused 等等待条件组合起来 | 当前 README 是 Windows-only；不要沿用旧文件的 macOS 宣称。默认 locator timeout 为 0，不能说所有调用自动等待 |
| [computer-use-mcp][zavora] / `8a140ecb`，09-08 | Rust N-API + TS tool registry / doctor | [单 registry][zavora]约束声明、schema 与执行器；[doctor][zavora-doctor]提供环境能力检查，适合借鉴入口一致性 | 64 工具不等于 64 项跨平台已验证；其缓存目标也不等于 process-generation identity |
| [Microsoft UFO][ufo] / `364eb796`，09-02 | Automator 的 receiver/command 分发 | 同一工作流可结合 GUI 与应用原生 API，支持 AUV app-owned typed operation 的方向 | 主要参考 Windows app automation；完整 agent/多设备系统不应成为 AUV 基础 driver 的范围 |
| [KWWKComputerUseCore][k-client] / `5201e300`，08-05 | 原生 Swift client/session/AX action | 最直接的独立 macOS 组件参考，AUV 已有代码来源关系 | 非近期追更目标；settled snapshot 和打分匹配仍需更强的效果/歧义边界 |
| [libei](https://libinput.pages.freedesktop.org/libei/) | Wayland input 的底层组件 | 研究 portal/compositor/native input 权限与生命周期的标准接口 | 基础设施，不是完整 automation framework；不能由此推导任意后台窗口输入 |

CUA 的 [native-window ↔ CDP binding][c-browser] 也值得作为独立模块研究：unique bounds/cardinality 的证明、歧义拒绝、仅只读的标题启发式，比“连接 Chrome 调试端口”本身更有价值。

## 哪些路线要保留，哪些要纠正

| 判断 | 具体内容 | 证据所支持的结论 |
| --- | --- | --- |
| 保留 | typed InputActionResult、投递/验证分开、disturbance 与 run artifacts | 问题抽象合理；可逐步丰富 effect/refusal/unknown，不能用动画或已发送冒充成功 |
| 保留 | Rust + Swift 原生调用、capability-oriented driver、frontend-owned run lifecycle | 没有配对性能/可靠性基准支持换语言、删 gRPC 或合成 runtime crate |
| 保留 | Wayland portal、声明不支持未知窗口来源、Linux 暂不做 X11 | 是明确范围与平台限制；[当前捕获边界][a-linux]诚实，不是路线失误 |
| 保留 | 暂不开放无释放保证的 held input，退休 candidate-action/SkillBundle | 延后契约合理；需要真实消费者与批准的 slice 才扩展 |
| 纠正 | 默认使用历史 Chromium compatibility recipe，却缺少与上游修复对应的接收端回归 | 这是具体实现选择需要复核；已发现 count clamp 不一致 |
| 补强 | target/process/window/snapshot identity、partial retry、capture metadata | 已有 primitives 需要更严格的不变量与可执行证据 |
| 接通 | 同一 typed operation 在 CLI/MCP/selected Runner 的一致可用性与结果 | 是当前 core convergence 的工作，优先级高于再加一套外围系统 |
| 补证据 | CI package selection，少量关键 native fixture，失败/拒绝也可核验 | 先建立可靠基线，才有条件声称支持或比较领先 |

AUV 在“可记录、可检查、可复用的应用操作”上有清晰产品空间；这与“通用 OS primitives 最完整”是两个可分别验证的判断。现有证据支持继续深耕这个空间，**不支持宣称已经全面领先**。也没有依据把 gap 数量折算为开发周数或落后百分比。

## 候选下一步与验收方式

仅记录候选，未实施、未扩展路线图。优先采用 test-only reproduction，确认后再做窄修复。

| 顺序 | 候选 slice | 最小有效验收 |
| --- | --- | --- |
| 1 | CI 显式选择 owning test packages | 按平台列出并执行预期 driver/common/invoke/tracing tests；保留 CLI integration，避免盲开所有 features |
| 2 | macOS click count 与单 transport 路由 | AppKit/Electron 独立接收端记录 down/up/count/target；1、2、3 次请求和实际消费一致；验证需要 primer 的应用不退化 |
| 3 | TypeText 部分投递后的 retry classification | 在第 N 个字符失败时，验证不会重放整个已部分投递请求；区分未投递、部分、未知 |
| 4 | 一个现有 observe→action 消费者的目标身份 | 同 PID 双窗口、窗口关闭重建、同 role 节点替换；无法证明目标时明确拒绝，记录原始证据 |
| 5 | window capture 的几何一致性 | resolve→capture 间移动/resize、scale 改变；返回同次观察的 metadata 或有限重试/拒绝 |
| 6 | 一个已存在 driver capability 的入口接通 | 例如 captureRegion 或 general scroll；对 CLI/MCP/selected Runner 核验同 typed contract 与 recording |
| 7 | 再补 drag/hold、通用语义动作与 predicate | 先确定真实消费者、取消/释放/验证边界，再逐平台开放 |

现有 AUV 无 GUI event recorder 注入点主要覆盖 key combinations，不能验证 Swift pointer posts 或逐字符 TypeText。此次没有添加只重复 `min/max` 表达式的伪回归测试，也没有把上游 fixture 通过当作 AUV 通过。

## 研究可复现性

检查了各仓库 revision、近期 git log、关键 git show/blame、对应源文件与测试；KWWK feature branch 使用 branch containment/merge-base 验证。AUV 使用 cargo metadata 验证 workspace/default-members，并与 CI shell 命令和 Cargo 官方规则交叉核对。补充项目固定 GitHub revision 后读取官方 source/README；没有安装或运行外部代码。

本地三个目标路径为 `~/Git/github.com/openclaw/peekaboo`、`~/Git/github.com/trycua/cua`、`~/Git/github.com/EYHN/kwwk`。额外 core 与 CUA 更新在临时研究目录，不改变用户已有分支。文档校验仅涉及 Markdown 路径、格式与 diff，不代表运行时验证。

[a-pointer]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L251
[a-count]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L356
[a-runner-click]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/runner.rs#L535
[a-keyboard]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Keyboard.swift#L104
[a-keyboard-session]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L563
[a-fallback]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L629
[a-keyboard-contract]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/docs/ai/references/invoke-cli/2026-09-08-targeted-keyboard-contract.md
[a-window-check]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Window.swift#L574
[a-ax]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/AxTree.swift#L138
[a-capture]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Capture.swift#L65
[a-capture-result]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/src/session.rs#L1822
[a-input]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/input.rs#L509
[a-hold]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/input.rs#L250
[a-invoke]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/invoke.rs#L76
[a-mcp]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/mcp.rs#L253
[a-cancel]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/commands/mcp.rs#L318
[a-screen]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/commands/screen.rs#L45
[a-runner-screen]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli-invoke/src/runner.rs#L159
[a-ownership]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/docs/ai/references/invoke-cli/2026-08-04-core-cli-command-ownership-design.md#L220
[a-cargo]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/Cargo.toml#L45
[a-ci]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/.github/workflows/check.yml#L43
[a-integrated]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/tests/integrated.rs
[a-linux]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-linux/src/window.rs#L34
[a-motion]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-cli/src/runner/local_driver.rs#L824
[a-source]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Pointer.swift#L608
[a-geometry]: https://github.com/moeru-ai/auv/blob/bae42bf9905614b19347566d5d41b3a9998e8a35/crates/auv-driver-common/src/geometry.rs#L24
[p-pointer]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/WindowRoutedPointerDriver.swift#L559
[p-receipt]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Strategy/DesktopOperationSnapshotReceiptValidator.swift
[p-actions]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/UIAutomationService+ElementActions.swift#L30
[p-literal]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactLiteralTypingEffectConfirmation.swift#L39
[p-outcome]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooFoundation/Sources/PeekabooFoundation/DesktopActionOutcome.swift#L8
[p-held]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/UI/ExactWindowHeldPointerLifecycle.swift
[p-capture]: https://github.com/openclaw/Peekaboo/blob/6cd38c0d319ab8db52b05d22409072efc4a20021/Core/PeekabooAutomationKit/Sources/PeekabooAutomationKit/Services/Capture/LegacyWindowCaptureGeometry.swift#L37
[c-pointer]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/input/mouse.rs#L259
[c-background]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/background_input.rs#L181
[c-lease]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/background_mutation.rs
[c-token]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/element_token.rs
[c-capture]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/platform-macos/src/capture.rs#L556
[c-output]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-contract/src/outputs.rs#L408
[c-verify]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-contract/src/verification.rs
[c-tests]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-testkit/src/e2e.rs#L525
[c-browser]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-core/src/browser/binding.rs
[c-runtime]: https://github.com/trycua/cua/blob/467c103be28384502cdd77b9edd5ea46da0b8ded/libs/cua-driver/rust/crates/cua-driver-sdk/src/runtime.rs#L223
[k-client]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseClient.swift#L89
[k-core]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseCore.swift#L197
[k-session]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseSession.swift
[k-drag]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/Sources/KWWKComputerUseCore/ComputerUseActions.swift#L389
[k-license]: https://github.com/EYHN/kwwk-computer-use-core/blob/5201e300ceb58f2aaf501b20477aff5a912efb9a/LICENSES/cua-driver-MIT.txt
[omp-frame]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/crates/pi-natives/src/desktop/frame.rs
[omp-ax]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/crates/pi-natives/src/desktop/ax.rs
[omp-worker]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/packages/coding-agent/src/tools/computer/supervisor.ts
[omp-doc]: https://github.com/can1357/oh-my-pi/blob/a33cc26824e3c91edd9fa42d681f10dceb4ac2f0/docs/computer-use.md
[terminator]: https://github.com/mediar-ai/terminator/blob/73a381c0c1c33eda55f2c0ecb1d918bf5ec7561a/crates/terminator/src/locator.rs
[terminator-readme]: https://github.com/mediar-ai/terminator/blob/73a381c0c1c33eda55f2c0ecb1d918bf5ec7561a/README.md
[zavora]: https://github.com/zavora-ai/computer-use-mcp/blob/8a140ecbf6437e1e2f5c033abdd5b8c1789d2878/src/registry/registry.ts
[zavora-doctor]: https://github.com/zavora-ai/computer-use-mcp/blob/8a140ecbf6437e1e2f5c033abdd5b8c1789d2878/src/session/doctor.ts
[ufo]: https://github.com/microsoft/UFO/blob/364eb7969d392e857299ceaf14bd6057e5b00078/ufo/automator/puppeteer.py

专题补审：[后台输入、AX tree 与音视频采集](2026-09-09-background-ax-and-media-gap-review.md)进一步拆解既有风险，另列 MEDIA-1～3 新能力候选；多数 BG/AX 分组与原候选重叠，不能累加为 bug 数。
