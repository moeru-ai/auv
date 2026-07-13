# AUV Scan S1 Slices 2–4 — GAN Implementation Spec

**Date:** 2026-07-02  
**Status:** planner spec — **no code**  
**Prerequisite:** [S1 slice 1 handoff](2026-07-03-scan-temporal-core-landed.md) (`scan-frame-v0` landed in `crates/auv-scan`)  
**Owner brief:** minimal real producer → inspect reader → multi-frame/temporal (only after single-frame loop is stable)

> Generated from owner intent: S1-2 producer, S1-3 reader, S1-4 multi-frame deferred; **not** multi-source abstraction, generic traits, platform refactor, live complex timing, or full viewer.

---

## Vision（中文 · owner-facing）

S1 slice 1 已锁定 `scan-frame-v0` 单帧 wire 与 hermetic fixture。接下来三片的目标是把 **「能产出一帧 → 能读回并断言 → 再谈多帧」** 这条最小闭环跑通，而不是提前铺 motion / tracks / OCR fusion / viewer。

- **S1-2**：有一个**最小真实 producer** 能把一次观测落成 `scan-frame-NNNN.json`（及可选 PNG 兄弟文件），可 hermetic 也可选 live。
- **S1-3**：有一个 **inspect/reader** 从 artifact 目录或 run 存储读回 `ScanFrame`，做字段级断言；**不要**大 UI。
- **S1-4**：在单帧闭环稳定后，再引入 **多帧序列** 与 temporal 能力（motion、timeline 等按 [S1 temporal core plan](2026-07-03-scan-temporal-core-landed.md) 后续步骤拆分，**本 spec 只规划方向，不批准实现**）。

铁律：**先 hermetic，后 live；先单帧，后多帧；先窄 API，后抽象层。**

---

## Slice product goals（每片一段）

### S1-2 — Minimal real producer

Slice 1 提供了 `write_frame_artifact` / `read_frame_artifact` 与 test-only `build_frame_from_fixture`，但**没有**从真实像素源到落盘的端到端路径。S1-2 要补一条**最小 producer**：输入为「已有 PNG + 已知 bounds 元数据」或「一次 `auv_driver::Capture`」，输出为 artifact 目录内的 `scan-frame-0001.json` 与 `image.file_name` 指向的 PNG。Producer 只负责 **normalize + persist**，不引入 motion、OCR、run 隐式录制、CLI 子命令面（除非 owner 在实现 slice 时单独点名）。Hermetic 路径必须是 merge gate；live 路径（macOS window capture）标 **`live`**，非 gate。

### S1-3 — Inspect / reader consumes artifact

当磁盘上已有 `scan-frame-v0` artifact 时，调用方需要**稳定的读侧聚合**：按目录或显式路径列表加载帧、校验 schema/bounds、暴露可测试的字段断言（`frame_id`、`sequence_index`、`window_bounds`、`image` 尺寸与文件名一致性等）。读侧参考 `auv-view` 的 `inspect.rs` 与 `run_read` 中 scroll-scan 提取模式，但**范围仅限单帧 ScanFrame**——不建 viewer、不渲染 PNG、不回答 S0 五个问题的完整 temporal 答案。目标：Generator 实现后，Evaluator 能用 hermetic fixture **无 UI** 验证「写入 → 读出 → 字段一致」。

### S1-4 — Multi-frame / temporal（future）

在 S1-2/3 单帧 producer→reader 闭环稳定且 hermetic 回归绿之后，才批准多帧：至少两帧 fixture 目录、`sequence_index` 单调、可选 `scan-timeline.json` 占位或 motion slice。Temporal association、tracks、diagnostics 仍属 [S1 temporal core plan](2026-07-03-scan-temporal-core-landed.md) steps 2–5，**不得**借 S1-4 之名一次性并入。S1-4 的「多帧」首期仅指：**有序多帧 artifact 序列 + 读侧枚举 + 序列级 hermetic 测试**；motion/OCR/track 各为后续独立 slice。

---

## Design direction（minimal, no over-engineering）

