# Linux Wayland 持续帧流与 daemon/Runner 生命周期调整设计

Date: 2026-08-30

Status: Proposed design based on source inspection and one live RC cluster investigation. This document guides an owner-selected implementation slice; it is not a Linux support claim by itself.

本文不改变 [`SUPPORT_MATRIX.md`](../../../SUPPORT_MATRIX.md)：Linux screenshot/capture 仍为 `not claimed`，直到公开 setup、自动 regression gate、current live closure 和 maintenance boundary 同时具备。

## TL;DR

这次调查从 RC 的 headless Sway workspace 开始。`grim` 可以稳定截图，AUV 通过 ScreenCast portal 和 PipeWire 截图时却先后遇到了 SHM-only 初始化失败、静态画面没有首帧、每次命令重新建立 session 等问题。前两个问题位于 `xdg-desktop-portal-wlr`（下文简称 xdpw）和 compositor compatibility 层；第三个问题来自 AUV 当前的资源生命周期。三者需要分别处理，不能用一个新的截图 fallback 混在一起。

AUV 在 Linux Wayland 上应继续使用 `xdg-desktop-portal ScreenCast + PipeWire` 作为主要采集方式。它适合动画调试、高频 OCR、动作后观察和未来的录制型能力。`display.capture` 应读取已经运行的帧流，而不是临时创建一次 ScreenCast。`grim` 适合作为 compositor 诊断工具和集成测试 oracle，不适合作为 AUV production runtime 的 primary 或 fallback backend。

长生命周期资源应由 platform Driver Runner 持有，daemon 负责 Runner 的创建、复用、路由、健康、drain 和关闭。RC workspace 应在 Electron 启动前 prewarm 一个 `unless-shutdown` Driver Runner；Portal session、PipeWire stream、latest-frame cache 和可选的 bounded frame ring 都随这个 Runner 存活。当前 xdpw 0.8.4 patch 仍需保留，直到对应改动进入上游或 runner 升级到经实测可用的 compositor/xdpw 组合。

## 1. 背景与本文回答的问题

AUV 的 `display.capture`、`screen.findText`、`window.capture` 和动作后验证都需要桌面像素。对于静态检查，单张截图看起来已经足够；对于 Electron 启动动画、过渡、短暂错误提示、hover 状态和动作后的局部变化，调用开始前已经丢失的帧无法通过更快的单次截图恢复。

本文回答以下问题：

1. 为什么 `grim` 在当前 workspace 可以截图，而 AUV 的 PipeWire 路径失败。
2. AUV 是否应把 `grim` 或 direct screencopy 作为新的 fallback。
3. ScreenCast/PipeWire 的资源应由哪个进程角色持有。
4. `display.capture` 如何从「建立一次截图」调整为「读取持续帧流」。
5. 动画调试需要怎样的 latest frame、frame cursor 和 bounded ring。
6. 当前 xdpw patch 为什么仍需保留，以及何时可以删除。
7. 这项调整会影响哪些 crate、协议、trace 和测试。

## 2. 范围与非目标

### 2.1 范围

- Linux Wayland display/region capture。
- xdg-desktop-portal ScreenCast、xdpw 和 PipeWire 的生命周期。
- `auv invoke`、MCP、daemon、Run affinity 和 platform Driver Runner 的关系。
- latest-frame capture、动作后帧同步和短时间动画 trace。
- 当前 RC headless Sway/pixman 环境所需的 xdpw compatibility patch。

### 2.2 非目标

- 不把 `Session` 重新引入为 public control-plane resource。
- 不创建新的 catch-all `auv-runtime` crate。
- 不把 `grim`、`slurp` 或 Screenshot portal 变成自动 fallback。
- 不在本设计中承诺 GNOME、KDE、Hyprland 或任意 minimized window 的统一行为。
- 不在第一阶段定义通用视频编辑、编码或直播协议。
- 不因为 Linux 需求改变 macOS ScreenCaptureKit、Windows WGC/DXGI 的已接受方向。

## 3. 现状证据

### 3.1 调查环境

2026-08-30 使用以下环境完成 live investigation：

| 项目 | 值 |
| --- | --- |
| Kubernetes config | `KUBECONFIG="$HOME/.kube/config.d/ihome.conf"` |
| Namespace | `rc-dev` |
| Workspace | `lobehub-dev` |
| GUI | LobeHub Electron，`--ozone-platform=wayland` |
| Compositor | Sway 1.9 / wlroots 0.17.1 |
| Renderer | headless pixman |
| Render node | 无 `/dev/dri` |
| AUV | `0.0.13` |
| AUV source revision | `3faa1d4b` |
| xdg-desktop-portal | `1.18.4` |
| Distribution xdpw | `0.7.1` |
| Tested source build | xdpw `0.8.4` |
| grim | `1.4.0` |

最终通过 live test 的 runner image 为：

```text
ghcr.io/nekomeowww/rc/runner-wayland:xdpw-0.8.4-shm-first-frame-test
sha256:3dba123cf468d5cca9d9e77a551dcb6a347b5887ec71135938b4cdf3d363b63e
```

该镜像中的 patched xdpw binary SHA-256 为：

```text
b50bc49e4f915ac2f796b851c377d0d9cece7183c60e8700cd126dc0e155c895
```

### 3.2 两条采集路径

`grim` 的路径较短：

```text
grim
  -> zwlr_screencopy_manager_v1
  -> Sway/wlroots
  -> wl_shm buffer
  -> PNG
```

