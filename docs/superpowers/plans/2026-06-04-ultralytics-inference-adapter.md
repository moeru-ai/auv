# Ultralytics Inference Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the custom `auv-inference-yolo` backend with `auv-inference-common` plus an `ultralytics-inference` adapter while preserving Balatro parity and artifact output.

**Architecture:** `auv-inference-common` owns stable AUV result types and rendering. `auv-inference-ultralytics` owns `ultralytics-inference`, maps AUV config/device/options into upstream config, converts upstream `Results` into common `DetectionSet`, and exposes path/frame detection APIs. The old custom YOLO crate is removed after parity and example coverage move to the adapter.

**Tech Stack:** Rust 2024, `ultralytics-inference 0.0.18`, `image`, `serde`, `serde_json`, `thiserror`, existing Balatro ONNX fixtures.

---

## Scope And File Map

This plan implements `docs/superpowers/specs/2026-06-04-ultralytics-inference-adapter-design.md`.

**Create:**
- `crates/auv-inference-common/Cargo.toml` - common inference crate manifest.
- `crates/auv-inference-common/src/lib.rs` - common API exports.
- `crates/auv-inference-common/src/error.rs` - backend-neutral inference errors.
- `crates/auv-inference-common/src/render.rs` - annotated image artifact rendering.
- `crates/auv-inference-common/src/types.rs` - common inference domain types.
- `crates/auv-inference-ultralytics/Cargo.toml` - adapter manifest and feature map.
- `crates/auv-inference-ultralytics/src/lib.rs` - adapter API exports.
- `crates/auv-inference-ultralytics/src/convert.rs` - conversion from upstream results to common types.
- `crates/auv-inference-ultralytics/src/detector.rs` - `UltralyticsDetector`.
- `crates/auv-inference-ultralytics/src/device.rs` - AUV-owned device enum/parser and upstream mapping.
- `crates/auv-inference-ultralytics/examples/detect.rs` - callable detector example.
- `crates/auv-inference-ultralytics/tests/fixture_parity.rs` - Balatro parity tests.
- `crates/auv-inference-ultralytics/tests/fixtures/balatro/` - moved Balatro fixtures.

**Modify:**
- `Cargo.toml` - workspace members and dependencies.
- `Cargo.lock` - dependency graph.
- `scripts/generate-balatro-yolo-fixtures.py` - keep script but write fixtures to the new adapter crate.

**Delete:**
- `crates/auv-inference-yolo/` - remove custom backend, tests, and example after replacement coverage exists.

**Do not modify:**
- `src/catalog.rs`, runtime dispatch, `auv-driver`, `auv-driver-macos`, `auv-overlay-macos`, `auv-game-balatro`, OCR code, Steam automation, Swift overlay code.

## Task 1: Add `auv-inference-common`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-inference-common/Cargo.toml`
- Create: `crates/auv-inference-common/src/lib.rs`
- Create: `crates/auv-inference-common/src/error.rs`
- Create: `crates/auv-inference-common/src/types.rs`

- [ ] **Step 1: Add the workspace member**

Modify root `Cargo.toml`:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-inference-common",
  "crates/auv-inference-yolo",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
  "crates/auv-view",
]
```

- [ ] **Step 2: Create the common crate manifest**

Create `crates/auv-inference-common/Cargo.toml`:

```toml
[package]
name = "auv-inference-common"
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
image.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 3: Create common errors**

Create `crates/auv-inference-common/src/error.rs`:

```rust
use std::path::PathBuf;

pub type InferenceResult<T> = Result<T, InferenceError>;

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
  #[error("model file does not exist: {}", path.display())]
  MissingModel { path: PathBuf },
  #[error("class list must not be empty")]
  EmptyClassList,
  #[error("{name} threshold must be finite and between 0 and 1, got {value}")]
  InvalidThreshold { name: &'static str, value: f32 },
  #[error("input size must be greater than zero, got {input_size}")]
  InvalidInputSize { input_size: u32 },
  #[error("max detections must be greater than zero, got {max_detections}")]
  InvalidMaxDetections { max_detections: usize },
  #[error("image dimensions must be greater than zero, got {width}x{height}")]
  InvalidImageSize { width: u32, height: u32 },
  #[error("detector session is unavailable: {reason}")]
  SessionUnavailable { reason: String },
  #[error("backend returned no detection result")]
  MissingResult,
  #[error("backend result does not contain detection boxes")]
  MissingBoxes,
  #[error("backend class id {class_id} has no label")]
  MissingClassLabel { class_id: usize },
  #[error("failed to decode image: {source}")]
  ImageDecode {
    #[from]
    source: image::ImageError,
  },
  #[error("backend error: {message}")]
  Backend { message: String },
  #[error("I/O error: {source}")]
  Io {
    #[from]
    source: std::io::Error,
  },
  #[error("JSON error: {source}")]
  Json {
    #[from]
    source: serde_json::Error,
  },
}
```

- [ ] **Step 4: Create common types**

Create `crates/auv-inference-common/src/types.rs`:

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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  pub bbox: BoundingBox,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DetectionSet {
  pub model_id: ModelId,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub input_size: Option<u32>,
}
```

- [ ] **Step 5: Create common exports and tests**

Create `crates/auv-inference-common/src/lib.rs`:

```rust
pub mod error;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use types::{
  BoundingBox, Detection, DetectionOptions, DetectionSet, ImageFrame, ImageSize, ModelConfig,
  ModelId,
};

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detection_options_default_matches_balatro_fixture_thresholds() {
    let options = DetectionOptions::default();

    assert_eq!(options.confidence_threshold, 0.25);
    assert_eq!(options.iou_threshold, 0.45);
    assert_eq!(options.max_detections, 300);
  }

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

- [ ] **Step 6: Verify the common crate**

Run:

```bash
cargo test -p auv-inference-common
cargo fmt --check
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 7: Commit common crate**

Run:

```bash
git add Cargo.toml crates/auv-inference-common
git commit -m "feat(auv-inference-common): add inference result types"
```

If signing hangs, use:

```bash
git -c commit.gpgsign=false commit -m "feat(auv-inference-common): add inference result types"
```

## Task 2: Move Rendering To Common

**Files:**
- Create: `crates/auv-inference-common/src/render.rs`
- Modify: `crates/auv-inference-common/src/lib.rs`

- [ ] **Step 1: Add renderer tests and implementation**

Create `crates/auv-inference-common/src/render.rs`:

```rust
use crate::{BoundingBox, Detection};
use image::{Rgb, RgbImage};

pub fn render_annotated_image(image: &RgbImage, detections: &[Detection]) -> RgbImage {
  let mut annotated = image.clone();
  if annotated.width() == 0 || annotated.height() == 0 {
    return annotated;
  }

  for detection in detections {
    let Some((x1, y1, x2, y2)) =
      clamped_bbox(detection.bbox, annotated.width(), annotated.height())
    else {
      continue;
    };
    let color = class_color(detection.class_id);
    for x in x1..=x2 {
      annotated.put_pixel(x, y1, color);
      annotated.put_pixel(x, y2, color);
    }
    for y in y1..=y2 {
      annotated.put_pixel(x1, y, color);
      annotated.put_pixel(x2, y, color);
    }
  }

  annotated
}

fn clamped_bbox(
  bbox: BoundingBox,
  image_width: u32,
  image_height: u32,
) -> Option<(u32, u32, u32, u32)> {
  if !bbox.x1.is_finite() || !bbox.y1.is_finite() || !bbox.x2.is_finite() || !bbox.y2.is_finite() {
    return None;
  }
  let min_x = bbox
    .x1
    .min(bbox.x2)
    .floor()
    .clamp(0.0, (image_width - 1) as f32) as u32;
  let max_x = bbox
    .x1
    .max(bbox.x2)
    .ceil()
    .clamp(0.0, (image_width - 1) as f32) as u32;
  let min_y = bbox
    .y1
    .min(bbox.y2)
    .floor()
    .clamp(0.0, (image_height - 1) as f32) as u32;
  let max_y = bbox
    .y1
    .max(bbox.y2)
    .ceil()
    .clamp(0.0, (image_height - 1) as f32) as u32;
  Some((min_x, min_y, max_x, max_y))
}

fn class_color(class_id: usize) -> Rgb<u8> {
  const PALETTE: [Rgb<u8>; 12] = [
    Rgb([230, 25, 75]),
    Rgb([60, 180, 75]),
    Rgb([0, 130, 200]),
    Rgb([245, 130, 48]),
    Rgb([145, 30, 180]),
    Rgb([70, 240, 240]),
    Rgb([240, 50, 230]),
    Rgb([210, 245, 60]),
    Rgb([250, 190, 190]),
    Rgb([0, 128, 128]),
    Rgb([230, 190, 255]),
    Rgb([170, 110, 40]),
  ];
  PALETTE[class_id % PALETTE.len()]
}

#[cfg(test)]
mod tests {
  use super::*;

  fn detection(class_id: usize, bbox: BoundingBox) -> Detection {
    Detection {
      class_id,
      label: format!("class-{class_id}"),
      confidence: 0.9,
      bbox,
    }
  }

  #[test]
  fn render_changes_bbox_border_and_preserves_background() {
    let source = RgbImage::from_pixel(8, 8, Rgb([8, 9, 10]));
    let rendered = render_annotated_image(
      &source,
      &[detection(
        1,
        BoundingBox {
          x1: 2.0,
          y1: 2.0,
          x2: 5.0,
          y2: 5.0,
        },
      )],
    );

    assert_ne!(rendered.get_pixel(2, 2), source.get_pixel(2, 2));
    assert_eq!(rendered.get_pixel(0, 0), source.get_pixel(0, 0));
  }

  #[test]
  fn render_clamps_bbox_to_image_bounds() {
    let source = RgbImage::from_pixel(3, 3, Rgb([20, 30, 40]));
    let rendered = render_annotated_image(
      &source,
      &[detection(
        2,
        BoundingBox {
          x1: -5.0,
          y1: -4.0,
          x2: 8.0,
          y2: 7.0,
        },
      )],
    );

    assert_ne!(rendered.get_pixel(0, 0), source.get_pixel(0, 0));
    assert_ne!(rendered.get_pixel(2, 2), source.get_pixel(2, 2));
  }
}
```

- [ ] **Step 2: Export the renderer**

Modify `crates/auv-inference-common/src/lib.rs`:

```rust
pub mod error;
pub mod render;
pub mod types;

pub use error::{InferenceError, InferenceResult};
pub use render::render_annotated_image;
pub use types::{
  BoundingBox, Detection, DetectionOptions, DetectionSet, ImageFrame, ImageSize, ModelConfig,
  ModelId,
};
```

Keep the existing tests from Task 1 below these exports.

- [ ] **Step 3: Verify rendering**

Run:

```bash
cargo test -p auv-inference-common render -- --nocapture
cargo test -p auv-inference-common
cargo fmt --check
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 4: Commit renderer**

Run:

```bash
git add crates/auv-inference-common
git commit -m "feat(auv-inference-common): add annotated rendering"
```