| Principle | S1-2/3 stance |
|-----------|---------------|
| Owning crate | 继续 **`crates/auv-scan`**；producer/reader API 增在 crate 内，不拆 `scan-producer` crate |
| Abstraction | **禁止** generic `FrameSource` trait、multi-platform capture facade、可插拔 backend registry |
| Platform | **禁止** S1-2 内 macOS/Windows driver 重构；live 仅允许薄映射函数 + `cfg` gate |
| Wire | **不扩展** `scan-frame-v0` 字段；新能力用新文件或新 schema version（需新 slice 批准） |
| Run recording | S1-2/3 **不强制** 接入 `src/runtime.rs` 隐式录制；若接入，单独子 slice |
| UI | **零** viewer 变更；inspect 输出为 Rust 结构 + 可选纯文本 summary，供测试断言 |
| Errors | 延续 `ScanArtifactError` 变体断言风格；读侧新增错误用新 enum 或扩展 variant，不断言 `Display` 字符串 |
| Fixtures | 延续 `tests/fixtures/scan/temporal/<scenario>/`；S1-2 可增 `producer_single_frame_v0/`，S1-4 再增 `two_frame_v0/` |

**Anti-patterns（显式禁止）：**

- 为「将来 multi-source」预留的 trait 层或 `Box<dyn …>`
- 把 `scroll_scan` 整页循环嵌入 temporal producer
- 在 S1-3 复制 `inspect_server_viewer.html` 或新增 HTTP 端点
- 用 live 高滚动 UI 测试替代 hermetic gate

**视觉 / UX：** 本阶段无 UI。若需人工验证，owner 用文件浏览器或 `jq` 查看 JSON 即可。

---

## Recommended producer candidate（含权衡）

### 推荐：**Option A — `auv-scan` 内 `produce_frame_from_capture`（主路径）+ hermetic `produce_frame_from_fixture_dir`（gate）**

| 方面 | 说明 |
|------|------|
| **形状** | `produce_frame_from_fixture_dir(dir, out_dir) -> Result<ProducedFrame, …>` 复制 manifest 图像、写 JSON；`produce_frame_from_driver_capture(capture, meta, out_dir) -> …` 写 PNG + JSON（`#[cfg(feature = "live-capture")]` 或 `cfg(target_os = "macos")` + dev-dep） |
| **优点** | 契约与 IO 同 crate；hermetic 与 live 共享同一 `ScanFrame` 构建逻辑；不牵动 `scroll_scan` / runtime |
| **缺点** | `auv-scan` 若 dev-dep `auv-driver`，需注意依赖方向（scan 不应成为 driver 的硬依赖 — live 用 optional feature） |
| **Gate** | Hermetic：`produce_frame_from_fixture_dir(single_frame_v0)` 输出字节级等于 golden 或 roundtrip 等价 |

### Option B — 扩展 test-only `build_frame_from_fixture` 为公开 `build_frame_from_fixture`（仅 hermetic）

| 方面 | 说明 |
|------|------|
| **优点** | 最小 diff；零 driver 依赖 |
| **缺点** | **不满足** owner「minimal **real** producer」— 无 PNG 落盘、无 live 路径 |
| **verdict** | 仅作 S1-2 内部步骤，**不能**作为 slice 完成定义 |

### Option C — `scroll_scan` observe 页截图路径 donor

| 方面 | 说明 |
|------|------|
| **优点** | 已有 screenshot + bounds 元数据 |
| **缺点** | 强耦合 page 循环、`ScrollScanArtifact`、observe 副作用；违背「单帧闭环」与 scope 纪律 |
| **verdict** | **Reject** 作为 S1-2 主 producer；日后可作为 **donor 字段映射参考** 只读对照 |

### Option D — 新 CLI `auv scan capture-frame`

| 方面 | 说明 |
|------|------|
| **优点** | 人工 live 探针方便 |
| **缺点** | 扩大 slice 到 CLI/catalog/runtime；与「crate 内闭环」无关 |
| **verdict** | **Defer** 到 S1-2 之后的 optional 子 slice，非 S1-2 DoD |

### Option E — `auv-driver-macos` `window.capture` 直接写 artifact

| 方面 | 说明 |
|------|------|
| **优点** | 最真实 live |
| **缺点** | 把 S-line wire 塞进 driver crate，边界错误；难 hermetic |
| **verdict** | **Reject**；driver 只提供 `Capture`，映射留在 `auv-scan` |

