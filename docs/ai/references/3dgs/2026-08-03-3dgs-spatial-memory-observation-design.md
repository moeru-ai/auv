# 3DGS 空间记忆观测设计

日期：2026-08-03

状态：**当前设计基线，尚未批准实现**

这是 3DGS spatial memory 的重点入口。本文先回答一个比“用哪个训练器”更
基础的问题：

> 当游戏是闭源的，智能体到底能观测到什么、可以推断什么，以及内置空间
> 记忆允许被怎样修改？

第一验证环境是 Minecraft。Minecraft 的 mod/telemetry 可以提供答案键，但
这不代表它等价于闭源游戏；验证时必须把“模型输入”和“评分真值”分开。

## 1. 先定结论

1. **闭源游戏可以做。** 通用基线是外部观测：窗口截图、时间、窗口信息、
   AUV 输入历史，以及从连续截图中计算出来的信号。
2. **单张截图只能产生假设，不能产生事实。** 它可以描述可见表面和可能的
   相对几何，但不能证明隐藏几何、精确尺度、碰撞边界或世界坐标。
3. **内置 Prompt 只写入 hypothesis 层。** 确定性 writer 可以把 Prompt 的
   patch 追加进候选记忆，但只有独立观测或答案键验证通过后，才能提升为
   confirmed memory。
4. **3DGS 不是空间记忆本身。** 它是一个外观重建和视角重投影 backend。
   没有训练结果时，空间记忆查询仍然应该有清楚的失败和未知语义。
5. **Minecraft 先做可证伪验证。** 模型输入限制为黑盒观测包，Minecraft
   的 telemetry/world state 只用于事后评分，不能偷偷喂给 Prompt。
6. **先采集和评估，再训练。** 真实多视角 capture、单视角/多视角假设评分、
   memory query 评分，都应先于把 Brush 或 OpenSplat 当成产品证据。

这些是本条 3DGS 研究线的设计决定，不自动批准新的 AUV core contract 或
新的 CLI surface。

## 2. “遥感信号”在游戏里对应什么

遥感类比只有在把“信号”理解成传感器模型产生的测量时才有用。游戏截图不是
现实世界的物理辐射，而是引擎把几何、材质、光照、阴影、后处理、tone mapping
和 UI 合成后的最终图像。

| 遥感概念 | 游戏侧对应物 | 证据强度 |
| --- | --- | --- |
| 被动传感器接收的光能 | 最终 RGB/窗口 framebuffer/scene color | 只能证明渲染外观，不等于几何真值 |
| 发射信号并接收返回 | 主动移动相机、连续视角、raycast/trace、collision query | 平移产生视觉视差；trace 只有在被暴露时才是引擎真值 |
| 多光谱/多通道观测 | depth、normal、motion、albedo/base color、roughness、object id、semantic mask、G-buffer | 只有游戏实际暴露时才可作为强辅助信号 |
| 传感器模型 | camera intrinsics/extrinsics、view/projection matrix、viewport、capture time | 让观测可进行几何解释 |
| 地理配准 | world coordinates、稳定 anchor、relative pose、timestamp 对齐 | 用于把多个观测合到同一世界记忆 |
| DEM/地图/地形先验 | depth map、point cloud、collision mesh、block grid、navmesh、level geometry、scene graph | 来源可靠时可以是精确真值，否则只是先验 |

工程上可以这样记：

```text
RGB screenshot              = 虚拟被动光学观测
多张带平移的截图            = 虚拟时序/立体观测
受控相机移动                = 虚拟主动扫描
raycast / line trace        = 引擎侧主动探针
depth / G-buffer            = 引擎侧多通道传感器
world pose + timestamp      = 配准模型
```

不能因为某个引擎理论上有 depth、raycast、G-buffer 或 world pose，就把它们
写进当前 observation。输入中没有明确存在的信号必须记为 unknown，Prompt
不得自行补全。

## 3. 闭源游戏的信号层级

下面的层级描述 AUV 在不做游戏专用集成时能依赖什么。高层级是可选 adapter，
不是通用假设。

### Tier 0：外部黑盒观测

大多数桌面游戏都可能拿到：

- 外部 RGB 截图；
- capture timestamp 和单调顺序；
- 窗口边界、viewport 尺寸、display scale、capture backend；
- 最近的 AUV 输入历史，包括镜头旋转和移动动作；
- capture source 能证明的应用/窗口身份。

这是跨游戏的可移植基线。它不能直接提供精确深度、世界坐标、碰撞、隐藏表面
或稳定的语义对象身份。

### Tier 1：游戏外推导信号

