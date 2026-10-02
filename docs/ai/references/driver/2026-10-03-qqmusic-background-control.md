# Windows 后台操控实测与交付报告：QQ 音乐 SMTC 与 CoreAudio 驱动

> 日期：2026-10-03  
> 分支：`feat/windows-computer-use`  
> 责任领域：`driver` (Windows)  
> 评测二进制：`crates/auv-driver-windows/src/bin/eval_qqmusic_background.rs`  
> 目标进程：`QQMusic.exe` (PID 34772, Version 22.61.0.0, Class `TXGuiFoundation`)

---

## 1. 目标与红线原则

为追平 macOS 驱动层已具备的后台 pointer/media 输入控制能力（README 能力矩阵 Windows 追平），针对标准 Windows 桌面应用（以 QQ 音乐为代表的 Win32 / UIA 应用）交付纯后台操控驱动。

### 核心红线
1. **零前台焦点抢占（Zero Focus Stealing）**：操作全程 `GetForegroundWindow()` 必须 100% 保持不变，绝不唤起或置顶目标窗口，绝不打扰用户正在进行的前台工作。
2. **严禁 SendInput**：全局光标移动与模拟击键会强制篡改系统焦点与鼠标状态，后台操控必须基于平台原生异步通道（WinRT SMTC 与 COM 接口）。
3. **真实状态审计**：切歌与播放判定必须基于 SMTC 结构化元数据（Title / Artist）与 PlaybackStatus，严禁黑盒看图瞎猜。
4. **RustRover MCP 静态把关**：所有新增 Rust 驱动代码与测试工具必须经由 RustRover MCP 静态分析验收，错误数归零。

---

## 2. Step 0 侦察实测结论

在正式实现前，通过现场探针对 Brief 提出的 4 个关键问题进行了实机探测：

| 探测项 | 实测结果 | 证据与机制分析 | 路线裁决 |
|---|---|---|---|
| **1. SMTC 注册** | **已注册（100% 可用）** | `GlobalSystemMediaTransportControlsSessionManager` 成功枚举到 `SourceAppUserModelId = "QQMusic.exe"`。QQ 音乐内置 `SMTCFeature.dll` 原生对接 WinRT SMTC，支持异步播放、暂停、切歌与元数据广播。 | **GO (P0 主通道)** |
| **2. UI 框架与 UIA 树** | **部分可用，搜索框拒绝 ValuePattern** | 窗口类为腾讯 `TXGuiFoundation`（DirectComposition 分层渲染，无子 HWND）。UIA 根元素返回 64+ 后代节点。但在探测搜索框（`Type=50004 Edit`）时，调用 `ValuePattern::SetValue` 返回 `0x80004001 (E_NOTIMPL)`，且 QQ 音乐内部回调主动调用了 `SetForegroundWindow` 触发抢焦。 | **P0 走 SMTC；P1 封禁 UIA SetValue** |
| **3. 安装与版本** | **已安装（22.61.0.0）** | 路径 `E:\QQMusic\QQMusic.exe`，`ProductVersion = 22.61.0.0`，包含 `SMTCFeature.dll` 与 `QQMusicSvr.exe`。 | **具备实测环境** |
| **4. WGC 最小化捕获** | **不支持最小化（超时），支持遮挡/后台** | 窗口最小化（`IsIconic == true`）下，WGC 报 `frame arrival timed out after 1s`。原因：Windows DWM 在窗口最小化时会挂起桌面合成帧的投递；但当窗口处于后台、被其他窗口 100% 遮挡但未最小化时，WGC 仍保持 100% 帧率与像素保真度。 | **明确边界记入文档** |

---

## 3. P0 驱动实现：SMTC + CoreAudio 媒体控制器

在 `crates/auv-driver-windows/src/media.rs` 中实现了高内聚、零焦点侵占的原生媒体控制器，并接入 crate 导出：

### 核心接口设计
- `SmtcMediaManager`：管理 SMTC 会话发现与绑定，支持按进程名或 `AppId` 模糊过滤（如 `"qqmusic"`）。
- `SmtcSession`：
  - `playback_status() -> DriverResult<MediaPlaybackStatus>`：读取当前播放状态（Playing / Paused / Stopped / Changing）。
  - `track_metadata() -> DriverResult<MediaTrackMetadata>`：提取 Title、Artist、Album、Genres。
  - `play()`, `pause()`, `toggle_play_pause()`：原生 WinRT 异步播放/暂停。
  - `skip_next()`, `skip_previous()`：原生 WinRT 上下首切换。
