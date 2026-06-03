# AUV Inference YOLO Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `crates/auv-inference-yolo`, a narrow Rust ONNX YOLO detector, validate it against real Balatro YOLO models and images, and provide a callable example that writes JSON plus annotated image artifacts.

**Architecture:** The crate owns YOLO-specific inference, preprocessing, decoding, and NMS. It is platform-neutral and does not depend on AUV macOS drivers, runtime recording, OCR, or Balatro game semantics. Public callers load a model with labels and receive detections in source-image pixel coordinates.

**Tech Stack:** Rust 2024, `ort`, `ndarray`, `image`, `serde`, `serde_json`, Python fixture generation through the Balatro repo's `pixi` environment.

---

## Scope And File Map

This plan implements the design in `docs/superpowers/specs/2026-06-04-auv-inference-yolo-design.md`.

**Create:**
- `crates/auv-inference-yolo/Cargo.toml` - crate manifest and dependencies.
- `crates/auv-inference-yolo/src/lib.rs` - public API exports.
- `crates/auv-inference-yolo/src/error.rs` - crate error type.
- `crates/auv-inference-yolo/src/types.rs` - config, options, image, bbox, detection types.
- `crates/auv-inference-yolo/src/letterbox.rs` - letterbox preprocessing metadata and projection.
- `crates/auv-inference-yolo/src/decode.rs` - Ultralytics `[1, C, N]` output decoder.
- `crates/auv-inference-yolo/src/nms.rs` - class-aware NMS.
- `crates/auv-inference-yolo/src/detector.rs` - `ort` model loading and inference.
- `crates/auv-inference-yolo/src/render.rs` - annotated detection image rendering.
- `crates/auv-inference-yolo/examples/detect.rs` - minimal callable detector example.
- `crates/auv-inference-yolo/tests/fixture_parity.rs` - Balatro fixture parity tests.
- `crates/auv-inference-yolo/tests/fixtures/balatro/.gitkeep` - fixture directory placeholder.
- `scripts/generate-balatro-yolo-fixtures.py` - Python fixture generator run from AUV repo.

**Modify:**
- `Cargo.toml` - add workspace member and workspace dependencies.

**Do not modify:**
- `auv-driver`, `auv-driver-macos`, `auv-game-balatro`, OCR code, Balatro source files, `src/catalog.rs`, runtime dispatch, or overlay code.

**Explicit deferrals from the owner discussion:**
- `auv invoke` integration is deferred because it would touch command catalog/runtime surfaces that Collabi currently reports as active intent conflict paths.
- Steam Play automation, realtime Balatro capture, and Swift transparent overlay visualization are deferred to later `auv-game-balatro` and overlay slices.

## Task 1: Generate A Verified Balatro Fixture Baseline

**Files:**
- Create: `scripts/generate-balatro-yolo-fixtures.py`
- Create: `crates/auv-inference-yolo/tests/fixtures/balatro/.gitkeep`

- [ ] **Step 1: Create the fixture generator script**

Create `scripts/generate-balatro-yolo-fixtures.py`:

```python
#!/usr/bin/env python3
"""Generate Balatro YOLO fixture detections for auv-inference-yolo tests.

Run from the AUV repo root:
  python scripts/generate-balatro-yolo-fixtures.py

The script intentionally decodes Ultralytics ONNX output shape [1, C, N].
It does not reuse game-playing-ai-balatro's current ONNX postprocess helper.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import cv2
import numpy as np
import onnxruntime as ort


BALATRO_ROOT = Path("/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro")
AUV_ROOT = Path(__file__).resolve().parents[1]
OUT_DIR = AUV_ROOT / "crates/auv-inference-yolo/tests/fixtures/balatro"
IMAGE = BALATRO_ROOT / "data/datasets/games-balatro-2024-entities-detection/data/val/yolo/images/out_01707.jpg"

MODELS = {
    "entities": {
        "model": BALATRO_ROOT / "models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx",
        "classes": BALATRO_ROOT / "data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt",
    },
    "ui": {
        "model": BALATRO_ROOT / "models/games-balatro-2024-yolo-ui-detection/onnx/model.onnx",
        "classes": BALATRO_ROOT / "data/datasets/games-balatro-2024-ui-detection/data/train/yolo/classes.txt",
    },
}


def load_classes(path: Path) -> list[str]:
    return [line.strip() for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def letterbox(image: np.ndarray, size: int = 640) -> tuple[np.ndarray, dict[str, float]]:
    height, width = image.shape[:2]
    scale = min(size / width, size / height)
    resized_width = int(round(width * scale))
    resized_height = int(round(height * scale))
    resized = cv2.resize(image, (resized_width, resized_height), interpolation=cv2.INTER_LINEAR)
    pad_left = (size - resized_width) // 2
    pad_top = (size - resized_height) // 2
    padded = np.full((size, size, 3), 114, dtype=np.uint8)
    padded[pad_top : pad_top + resized_height, pad_left : pad_left + resized_width] = resized
    return padded, {
        "source_width": float(width),
        "source_height": float(height),
        "input_size": float(size),
        "scale": float(scale),
        "pad_left": float(pad_left),
        "pad_top": float(pad_top),
    }


def iou(a: dict, b: dict) -> float:
    ax1, ay1, ax2, ay2 = a["bbox"]
    bx1, by1, bx2, by2 = b["bbox"]
    ix1 = max(ax1, bx1)
    iy1 = max(ay1, by1)
    ix2 = min(ax2, bx2)
    iy2 = min(ay2, by2)
    if ix2 <= ix1 or iy2 <= iy1:
        return 0.0
    inter = (ix2 - ix1) * (iy2 - iy1)
    area_a = (ax2 - ax1) * (ay2 - ay1)
    area_b = (bx2 - bx1) * (by2 - by1)
    return inter / (area_a + area_b - inter)


def nms(detections: list[dict], iou_threshold: float) -> list[dict]:
    kept: list[dict] = []
    for det in sorted(detections, key=lambda item: item["confidence"], reverse=True):
        if all(det["class_id"] != kept_det["class_id"] or iou(det, kept_det) < iou_threshold for kept_det in kept):
            kept.append(det)
    return kept


def decode(output: np.ndarray, classes: list[str], meta: dict[str, float], confidence: float, iou_threshold: float) -> list[dict]:
    if output.ndim != 3 or output.shape[0] != 1:
        raise ValueError(f"unsupported output shape {output.shape}")
    channels = output.shape[1]
    anchors = output.shape[2]
    if channels != 4 + len(classes):
        raise ValueError(f"expected {4 + len(classes)} channels, got {channels}")
    raw = output[0]
    detections: list[dict] = []
    for anchor in range(anchors):
        x_center = float(raw[0, anchor])
        y_center = float(raw[1, anchor])
        width = float(raw[2, anchor])
        height = float(raw[3, anchor])
        scores = raw[4:, anchor]
        class_id = int(np.argmax(scores))
        score = float(scores[class_id])
        if score < confidence:
            continue
        x1 = (x_center - width / 2.0 - meta["pad_left"]) / meta["scale"]
        y1 = (y_center - height / 2.0 - meta["pad_top"]) / meta["scale"]
        x2 = (x_center + width / 2.0 - meta["pad_left"]) / meta["scale"]
        y2 = (y_center + height / 2.0 - meta["pad_top"]) / meta["scale"]
        x1 = max(0.0, min(x1, meta["source_width"]))
        y1 = max(0.0, min(y1, meta["source_height"]))
        x2 = max(0.0, min(x2, meta["source_width"]))
        y2 = max(0.0, min(y2, meta["source_height"]))
        detections.append({
            "class_id": class_id,
            "label": classes[class_id],
            "confidence": score,
            "bbox": [x1, y1, x2, y2],
        })
    return nms(detections, iou_threshold)


def run_model(name: str, image_rgb: np.ndarray, confidence: float, iou_threshold: float) -> dict:
    config = MODELS[name]
    classes = load_classes(config["classes"])
    session = ort.InferenceSession(str(config["model"]))
    padded, meta = letterbox(image_rgb)
    input_tensor = padded.astype(np.float32) / 255.0
    input_tensor = np.transpose(input_tensor, (2, 0, 1))[None, :, :, :]
    output = session.run(None, {session.get_inputs()[0].name: input_tensor})[0]
    detections = decode(output, classes, meta, confidence, iou_threshold)
    return {
        "model": name,
        "model_path": str(config["model"]),
        "classes_path": str(config["classes"]),
        "image_path": str(IMAGE),
        "confidence_threshold": confidence,
        "iou_threshold": iou_threshold,
        "image_width": int(image_rgb.shape[1]),
        "image_height": int(image_rgb.shape[0]),
        "output_shape": list(output.shape),
        "detections": detections,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--confidence", type=float, default=0.25)
    parser.add_argument("--iou", type=float, default=0.45)
    args = parser.parse_args()
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    image_bgr = cv2.imread(str(IMAGE), cv2.IMREAD_COLOR)
    if image_bgr is None:
        raise RuntimeError(f"failed to read fixture image {IMAGE}")
    image_rgb = cv2.cvtColor(image_bgr, cv2.COLOR_BGR2RGB)
    cv2.imwrite(str(OUT_DIR / "balatro.jpg"), image_bgr)
    for name in MODELS:
        result = run_model(name, image_rgb, args.confidence, args.iou)
        (OUT_DIR / f"{name}.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {OUT_DIR / f'{name}.json'} with {len(result['detections'])} detections")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Add the fixture directory placeholder**

Run: `mkdir -p crates/auv-inference-yolo/tests/fixtures/balatro`

Create `crates/auv-inference-yolo/tests/fixtures/balatro/.gitkeep` as an empty file.

- [ ] **Step 3: Run the generator through Balatro pixi**

Run from the AUV repo root:

```bash
cd /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro
pixi run python /Users/neko/Git/github.com/moeru-ai/auv/scripts/generate-balatro-yolo-fixtures.py
```

Expected: prints two `wrote ... entities.json` / `wrote ... ui.json` lines.

- [ ] **Step 4: Inspect generated fixture metadata**

Run from the AUV repo root:

```bash
sed -n '1,80p' crates/auv-inference-yolo/tests/fixtures/balatro/entities.json
sed -n '1,80p' crates/auv-inference-yolo/tests/fixtures/balatro/ui.json
```

Expected: `output_shape` is `[1, 14, 8400]` for entities and `[1, 37, 8400]` for UI.

- [ ] **Step 5: Commit fixture script and generated fixture**

Run:

```bash
git add scripts/generate-balatro-yolo-fixtures.py crates/auv-inference-yolo/tests/fixtures/balatro
git commit -m "test(auv-inference-yolo): add balatro yolo fixtures"
```

## Task 2: Scaffold Crate And Public Types

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-inference-yolo/Cargo.toml`
- Create: `crates/auv-inference-yolo/src/lib.rs`
- Create: `crates/auv-inference-yolo/src/error.rs`
- Create: `crates/auv-inference-yolo/src/types.rs`

- [ ] **Step 1: Add workspace member and dependencies**

Modify root `Cargo.toml`:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-inference-yolo",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
  "crates/auv-view",
]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
image = { version = "0.25", default-features = false, features = ["png", "jpeg"] }
ndarray = "0.16"
ort = "2.0.0-rc.10"
xcap = "0.6"
swift-bridge = "0.1.59"
swift-bridge-build = "0.1.59"
objc2-core-graphics = { version = "0.3", features = ["CGWindow", "CGImage", "CGDataProvider", "CGGeometry"] }
objc2-core-foundation = "0.3"
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/auv-inference-yolo/Cargo.toml`:

```toml
[package]
name = "auv-inference-yolo"
version.workspace = true
edition.workspace = true
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
ndarray.workspace = true
ort.workspace = true
serde.workspace = true
serde_json.workspace = true
```

- [ ] **Step 3: Create the public API skeleton**

Create `crates/auv-inference-yolo/src/lib.rs`:

```rust
pub mod decode;
pub mod detector;
pub mod error;
pub mod letterbox;
pub mod nms;
pub mod render;
pub mod types;