AUV 或视觉模型可以从连续观测推导：

- feature tracks 和 optical flow；
- 相对相机运动和 parallax；
- monocular depth 或 surface normal 估计；
- 图像分割、object candidate、线/平面假设；
- visual loop-closure candidate。

这些是有用的假设，不是传感器真值。monocular depth 不是 depth sensor，
VLM/LLM 的场景描述也不是 world anchor。

### Tier 2：暴露出来的渲染信号

某些游戏、debug mode、plugin、capture API 或 graphics hook 可能暴露：

- depth；
- normal；
- motion vector；
- albedo/material channel；
- object id 或 semantic mask；
- G-buffer 或 scene-texture 数据。

这些通道比 RGB 更适合重建，但 adapter 必须记录来源、坐标系，以及它们是否
和 RGB capture 在时间上对齐。通用闭源路径不能依赖这些通道。

### Tier 3：引擎或游戏集成

官方 API、mod、plugin 或获准的 integration 可能暴露：

- camera pose 和投影参数；
- raycast/line trace 或 shape trace；
- collision geometry；
- navmesh、level geometry、scene graph 或 object id；
- 稳定 world coordinates 和对象生命周期。

这可以是引擎真值，但它已经不是纯黑盒。Minecraft 在验证时使用这一层作为
答案键；不能把 Minecraft 的 Tier 3 结果说成任意闭源游戏都能做到。

每条记忆 claim 都应保留 signal tier 和 provenance。一个来自 Tier 0 RGB 的
claim，不能因为后来出现了 Tier 3 adapter，就被静默升级成事实。

## 4. 单视角猜测策略

单视角输出必须分成三类：

| 输出类别 | 例子 | 写入策略 |
| --- | --- | --- |
| 直接观测 | 可见颜色边界、表面、屏幕区域、疑似对象 | 追加与 observation 绑定的证据 |
| 空间假设 | 可能的墙面、深度顺序、可能的开口、目标位置 | 追加为带不确定性的 hypothesis |
| 未知/矛盾 | 背面、尺度、碰撞、遮挡关系、缺少 pose | 保留 unknown 或请求更多证据 |

模型可能很确定某个像素边界存在，但对它在三维空间中的解释仍然很不确定。
因此不能只有一个总 confidence，至少要分开：

```text
appearance_confidence
geometry_confidence
metric_scale_confidence
semantic_confidence
world_registration_confidence
```

普通单张 RGB 截图不能确认：

- 物体背面或内部；
- 没有尺度来源时的精确深度和物体尺寸；
- 碰撞或交互边界；
- reload 后仍然成立的 world coordinates 或 persistent identity；
- 被遮挡物体是否存在；
- 一个视觉上合理的点是否真的是可操作目标。

## 5. 主动补采策略

当不确定性会影响空间查询或动作时，Prompt 应该请求最小的下一次观测，而不是
用更强的措辞掩盖证据不足。

推荐顺序：

1. **小幅横向或前后移动。** 产生 parallax，是黑盒深度的首要信号。
2. **短距离绕目标移动。** 目标仍在视野内时，可以补充表面关系。
3. **返回或重访原视角。** 检查时间稳定性和 loop closure。
4. **原地 yaw/pitch sweep。** 能补充外观覆盖，但没有平移 baseline，不能
   替代三角测量。
5. **对目标做 raycast/trace。** 只在 integration tier 可用，不是闭源通用能力。

每次请求必须写明：要解决的 uncertainty、建议动作、需要采集的最小信号。
“再多看一看”不是 capture protocol。

## 6. 记忆层级与写入边界

空间记忆必须是追加式并且和证据绑定：

```text
RawObservation
  -> SpatialHypothesisPatch
  -> CandidateSpatialMemory
  -> ConfirmedSpatialMemory
```

### RawObservation

不可变的原始记录，包含截图 artifact、时间、capture/window metadata、输入历史，
以及实际可用的可选信号。它不应包含模型对图像的重新解释。

### SpatialHypothesisPatch

内置 Prompt 的输出。它引用一个或多个 observation，可以增加候选表面、对象、
关系、屏幕投影、unknown 和 follow-up capture request。它允许出错，但必须明确
标记为 hypothesis。

### CandidateSpatialMemory

确定性 writer 可以合并兼容的 hypothesis patch，形成候选地图，但必须保留：

- provenance；
- 各维度 confidence；
- 冲突 claim；
- coordinate space；
- promotion state。

合并不等于确认。

### ConfirmedSpatialMemory

只有验证规则通过后才能进入。例如：

