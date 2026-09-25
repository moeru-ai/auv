# 结构化空间记忆模块设计（2026-09-26）

> 替代 3DGS-as-memory 路线的确定性、typed、file-backed 结构化空间记忆实现设计。

---

## 1. 目的与核心原则

在稀疏视角（2–3 视角）下，3DGS 训练严重过拟合且无法泛化到未见视角（详见
`docs/ai/references/3dgs/2026-09-25-slice-b-closeout.md`）。因此，3DGS 作为空间记忆的方案已暂停。

本模块的目标是：**利用 M2 会话中已有的结构化数据（telemetry raycast 命中、方块绝对坐标、视角位姿），
在 `auv-game-minecraft` crate 内实现一个确定性、类型安全（typed）、支持文件落盘（file-backed）的空间记忆模块。**

### 核心原则
1. **真实几何锚点**：空间记忆的几何主键是游戏引擎 telemetry / raycast 的方块坐标（`BlockPosition`），绝不依赖 VLM 幻觉或猜测。
2. **确定性几何查询**：可见性与屏幕投影查询通过精确投影矩阵与空间向量运算完成，查询路径不经过 VLM。
3. **语义与几何解耦**：VLM 仅用于生成非几何的描述性属性（description），禁止其对世界坐标做断言。
4. **离线重放验证**：所有单元测试与回归测试基于真实录制的 M2 会话（如 `.tmp/m2-session/`）重放，不启动 Minecraft 游戏进程。
5. **诚实边界声明**：视锥外部（OutOfFrustum）是几何确定的；而在缺乏稠密深度缓冲与逐方块遮挡探测信号时，遮挡状态一律诚实返回 `Unknown`，禁止猜测 `Occluded`。

---

## 2. 模块划分与类型系统

### 2.1 存储模块：`spatial_memory_store.rs`

负责持久化管理结构化地标（`SpatialLandmark`）。

```rust
pub enum LandmarkKind {
  BlockSurface,
  Object,
  Region,
  PathNode,
}

pub enum LandmarkSource {
  TelemetryRaycast,
  MultiViewTriangulation,
  VlmHypothesis,
}

pub struct ObservationRef {
  pub observation_id: String,
  pub captured_at_millis: u64,
}

pub struct LandmarkObservation {
  pub observation_ref: ObservationRef,
  pub source: LandmarkSource,
  pub hit_face: Option<BlockFace>,
  pub block_id: Option<String>,
}

pub struct SpatialLandmark {
  pub landmark_id: String,              // 格式 "lm-<x>-<y>-<z>-<kind>"
  pub kind: LandmarkKind,
  pub position: BlockPosition,          // 几何主键
  pub first_observed: ObservationRef,
  pub observations: Vec<LandmarkObservation>, // 追加式观测证据
  pub status: SpatialClaimStatus,       // 复用 SpatialClaimStatus::Confirmed / Candidate / Hypothesis
  pub source: LandmarkSource,
  pub description: Option<String>,      // 属性描述（非几何）
}

pub struct SpatialMemoryStore {
  landmarks: HashMap<String, SpatialLandmark>,
  path: PathBuf,
}
```

- **去重与合并**：当新传入的 raycast 命中与已有地标的欧氏距离 $< 0.6\text{ m}$ 时合并（追加 observation，保持 first_observed 不变）；否则新建地标。
- **文件落盘**：`open` 自动支持加载已存 JSON 文件或创建空存储；`save` 输出 pretty-printed JSON。
- **范围查询**：`query_radius(center, radius_m)` 支持三维欧氏距离内的邻近地标过滤。

### 2.2 会话摄入模块：`spatial_memory_ingest.rs`

将多视角会话（`M2Session`）中的 raycast 信号提取为记忆地标。

```rust
pub struct IngestReport {
  pub landmarks_created: usize,
  pub landmarks_merged: usize,
  pub observations_skipped: usize,
}

pub fn ingest_m2_session(store: &mut SpatialMemoryStore, session: &M2Session) -> IngestReport;
```

- 遍历 `session.observations`，依据 `telemetry_frame_id` 关联其真值遥测帧。
- 提取 `raycast_hit` 并执行 `store.upsert_from_raycast()`。
- **无信号不编造**：无 raycast 命中的视角（如 M2 会话中的 v03）严格记入 `observations_skipped`，绝不凭空捏造空间坐标。

### 2.3 几何查询模块：`spatial_memory_query.rs`

提供给定观察者位姿（`PlayerPose`）对目标地标的几何查询能力。

```rust
pub enum LandmarkTarget {
  LandmarkId(String),
  BlockPos(BlockPosition),
}

pub enum QueryKind {
  Visibility,
  ScreenProjection,
  Direction,
}

pub enum AnswerStatus {
  Answered,
  Unknown,
  Refusal,
}

pub enum VisibilityClass {
  Visible,
  Occluded,
  OutOfFrustum,
  Unknown,
}

pub struct SpatialMemoryQuery {
  pub observer_viewpoint: PlayerPose,
  pub target: LandmarkTarget,
  pub query_kind: QueryKind,
  pub observer_frame: Option<MinecraftSpatialFrame>,
  pub viewport: Option<Viewport>,
}

pub struct SpatialMemoryAnswer {
  pub status: AnswerStatus,
  pub visibility: VisibilityClass,
  pub screen_xy: Option<(f64, f64)>,
  pub yaw_pitch_delta: Option<(f64, f64)>,
  pub confidence: f64,
  pub evidence_observation_ids: Vec<String>,
}

pub fn query_spatial_memory(store: &SpatialMemoryStore, q: &SpatialMemoryQuery) -> SpatialMemoryAnswer;
```

- **目标查找**：优先通过 ID 或坐标在 store 中检索；目标不存在时直接返回 `status = Unknown`。
- **投影计算**：利用 `MinecraftProjector` 将方块目标投影到视口坐标。支持传入实际观察帧，或依据 observer `PlayerPose` 与 70° 标准 FOV 纯数学构建相机矩阵。
- **角度偏移**：计算目标相对于观察者朝向的 `yaw_pitch_delta`（偏航与俯仰角度偏移量）。
- **置信度模型**：Confirmed 地标基础置信度 0.90，每增加一次独立视角观测递增 0.02，上限 0.99；Hypothesis 地标上限 0.50。

---

## 3. 验证与基准对齐

1. **真实 M2 会话 Replay**（`tests/spatial_memory_ingest_replay.rs`）：
   - 会话 `F:/auv/.tmp/m2-session/`（v01、v02 命中同一 `(-22, 81, 43)` 方块，v03 无命中）。
   - 验证摄入结果：`landmarks_created = 1`，`landmarks_merged = 1`，`observations_skipped = 1`，地标全部为 `TelemetryRaycast` 与 `Confirmed`。
2. **M3 几何交叉验证**（`tests::cross_validation_with_m3_scoring_tolerance`）：
   - 以 anchor 视角投影 revisit 目标方块，计算屏幕坐标。
   - 与 M3 几何评分标准（`M3_PROJECTION_TOLERANCE_PX = 48.0`）交叉比对，误差在容差范围内（实际几何对齐误差 $< 0.1\text{ px}$）。
3. **M4/3DGS 废弃标记**：
   - `src/m4_trainer.rs` 标记模块级 `#[deprecated]`。
   - `src/training_launch.rs` 与 `src/training_package.rs` 中的 3DGS 专用公开函数标记 `#[deprecated]`。