## Task 3: Add `auv-inference-ultralytics` Scaffold And Features

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-inference-ultralytics/Cargo.toml`
- Create: `crates/auv-inference-ultralytics/src/lib.rs`
- Create: `crates/auv-inference-ultralytics/src/device.rs`

- [ ] **Step 1: Add workspace dependency and member**

Modify root `Cargo.toml`:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-inference-common",
  "crates/auv-inference-ultralytics",
  "crates/auv-inference-yolo",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
  "crates/auv-view",
]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
image = { version = "0.25", default-features = false, features = ["png"] }
ndarray = "0.17"
ort = "2.0.0-rc.10"
ultralytics-inference = { version = "0.0.18", default-features = false }
xcap = "0.6"
swift-bridge = "0.1.59"
swift-bridge-build = "0.1.59"
objc2-core-graphics = { version = "0.3", features = ["CGWindow", "CGImage", "CGDataProvider", "CGGeometry"] }
objc2-core-foundation = "0.3"
```

- [ ] **Step 2: Create adapter manifest**

Create `crates/auv-inference-ultralytics/Cargo.toml`:

```toml
[package]
name = "auv-inference-ultralytics"
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

[dependencies]
auv-inference-common = { path = "../auv-inference-common" }
image = { workspace = true, features = ["jpeg"] }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
ultralytics-inference.workspace = true
```

- [ ] **Step 3: Create AUV-owned device mapping**

Create `crates/auv-inference-ultralytics/src/device.rs`:

```rust
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InferenceDevice {
  Cpu,
  Cuda(usize),
  CoreMl,
  DirectMl(usize),
  OpenVino,
  Xnnpack,
  TensorRt(usize),
  Rocm(usize),
}

impl Default for InferenceDevice {
  fn default() -> Self {
    Self::Cpu
  }
}

impl FromStr for InferenceDevice {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let value = value.to_lowercase();
    if let Some(rest) = value.strip_prefix("cuda") {
      return Ok(Self::Cuda(parse_device_index(rest)));
    }
    if let Some(rest) = value.strip_prefix("directml") {
      return Ok(Self::DirectMl(parse_device_index(rest)));
    }
    if let Some(rest) = value.strip_prefix("tensorrt") {
      return Ok(Self::TensorRt(parse_device_index(rest)));
    }
    if let Some(rest) = value.strip_prefix("rocm") {
      return Ok(Self::Rocm(parse_device_index(rest)));
    }
    match value.as_str() {
      "cpu" => Ok(Self::Cpu),
      "coreml" => Ok(Self::CoreMl),
      "openvino" => Ok(Self::OpenVino),
      "xnnpack" => Ok(Self::Xnnpack),
      _ => Err(format!("unknown inference device: {value}")),
    }
  }
}

impl From<InferenceDevice> for ultralytics_inference::Device {
  fn from(value: InferenceDevice) -> Self {
    match value {
      InferenceDevice::Cpu => Self::Cpu,
      InferenceDevice::Cuda(index) => Self::Cuda(index),
      InferenceDevice::CoreMl => Self::CoreMl,
      InferenceDevice::DirectMl(index) => Self::DirectMl(index),
      InferenceDevice::OpenVino => Self::OpenVino,
      InferenceDevice::Xnnpack => Self::Xnnpack,
      InferenceDevice::TensorRt(index) => Self::TensorRt(index),
      InferenceDevice::Rocm(index) => Self::Rocm(index),
    }
  }
}

fn parse_device_index(value: &str) -> usize {
  value
    .strip_prefix(':')
    .and_then(|index| index.parse::<usize>().ok())
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_cpu_and_indexed_gpu_devices() {
    assert_eq!("cpu".parse::<InferenceDevice>().unwrap(), InferenceDevice::Cpu);
    assert_eq!(
      "cuda:1".parse::<InferenceDevice>().unwrap(),
      InferenceDevice::Cuda(1)
    );
    assert_eq!(
      "tensorrt".parse::<InferenceDevice>().unwrap(),
      InferenceDevice::TensorRt(0)
    );
  }

  #[test]
  fn rejects_unknown_devices() {
    assert!("mps".parse::<InferenceDevice>().is_err());
  }
}
```

- [ ] **Step 4: Create adapter exports**

Create `crates/auv-inference-ultralytics/src/lib.rs`:

```rust
pub mod device;

pub use device::InferenceDevice;
```

- [ ] **Step 5: Verify feature scaffold**

Run:

```bash
cargo check -p auv-inference-ultralytics --no-default-features
cargo check -p auv-inference-ultralytics
cargo test -p auv-inference-ultralytics device -- --nocapture
cargo fmt --check
git diff --check
```

Expected: all commands PASS. If `cargo check --no-default-features` fails because `ultralytics-inference` requires a default feature for detection, record the exact feature needed and add only that feature to `auv-inference-ultralytics/default`.

- [ ] **Step 6: Commit adapter scaffold**

Run:

```bash
git add Cargo.toml Cargo.lock crates/auv-inference-ultralytics
git commit -m "feat(auv-inference-ultralytics): scaffold adapter crate"
```

## Task 4: Implement Ultralytics Detector And Conversion

**Files:**
- Create: `crates/auv-inference-ultralytics/src/convert.rs`
- Create: `crates/auv-inference-ultralytics/src/detector.rs`
- Modify: `crates/auv-inference-ultralytics/src/lib.rs`

- [ ] **Step 1: Write conversion and validation tests**

Create `crates/auv-inference-ultralytics/src/convert.rs`:

```rust
use auv_inference_common::{
  BoundingBox, Detection, DetectionSet, ImageSize, InferenceError, InferenceResult, ModelId,
};
use ultralytics_inference::Results;

pub fn detection_set_from_result(
  model_id: ModelId,
  result: &Results,
  class_names_override: Option<&[String]>,
) -> InferenceResult<DetectionSet> {
  let boxes = result.boxes.as_ref().ok_or(InferenceError::MissingBoxes)?;
  let xyxy = boxes.xyxy();
  let conf = boxes.conf();
  let cls = boxes.cls();
  let mut detections = Vec::with_capacity(boxes.len());
  for index in 0..boxes.len() {
    let class_id = cls[index] as usize;
    let label = class_label(class_id, class_names_override, result)?;
    detections.push(Detection {
      class_id,
      label,
      confidence: conf[index],
      bbox: BoundingBox {
        x1: xyxy[[index, 0]],
        y1: xyxy[[index, 1]],
        x2: xyxy[[index, 2]],
        y2: xyxy[[index, 3]],
      },
    });
  }
  Ok(DetectionSet {
    model_id,
    image_size: ImageSize {
      width: result.orig_shape.1,
      height: result.orig_shape.0,
    },
    detections,
  })
}

fn class_label(
  class_id: usize,
  class_names_override: Option<&[String]>,
  result: &Results,
) -> InferenceResult<String> {
  if let Some(class_names) = class_names_override {
    return class_names
      .get(class_id)
      .cloned()
      .ok_or(InferenceError::MissingClassLabel { class_id });
  }
  result
    .names
    .get(&class_id)
    .cloned()
    .ok_or(InferenceError::MissingClassLabel { class_id })
}
```

The conversion is covered by Balatro parity in Task 5 because constructing upstream `Results` and `Boxes` directly would duplicate upstream internals. The `class_names_override` path is required so exported Balatro class files remain an explicit API input when ONNX metadata is absent or stale. Do not create a fake upstream result test unless the upstream constructors are public and simple.

- [ ] **Step 2: Implement detector**

Create `crates/auv-inference-ultralytics/src/detector.rs`:

```rust
use crate::{convert::detection_set_from_result, InferenceDevice};
use auv_inference_common::{
  DetectionOptions, DetectionSet, ImageFrame, InferenceError, InferenceResult, ModelConfig, ModelId,
};
use image::DynamicImage;
use std::{path::Path, sync::Mutex};
use ultralytics_inference::{InferenceConfig, YOLOModel};

#[derive(Clone, Debug)]
pub struct UltralyticsModelConfig {
  pub model_id: ModelId,
  pub model_path: std::path::PathBuf,
  pub input_size: Option<u32>,
  pub options: DetectionOptions,
  pub device: InferenceDevice,
  pub class_names_override: Option<Vec<String>>,
}

impl From<ModelConfig> for UltralyticsModelConfig {
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

pub struct UltralyticsDetector {
  model_id: ModelId,
  class_names_override: Option<Vec<String>>,
  model: Mutex<YOLOModel>,
}

impl UltralyticsDetector {
  pub fn load(config: UltralyticsModelConfig) -> InferenceResult<Self> {
    validate_model_config(&config)?;
    validate_options(config.options)?;
    validate_class_names(config.class_names_override.as_deref())?;
    let inference_config =
      build_inference_config(config.options, config.input_size, config.device.clone());
    let model = YOLOModel::load_with_config(&config.model_path, inference_config)
      .map_err(|error| InferenceError::Backend {
        message: error.to_string(),
      })?;
    Ok(Self {
      model_id: config.model_id,
      class_names_override: config.class_names_override,
      model: Mutex::new(model),
    })
  }

  pub fn detect_path(&self, path: impl AsRef<Path>) -> InferenceResult<DetectionSet> {
    let mut model = self.model.lock().map_err(|_| InferenceError::SessionUnavailable {
      reason: "ultralytics model mutex was poisoned".to_string(),
    })?;
    let results = model.predict(path.as_ref()).map_err(|error| InferenceError::Backend {
      message: error.to_string(),
    })?;
    let result = results.first().ok_or(InferenceError::MissingResult)?;
    detection_set_from_result(
      self.model_id.clone(),
      result,
      self.class_names_override.as_deref(),
    )
  }

  pub fn detect_frame(&self, frame: &ImageFrame) -> InferenceResult<DetectionSet> {
    validate_frame(frame)?;
    let mut model = self.model.lock().map_err(|_| InferenceError::SessionUnavailable {
      reason: "ultralytics model mutex was poisoned".to_string(),
    })?;
    let image = DynamicImage::ImageRgb8(frame.image.clone());
    let results = model
      .predict_image(&image, "memory".to_string())
      .map_err(|error| InferenceError::Backend {
      message: error.to_string(),
    })?;
    let result = results.first().ok_or(InferenceError::MissingResult)?;
    detection_set_from_result(
      self.model_id.clone(),
      result,
      self.class_names_override.as_deref(),
    )
  }
}

fn build_inference_config(
  options: DetectionOptions,
  input_size: Option<u32>,
  device: InferenceDevice,
) -> InferenceConfig {
  let mut config = InferenceConfig::new()
    .with_confidence(options.confidence_threshold)
    .with_iou(options.iou_threshold)
    .with_max_det(options.max_detections)
    .with_device(device.into())
    .with_save(false);
  if let Some(input_size) = input_size {
    config = config.with_imgsz(input_size as usize, input_size as usize);
  }
  config
}

fn validate_model_config(config: &UltralyticsModelConfig) -> InferenceResult<()> {
  if let Some(input_size) = config.input_size {
    if input_size == 0 {
      return Err(InferenceError::InvalidInputSize { input_size });
    }
  }
  if !config.model_path.exists() {
    return Err(InferenceError::MissingModel {
      path: config.model_path.clone(),
    });
  }
  Ok(())
}

fn validate_options(options: DetectionOptions) -> InferenceResult<()> {
  validate_threshold("confidence", options.confidence_threshold)?;
  validate_threshold("iou", options.iou_threshold)?;
  if options.max_detections == 0 {
    return Err(InferenceError::InvalidMaxDetections {
      max_detections: options.max_detections,
    });
  }
  Ok(())
}

fn validate_class_names(class_names: Option<&[String]>) -> InferenceResult<()> {
  if matches!(class_names, Some(class_names) if class_names.is_empty()) {
    return Err(InferenceError::EmptyClassList);
  }
  Ok(())
}

fn validate_threshold(name: &'static str, value: f32) -> InferenceResult<()> {
  if !value.is_finite() || !(0.0..=1.0).contains(&value) {
    return Err(InferenceError::InvalidThreshold { name, value });
  }
  Ok(())
}

fn validate_frame(frame: &ImageFrame) -> InferenceResult<()> {
  let size = frame.size();
  if size.width == 0 || size.height == 0 {
    return Err(InferenceError::InvalidImageSize {
      width: size.width,
      height: size.height,
    });
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn load_rejects_missing_model_before_backend_load() {
    let error = UltralyticsDetector::load(UltralyticsModelConfig {
      model_id: ModelId("missing".to_string()),
      model_path: "missing.onnx".into(),
      input_size: Some(640),
      options: DetectionOptions::default(),
      device: InferenceDevice::Cpu,
      class_names_override: None,
    })
    .unwrap_err();

    assert!(matches!(error, InferenceError::MissingModel { .. }));
  }

  #[test]
  fn load_rejects_zero_input_size() {
    let error = UltralyticsDetector::load(UltralyticsModelConfig {
      model_id: ModelId("missing".to_string()),
      model_path: "missing.onnx".into(),
      input_size: Some(0),
      options: DetectionOptions::default(),
      device: InferenceDevice::Cpu,
      class_names_override: None,
    })
    .unwrap_err();

    assert!(matches!(error, InferenceError::InvalidInputSize { input_size: 0 }));
  }

  #[test]
  fn validate_options_rejects_nan_confidence() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: f32::NAN,
      iou_threshold: 0.45,
      max_detections: 300,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      InferenceError::InvalidThreshold {
        name: "confidence",
        value
      } if value.is_nan()
    ));
  }

  #[test]
  fn validate_options_rejects_zero_max_detections() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
      max_detections: 0,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      InferenceError::InvalidMaxDetections { max_detections: 0 }
    ));
  }
}
```