- 同一个 anchor 从独立视角被观察到，且相对几何一致；
- 目标在另一视角的预测投影和独立观测一致；
- 暴露的引擎信号或 Minecraft 答案键与 claim 一致；
- holdout view 在声明的误差范围内确认外观/投影行为。

Prompt 不拥有直接写入这一层的权限。它只能请求 promotion，validator 才能决定。

所以“内置 Prompt 依据观测修改 3D memory”的安全实现是：Prompt 通过显式
patch API 追加 hypothesis/candidate memory，但不能覆盖 confirmed memory。

## 7. 内置 Prompt 角色

它应该是受约束的空间观测解释器，不是泛化的场景旁白。输入必须显式列出哪些
信号真的存在。

### Prompt 基线

```text
你是单视角空间记忆解释器，不是世界真值生成器。

你只能使用输入中明确存在的截图、时间、视口信息、输入历史，以及被标记为
available 的 depth、normal、motion、raycast、world pose 或 telemetry。
禁止假设不存在的信号存在。禁止把模型常识、游戏常识或语言补全当作观测。

请把结果分成：
1. 直接观察到的内容；
2. 基于观测提出的空间假设；
3. 当前无法确认或互相矛盾的内容。

每个空间假设必须包含：证据引用、坐标空间、几何/尺度/语义/配准置信度，
以及明确的 hypothesis 状态。单张普通 RGB 截图不能确认隐藏表面、精确深度、
碰撞边界、世界坐标或可交互性。RGB-only 时，`unknowns` 或
`unsupported_inferences` 必须包含精确 token `world_coordinate` 和
`hidden_geometry`。

如果证据不足，请提出最小的后续采集动作。优先请求小幅横向或前后移动来制造
视差；原地转头只能补充外观，不能替代平移基线。

输出 SpatialHypothesisPatch。你只能写入 hypothesis 或 candidate 范围，不能
直接覆盖 confirmed spatial memory。不要删除冲突证据，不要把 unknown 改写成
false，也不要把 blocked 改写成 ready。
```

### 建议输出结构

第一版应使用有版本、可 inspect 的 JSON，而不是自由文本：

```json
{
  "schema_version": 1,
  "observation_ids": ["obs-123"],
  "claims": [
    {
      "claim_id": "claim-1",
      "kind": "surface|object|relation|projection",
      "description": "前方可能存在一面垂直表面",
      "coordinate_space": "screen_relative|camera_relative|world|unknown",
      "status": "hypothesis",
      "confidence": {
        "appearance": 0.9,
        "geometry": 0.45,
        "metric_scale": 0.05,
        "semantics": 0.35,
        "world_registration": 0.0
      },
      "evidence_refs": ["rgb_screenshot", "perspective_lines"],
      "unsupported_inferences": ["backside", "collision_boundary"]
    }
  ],
  "unknowns": ["world_coordinate", "hidden_geometry"],
  "requested_follow_up_capture": {
    "action": "strafe_right",
    "reason": "需要横向视差确认平面关系",
    "minimum_observations": 1
  },
  "write_scope": "hypothesis_only"
}
```

具体 Rust type name 暂不冻结。应先用 MC 实验确认真正被消费的字段，再批准
实现 slice，避免先造一个没人使用的大 schema。

## 8. Minecraft 验证顺序

Minecraft 同时扮演“有答案键的验证环境”和“可模拟黑盒输入的候选环境”，两种
测试必须分开。

### M0：fixture 和 contract test

用固定 screenshot/observation fixture，验证 Prompt 输出：

- 分开 observation、hypothesis、unknown；
- 带 evidence reference 和多维 confidence；
- 只能写 `hypothesis_only` 或 `candidate`；
- 不会凭空生成 depth、raycast、world pose 或 G-buffer 字段。

这一步不需要 3DGS 训练。

**当前状态（2026-08-03）：M0 已落地。**

`supported/games/auv-game-minecraft/src/spatial_memory_observation.rs` 现在提供
了 signal tier、`SpatialObservationPacket`、`SpatialHypothesisPatch`、内置
`SINGLE_VIEW_SPATIAL_MEMORY_PROMPT` 和只允许追加 hypothesis/candidate 的
`SpatialHypothesisMemory`。validator 会拒绝：

- Prompt 直接写入 confirmed memory；
- patch 或 observation schema 不匹配；
- 黑盒输入未提供的 raycast/world pose/depth 等信号；
- 没有 screenshot artifact ref 的 RGB observation；
- 没有 world pose 却声称 world 坐标；
- 超出 `[0, 1]` 的 confidence。