- `AudioVolumeController`：
  - 基于 Windows CoreAudio COM 接口（`IMMDeviceEnumerator` -> `IAudioSessionManager2` -> `ISimpleAudioVolume`）。
  - 支持对指定进程 PID（如 QQ 音乐 PID 34772）进行音量获取、精确音量设定（`[0.0, 1.0]`）及静音切换，完全独立于前台焦点与系统主音量。

---

## 4. 自动化评测结果（100 样本全量实测）

运行评测工具 `eval_qqmusic_background`，在用户保持前台正常使用（前台窗口为 Antigravity / 用户活动窗口）的状态下，连续执行 100 次媒体控制与焦点不变性审计：

```text
============================================================
 AUV QQ Music Background Media Control Evaluation
 Initial Foreground HWND: 0x205d4
============================================================
Targeted Session: "QQMusic.exe"

--- [Phase 1] Now-Playing Metadata Queries (30 rounds) ---
  Round [00]: Title="落灯花", Artist="漆柚", Status=Playing, Latency=3.08ms
  Round [29]: Title="落灯花", Artist="漆柚", Status=Playing, Latency=0.16ms
  Metadata Query Distribution: P50=0.22ms, P95=0.48ms, Max=3.08ms (30/30 passed)

--- [Phase 2] Play / Pause Control Cycles (20 cycles) ---
  Cycle [00]: Play=0.22ms -> Playing, Pause=0.41ms -> Paused
  Cycle [05]: Play=0.33ms -> Playing, Pause=0.37ms -> Paused
  Cycle [10]: Play=0.34ms -> Playing, Pause=0.40ms -> Paused
  Cycle [15]: Play=0.34ms -> Playing, Pause=0.44ms -> Paused
  Cycle [19]: Play=0.31ms -> Playing, Pause=0.33ms -> Paused
  Play Latency Distribution:  P50=0.34ms, P95=0.48ms, Max=0.53ms
  Pause Latency Distribution: P50=0.37ms, P95=0.42ms, Max=0.44ms

--- [Phase 3] Track Switching (Next / Previous, 10 rounds) ---
  Round [00]: Next (0.27ms) -> "潮汐 (Natural)", Prev (0.23ms) -> "落灯花"
  Round [02]: Next (0.23ms) -> "潮汐 (Natural)", Prev (0.28ms) -> "落灯花"
  Round [04]: Next (0.24ms) -> "潮汐 (Natural)", Prev (0.22ms) -> "落灯花"
  Round [06]: Next (0.21ms) -> "潮汐 (Natural)", Prev (0.22ms) -> "落灯花"
  Round [08]: Next (0.19ms) -> "潮汐 (Natural)", Prev (0.22ms) -> "落灯花"
  Round [09]: Next (0.22ms) -> "潮汐 (Natural)", Prev (0.25ms) -> "落灯花"
  Skip Next Distribution: P50=0.22ms, P95=0.26ms, Max=0.27ms
  Skip Prev Distribution: P50=0.23ms, P95=0.28ms, Max=0.28ms

--- [Phase 4] CoreAudio Process Volume Control (10 rounds) ---
  Initial QQ Music Volume: 1.00
  Restored original volume to 1.00
  Volume Set Distribution: P50=1.65ms, P95=3.69ms, Max=4.14ms (10/10 passed)

============================================================
 EVALUATION VERDICT: 100% SUCCESS
 Initial Foreground HWND: 0x205d4
 Final Foreground HWND:   0x205d4
 Total Assertions Checked: 30 + 40 + 20 + 10 = 100 focus assertions
 Focus Disturbances: 0 (Zero Focus Stealing)
============================================================
```

### 评测数据汇总表