- [ ] **Step 3: Export detector and conversion modules**

Modify `crates/auv-inference-ultralytics/src/lib.rs`:

```rust
pub mod convert;
pub mod detector;
pub mod device;

pub use detector::{UltralyticsDetector, UltralyticsModelConfig};
pub use device::InferenceDevice;
```

- [ ] **Step 4: Verify detector validation**

Run:

```bash
cargo test -p auv-inference-ultralytics detector -- --nocapture
cargo check -p auv-inference-ultralytics
cargo fmt --check
git diff --check
```

Expected: all commands PASS. If `with_save(false)` does not compile, remove only that call and keep the rest of the config unchanged.

- [ ] **Step 5: Commit detector**

Run:

```bash
git add crates/auv-inference-ultralytics
git commit -m "feat(auv-inference-ultralytics): add detector adapter"
```

## Task 5: Port Balatro Fixtures And Parity Tests

**Files:**
- Modify: `scripts/generate-balatro-yolo-fixtures.py`
- Create: `crates/auv-inference-ultralytics/tests/fixture_parity.rs`
- Move: `crates/auv-inference-yolo/tests/fixtures/balatro/*` to `crates/auv-inference-ultralytics/tests/fixtures/balatro/*`

- [ ] **Step 1: Move fixture files**

Run:

```bash
mkdir -p crates/auv-inference-ultralytics/tests/fixtures/balatro
git mv crates/auv-inference-yolo/tests/fixtures/balatro/balatro.jpg crates/auv-inference-ultralytics/tests/fixtures/balatro/balatro.jpg
git mv crates/auv-inference-yolo/tests/fixtures/balatro/entities.json crates/auv-inference-ultralytics/tests/fixtures/balatro/entities.json
git mv crates/auv-inference-yolo/tests/fixtures/balatro/ui.json crates/auv-inference-ultralytics/tests/fixtures/balatro/ui.json
```

- [ ] **Step 2: Update fixture generator output path**

Modify `scripts/generate-balatro-yolo-fixtures.py` so the output directory points to the adapter crate:

```python
OUT_DIR = AUV_ROOT / 'crates/auv-inference-ultralytics/tests/fixtures/balatro'
```

Keep all existing model paths, class paths, thresholds, and fixture metadata intact.

- [ ] **Step 3: Create parity test**

Create `crates/auv-inference-ultralytics/tests/fixture_parity.rs`:

```rust
use auv_inference_common::{Detection, DetectionOptions, ModelId};
use auv_inference_ultralytics::{
  InferenceDevice, UltralyticsDetector, UltralyticsModelConfig,
};
use serde::Deserialize;
use std::{
  error::Error,
  fs,
  path::{Path, PathBuf},
};

const BALATRO_REPO: &str = "/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro";
const INPUT_SIZE: u32 = 640;
const CONFIDENCE_TOLERANCE: f32 = 0.01;
const BBOX_TOLERANCE: f32 = 3.0;

#[derive(Debug, Deserialize)]
struct Fixture {
  classes: FixtureClasses,
  detection_count: usize,
  detections: Vec<FixtureDetection>,
  image: Option<FixtureImage>,
  model: FixtureModel,
  thresholds: FixtureThresholds,
}

impl Fixture {
  fn model_path(&self, balatro_repo: &Path) -> PathBuf {
    balatro_repo.join(&self.model.balatro_asset)
  }
}

#[derive(Debug, Deserialize)]
struct FixtureClasses {
  #[serde(rename = "balatro_asset")]
  balatro_asset: PathBuf,
  count: usize,
  labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FixtureDetection {
  class_id: usize,
  label: String,
  confidence: f32,
  bbox: [f32; 4],
}

#[derive(Debug, Deserialize)]
struct FixtureImage {
  height: u32,
  width: u32,
}

#[derive(Debug, Deserialize)]
struct FixtureModel {
  #[serde(rename = "balatro_asset")]
  balatro_asset: PathBuf,
  name: String,
}

#[derive(Debug, Deserialize)]
struct FixtureThresholds {
  confidence: f32,
  iou: f32,
}

#[test]
fn balatro_fixtures_match_ultralytics_adapter() -> Result<(), Box<dyn Error>> {
  let balatro_repo = Path::new(BALATRO_REPO);
  if !balatro_repo.exists() {
    eprintln!("skipping Balatro fixture parity: {BALATRO_REPO} does not exist");
    return Ok(());
  }

  let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro");
  let image_path = fixture_dir.join("balatro.jpg");
  for fixture_name in ["entities", "ui"] {
    assert_fixture_matches(&fixture_dir, fixture_name, balatro_repo, &image_path)?;
  }
  Ok(())
}

fn assert_fixture_matches(
  fixture_dir: &Path,
  fixture_name: &str,
  balatro_repo: &Path,
  image_path: &Path,
) -> Result<(), Box<dyn Error>> {
  let fixture = load_fixture(&fixture_dir.join(format!("{fixture_name}.json")))?;
  assert_eq!(fixture.detection_count, fixture.detections.len());
  assert_eq!(fixture.classes.count, fixture.classes.labels.len());

  let model_path = fixture.model_path(balatro_repo);
  assert!(
    model_path.exists(),
    "{fixture_name} fixture model path is missing: {}",
    model_path.display()
  );

  let detector = UltralyticsDetector::load(UltralyticsModelConfig {
    model_id: ModelId(fixture.model.name.clone()),
    model_path,
    input_size: Some(INPUT_SIZE),
    options: DetectionOptions {
      confidence_threshold: fixture.thresholds.confidence,
      iou_threshold: fixture.thresholds.iou,
      max_detections: 300,
    },
    device: InferenceDevice::Cpu,
    class_names_override: Some(load_class_names(balatro_repo.join(&fixture.classes.balatro_asset))?),
  })?;
  let result = detector.detect_path(image_path)?;

  if let Some(image) = &fixture.image {
    assert_eq!(result.image_size.width, image.width);
    assert_eq!(result.image_size.height, image.height);
  }
  assert_eq!(
    result.detections.len(),
    fixture.detections.len(),
    "{fixture_name} detection count differs\nexpected: {}\nactual: {}",
    summarize_fixture_detections(&fixture.detections),
    summarize_detections(&result.detections)
  );
  assert_detection_set_matches(fixture_name, &fixture.detections, &result.detections);
  Ok(())
}

fn load_fixture(path: &Path) -> Result<Fixture, Box<dyn Error>> {
  Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn load_class_names(path: PathBuf) -> Result<Vec<String>, Box<dyn Error>> {
  Ok(fs::read_to_string(path)?
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(ToOwned::to_owned)
    .collect())
}

fn assert_detection_set_matches(
  fixture_name: &str,
  expected: &[FixtureDetection],
  actual: &[Detection],
) {
  let mut unmatched_actual = vec![true; actual.len()];
  for (expected_index, expected_detection) in expected.iter().enumerate() {
    let Some(actual_index) = actual.iter().enumerate().position(|(actual_index, detection)| {
      unmatched_actual[actual_index] && detection_matches(expected_detection, detection)
    }) else {
      panic!(
        "{fixture_name} detection {expected_index} had no matching actual detection: expected {} actual {}",
        summarize_fixture_detection(expected_detection),
        summarize_detections(actual)
      );
    };
    unmatched_actual[actual_index] = false;
  }
}

fn detection_matches(expected: &FixtureDetection, actual: &Detection) -> bool {
  actual.class_id == expected.class_id
    && actual.label == expected.label
    && (actual.confidence - expected.confidence).abs() < CONFIDENCE_TOLERANCE
    && bbox_matches(expected.bbox, actual)
}

fn bbox_matches(expected: [f32; 4], actual: &Detection) -> bool {
  let actual_bbox = [
    actual.bbox.x1,
    actual.bbox.y1,
    actual.bbox.x2,
    actual.bbox.y2,
  ];
  expected
    .into_iter()
    .zip(actual_bbox)
    .all(|(expected, actual)| (actual - expected).abs() < BBOX_TOLERANCE)
}

fn summarize_fixture_detection(detection: &FixtureDetection) -> String {
  format!(
    "{}:{}:{:.6}:{:?}",
    detection.class_id, detection.label, detection.confidence, detection.bbox
  )
}

fn summarize_fixture_detections(detections: &[FixtureDetection]) -> String {
  detections
    .iter()
    .map(summarize_fixture_detection)
    .collect::<Vec<_>>()
    .join(", ")
}

fn summarize_detections(detections: &[Detection]) -> String {
  detections
    .iter()
    .map(|detection| {
      format!(
        "{}:{}:{:.6}:[{:.3}, {:.3}, {:.3}, {:.3}]",
        detection.class_id,
        detection.label,
        detection.confidence,
        detection.bbox.x1,
        detection.bbox.y1,
        detection.bbox.x2,
        detection.bbox.y2
      )
    })
    .collect::<Vec<_>>()
    .join(", ")
}
```

- [ ] **Step 4: Run parity test**

Run:

```bash
cargo test -p auv-inference-ultralytics --test fixture_parity -- --nocapture
```

Expected: PASS. If parity fails because Ultralytics official preprocessing/NMS differs from the old Python fixture generator, update `scripts/generate-balatro-yolo-fixtures.py` to generate fixture JSON using `ultralytics-inference` behavior through the adapter example output, then rerun parity. Do not reintroduce custom YOLO postprocessing.

- [ ] **Step 5: Verify adapter tests**

Run:

```bash
cargo test -p auv-inference-ultralytics
cargo fmt --check
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 6: Commit parity port**

Run:

```bash
git add scripts/generate-balatro-yolo-fixtures.py crates/auv-inference-ultralytics crates/auv-inference-yolo/tests/fixtures
git commit -m "test(auv-inference-ultralytics): verify balatro parity"
```

## Task 6: Add Callable Example

**Files:**
- Create: `crates/auv-inference-ultralytics/examples/detect.rs`

- [ ] **Step 1: Create example**

Create `crates/auv-inference-ultralytics/examples/detect.rs`:

```rust
use auv_inference_common::{render_annotated_image, DetectionOptions, ModelId};
use auv_inference_ultralytics::{
  InferenceDevice, UltralyticsDetector, UltralyticsModelConfig,
};
use image::ImageReader;
use std::{
  env,
  error::Error,
  ffi::OsString,
  fs,
  path::PathBuf,
};

const DEFAULT_INPUT_SIZE: u32 = 640;

#[derive(Debug)]
struct Args {
  model: PathBuf,
  classes: Option<PathBuf>,
  image: PathBuf,
  json_out: PathBuf,
  annotated_out: Option<PathBuf>,
  confidence: f32,
  iou: f32,
  max_detections: usize,
  input_size: Option<u32>,
  device: InferenceDevice,
}

fn main() -> Result<(), Box<dyn Error>> {
  let args = Args::parse(env::args_os().skip(1))?;
  let detector = UltralyticsDetector::load(UltralyticsModelConfig {
    model_id: model_id(&args.model),
    model_path: args.model,
    options: DetectionOptions {
      confidence_threshold: args.confidence,
      iou_threshold: args.iou,
      max_detections: args.max_detections,
    },
    input_size: args.input_size,
    device: args.device,
    class_names_override: args.classes.map(load_class_names).transpose()?,
  })?;
  let detections = detector.detect_path(&args.image)?;
  fs::write(&args.json_out, serde_json::to_vec_pretty(&detections)?)?;
  if let Some(path) = &args.annotated_out {
    let image = ImageReader::open(&args.image)?.decode()?.to_rgb8();
    render_annotated_image(&image, &detections.detections).save(path)?;
  }
  println!(
    "wrote {} detections to {}",
    detections.detections.len(),
    args.json_out.display()
  );
  Ok(())
}

impl Args {
  fn parse(mut values: impl Iterator<Item = OsString>) -> Result<Self, Box<dyn Error>> {
    let mut model = None;
    let mut classes = None;
    let mut image = None;
    let mut json_out = None;
    let mut annotated_out = None;
    let mut confidence = DetectionOptions::default().confidence_threshold;
    let mut iou = DetectionOptions::default().iou_threshold;
    let mut max_detections = DetectionOptions::default().max_detections;
    let mut input_size = Some(DEFAULT_INPUT_SIZE);
    let mut device = InferenceDevice::Cpu;

    while let Some(flag) = values.next() {
      let flag = flag
        .to_str()
        .ok_or_else(|| format!("argument flag is not valid UTF-8: {flag:?}"))?;
      match flag {
        "--model" => model = Some(next_path(&mut values, flag)?),
        "--classes" => classes = Some(next_path(&mut values, flag)?),
        "--image" => image = Some(next_path(&mut values, flag)?),
        "--json-out" => json_out = Some(next_path(&mut values, flag)?),
        "--annotated-out" => annotated_out = Some(next_path(&mut values, flag)?),
        "--confidence" => confidence = next_f32(&mut values, flag)?,
        "--iou" => iou = next_f32(&mut values, flag)?,
        "--max-detections" => max_detections = next_usize(&mut values, flag)?,
        "--input-size" => input_size = Some(next_u32(&mut values, flag)?),
        "--device" => device = next_string(&mut values, flag)?.parse()?,
        "--help" | "-h" => return Err(usage().into()),
        unknown => return Err(format!("unknown argument: {unknown}\n\n{}", usage()).into()),
      }
    }

    Ok(Self {
      model: required_path(model, "--model")?,
      classes,
      image: required_path(image, "--image")?,
      json_out: required_path(json_out, "--json-out")?,
      annotated_out,
      confidence,
      iou,
      max_detections,
      input_size,
      device,
    })
  }
}

