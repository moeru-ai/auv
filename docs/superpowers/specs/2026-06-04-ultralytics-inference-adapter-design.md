# Ultralytics Inference Adapter Design

## Scope

Replace the self-owned YOLO backend in `crates/auv-inference-yolo` with a
community-first inference stack built around `ultralytics-inference`, while
keeping AUV-owned inference result types for consumers.

This slice should remove the current custom ONNX YOLO preprocessing, decoding,
and NMS implementation instead of maintaining it as a parallel backend.

## Goals

- Add `crates/auv-inference-common` for stable AUV inference result types.
- Add `crates/auv-inference-ultralytics` as the Ultralytics adapter crate.
- Depend on `ultralytics-inference = "0.0.18"` for YOLO model loading,
  preprocessing, inference, postprocessing, and optional annotation support.
- Convert Ultralytics detection results into AUV-owned common types.
- Keep upper-level consumers independent from `ultralytics-inference` types.
- Re-run Balatro entities/UI parity against the same real ONNX models and
  fixture image.
- Keep a callable example that writes JSON detections and an annotated image
  artifact.
- Delete `crates/auv-inference-yolo` once parity and example coverage exist in
  the adapter crate.

## Non-Goals

- No `auv invoke` command catalog integration in this slice.
- No `auv-game-balatro` game state or RL environment implementation.
- No OCR/RapidOCR work.
- No capture, MediaStream, Swift overlay, Steam launch automation, or macOS
  driver API changes.
- No attempt to preserve the current custom YOLO decoder as a fallback backend.
- No AUV core terminology change in `docs/TERMS_AND_CONCEPTS.md`.

## Licensing Decision

`ultralytics-inference 0.0.18` is AGPL-3.0. The owner has approved using this
dependency in AUV despite AUV being Apache-2.0-oriented, and the adapter should
not block on licensing concerns in this implementation slice.

## Crate Layout

### `auv-inference-common`

Owns common inference domain types that can be consumed by future Balatro,
runtime artifact, or CLI surfaces without importing backend-specific crates.

Public types:

- `ModelId(pub String)`
- `ImageSize { width: u32, height: u32 }`
- `ImageFrame { image: image::RgbImage }`
- `BoundingBox { x1: f32, y1: f32, x2: f32, y2: f32 }`
- `Detection { class_id: usize, label: String, confidence: f32, bbox: BoundingBox }`
- `DetectionSet { model_id: ModelId, image_size: ImageSize, detections: Vec<Detection> }`
- `DetectionOptions { confidence_threshold: f32, iou_threshold: f32, max_detections: usize }`
- `InferenceError` and `InferenceResult<T>`

The common crate should contain renderer support if it only depends on common
types and `image`. A simple `render_annotated_image` helper may live here so
future backends can share the same annotated artifact behavior.

### `auv-inference-ultralytics`

Owns the dependency on `ultralytics-inference` and converts backend results into
`auv-inference-common`.

Public API:

- `UltralyticsDetector::load(config: UltralyticsModelConfig) -> InferenceResult<Self>`
- `UltralyticsDetector::detect_path(path, options) -> InferenceResult<DetectionSet>`
- `UltralyticsDetector::detect_frame(frame, options) -> InferenceResult<DetectionSet>` if the
  upstream API can accept in-memory images without reimplementing preprocessing.
- `UltralyticsModelConfig { model_id, model_path, input_size_override }`

The adapter should use `ultralytics_inference::YOLOModel` and
`ultralytics_inference::InferenceConfig`. If the upstream API is path-first,
the first implementation may expose path-based detection and keep in-memory
frame detection deferred with a `TODO:` marker at the API boundary.

The adapter should use Rust conversion traits where practical:

```rust
impl TryFrom<&ultralytics_inference::Results> for DetectionSet { ... }
```

or a local helper if orphan/lifetime constraints make a trait implementation
awkward. Since `DetectionSet` is AUV-owned, implementing `TryFrom` for it is
allowed.

## Data Flow

```text
image path or image frame
  -> ultralytics-inference YOLOModel
  -> upstream Results
  -> adapter extracts boxes.xyxy(), boxes.conf(), boxes.cls(), names
  -> auv-inference-common DetectionSet
  -> JSON artifact and annotated PNG artifact
```

Upper-level AUV consumers should only see `DetectionSet` and related common
types. They should not import `ultralytics_inference::Results`, `Boxes`, or
`YOLOModel`.

## Dependency Notes

`ultralytics-inference` default features include annotation/visualization. The
adapter should start with `default-features = false` if the library API still
supports model loading and prediction without them. If prediction requires
default features, the implementation plan should identify the exact feature set
needed and avoid enabling windowed visualization unless required.

`auv-inference-common` may depend on `image`, `serde`, `serde_json`, and
`thiserror`. It must not depend on `ultralytics-inference`, `ort`, AUV macOS
drivers, runtime, overlay, or Balatro-specific crates.

## Feature Mapping

`ultralytics-inference` exposes hardware execution provider features such as
`cuda`, `tensorrt`, `coreml`, `directml`, `openvino`, `onednn`, `xnnpack`,
`rocm`, `migraphx`, `webgpu`, `mobile`, and grouped features such as `nvidia`,
`amd`, and `intel`. These should be mapped in the adapter crate, but upper-level
Balatro consumers should see AUV/Balatro semantic feature names rather than
third-party dependency paths.