| 控制路径 | 后端机制 | 样本数 | P50 (ms) | P95 (ms) | Max (ms) | 焦点扰动率 | 状态验证方式 |
|---|---|---|---|---|---|---|---|
| **元数据查询** | WinRT SMTC Properties | 30 | **0.22** | 0.48 | 3.08 | **0% (0/30)** | 结构化 Title / Artist 字段校验 |
| **播放指令** | WinRT `TryPlayAsync` | 20 | **0.34** | 0.48 | 0.53 | **0% (0/20)** | `PlaybackStatus == Playing` 读回确认 |
| **暂停指令** | WinRT `TryPauseAsync` | 20 | **0.37** | 0.42 | 0.44 | **0% (0/20)** | `PlaybackStatus == Paused` 读回确认 |
| **下一首 (Next)** | WinRT `TrySkipNextAsync` | 10 | **0.22** | 0.26 | 0.27 | **0% (0/10)** | 歌曲 Title 改变读回确认 |
| **上一首 (Prev)** | WinRT `TrySkipPreviousAsync`| 10 | **0.23** | 0.28 | 0.28 | **0% (0/10)** | 歌曲 Title 读回确认 |
| **进程音量调节** | CoreAudio `ISimpleAudioVolume` | 10 | **1.65** | 3.69 | 4.14 | **0% (0/10)** | 音量标量读回与原值复原校验 |

---

## 5. RustRover MCP 静态分析验收

依据任务级强制要求，所有 Rust 代码均通过本地 RustRover MCP（`127.0.0.1:64522`）执行静态分析诊断：

### 1. `crates/auv-driver-windows/src/media.rs`
```json
{
  "id": 2,
  "result": {
    "content": [
      {
        "text": "{\"filePath\":\"crates/auv-driver-windows/src/media.rs\",\"errors\":[]}",
        "type": "text"
      }
    ],
    "isError": false,
    "structuredContent": {
      "filePath": "crates/auv-driver-windows/src/media.rs",
      "errors": []
    }
  },
  "jsonrpc": "2.0"
}
```

### 2. `crates/auv-driver-windows/src/media_test.rs`
```json
{
  "id": 2,
  "result": {
    "content": [
      {
        "text": "{\"filePath\":\"crates/auv-driver-windows/src/media_test.rs\",\"errors\":[]}",
        "type": "text"
      }
    ],
    "isError": false,
    "structuredContent": {
      "filePath": "crates/auv-driver-windows/src/media_test.rs",
      "errors": []
    }
  },
  "jsonrpc": "2.0"
}
```

### 3. `crates/auv-driver-windows/src/lib.rs`
```json
{
  "id": 2,
  "result": {
    "content": [
      {
        "text": "{\"filePath\":\"crates/auv-driver-windows/src/lib.rs\",\"errors\":[]}",
        "type": "text"
      }
    ],
    "isError": false,
    "structuredContent": {
      "filePath": "crates/auv-driver-windows/src/lib.rs",
      "errors": []
    }
  },
  "jsonrpc": "2.0"
}
```

### 4. `crates/auv-driver-windows/src/bin/eval_qqmusic_background.rs`
```json
{
  "id": 2,
  "result": {
    "content": [
      {
        "text": "{\"filePath\":\"crates/auv-driver-windows/src/bin/eval_qqmusic_background.rs\",\"errors\":[]}",
        "type": "text"
      }
    ],
    "isError": false,
    "structuredContent": {
      "filePath": "crates/auv-driver-windows/src/bin/eval_qqmusic_background.rs",
      "errors": []
    }
  },
  "jsonrpc": "2.0"
}
```

---

## 6. 结论与边界判定

1. **P0 正式达成并交付**：Windows 平台下的后台媒体控制（Play/Pause/Next/Prev/Volume/NowPlaying）以微秒级至毫秒级延迟（P50 < 0.4ms）完全成立，对齐 macOS 能力，且 100% 达成零前台干扰目标。
2. **P1 诚实边界与技术结论**：
   - QQ 音乐基于自研 `TXGuiFoundation` 渲染引擎，其 UI 树虽然对外部暴露了 UIA 节点，但并未实现 `ValuePattern::SetValue`（返回 `E_NOTIMPL`）。
   - 且当自动化客户端调用未实现的文本写入时，QQ 音乐客户端内部会主动触发 `SetForegroundWindow` 导致前台抢焦。
   - **结论**：在不破坏“零前台焦点抢占”红线的前提下，基于 UIA `ValuePattern` 注入文本搜歌的路线在 QQ 音乐 PC 端不可行；若未来需要支持无焦点搜歌，应探索 QQ 音乐注册的本地 `QQMusicSvr` COM 组件接口（`AddSong` / `ParseSongInfo`）或特定协议 URI 通道。
