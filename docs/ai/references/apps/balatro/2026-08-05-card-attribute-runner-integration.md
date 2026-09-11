# Balatro card-attribute Runner integration

Date: 2026-08-05

## Current contract

Balatro observation owns four full-frame card-attribute detectors in addition
to the existing entities, optional hand-card, and UI detectors:

- `balatro-card-identity`, input size 960
- `balatro-card-enhancement`, input size 640
- `balatro-card-edition`, input size 640
- `balatro-card-seal`, input size 640

The default assets are the corresponding published `proj-airi` Mod
ground-truth ONNX model repositories. CLI model-path options override each
asset independently. `--device` is translated into the Runner's typed
`InferenceDevice`; no CUDA index is embedded in the operation. CUDA-capable
Runner builds enable the crate's `cuda` feature. A CUDA request is rejected
when that compile-time provider capability is absent, rather than silently
running the detector set on CPU.

The observation client sends one full-frame batch RPC. The Balatro Runner
decodes that frame once and runs independent detectors concurrently. The
optional normalized-hand detector remains a separate concurrent request because
it consumes a different cropped frame. The Runner resolves a cache key from detector ID,
model path, input size, thresholds, maximum detections, device, and class-name
override. Its per-key `OnceLock` initializes an ONNX session once and returns
the same `Arc` on later observations. Different keys initialize and infer
independently; the cache-map mutex is not held during model loading.

Identity detections enrich the existing hand-slot `reading`. Enhancement,
edition, and seal detections enrich the typed `attributes` object. Association
uses a greedy, descending-IoU one-to-one assignment with a minimum IoU of 0.2.
One attribute detection therefore cannot read multiple overlapping hand slots.
Raw evidence from every model remains available in `raw_entities` so incorrect
associations can be diagnosed without treating the fused state as ground truth.

## Evidence

Focused unit coverage verifies:

- trained input size and CUDA index survive protobuf construction;
- rank/suit and three attribute results associate with the intended hand slot;
- identical cache keys initialize once and return the same session;
- distinct model keys can enter their loaders concurrently.
- an empty detector batch is rejected before frame decoding;
- one identity box cannot enrich two overlapping hand slots.

On `neko-gpu-1` (RTX 4080 SUPER), CUDA activation required both the crate
feature and CUDA 12 runtime libraries in `LD_LIBRARY_PATH`. `nvtop -s`, loaded
library inspection, and `nvidia-smi` then showed the Runner using the CUDA
provider; framebuffer memory rose from roughly 0.9 GiB to 2.2 GiB while the six
sessions were resident. A 2198x562 fixed-frame hot observation measured
2.00-2.16 seconds before batching and 2.03-2.14 seconds after batching. A
2560x1440 live observation remained about 6.1 seconds when warm, showing that
per-model resize/preprocessing and inference dominate over repeated gRPC frame
decoding. Cold live observations measured roughly 9.7-12.6 seconds.

Live evidence also showed that the identity model can emit several distinct
`C_7` detections for cards in the generated overlay fixture. The one-to-one
fusion fix prevents reuse of one box, but it cannot repair independent model
misclassifications; test-set mAP is not yet a live-domain accuracy claim.

TODO(card-attribute-association-fixtures): replace the provisional IoU threshold
with fixture-derived matching policy after representative live frames include
overlapping, highlighted, debuffed, and face-down hands.
