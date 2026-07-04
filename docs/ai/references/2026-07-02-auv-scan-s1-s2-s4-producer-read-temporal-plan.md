# AUV Scan S1：Slice 2–4 工程实施计划（Producer → Read → Temporal）

**Date:** 2026-07-02  
**Status:** implementation plan — **S1-2 / S1-3 / S1-4a / S1-4b landed** on `main`; S1-4c+ N-frame timeline still **blocked** (see [S-line graduation review](2026-07-04-auv-s-line-graduation-review.md))
**Companion spec:** [GAN implementation spec](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md)（产品目标、评估 rubric、风险登记 — 本文档侧重工程切片清单）  
**Prerequisite:** [S0 charter](2026-07-02-auv-scan-s0-charter.md)、[S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md)、[S1 Slice 1 handoff](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md)（`scan-frame-v0` 已落地）  
**Owner 重排:** 原 S1 step 2（motion）延后；顺序为 **S1-2 producer → S1-3 read-side → S1-4 multi-frame 大纲**。

## 一句话

Slice 1 已锁 `scan-frame-v0`；本计划不再扩契约。在 **`crates/auv-scan` 内**完成 **fixture-first hermetic producer → crate 内 inspect reader →（稳定后）多帧**，**拒绝** `scroll_scan` 耦合、多源 trait、runtime/viewer 大改。

## 全局依赖

```text
[S1 Slice 1 DONE]  scan-frame-v0 + write/read_frame_artifact + single_frame_v0 fixture
        │
        ▼
[S1-2]  produce_frame_from_fixture_dir (+ 可选 live capture 映射，feature gate)
        │
        ▼
[S1-3]  load_scan_frames_from_dir + 字段断言 + PNG 尺寸交叉校验（无大 UI）
        │
        ▼
[S1-4]  大纲 only — 多帧序列、timeline/motion 子切片（blocked）
```

| 切片 | 阻塞于 |
| --- | --- |
| S1-2 | Slice 1 基线可用 |
| S1-3 | S1-2 能在 artifact 目录产出合规 `scan-frame-0001.json` + PNG |
| S1-4 | S1-2 + S1-3 handoff 合并且 hermetic 全绿 |

---

# S1-2：Producer Wiring（单帧产出）

## 概述

