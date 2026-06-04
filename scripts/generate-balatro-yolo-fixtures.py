#!/usr/bin/env python3
"""Generate Balatro YOLO ONNX fixture outputs for AUV tests."""

from __future__ import annotations

import json
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import cv2
import numpy as np
import onnxruntime as ort


BALATRO_ROOT = Path('/Users/neko/Git/github.com/proj-airi/game-playing-ai-balatro')
CONFIDENCE_THRESHOLD = 0.25
IOU_THRESHOLD = 0.45
FIXTURE_IMAGE_NAME = 'balatro.jpg'


@dataclass(frozen=True)
class ModelSpec:
    name: str
    model_asset: str
    classes_asset: str
    output_path: Path

    @property
    def model_path(self) -> Path:
        return balatro_path(self.model_asset)

    @property
    def classes_path(self) -> Path:
        return balatro_path(self.classes_asset)


@dataclass(frozen=True)
class Letterbox:
    ratio: float
    pad_left: float
    pad_top: float
    input_width: int
    input_height: int


def auv_root() -> Path:
    return Path(__file__).resolve().parents[1]


def balatro_path(relative_path: str) -> Path:
    return BALATRO_ROOT / relative_path


def read_classes(path: Path) -> list[str]:
    return [
        line.strip()
        for line in path.read_text(encoding='utf-8').splitlines()
        if line.strip()
    ]


def model_input_size(session: ort.InferenceSession) -> tuple[int, int]:
    shape = session.get_inputs()[0].shape
    height, width = shape[2], shape[3]
    if isinstance(height, int) and isinstance(width, int):
        return width, height
    return 640, 640


def preprocess(
    rgb_image: np.ndarray, input_width: int, input_height: int
) -> tuple[np.ndarray, Letterbox]:
    source_height, source_width = rgb_image.shape[:2]
    ratio = min(input_width / source_width, input_height / source_height)
    resized_width = round(source_width * ratio)
    resized_height = round(source_height * ratio)

    resized = cv2.resize(
        rgb_image, (resized_width, resized_height), interpolation=cv2.INTER_LINEAR
    )
    padded = np.full((input_height, input_width, 3), 114, dtype=np.uint8)

    pad_left = (input_width - resized_width) / 2
    pad_top = (input_height - resized_height) / 2
    left = int(round(pad_left - 0.1))
    top = int(round(pad_top - 0.1))
    padded[top : top + resized_height, left : left + resized_width] = resized

    input_tensor = padded.astype(np.float32) / 255.0
    input_tensor = np.transpose(input_tensor, (2, 0, 1))
    input_tensor = np.expand_dims(input_tensor, axis=0)
    return input_tensor, Letterbox(
        ratio=ratio,
        pad_left=float(left),
        pad_top=float(top),
        input_width=input_width,
        input_height=input_height,
    )


def iou(left: np.ndarray, right: np.ndarray) -> float:
    inter_x1 = max(left[0], right[0])
    inter_y1 = max(left[1], right[1])
    inter_x2 = min(left[2], right[2])
    inter_y2 = min(left[3], right[3])
    inter_width = max(0.0, inter_x2 - inter_x1)
    inter_height = max(0.0, inter_y2 - inter_y1)
    intersection = inter_width * inter_height

    left_area = max(0.0, left[2] - left[0]) * max(0.0, left[3] - left[1])
    right_area = max(0.0, right[2] - right[0]) * max(0.0, right[3] - right[1])
    union = left_area + right_area - intersection
    if union <= 0:
        return 0.0
    return float(intersection / union)


def nms(detections: list[dict[str, Any]]) -> list[dict[str, Any]]:
    kept: list[dict[str, Any]] = []
    remaining = sorted(
        detections, key=lambda detection: detection['confidence'], reverse=True
    )

    while remaining:
        current = remaining.pop(0)
        kept.append(current)
        current_bbox = np.array(current['bbox'], dtype=np.float32)
        remaining = [
            candidate
            for candidate in remaining
            if candidate['class_id'] != current['class_id']
            or iou(current_bbox, np.array(candidate['bbox'], dtype=np.float32))
            < IOU_THRESHOLD
        ]

    return kept


def decode_output(
    output: np.ndarray,
    classes: list[str],
    letterbox: Letterbox,
    image_width: int,
    image_height: int,
) -> list[dict[str, Any]]:
    if output.ndim != 3 or output.shape[0] != 1:
        raise ValueError(
            f'expected Ultralytics ONNX output [1, C, N], got {list(output.shape)}'
        )

    channels = output.shape[1]
    if channels < 5:
        raise ValueError(f'expected at least 5 output channels, got {channels}')

    expected_channels = 4 + len(classes)
    if channels != expected_channels:
        raise ValueError(
            f'expected 4 + class_count channels ({expected_channels}), got {channels}'
        )

    predictions = output[0].T
    boxes = predictions[:, :4]
    class_scores = predictions[:, 4:]
    class_ids = np.argmax(class_scores, axis=1)
    confidences = class_scores[np.arange(class_scores.shape[0]), class_ids]

    candidate_indices = np.nonzero(confidences >= CONFIDENCE_THRESHOLD)[0]
    detections: list[dict[str, Any]] = []
    for index in candidate_indices:
        x_center, y_center, width, height = boxes[index]

        x1 = (x_center - width / 2 - letterbox.pad_left) / letterbox.ratio
        y1 = (y_center - height / 2 - letterbox.pad_top) / letterbox.ratio
        x2 = (x_center + width / 2 - letterbox.pad_left) / letterbox.ratio
        y2 = (y_center + height / 2 - letterbox.pad_top) / letterbox.ratio

        clipped = [
            round(float(np.clip(x1, 0, image_width)), 3),
            round(float(np.clip(y1, 0, image_height)), 3),
            round(float(np.clip(x2, 0, image_width)), 3),
            round(float(np.clip(y2, 0, image_height)), 3),
        ]
        class_id = int(class_ids[index])
        detections.append(
            {
                'class_id': class_id,
                'label': classes[class_id],
                'confidence': round(float(confidences[index]), 6),
                'bbox': clipped,
            }
        )

    return nms(detections)


