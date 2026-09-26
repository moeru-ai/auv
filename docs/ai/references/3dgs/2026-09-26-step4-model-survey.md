# Step 4 模型可用性调研报告（Model Availability Survey）

> 日期：2026-09-26  
> 任务：Step 4 视觉感知管线（YOLO-World 2D 检测 + 单目深度估计 + 反投影）前置调研（T0）  
> 状态：调研完成，所有关键模型均已实测下载并经 Rust `ort` 引擎加载验证。

---

## 1. 调研背景与原则

根据 Step 4 执行 Brief，在实现全图视觉感知前，必须对所需视觉与深度模型的可用性、来源、输入输出规格、许可证以及 Rust 端推理环境进行严谨调研。
原则：**若关键 ONNX 权重或运行环境无法获取，立即停止并向用户汇报，坚决不编造轻量级伪实现。**

本报告记录调研实测数据与验证结果。

---

## 2. 2D 检测模型：YOLO-World ONNX

### 2.1 候选来源与选用方案

YOLO-World 为开放词表（Open-Vocabulary）目标检测模型。经调查，官方/开源社区提供两种 ONNX 结构：

1. **动态文本嵌入架构（推荐）**：
   - **来源**：Hugging Face `Instemic/yolo-world-onnx`（基于 Ultralytics YOLOv8-World v2 官方权重导出）
   - **模型文件**：`yolov8s-worldv2.onnx`
   - **下载 URL**：`https://huggingface.co/Instemic/yolo-world-onnx/resolve/main/yolov8s-worldv2.onnx`
   - **文件大小**：51,142,204 字节（48.77 MB）
   - **参数量**：12.7M
   - **许可证**：AGPL-3.0（继承自 Ultralytics 官方权重）

2. **输入/输出规格（经 Rust `ort` 实测验证）**：
   - **Input 0**：`images`
     - 形状：`[batch, 3, height, width]`（动态尺寸，推荐 imgsz=640）
     - 数据类型：`Float32`
     - 归一化：`[0.0, 1.0]`，RGB 顺序
   - **Input 1**：`txt_feats`
     - 形状：`[batch, num_classes, 512]`（动态类别数量）
     - 数据类型：`Float32`
     - 含义：标准 CLIP ViT-B/32（`openai/clip-vit-base-patch32`）文本投影嵌入向量（经 L2 归一化）
   - **Output 0**：`output0`
     - 形状：`[batch, num_classes + 4, num_anchors]`
     - 格式：4 个边框回归值 `[cx, cy, w, h]` + `num_classes` 个类别的分类 logits

3. **文本 Prompt 编码策略**：
   - 目标类别集合（Brief 规定）：`["chest", "furnace", "crafting table", "door", "bed", "torch", "tree", "sheep", "pig", "cow"]`（10 个类别）。
   - **方案 A（离线固化嵌入）**：将预计算的 10 个类别的 $10 \times 512$ Float32 矩阵（仅 20 KB）以静态常量数组内嵌于 Rust crate 中，推理时无需额外加载 250MB 的 CLIP 文本模型，零额外推理开销。
   - **方案 B（在线动态嵌入）**：若后续需要任意文本 prompt，可引入 `clip-vit-base-patch32-text.onnx`（HuggingFace 可下，254MB）。

---

## 3. 单目深度估计模型：MiDaS & Depth Anything V2

我们对两款主流单目深度估计模型进行了全链路下载与 Rust `ort` 会话测试：

### 3.1 方案 A：Intel ISL MiDaS v2.1 Small（基线方案）

- **来源**：Intel Intelligent Systems Lab 官方 GitHub Release
- **模型文件**：`model-small.onnx`
- **下载 URL**：`https://github.com/isl-org/MiDaS/releases/download/v2_1/model-small.onnx`
- **文件大小**：66,764,249 字节（63.67 MB）
- **许可证**：MIT License
- **输入规格**：
  - Name：`'0'`
  - 形状：`[1, 3, 256, 256]`（固定尺寸）
  - 数据类型：`Float32`
  - 归一化：RGB，按 ImageNet 均值 `[0.485, 0.456, 0.406]` 与标准差 `[0.229, 0.224, 0.225]` 标准化