在 `auv-scan` 内补全 **最小真实 producer**：输入为 fixture 目录（**merge gate**）或可选 `auv_driver::Capture`（**live，非 gate**），输出 artifact 目录内的 `scan-frame-NNNN.json` 与 `image.file_name` 指向的 PNG 兄弟文件。与 [GAN spec Option A](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#recommended-producer-candidate含权衡) 对齐。

### Owner 审查卡点（S1-2 开代码前锁定）

以下五条为 **merge 前硬边界**；实现不得临场改写。

| # | 卡点 | 锁定决策 |
| --- | --- | --- |
| 1 | **同一路径产物** | fixture 与 live **必须**经同一套纯函数链：`build_scan_frame`（或等价）→ `write_frame_with_image` → `write_frame_artifact`。**禁止**第二条并行 serializer / 独立 JSON 手写路径。 |
| 2 | **失败语义** | **Fail-closed，不落 degraded artifact。** 任一步失败 → **目录内不得出现** 部分 `scan-frame-*.json` 或孤儿 PNG；已写文件须回滚删除或写临时目录后原子 rename（handoff 固定一种）。**不**引入 `degraded` / `partial` wire 变体。 |
| 3 | **Non-goals 写硬** | 见下文 §2；另 **明确不做**：`scroll_scan` 接线、viewer / `inspect_server`、multi-frame 批量、compare / cross-run diff、跨 crate shared abstraction。 |
| 4 | **验证口径可 merge** | **Merge gate = hermetic only**（`cargo test -p auv-scan`，无 `live-capture` feature）。Live `#[ignore]` + label `live`；**不得**阻塞 merge。 |
| 5 | **后续抽 shared 的触发条件** | **仅当** 第二个**真实** producer 也需要同一 artifact 落盘语义时，才允许从 `auv-scan` 往外抽 shared helper，且须 owner 点子切片。**S1-2 禁止**预建 shared crate / `FrameSource` trait。 |

**架构风险（写进 handoff）：** builder / serializer **纯函数**，**ownership 留在 `auv-scan`**。live 只做 `Capture` → `ScanFrame` 映射。日后抽取须有清晰 crate 边界，避免半生不熟的 shared 层。

---

### 1. Classification + Veto Checklist

| 项 | 值 |
| --- | --- |
| **Classification** | `owner-approved feature` |
| 混合行为变更与 packaging/move-only refactor？ | **否** |
| 混合 feature 完成与 helper 提取？ | **否** |
| 因「感觉该做了」重开暂停 lane？ | **否** |
| 未命名情况下同时改 runtime / inspect / MCP / proto？ | **否** |
| 引入无清晰归属的新抽象？ | **否**（无 `FrameSource` trait / registry） |
| 复制已有 type/contract 仅换皮？ | **否**（复用 [Slice 1 `ScanFrame`](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md#approved-wire-slice-1-only)） |
| 无显式 seam 的环境耦合行为？ | **否**（live 经 `live-capture` feature + `#[ignore]`） |
| 行为变更无回归测试计划？ | **否** |
| 顺手改无关杂乱代码？ | **否** |
| 声称「obvious next step」无 owner/evidence？ | **否**（owner 重排 + GAN spec） |

---

### 2. Non-goals（显式）

- 不扩展 `scan-frame-v0` wire（见 [Slice 1 handoff](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md)）
- **不**在 `scroll_scan` / `src/runtime.rs` / CLI 接线（**拒绝 scroll_scan 耦合**）
- 不做 motion、tracks、OCR fusion、diagnostics
- 不建 multi-source 抽象、platform 大重构、**跨 crate shared helper / trait registry**
- 不改 `run_read` / `inspect_server` / **viewer HTML**（→ S1-3 默认在 crate 内）
- 不做 `sequence_index > 0` 批量多帧（→ S1-4）
- **不做** inspect compare / cross-run diff API（→ [B2c deferred](2026-06-30-auv-scenebridge-b2c-inspect-cross-run-compare-deferred.md)）
- 不把 `build_frame_from_fixture` 提升为唯一 public API（须含 PNG 落盘路径）
- **不**写 degraded / partial artifact 作为失败回退

---

### 3. Producer 候选对比与推荐（ONE）

| 候选 | 结论 |
| --- | --- |
| **A. `auv-scan` 内 fixture producer + 可选 `frame_from_capture`（live feature）** | **✅ 选用** — hermetic gate + 共享 `ScanFrame` 构建；边界在 S-line crate |
| B. 仅公开 test-only `build_frame_from_fixture` | **拒绝** — 无 PNG 落盘，不满足「minimal real producer」 |
| C. `scroll_scan` observe 页截图 donor | **拒绝** — 强耦合 page 循环与 `ScrollScanArtifact`；仅作日后字段对照 |
| D. 新 CLI `auv scan capture-frame` | **Defer** — 非 S1-2 DoD |
| E. `auv-driver-macos` 直接写 artifact | **拒绝** — wire 归属错误；driver 只供 `Capture` |

**推荐（ONE）：Option A**

- **主路径（gate）：** `produce_frame_from_fixture_dir(fixture_dir, out_dir)` — 读 manifest + PNG，经 **共享** `build_scan_frame` + `write_frame_with_image` 落盘。
- **可选 live：** `frame_from_capture(&Capture, FrameCaptureMeta)` 仅负责像素→`ScanFrame`；落盘 **必须**调用与 fixture 相同的 `write_frame_with_image`（**同一路径**）。
- **Rationale：** 契约与 IO 同 crate；hermetic 与 live 共享构建逻辑；不牵动 `scroll_scan`；符合 GAN spec 铁律「先 hermetic，后 live」。

---

### 4. Owning crate / boundary

| 层 | 职责 |
| --- | --- |
| **`crates/auv-scan`** | 全部 producer API、`ProducedFrame` 结果类型、PNG+JSON 写入顺序 |
| **`crates/auv-scan/src/producer/`** | `mod.rs`（fixture + 共享逻辑）、`live.rs` 或 `driver.rs`（optional feature） |
| **`auv-driver`** | 仅 optional dep；提供 `Capture`，**不**写 `scan-frame` JSON |
| **`scroll_scan`** | **不修改**（donor 参考 only） |

---

### 5. Public API changes（尽量少）

与 [GAN spec API sketch](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#api-sketchprovisional--实现-slice-须-owner-批准符号名) 对齐；`lib.rs` re-export：

| 符号 | 角色 |
| --- | --- |
| `ProducedFrame` | `{ json_path, image_path, frame: ScanFrame }` |
| `FrameCaptureMeta` | `frame_id`, `sequence_index`, `captured_at_millis`, `window_bounds`, `viewport_bounds?` |
| `produce_frame_from_fixture_dir(fixture_dir, out_dir)` | Hermetic producer（gate） |
| `frame_from_capture(capture, meta)` | `Capture` → `ScanFrame`（内存 RGBA 可测） |
| `write_frame_with_image(dir, frame, image_bytes)` | 先 PNG 后 JSON；缺失图像 → 确定性 error variant |
| `bounds_to_scan_bounds(...)` | 单一 f64 `Rect` → `ScanBounds` 舍入函数（live 路径共用） |
| `produce_frame_from_window_capture(...)` | optional feature only |

**不新增：** generic trait、`FrameSource` registry。  
**保持 test-only：** `build_frame_from_fixture`（不 re-export）。

---

### 6. Files / modules to touch

| 文件 | 动作 |
| --- | --- |
| `crates/auv-scan/src/producer/mod.rs` | **新增** — fixture producer、`write_frame_with_image`、共享构建 |
| `crates/auv-scan/src/producer/live.rs` | **新增** — `frame_from_capture`、`bounds_to_scan_bounds`（feature gate） |
| `crates/auv-scan/src/lib.rs` | `mod producer`; re-export |
| `crates/auv-scan/Cargo.toml` | `image` dep；optional `live-capture` → `auv-driver` |
| `crates/auv-scan/tests/fixtures/scan/temporal/producer_single_frame_v0/` | **可选** — 若与 `single_frame_v0` 复用则省略 |
| **不修改** | `src/scroll_scan/*`、`src/runtime.rs`、`src/run_read.rs`、`src/inspect.rs`、CLI |

**写入顺序（锁 R-03）：** 先写 PNG，再 `write_frame_artifact`；handoff 记录所选策略。

---

### 7. Hermetic tests（名称 + 断言）

| 测试名 | 断言 |
| --- | --- |
| `produce_frame_from_fixture_dir_matches_golden` | 输出 `read_frame_artifact` == [golden `scan-frame-0001.json`](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md#hermetic-fixture) |
| `produce_frame_from_fixture_dir_writes_png_sibling` | `out_dir` 含 `frame-0001.png`；`image.file_name` 可解析 |
| `write_frame_with_image_roundtrip` | 字节写入 → load 字段一致 |
| `frame_from_capture_builds_scan_frame_from_rgba` | 内存 `RgbaImage`/`Capture`，无 OS API |
| `bounds_to_scan_bounds_rounding_table` | 表驱动舍入边界 |
| `produce_frame_from_fixture_dir_rejects_missing_png` | `ScanArtifactError` 或 `ScanProducerError` **变体**；**out_dir 无残留** JSON/PNG |
| `produce_failure_leaves_no_partial_artifact` | 模拟 mid-write IO 失败 → 目标目录无 `scan-frame-*.json`、无孤儿 PNG |
| `frame_from_capture_rejects_zero_dimension` | `InvalidBounds` 或等价 variant |

**Live（非 gate）：** `produce_frame_from_window_capture_writes_artifact` — `#[ignore]` + label `live`。

---

### 8. Validation commands

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

Optional live:

```sh
cargo test -p auv-scan --features live-capture -- --ignored
```

---

### 9. Handoff requirements

实现完成后撰写：`docs/ai/references/2026-07-02-auv-scan-s1-slice2-producer-handoff.md`

须包含：

- 选用 Option A、拒绝 scroll_scan / driver-direct / CLI 的理由
- **Owner 审查卡点** 五条对照表（同一路径、fail-closed、non-goals、hermetic gate、shared 抽取触发条件）
- builder **纯函数** + ownership 在 `auv-scan` 的声明
- 稳定 public API 表（与上文一致或微调名）
- `live-capture` feature 与 default 构建矩阵
- PNG+JSON 写入顺序、错误变体表
- 测试名 + 断言摘要（对照 [GAN spec P1–P5](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#s1-2-success-metrics)）
- 验证命令块
- 声明 **S1-3 前置满足**：任意目录可含 S1-2 产出

---

### 10. Dependencies between slices

| 关系 | 说明 |
| --- | --- |
| **依赖 Slice 1** | `write_frame_artifact`、`ScanFrame`、`single_frame_v0` golden |
| **阻塞 S1-3** | S1-3 需要 S1-2 产出的 artifact 目录作读侧输入 |
| **不阻塞 S1-4** | S1-4 等待 S1-2 **与** S1-3 |

**建议实现顺序：** `write_frame_with_image` → `produce_frame_from_fixture_dir` → `frame_from_capture` → optional live → handoff。

---

# S1-3：Read-side Consume（crate 内 inspect reader）

## 概述

当磁盘已有 S1-2 产出的 `scan-frame-v0` artifact 时，提供 **稳定读侧聚合**：按目录 glob `scan-frame-*.json`，`read_frame_artifact` 解析，字段级断言，PNG 尺寸与 `ScanImageRef` 交叉校验。**无** viewer、**默认不**改 `run_read`（GAN spec：run 集成单列后续 slice）。

与 [GAN spec S1-3](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#sprint-s1-3-frame-inspect-reader可实施) 对齐；模式 **参考** `src/run_read.rs` scroll-scan 提取与 `auv-view` inspect，**实现落在 `auv-scan`**。

---

### 1. Classification + Veto Checklist

| 项 | 值 |
| --- | --- |
| **Classification** | `owner-approved feature` |
| 全部 CONTRIBUTING implementation veto 项 | **否** |

---

### 2. Non-goals（显式）

- 不扩展 `scan-frame-v0` wire
- 不新增 `inspect_server` 端点、viewer HTML、HTTP compare API
- **默认不**实现 `run_read::extract_scan_frames`（owner 点名才开子切片）
- 不回答 S0 跨帧问题（身份、视口运动、reacquire）
- 不做 `scan-timeline.json` / tracks / motion
- 不修改 `scroll_scan`、runtime、CLI
- 不渲染 PNG；仅尺寸/metadata 交叉校验

---

### 3. Producer 选择

本切片 **无新 producer**。消费 S1-2 写入的 artifact 目录（或显式路径列表）。

---

### 4. Owning crate / boundary

| 层 | 职责 |
| --- | --- |
| **`crates/auv-scan/src/inspect.rs`** 或 **`reader.rs`** | `load_scan_frames_from_dir`、`ScanFrameInspect`、断言 helper、`summarize_scan_frame_text` |
| **`crates/auv-scan`** | 读解析复用 [Slice 1 `read_frame_artifact`](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md#stable-public-api-auv_scan-crate-root-re-exports) |
| **`src/run_read.rs` / `src/inspect.rs`** | **默认不修改** |

---

### 5. Public API changes

| 符号 | 角色 |
| --- | --- |
| `ScanFrameInspect` | `{ frames: Vec<ScanFrame>, source_dir, loaded_paths }` — S1-3 测试 0–1 帧；形状预留 S1-4 |
| `load_scan_frames_from_dir(dir)` | glob `scan-frame-*.json`；按 `sequence_index` 排序；逐个 `read_frame_artifact` |
| `assert_frame_matches_expectation(frame, expectation)` 或 `FrameFieldExpectation` | 测试/Evaluator 字段断言 |
| `verify_image_dimensions(frame, dir)` | PNG 宽高 vs `image.width/height`；不一致 → 确定性 error variant |
| `summarize_scan_frame_text(frame) -> String` | 短文本摘要（`frame_id`、`sequence_index`、bounds）；非 UI |

**错误：** 扩展 `ScanArtifactError` 或新 `ScanInspectError`；测试断言 **variant**，非 `Display`。

---

### 6. Files / modules to touch

| 文件 | 动作 |
| --- | --- |
| `crates/auv-scan/src/inspect.rs`（或 `reader.rs`） | **新增** 读侧模块 |
| `crates/auv-scan/src/lib.rs` | re-export |
| `crates/auv-scan/Cargo.toml` | 若交叉校验需 `image::io` — 复用已有 `image` dep |
| **默认不修改** | `src/run_read.rs`、`src/inspect.rs`、`inspect_server/*`、viewer |

---

### 7. Hermetic tests（名称 + 断言）

| 测试名 | 断言 |
| --- | --- |
| `load_scan_frames_from_dir_reads_golden_directory` | 对 S1-2 / Slice 1 golden 目录 → `frames.len()==1`；字段与 manifest 一致 |
| `load_scan_frames_from_dir_sorts_by_sequence_index` | 为 S1-4 预埋；S1-3 可用单文件 |
| `verify_image_dimensions_matches_png` | 宽高一致 → Ok |
| `verify_image_dimensions_rejects_mismatch` | 故意改 wire height → 确定性 error variant |
| `load_scan_frames_from_dir_empty_dir_errors` | 空目录 → Err（或 `frames` 空 — handoff 须固定一种） |
| `load_scan_frames_from_dir_rejects_bad_schema` | 坏 `schema_version` → `SchemaMismatch` |
| `summarize_scan_frame_text_includes_key_fields` | 含 `frame_id`、`sequence_index`、bounds 摘要 |
| `producer_then_reader_roundtrip` | **集成：** `produce_frame_from_fixture_dir` → `load_scan_frames_from_dir` → 字段相等 |

**关键字段（最低）：** `schema_version`, `frame_id`, `sequence_index`, `captured_at_millis`, `window_bounds`, `image.file_name`, `image.width`, `image.height`。

---

### 8. Validation commands

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

---

### 9. Handoff requirements

`docs/ai/references/2026-07-02-auv-scan-s1-s3-read-side-handoff.md`

须包含：

- `ScanFrameInspect` / `load_scan_frames_from_dir` 签名
- 与 [Slice 1 read API](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md) 关系
- glob 规则、`sequence_index` 排序语义
- PNG 交叉校验错误变体表
- 测试名 + 断言（对照 [GAN spec R1–R4](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#s1-3-success-metrics)）
- 显式 **未做** `run_read` 集成的理由与后续 trigger
- 声明 **S1-4 前置满足**：单帧 produce→read 闭环稳定

---

### 10. Dependencies between slices

| 关系 | 说明 |
| --- | --- |
| **阻塞于 S1-2** | 读侧输入为 S1-2 产出目录；无 artifact 则无法验收 |
| **阻塞 S1-4** | 多帧读侧扩展依赖单帧 `load_scan_frames_from_dir` 行为稳定 |
| **参考 Slice 1** | `read_frame_artifact`、golden、`ScanArtifactError` 变体风格 |

---

# S1-4：Temporal / Multi-frame（大纲 only — blocked）

## 概述

在 S1-2 producer + S1-3 reader **单帧闭环** hermetic 全绿后，才批准多帧：有序 `scan-frame-0001..N`、读侧枚举、序列级测试。Motion / tracks / diagnostics 仍属 [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) 后续步骤，**不得**借 S1-4 一次性并入。详见 [GAN spec S1-4](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#sprint-s1-4-multi-frame-sequencefuture--本-spec-不批准实现)。

> **本切片仅规划；实现 agent 不得在本阶段写 motion/track wire 代码。**

---

### 1. Classification + Veto Checklist（规划态）

| 项 | 值 |
| --- | --- |
| **Classification** | `owner-approved feature`（启动时重新确认） |
| 全部 veto 项（规划时预判） | **否** — 若启动时混入 S1-2/3 代码或 scroll_scan 耦合则 **是** → 缩小切片 |

---

### 2. Non-goals（显式）

- 不扩展 `scan-frame-v0` 单帧字段
- 不在 S1-4 v0 引入 `ViewportTransform`、`TemporalTrack`、`ScanDiagnostic` wire
- 不耦合 `scroll_scan` 页循环作为 producer
- 不做 OCR fusion、ViewMemory、SceneBridge S3
- 不做 live 高滚动探针作为 merge gate
- 不做 3D / SLAM、inspect compare API
- 不建 multi-source producer 抽象

---

### 3. Producer 选择（规划）

| 方向 | 说明 |
| --- | --- |
| **首期** | 扩展 `produce_frame_from_fixture_dir` 为 **多帧 fixture 目录**（`two_frame_v0/`），按 `sequence_index` 写 `scan-frame-0001.json`、`scan-frame-0002.json` |
| **拒绝** | `scroll_scan` 多页 observe 作为 temporal producer |
| **后续子切片** | `scan-timeline-v0` motion artifact（S1-4b，独立批准） |

---

### 4. Owning crate / boundary（规划）

| 层 | 职责 |
| --- | --- |
| **`crates/auv-scan` producer** | 多帧 fixture 批量写出 |
| **`crates/auv-scan` inspect** | `load_scan_frames_from_dir` 返回 N 帧、单调 `sequence_index` |
| **`scroll_scan`** | 仅 **只读** donor（如 `ScreenshotDiffStability` 思路），不接线 |
| **新 schema** | motion/timeline 需 **新 slice** 新文件/版本，非 `scan-frame-v0` 扩展 |

---

### 5. Public API changes（规划 — 未批准）

| 方向 | 暂定 |
| --- | --- |
| `produce_frames_from_fixture_dir` | 多帧版 producer |
| `load_scan_frames_from_dir` | S1-3 已有；S1-4 验证 `len>=2`、排序、唯一 `frame_id` |
| `replay_scan_frames_from_dir` | 顺序读帧，不调用 driver/observe（S1-4d） |

符号名实现时须 owner 批准；**本计划不锁定**。

---

### 6. Files / modules to touch（规划）

| 文件 | 动作（未来） |
| --- | --- |
| `crates/auv-scan/src/producer/mod.rs` | 多帧 fixture 写出 |
| `crates/auv-scan/tests/fixtures/scan/temporal/two_frame_v0/` | 2 PNG + manifest + golden JSON ×2 |
| `crates/auv-scan/src/inspect.rs` | 序列级断言 helper |
| **未来可选** | `scan-timeline-v0` 新模块 — S1-4b 单独计划 |

---

### 7. Hermetic tests（规划 — 名称 + 断言）

| 测试名（规划） | 断言 |
| --- | --- |
| `produce_two_frame_fixture_writes_monotonic_indices` | `scan-frame-0001/0002.json` 存在；`sequence_index` 0/1 |
| `load_scan_frames_from_dir_returns_two_sorted` | `len==2`；按 `sequence_index` 升序 |
| `load_scan_frames_ignores_lexicographic_order` | 文件名乱序仍按 index 排序（[GAN M3](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#s1-4-success-metricsfuture-placeholder)） |
| `two_frame_ids_are_unique` | `frame_id` 不重复 |
| `replay_scan_frames_does_not_invoke_capture` | 纯目录读；无 driver mock 调用 |

Motion 相关测试 **不属于** S1-4 v0。

---

### 8. Validation commands（规划）

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan two_frame
git diff --check
```

S1-4b motion 切片另行定义 filter。

---

### 9. Handoff requirements（规划）

启动 S1-4 时 **新写** 子计划或 handoff（例如 `...-s1-s4a-multi-frame-handoff.md`），须包含：

- 与 S1-2/3 handoff 的衔接
- `two_frame_v0` fixture 布局
- 批准/拒绝的 wire（明确 **无** `ViewportTransform` 除非新 slice）
- 测试表与 [GAN M1–M3](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md#s1-4-success-metricsfuture-placeholder)

**本工程计划 handoff 不包含 S1-4 实现细节。**

---

### 10. Dependencies between slices

| 关系 | 说明 |
| --- | --- |
| **阻塞于 S1-2 + S1-3** | 单帧 produce→read 闭环稳定且 handoff 合并 |
| **阻塞于 owner** | 显式点名 S1-4a/b/c/d 子切片 |
| **依赖 Slice 1** | 单帧 wire 不变；多帧仅增文件数量 |
| **后续** | [S1 plan](2026-07-02-auv-scan-s1-temporal-core-plan.md) step 2 motion → S1-4b 或更后 |

---

## 与原 S1 Temporal Core Plan 的对照

| 原 step | 本计划 |
| --- | --- |
| 1 Frame contract | ✅ [Slice 1 handoff](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md) |
| 2 Motion | S1-4b 或之后（blocked） |
| 3–5 Fusion / tracks / diagnostics | S1-4 之后 |
| Producer-first 重排 | S1-2（本计划） |

## 相关文档

- [GAN implementation spec](2026-07-02-auv-scan-s1-s2-s4-gan-spec.md)
- [S1 Slice 1 handoff](2026-07-02-auv-scan-s1-slice1-frame-contract-handoff.md)
- [S0 charter](2026-07-02-auv-scan-s0-charter.md)
- [S1 temporal core plan](2026-07-02-auv-scan-s1-temporal-core-plan.md)
- [Scroll scan design](2026-05-21-scroll-scan-design.md) — donor only，不耦合
- `CONTRIBUTING.local.md`

## Validation（本文档 only）

```sh
git diff --check
```