def run_model(
    spec: ModelSpec, rgb_image: np.ndarray, image_path: Path
) -> dict[str, Any]:
    classes = read_classes(spec.classes_path)
    session = ort.InferenceSession(
        str(spec.model_path), providers=['CPUExecutionProvider']
    )
    input_name = session.get_inputs()[0].name
    input_width, input_height = model_input_size(session)
    input_tensor, letterbox = preprocess(rgb_image, input_width, input_height)
    output = session.run(None, {input_name: input_tensor})[0]
    image_height, image_width = rgb_image.shape[:2]
    detections = decode_output(output, classes, letterbox, image_width, image_height)

    return {
        'model': {
            'name': spec.name,
            'balatro_asset': spec.model_asset,
        },
        'classes': {
            'balatro_asset': spec.classes_asset,
            'count': len(classes),
            'labels': classes,
        },
        'image': {
            'path': str(image_path.relative_to(auv_root())),
            'source_balatro_asset': SOURCE_IMAGE_ASSET,
            'width': image_width,
            'height': image_height,
        },
        'thresholds': {
            'confidence': CONFIDENCE_THRESHOLD,
            'iou': IOU_THRESHOLD,
        },
        'preprocess': {
            'color': 'RGB',
            'letterbox': {
                'ratio': round(letterbox.ratio, 8),
                'pad_left': letterbox.pad_left,
                'pad_top': letterbox.pad_top,
                'input_width': letterbox.input_width,
                'input_height': letterbox.input_height,
            },
        },
        'output_shape': list(output.shape),
        'detection_count': len(detections),
        'detections': detections,
    }


SOURCE_IMAGE_ASSET = 'data/datasets/games-balatro-2024-entities-detection/data/val/yolo/images/out_01707.jpg'
SOURCE_IMAGE = balatro_path(SOURCE_IMAGE_ASSET)
FIXTURE_DIR = auv_root() / 'crates/auv-inference-ultralytics/tests/fixtures/balatro'


def main() -> None:
    if not BALATRO_ROOT.exists():
        raise FileNotFoundError(f'Balatro repo root not found: {BALATRO_ROOT}')
    if not SOURCE_IMAGE.exists():
        raise FileNotFoundError(f'Balatro source image not found: {SOURCE_IMAGE}')

    FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
    fixture_image = FIXTURE_DIR / FIXTURE_IMAGE_NAME

    if fixture_image.resolve().is_relative_to(BALATRO_ROOT.resolve()):
        raise RuntimeError(
            f'fixture image would be written under Balatro root: {fixture_image}'
        )
    shutil.copyfile(SOURCE_IMAGE, fixture_image)

    bgr_image = cv2.imread(str(fixture_image), cv2.IMREAD_COLOR)
    if bgr_image is None:
        raise RuntimeError(f'failed to read fixture image: {fixture_image}')
    rgb_image = cv2.cvtColor(bgr_image, cv2.COLOR_BGR2RGB)

    specs = [
        ModelSpec(
            name='games-balatro-2024-yolo-entities-detection',
            model_asset='models/games-balatro-2024-yolo-entities-detection/onnx/model.onnx',
            classes_asset='data/datasets/games-balatro-2024-entities-detection/data/train/yolo/classes.txt',
            output_path=FIXTURE_DIR / 'entities.json',
        ),
        ModelSpec(
            name='games-balatro-2024-yolo-ui-detection',
            model_asset='models/games-balatro-2024-yolo-ui-detection/onnx/model.onnx',
            classes_asset='data/datasets/games-balatro-2024-ui-detection/data/train/yolo/classes.txt',
            output_path=FIXTURE_DIR / 'ui.json',
        ),
    ]

    for spec in specs:
        for path in [spec.model_path, spec.classes_path]:
            if not path.exists():
                raise FileNotFoundError(f'required Balatro asset not found: {path}')
        metadata = run_model(spec, rgb_image, fixture_image)
        spec.output_path.write_text(
            json.dumps(metadata, indent=2, sort_keys=True) + '\n',
            encoding='utf-8',
        )
        print(
            f'{spec.output_path}: shape={metadata["output_shape"]} detections={metadata["detection_count"]}'
        )


if __name__ == '__main__':
    main()