fn next_path(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<PathBuf, Box<dyn Error>> {
  Ok(PathBuf::from(next_value(values, flag)?))
}

fn next_string(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String, Box<dyn Error>> {
  next_value(values, flag)?
    .into_string()
    .map_err(|value| format!("{flag} value is not valid UTF-8: {value:?}").into())
}

fn next_f32(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<f32, Box<dyn Error>> {
  Ok(next_string(values, flag)?.parse()?)
}

fn next_u32(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<u32, Box<dyn Error>> {
  Ok(next_string(values, flag)?.parse()?)
}

fn next_usize(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<usize, Box<dyn Error>> {
  Ok(next_string(values, flag)?.parse()?)
}

fn next_value(values: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<OsString, Box<dyn Error>> {
  values.next().ok_or_else(|| format!("{flag} requires a value").into())
}

fn required_path(value: Option<PathBuf>, flag: &str) -> Result<PathBuf, Box<dyn Error>> {
  value.ok_or_else(|| format!("{flag} is required\n\n{}", usage()).into())
}

fn model_id(path: &std::path::Path) -> ModelId {
  ModelId(
    path
      .file_stem()
      .and_then(|value| value.to_str())
      .unwrap_or("ultralytics-model")
      .to_string(),
  )
}

fn load_class_names(path: PathBuf) -> Result<Vec<String>, Box<dyn Error>> {
  Ok(fs::read_to_string(path)?
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(ToOwned::to_owned)
    .collect())
}

fn usage() -> &'static str {
  "usage: detect --model <model.onnx> --image <image> --json-out <detections.json> [--classes <classes.txt>] [--annotated-out <annotated.png>] [--confidence <0..1>] [--iou <0..1>] [--max-detections <n>] [--input-size <px>] [--device <cpu|coreml|cuda:0|tensorrt:0|directml:0|openvino|xnnpack|rocm:0>]"
}
```

- [ ] **Step 2: Run the example against Balatro fixture**

Run:

```bash
cargo run -p auv-inference-ultralytics --example detect -- \
  --model /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx \
  --classes /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt \
  --image crates/auv-inference-ultralytics/tests/fixtures/balatro/balatro.jpg \
  --json-out /private/tmp/auv-ultralytics-entities.json \
  --annotated-out /private/tmp/auv-ultralytics-entities.png \
  --confidence 0.25 \
  --iou 0.45 \
  --input-size 640 \
  --device cpu
```

Expected:

```text
wrote 11 detections to /private/tmp/auv-ultralytics-entities.json
```

Run:

```bash
test -s /private/tmp/auv-ultralytics-entities.json
test -s /private/tmp/auv-ultralytics-entities.png
```

Expected: both commands PASS.

- [ ] **Step 3: Verify examples and tests**

Run:

```bash
cargo test -p auv-inference-ultralytics --examples
cargo test -p auv-inference-ultralytics
cargo fmt --check
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 4: Commit example**

Run:

```bash
git add crates/auv-inference-ultralytics/examples/detect.rs
git commit -m "feat(auv-inference-ultralytics): add detection example artifact"
```

## Task 7: Remove Custom YOLO Crate

**Files:**
- Modify: `Cargo.toml`
- Delete: `crates/auv-inference-yolo/`

- [ ] **Step 1: Remove workspace member**

Modify root `Cargo.toml`:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-inference-common",
  "crates/auv-inference-ultralytics",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
  "crates/auv-view",
]
```

Keep `ndarray` and `ort` workspace dependencies only if another workspace crate still uses them. If `rg "ndarray\\.workspace|ort\\.workspace" Cargo.toml crates` finds no references after removing `auv-inference-yolo`, remove those workspace dependency entries.

- [ ] **Step 2: Delete old crate**

Run:

```bash
git rm -r crates/auv-inference-yolo
```

- [ ] **Step 3: Verify old crate references are gone**

Run:

```bash
rg "auv-inference-yolo|auv_inference_yolo" Cargo.toml crates scripts docs --glob '!docs/superpowers/**'
```

Expected: no output, except historical docs under `docs/superpowers/**` are intentionally excluded.

- [ ] **Step 4: Verify workspace**

Run:

```bash
cargo check
cargo test -p auv-inference-common
cargo test -p auv-inference-ultralytics
cargo fmt --check
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 5: Commit removal**

Run:

```bash
git add Cargo.toml Cargo.lock
git commit -m "refactor: replace custom yolo crate with ultralytics adapter"
```

## Task 8: Final Validation

**Files:**
- Modify if needed: `docs/superpowers/specs/2026-06-04-ultralytics-inference-adapter-design.md`

- [ ] **Step 1: Run focused validation**

Run:

```bash
cargo test -p auv-inference-common
cargo test -p auv-inference-ultralytics
cargo test -p auv-inference-ultralytics --test fixture_parity -- --nocapture
cargo test -p auv-inference-ultralytics --examples
```

Expected: all commands PASS.

- [ ] **Step 2: Run callable example**

Run:

```bash
cargo run -p auv-inference-ultralytics --example detect -- \
  --model /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx \
  --classes /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt \
  --image crates/auv-inference-ultralytics/tests/fixtures/balatro/balatro.jpg \
  --json-out /private/tmp/auv-ultralytics-entities.json \
  --annotated-out /private/tmp/auv-ultralytics-entities.png \
  --confidence 0.25 \
  --iou 0.45 \
  --input-size 640 \
  --device cpu
```

Expected: JSON and PNG outputs are non-empty.

- [ ] **Step 3: Run workspace validation**

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 4: Run AUV inventory commands**

Run:

```bash
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
cargo run --quiet -- skill bundle list
```

Expected: all commands PASS.

- [ ] **Step 5: Update spec only if implementation changed a boundary**

If implementation required a boundary change, update:

```text
docs/superpowers/specs/2026-06-04-ultralytics-inference-adapter-design.md
```

Do not update `docs/TERMS_AND_CONCEPTS.md` because this slice introduces no AUV core term.

- [ ] **Step 6: Commit docs if changed**

If Step 5 changed the spec:

```bash
git add docs/superpowers/specs/2026-06-04-ultralytics-inference-adapter-design.md
git commit -m "docs: update ultralytics adapter design after implementation"
```

If no spec change is needed, do not create an empty commit.

## Self-Review

- Spec coverage: The plan covers common types, rendering, Ultralytics adapter, hardware feature mapping, device config, class-list overrides, conversion, Balatro parity, callable example, old crate removal, and final validation.
- Scope check: It does not touch runtime/catalog/driver/overlay, `auv-game-balatro`, OCR, Steam automation, capture, or Swift overlay.
- Type consistency: Common types use `DetectionSet`, `Detection`, `BoundingBox`, `ImageFrame`, `ImageSize`, `DetectionOptions`, and `ModelId` throughout. Adapter types use `UltralyticsDetector`, `UltralyticsModelConfig`, and `InferenceDevice` throughout.
- Upstream API check: `ultralytics-inference 0.0.18` stores `InferenceConfig` inside `YOLOModel`, so thresholds, IOU, max detections, input size, and device are configured at load time instead of pretending to be per-call options.
- Risk notes: If upstream behavior differs from the old Python fixture generator, the plan updates fixture generation to community backend behavior rather than reintroducing custom YOLO postprocessing.
