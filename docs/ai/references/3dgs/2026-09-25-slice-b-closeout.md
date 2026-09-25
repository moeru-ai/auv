# 3DGS 线诚实 Closeout（2026-09-25）

## 结论

**3DGS 模型记住了训练视角，但无法泛化到未见视角。** 2–3 稀疏 Minecraft 视角下，
Brush 训练呈现典型过拟合：训练视角 PSNR 随步数单调上升，未见视角 PSNR 随步数
单调下降。没有证据支持"3DGS 可作为空间记忆模块合成新视角"。

## 证据

### M2 采集 gate：true
真实会话 F:/auv/.tmp/m2-session（2026-09-12）经代码重放验证；三视角位移
1.92m / 2.64m / 4.47m，非纯旋转。

### 训练视角重建（3 视角全训练，5000 步）
splat_1000.ply→splat_5000.ply：53KB→177KB（持续 densification = 真实训练）。
frame_2 PSNR 14.28→19.72dB / SSIM 0.525→0.568；
frame_3 PSNR 15.26→18.85dB / SSIM 0.467→0.514。
单调提升但绝对值差（好 3DGS 应 25+/0.8+），5000 步未收敛。

### 真 holdout（训练 frame 1+2，frame 3=revisit 全程未见，5000 步）
holdout_1000.ply→holdout_5000.ply：48KB→144KB（真实训练）。
未见 frame_3：PSNR 10.41→7.54dB / SSIM 0.357→0.244，随训练单调下降。
对照（同一 frame_3，同样 5000 步）：见过 18.85dB vs 未见 7.54dB，差 11.3dB。

### 测量方法
仓库内 Slice A Rust PSNR + 纯 Rust 8x8 box-window SSIM（compare_png_pair），
直接对比 Brush --eval-save-to-disk 渲染与 GT，无 resize/crop，对尺寸不匹配报错。
旧 holdout-metrics.json（PSNR~14/SSIM~0.54）是训练视角内比较，不是泛化证据。

## 已知限制
- 仅 3 视角；holdout 训练集仅 2 视角（极度稀疏）。
- 种子点云 points3d.ply（190 点灰色稀疏网格）provenance 不清，不含颜色信息，
  已披露。
- Minecraft 低纹理场景；Brush 0.3.0 默认超参。

## 决定：暂停
否定性结果即交付物。保留 M2 采集 / Brush 接入 / 测量工具等基础设施，
不继续投"记忆模块"方向。继续的唯一前提是有人投入做 20+ 视角稠密采集，
那是一个新实验。