pub use detector::YoloDetector;
pub use error::{YoloError, YoloResult};
pub use render::render_annotated_image;
pub use types::{
  BoundingBox, Detection, DetectionOptions, DetectionSet, ImageFrame, ImageSize, ModelId,
  YoloFamily, YoloModelConfig,
};
```

- [ ] **Step 4: Create the error type**

Create `crates/auv-inference-yolo/src/error.rs`:

```rust
use std::fmt;
use std::path::PathBuf;

pub type YoloResult<T> = Result<T, YoloError>;

#[derive(Debug)]
pub enum YoloError {
  EmptyClassList,
  InvalidThreshold { name: &'static str, value: f32 },
  MissingModel { path: PathBuf },
  ImageDecode(String),
  UnsupportedOutputShape { shape: Vec<usize> },
  ClassCountMismatch { expected_channels: usize, actual_channels: usize },
  Ort(String),
}

impl fmt::Display for YoloError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::EmptyClassList => write!(f, "class list must not be empty"),
      Self::InvalidThreshold { name, value } => {
        write!(f, "{name} threshold must be between 0 and 1, got {value}")
      }
      Self::MissingModel { path } => write!(f, "model file does not exist: {}", path.display()),
      Self::ImageDecode(message) => write!(f, "failed to decode image: {message}"),
      Self::UnsupportedOutputShape { shape } => write!(f, "unsupported YOLO output shape: {shape:?}"),
      Self::ClassCountMismatch {
        expected_channels,
        actual_channels,
      } => write!(
        f,
        "YOLO output channel count mismatch: expected {expected_channels}, got {actual_channels}"
      ),
      Self::Ort(message) => write!(f, "ONNX Runtime error: {message}"),
    }
  }
}

impl std::error::Error for YoloError {}
```

- [ ] **Step 5: Create public types**

Create `crates/auv-inference-yolo/src/types.rs`:

```rust
use std::path::PathBuf;