- **输出规格**：
  - Name：`'797'`
  - 形状：`[1, 256, 256]`，`Float32` 逆相对深度图（inverse relative depth）
- **特点**：轻量级、推理极快、在 CPU 上仅需几十毫秒。

### 3.2 方案 B：Depth Anything V2 Small（高精度方案）

- **来源**：Hugging Face `onnx-community/depth-anything-v2-small`
- **模型文件**：`depth_anything_v2_vits.onnx`
- **下载 URL**：`https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx`
- **文件大小**：99,060,839 字节（94.47 MB）
- **许可证**：Apache-2.0
- **输入规格**：
  - Name：`'pixel_values'`
  - 形状：`[batch, 3, height, width]`（动态尺寸，要求尺寸必须为 patch size 14 的倍数，如 518x518）
  - 数据类型：`Float32`
- **输出规格**：
  - Name：`'predicted_depth'`
  - 形状：`[batch, 14*floor(H/14), 14*floor(W/14)]`，`Float32` 相对深度图
- **特点**：对于复杂场景和边缘保持表现极佳。

---

## 4. Rust 推理端与运行环境评估

### 4.1 现有 Workspace 依赖状态

AUV workspace 中已有两个完善的推理 backend crates：
1. `crates/auv-inference-ort`：
   - 依赖：`ort = { version = "2.0.0-rc.12" }`（实际锁版本为 `2.0.0-rc.13`）
   - Features：开启了 `ort/download-binaries`、`ort/copy-dylibs`、`ort/api-24`
   - 实测：在 Windows 11 环境下首次编译自动下载官方 ONNX Runtime 动态链接库（`onnxruntime.dll`），`cargo check` 与 `cargo run` 顺利通过，无需手动安装系统级 ORT。
2. `crates/auv-inference-ultralytics`：
   - 依赖：`ultralytics-inference = "0.0.18"`
   - 针对标准 YOLO 导出 ONNX 提供了开箱即用的推理封装。

### 4.2 本机硬件资源与显存/内存需求

- **GPU**：NVIDIA GeForce RTX 4070 Ti（12GB GDDR6X 显存，驱动 CUDA 13.4）
- **CPU**：AMD 现代多核处理器
- **模型开销评估**：
  - YOLOv8s-World v2：~49 MB 权重，推理常驻内存 < 150 MB
  - MiDaS v2.1 Small：~64 MB 权重，推理常驻内存 < 120 MB
  - Depth Anything V2 Small：~95 MB 权重，推理常驻内存 < 200 MB
- **并发资源占用**：
  - 两个模型同时加载在内存中占用 < 350 MB，即使全部移入 GPU 显存占用也 < 500 MB，远低于 12GB 显存上限。
  - CPU Baseline 延迟评估：YOLO (~40ms) + MiDaS (~20ms) = 单帧离线流水线延迟 < 100ms，完全满足离线与近实时需求。

### 4.3 跨平台支持（Windows / Linux / macOS）

- `ort` 2.0 提供了针对 Windows（x64 DirectML / CUDA / CPU）、Linux（x64 CPU / CUDA）、macOS（ARM64 / CoreML）的自动二进制分发（`download-binaries`）。
- 模型文件为标准 ONNX（Opset 14-18），无专有平台自定义算子，具备完整的跨平台移植性。

---

## 5. 结论与执行建议

1. **可用性判定**：**PASS（完全可用）**。所需关键模型均已通过网络成功下载，且通过 Rust `ort` 实际加载验证了 Tensor 签名。
2. **Step 4 推进路线**：
   - **T1（2D 检测）**：采用 `yolov8s-worldv2.onnx`，针对 10 个预设 Minecraft 类别注入文本向量，对 v01 截图执行全图 2D 检测。
   - **T2（深度估计）**：优先采用 `model-small.onnx`（MiDaS v2.1）作为确定性轻量标定基线（亦兼容 `depth_anything_v2_vits.onnx`），通过 crosshair raycast 真值计算尺度因子完成绝对深度标定。
   - **T3（反投影）**：结合相机参数由 2D 目标中心像素反投影生成 3D 浮点坐标。
   - **T4（Ingest 接入）**：实现 `LandmarkIngest` 并注入 `SpatialMemoryStore`。