`auv-inference-ultralytics` should expose direct adapter features:

```toml
[features]
default = ["cpu"]

cpu = []
xnnpack = ["ultralytics-inference/xnnpack"]

coreml = ["ultralytics-inference/coreml"]

cuda = ["ultralytics-inference/cuda"]
tensorrt = ["ultralytics-inference/tensorrt"]
nvidia = ["ultralytics-inference/nvidia"]
cuda-preprocess = ["ultralytics-inference/cuda-preprocess"]

directml = ["ultralytics-inference/directml"]

openvino = ["ultralytics-inference/openvino"]
onednn = ["ultralytics-inference/onednn"]
intel = ["ultralytics-inference/intel"]

rocm = ["ultralytics-inference/rocm"]
migraphx = ["ultralytics-inference/migraphx"]
amd = ["ultralytics-inference/amd"]

webgpu = ["ultralytics-inference/webgpu"]
mobile = ["ultralytics-inference/mobile"]

annotate = ["ultralytics-inference/annotate"]
visualize = ["ultralytics-inference/visualize"]
video = ["ultralytics-inference/video"]
```

`auv-game-balatro` should not directly expose upstream feature names. It should
use AUV/Balatro semantic features and map them to the adapter:

```toml
[features]
default = ["vision-yolo", "vision-artifacts"]

vision-yolo = ["dep:auv-inference-ultralytics"]
vision-artifacts = ["auv-inference-common/render"]

vision-coreml = ["vision-yolo", "auv-inference-ultralytics/coreml"]
vision-cuda = ["vision-yolo", "auv-inference-ultralytics/cuda"]
vision-tensorrt = ["vision-yolo", "auv-inference-ultralytics/tensorrt"]
vision-nvidia = ["vision-yolo", "auv-inference-ultralytics/nvidia"]
vision-directml = ["vision-yolo", "auv-inference-ultralytics/directml"]
vision-openvino = ["vision-yolo", "auv-inference-ultralytics/openvino"]
vision-xnnpack = ["vision-yolo", "auv-inference-ultralytics/xnnpack"]

vision-debug-window = ["vision-yolo", "auv-inference-ultralytics/visualize"]
vision-video = ["vision-yolo", "auv-inference-ultralytics/video"]
```

Feature flags should control what execution providers are compiled in. Runtime
configuration should choose which compiled provider to use for a run. The
adapter should provide an AUV-owned device enum or parser with values such as
`cpu`, `coreml`, `cuda:0`, `tensorrt:0`, `directml:0`, `openvino`, `xnnpack`,
and `rocm:0`, then map it to `ultralytics_inference::Device`.

Default Balatro usage should remain CPU/basic artifact output. Hardware features
such as `vision-coreml` or `vision-cuda` should be opt-in so the default build
does not require platform-specific GPU dependencies.

## Balatro Parity

The existing Balatro fixture strategy should survive the refactor:

- Use the same `balatro.jpg`, `entities.json`, and `ui.json` fixtures.
- Use the same real Balatro ONNX model paths and class files.
- Run entities and UI models through `auv-inference-ultralytics`.
- Compare detection count, class id, label, confidence, and bbox coordinates.
- Keep confidence and bbox tolerances narrow enough to catch real regressions.

If Ultralytics official preprocessing/NMS differs from the Python fixture
generator, prefer updating fixture generation to reflect the community backend
behavior instead of reintroducing custom YOLO postprocessing. Only adjust the
adapter if the difference is a conversion bug.

## Example Artifact

Replace the current `auv-inference-yolo` example with an
`auv-inference-ultralytics` example:

```bash
cargo run -p auv-inference-ultralytics --example detect -- \
  --model <model.onnx> \
  --image <image> \
  --json-out /private/tmp/auv-ultralytics-detections.json \
  --annotated-out /private/tmp/auv-ultralytics-detections.png
```

Class labels should come from upstream model metadata when available. If the
Balatro models do not carry usable metadata, the example and config may accept
an optional `--classes` path and the adapter should document that fallback.

## Migration Plan

The implementation plan should migrate in this order:

1. Create `auv-inference-common` and move shared types/rendering from
   `auv-inference-yolo`.
2. Add `auv-inference-ultralytics` and verify `ultralytics-inference` builds
   with the minimal feature set.
3. Implement conversion from upstream detection results to common detections.
4. Port Balatro fixture parity tests to the Ultralytics adapter.
5. Port the callable example and annotated artifact output.
6. Remove `auv-inference-yolo` from the workspace and delete its custom
   preprocessing/decode/NMS/detector code.
7. Run focused and workspace validation.

## Validation

Required focused validation:

- `cargo test -p auv-inference-common`
- `cargo test -p auv-inference-ultralytics`
- `cargo test -p auv-inference-ultralytics --test fixture_parity -- --nocapture`
- `cargo run -p auv-inference-ultralytics --example detect -- ...`

Required workspace validation:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

Use the AUV inventory commands after the refactor:

- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`
- `cargo run --quiet -- skill bundle list`

## Open Deferrals

- `auv-game-balatro` integration remains deferred.
- `auv invoke` integration remains deferred until the command catalog/runtime
  slice is explicitly approved and Collabi conflicts are clear.
- OCR/RapidOCR remains deferred to an OCR-specific inference adapter.
- Realtime capture and Swift overlay visualization remain deferred.