use image::RgbImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum YoloFamily {
  UltralyticsV8Like,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YoloModelConfig {
  pub model_id: ModelId,
  pub model_path: PathBuf,
  pub class_names: Vec<String>,
  pub input_size: u32,
  pub family: YoloFamily,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionOptions {
  pub confidence_threshold: f32,
  pub iou_threshold: f32,
}

impl Default for DetectionOptions {
  fn default() -> Self {
    Self {
      confidence_threshold: 0.25,
      iou_threshold: 0.45,
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
  pub x1: f32,
  pub y1: f32,
  pub x2: f32,
  pub y2: f32,
}

impl BoundingBox {
  pub fn width(self) -> f32 {
    self.x2 - self.x1
  }

  pub fn height(self) -> f32 {
    self.y2 - self.y1
  }

  pub fn area(self) -> f32 {
    self.width().max(0.0) * self.height().max(0.0)
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detection {
  pub class_id: usize,
  pub label: String,
  pub confidence: f32,
  pub bbox: BoundingBox,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DetectionSet {
  pub model_id: ModelId,
  pub image_size: ImageSize,
  pub detections: Vec<Detection>,
}
```

- [ ] **Step 6: Run check for scaffold**

Run: `cargo check -p auv-inference-yolo`

Expected: fails because modules `decode`, `detector`, `letterbox`, `nms`, and `render` are declared but not created.

- [ ] **Step 7: Commit scaffold**

Run:

```bash
git add Cargo.toml crates/auv-inference-yolo
git commit -m "feat(auv-inference-yolo): scaffold crate"
```

## Task 3: Letterbox Preprocessing

**Files:**
- Create: `crates/auv-inference-yolo/src/letterbox.rs`

- [ ] **Step 1: Write letterbox tests**

Create `crates/auv-inference-yolo/src/letterbox.rs`:

```rust
use image::{Rgb, RgbImage};
use ndarray::Array4;

use crate::types::{BoundingBox, ImageFrame};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
  pub source_width: u32,
  pub source_height: u32,
  pub input_size: u32,
  pub scale: f32,
  pub pad_left: u32,
  pub pad_top: u32,
}

pub fn prepare_input(_frame: &ImageFrame, _input_size: u32) -> (Array4<f32>, Letterbox) {
  unimplemented!("implemented in Step 3")
}

pub fn project_model_bbox_to_source(_bbox: BoundingBox, _letterbox: Letterbox) -> BoundingBox {
  unimplemented!("implemented in Step 3")
}

fn clamp(value: f32, max: f32) -> f32 {
  value.max(0.0).min(max)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn letterbox_records_scale_and_padding_for_wide_image() {
    let frame = ImageFrame::new(RgbImage::from_pixel(1280, 720, Rgb([10, 20, 30])));

    let (tensor, letterbox) = prepare_input(&frame, 640);

    assert_eq!(tensor.shape(), &[1, 3, 640, 640]);
    assert_eq!(letterbox.source_width, 1280);
    assert_eq!(letterbox.source_height, 720);
    assert_eq!(letterbox.input_size, 640);
    assert!((letterbox.scale - 0.5).abs() < 0.0001);
    assert_eq!(letterbox.pad_left, 0);
    assert_eq!(letterbox.pad_top, 80);
  }

  #[test]
  fn letterbox_converts_rgb_to_normalized_chw_tensor() {
    let mut image = RgbImage::from_pixel(2, 2, Rgb([0, 0, 0]));
    image.put_pixel(0, 0, Rgb([255, 128, 64]));
    let frame = ImageFrame::new(image);

    let (tensor, _) = prepare_input(&frame, 2);

    assert!((tensor[[0, 0, 0, 0]] - 1.0).abs() < 0.0001);
    assert!((tensor[[0, 1, 0, 0]] - (128.0 / 255.0)).abs() < 0.0001);
    assert!((tensor[[0, 2, 0, 0]] - (64.0 / 255.0)).abs() < 0.0001);
  }

  #[test]
  fn project_model_bbox_removes_padding_and_scale() {
    let letterbox = Letterbox {
      source_width: 1280,
      source_height: 720,
      input_size: 640,
      scale: 0.5,
      pad_left: 0,
      pad_top: 80,
    };
    let model_bbox = BoundingBox {
      x1: 100.0,
      y1: 130.0,
      x2: 200.0,
      y2: 230.0,
    };

    let projected = project_model_bbox_to_source(model_bbox, letterbox);

    assert_eq!(
      projected,
      BoundingBox {
        x1: 200.0,
        y1: 60.0,
        x2: 400.0,
        y2: 260.0
      }
    );
  }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p auv-inference-yolo letterbox -- --nocapture`

Expected: FAIL because `prepare_input` and `project_model_bbox_to_source` are not implemented.

- [ ] **Step 3: Implement letterbox preprocessing**

Replace the two `unimplemented!` functions in `crates/auv-inference-yolo/src/letterbox.rs`:

```rust
pub fn prepare_input(frame: &ImageFrame, input_size: u32) -> (Array4<f32>, Letterbox) {
  let source_width = frame.image.width();
  let source_height = frame.image.height();
  let scale = (input_size as f32 / source_width as f32)
    .min(input_size as f32 / source_height as f32);
  let resized_width = (source_width as f32 * scale).round() as u32;
  let resized_height = (source_height as f32 * scale).round() as u32;
  let pad_left = (input_size - resized_width) / 2;
  let pad_top = (input_size - resized_height) / 2;
  let resized = image::imageops::resize(
    &frame.image,
    resized_width,
    resized_height,
    image::imageops::FilterType::Triangle,
  );
  let mut padded = RgbImage::from_pixel(input_size, input_size, Rgb([114, 114, 114]));
  image::imageops::replace(
    &mut padded,
    &resized,
    i64::from(pad_left),
    i64::from(pad_top),
  );
  let mut tensor = Array4::<f32>::zeros((
    1,
    3,
    input_size as usize,
    input_size as usize,
  ));
  for (x, y, pixel) in padded.enumerate_pixels() {
    tensor[[0, 0, y as usize, x as usize]] = f32::from(pixel[0]) / 255.0;
    tensor[[0, 1, y as usize, x as usize]] = f32::from(pixel[1]) / 255.0;
    tensor[[0, 2, y as usize, x as usize]] = f32::from(pixel[2]) / 255.0;
  }
  (
    tensor,
    Letterbox {
      source_width,
      source_height,
      input_size,
      scale,
      pad_left,
      pad_top,
    },
  )
}

pub fn project_model_bbox_to_source(bbox: BoundingBox, letterbox: Letterbox) -> BoundingBox {
  let pad_left = letterbox.pad_left as f32;
  let pad_top = letterbox.pad_top as f32;
  let source_width = letterbox.source_width as f32;
  let source_height = letterbox.source_height as f32;
  BoundingBox {
    x1: clamp((bbox.x1 - pad_left) / letterbox.scale, source_width),
    y1: clamp((bbox.y1 - pad_top) / letterbox.scale, source_height),
    x2: clamp((bbox.x2 - pad_left) / letterbox.scale, source_width),
    y2: clamp((bbox.y2 - pad_top) / letterbox.scale, source_height),
  }
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test -p auv-inference-yolo letterbox -- --nocapture`

Expected: PASS for the three letterbox tests.

- [ ] **Step 5: Commit letterbox implementation**

Run:

```bash
git add crates/auv-inference-yolo/src/letterbox.rs
git commit -m "feat(auv-inference-yolo): add letterbox preprocessing"
```

## Task 4: Decode YOLO Output And NMS

**Files:**
- Create: `crates/auv-inference-yolo/src/decode.rs`
- Create: `crates/auv-inference-yolo/src/nms.rs`

- [ ] **Step 1: Write decoder tests**

Create `crates/auv-inference-yolo/src/decode.rs`:

```rust
use ndarray::ArrayViewD;

use crate::error::{YoloError, YoloResult};
use crate::letterbox::{project_model_bbox_to_source, Letterbox};
use crate::types::{BoundingBox, Detection, DetectionOptions};

pub fn decode_ultralytics_v8_like(
  _output: ArrayViewD<'_, f32>,
  _classes: &[String],
  _letterbox: Letterbox,
  _options: DetectionOptions,
) -> YoloResult<Vec<Detection>> {
  unimplemented!("implemented in Step 3")
}

#[cfg(test)]
mod tests {
  use ndarray::Array3;

  use super::*;

  fn labels() -> Vec<String> {
    vec!["card".to_string(), "button".to_string()]
  }

  fn letterbox() -> Letterbox {
    Letterbox {
      source_width: 640,
      source_height: 640,
      input_size: 640,
      scale: 1.0,
      pad_left: 0,
      pad_top: 0,
    }
  }

  #[test]
  fn decodes_highest_class_from_channel_first_output() {
    let mut output = Array3::<f32>::zeros((1, 6, 2));
    output[[0, 0, 0]] = 100.0;
    output[[0, 1, 0]] = 110.0;
    output[[0, 2, 0]] = 20.0;
    output[[0, 3, 0]] = 30.0;
    output[[0, 4, 0]] = 0.20;
    output[[0, 5, 0]] = 0.90;

    let detections = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &labels(),
      letterbox(),
      DetectionOptions::default(),
    )
    .unwrap();

    assert_eq!(detections.len(), 1);
    assert_eq!(detections[0].class_id, 1);
    assert_eq!(detections[0].label, "button");
    assert!((detections[0].confidence - 0.90).abs() < 0.0001);
    assert_eq!(
      detections[0].bbox,
      BoundingBox {
        x1: 90.0,
        y1: 95.0,
        x2: 110.0,
        y2: 125.0
      }
    );
  }

  #[test]
  fn rejects_shape_with_wrong_channel_count() {
    let output = Array3::<f32>::zeros((1, 5, 1));
    let error = decode_ultralytics_v8_like(
      output.view().into_dyn(),
      &labels(),
      letterbox(),
      DetectionOptions::default(),
    )
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::ClassCountMismatch {
        expected_channels: 6,
        actual_channels: 5
      }
    ));
  }
}
```

- [ ] **Step 2: Write NMS tests**

Create `crates/auv-inference-yolo/src/nms.rs`:

```rust
use crate::types::{BoundingBox, Detection};

pub fn class_aware_nms(_detections: Vec<Detection>, _iou_threshold: f32) -> Vec<Detection> {
  unimplemented!("implemented in Step 4")
}

pub fn iou(a: BoundingBox, b: BoundingBox) -> f32 {
  let x1 = a.x1.max(b.x1);
  let y1 = a.y1.max(b.y1);
  let x2 = a.x2.min(b.x2);
  let y2 = a.y2.min(b.y2);
  if x2 <= x1 || y2 <= y1 {
    return 0.0;
  }
  let intersection = (x2 - x1) * (y2 - y1);
  let union = a.area() + b.area() - intersection;
  if union <= 0.0 { 0.0 } else { intersection / union }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn detection(class_id: usize, confidence: f32, bbox: BoundingBox) -> Detection {
    Detection {
      class_id,
      label: format!("class_{class_id}"),
      confidence,
      bbox,
    }
  }

  #[test]
  fn suppresses_lower_confidence_same_class_overlap() {
    let detections = vec![
      detection(0, 0.9, BoundingBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0 }),
      detection(0, 0.8, BoundingBox { x1: 10.0, y1: 10.0, x2: 110.0, y2: 110.0 }),
    ];

    let kept = class_aware_nms(detections, 0.45);

    assert_eq!(kept.len(), 1);
    assert!((kept[0].confidence - 0.9).abs() < 0.0001);
  }

  #[test]
  fn keeps_overlapping_different_classes() {
    let detections = vec![
      detection(0, 0.9, BoundingBox { x1: 0.0, y1: 0.0, x2: 100.0, y2: 100.0 }),
      detection(1, 0.8, BoundingBox { x1: 10.0, y1: 10.0, x2: 110.0, y2: 110.0 }),
    ];

    let kept = class_aware_nms(detections, 0.45);

    assert_eq!(kept.len(), 2);
  }
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p auv-inference-yolo decode nms -- --nocapture`

Expected: FAIL because decoder and NMS functions are not implemented.

- [ ] **Step 4: Implement decoder**

Replace `decode_ultralytics_v8_like` in `crates/auv-inference-yolo/src/decode.rs`:

```rust
pub fn decode_ultralytics_v8_like(
  output: ArrayViewD<'_, f32>,
  classes: &[String],
  letterbox: Letterbox,
  options: DetectionOptions,
) -> YoloResult<Vec<Detection>> {
  let shape = output.shape();
  if shape.len() != 3 || shape[0] != 1 {
    return Err(YoloError::UnsupportedOutputShape {
      shape: shape.to_vec(),
    });
  }
  let expected_channels = 4 + classes.len();
  let actual_channels = shape[1];
  if actual_channels != expected_channels {
    return Err(YoloError::ClassCountMismatch {
      expected_channels,
      actual_channels,
    });
  }
  let anchors = shape[2];
  let mut detections = Vec::new();
  for anchor in 0..anchors {
    let x_center = output[[0, 0, anchor]];
    let y_center = output[[0, 1, anchor]];
    let width = output[[0, 2, anchor]];
    let height = output[[0, 3, anchor]];
    let mut best_class_id = 0;
    let mut best_score = f32::NEG_INFINITY;
    for class_id in 0..classes.len() {
      let score = output[[0, 4 + class_id, anchor]];
      if score > best_score {
        best_score = score;
        best_class_id = class_id;
      }
    }
    if best_score < options.confidence_threshold {
      continue;
    }
    let model_bbox = BoundingBox {
      x1: x_center - width / 2.0,
      y1: y_center - height / 2.0,
      x2: x_center + width / 2.0,
      y2: y_center + height / 2.0,
    };
    detections.push(Detection {
      class_id: best_class_id,
      label: classes[best_class_id].clone(),
      confidence: best_score,
      bbox: project_model_bbox_to_source(model_bbox, letterbox),
    });
  }
  Ok(crate::nms::class_aware_nms(
    detections,
    options.iou_threshold,
  ))
}
```

- [ ] **Step 5: Implement NMS**

Replace `class_aware_nms` in `crates/auv-inference-yolo/src/nms.rs`:

```rust
pub fn class_aware_nms(mut detections: Vec<Detection>, iou_threshold: f32) -> Vec<Detection> {
  detections.sort_by(|a, b| {
    b.confidence
      .partial_cmp(&a.confidence)
      .unwrap_or(std::cmp::Ordering::Equal)
  });
  let mut kept = Vec::new();
  for detection in detections {
    let should_suppress = kept.iter().any(|kept_detection: &Detection| {
      detection.class_id == kept_detection.class_id
        && iou(detection.bbox, kept_detection.bbox) >= iou_threshold
    });
    if !should_suppress {
      kept.push(detection);
    }
  }
  kept
}
```

- [ ] **Step 6: Run tests to verify pass**

Run: `cargo test -p auv-inference-yolo decode nms -- --nocapture`

Expected: PASS for decoder and NMS tests.

- [ ] **Step 7: Commit decoder and NMS**

Run:

```bash
git add crates/auv-inference-yolo/src/decode.rs crates/auv-inference-yolo/src/nms.rs
git commit -m "feat(auv-inference-yolo): decode ultralytics detections"
```

## Task 5: Detector API With ONNX Runtime

**Files:**
- Create: `crates/auv-inference-yolo/src/detector.rs`
- Modify: `crates/auv-inference-yolo/src/types.rs`

- [ ] **Step 1: Add validation tests**

Create `crates/auv-inference-yolo/src/detector.rs`:

```rust
use std::path::Path;

use crate::error::{YoloError, YoloResult};
use crate::types::{DetectionOptions, DetectionSet, ImageFrame, YoloModelConfig};

pub struct YoloDetector {
  config: YoloModelConfig,
}

impl YoloDetector {
  pub fn load(config: YoloModelConfig) -> YoloResult<Self> {
    validate_config(&config)?;
    Ok(Self { config })
  }

  pub fn detect(&self, _frame: &ImageFrame, _options: DetectionOptions) -> YoloResult<DetectionSet> {
    unimplemented!("implemented in Step 4")
  }
}

fn validate_config(config: &YoloModelConfig) -> YoloResult<()> {
  if config.class_names.is_empty() {
    return Err(YoloError::EmptyClassList);
  }
  if !Path::new(&config.model_path).exists() {
    return Err(YoloError::MissingModel {
      path: config.model_path.clone(),
    });
  }
  Ok(())
}

fn validate_options(options: DetectionOptions) -> YoloResult<()> {
  if !(0.0..=1.0).contains(&options.confidence_threshold) {
    return Err(YoloError::InvalidThreshold {
      name: "confidence",
      value: options.confidence_threshold,
    });
  }
  if !(0.0..=1.0).contains(&options.iou_threshold) {
    return Err(YoloError::InvalidThreshold {
      name: "iou",
      value: options.iou_threshold,
    });
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use crate::types::{ModelId, YoloFamily};

  use super::*;

  #[test]
  fn load_rejects_empty_classes() {
    let error = YoloDetector::load(YoloModelConfig {
      model_id: ModelId("test".to_string()),
      model_path: PathBuf::from("missing.onnx"),
      class_names: Vec::new(),
      input_size: 640,
      family: YoloFamily::UltralyticsV8Like,
    })
    .unwrap_err();

    assert!(matches!(error, YoloError::EmptyClassList));
  }

  #[test]
  fn validate_options_rejects_negative_confidence() {
    let error = validate_options(DetectionOptions {
      confidence_threshold: -0.1,
      iou_threshold: 0.45,
    })
    .unwrap_err();

    assert!(matches!(
      error,
      YoloError::InvalidThreshold {
        name: "confidence",
        value: -0.1
      }
    ));
  }
}
```

- [ ] **Step 2: Run validation tests**

Run: `cargo test -p auv-inference-yolo detector -- --nocapture`

Expected: PASS for validation tests; `detect` is not called yet.

- [ ] **Step 3: Add session field to detector**

Modify `crates/auv-inference-yolo/src/detector.rs` imports and struct:

```rust
use ort::session::Session;

pub struct YoloDetector {
  config: YoloModelConfig,
  session: Session,
}
```

Modify `load`:

```rust
pub fn load(config: YoloModelConfig) -> YoloResult<Self> {
  validate_config(&config)?;
  let session = Session::builder()
    .map_err(|error| YoloError::Ort(error.to_string()))?
    .commit_from_file(&config.model_path)
    .map_err(|error| YoloError::Ort(error.to_string()))?;
  Ok(Self { config, session })
}
```

- [ ] **Step 4: Implement `detect`**

Replace `detect` in `crates/auv-inference-yolo/src/detector.rs`:

```rust
pub fn detect(&self, frame: &ImageFrame, options: DetectionOptions) -> YoloResult<DetectionSet> {
  validate_options(options)?;
  let (input, letterbox) = crate::letterbox::prepare_input(frame, self.config.input_size);
  let outputs = self
    .session
    .run(ort::inputs![input])
    .map_err(|error| YoloError::Ort(error.to_string()))?;
  let output = outputs
    .get(0)
    .ok_or_else(|| YoloError::UnsupportedOutputShape { shape: Vec::new() })?;
  let tensor = output
    .try_extract_tensor::<f32>()
    .map_err(|error| YoloError::Ort(error.to_string()))?;
  let detections = match self.config.family {
    crate::types::YoloFamily::UltralyticsV8Like => crate::decode::decode_ultralytics_v8_like(
      tensor.view(),
      &self.config.class_names,
      letterbox,
      options,
    )?,
  };
  Ok(DetectionSet {
    model_id: self.config.model_id.clone(),
    image_size: frame.size(),
    detections,
  })
}
```

- [ ] **Step 5: Compile detector**

Run: `cargo check -p auv-inference-yolo`

Expected: PASS. If `ort` API names differ from the snippet, adjust only in `detector.rs` and keep public API unchanged.

- [ ] **Step 6: Commit detector API**

Run:

```bash
git add crates/auv-inference-yolo/src/detector.rs crates/auv-inference-yolo/src/types.rs Cargo.toml crates/auv-inference-yolo/Cargo.toml
git commit -m "feat(auv-inference-yolo): run onnx yolo detector"
```

## Task 6: Balatro Fixture Parity Tests

**Files:**
- Create: `crates/auv-inference-yolo/tests/fixture_parity.rs`
- Modify if needed: `crates/auv-inference-yolo/src/types.rs`

- [ ] **Step 1: Write parity test**

Create `crates/auv-inference-yolo/tests/fixture_parity.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use auv_inference_yolo::{
  DetectionOptions, ImageFrame, ModelId, YoloDetector, YoloFamily, YoloModelConfig,
};
use serde::Deserialize;

const BALATRO_ROOT: &str = "/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro";

#[derive(Debug, Deserialize)]
struct Fixture {
  model: String,
  model_path: String,
  classes_path: String,
  confidence_threshold: f32,
  iou_threshold: f32,
  detections: Vec<FixtureDetection>,
}

#[derive(Debug, Deserialize)]
struct FixtureDetection {
  class_id: usize,
  label: String,
  confidence: f32,
  bbox: [f32; 4],
}

fn load_classes(path: impl AsRef<Path>) -> Vec<String> {
  fs::read_to_string(path)
    .unwrap()
    .lines()
    .filter_map(|line| {
      let trimmed = line.trim();
      (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
    .collect()
}

fn load_fixture(name: &str) -> Fixture {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests/fixtures/balatro")
    .join(format!("{name}.json"));
  serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn load_frame() -> ImageFrame {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/balatro/balatro.jpg");
  let image = image::open(path).unwrap().to_rgb8();
  ImageFrame::new(image)
}

fn assert_matches_fixture(model: &str) {
  let fixture = load_fixture(model);
  let detector = YoloDetector::load(YoloModelConfig {
    model_id: ModelId(fixture.model.clone()),
    model_path: PathBuf::from(&fixture.model_path),
    class_names: load_classes(&fixture.classes_path),
    input_size: 640,
    family: YoloFamily::UltralyticsV8Like,
  })
  .unwrap();
  let detected = detector
    .detect(
      &load_frame(),
      DetectionOptions {
        confidence_threshold: fixture.confidence_threshold,
        iou_threshold: fixture.iou_threshold,
      },
    )
    .unwrap();

  assert_eq!(detected.model_id.0, fixture.model);
  assert_eq!(
    detected.detections.len(),
    fixture.detections.len(),
    "detection count changed for {model}"
  );
  for (actual, expected) in detected.detections.iter().zip(fixture.detections.iter()) {
    assert_eq!(actual.class_id, expected.class_id);
    assert_eq!(actual.label, expected.label);
    assert!(
      (actual.confidence - expected.confidence).abs() < 0.001,
      "confidence mismatch for {}: actual {} expected {}",
      actual.label,
      actual.confidence,
      expected.confidence
    );
    let actual_bbox = [actual.bbox.x1, actual.bbox.y1, actual.bbox.x2, actual.bbox.y2];
    for (actual_coord, expected_coord) in actual_bbox.iter().zip(expected.bbox.iter()) {
      assert!(
        (actual_coord - expected_coord).abs() < 2.0,
        "bbox mismatch for {}: actual {:?} expected {:?}",
        actual.label,
        actual_bbox,
        expected.bbox
      );
    }
  }
}

#[test]
fn balatro_entities_fixture_matches_rust_detector() {
  if !Path::new(BALATRO_ROOT).exists() {
    eprintln!("skipping Balatro fixture parity: {BALATRO_ROOT} does not exist");
    return;
  }
  assert_matches_fixture("entities");
}

#[test]
fn balatro_ui_fixture_matches_rust_detector() {
  if !Path::new(BALATRO_ROOT).exists() {
    eprintln!("skipping Balatro fixture parity: {BALATRO_ROOT} does not exist");
    return;
  }
  assert_matches_fixture("ui");
}
```

- [ ] **Step 2: Run parity tests**

Run: `cargo test -p auv-inference-yolo --test fixture_parity -- --nocapture`

Expected: PASS. If Rust and fixture disagree, inspect whether the difference is due to RGB/BGR. Keep Rust input as RGB and adjust the Python generator to produce the same interpretation before changing Rust public API.

- [ ] **Step 3: Commit parity tests**

Run:

```bash
git add crates/auv-inference-yolo/tests/fixture_parity.rs
git commit -m "test(auv-inference-yolo): verify balatro model parity"
```

## Task 7: Callable Example And Annotated Artifact

**Files:**
- Create: `crates/auv-inference-yolo/src/render.rs`
- Create: `crates/auv-inference-yolo/examples/detect.rs`
- Modify: `crates/auv-inference-yolo/Cargo.toml`

- [ ] **Step 1: Add example dependencies**

Modify `crates/auv-inference-yolo/Cargo.toml`:

```toml
[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 2: Write render tests and implementation**

Create `crates/auv-inference-yolo/src/render.rs`:

```rust
use image::{Rgb, RgbImage};

use crate::types::{BoundingBox, Detection};

pub fn render_annotated_image(image: &RgbImage, detections: &[Detection]) -> RgbImage {
  let mut annotated = image.clone();
  for detection in detections {
    draw_rect(&mut annotated, detection.bbox, color_for_class(detection.class_id));
  }
  annotated
}

fn color_for_class(class_id: usize) -> Rgb<u8> {
  const COLORS: [Rgb<u8>; 6] = [
    Rgb([230, 57, 70]),
    Rgb([29, 163, 83]),
    Rgb([0, 119, 182]),
    Rgb([255, 183, 3]),
    Rgb([131, 56, 236]),
    Rgb([251, 133, 0]),
  ];
  COLORS[class_id % COLORS.len()]
}

fn draw_rect(image: &mut RgbImage, bbox: BoundingBox, color: Rgb<u8>) {
  let max_x = image.width().saturating_sub(1) as i32;
  let max_y = image.height().saturating_sub(1) as i32;
  let x1 = (bbox.x1.round() as i32).clamp(0, max_x);
  let y1 = (bbox.y1.round() as i32).clamp(0, max_y);
  let x2 = (bbox.x2.round() as i32).clamp(0, max_x);
  let y2 = (bbox.y2.round() as i32).clamp(0, max_y);
  for x in x1..=x2 {
    image.put_pixel(x as u32, y1 as u32, color);
    image.put_pixel(x as u32, y2 as u32, color);
  }
  for y in y1..=y2 {
    image.put_pixel(x1 as u32, y as u32, color);
    image.put_pixel(x2 as u32, y as u32, color);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn annotated_image_draws_detection_border() {
    let image = RgbImage::from_pixel(20, 20, Rgb([0, 0, 0]));
    let detections = vec![Detection {
      class_id: 0,
      label: "card".to_string(),
      confidence: 0.9,
      bbox: BoundingBox {
        x1: 2.0,
        y1: 3.0,
        x2: 10.0,
        y2: 12.0,
      },
    }];

    let annotated = render_annotated_image(&image, &detections);

    assert_ne!(annotated.get_pixel(2, 3), image.get_pixel(2, 3));
    assert_eq!(annotated.get_pixel(0, 0), image.get_pixel(0, 0));
  }
}
```

- [ ] **Step 3: Create callable example**

Create `crates/auv-inference-yolo/examples/detect.rs`:

```rust
use std::fs;
use std::path::PathBuf;

use auv_inference_yolo::{
  render_annotated_image, DetectionOptions, ImageFrame, ModelId, YoloDetector, YoloFamily,
  YoloModelConfig,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  let value = |name: &str| -> Result<String, String> {
    args
      .windows(2)
      .find(|pair| pair[0] == name)
      .map(|pair| pair[1].clone())
      .ok_or_else(|| format!("missing required argument {name}"))
  };
  let model = PathBuf::from(value("--model")?);
  let classes = PathBuf::from(value("--classes")?);
  let image_path = PathBuf::from(value("--image")?);
  let json_out = PathBuf::from(value("--json-out")?);
  let annotated_out = args
    .windows(2)
    .find(|pair| pair[0] == "--annotated-out")
    .map(|pair| PathBuf::from(&pair[1]));
  let confidence = args
    .windows(2)
    .find(|pair| pair[0] == "--confidence")
    .map(|pair| pair[1].parse::<f32>())
    .transpose()?
    .unwrap_or(0.25);
  let iou = args
    .windows(2)
    .find(|pair| pair[0] == "--iou")
    .map(|pair| pair[1].parse::<f32>())
    .transpose()?
    .unwrap_or(0.45);
  let class_names = fs::read_to_string(classes)?
    .lines()
    .filter_map(|line| {
      let trimmed = line.trim();
      (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
    .collect::<Vec<_>>();
  let image = image::open(&image_path)?.to_rgb8();
  let frame = ImageFrame::new(image.clone());
  let detector = YoloDetector::load(YoloModelConfig {
    model_id: ModelId("example".to_string()),
    model_path: model,
    class_names,
    input_size: 640,
    family: YoloFamily::UltralyticsV8Like,
  })?;
  let detections = detector.detect(
    &frame,
    DetectionOptions {
      confidence_threshold: confidence,
      iou_threshold: iou,
    },
  )?;
  fs::write(&json_out, serde_json::to_string_pretty(&detections)? + "\n")?;
  if let Some(path) = annotated_out {
    render_annotated_image(&image, &detections.detections).save(path)?;
  }
  println!("detections: {}", detections.detections.len());
  println!("json: {}", json_out.display());
  Ok(())
}
```

- [ ] **Step 4: Run render tests**

Run: `cargo test -p auv-inference-yolo render -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Run the example against Balatro fixture**

Run:

```bash
cargo run -p auv-inference-yolo --example detect -- \
  --model /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx \
  --classes /Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro/data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt \
  --image crates/auv-inference-yolo/tests/fixtures/balatro/balatro.jpg \
  --json-out /private/tmp/auv-yolo-entities.json \
  --annotated-out /private/tmp/auv-yolo-entities.png
```

Expected: prints `detections: <n>`, writes `/private/tmp/auv-yolo-entities.json`, and writes `/private/tmp/auv-yolo-entities.png`.

- [ ] **Step 6: Commit callable example and renderer**

Run:

```bash
git add crates/auv-inference-yolo/src/render.rs crates/auv-inference-yolo/examples/detect.rs crates/auv-inference-yolo/Cargo.toml
git commit -m "feat(auv-inference-yolo): add detection example artifact"
```

## Task 8: Workspace Verification And Documentation Notes

**Files:**
- Modify if needed: `docs/superpowers/specs/2026-06-04-auv-inference-yolo-design.md`

- [ ] **Step 1: Run focused crate tests**

Run: `cargo test -p auv-inference-yolo`

Expected: PASS.

- [ ] **Step 2: Run workspace checks**

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 3: Update spec only if the implementation changed the agreed boundary**

If implementation required a boundary change, update `docs/superpowers/specs/2026-06-04-auv-inference-yolo-design.md` with the final decision. Do not add YOLO implementation details to `docs/TERMS_AND_CONCEPTS.md` in this slice because no AUV core term changes.

- [ ] **Step 4: Commit verification/doc update if needed**

If Step 3 changed the spec, run:

```bash
git add docs/superpowers/specs/2026-06-04-auv-inference-yolo-design.md
git commit -m "docs: update yolo inference design after implementation"
```

If Step 3 did not change files, do not create an empty commit.

## Self-Review

- Spec coverage: Tasks cover crate creation, Ultralytics v8/v11-like layout, class labels, image fixtures, `ort` inference, letterbox, reverse projection, confidence filtering, NMS, typed detections, callable example output, annotated image artifacts, and Balatro fixture parity.
- Placeholder scan: no task asks the implementer to invent unspecified behavior; code snippets define the intended signatures and behavior.
- Type consistency: public names match the spec: `YoloDetector`, `YoloModelConfig`, `DetectionOptions`, `DetectionSet`, `Detection`, and `BoundingBox`.
- Scope check: OCR, Balatro game logic, AUV driver capture, media stream, `auv invoke`, Steam Play automation, Swift overlay, and `auv-shared` remain explicitly deferred.