本地验证结果：`cargo test -p auv-game-minecraft --lib --quiet` 为 **180 passed**，
`cargo check -p auv-game-minecraft --all-targets --quiet` 通过。这里没有 LLM
transport、真实 Minecraft capture、多观察合并或 confirmed-memory promotion；
这些仍属于后续 M1-M3 验证，不是 M0 的隐含成果。

### M1：黑盒单视角 baseline

模型只接收：

- Minecraft screenshot；
- capture time、viewport、window metadata；
- 可用的 input history；
- 不包含 `nearby_blocks`、block coordinates、raycast hit、camera matrices 或
  hidden telemetry。

事后用 Minecraft telemetry/world state 给 claim 打分，测量 Prompt 在不作弊时
能产生多有用的假设。

**当前状态（2026-08-12）：M1 请求/响应边界已开始落地，但真实 baseline 仍未完成。**

`m1_black_box_baseline.rs` 现在可以生成 provider-neutral 的请求 artifact，且只接受
Tier 0 黑盒信号；它按 signal kind 和 tier 双重 allowlist 拒绝 derived、render 或
engine-truth signal，并约束 provenance、input action 和 canonical AUV screenshot
artifact URI，避免把自由字符串当成隐藏真值通道。反序列化后的请求还会重新验证
schema 和内置 Prompt，不能靠持久化请求篡改绕开 constructor。序列化后的模型请求
不含 camera matrix、player pose、raycast、nearby blocks、telemetry 或 depth buffer。
外部模型返回的 JSON 会被解析成
`SpatialHypothesisPatch`，再经过 M0 validator 和 M1 follow-up gate；parse/contract
failure 会形成明确的 rejected report。

本 slice **没有**选择或调用 LLM provider，也没有把“JSON 合法”当成语义正确。
仓库当前不存在可复用的 LLM/VLM transport，直接添加 OpenAI/Anthropic HTTP client
会凭空冻结新的 secret、provider 和 retry 接口。真实模型 transport 和原始响应
artifact 持久化仍是后续 slice。

**当前状态（2026-09-12）：** scorer / 校准聚合器先落地；同日晚间 live 5 样本将 honesty-calibration baseline 收口（见本节末）。不是 M2。

`m1_black_box_observation.rs` 把 `BoundSpatialFrame` 拆成黑盒 `SpatialObservationPacket`
和 `M1WithheldMinecraftTruth`：截图 URI、capture clock、viewport 进入 observation；
pose、raycast、`nearby_blocks`、相机矩阵、库存和资源包只留在 withheld 一侧。
AUV input history 仍是调用方传入的；sidecar 不会从 pose 差分捏造输入。

`prepare_m1_black_box_from_telemetry_tail` 是这条链的 live 入口：读 sidecar JSONL
最新一帧，拒绝 `menu` / `loading_or_overlay`（以及缺失 `screen_state`），再 bind
调用方提供的 canonical screenshot URI 并 `prepare_m1_black_box_request`。Windows
上 live 截图 producer 已固定为 `auv invoke window.capture`（`auv-driver-windows`
PrintWindow/xcap，canonical artifact URI）。2026-09-12 live：`window.capture --title Minecraft`
得到 `backend=printwindow.windows`、无 fallback、非黑 in-game PNG。短生命周期
`auv invoke` 曾把 process-local `Instant` 打成 `capture_monotonic_timestamp_ms: 0`；
Windows 现改为 `GetTickCount64`。GLFW/MC 窗口仍可能黑帧；本 slice 不加 WGC。

`m1_black_box_scoring.rs` 只对 *已经通过* contract gate 的 patch 打分，并把
`M1WithheldMinecraftTruth` 留在 request 类型之外。当前 claim schema 没有结构化
世界坐标，所以这一刀评的是诚实性和校准，不是像素/方块命中率：

- 序列化后的 request 不得泄漏 withheld eye pose 或 block id；
- RGB-only patch 必须声明 `world_coordinate`（以及有 raycast 真值时的
  `hidden_geometry`）unknown；
- `world_registration` / `metric_scale` 有 RGB-only 上限；
- geometry 仍不确定时，follow-up 必须请求能产生视差的平移，而不能只 yaw/pitch。