**实现建议（S1-2）：** 以 **Option A** 为默认；公开 API 控制在 2–3 个函数 + 1 个 `ProducedFrame { json_path, image_path, frame }` 结果类型；live 映射函数放在 `producer/live.rs` 或 `producer/driver.rs`，`default-features = false` 下 crate 仍可纯 hermetic 构建。

---

## Sprint breakdown

### Sprint S1-2: Single-frame producer（可实施）

**Goals**

- 端到端：输入（fixture 或 Capture）→ artifact 目录含 `scan-frame-0001.json` + PNG
- 与 slice 1 golden wire 兼容；不修改 `scan-frame-v0` schema

**Features**

1. **`produce_frame_from_fixture_dir`** — 读现有 `single_frame_v0` manifest + PNG，写入 `out_dir`，返回路径
2. **`write_frame_with_image`** — 先写 PNG（`image.file_name`），再 `write_frame_artifact`；PNG 缺失时明确错误 variant
3. **`frame_from_capture`**（纯函数）— `auv_driver::Capture` + `FrameCaptureMeta` → `ScanFrame`；bounds 从 `Capture.bounds` 映射为 `ScanBounds`（i64，单一舍入函数）
4. **Optional live** — `produce_frame_from_window_capture` behind feature；**不**进默认 CI

**Definition of done**

- [ ] `cargo test -p auv-scan` 全绿（含新 producer 测试）
- [ ] Hermetic：producer 输出经 `read_frame_artifact` 与 golden 一致（或 PNG 哈希 + JSON 相等）
- [ ] 无新 public trait；`lib.rs` re-export 列表在 handoff 中更新
- [ ] `cargo fmt --check`、`cargo check -p auv-scan`、`git diff --check`
- [ ] 新 handoff：`2026-07-02-auv-scan-s1-slice2-producer-handoff.md`（实现 agent 撰写，非本 spec）

**Out of scope**

- Multi-frame、`sequence_index > 0` 批量目录约定（→ S1-4）
- Runtime artifact role 注册、CLI
- OCR / motion / tracks

---

### Sprint S1-3: Frame inspect reader（可实施）

**Goals**

- 读侧 API 消费 S1-2 产出（或任意合规 `scan-frame-v0` 目录）
- 字段断言可供 Evaluator 直接调用；无 UI

**Features**

1. **`ScanFrameInspect`** — 聚合：`frames: Vec<ScanFrame>`、`source_dir`、`loaded_paths`（S1-3 仅 0–1 帧；API 形状可为 `Vec` 以便 S1-4 扩展）
2. **`load_scan_frames_from_dir(dir)`** — glob `scan-frame-*.json`，按 `sequence_index` 排序，逐个 `read_frame_artifact`；**S1-3 测试仅单文件**
3. **Field assertion helpers** — `assert_matches_manifest` 或 `FrameFieldExpectation` 供测试
4. **`summarize_scan_frame_text(frame) -> String`** — 短文本，供日志/测试 snapshot；非 viewer
5. **Cross-check** — `image.width/height` 与磁盘 PNG 尺寸一致；不一致 → 确定性错误 variant

**Definition of done**

- [ ] 测试：`load` golden 目录 → `assert` 与 manifest 字段一致
- [ ] 测试：PNG 尺寸与 `ScanImageRef` 不一致时失败（variant 断言）
- [ ] 测试：空目录 / 坏 schema 错误路径
- [ ] **不**修改 `inspect_server` / viewer HTML
- [ ] 可选：`run_read` 提取函数 **仅** 在 owner 点名时做；**默认不在 S1-3 DoD**

**Out of scope**

- 回答 S0 五问中的跨帧身份、视口运动（单帧只能部分回答「当前视口有什么」）
- `scan-timeline.json` / tracks
- HTTP / Web UI

---

### Sprint S1-4: Multi-frame sequence（future — 本 spec 不批准实现）

**Goals（规划 only）**

