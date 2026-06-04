# AUV Inference YOLO Design

> NOTICE: This document now describes a deferred alternative route:
> an AUV-owned raw YOLO decode / letterbox / NMS pipeline.
> It is not the current implementation path on `main`.
> The current implemented route is `auv-inference-common` plus
> `auv-inference-ultralytics`.

## Scope

Build a narrow Rust YOLO ONNX inference crate for AUV and verify it against
real Balatro models and images from
`/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro`.

This slice only proves object detection inference. It does not implement
Balatro game state, OCR, action execution, media stream abstractions, or AUV
driver capture changes.

## Goals

- Add `crates/auv-inference-yolo` as a workspace crate.
- Support Ultralytics v8/v11-like object detection ONNX exports with output
  layout `[1, 4 + class_count, anchor_count]`.
- Load class labels from caller-provided names.
- Accept image input from file-backed test fixtures.
- Run ONNX inference through `ort`.
- Implement letterbox preprocessing, reverse-letterbox bbox projection,
  confidence filtering, and NMS.
- Produce typed detections with bounds, class id, label, confidence, model id,
  and source image size.
- Provide a minimal callable example that accepts model, image, classes, and
  threshold parameters, writes detections JSON, and can write an annotated PNG
  artifact.
- Validate against Balatro entities and UI ONNX models using a real image and a
  Python-generated fixture.

## Non-Goals

- No `auv-game-balatro` crate in this slice.
- No RapidOCR or OCR model pipeline.
- No generic `auv-shared` crate unless implementation proves it is needed for
  this slice.
- No macOS driver API changes.
- No `auv invoke` command catalog integration in this slice because that would
  touch `src/catalog.rs` and runtime dispatch surfaces that currently have live
  Collabi intent conflicts. The example binary is the minimum callable API for
  this slice.
- No Steam/Balatro launch automation or Swift overlay visualization in this
  slice; those belong to later `auv-game-balatro` and visual overlay slices.
- No YOLO segmentation, pose, oriented boxes, tracking, or built-in model-NMS
  exports.
- No long-term public contract commitment for every internal YOLO raw type.

## Architecture

`auv-inference-yolo` owns model inference and YOLO-specific image processing.
It does not depend on AUV platform drivers. Callers provide a model path, class
names, and image bytes/path; the crate returns detections in source-image pixel
coordinates.

The crate should expose a compact API:

- `YoloDetector::load(config) -> Result<YoloDetector>`
- `YoloDetector::detect(image, options) -> Result<DetectionSet>`
- `YoloModelConfig`
- `DetectionOptions`
- `DetectionSet`
- `Detection`
- `BoundingBox`
- `render_annotated_image`

Internally the crate keeps YOLO-specific concepts:

- `Letterbox`
- tensor input preparation
- raw output decoding
- NMS

The first supported family is `YoloFamily::UltralyticsV8Like`, which covers the
Balatro YOLO11n ONNX exports observed during investigation.

## Data Flow

```text
image file
  -> decode RGB image
  -> letterbox to model input size, default 640x640
  -> ONNX inference through ort
  -> decode [1, 4 + classes, anchors]
  -> choose max class score per anchor
  -> confidence threshold
  -> reverse letterbox to source image pixels
  -> class-aware NMS
  -> DetectionSet
  -> optional JSON output and annotated PNG artifact
```

The Python fixture should be generated before Rust implementation using the
Balatro repository's pixi environment. The fixture must use the same real ONNX
models and one real dataset image, and write JSON detections that Rust tests can
compare against.

## Fixture Strategy

Use these Balatro assets:

- Entities model:
  `/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx`
- UI model:
  `/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/models/games-balatro-2024-yolo-ui-detection/onnx/model.onnx`
- Entities classes:
  `/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt`
- UI classes:
  `/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/data/datasets/games-balatro-2024-ui-detection/data/train/yolo/classes.txt`

The fixture generator should avoid copying Balatro's current ONNX postprocess
implementation because that code assumes a different raw output shape. It
should use a verified decoder for `[1, C, N]` outputs or Ultralytics prediction
as the baseline.

## Error Handling

The crate should return explicit errors for:

- model file missing
- image decode failure
- unsupported YOLO output shape
- class count mismatch
- ONNX Runtime session/load/run failure
- invalid thresholds
- empty class list

Errors should be useful in CLI output and tests. They do not need to introduce a
full AUV-wide error type in this slice.

## Testing

Add focused Rust tests for:

- letterbox metadata and reverse projection
- raw output shape decoding for `[1, C, N]`
- confidence filtering
- NMS suppression and class-aware behavior
- Balatro fixture parity for entities and UI models
- example invocation that writes JSON and an annotated image artifact

Parity tests should compare class id, label, confidence within tolerance, and
bbox coordinates within a small pixel tolerance. Exact equality is not required
because backend numerical details may vary.

## Open Deferrals

- `auv-shared` geometry/media types are deferred until more than this crate
  needs them.
- OCR inference is deferred to a separate `auv-inference-ocr` slice.
- Capture source, video stream, and driver coordinate projection are deferred to
  driver/media design work.
- Balatro state/action/RL episode integration is deferred to
  `auv-game-balatro`.
- `auv invoke` integration is deferred until the active Collabi conflicts around
  command catalog/runtime surfaces are cleared and the owner approves that
  runtime-facing slice.
- Swift transparent overlay visualization is deferred until after the inference
  crate can produce stable detections and annotated artifacts.
