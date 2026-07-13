# Maa Recognition and Pipeline Patterns — Research Notes

**Date**: 2026-05-24  
**Source**: MaaXYZ/MaaFramework main branch (docs + C++ source)  
**Purpose**: Verify terminology, result shapes, and runtime cache behavior before writing Phase 7.

---

## 1. Pipeline Protocol: What Maa Calls a "Node"

Each pipeline node is:

```jsonc
{
  "NodeName": {
    "recognition": "OCR",       // algorithm type
    "action": "Click",          // what to do after recognition
    "next": ["NodeB", "NodeC"], // successor list (sequential probe)
    "roi": [x, y, w, h],        // search boundary
    "timeout": 20000
  }
}
```

Execution loop: repeatedly probe `next` list in order until first hit, or timeout.  
`roi` constrains the search area. `box` is the recognition hit (output).  
`target` is the action destination — defaults to `box`, can be overridden.

**AUV parallel**: Skill recipes + inline hook are roughly the action layer.  
`roi` → `RecognitionScope`, `box` → `RecognizedItem.box_`, `target` → click target.  
AUV does NOT use a static successor graph; decisions go through agent hooks instead.

---

## 2. Recognition Result Shape (from source)

`MaaTaskerGetRecognitionDetail` returns:

| field       | type            | notes                                     |
|-------------|-----------------|-------------------------------------------|
| node_name   | string          | which pipeline node triggered this reco   |
| algorithm   | string          | e.g. "OCR", "TemplateMatch", "NeuralNetworkDetect" |
| hit         | bool            | whether best_result is populated          |
| box         | [x, y, w, h]   | best hit bounding box, screen coordinates |
| detail_json | JSON            | algorithm-specific detail (see below)     |

All recognizers share the same three-bucket pattern via `RecoResultAPI<T>`:

```
all_results_      // every candidate found
filtered_results_ // candidates passing threshold/expected filter
best_result_      // single winner after order_by + index
```

### Algorithm-specific `detail_json` shapes

**OCR**:
```json
{ "text": "...", "box": [x, y, w, h], "score": 0.97 }
```
(wstring in C++, UTF-8 in JSON)

**TemplateMatch**:
```json
{ "box": [x, y, w, h], "score": 0.87 }
```

**NeuralNetworkDetect** (YOLO — the Phase 7c target):
```json
{ "cls_index": 2, "label": "play_button", "box": [x, y, w, h], "score": 0.91 }
```
Supports YOLOv8 and YOLOv11 exported as ONNX. Model placed under `model/detect/`.

**NeuralNetworkClassify**:
```json
{ "cls_index": 0, "label": "selected", "box": [x, y, w, h], "score": 0.99 }
```
(also has `raw` and `probs` vectors in the C++ struct, not serialized to JSON by default)

### Mapping to AUV's `RecognizedItem`

| Maa field      | AUV field              | compatible? |
|----------------|------------------------|-------------|
| box [x,y,w,h]  | box_: RecognitionBox   | ✅ same shape |
| score          | provider_score: f64    | ✅ same meaning |
| label          | kind: String           | ✅ label ≈ kind |
| cls_index      | detail: Value          | ✅ put in detail |
| all_results_   | all: Vec<RecognizedItem> | ✅ same |
| filtered_results_ | filtered: Vec<...>  | ✅ same |
| best_result_   | best: Option<...>      | ✅ same |

AUV already has the right shape. No renaming needed.

---

## 3. Runtime Cache Lookup by reco_id

Maa assigns a numeric `reco_id` to each recognition run.  
It flows through callbacks (`Node.Recognition.Succeeded { reco_id }`) and can be  
queried later via `MaaTaskerGetRecognitionDetail(reco_id, ...)`.

**AUV parallel**: AUV uses `recognition_id: String` (UUID) in `RecognitionResult`.  
The `evidence: Vec<ArtifactRef>` field in `RecognitionResult` plays the role of  
the reco_id cache — callers query the artifact store by ref rather than a numeric ID.  
No change needed; AUV's UUID + artifact store is a superset of Maa's numeric cache.

---

## 4. Pipeline Override as Execution Context

`MaaContextRunTask(entry, pipeline_override)` lets the caller override any node's  
parameters at runtime without touching the stored pipeline JSON.

```jsonc
// override: make NodeA use a different template at runtime
{
  "NodeA": { "template": "runtime_variant.png" }
}
```

**AUV parallel**: This is the conceptual ancestor of AUV's inline hook block  
(`SkillInlineHook` in `skill.rs`, Phase 6). The hook block overrides scan behavior  
per-invocation, similar to how pipeline_override scopes node parameters per task run.  
The naming is different; the concept is the same.