- 扩展 producer 写入 `scan-frame-0001.json` … `scan-frame-000N.json`
- Reader 枚举 N 帧、断言 `sequence_index` 单调、帧 ID 唯一
- 为 [S1 plan](2026-07-03-scan-temporal-core-landed.md) step 2（motion between two frames）准备 `two_frame_v0` fixture

**Blocked until**

- S1-2 + S1-3 handoff 合并
- 单帧 producer/reader hermetic 套件连续绿或 owner 显式 sign-off

**Not in S1-4 v0**

- `ViewportTransform`、`TemporalTrack`、`ScanDiagnostic` wire
- Live 高滚动探针

---

## Evaluation criteria / success metrics

### Rubric weights（Evaluator 用）

| Dimension | Weight | S1-2/3 focus |
|-----------|--------|----------------|
| Contract fidelity | 0.35 | `scan-frame-v0` 不变；roundtrip 与 golden |
| Hermetic reliability | 0.35 | 无 live 依赖的 CI 测试 |
| API minimalism | 0.15 | 无 trait 膨胀、无跨 crate 泄漏 |
| Craft | 0.15 | 错误变体、PNG+JSON 一致性、清晰模块边界 |

### S1-2 success metrics

| ID | Metric | Gate |
|----|--------|------|
| P1 | Fixture producer 输出 JSON **equals** golden `ScanFrame` | **Hermetic — required** |
| P2 | 兄弟 PNG 存在且 `image.file_name` 可解析 | **Hermetic — required** |
| P3 | `frame_from_capture` 单元测试使用内存 `RgbaImage` Capture，无 OS API | **Hermetic — required** |
| P4 | macOS live 单次 window capture 产出可读 artifact | **Live — optional**, label `live`, manual checklist |
| P5 | `cargo test -p auv-scan` 无 `#[ignore]` 的 live 测试在 CI 默认跑 | **Required** |

### S1-3 success metrics

| ID | Metric | Gate |
|----|--------|------|
| R1 | `load_scan_frames_from_dir` 加载 golden 目录，字段断言全过 | **Hermetic — required** |
| R2 | PNG 尺寸与 wire 不一致 → 确定性错误 variant | **Hermetic — required** |
| R3 | `summarize_scan_frame_text` 含 `frame_id`、`sequence_index`、bounds 摘要 | **Hermetic — required** |
| R4 | 人类用 `jq` / 文件浏览器核对 artifact | **Live — optional** |

### S1-4 success metrics（future placeholder）

| ID | Metric | Gate |
|----|--------|------|
| M1 | `two_frame_v0` fixture：2 JSON + 2 PNG，index 0/1 | Hermetic |
| M2 | Reader 返回 `len==2` 且排序正确 | Hermetic |
| M3 | 故意打乱文件名顺序仍按 `sequence_index` 排序 | Hermetic |

### Live protocol（label: `live`）

- **When:** S1-2 P4 或 owner 手动验证；**never** merge gate until P1–P3 / R1–R3 green
- **How:** macOS 前台普通窗口，`produce_frame_from_window_capture` 一次 → `read_frame_artifact` + 目视 PNG
- **Checklist（单帧）：** JSON schema_version；bounds 正数；PNG 可打开；与窗口大致相符（人工）

---

## Risk register

| ID | Risk | Likelihood | Impact | Mitigation |
|----|------|------------|--------|------------|
| R-01 | `auv-scan` → `auv-driver` 依赖污染默认构建 | Med | Med | optional feature `live-capture`；default 关闭 |
| R-02 | `Capture.bounds`（f64 `Rect`）→ `ScanBounds`（i64）舍入不一致 | Med | High | 单一 `bounds_to_scan_bounds` 函数 + 表驱动舍入测试 |
| R-03 | Producer 写 PNG 与 JSON 非原子，读侧看到半写状态 | Low | Med | 先写 PNG 再 JSON；或写临时名后 rename（实现 slice 二选一，文档化） |
| R-04 | S1-3 scope creep 进 `run_read` / viewer | High | High | DoD 显式排除；run 集成单列 slice |
| R-05 | S1-4 提前做 motion/tracks | Med | High | 本 spec blocked until；Evaluator 拒绝超 scope PR |
| R-06 | 与 `scroll_scan` screenshot 字段语义漂移 | Med | Med | 映射函数旁 `NOTICE:` 引用 scroll_scan donor，不共享类型 |
| R-07 | Live CI flaky | High | Low | live 测试 `#[ignore]` + manual label only |
| R-08 | `image.file_name` 与 glob 模式不一致 | Low | Med | 同目录 + manifest 指定 file_name；测试覆盖路径遍历拒绝 |

