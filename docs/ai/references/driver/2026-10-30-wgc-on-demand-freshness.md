# WGC on-demand frame freshness

Status: native regression reproduced before repair. Stream reuse is implemented; native repair acceptance is pending.

Classification: bug fix from the owner's Windows Phase 2 capture-freshness goal.
Base: `main@d9d2bdcc6a27da0c1a70a17c4b265c56cbd0f7f3`.
This work is independent of closed PR #429 and its retired screenshot predicate.

## Problem and regression

The on-demand WGC image path retains one global two-buffer frame pool.
Its callback sends wake tokens but does not consume frames.
When the caller is idle, that FIFO fills and later paints can be dropped.
The next capture returns its oldest queued frame, even after the application has completed newer paints.
Draining only at read time cannot recover a final paint that never entered the full pool.
The [earlier background-session evidence](2026-10-10-windows-background-session.md#one-shot-wgc-capture-returns-an-older-frame)
records this behavior separately from the continuously drained `FrameStream`.

The regression uses the existing native WGC test window and its public Driver APIs.
It warms on-demand capture, then paints red, green, and yellow without another on-demand read.
Each paint must match `PrintWindow` and an independent continuously drained WGC pool.
The first subsequent on-demand capture must return yellow, without a capture retry.
Only the owned test window is painted. No mouse or keyboard input is needed.
Native execution belongs on an isolated interactive Windows desktop; local validation compiles the test without running it.

## Repair boundary

Reuse the existing `FrameStream` for both on-demand window and display images.
Extend its internal construction to accept the already-resolved capture item and Direct3D context.
Keep that constructor private to the WGC module.
The existing one-target cache must own the stream, its target dimensions, and its last CPU image.
Its callback drains the pool while callers are idle and retains only the latest received GPU texture.
CPU mapping remains on demand; an unchanged read may reuse its last image.
Target switches, size changes, stream errors, and device loss must release the old stream instead of returning its cached image.

This contract means latest received frame, not a frame produced after the request started.
It does not establish that the compositor has painted a pending application action.
It does not make capture and input atomic or validate a click's decision premise.
The [#429 closure decision](https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529) remains in effect.

The lightweight health sampler is a separate producer with a frozen latency boundary.
Its policy, freshness, and pixel thresholds remain outside this image-capture repair.
No Driver method, command, RPC, SDK accessor, or input policy is added.

## Native reproduction

The [test-only run](https://github.com/moeru-ai/auv/actions/runs/40915621800)
used source `369fbdf20416bc21cecac9102cf2dc231a0a18ab` and workflow `c8e0e54b30d56ca7123d51036ef1956d7d0b7e3d`.
`PrintWindow` matched all three requested paints.
The independent stream reported red at frame 3, green at frame 5, and yellow at frame 7.
The first subsequent on-demand read returned red in 8.4952 ms, and the named regression failed.
The workflow required that exact assertion failure; compilation or setup failure cannot count as reproduction.

Preflight recorded Windows Server 2025 build 26100, hosted image `20261021.278.1`, interactive session 2, and an accessible input desktop.
The one display was 1024 by 768, and an owned hidden per-monitor-v2 window reported 96 DPI.
The probe destroyed its window and restored its thread context without changing system scaling.
The [raw regression artifact](https://github.com/moeru-ai/auv/actions/runs/40915621800/artifacts/12916061290)
has SHA-256 `1c8ed396964a7baa0b6f64a215e47b18ae78db68d050798497b54faae0f3a86a`.
The downloaded archive matched this digest. Retention ends 2026-11-12.

The [first attempt](https://github.com/moeru-ai/auv/actions/runs/40915080388) stopped at a test-only brush-handle comparison compile error.
Its artifact remains separate and does not count as stale-frame evidence.

## Validation still required

- Run the unchanged regression against the repaired implementation on an isolated desktop.
- Run existing WGC pixel, occlusion, resize, display, health-concurrency, and COM-apartment checks.
- Check formatting, whitespace, and affected Rust targets without local GUI execution.
- Record exact source, workflow, desktop, actual DPI, raw logs, and artifact digest.

No functional or performance claim follows from test compilation or this plan.