grim 绑定 `zwlr_screencopy_manager_v1`，请求 output frame，在 compositor 提供的 SHM 格式上调用普通 `copy`。其运行不需要 desktop portal 或 PipeWire。实现可见 [grim `main.c`](https://github.com/emersion/grim/blob/master/main.c)。

AUV 0.0.13 的 primary 路径为：

```text
AUV
  -> org.freedesktop.portal.ScreenCast
  -> xdg-desktop-portal
  -> xdg-desktop-portal-wlr
  -> zwlr_screencopy / ext-image-copy-capture
  -> PipeWire
  -> AUV PipeWire receiver
  -> RgbaImage
```

AUV 当前在 ScreenCast 失败后调用 Screenshot portal。该逻辑位于 [`crates/auv-driver-linux/src/capture.rs`](../../../../crates/auv-driver-linux/src/capture.rs)。Screenshot portal 的语义是交互式截图，不适合自动化；xdpw 的 Screenshot 实现内部还会执行 `grim -g "$(slurp)"`，见 [xdpw screenshot implementation](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.4/src/screenshot/screenshot.c)。

### 3.3 live investigation 得到的分层结论

| 观察 | 证据 | 归属层 |
| --- | --- | --- |
| `grim` 在 HEADLESS-1 上成功 | compositor 暴露 wlr screencopy 和 wl_shm | compositor/direct protocol 可用 |
| xdpw 0.7.1 报 `unable to receive a valid format from wlr_screencopy` | pixman 只提供 SHM，没有可用 DMA-BUF | xdpw format compatibility |
| stock xdpw 0.8.4 无法启动 legacy screencopy | 0.8.4 要求 `screencopy_manager && linux_dmabuf` | xdpw initialization policy |
| AUV 等待时修改 Sway 背景，capture 立即完成 | `copy_with_damage` 在静态画面上没有交付首帧 | xdpw initial-frame semantics |
| first frame 使用普通 `copy` 后连续静态 capture 成功 | 三次连续静态 AUV capture 通过 | xdpw patch behavior |
| LobeHub Electron Wayland 窗口可被 AUV 捕获 | patched runner 中完成真实应用截图 | end-to-end live evidence |

xdpw 0.8.4 的 legacy path 初始化条件可见 [wlr_screencast.c](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.4/src/screencast/wlr_screencast.c#L571-L594)，`copy_with_damage` 调用可见 [wlr_screencopy.c](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.4/src/screencast/wlr_screencopy.c#L127-L159)。

这些证据支持两个不同判断：

1. 当前 compositor 已经能提供截图数据，因此 `grim` 成功不代表 AUV 应依赖 grim；它说明 portal/PipeWire 中间层存在额外 compatibility 条件。
2. AUV 即使修好中间层，短生命周期命令仍会错过调用前的动画；该问题需要调整 AUV 的 Runner 和 frame ownership。

## 4. AUV 当前实现的生命周期问题

### 4.1 每条 invoke command 创建新的 local Driver session

Linux `display.capture` handler 当前直接调用：

```rust
let session = auv_driver::open_local()?;
```

实现位于 [`crates/auv-cli-invoke/src/commands/display.rs`](../../../../crates/auv-cli-invoke/src/commands/display.rs)。`screen`、`window` 和多个 input command 也采用相同方式。

这会带来以下结果：

1. `auv invoke` 是短生命周期进程，命令退出后 ScreenCast session 和 PipeWire stream 一起销毁。
2. MCP frontend 虽然常驻，但它调用同一组 `InvokeCommand`；command 内部仍重新 `open_local()`，因此常驻 frontend 没有形成常驻 Driver session。
3. Portal restore token 可以减少重新选择，但不能消除 D-Bus session、PipeWire remote、stream negotiation 和首帧等待。
4. Electron 在 command 启动前出现的动画已经不可恢复。

### 4.2 PipeWire receiver 保持连接，但没有持续刷新 latest frame

当前 `PipeWireFrameReceiver` 已经有一个 worker thread、一个 `latest` frame 和一个 pending capture request。该实现比每次完全重建 PipeWire receiver 更接近所需方向。

目前的 process callback 在没有 pending request 且已经有 latest frame 时，会 dequeue 并立即释放新 buffer，不进行像素转换，也不更新 latest frame。实现位于 [`crates/auv-driver-linux/src/native/portal/screencast.rs`](../../../../crates/auv-driver-linux/src/native/portal/screencast.rs)。

这项优化降低了 idle CPU 开销，但也意味着：

- 调用期间没有 waiter 的动画不会进入 latest frame；
- 下一次 capture 只能等待下一张 damage frame；
- 动画已经结束且画面恢复静态时，期间的状态没有留下记录；
- 同一 session 内的 cached-frame fix 只能处理静态重复 capture，不能提供 retrospective animation debug。

### 4.3 `display.capture` 同时承担 acquisition 和 projection

当前 capture module 同时处理：

- output resolution；
- ScreenCast session lazy initialization；
- PipeWire frame wait；
- Screenshot portal fallback；
- region crop；
- backend/fallback metadata；
- `RgbaImage` materialization。

调用者得到一张结构化 `Capture`，但 capture module 没有表达帧流生命周期、source generation、sequence、delivery timestamp 或 stream health。随着 daemon 和 animation trace 接入，这些行为会继续堆在 command handler 和 portal code 中。

## 5. 设计目标

### 5.1 行为目标

1. RC workspace 能在 Electron 启动前建立 capture stream。
2. `display.capture` 的 hot path 不重新创建 Portal/PipeWire session。
3. 静态画面可以返回已缓存且仍有效的 latest frame。
4. 动作执行后可以等待同一 source generation 中 sequence 更大的帧。
5. 动画调试可以取得一个有界时间范围内的帧序列。
6. Runner restart、source replacement 和 PipeWire reconnect 不会伪造连续 sequence。
7. Screenshot portal 不再作为无提示的 automatic fallback。
8. backend、transport、timing、repeated frame 和 stream health 可以进入 trace evidence。

### 5.2 性能目标

1. Portal authorization、PipeWire connection 和 format negotiation 在 Runner 生命周期内摊销。
2. PipeWire callback 不执行 PNG 编码或 artifact I/O。
3. latest-frame 模式使用有界、可复用的内存。
4. animation trace 只有在明确启用时才扩大 frame retention。
5. OCR/recognition 与 capture 在同一 Runner 内组合时，不要求先通过 gRPC 传输完整 RGBA frame。
6. 通过 PTS、sequence 和 dropped-frame counters 评估实际丢帧，而不是假设配置 FPS 等于有效 FPS。

## 6. 核心设计决策

### 6.1 Linux Wayland primary capture 保持 Portal ScreenCast + PipeWire

Wayland 上继续采用 portal 的理由包括：

- ScreenCast 是 GNOME、KDE 和 wlroots compositor 之间可移植的授权与选择接口；
- PipeWire 提供持续 frame transport；
- 可在支持时使用 DMA-BUF，在当前 pixman 环境中使用 memory-backed buffer；
- restore token 能让常驻 Runner 复用已批准 source；
- 动画和动作后观察需要在请求到来前就存在的 frame stream。

该方向与现有 [OBS platform capture research](2026-08-05-obs-platform-capture-backends-research.md) 一致。

### 6.2 grim 是诊断工具和 test oracle

grim 的 runtime 优势是实现简单、直接访问 wlr screencopy、静态 snapshot 行为明确。将它作为 AUV production backend 会增加：

- subprocess 生命周期和 executable discovery；
- PNG encode/decode；
- stdout/stderr 到 typed error 的映射；
- grim 版本和 CLI contract dependency；
- wlroots-only support claim；
- display、region、scale、transform metadata 的二次重建。

因此 grim 的定位为：

- 验证 compositor screencopy 是否工作；
- 在静态 fixture 上与 AUV output 做像素或区域对比；
- 故障时区分 compositor 和 portal/PipeWire 层；
- CI/live test 中的外部 oracle。

它不进入 primary/fallback selection。

### 6.3 backend selection 不是运行时错误 fallback

AUV operation interface 已经规定 backend selection 一旦解析就保持 sticky。Linux capture 也应遵循该规则：

- daemon context/Device/RunnerClass 决定调用在哪个 Runner 执行；
- Runner 配置决定使用 Portal/PipeWire capture；
- initialization failure、permission failure、frame timeout 和 reconnect failure 返回原层错误；
- 不因为 primary capture 失败而悄悄切换到 interactive Screenshot portal 或本机 grim。

如果未来确有第二个 production adapter，应在 source initialization 时按明确 policy 选择，并将选择结果写入 provenance。不能在 `capture()` 超时后改变 capture semantics。

### 6.4 xdpw patch 属于 runner compatibility 层

AUV 不应通过改变 capture semantics 掩盖 xdpw 的 SHM-only 和 initial-frame 问题。当前 patch 需要继续保留：

1. build compatibility：补充 `<unistd.h>`。
2. SHM-only legacy screencopy：没有 `linux-dmabuf` 时允许 `zwlr_screencopy + wl_shm`。
3. initial full frame：`cast->seq == 0` 使用普通 `copy`，后续使用 `copy_with_damage`。

第三项保留 damage-driven stream 的性能特点，同时保证每个新 stream 至少有一张基准帧。

## 7. 目标 ownership

### 7.1 责任分配

| Owner | 持有内容 | 不持有内容 |
| --- | --- | --- |
| `auv-daemon` | RunnerClass admission、Runner creation/reuse、routing、health、drain、shutdown、Run affinity | PipeWire node、frame buffer、capture pixel conversion |
| platform Driver Runner | `LinuxDriverSession`、Portal authorization、ScreenCast session、PipeWire stream、latest frame、bounded ring、capture/OCR/input-local coordination | public Device/Run identity、daemon listener |
| `auv::Client` / operation interface | local/remote backend resolution、Device/Run context、typed request/result | daemon lifecycle、concrete PipeWire implementation |
| invoke/MCP frontend | parse、presentation、run-context lifecycle、artifact projection | `open_local()` policy、capture session ownership |
| `auv-tracing` | events、artifact persistence、run records | capture execution、frame scheduling |
| rc runner image | compositor、portal、PipeWire services、xdpw build/patch/config | AUV command semantics |

`Session` 只保留为 Driver/Runner 内部实现词。它不成为 public AUV resource，也不替代 Run affinity。

### 7.2 推荐进程与生命周期

```text
RC workspace start
  -> start Sway / portal / PipeWire
  -> start `auv serve`
  -> daemon prewarms platform Driver Runner
  -> Driver Runner opens Portal ScreenCast session
  -> Driver Runner connects PipeWire stream
  -> first full frame becomes available
  -> capture readiness becomes observable
  -> start Electron
  -> invoke/MCP/library calls route to the same Runner
  -> workspace shutdown drains and stops the Runner
```

对于 RC workspace，platform Driver Runner 应使用 `unless-shutdown` lifecycle。`ephemeral` 会在请求结束后丢失 stream；短 idle timeout 的 `unless-idle` 会让动画 pre-roll 和 portal warm state 在空闲期消失。

一般调用不应通过 live Runner ID 选择执行位置。显式 `CreateRunner` 只用于 prewarm/operations；业务调用仍按 Device、可选 Run 和 RunnerClass registration 路由。

### 7.3 Run affinity

带 Run association 的调用继续使用现有 affinity：

```text
(Run, Device, RunnerClass registration) -> live Runner
```

这使同一交互中的 capture、OCR、input 和 verification 更可能落在同一个 Driver Runner 中。多个 Run 可以共享一个 `unless-shutdown` Runner；frame stream 是 Runner-owned resource，Run 只提供调用与 trace association，不独占 compositor stream。

## 8. `LinuxFrameStream` module

### 8.1 seam 位置

新 seam 应放在 Linux Driver 内部的 live source 与 capture operations 之间。第一阶段只有 Portal/PipeWire 一个 production implementation，因此不需要先引入公开 backend trait。一个具体的深 module 可以先承接行为：

```rust
struct LinuxFrameStream {
  // Portal session, PipeWire worker, source identity, frame cache,
  // reconnect policy and health are private implementation details.
}
```

建议 interface 保持在少量高语义方法上：

```rust
impl LinuxFrameStream {
  fn latest(&self, request: LatestFrameRequest) -> DriverResult<FrameLease>;
  fn cursor(&self) -> FrameCursor;
  fn wait_after(&self, cursor: FrameCursor, timeout: Duration)
    -> DriverResult<FrameLease>;
  fn frames_between(&self, range: FrameRange)
    -> DriverResult<FrameSequence>;
}
```

这些名字是 design sketch，不在本文中建立新的 public term。接受实现 slice 时，应再决定哪些类型进入 `auv-driver-common`，哪些只保留在 Linux implementation 内部。

### 8.2 module 应隐藏的行为

`LinuxFrameStream` 内部负责：

- Portal session creation、restore token 和 source selection；
- PipeWire remote/node connection；
- SPA format negotiation；
- frame dequeue、copy/import 和 buffer return；
- source identity 与 source generation；
- sequence、PTS、receive timestamp；
- latest frame replacement；
- optional bounded ring；
- static/damage-driven source semantics；
- stalled、closed、permission-revoked 和 reconnect state；
- reconnect 后 generation rollover；
- capture request freshness validation。

调用者不需要知道 node ID、wl_shm、DMA-BUF、damage 或 PipeWire mainloop。

### 8.3 session state 调整

当前 [`LinuxDriverSessionState`](../../../../crates/auv-driver-linux/src/driver.rs) 直接保存 `Option<ScreenCastSession>`。建议调整为 capture-owned state，例如：

```rust
pub(crate) struct LinuxDriverSessionState {
  pub(crate) clipboard_session: Option<ClipboardSession>,
  pub(crate) input_session: Option<InputSession>,
  pub(crate) capture: LinuxCaptureState,
  pub(crate) restore_tokens: Option<RestoreTokenStore>,
}
```

`LinuxCaptureState` 可以 lazy start，以保持普通 local library caller 的兼容行为；daemon prewarm path 则明确调用 readiness/start。在同一个 Runner 内，所有 display/screen/window/recognition operations 共享该 state。

## 9. frame identity、freshness 与静态画面

### 9.1 建议 metadata

当前 domain `Capture` 和 protobuf `CapturedFrame` 主要包含 image、bounds、scale、backend 和 `fallback_reason`。持续帧流需要补充至少以下证据：

| 字段 | 目的 |
| --- | --- |
| `source_id` | 标识 portal-selected source/stream |
| `source_generation` | reconnect、source replacement 或 stream rebuild 时递增 |
| `sequence` | 同一 generation 内严格递增的有效帧编号 |
| `source_timestamp` | PipeWire/compositor 提供的时间，若可用 |
| `received_at` | AUV 收到 frame 的 monotonic time |
| `requested_at` | operation 请求时间 |
| `repeated` | 本次返回的是 cached frame，未观察到新 damage |
| `stream_health` | advancing、idle/stable、stalled、closed、reconnecting、unknown |
| `transport` | memory/SHM 或 DMA-BUF |
| `pixel_format` | 保留原始 frame representation 的解释信息 |
| `dropped_frames` | backend 可观测时记录累计或区间值 |

这些字段应先进入 domain contract，再投影到 protobuf 和 trace。protobuf 仍是 wire projection，不能反向成为 domain owner。

### 9.2 静态画面的 freshness

damage-driven stream 在画面不变时可能不发送新 buffer。旧 `source_timestamp` 不一定表示像素已经失效。AUV 需要区分：

- source 仍健康且没有新 damage：cached frame 可以作为当前画面的重复值；
- source stalled/closed/reconnected：cached frame 只能作为历史证据，不能声称 fresh；
- 调用要求「动作后视觉变化」：必须等待 cursor 之后的新 sequence，cached frame 不满足条件；
- 调用只要求「读取当前显示内容」：健康 stream 的 latest frame 可以立即返回并标记 `repeated=true`。

因此，单一 `max_age` 无法表达完整 freshness policy。请求至少需要区分「允许健康 source 的 repeated frame」与「必须观察到 cursor 之后的新 frame」。

### 9.3 cursor 与 generation

建议 cursor 形状：

```rust
struct FrameCursor {
  source_generation: u64,
  sequence: u64,
}
```

如果 Runner 或 PipeWire stream 重启，新的 `source_generation` 使旧 cursor 自动失效。`wait_after` 应返回 typed continuity error，而不是在新 source 上继续比较 sequence。

## 10. latest frame 与 bounded ring

### 10.1 默认 latest-frame 模式

PipeWire callback 应持续刷新 latest frame，不能只在 command 正在等待时解码。默认模式只需要持有当前 latest frame 和用于安全替换的少量 buffer slots：

```text
PipeWire process callback
  -> dequeue newest available buffer
  -> copy/import into reusable slot
  -> attach sequence/timing/health metadata
  -> atomically publish latest
  -> release previous unleased slot
  -> return PipeWire buffer promptly
```

如果 queued buffers 中已有多帧，应优先 drain 到最新可用帧，再处理最新一帧，避免 consumer 积压增加视觉延迟。这一点可以参考 [OBS PipeWire implementation](https://github.com/obsproject/obs-studio/blob/0052d024fd6a5ff1aa04c76cbdffd3085a5dfacc/plugins/linux-pipewire/pipewire.c)。

### 10.2 避免每帧 PNG/RGBA allocation

PipeWire callback 不应：

- 分配 artifact filename；
- 编码 PNG；
- 写 tracing store；
- 为没有 consumer 的每帧创建新的 `RgbaImage` allocation；
- 阻塞等待 OCR 或 gRPC response。

建议让 internal frame slot 暂时保留 negotiated pixel format 和 stride。只有需要现有 `Capture.image: RgbaImage`、OCR CPU input 或 encoded artifact 时才 materialize/convert。

memory-backed PipeWire buffer 在归还后不能被 AUV 当作长期 frame reference。当前 pixman/SHM path 仍需要把数据复制到 AUV-owned slot。DMA-BUF zero-copy 是否能与 AUV inference runtime 直接组合，需要独立 evidence，不能仅根据 transport 名称声称 zero-copy。

### 10.3 animation trace 模式

latest frame 只能回答现在的画面，不能还原已经结束的动画。animation trace 需要显式启用 bounded ring：

```text
arm trace
  -> record start cursor
  -> retain frames under duration/frame/byte limits
  -> execute or observe action
  -> record end cursor
  -> freeze selected frame range
  -> encode/persist outside PipeWire callback
```

ring 必须同时受以下限制：

- maximum duration；
- maximum frame count；
- maximum retained bytes。

以未压缩 RGBA8 估算：

| Resolution | One frame | 500 ms at 30 FPS | 500 ms at 60 FPS |
| --- | ---: | ---: | ---: |
| 1280×720 | 3.52 MiB | 52.7 MiB | 105.5 MiB |
| 1920×1080 | 7.91 MiB | 118.7 MiB | 237.3 MiB |

因此第一阶段不建议默认保留数秒全量 RGBA。对于 AUV 主动触发的操作，可以在 input 前 arm trace，通常不需要很长 pre-roll。应用启动动画则要求 Driver Runner 先于应用启动并提前 arm。

视频编码、delta compression 和 GPU-backed ring 可以在有具体 consumer 与 benchmark 后单独设计；不应先进入 PipeWire callback。

## 11. operation semantics

### 11.1 `display.capture`

目标语义：

```text
lease the freshest acceptable frame from the already running display source
```

默认行为：

- stream healthy 且有 latest frame：立即返回；
- 没有新 damage：返回 cached frame，`repeated=true`；
- stream 尚未产生 initial frame：等待 bounded readiness timeout；
- stream generation 改变：返回新 generation frame，并在 metadata 中体现；
- stream unavailable：返回原层 typed error，不调用 Screenshot portal。

### 11.2 `screen.captureRegion` 与 `window.capture`

第一阶段可以继续从同一 display frame crop，但必须保留：

- selected display identity；
- logical bounds 与 pixel scale；
- source generation/sequence；
- crop provenance；
- window identity safety checks。

未来 `ext-foreign-toplevel-image-capture-source` 或 compositor-specific independent window source 属于不同 delivered scope。它们不应在没有 metadata 变化的情况下替换 display crop semantics。

### 11.3 OCR 与 recognition

`screen.findText`、`window.findText` 等组合操作应在 Driver Runner 内取得 `FrameLease` 并完成 OCR，返回 recognition result 和同一 source capture evidence。这样避免：

```text
Runner RGBA -> gRPC -> frontend -> another process OCR
```

现有 remote design 已允许 capture + OCR 在一个 Runner 内完成；本调整应保持该方向。

### 11.4 input 后观察

输入与视觉观察需要显式 barrier：

```text
cursor = frame_stream.cursor()
input.click(...)
frame = frame_stream.wait_after(cursor, timeout)
```

`wait_after` 只证明观察到后续 frame，不证明点击的业务语义成功。semantic verification 仍由 owning operation 单独完成。

### 11.5 animation debug

animation debug 应返回 frame sequence 或 encoded artifact，并携带：

- start/end cursor；
- source generation；
- per-frame timing；
- dropped/omitted frame information；
- ring truncation reason；
- backend/transport；
- 关联的 input/operation events。

具体 CLI/RPC 名称在 implementation slice 中确定。本文不建立新的顶层 `animation` namespace。

## 12. daemon、Runner 与 frontend 调整

### 12.1 daemon 是 lifecycle owner，Runner 是 resource owner

Daemon 不应直接知道 PipeWire node 或 frame buffer。它负责：

- admission：启用哪个 platform Driver RunnerClass；
- prewarm：在应用启动前创建 Runner；
- routing：把 capability call 送到 live Runner；
- affinity：维持 Run/Device/RunnerClass 到 Runner 的关联；
- health/drain：停止接收新请求并等待 active operations；
- shutdown：终止 owned Executable Runner。

Driver Runner 负责 capture resource 和 recovery policy。该分工与 [`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md) 中现有 Daemon、RunnerClass 和 Runner 定义一致。

### 12.2 invoke/MCP 不再直接决定 `open_local()`

当前 invoke command handler 直接构造 local Driver，绕过了 operation interface 已定义的 local/remote backend resolution。建议将 capability provider 或 `auv::Client` 从 frontend authority 注入 command execution：

```text
CLI / MCP / library
  -> AuvContext resolves local or daemon-backed client
  -> typed operation interface
  -> local Driver OR daemon route
  -> selected Driver Runner
```

选择结果保持 sticky。daemon connection/capability failure 不触发 implicit local fallback。

MCP server 的进程常驻本身不能解决问题；只有当 MCP commands 复用同一个 daemon-backed operation client，并由 daemon 路由到同一个 long-lived Runner，Portal/PipeWire resource 才能复用。

### 12.3 prewarm 与 readiness

RC workspace 的推荐顺序：

1. Sway、D-Bus、PipeWire、WirePlumber 和 portals ready。
2. `auv serve` ready。
3. 创建/prewarm platform Driver Runner。
4. Runner 恢复或建立 ScreenCast authorization。
5. PipeWire stream connected。
6. initial frame received。
7. 标记 capture capability ready。
8. 启动 Electron 和 automation tasks。

普通 gRPC Health serving 只能证明 process/listener 可响应。是否已经收到 initial frame 应通过 capture capability 的 readiness/detail 暴露，不能让 orchestration 通过第一次业务 capture 猜测。

### 12.4 restart 与 continuity

Runner restart 后：

- Portal restore token 可以尝试恢复 selection；
- PipeWire node/stream identity 可能变化；
- `source_generation` 必须变化；
- previous frame cursor 和 ring range 失效；
- daemon 可以重新路由到 replacement Runner，但不能声称 frame continuity；
- active animation trace 应以 typed interruption 结束并记录 restart evidence。

## 13. xdpw patch 的保留、上游与退出条件

### 13.1 当前 patch

RC 仓库当前包含：

```text
images/runner-wayland/patches/xdg-desktop-portal-wlr-0.8.4-build.patch
images/runner-wayland/patches/xdg-desktop-portal-wlr-0.8.4-shm.patch
```

行为 patch 包含：

```c
if (cast->seq == 0) {
    zwlr_screencopy_frame_v1_copy(frame, buffer);
} else {
    zwlr_screencopy_frame_v1_copy_with_damage(frame, buffer);
}
```

这与持续帧流的契约一致：first frame 建立完整基准，之后只在 damage 时产生新帧。

### 13.2 为什么 daemon 不能替代 patch

长生命周期 Runner 可以摊销 startup 和 authorization，也能保留动画历史；它无法让一个不能初始化的 xdpw backend 启动，也无法制造 compositor 没有交付的 initial frame。

因此两项工作并行存在：

```text
xdpw patch
  -> 让 SHM-only capture stream 正确建立并交付首帧

AUV daemon/Runner adjustment
  -> 让已建立的 stream 跨命令存活并持续提供帧
```

### 13.3 删除 patch 的条件

满足以下任一路径后才删除 behavior patch：

1. 对应 SHM-only 与 initial-frame 改动进入 xdpw upstream release，并在 RC live cluster 复测通过。
2. runner 升级到支持 `ext-image-copy-capture` 的 Sway/wlroots，stock xdpw 0.8.4+ 走新路径，并通过相同静态/动画测试。
3. runner 获得经验证的 GPU/DMA-BUF path，且 stock xdpw legacy path 的 initial-frame behavior 同样通过。

删除前需要保留一次 A/B evidence，不能只依据 compilation 或 protocol advertisement。

## 14. error 与 recovery

建议区分以下错误层：

| Layer | 示例 | Recovery owner |
| --- | --- | --- |
| environment | session bus、Wayland display、PipeWire unavailable | runner/workspace |
| authorization | portal denied、restore token rejected、source selection cancelled | Driver Runner/user policy |
| backend initialization | xdpw unavailable、no compatible capture protocol/format | runner image/compositor |
| transport | PipeWire disconnected、node removed、format renegotiation failed | `LinuxFrameStream` |
| frame | initial-frame timeout、decode failure、unsupported format | `LinuxFrameStream` |
| continuity | source generation changed while waiting | owning operation |
| freshness | request requires post-cursor frame but none arrived | owning operation |
| artifact | PNG/video persistence failed after frame acquired | caller/tracing layer |

Recovery policy：

- recoverable PipeWire disconnect 可以在同一 Runner 中 rebuild source；
- rebuild 必须递增 generation；
- permission denial 不自动重试或切换 backend；
- Screenshot portal 不作为 recovery；
- artifact failure 不应销毁一个仍健康的 stream；
- repeated failure 进入 health detail 和 trace，daemon 决定是否 replace Runner。

## 15. tracing、artifact 与 inspection

### 15.1 capture event

每个返回给 operation 的 capture 应记录：

- source ID/generation/sequence；
- backend 与 transport；
- requested、received、source timestamps；
- repeated/cropped；
- delivered bounds、pixel size、scale；
- stream health；
- conversion/copy performed；
- artifact receipt（如果 persisted）。

`Capture Frame` 仍是内存结果，caller 或 instrumentation 决定是否保存 artifact。该原则已在 [`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md) 中定义。

### 15.2 stream lifecycle event

建议记录低频 lifecycle events，而不是每个 idle tick：

- stream starting；
- authorization restored/new/interactive；
- stream ready with first frame；
- format negotiated；
- source generation changed；
- stream stalled/resumed；
- reconnect started/completed/failed；
- stream stopped；
- ring armed/frozen/truncated。

### 15.3 animation artifact

animation artifact 应可回指：

- Run；
- owning operation/input event；
- source generation；
- start/end cursor；
- frame timing；
- encoding parameters；
- dropped/truncated evidence。

编码后的文件只是 artifact projection；frame sequence 和 timing contract 仍由 producing module 定义。

## 16. 代码与协议影响面

| Area | 预期调整 |
| --- | --- |
| `auv-driver-common` | capture provenance/freshness/timing domain types；保持 capture result 的 coordinate contract |
| `auv-driver-linux::driver` | `ScreenCastSession` ownership 调整为 capture-owned state；增加 explicit start/readiness/recovery |
| `auv-driver-linux::capture` | 从 portal/fallback orchestration 调整为消费 `LinuxFrameStream`；移除 automatic Screenshot fallback |
| `auv-driver-linux::native::portal::screencast` | 持续刷新 latest；sequence/PTS/generation；reusable slots；bounded ring；reconnect |
| `auv-cli-invoke` | handlers 不再直接决定 `open_local()`；从 operation authority 获取 provider/client |
| `auv-cli` MCP | 复用 daemon-backed operation client；避免每次 command 创建 local Driver |
| `auv` | 保持 canonical operation interface 的 local/remote resolution 和 sticky backend selection |
| `auv-daemon` | prewarm、Runner lifecycle/affinity/health；不引入 PipeWire-specific forwarding method |
| platform Driver Runner host | Runner 启动时创建一个可复用 Driver session；服务 display/screen/window/input/recognition RPC |
| `auv.api.driver.v1` | `CapturedFrame` 增加 provenance/freshness/timing wire projection；必要时增加 animation/frame-sequence capability |
| `auv-tracing` | capture 和 stream lifecycle events；artifact receipt 保持 caller-owned |
| rc runner image | 继续构建 patched xdpw 0.8.4；明确 patch/source SHA；配置 FPS 与 readiness |

protobuf package 继续由 Driver capability domain 拥有；daemon control package 不导入 capture-specific message，也不增加逐方法 forwarding。

## 17. 推荐 implementation slices

### Slice 1：补齐 frame provenance 与 freshness contract

分类：approved feature 前的 contract slice。

工作内容：

1. 在 `auv-driver-common` 定义 source generation、sequence、timing、repeated 和 stream health。
2. 扩展 `Capture` 与 `CapturedFrame` projection。
3. 更新 capture/recognition proto conversion tests。
4. 更新 tracing projection。
5. 不改变现有 backend behavior。

验收：local 与 remote capture 往返后 metadata 不丢失；旧 `fallback_reason` 不再承担 source selection 说明。

### Slice 2：将 PipeWire receiver 调整为持续 latest-frame source

分类：Linux capture feature/behavior change。

工作内容：

1. 抽出具体 `LinuxFrameStream` module。
2. 每个新 damage frame 持续更新 latest。
3. 使用 reusable frame slots，PNG/artifact 保持在 callback 外。
4. 实现 first-frame readiness、cursor、wait-after 与 generation rollover。
5. 删除 automatic Screenshot portal fallback。

验收：同一 Driver session 中，动画期间没有 capture waiter 时 latest 仍会更新；静态画面 capture 可立即返回 repeated frame。

### Slice 3：让 Driver Runner 持有长生命周期 stream

分类：daemon/Runner integration feature。

工作内容：

1. platform Driver Runner 启动时构造共享 Driver session。
2. display/screen/window/input/recognition services 复用它。
3. 配置 `unless-shutdown` lifecycle 和 prewarm。
4. 暴露 capture readiness/health。
5. 验证 Run affinity 下的跨请求复用。

验收：连续两次 remote `display.capture` 使用同一 source generation；command 之间 Electron 动画仍进入 latest frame。

### Slice 4：invoke/MCP operation interface convergence

分类：active core lane convergence。

工作内容：

1. 从 command handler 移除直接 `open_local()` policy。
2. frontend authority 注入 resolved `auv::Client`/capability provider。
3. local direct mode 和 daemon-backed mode 返回同一 domain result。
4. MCP 与 CLI 共享相同 routing behavior。
5. connection failure 不触发 implicit local fallback。

验收：`auv invoke` 与 MCP 在 daemon context 下都命中相同 Runner；无 daemon context 时 local library path 仍可单次工作。

### Slice 5：bounded animation trace

分类：owner-approved animation debug feature。

工作内容：

1. 实现 duration/frame/byte 三重上限的 ring。
2. 实现 arm/freeze/range extraction。
3. 关联 input/operation events。
4. callback 外编码并写 artifact。
5. 记录 dropped/truncated/continuity evidence。

验收：已知 200–500 ms 测试动画能被完整或明确截断地记录；Runner restart 产生 typed interruption。

### Slice 6：xdpw patch upstream/retirement evidence

分类：ops/compatibility。

工作内容：

1. 将 SHM-only 和 first-frame 改动提交或关联上游 issue/PR。
2. 构建 stock 与 patched A/B images。
3. 测试 legacy SHM、ext-image-copy 和可选 DMA-BUF paths。
4. 记录删除 patch 的 live evidence。

验收：满足第 13.3 节的退出条件后再删除 patch。

## 18. 测试与 eval

### 18.1 unit tests

- sequence 在同一 generation 内单调递增。
- reconnect 后 generation 改变、sequence 重新开始不造成错误比较。
- static source 返回 repeated latest frame。
- `wait_after` 不接受 cursor 之前或其他 generation 的 frame。
- ring 同时遵守 duration/frame/byte limits。
- leased slot 不被 writer 覆盖，release 后可复用。
- stream failure 与 artifact failure 分层。
- backend failure 不调用 Screenshot portal。

### 18.2 in-process Driver tests

- fake frame producer 在没有 waiter 时持续更新 latest。
- burst producer drain 到 newest frame。
- pending `wait_after` 在后续 sequence 到达时完成。
- no-damage interval 不把 healthy static source 标记为 stale。
- format/size change 触发 generation 或明确 reconfiguration contract。

### 18.3 daemon/Runner integration tests

- 两个 RPC 复用同一个 Driver session/source generation。
- MCP 与 CLI 通过同一 daemon route 命中同一 RunnerClass。
- Run affinity 在 Runner 存活期间稳定。
- `unless-shutdown` Runner 在 idle 后仍保留 stream。
- drain 等待 active capture/trace 完成或被明确取消。
- Runner replacement 使旧 cursor 失效。

### 18.4 live RC tests

建议固定以下矩阵：

| Case | 预期 |
| --- | --- |
| static Sway background，三次 capture | 首次成功，后续 repeated frame 低延迟返回 |
| background color change | sequence 增加，像素变化可见 |
| Electron Wayland startup | stream 先 ready，启动动画出现在 frame sequence 中 |
| 200 ms CSS transition | trace 捕获多个有序 frame 或报告采样限制 |
| 画面静止 30 秒后 capture | 不重建 Portal/PipeWire；返回 healthy repeated frame |
| restart xdpw/PipeWire | generation 改变；旧 cursor continuity error |
| restart Driver Runner | daemon replacement 可用；旧 ring/lease 明确失效 |
| remove xdpw patch | 对照测试稳定失败，保留可重复 evidence |
| stock newer compositor path | 只有 live pass 后才允许替换 patched image |

### 18.5 指标

至少记录：

- cold Portal-to-first-frame latency；
- prewarmed `display.capture` p50/p95/p99 latency；
- input-event-to-first-post-cursor-frame latency；
- effective FPS 与 configured max FPS；
- dropped/dequeued/processed frame counts；
- callback CPU time；
- pixel conversion CPU time；
- latest cache bytes；
- ring retained bytes 与 truncation count；
- reconnect count/time；
- PNG/video encoding time（callback 外）。

## 19. rollout 与兼容策略

1. 先增加 metadata 和 tests，不改变 command routing。
2. 在 local Linux Driver session 内替换 receiver behavior，验证静态与动画。
3. 在 Driver Runner 中开启 persistent stream，保留 local direct path。
4. 让一个受控 frontend（建议 MCP 或 RC automation path）先使用 daemon-backed client。
5. 对比 local one-shot 与 daemon prewarmed metrics。
6. 再迁移其余 invoke commands，避免各 handler 同时维护两套 selection policy。
7. animation trace 在 latest-frame path 稳定后单独启用。
8. xdpw patch retirement 独立进行，不与 AUV lifecycle migration 混成一个 release gate。

当前 protobuf 标记为 experimental/unstable，因此应优先形成正确 domain contract；不要为了尚未承诺的 wire compatibility 增加 ad-hoc shim。已经落盘的 run/artifact schema 如需读取兼容，应另行明确 migration boundary。

## 20. 不采用的方案

### 20.1 grim runtime backend

不采用原因：subprocess、PNG round trip、weak typed errors、wlroots-only、无法提供调用前 frame history。保留为 test/diagnostic dependency。

### 20.2 Screenshot portal automatic fallback

不采用原因：interactive permission/selection、xdpw 中依赖 `slurp`、与 unattended automation contract 不一致。可以保留为未来明确命名的 user-driven operation，但不由 capture error 隐式触发。

### 20.3 每次调用 direct screencopy snapshot

不采用为 primary 的原因：即便使用 Rust in-process 实现、没有 fork，它仍只能观察 request 之后的一帧，无法恢复已经发生的动画，也无法复用 Portal authorization 和 PipeWire frame timing。它可以成为 compositor diagnostic probe，不替代持续帧流。

### 20.4 daemon 直接持有 PipeWire implementation

不采用原因：daemon control responsibility 会因此了解 Driver-specific node、format 和 frame buffer；Driver Runner 已经是共享 Driver handle 和 permission state 的 lifecycle unit。

### 20.5 只让 MCP 进程常驻

不采用原因：当前 invoke command 仍每次 `open_local()`。frontend process longevity 不等于 Driver resource longevity。

## 21. 风险与待测问题

1. 当前 memory-backed PipeWire path 持续复制最新帧的 CPU/memory bandwidth 成本是多少。
2. 30 FPS 是否足够覆盖 Electron 动画调试；提高到 60 FPS 后 pixman/xdpw/CPU 是否稳定。
3. PipeWire PTS、SPA header 和 compositor damage metadata 在当前版本组合中的完整程度。
4. output resize、scale、transform 和 hotplug 应触发 generation rollover 还是 in-generation format event。
5. 默认是否保留短 pre-roll；如果保留，duration/bytes 上限是多少。
6. animation artifact 第一版采用 frame sequence、APNG、WebM 还是其他 encoding。
7. 哪些 OCR/inference consumers 能在 Driver Runner 内消费原始 pixel format，避免 RGBA materialization。
8. Portal restore token 在 Runner replacement、workspace restart 和不同 compositor 上的行为。
9. xdpw SHM-only 与 first-frame patch 是否会被上游接受，或新版 ext-image-copy path 是否能完全替代。
10. capture readiness 应放入现有 Driver readiness detail 还是新增 capability-specific health observation。

## 22. 完成标准

本设计对应的整体调整只有在以下条件同时满足后才可称为完成：

1. Linux Wayland primary path 仍为 Portal ScreenCast + PipeWire。
2. Driver Runner 在 workspace/application 启动前可以 prewarm stream。
3. CLI/MCP daemon-backed calls 不再每次新建 local Driver session。
4. latest frame 在无 waiter 的动画期间持续更新。
5. 静态画面返回 repeated frame，不因无 damage 误报失败。
6. 动作后等待通过 generation + sequence 表达。
7. bounded animation trace 有 timing、drop 和 truncation evidence。
8. Screenshot portal/grim 不在 automatic fallback path。
9. xdpw patch 有固定版本、source、binary digest 和 live tests。
10. patch 只有在 stock replacement 通过相同 live matrix 后才删除。
11. local 与 remote operation 返回同一 domain result semantics。
12. tracing/Inspect 可以解释 capture 来自哪个 source generation、何时到达、是否 repeated，以及是否发生 continuity break。

## 23. 相关资料

- [OBS platform capture backends and AUV implications](2026-08-05-obs-platform-capture-backends-research.md)
- [AUV Device/Run/Runner aggregated API design](../session-api/2026-07-31-device-run-runner-aggregated-api-design.md)
- [AUV facade, daemon, and Runner architecture](../session-api/2026-08-03-auv-facade-daemon-runner-architecture.md)
- [AUV shared terms](../../../TERMS_AND_CONCEPTS.md)
- [AUV Linux capture implementation](../../../../crates/auv-driver-linux/src/capture.rs)
- [AUV Linux PipeWire receiver](../../../../crates/auv-driver-linux/src/native/portal/screencast.rs)
- [xdg-desktop-portal ScreenCast](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [xdg-desktop-portal PipeWire integration](https://flatpak.github.io/xdg-desktop-portal/docs/pipewire.html)
- [xdpw 0.8.4 screencast initialization](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.4/src/screencast/wlr_screencast.c)
- [xdpw 0.8.4 legacy screencopy](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.4/src/screencast/wlr_screencopy.c)
- [grim source](https://github.com/emersion/grim)
