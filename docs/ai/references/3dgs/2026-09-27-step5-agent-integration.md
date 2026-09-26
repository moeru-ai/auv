# Step 5: Agent 集成与有界实战测试

本篇文档记录空间记忆模块第一次作为完整 Live Agent 循环消费者的系统集成方案、工程落地与有界实战验证结果。

- **前置基线**：Top 3 生产卡点攻坚（P1 遮挡判定、P2 鲁棒深度与零锚点门控、P3 动静双轨与生命周期闭环，commit `dfb963d7`）。
- **核心目标**：将空间记忆模块接通至动作派遣链路（`SpatialMemoryStore` -> query -> 3D->2D 投影 -> 窗口点击），在代码层强制贯彻 4 条作战守则，并在无 live 游戏的离线环境下跑通有明确指标判定的实战测试。

---

## 1. 现状调查（T0 调查结论）

在实现前，对现有相关模块与 live 采集链路进行了系统调查：

1. **M2 Live 采集路径与复用性**：
   - **截图采集**：在 `supported/games/auv-game-minecraft/src/cli/projection_workflow.rs` 的 `capture_target_screenshot` 中，通过 `auv_driver::open_local()` 获取本地驱动会话，按 `target_app` 与 `target_title` 定位 Minecraft 窗口并执行 `session.window().capture(&window)`。同时支持直接读取磁盘图片（`load_screenshot`）。
   - **遥测数据采集**：Minecraft Mod (Fabric/Forge sidecar) 将每帧玩家位姿 `PlayerPose` 与光线投射 `RaycastHit` 持续追加写入 `telemetry.jsonl`。在 `src/ingest.rs` 中已提供高效的尾部轮询器 `read_latest_spatial_frame_from_tail` 与 `read_latest_spatial_frame_newer_than`。
   - **帧绑定机制**：`src/bind.rs` 中的 `bind_capture_to_frame` 负责完成单帧截图与遥测帧的时间戳偏斜对齐（skew recording）。
   - **结论**：工程具备完整的采集底层，但此前缺乏单 tick 统一的采集流结构。本任务抽象出统一的 `LiveCapture` 结构体，供 Agent 循环单步调用。

2. **Wiring 模式对比（旧 vs 新）**：
   - **旧 wiring (`training_result_spatial_query_action_wiring.rs`)**：输入依赖离线 3DGS 评测生成的静态 `TrainingResultSpatialQueryManifest`，通过 `derive_action_readiness` 判断 eligibility，数据源是一次性离线产物，无法支撑动态游戏循环。
   - **新 wiring (`memory_action_wiring.rs`)**：直接面向实时维护的 `SpatialMemoryStore`。Agent 发起语义标签查询（例如 `"chest"` 或 `"grass_block"`），通过几何反投影计算屏幕坐标，并结合当前帧 `MetricDepthMap` 执行物理前向遮挡判定。如遇遮挡直接拒绝动作（`refusal_reason: "target occluded"`），避免盲点误触。

3. **`DirectWindowPointClickExecutor` 可用性**：
   - 依赖系统实际运行的 Minecraft 窗口（通过窗口选择器解析 `java.exe`）。
   - 在开发机或 CI 等离线无游戏环境中，窗口解析必定失败。因此必须通过 `ActionExecutor` trait 对点击能力进行接口抽象，提供 `MockActionExecutor` 供离线实战 harness 运行，生产环境中则无缝接入 `DirectWindowPointClickExecutor`。

---

## 2. `AgentMemoryLoop` 架构与作战守则落地

