# 3DGS 线诚实 Closeout（2026-09-25）

## 结论（先说）

**3DGS 模型记住了训练视角，但无法泛化到未见视角。** 在 2–3 个稀疏 Minecraft
视角下，Brush 训练出现典型过拟合：训练视角 PSNR 随步数单调上升，未见视角
PSNR 随步数单调下降。没有证据支持"3DGS 可作为空间记忆模块合成新视角"
这一核心假设。

## 一手证据

### M2 采集 gate：✅ true

- 真实会话 `F:/auv/.tmp/m2-session`（2026-09-12，非合成）经代码重放验证。
- 三视角位移 1.92m / 2.64m / 4.47m，非纯旋转。
- 测试 `m2_live_session_gate.rs` 已提交（a0b36d57）。

### 实验一：训练视角重建（3 视角全训练，5000 步）

`splat_1000.ply`→`splat_5000.ply`：53KB→177KB（densification 持续发生 = 真实训练，非卡死）。

| step | frame_2 PSNR | frame_2 SSIM | frame_3 PSNR | frame_3 SSIM |
|------|-------------|-------------|-------------|-------------|
| 500  | 14.28 | 0.525 | 15.26 | 0.467 |
| 5000 | 19.72 | 0.568 | 18.85 | 0.514 |

- 单调提升 = 优化器在真实工作。
- 但绝对值很差（好的 3DGS 应为 PSNR 25+ / SSIM 0.8+），且 5000 步仍未收敛。

### 实验二：真 holdout（训练 frame 1+2，frame 3=revisit 全程未见，5000 步）

`holdout_1000.ply`→`holdout_5000.ply`：48KB→144KB（真实训练）。

| step | frame_3（未见）PSNR | frame_3（未见）SSIM |
|------|-------------------|-------------------|
| 500  | 10.41 | 0.357 |
| 5000 | 7.54  | 0.244 |

- **随训练步数单调下降** = 对训练视角过拟合，未见视角越训越差。
- 对照实验（同一视角 frame_3，同样 5000 步，唯一变量是是否参与训练）：
  - 见过：18.85 dB / 0.514
  - 未见：7.54 dB / 0.244
  - **差距 11.3 dB** —— 模型没有学到可迁移的三维结构。

### 测量方法（诚实性）

- 用仓库内 Slice A 的 Rust PSNR + 纯 Rust 8×8 box-window SSIM（`compare_png_pair`，
  已提交 50753bb3），直接对比 Brush `--eval-save-to-disk` 输出的渲染图与 GT。
- 无 resize、无 crop、无对齐；尺寸不匹配直接报错而非输出 partial 指标。
- 旧 `holdout-metrics.json`（PSNR~14/SSIM~0.54）是训练视角内比较，**不是**泛化证据，
  已明确区分，不再引用。

## 已知限制

- 只有 3 个视角；holdout 实验训练集仅 2 个视角（极度稀疏）。
- 种子点云 `points3d.ply`（190 点灰色稀疏网格）的 provenance 不清，与当前代码的
  "每帧一个 raycast hit" 逻辑对不上；它不含颜色信息，作为几何先验复用，
  已在分析中披露。
- Minecraft 场景（低纹理、几何简单）；Brush 0.3.0 默认超参。
- 未尝试 20+ 视角的稠密采集（那是另一个量级的投入）。

## 建议

**暂停 3DGS-as-memory 这条线**，保留基础设施。理由：

1. 否定性结果本身是交付物：2–3 稀疏视角下 3DGS 不泛化，这是一个诚实的、可复现的结论。
2. M2 采集、Brush 接入、诚实测量工具都是真实资产，可复用。
3. 在没有"稠密视角下泛化出现"的证据之前，继续投"记忆模块"方向不具合理性。

继续的唯一合理前提：有人愿意投入做 20+ 视角稠密采集，验证泛化是否随视角数出现。
那是一个新的实验，不是对当前结果的延续。

## 废物清理记录（2026-09-25）

为防止后来 agent 被误导，已清理以下"验证出的废物"：

- `F:/auv/.tmp/m4-session/m4-report.json`（曾宣称 `meets_m4_trainer_gate=true`，
  但其 holdout 证据实为训练视角内比较）→ 重命名为
  `m4-report-SUPERSEDED-2026-09-25.json`。
- `F:/auv/.tmp/m4-session/holdout-metrics.json`（PSNR~14/SSIM~0.54，文件名暗示泛化）
  → 重命名为 `in-training-view-metrics-NOT-unseen-generalization.json`。
  （文件内的 honesty 块本就诚实，改名让文件名与内容一致。）
- 新增 `F:/auv/.tmp/m4-session/SUPERSEDED-NOTICE.md`，说明上述文件已被本 closeout 取代，
  警告不得用作"3DGS 有效"的证据。
- 仓库内历史设计文档（2026-08-03、2026-07-27）均含明确 NOTICE 说明 holdout 视角
  在训练集内，属诚实历史记录，未改动。

## 产物位置清单

本地持久化产物（均在 `.tmp/` 下，未提交大文件至版本库）：

- 真实 M2 会话：`F:/auv/.tmp/m2-session`（2026-09-12 真实会话，1.28 MB / 1,337,958 字节，20 文件）
- Train-view 训练输出：`F:/auv/.tmp/sliceb-training`（splat_*.ply + eval_*/，5.95 MB / 6,239,578 字节，27 文件）
- Holdout 训练输出：`F:/auv/.tmp/sliceb-holdout-training`（holdout_*.ply + eval_*/，3.59 MB / 3,769,512 字节，17 文件）
- Holdout package：`F:/auv/.tmp/sliceb-holdout`（2 训练 + 1 val，1.19 MB / 1,248,887 字节，8 文件）
- 原始 3 视角训练包：`F:/auv/.tmp/m4-session/training-package`（2.40 MB / 2,521,147 字节，18 文件）
- 远端：`origin/3dgs-research`（含 M2 replay 测试、诚实测量工具）