`m1_black_box_verification.rs` 把上述三步收成一条库 API：
`verify_m1_black_box_from_telemetry_tail`（telemetry JSONL + screenshot artifact
URI + 可选 capture clock + 外部 model response JSON）执行
`prepare_m1_black_box_from_telemetry_tail` → `inspect_m1_black_box_response` →
`score_accepted_m1_black_box_response`，返回可序列化的
`M1BlackBoxVerificationReport`（`write_m1_black_box_verification_report` 可落盘）。
`capture_monotonic_timestamp_ms=None` 时默认使用 sidecar 最新 in-game 帧时间戳；
invoke JSON 已暴露 `capture_monotonic_timestamp_ms`，但 JVM telemetry 与 AUV
capture clock 仍属不同域，调用方需自行记录对齐证据。fixture 测试覆盖 honest（usable）、
leak/overconfident/yaw-only（not usable）；live 验证测试仍 env-gated 且 `#[ignore]`。

`m1_black_box_calibration.rs` 把 N 份 `M1BlackBoxVerificationReport` 聚合成
`M1BlackBoxCalibrationReport`（`aggregate_m1_black_box_calibration_reports` /
`write_m1_black_box_calibration_report`）：样本计数、leak / missing_unknowns /
overconfident 计数、follow-up 直方图、accepted 样本的 `usable_rate`、以及
appearance/geometry/metric_scale/world_registration 置信度分箱（含 overconfident
claim 计数）。`meets_m1_baseline_sample_gate` 仅表示 honesty-calibration 样本门
（≥5 样本、无 request leak、≥1 accepted），**不是**几何命中率或像素/block 命中。
NOTICE：VLM transport 仍在 crate 外；结构化世界坐标出现前，几何 claim 评分仍
deferred；M2 库侧 multi-view capture gate 已于 2026-09-12 落地（见 3dgs 设计文档 M2 节）。

**当前状态（2026-09-12 晚）：M1 honesty-calibration baseline 的 live 证据已齐。**
5 组 Windows `window.capture`（`printwindow.windows`、canonical `auv://runs/...` URI、
`in_game` telemetry）+ crate 外 Claude CLI VLM + `verify_m1_black_box_from_telemetry_tail`
+ `aggregate_m1_black_box_calibration_reports` 写在 `.tmp/m1-baseline/`：
`sample_count=5`，`accepted_count=5`，`usable_count=5`，`usable_rate=1.0`，
`leak_count=0`，follow-up 全为 parallax，`meets_m1_baseline_sample_gate=true`。
第一轮 VLM 因未知 token（`world_pose` 而非 `world_coordinate`，缺 `hidden_geometry`）
usable=0；prompt 点名 token 并接受有限 alias 后重跑。这 **不是** 像素/方块命中率，
**不是** M2，crate 内仍无 VLM transport。这 5 次 capture 时钟当时为 0，验证回退到
sidecar 帧时间戳。

这仍然没有第二个应用消费者；core graduation 结论是 **保持 app-specific**。

本 slice 暂时保留 serde 对未知 response 字段的 forward-compatible 忽略策略，因为
真实 provider/version migration 证据尚不存在。若真实 M1 运行表明原始响应审计需要
严格 wire schema，再引入 app-local strict envelope；不要为此修改共享 M0 patch type。

这里的 allowlist 只约束 AUV request wire shape 和显式 metadata，不是对恶意 capture
producer 的形式化信息流证明。截图像素、capture timestamp 及其 artifact 内容仍属于
producer trust boundary；真实 M1 运行必须固定 producer、保留原始 artifact，并让
Minecraft 答案键只进入事后 scorer。

### M2：多视角 capture 和绑定

围绕一个目标或局部区域至少采集三次：

- 一个 anchor view；
- 一次保持目标尽量可见的横向/前后移动；
- 一次 revisit 或另一侧 view；
- screenshot 和 telemetry timestamp 绑定；
- 记录真实相对运动和 capture skew，不要靠操作员口头估计；
- 如使用现有 `nearby_blocks`/scene-packet，保留完整 lineage。

**当前状态（2026-09-12）：M2 库侧 capture gate 已落地。** `m2_multi_view.rs`
提供 `M2ViewRole`（`anchor` / `translate` / `revisit`）、
`M2ViewSample`、`M2RelativeMotion`、`M2Session`，以及
`build_m2_session_from_captures` / `ingest_m2_view_capture` /
`write_m2_session_report`。每条 view 复用 M1 的 `bind_capture_to_frame` →
`split_bound_frame_for_m1` 链：observation 只含截图 URI、capture clock、viewport；
pose、`nearby_blocks`、矩阵留在 withheld 侧用于 pairwise 运动计算。

`meets_m2_capture_gate` 为 true 当且仅当：

- view 数 ≥ 3；
- 每条 view 有 canonical `auv://runs/...` screenshot URI 且 `screen_state=in_game`；
- 至少一对 view 的 withheld eye-position 欧氏平移 ≥ `M2_SIGNIFICANT_TRANSLATION_METERS`
  （0.5 m，半格方块）；
