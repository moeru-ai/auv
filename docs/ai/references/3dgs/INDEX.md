# 3DGS 空间记忆

本文件夹负责视觉观测、空间假设、3DGS 外观重建和已确认空间记忆之间的设计边界。

状态：**当前设计基线**。这份设计本身不等于批准实现。第一验证环境是 Minecraft，
因为它可以通过现有 telemetry/mod 路径提供答案键，同时仍然可以把模型输入限制在
闭源黑盒观测范围内。

## 重点入口

> **先读这份文档。**
>
> [`2026-08-03-3dgs-spatial-memory-observation-design.md`](2026-08-03-3dgs-spatial-memory-observation-design.md)
>
> 这份文档记录闭源游戏、遥感类比、内置单视角 Prompt、记忆写入边界和 Minecraft
> 验证顺序的当前决定。在边界和证据 gate 被接受前，不要先写 trainer wrapper。

## 当前方向

- 闭源游戏通过黑盒观测层支持：截图、时间、窗口 metadata、输入历史，以及从多次
  观测中推导出的信号。
- 单视角 Prompt 可以生成空间假设并请求后续 capture，但不能只凭一张截图把 claim
  标成 confirmed。
- 3DGS 是外观/重投影 backend，不是空间记忆的定义。没有训练 splat 时，记忆 contract
  也必须能表达查询、未知和失败。
- Minecraft 是答案键 gym。它的 telemetry 和世界坐标只用于评分，不能在 vision-only
  实验中意外泄漏进黑盒 Prompt 输入。

## 相关文档

- [`../apps/minecraft/INDEX.md`](../apps/minecraft/INDEX.md) - Minecraft vertical 历史和当前 3DGS lane 记录
- [`../apps/minecraft/2026-07-27-minecraft-3dgs-spatial-memory-lane-handoff.md`](../apps/minecraft/2026-07-27-minecraft-3dgs-spatial-memory-lane-handoff.md) - 已知 capture 和 reacquisition 限制
- [`../apps/minecraft/2026-07-26-minecraft-3dgs-trainer-backend-evidence.md`](../apps/minecraft/2026-07-26-minecraft-3dgs-trainer-backend-evidence.md) - trainer 可达性证据；未宣称真实 trainer 已运行
- [`../scan/2026-07-05-surface-slam-direction.md`](../scan/2026-07-05-surface-slam-direction.md) - viewpoint-conditioned spatial grounding 方向

## 维护规则

1. 当前设计决定和开放问题放在重点设计基线中。
2. 只有 command、capture 或 fixture 结果可复现时，才新增 evidence/validation note。
3. 没有独立黑盒验证时，不要把 Minecraft 答案键结果说成跨引擎支持结论。