---

## 5. Action Wait / Stability Checks

Maa has `pre_wait_freezes` / `post_wait_freezes`: wait until screen stops changing  
(template match with threshold 0.95) before/after an action.

**AUV parallel**: AUV's `observeWindowRegion` does screenshot diff stability  
implicitly via scroll boundary checks (TODO 1385). Maa's explicit `wait_freezes`  
with configurable `threshold` and `timeout` is a cleaner model worth borrowing for  
Phase 7's segmentation stability check.

---

## 6. Debugger / Runtime Visualization

Maa provides `draws` (annotated screenshots with bounding boxes) via  
`MaaTaskerGetRecognitionDetail(..., raw, draws)` — only populated in debug mode.

**AUV parallel**: AUV writes recognition artifacts to the artifact store  
(`artifact_0004` style). The draws field's purpose is served by AUV's debug  
artifacts plus the `live-inspect` server. No direct adoption needed.

---

## 7. What AUV Will Reuse (Conceptual)

| Maa concept                         | AUV adoption                                  |
|-------------------------------------|-----------------------------------------------|
| `roi / box / target` terminology    | Keep AUV's `scope / box_ / click_target`; compatible semantics |
| Three-bucket result (all/filtered/best) | Already in `RecognitionResult` ✅            |
| `detail_json` per algorithm         | Already `RecognizedItem.detail: Value` ✅     |
| Pipeline override as execution context | Phase 6 `SkillInlineHook` — already done ✅  |
| NeuralNetworkDetect result shape    | Phase 7c: emit as `RecognitionSource::NeuralNetworkDetect` with `detail: { cls_index, label, score }` |
| `wait_freezes` stability model      | Consider for Phase 7a segmentation stability gate |

## 8. What AUV Will NOT Copy

| Maa pattern                         | Reason to skip                                |
|-------------------------------------|-----------------------------------------------|
| Static pipeline graph (`next` list) | AUV uses agent hooks; dynamic decision loop   |
| Node-level `timeout` loops          | AUV's scroll_scan owns retry/stop policy      |
| Numeric `reco_id`                   | AUV uses UUID + artifact store; superset      |
| Screen-only coordinate system       | AUV has window/region scope with projection   |
| `max_hit` / `anchor` / `jump_back`  | AUV has no equivalent static graph            |

---

## 9. Phase 7 Readiness Conclusions

**Phase 7a (rule-based segmentation)**:
- Emit `RecognitionResult(source=segmented_region)` with `box_` = detected band rect.
- No Maa names to borrow; rule logic is AUV-specific.
- Consider borrowing `wait_freezes`-style stability check (N consecutive stable frames)
  before declaring a segmentation boundary.

**Phase 7b (template icon match)**:
- Emit `RecognitionResult(source=icon_match)` with `detail: { template, score }`.
- Maa's `TemplateMatcherResult { box, score }` confirms the right shape — AUV already has it.
- Use `green_mask` concept for masking irrelevant regions during match (optional).

**Phase 7c (YOLO plug-in)**:
- Add `NeuralNetworkDetect` (YOLOv8/v11 ONNX) as a separate backend and command;
  do not replace Phase 7b's NCC template command.
- Output shape: `RecognizedItem { kind: label, box_: ..., provider_score: score, detail: { cls_index } }`.
- Use `RecognitionSource::NeuralNetworkDetect`; keep `RecognitionSource::IconMatch`
  reserved for template/icon matching.
- Load the model through the `auv-onnx-runner` subprocess boundary so Rust does
  not take a hard ONNX runtime dependency.

---

## Sources

- `MaaXYZ/MaaFramework` @ main, `docs/zh_cn/3.1-任务流水线协议.md`
- `MaaXYZ/MaaFramework` @ main, `docs/zh_cn/2.2-集成接口一览.md`
- `MaaXYZ/MaaFramework` @ main, `docs/zh_cn/2.3-回调协议.md`
- `MaaXYZ/MaaFramework` @ main, `source/MaaFramework/Vision/VisionTypes.h`
- `MaaXYZ/MaaFramework` @ main, `source/MaaFramework/Vision/VisionBase.h`
- `MaaXYZ/MaaFramework` @ main, `source/MaaFramework/Vision/NeuralNetworkDetector.h`
- `MaaXYZ/MaaFramework` @ main, `source/MaaFramework/Vision/OCRer.h`
- `MaaXYZ/MaaFramework` @ main, `source/MaaFramework/Vision/TemplateMatcher.h`