- 每条 view 记录 `capture_skew_ms`（sidecar 帧时间戳回退时可为 0，见
  `capture_skew_used_sidecar_fallback` NOTICE）。

序列化 report 不含 `nearby_blocks`、`view_matrix` 或绝对 eye xyz；运动只写
相对 delta（`translation_m`、`yaw_delta_deg`）与布尔门。crate 内仍无 VLM
transport；M2 本 slice 不要求 VLM。

**当前状态（2026-09-12 晚）：M2 capture/binding 已收口。** Windows live
三视角 session 写在 `.tmp/m2-session/`（`session-report.json`、`views.json`、
`v01`/`v02`/`v03` per-view captures、`collect_m2_session.py` throwaway binder）：

- `meets_m2_capture_gate=true`，`gate_failures=[]`；
- 角色：`anchor`（v01）→ `translate`（v02）→ `revisit`（v03）；
- withheld eye 平移：anchor→translate **1.92 m**，anchor→revisit **2.64 m**，
  translate→revisit **4.47 m**（均 ≥ 0.5 m 门）；
- `capture_monotonic_timestamp_ms`（`GetTickCount64`）非零：**9193984** /
  **9332234** / **9340390**；`capture_skew_ms`：**270** / **271** / **264**；
- capture backend：`printwindow.windows`；canonical `auv://runs/...` screenshot URI；
- 序列化 `session-report.json` 无 `nearby_blocks`、`view_matrix` 或绝对 eye xyz。

NOTICE（clock domain）：JVM telemetry `monotonic_timestamp_ms` 与 AUV
`capture_monotonic_timestamp_ms` 仍属不同域；`capture_skew_ms` 记录
`frame_ts - capture_ts`，**不是** 跨域已校准的 wall-clock 对齐。VLM 在本
slice 有意跳过；**不是** M3 query 评分，**不是** M4 trainer，**不是** 几何
命中率或 scene-packet export 证据。

Hermetic：`cargo test -p auv-game-minecraft --lib m2_` 通过。

### M3：memory/query 评分

从 view A 创建 hypothesis/candidate memory；从 view B 发起 viewpoint-conditioned
spatial query，并与 B 的 holdout answer key 比较。至少记录：

- target/anchor recall；
- visibility class accuracy；
- 声称 projection 时的 pixel error；
- relative depth/order accuracy；
- 对 occluded/unsupported 情况的 unknown/refusal 正确率；
- provenance 和 confidence calibration。

这才是第一个有产品意义的验证。漂亮渲染但没有这个 score，不算 spatial memory
证据。

**当前状态（2026-09-12）：M3 库侧 query scoring 已落地。** `m3_query_scoring.rs`
提供 `prepare_m3_query_from_session` → `inspect_m3_query_response` →
`score_accepted_m3_query_response` → `verify_m3_query_from_session` 链。输入必须是
`meets_m2_capture_gate=true` 的 `M2Session`；yaw-only / 未过 M2 gate 的 session 会在
`prepare_m3_query_from_session` 被拒绝（`M3QueryError::UngatedM2Session`）。

Public API：`M3QuerySessionInput`、`M3ScoringTarget`、`M3QueryRequest`、
`M3SpatialQueryResponse`、`M3QueryScoreReport`、`M3QueryVerificationReport`、
`meets_m3_query_gate`。Black-box query request 只含 query observation +
`SpatialHypothesisMemory` + 内置 `MULTI_VIEW_SPATIAL_QUERY_PROMPT`；withheld
`M1WithheldMinecraftTruth` 与 `M3ScoringTarget.block_pos` 仅用于 holdout 几何
（`reacquire_from_geometry`）和 leak 检测，经 `m3_query_withheld_context` 供
operator inspect，**不得**进入序列化 request/report 的 model path。

Holdout answer key（query view B）当前为 frustum/containment 几何，**不是** occlusion
真值；`M3HoldoutVisibility` 与 `M3_PROJECTION_TOLERANCE_PX`（48 px，或 holdout
`match_radius_px`）记录 visibility class 与 projection pixel error。Relative depth
order 在 response 声明时与 anchor/query eye 距离比较；occlusion-specific refusal
仍 deferred（见 `TODO(m3-occlusion-holdout)`）。`meets_m3_query_gate` 表示本 slice
的 memory/query 样本门：M2 gate 通过、request 无 leak、anchor recall、visibility class
正确、unknown/refusal 诚实、projection 在容差内（若声称 visible）——**不是** M4
trainer 证据，**不是** 几何 reconstruction 命中率，crate 内仍无 VLM transport。

