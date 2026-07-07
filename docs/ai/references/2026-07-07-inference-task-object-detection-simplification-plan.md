# Inference Task Object Detection Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split object detection task contracts out of inference backend crates, delete the old detector evidence wrapper stack, and remove `auv-cli` dependencies from inference backend tests.

**Architecture:** `auv-inference-common` keeps only cross-task primitives and errors. `auv-inference-ultralytics` becomes a backend session that predicts `UltralyticsPrediction`. New crate `auv-task-object-detection` owns `Detection`, `DetectionResult`, `DetectionOptions`, rendering, and the `UltralyticsObjectDetector` adapter.

**Tech Stack:** Rust 2024, Cargo workspace, `ultralytics-inference`, `image`, `serde`, existing AUV inference crates, existing Balatro/osu consumer crates.

---

## File Structure

Create:

- `crates/auv-task-object-detection/Cargo.toml`: task crate manifest.
- `crates/auv-task-object-detection/src/lib.rs`: public exports.
- `crates/auv-task-object-detection/src/types.rs`: `Detection`, `DetectionResult`, `DetectionOptions`.
- `crates/auv-task-object-detection/src/render.rs`: detection annotation renderer moved from common.
- `crates/auv-task-object-detection/src/ultralytics.rs`: Ultralytics-backed object detector adapter.

Modify:

- `Cargo.toml`: add workspace member and dependency entries where needed.
- `crates/auv-inference-common/src/lib.rs`: stop exporting detection task types and renderer.
- `crates/auv-inference-common/src/types.rs`: keep only primitives and `ModelConfig`.
- `crates/auv-inference-common/src/render.rs`: delete after renderer moves.
- `crates/auv-inference-ultralytics/Cargo.toml`: remove `auv-cli` dev-dependency and add no task crate dependency.
- `crates/auv-inference-ultralytics/src/detector.rs`: rename/reshape backend wrapper as session.
- `crates/auv-inference-ultralytics/src/convert.rs`: delete or replace with backend accessors.
- `crates/auv-inference-ultralytics/src/lib.rs`: export backend session/prediction types.
- `crates/auv-inference-ultralytics/examples/detect.rs`: move or rewrite as task crate example.
- `crates/auv-game-balatro/Cargo.toml`: depend on `auv-task-object-detection`.
- `crates/auv-game-balatro/src/*.rs`: import detection task types from the task crate.
- `crates/auv-game-osu/Cargo.toml`: depend on `auv-task-object-detection`.
- `crates/auv-game-osu/src/*.rs`: import detection task types from the task crate and replace detection coordinate-space dependency with a local dataset enum.
- `src/lib.rs`: remove `pub mod inference_recognition`.
- `src/inference_recognition.rs`: delete.
- `src/verticals/osu/mod.rs`: import test detection types from the task crate.
- `docs/ai/references/INDEX.md`: add this plan entry if not already present.

Delete:

- `crates/auv-inference-ultralytics/tests/detection_set_boundary.rs`
- `crates/auv-inference-ultralytics/tests/fixture_parity.rs`
- `crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs`
- `crates/auv-inference-common/src/render.rs`
- `src/inference_recognition.rs`

---

### Task 1: Create `auv-task-object-detection`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-task-object-detection/Cargo.toml`
- Create: `crates/auv-task-object-detection/src/lib.rs`
- Create: `crates/auv-task-object-detection/src/types.rs`
- Create: `crates/auv-task-object-detection/src/render.rs`

- [ ] **Step 1: Add the workspace member**

Modify root `Cargo.toml` workspace members by inserting:

```toml
  "crates/auv-task-object-detection",
```

Place it after `"crates/auv-inference-ultralytics",` so the inference/task grouping stays readable.

- [ ] **Step 2: Create the task crate manifest**

Create `crates/auv-task-object-detection/Cargo.toml`:

```toml
[package]
name = "auv-task-object-detection"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true
readme.workspace = true
license.workspace = true
authors.workspace = true
description.workspace = true
documentation.workspace = true
homepage.workspace = true
repository.workspace = true

[dependencies]
auv-inference-common = { path = "../auv-inference-common" }
image.workspace = true
serde.workspace = true

[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 3: Create public exports**

Create `crates/auv-task-object-detection/src/lib.rs`:

```rust
pub mod render;
pub mod types;