---

## Technical stack（本阶段）

| Layer | Choice |
|-------|--------|
| Contract crate | `crates/auv-scan`（已有） |
| Serialization | `serde` / `serde_json`（已有） |
| PNG IO | `image` crate（crate 已有或新增轻 dep） |
| Live capture donor | `auv-driver` + `auv-driver-macos`（optional feature only） |
| Reader pattern donor | `crates/auv-view/src/memory/inspect.rs`、`src/run_read.rs` scroll-scan 提取（只读参考） |
| Tests | in-crate `#[cfg(test)]` + `tests/fixtures/scan/temporal/` |

**不引入：** 新 binary、新 HTTP、React viewer、generic plugin registry。

---

## Wire reference（`scan-frame-v0` — 只读，不扩展）

Synthetic example (from slice 1 golden):

```json
{
  "schema_version": "scan-frame-v0",
  "frame_id": "frame-0001",
  "sequence_index": 0,
  "captured_at_millis": 1700000000000,
  "window_bounds": { "x": 0, "y": 0, "width": 800, "height": 600 },
  "viewport_bounds": null,
  "image": {
    "file_name": "frame-0001.png",
    "width": 8,
    "height": 8,
    "media_type": "image/png"
  }
}
```

Artifact naming: `scan-frame-NNNN.json` where `NNNN = sequence_index + 1` (4-digit zero-padded).

---

## API sketch（provisional — 实现 slice 须 owner 批准符号名）

```text
// S1-2 (proposed)
pub struct ProducedFrame { pub json_path, pub image_path, pub frame: ScanFrame }
pub struct FrameCaptureMeta { frame_id, sequence_index, captured_at_millis, window_bounds, viewport_bounds? }

pub fn produce_frame_from_fixture_dir(fixture_dir, out_dir) -> Result<ProducedFrame, ScanArtifactError>
pub fn frame_from_capture(capture: &Capture, meta: FrameCaptureMeta) -> Result<ScanFrame, ScanArtifactError>
pub fn write_frame_with_image(dir, frame, image_bytes) -> Result<ProducedFrame, ScanArtifactError>

// S1-3 (proposed)
pub struct ScanFrameInspect { pub frames: Vec<ScanFrame>, ... }
pub fn load_scan_frames_from_dir(dir) -> Result<ScanFrameInspect, ScanArtifactError>
pub fn summarize_scan_frame_text(frame: &ScanFrame) -> String
```

NOTICE: 上名为 **planner 词汇**；实现 agent 可微调命名，但不得扩展 wire schema。

---

## Validation commands（实现后）

```sh
cargo fmt --check
cargo check -p auv-scan
cargo test -p auv-scan
git diff --check
```

Live（optional）:

```sh
cargo test -p auv-scan --features live-capture -- --ignored
```

---

## Related

- [S0 charter](2026-07-02-scan-charter.md)
- [S1 temporal core plan](2026-07-03-scan-temporal-core-landed.md)
- [S1 slice 1 handoff](2026-07-03-scan-temporal-core-landed.md)
- [Scroll scan design](../view-memory/2026-05-21-scroll-scan-design.md) — donor only

---

## Appendix: Evaluator rubric（machine-friendly）

| Check | S1-2 | S1-3 |
|-------|------|------|
| Golden JSON match | ✓ | ✓ |
| PNG sibling exists | ✓ | — |
| Reader field assert | — | ✓ |
| Image dimension cross-check | — | ✓ |
| No new traits in public API | ✓ | ✓ |
| No viewer/HTML changes | ✓ | ✓ |
| `cargo test -p auv-scan` green | ✓ | ✓ |

**Fail fast if:** schema 扩展、scroll_scan 耦合 producer、`inspect_server` diff、或 S1-4 motion 代码出现在 S1-2/3 PR。