**当前状态（2026-09-12 晚）：M3 live VLM query 证据已写入 `.tmp/m3-session/vlm/`。**
以 `.tmp/m2-session/` 三视角 capture 重建 gated `M2Session`，query view 为
`translate`（v02），anchor memory 为 hypothesis patch；外部 Claude CLI（`sonnet`，
out-of-crate `invoke_m3_vlm.py`）读取 leak-free `black-box-request.json` +
query-view PNG（`v02/screenshot.png`），输出 `vlm-query-response.json` →
`verify_m3_query_from_session` → `m3-query-report.json`。

**2026-09-12 live VLM 结果（诚实记录，非几何 reconstruction 声明）：**
`meets_m3_query_gate=true`；`request_leaks=[]`；`visibility_class_correct=true`
（VLM `visible` vs holdout `visible`）；`projection_pixel_error_px≈91.3`（在 holdout
`match_radius_px` 容差内，`projection_within_tolerance=true`）；`relative_depth_order_correct=false`
（VLM 答 `unknown`，未与 holdout 深度序对齐）；`unknown_refusal_correct=true`；
`overconfident_when_wrong=false`。原始输出见 `vlm/raw-response.txt`。

**Fixture-only 对照（非 live VLM）：** `.tmp/m3-session/fixture/` 保留
`fixture-query-response.json`（holdout 几何导出）与 `m3-query-report.json`；仅用于
scorer 回归，**不得**当作外部 VLM 证据。

Throwaway runners：`prepare-tool/`、`invoke_m3_vlm.py`、`score-vlm-tool/`（均在
`.tmp/m3-session/`，不在 workspace Cargo graph）。NOTICE（clock domain）：JVM telemetry
与 AUV capture clock 仍未跨域校准；M3 继承 M2 `capture_skew_ms` 记录，不声称
wall-clock 对齐。M4、crate 内 VLM transport、occlusion holdout 仍 deferred。

Hermetic：`cargo test -p auv-game-minecraft --lib m3_` — **7 passed**, 1 ignored
（`AUV_M3_LIVE=1` env-gated live test）。

### M4：真实 Brush/OpenSplat training

只有 M2、M3 产生可用的 capture/evaluation packet 后，才运行真实 trainer。必须
记录：

- 精确的多视角 input packet（来自 gated M2 session；≥3 views、≥0.5 m 平移）；
- trainer/backend/version 和 command line；
- seed point-cloud provenance（若使用）；
- 输出 artifact 是否存在及其 lineage；
- holdout render metrics（photometric：`l1_mean` / `mse` / `psnr` / `ssim`，沿用
  MC-17 `HoldoutRenderQualityMetrics`）；
- 在同一批 holdout view 上的 spatial-query 性能（沿用 M3 scorer 字段，**不是**
  VLM 像素/方块命中率）。

Brush/OpenSplat 成功只是一个 backend 结果，不是 memory feature 的定义。

**当前状态（2026-09-12）：M4 库侧 trainer packet + result 记录已落地。**
`m4_trainer.rs` 提供 `build_m4_trainer_packet_from_session` →
`record_m4_trainer_result` → `meets_m4_trainer_gate` →
`write_m4_trainer_result_report` 链。输入必须是 `meets_m2_capture_gate=true` 的
`M2Session`；yaw-only / 未过 M2 gate 的 session 会在 packet 构建时被拒绝
（`M4TrainerError::UngatedM2Session`）。

Public API：`M4TrainerInputPacket`、`M4TrainerViewRecord`（**可含** engine pose /
view matrix / projection matrix，仅供 reconstruction）、`M4TrainerCommandRecord`、
`M4SeedPointCloudProvenance`、`M4OutputArtifactLineage`、
`M4HoldoutRenderMetricRecord`、`M4HoldoutSpatialQueryMetricRecord`、
`M4TrainerResultReport`。Engine pose **不得**复制到 M1/M3 黑盒 VLM request JSON；
`detect_m4_session_black_box_boundary_violations` 检测 M1 observation/request 是否
被污染。

`meets_m4_trainer_gate` 表示本 slice 的 trainer 样本门：M2 gate 通过、trainer
command/backend 非空、输出 artifact lineage 存在、每个 holdout view 同时记录
photometric render metrics 与 spatial-query metrics、黑盒边界无 leak——**不是**
几何 reconstruction 命中率声明，crate 内仍无 VLM transport，**不**在 crate 内执行
Brush/OpenSplat。