pub use auv_inference_common::{BoundingBox, ImageSize};
pub use render::render_annotated_image;
pub use types::{Detection, DetectionOptions, DetectionResult};
```

- [ ] **Step 4: Create lightweight detection types**

Create `crates/auv-task-object-detection/src/types.rs`:

```rust
use auv_inference_common::{BoundingBox, ImageSize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  /// Bounding box in source-image pixel coordinates after backend
  /// preprocessing and postprocessing have been mapped back to the source
  /// image.
  pub bbox: BoundingBox,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionResult {
  pub image_size: ImageSize,
  pub detections: Vec<Detection>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionOptions {
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
}

impl Default for DetectionOptions {
  fn default() -> Self {
    Self {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn detection_result_json_is_minimal() {
    let result = DetectionResult {
      image_size: ImageSize {
        width: 640,
        height: 480,
      },
      detections: vec![Detection {
        class_id: 1,
        label: "hit_circle".to_string(),
        confidence: 0.91,
        bbox: BoundingBox {
          x1: 100.0,
          y1: 120.0,
          x2: 140.0,
          y2: 160.0,
        },
      }],
    };

    let value = serde_json::to_value(result).expect("detection result should serialize");
    let object = value.as_object().expect("detection result should serialize as an object");

    assert_eq!(value["image_size"]["width"], json!(640));
    assert_eq!(value["detections"][0]["label"], json!("hit_circle"));
    for forbidden in [
      "model_id",
      "source_image",
      "model_run",
      "known_limits",
      "coordinate_space",
      "projection_basis",
      "evidence",
    ] {
      assert!(
        !object.contains_key(forbidden),
        "DetectionResult must not carry wrapper field `{forbidden}`"
      );
    }
  }

  #[test]
  fn detection_options_default_matches_object_detection_defaults() {
    let options = DetectionOptions::default();

    assert_eq!(options.confidence_threshold, 0.25);
    assert_eq!(options.iou_threshold, 0.45);
    assert_eq!(options.max_detections, 300);
  }
}
```

- [ ] **Step 5: Move the renderer into the task crate**

Create `crates/auv-task-object-detection/src/render.rs` by moving the existing implementation from `crates/auv-inference-common/src/render.rs` and changing the imports to:

```rust
// NOTICE(object-detection-render-owner): This renderer moves from
// `auv-inference-common` because it depends on object detection task types.
// Task 2 deletes the old common copy, leaving this crate as the sole owner.
use crate::{BoundingBox, Detection};
use image::{Rgb, RgbImage};
```

Keep the existing renderer tests in the moved file. They should now build against `auv_task_object_detection::Detection`.

- [ ] **Step 6: Run task crate tests**

Run:

```bash
cargo test -p auv-task-object-detection
```

Expected: tests compile and pass after this task crate is added.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/auv-task-object-detection
git commit -m "feat(auv-task-object-detection): add object detection task crate"
```

---

### Task 2: Slim `auv-inference-common`

**Files:**
- Modify: `crates/auv-inference-common/src/lib.rs`
- Modify: `crates/auv-inference-common/src/types.rs`
- Delete: `crates/auv-inference-common/src/render.rs`
- Modify: `crates/auv-inference-common/Cargo.toml`

- [ ] **Step 1: Remove detection exports**

Replace `crates/auv-inference-common/src/lib.rs` with:

```rust
pub mod error;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use types::{BoundingBox, ImageFrame, ImageSize, ModelConfig, ModelId};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn image_frame_reports_rgb_image_size() {
    let frame = ImageFrame::new(image::RgbImage::new(12, 7));

    assert_eq!(
      frame.size(),
      ImageSize {
        width: 12,
        height: 7
      }
    );
  }
}
```

- [ ] **Step 2: Remove detection task types from common**

Edit `crates/auv-inference-common/src/types.rs` so it contains only:

```rust
use image::RgbImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImageSize {
  pub width: u32,
  pub height: u32,
}

/// Inference-scoped RGB frame input.
///
/// NOTICE: This is currently an image-backed helper for inference crates, not a
/// general AUV media/shared-contract type.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageFrame {
  pub image: RgbImage,
}

impl ImageFrame {
  pub fn new(image: RgbImage) -> Self {
    Self { image }
  }

  pub fn size(&self) -> ImageSize {
    ImageSize {
      width: self.image.width(),
      height: self.image.height(),
    }
  }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct BoundingBox {
  pub x1: f32,
  pub y1: f32,
  pub x2: f32,
  pub y2: f32,
}

impl BoundingBox {
  pub fn width(&self) -> f32 {
    self.x2 - self.x1
  }

  pub fn height(&self) -> f32 {
    self.y2 - self.y1
  }

  pub fn area(&self) -> f32 {
    self.width().max(0.0) * self.height().max(0.0)
  }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
}
```

- [ ] **Step 3: Delete the common renderer file**

Delete:

```text
crates/auv-inference-common/src/render.rs
```

- [ ] **Step 4: Remove unused common dependencies**

In `crates/auv-inference-common/Cargo.toml`, remove `serde_json.workspace = true` if no common tests or errors still require it after Step 2.

- [ ] **Step 5: Run common tests**

Run:

```bash
cargo test -p auv-inference-common
```

Expected: common tests pass with only primitive coverage.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-inference-common
git rm crates/auv-inference-common/src/render.rs
git commit -m "refactor(auv-inference-common): keep only inference primitives"
```

---

### Task 3: Convert Ultralytics Backend To `UltralyticsSession`

**Files:**
- Modify: `crates/auv-inference-ultralytics/Cargo.toml`
- Modify: `crates/auv-inference-ultralytics/src/detector.rs`
- Delete: `crates/auv-inference-ultralytics/src/convert.rs`
- Modify: `crates/auv-inference-ultralytics/src/lib.rs`
- Modify: `crates/auv-inference-ultralytics/src/device.rs`

- [ ] **Step 1: Remove `auv-cli` dev-dependency**

In `crates/auv-inference-ultralytics/Cargo.toml`, remove:

```toml
auv-cli = { path = "../.." }
auv-tracing-driver = { path = "../auv-tracing-driver" }
axum = { version = "0.8", features = ["ws"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
tower = { version = "0.5", features = ["util"] }
```

Keep:

```toml
[dev-dependencies]
serde_json.workspace = true
ndarray.workspace = true
```

- [ ] **Step 2: Replace backend detector with session**

Replace `crates/auv-inference-ultralytics/src/detector.rs` with a backend-only session. The public surface should include these types and methods:

```rust
use crate::device::InferenceDevice;
use auv_inference_common::{
  ImageFrame, InferenceError, InferenceResult, ModelConfig, ModelId,
};
use image::{DynamicImage, ImageReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ultralytics_inference::{InferenceConfig, Results, YOLOModel};

#[derive(Clone, Debug)]
pub struct UltralyticsModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
  pub max_detections: usize,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsModelConfig {
  fn from(value: ModelConfig) -> Self {
    Self {
      model_id: value.model_id,
      model_path: value.model_path,
      input_size: value.input_size,
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 300,
      device: InferenceDevice::Cpu,
      class_names_override: None,
    }
  }
}

pub struct UltralyticsSession {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  model: Mutex<YOLOModel>,
}

pub struct UltralyticsPrediction {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  results: Vec<Results>,
}

pub struct UltralyticsBoxes<'a> {
  result: &'a Results,
  class_names_override: Option<&'a [String]>,
}
```

Implement these methods:

```rust
impl UltralyticsSession {
  pub fn load(config: UltralyticsModelConfig) -> InferenceResult<Self>;
  pub fn predict_path(&self, path: impl AsRef<Path>) -> InferenceResult<UltralyticsPrediction>;
  pub fn predict_frame(&self, frame: &ImageFrame) -> InferenceResult<UltralyticsPrediction>;
}

impl UltralyticsPrediction {
  pub fn model_id(&self) -> &ModelId;
  pub fn first_boxes(&self) -> InferenceResult<UltralyticsBoxes<'_>>;
}

impl UltralyticsBoxes<'_> {
  pub fn len(&self) -> usize;
  pub fn image_width(&self) -> u32;
  pub fn image_height(&self) -> u32;
  pub fn class_id(&self, index: usize) -> usize;
  pub fn confidence(&self, index: usize) -> f32;
  pub fn xyxy(&self, index: usize) -> [f32; 4];
  pub fn label(&self, index: usize) -> InferenceResult<String>;
}
```

Use the existing validation behavior from `validate_config`, with the field names changed from `config.options.confidence_threshold` to `config.confidence_threshold`, from `config.options.iou_threshold` to `config.iou_threshold`, and from `config.options.max_detections` to `config.max_detections`.

- [ ] **Step 3: Preserve authoritative class override behavior**

Implement `UltralyticsBoxes::label` so override labels remain authoritative:

```rust
pub fn label(&self, index: usize) -> InferenceResult<String> {
  let class_id = self.class_id(index);
  if let Some(class_names) = self.class_names_override {
    return class_names
      .get(class_id)
      .cloned()
      .ok_or(InferenceError::MissingClassLabel { class_id });
  }

  self
    .result
    .names
    .get(&class_id)
    .cloned()
    .ok_or(InferenceError::MissingClassLabel { class_id })
}
```

- [ ] **Step 4: Delete conversion module**

Delete:

```text
crates/auv-inference-ultralytics/src/convert.rs
```

- [ ] **Step 5: Update exports**

Replace `crates/auv-inference-ultralytics/src/lib.rs` with:

```rust
pub mod detector;
pub mod device;

pub use detector::{
  UltralyticsBoxes, UltralyticsModelConfig, UltralyticsPrediction,
  UltralyticsSession,
};
pub use device::InferenceDevice;
```

- [ ] **Step 6: Update backend unit tests**

Keep existing config validation tests in `detector.rs`, but change references from `UltralyticsDetector` to `UltralyticsSession` and from `options: DetectionOptions` to direct threshold fields:

```rust
fn valid_config() -> UltralyticsModelConfig {
  UltralyticsModelConfig {
    model_id: ModelId("test-model".to_string()),
    model_path: PathBuf::from("does-not-exist.onnx"),
    input_size: Some(640),
    confidence_threshold: 0.25,
    iou_threshold: 0.45,
    max_detections: 300,
    device: InferenceDevice::Cpu,
    class_names_override: None,
  }
}
```

- [ ] **Step 7: Run backend tests**

Run:

```bash
cargo test -p auv-inference-ultralytics
```

Expected: backend crate tests compile without `Detection`, `DetectionResult`, or `auv-cli`.

- [ ] **Step 8: Commit**

```bash
git add crates/auv-inference-ultralytics
git rm crates/auv-inference-ultralytics/src/convert.rs
git commit -m "refactor(auv-inference-ultralytics): expose backend prediction session"
```

---

### Task 4: Implement Ultralytics Object Detection Adapter

**Files:**
- Modify: `crates/auv-task-object-detection/Cargo.toml`
- Create: `crates/auv-task-object-detection/src/ultralytics.rs`
- Modify: `crates/auv-task-object-detection/src/lib.rs`

- [ ] **Step 1: Add the backend feature and dependencies**

In `crates/auv-task-object-detection/Cargo.toml`, add:

```toml
[features]
default = []
ultralytics = ["dep:auv-inference-ultralytics"]
```

Add the optional backend dependency:

```toml
auv-inference-ultralytics = { path = "../auv-inference-ultralytics", optional = true }
```

Add test dependencies needed by the adapter conversion tests:

```toml
ndarray.workspace = true
ultralytics-inference.workspace = true
```

- [ ] **Step 2: Add the adapter implementation**

Create `crates/auv-task-object-detection/src/ultralytics.rs`:

```rust
use crate::{BoundingBox, Detection, DetectionOptions, DetectionResult};
use auv_inference_common::{ImageFrame, InferenceResult, ModelConfig, ModelId};
use auv_inference_ultralytics::{
  InferenceDevice, UltralyticsModelConfig, UltralyticsPrediction,
  UltralyticsSession,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct UltralyticsObjectDetectorConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
  pub options: DetectionOptions,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsObjectDetectorConfig {
  fn from(value: ModelConfig) -> Self {
    Self {
      model_id: value.model_id,
      model_path: value.model_path,
      input_size: value.input_size,
      options: DetectionOptions::default(),
      device: InferenceDevice::Cpu,
      class_names_override: None,
    }
  }
}

pub struct UltralyticsObjectDetector {
  session: UltralyticsSession,
}

impl std::fmt::Debug for UltralyticsObjectDetector {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("UltralyticsObjectDetector")
      .finish_non_exhaustive()
  }
}

impl UltralyticsObjectDetector {
  pub fn load(config: UltralyticsObjectDetectorConfig) -> InferenceResult<Self> {
    let session = UltralyticsSession::load(UltralyticsModelConfig {
      model_id: config.model_id,
      model_path: config.model_path,
      input_size: config.input_size,
      confidence_threshold: config.options.confidence_threshold,
      iou_threshold: config.options.iou_threshold,
      max_detections: config.options.max_detections,
      device: config.device,
      class_names_override: config.class_names_override,
    })?;

    Ok(Self { session })
  }

  pub fn detect_path(&self, path: impl AsRef<Path>) -> InferenceResult<DetectionResult> {
    detection_result_from_prediction(self.session.predict_path(path)?)
  }

  pub fn detect_frame(&self, frame: &ImageFrame) -> InferenceResult<DetectionResult> {
    detection_result_from_prediction(self.session.predict_frame(frame)?)
  }
}

pub fn detection_result_from_prediction(
  prediction: UltralyticsPrediction,
) -> InferenceResult<DetectionResult> {
  let boxes = prediction.first_boxes()?;
  let mut detections = Vec::with_capacity(boxes.len());

  for index in 0..boxes.len() {
    let [x1, y1, x2, y2] = boxes.xyxy(index);
    detections.push(Detection {
      class_id: boxes.class_id(index),
      label: boxes.label(index)?,
      confidence: boxes.confidence(index),
      bbox: BoundingBox { x1, y1, x2, y2 },
    });
  }

  Ok(DetectionResult {
    image_size: auv_inference_common::ImageSize {
      width: boxes.image_width(),
      height: boxes.image_height(),
    },
    detections,
  })
}
```

- [ ] **Step 3: Export the adapter config**

In `crates/auv-task-object-detection/src/lib.rs`, add the feature-gated module and export:

```rust
#[cfg(feature = "ultralytics")]
pub mod ultralytics;

#[cfg(feature = "ultralytics")]
pub use ultralytics::{
  UltralyticsObjectDetector, UltralyticsObjectDetectorConfig,
  detection_result_from_prediction,
};
```

- [ ] **Step 4: Move conversion tests into the task crate**

Add tests in `crates/auv-task-object-detection/src/ultralytics.rs` using the existing `ultralytics_inference::{Boxes, Results, Speed}` fixture pattern from the old `convert.rs`. The expected result should omit `model_id`:

```rust
assert_eq!(
  detections,
  DetectionResult {
    image_size: ImageSize {
      width: 8,
      height: 8,
    },
    detections: vec![Detection {
      class_id: 1,
      label: "backend-one".to_string(),
      confidence: 0.9,
      bbox: BoundingBox {
        x1: 1.0,
        y1: 2.0,
        x2: 3.0,
        y2: 4.0,
      },
    }],
  }
);
```

- [ ] **Step 5: Run task crate tests with backend feature**

Run:

```bash
cargo test -p auv-task-object-detection --features ultralytics
```

Expected: task crate conversion and renderer tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-task-object-detection
git commit -m "feat(auv-task-object-detection): add ultralytics adapter"
```

---

### Task 5: Update Balatro Consumers

**Files:**
- Modify: `crates/auv-game-balatro/Cargo.toml`
- Modify: `crates/auv-game-balatro/src/detector.rs`
- Modify: `crates/auv-game-balatro/src/observation.rs`
- Modify: `crates/auv-game-balatro/src/model.rs`
- Modify: `crates/auv-game-balatro/src/cache.rs`
- Modify: `crates/auv-game-balatro/src/card_detection_producer.rs`
- Modify: `crates/auv-game-balatro/tests/real_balatro.rs`
- Modify any remaining `auv_inference_common::{Detection, DetectionSet}` imports found by `rg`.

- [ ] **Step 1: Add task dependency**

In `crates/auv-game-balatro/Cargo.toml`, add:

```toml
auv-task-object-detection = { path = "../auv-task-object-detection", features = ["ultralytics"] }
```

Keep `auv-inference-common` for `ImageSize`, `BoundingBox`, and `InferenceError`.

- [ ] **Step 2: Update Balatro detector wrapper**

In `crates/auv-game-balatro/src/detector.rs`, replace imports with:

```rust
use auv_inference_common::{InferenceResult, ModelId};
use auv_inference_ultralytics::InferenceDevice;
use auv_task_object_detection::{
  DetectionOptions, DetectionResult, UltralyticsObjectDetector,
  UltralyticsObjectDetectorConfig,
};
```

Change the structs:

```rust
#[derive(Debug)]
pub struct BalatroDetectors {
  entities: UltralyticsObjectDetector,
  ui: UltralyticsObjectDetector,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BalatroDetectionSets {
  pub entities: DetectionResult,
  pub ui: DetectionResult,
}
```

Change loader calls from `UltralyticsDetector::load(UltralyticsModelConfig { ... })` to `UltralyticsObjectDetector::load(UltralyticsObjectDetectorConfig { ... })`.

- [ ] **Step 3: Update Balatro imports**

Use these replacements across Balatro source and tests:

```text
auv_inference_common::Detection -> auv_task_object_detection::Detection
auv_inference_common::DetectionSet -> auv_task_object_detection::DetectionResult
auv_inference_common::DetectionOptions -> auv_task_object_detection::DetectionOptions
auv_inference_common::render_annotated_image -> auv_task_object_detection::render_annotated_image
```

Where a test fixture constructs old `DetectionSet { model_id, image_size, detections }`, replace it with:

```rust
DetectionResult {
  image_size,
  detections,
}
```

- [ ] **Step 4: Update JSON file readers**

In `crates/auv-game-balatro/src/card_detection_producer.rs`, replace `DetectionSet` reads with `DetectionResult` reads:

```rust
let ui_detections = read_json_file::<DetectionResult>(&ui_path, "balatro ui detection result")?;
let entities_detections =
  read_json_file::<DetectionResult>(&entities_path, "balatro entities detection result")?;
```

- [ ] **Step 5: Run Balatro tests**

Run:

```bash
cargo test -p auv-game-balatro
```

Expected: Balatro compiles with task detection types and no direct use of `UltralyticsDetector`.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-game-balatro
git commit -m "refactor(auv-game-balatro): consume object detection task crate"
```

---

### Task 6: Update osu Consumers

**Files:**
- Modify: `crates/auv-game-osu/Cargo.toml`
- Modify: `crates/auv-game-osu/src/dataset.rs`
- Modify: `crates/auv-game-osu/src/visual_eval.rs`
- Modify: `crates/auv-game-osu/src/benchmark.rs`
- Modify: `src/verticals/osu/mod.rs`

- [ ] **Step 1: Add task dependency**

In `crates/auv-game-osu/Cargo.toml`, add:

```toml
auv-task-object-detection = { path = "../auv-task-object-detection" }
```

- [ ] **Step 2: Update visual eval types**

In `crates/auv-game-osu/src/visual_eval.rs`, replace:

```rust
use auv_inference_common::{BoundingBox, DetectionSet};
```

with:

```rust
use auv_task_object_detection::{BoundingBox, DetectionResult};
```

Change `FrameDetections`:

```rust
pub struct FrameDetections {
  pub frame: FrameKey,
  pub detections: DetectionResult,
}

impl FrameDetections {
  pub fn new(frame: FrameKey, detections: DetectionResult) -> Self {
    Self { frame, detections }
  }
}
```

- [ ] **Step 3: Replace dataset coordinate-space dependency**

In `crates/auv-game-osu/src/dataset.rs`, remove `DetectionCoordinateSpace` from imports and add a local enum:

```rust
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetCoordinateSpace {
  SourceImagePixels,
}
```

Change `DatasetManifest`:

```rust
pub coordinate_space: DatasetCoordinateSpace,
```

Change manifest construction to:

```rust
coordinate_space: DatasetCoordinateSpace::SourceImagePixels,
```

- [ ] **Step 4: Update detection result construction**

In osu code and tests, replace:

```rust
DetectionSet {
  model_id: ModelId("...".to_string()),
  image_size,
  detections,
}
```

with:

```rust
DetectionResult {
  image_size,
  detections,
}
```

Remove unused `ModelId` imports from osu files.

- [ ] **Step 5: Simplify osu benchmark manifest handling**

In `crates/auv-game-osu/src/benchmark.rs`, remove `DetectionEvidenceManifest`, `ClassLabelSource`, and manifest-specific helper functions. Keep benchmark inputs as serialized `DetectionResult` files.

The read branch should become:

```rust
if let Ok(result) = read_json::<DetectionResult>(path) {
  detections_by_capture.insert(capture_file_name_from_detection_path(path)?, result);
  continue;
}
```

Implement `capture_file_name_from_detection_path` by using the file stem as the capture key:

```rust
fn capture_file_name_from_detection_path(path: &Path) -> OsuResult<String> {
  path
    .file_stem()
    .and_then(|stem| stem.to_str())
    .map(|stem| stem.to_string())
    .ok_or_else(|| format!("detection path has no UTF-8 file stem: {}", path.display()).into())
}
```

- [ ] **Step 6: Update root osu vertical test imports**

In `src/verticals/osu/mod.rs`, replace:

```rust
use auv_inference_common::{BoundingBox, Detection, DetectionSet, ImageSize, ModelId};
```

with:

```rust
use auv_task_object_detection::{BoundingBox, Detection, DetectionResult, ImageSize};
```

Change the fixture to construct `DetectionResult` without `model_id`.

- [ ] **Step 7: Add root task dependency if needed**

If `src/verticals/osu/mod.rs` imports `auv_task_object_detection`, add this to root `Cargo.toml` `[dependencies]`:

```toml
auv-task-object-detection = { path = "crates/auv-task-object-detection" }
```

Remove root `auv-inference-common` dependency if `rg "auv_inference_common" src Cargo.toml` shows no root-crate usage after `src/inference_recognition.rs` is deleted.

- [ ] **Step 8: Run osu tests**

Run:

```bash
cargo test -p auv-game-osu
cargo test --lib verticals::osu::tests::osu_detection_provider_projects_into_session_observation
```

Expected: osu crate and root osu vertical test compile with `DetectionResult`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml crates/auv-game-osu src/verticals/osu/mod.rs
git commit -m "refactor(auv-game-osu): consume object detection task result"
```

---

### Task 7: Delete Detector Manifest Recognition Bridge And Old Tests

**Files:**
- Delete: `src/inference_recognition.rs`
- Modify: `src/lib.rs`
- Delete: `crates/auv-inference-ultralytics/tests/detection_set_boundary.rs`
- Delete: `crates/auv-inference-ultralytics/tests/fixture_parity.rs`
- Delete: `crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs`
- Modify: `crates/auv-inference-ultralytics/examples/detect.rs`

- [ ] **Step 1: Delete root detector bridge module**

Delete:

```text
src/inference_recognition.rs
```

In `src/lib.rs`, remove:

```rust
pub mod inference_recognition;
```

- [ ] **Step 2: Delete old inference-ultralytics integration tests**

Delete:

```text
crates/auv-inference-ultralytics/tests/detection_set_boundary.rs
crates/auv-inference-ultralytics/tests/fixture_parity.rs
crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs
```

These tests validate the old `DetectionSet` and `DetectionEvidenceManifest` stack and the old root recognition bridge. The replacement coverage lives in `auv-task-object-detection`.

- [ ] **Step 3: Move example to task crate or rewrite it**

The existing `crates/auv-inference-ultralytics/examples/detect.rs` produces detection JSON and annotated output, so move it to:

```text
crates/auv-task-object-detection/examples/detect.rs
```

Update imports:

```rust
use auv_task_object_detection::{
  DetectionOptions, UltralyticsObjectDetector, UltralyticsObjectDetectorConfig,
  render_annotated_image,
};
use auv_inference_common::ModelId;
use auv_inference_ultralytics::InferenceDevice;
```

Update write function signature:

```rust
fn write_json(path: &Path, detections: &auv_task_object_detection::DetectionResult) -> ExampleResult<()> {
  let json = serde_json::to_string_pretty(detections)?;
  fs::write(path, format!("{json}\n"))?;
  Ok(())
}
```

- [ ] **Step 4: Remove old example if it no longer matches backend scope**

Delete:

```text
crates/auv-inference-ultralytics/examples/detect.rs
```

Keep backend examples out of this task unless they demonstrate `UltralyticsSession::predict_*` without producing task detection artifacts.

- [ ] **Step 5: Search for removed symbols**

Run:

```bash
rg "DetectionSet|DetectionEvidenceManifest|SourceImageEvidence|ProjectionBasis|DetectionCoordinateSpace|ClassLabelSource|ModelRunMetadata|UltralyticsDetector|inference_recognition|render_annotated_image" src crates -g '*.rs' -g 'Cargo.toml'
```

Expected: no matches for deleted symbols except unrelated `auv_driver::geometry::ProjectionBasis` and the new task crate `render_annotated_image`.

- [ ] **Step 6: Commit**

```bash
git rm src/inference_recognition.rs crates/auv-inference-ultralytics/tests/detection_set_boundary.rs crates/auv-inference-ultralytics/tests/fixture_parity.rs crates/auv-inference-ultralytics/tests/slay_the_spire_observe_only_boundary.rs crates/auv-inference-ultralytics/examples/detect.rs
git add src/lib.rs crates/auv-task-object-detection/examples
git commit -m "refactor(inference): remove detector manifest recognition bridge"
```

---

### Task 8: Workspace Validation And Reference Updates

**Files:**
- Inspect: `docs/ai/references/INDEX.md`
- Modify docs only if implementation changes the accepted crate names or task boundaries.

- [ ] **Step 1: Verify this plan is indexed**

Confirm `docs/ai/references/INDEX.md` contains this entry under `### misc` -> `#### plan`:

```markdown
- [`2026-07-07-inference-task-object-detection-simplification-plan.md`](2026-07-07-inference-task-object-detection-simplification-plan.md)
```

If the entry is missing, add it and update the reference counts in the same file.

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo fmt --check
```

Expected: pass. If it fails, run `cargo fmt`, inspect the diff, and rerun `cargo fmt --check`.

- [ ] **Step 3: Run focused tests**

Run:

```bash
cargo test -p auv-inference-common
cargo test -p auv-inference-ultralytics
cargo test -p auv-task-object-detection --features ultralytics
cargo test -p auv-game-balatro
cargo test -p auv-game-osu
```

Expected: all pass.

- [ ] **Step 4: Run workspace check**

Run:

```bash
cargo check
```

Expected: pass.

- [ ] **Step 5: Verify removed dependency direction**

Run:

```bash
rg "auv-cli|auv_cli" crates/auv-inference-common crates/auv-inference-ort crates/auv-inference-ultralytics -n
```

Expected: no output.

- [ ] **Step 6: Verify removed wrapper symbols**

Run:

```bash
rg "DetectionEvidenceManifest|SourceImageEvidence|DetectionCoordinateSpace|ClassLabelSource|ModelRunMetadata|UltralyticsDetector|inference_recognition" src crates -g '*.rs' -g 'Cargo.toml'
```

Expected: no output.

- [ ] **Step 7: Run diff whitespace check**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 8: Commit validation/docs updates if the index changed**

```bash
git add docs/ai/references/INDEX.md
git commit -m "docs(inference): add object detection split plan"
```

If `INDEX.md` did not change during implementation, skip this commit and include the final validation results in the handoff.
