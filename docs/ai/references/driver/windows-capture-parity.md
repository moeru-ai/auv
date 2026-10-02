# Windows capture parity (living doc)

Goal: bring Windows capture latency in line with macOS ScreenCaptureKit (~16 ms/frame). Append new phases here; do not create one doc per commit.

## Phase 1 — Latency baseline (2026-10-03)

Added `latency-telemetry` feature flag (zero cost when off) to 7 driver paths. 300 samples/cell, 2,100 records (`2026-10-03-windows-driver-latency-baseline.jsonl`).

| Path | Backend | P50 | P95 |
|---|---|---|---|
| capture_display (1440p) | xcap/GDI | 186 ms | 191 ms |
| capture_window | PrintWindow | 177 ms | 182 ms |
| click_at / press_key / scroll_at | SendInput | <1 ms | <3 ms |
| recognize_text_in_capture (400x100) | Windows.Media.Ocr | 16 ms | 18 ms |

Note: the 186 ms GDI figure was inflated by CPU contention during MHW shader compilation (see Phase 3); hot-state GDI is ~23–33 ms.

Decision: build a WGC backend.

## Phase 2 — WGC backend v1 (2026-10-03)

Native `windows` crate, no new dependencies. Session cache per target. Frame-pool Recreate on resize. Explicit error on non-B8G8R8A8 formats. Last-frame reuse when DWM has no new frame. Coexists with GDI backends under the `wgc.windows` tag. 1,200 records (`2026-10-03-wgc-latency-benchmark.jsonl`).

| Path (1440p) | Backend | P50 | P95 |
|---|---|---|---|
| capture_display | wgc.windows | 10.75 ms | 18.59 ms |
| capture_display | xcap.windows | 32.89 ms | 36.60 ms |
| capture_window | wgc.windows | 19.41 ms | 22.08 ms |

Acceptance: pixel-exact vs GDI on synthetic windows, occlusion isolation, resize without panic, 92/92 tests green.

Decision: ship WGC v1 (WGC only; DXGI deferred).

## Phase 3 — GPU-load degradation (2026-10-03)

Question: does WGC hold up under a saturated GPU (real gameplay)? MHW rendering at 97.8% mean GPU util (nvidia-smi, 47 s log). 900 records (`2026-10-03-wgc-vs-gdi-load-benchmark.jsonl`).

| Path | Load | P50 | vs idle |
|---|---|---|---|
| capture_display, wgc | 97.8% GPU | 11.28 ms | 1.05x (100% fresh frames) |
| capture_display, GDI | 97.8% GPU | 22.73 ms | 0.69x |
| capture_window, wgc | 97.8% GPU | 20.29 ms | 1.05x (static window: 100% reused frames, by design) |

Correction: GDI is CPU/memory-bandwidth bound, not GPU bound — the Phase 1 spike was CPU contention, not GPU saturation.

Decision: keep WGC. Fast-loop budget ≈ 11+50+20+5 = 86 ms.