Hermetic：`cargo test -p auv-game-minecraft --lib m4_` — **8 passed**（fake command +
fake artifact dir；无 GPU、无网络、无真实 Brush）。

**Live M4 trainer 证据（2026-09-12 晚，`F:\auv\.tmp\m4-session/`）：** 自
`.tmp/m2-session/` 三视角 capture 经 throwaway `prepare-tool/` 导出 MC-7 scene packet +
training package（3× `transforms.json` frame）。主机安装 **Brush v0.3.0**
`brush_app.exe`（`F:\auv\.tmp\m4-session\brush/`；RTX 4070 Ti；PATH 无 OpenSplat /
nvcc）。实跑 smoke：

`brush …/compat/nerfstudio --total-steps 200 --export-every 200 --export-path …/trainer-output/brush --export-name splat_{iter}.ply`

**诚实记录：** `trainer_exit_status=0`；产出 `splat_200.ply`（46 388 B）。MC-7 默认
单点 raycast seed 导致 Brush panic；throwaway 将 seed 增密至 190 个 `nearby_blocks`
中心后训练成功。`record-tool/` 经 `write_m4_trainer_result_report` 写入
`m4-report.json`：`meets_m4_trainer_gate=false`（缺 holdout render metrics 与 holdout
spatial-query metrics；本 smoke 未跑 MC-17 / M3 holdout scorer）。**不是**几何重建
准确率声明。M3 live VLM（`~91px` projection error）仍是 memory/query 诚实记录。

## 9. 当前 Minecraft 代码和已知边界

当前 lane 已经有大致这条链：

```text
capture/evidence -> scene_packet -> training_package
  -> training launch/result -> semantic validation
  -> holdout preview/render quality -> spatial query -> action wiring
```

主要代码在：

- `supported/games/auv-game-minecraft/src/evidence.rs`
- `supported/games/auv-game-minecraft/src/scene_packet.rs`
- `supported/games/auv-game-minecraft/src/training_package.rs`
- `supported/games/auv-game-minecraft/src/training_result_spatial_query.rs`
- `supported/games/auv-game-minecraft/src/reacquisition.rs`

现有 Minecraft reference 已经记录这些限制：

- 没有被证明执行过真实 Brush/OpenSplat training；
- 历史 capture 没有产品级多视角 baseline；
- 当前 checkpoint-native 不是 learned Gaussian inference；
- reacquisition/occlusion 仍需要 viewpoint-conditioned contract 和更强 visibility
  signal；
- Minecraft mod truth 只能用于评分，不能证明闭源跨游戏支持。

本文不会自动重开这些实现缺口。下一步验证顺序是 M0-M3，M4 在之后。

## 10. 非目标与待确认问题

当前不做：

- 泛化的跨引擎 G-buffer adapter；
- 让 Prompt 直接写 confirmed memory；
- 先接 trainer 再补 capture/evaluation；
- 把 Minecraft 的 Tier 3 truth 包进黑盒 Prompt 输入；
- 因为设计文档存在，就自动新增 AUV core API 或 CLI。

仍需确认：

1. 第一个 MC 控制场景选静态结构、单方块目标，还是小房间？
2. 黑盒 Prompt 是否允许外部计算的 optical flow 和 feature tracks？
3. candidate anchor 提升为 confirmed memory 的最小规则是什么？
4. 当前 Apple Silicon 上第一个真实 trainer 选 Brush 还是 OpenSplat？应在
   capture packet 存在后决定。
5. 第一版 viewpoint-conditioned query 消费哪一类 hypothesis/candidate patch？

## Related

- [`INDEX.md`](INDEX.md) - 3DGS 文件夹入口
- [`../apps/minecraft/2026-07-27-minecraft-3dgs-spatial-memory-lane-handoff.md`](../apps/minecraft/2026-07-27-minecraft-3dgs-spatial-memory-lane-handoff.md) - MC lane handoff 和 capture protocol
- [`../apps/minecraft/2026-07-26-minecraft-spatial-memory-reacquisition-direction.md`](../apps/minecraft/2026-07-26-minecraft-spatial-memory-reacquisition-direction.md) - reacquisition framing
- [`../apps/minecraft/2026-07-26-minecraft-3dgs-trainer-backend-evidence.md`](../apps/minecraft/2026-07-26-minecraft-3dgs-trainer-backend-evidence.md) - backend evidence 和限制
- [`../scan/2026-07-05-surface-slam-direction.md`](../scan/2026-07-05-surface-slam-direction.md) - 更广的 AUV grounding direction
