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
碰撞边界、世界坐标或可交互性。

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

### M1：黑盒单视角 baseline

模型只接收：

- Minecraft screenshot；
- capture time、viewport、window metadata；
- 可用的 input history；
- 不包含 `nearby_blocks`、block coordinates、raycast hit、camera matrices 或
  hidden telemetry。

事后用 Minecraft telemetry/world state 给 claim 打分，测量 Prompt 在不作弊时
能产生多有用的假设。

### M2：多视角 capture 和绑定

围绕一个目标或局部区域至少采集三次：

- 一个 anchor view；
- 一次保持目标尽量可见的横向/前后移动；
- 一次 revisit 或另一侧 view；
- screenshot 和 telemetry timestamp 绑定；
- 记录真实相对运动和 capture skew，不要靠操作员口头估计；
- 如使用现有 `nearby_blocks`/scene-packet，保留完整 lineage。

当前 Minecraft lane 已有 capture binding 和 scene-packet export 路径，但历史
capture 实际上接近单视角，也没有真实 trainer run。新的多 pose session 是
必须补的证据。

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

### M4：真实 Brush/OpenSplat training

只有 M2、M3 产生可用的 capture/evaluation packet 后，才运行真实 trainer。必须
记录：

- 精确的多视角输入 packet；
- trainer/backend/version 和 command；
- seed point-cloud provenance（如果使用）；
- 输出 artifact 是否存在及其 lineage；
- holdout render metrics；
- 在同一批 holdout view 上的 spatial-query 性能。

Brush/OpenSplat 成功只是一个 backend 结果，不是 memory feature 的定义。

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