模块位于 [`src/agent_memory_loop.rs`](file:///F:/auv/supported/games/auv-game-minecraft/src/agent_memory_loop.rs)。

### 2.1 4 条作战守则在代码层强制约束

| 作战守则 | 代码约束实现 | 行为保障 |
| :--- | :--- | :--- |
| **#1 无遥测拒绝启动** | `if self.config.require_mod_telemetry && capture.player_pose.is_none()` | 立即返回 `Err(LoopError::TelemetryRequired)`，绝不在无位姿时虚构坐标 |
| **#2 无锚点不反投影** | `if self.calibrator.fit().is_none()` 零锚点门控 | 跳过视觉感知反投影，`visual_skipped_reason = Some("no depth calibration anchors")` |
| **#3 高置信度感知** | `config.yolo_confidence_threshold = 0.50`，遍历检测时过滤 | 拦截低于 0.50 的 Minecraft 方块噪点与纹理误报 |
| **#4 1Hz 稳定周期** | `config.tick_interval_millis = 1000` | 控制模型推理开销与内存生命周期刷新频率 |

### 2.2 单步 Tick 处理时序

```text
LiveCapture (截图 + PlayerPose + RaycastHit + 时间戳)
  │
  ├─ 1. 守则校验: player_pose 是否存在？缺失直接 Err(TelemetryRequired)
  ├─ 2. 内存维护: MemoryMaintenance.maybe_prune 清理过期动态轨迹与低置信度方块
  ├─ 3. 遥测 Ingest:
  │     └─ 若有 RaycastHit:
  │           ├─ store.upsert_from_raycast (创建或合并 Confirmed Landmark)
  │           ├─ apply_raycast_negative_evidence (清理视线沿途穿透误报)
  │           └─ 深度标定器添加锚点: (预测准星深度, 真实光线投射欧氏距离)
  ├─ 4. 视觉 Ingest:
  │     └─ 若有截图且模型加载:
  │           ├─ 检查 calibrator.fit(): 若无有效拟合，触发零锚点门控跳过
  │           └─ YOLO-World 检测 (>=0.50) + 区域内缩深度中位数 + 反投影写入
  └─ 5. 输出 TickReport (新增数、合并数、跳过数、清理数、门控原因、耗时 ms)
```

---

## 3. 记忆→动作 Wiring 与遮挡防御

模块位于 [`src/memory_action_wiring.rs`](file:///F:/auv/supported/games/auv-game-minecraft/src/memory_action_wiring.rs)。

### 3.1 核心流程

```rust
pub fn wire_memory_query_to_action(
  store: &SpatialMemoryStore,
  query: &MemoryActionQuery,
  executor: &impl ActionExecutor,
) -> MemoryActionOutcome
```

1. **语义解析**：在 `SpatialMemoryStore` 中按 `query.label` 匹配 Landmark（支持 `description` 前缀匹配、Observation `block_id` 匹配以及 `landmark_id` 匹配）。
2. **几何投影**：构造 `SpatialMemoryQuery`，调用 `query_spatial_memory(store, &spatial_query, query.depth_map)`。
3. **可见性与遮挡门控**：
   - `VisibilityClass::Occluded`：前向深度图检测到更近的障碍物（例如墙体遮挡），返回 `refusal_reason: Some("target occluded")`，**坚决不派发点击**。
   - `VisibilityClass::OutOfFrustum`：目标在视锥外，返回 `refusal_reason: Some("target out of frustum")`。
   - `VisibilityClass::Unknown`：视线不可断定，返回 `refusal_reason: Some("target visibility unknown")`。
   - `VisibilityClass::Visible`：目标可见且无遮挡，提取投影屏幕坐标 `WindowPoint(Point { x, y })`。
4. **动作派发**：调用 `executor.click(window_point)` 并将物理点击动作记录至 outcome。

---

## 4. 有界实战测试与实测指标

集成测试位于 [`tests/field_test_scenario.rs`](file:///F:/auv/supported/games/auv-game-minecraft/tests/field_test_scenario.rs)，使用真实的 M2 会话数据（v01、v02、v03）进行回放验证。

### 4.1 测试场景设计

1. **探索建图**：
   - Tick 1（v01）：载入第一视角观测，光线投射命中 `(-22, 81, 43)`，创建 Confirmed Landmark，添加第 1 个标定锚点（此时锚点数 1 < 2，视觉感知被有效门控）。
   - Tick 2（v02）：载入平移后视角观测，光线投射再次命中 `(-22, 81, 43)`，成功合并 Landmark，添加第 2 个标定锚点（满足 OLS 拟合条件）。
   - Tick 3（v03）：载入第三视角观测（无光线投射命中），验证无光线输入时的稳定性。
2. **记忆查询**：查询目标 `"grass_block"`（真实方块 `(-22, 81, 43)`）。
3. **动作派遣**：在 v02 视角下反投影目标并调用 `MockActionExecutor` 执行窗口点击。

### 4.2 4 项核心实战指标上报

测试运行输出：

```text
================ FIELD TEST SCENARIO METRICS ================
  recall_success:      true
  projection_error_px: 0.0000 px
  false_landmarks:     0
  visual_gated_ticks:  1
=============================================================
```

- **`recall_success` (`true`)**：建图后能够在记忆中准确召回目标 landmark。
- **`projection_error_px` (`0.0000 px`)**：反投影派发点击坐标与真实投影真值完全重合（误差远小于 2.0px 上限）。
- **`false_landmarks` (`0`)**：静态白名单与高置信度门限生效，没有产生白名单外或无真值对应的错误 landmark。
- **`visual_gated_ticks` (`1`)**：在缺乏充分深度锚点的 Tick 1 中，零锚点门控成功拦截未标定的反投影，防止空气墙与漂移污染。

---

## 5. Live 游戏实战的前置条件

本任务提供的 harness 已在离线 replay 环境中验证全链路可用。若后续推进到真实 Minecraft 客户端实弹运行，需满足以下环境前置条件：

1. **客户端与窗口**：本地需运行 Java 版 Minecraft（版本 1.21.1），窗口标题包含 `Minecraft*` 且处于非最小化可见状态。
2. **遥测 Sidecar**：安装并加载 Fabric/Forge Telemetry Mod，实时将 `player_pose` 与 `raycast_hit` 写入 `telemetry.jsonl`。
3. **输入驱动权限**：Windows 平台需具备对游戏窗口的发送输入权限（`auv-driver` Windows backend 通过 PostMessage/SendInput 派发）。
4. **视觉推理模型**：本地 `F:/.auv/.tmp/models/` 需包含 `yolov8s-worldv2.onnx` 与 `model-small.onnx`。
