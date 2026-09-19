# driver

Platform drivers, input, window, capture, permissions

Count: **42**

- [`2026-05-20-macos-driver-namespace-after-window-screen-design.md`](2026-05-20-macos-driver-namespace-after-window-screen-design.md)
- [`2026-05-20-macos-osascript-backend-design.md`](2026-05-20-macos-osascript-backend-design.md)
- [`2026-05-20-macos-swift-bridge-migration-design.md`](2026-05-20-macos-swift-bridge-migration-design.md)
- [`2026-05-20-window-screen-ocr-click-design.md`](2026-05-20-window-screen-ocr-click-design.md)
- [`2026-05-20-window-screen-ocr-click-implementation-plan.md`](2026-05-20-window-screen-ocr-click-implementation-plan.md)
- [`2026-05-25-driver-platform-api-crates-design.md`](2026-05-25-driver-platform-api-crates-design.md)
- [`2026-05-26-driver-capture-input-interaction-roadmap.md`](2026-05-26-driver-capture-input-interaction-roadmap.md)
- [`2026-05-26-macos-capture-fast-path-design.md`](2026-05-26-macos-capture-fast-path-design.md)
- [`2026-05-26-macos-driver-legacy-typed-handoff.md`](2026-05-26-macos-driver-legacy-typed-handoff.md)
- [`2026-05-26-macos-no-steal-input-design.md`](2026-05-26-macos-no-steal-input-design.md)
- [`2026-06-04-driver-command-bridge-design.md`](2026-06-04-driver-command-bridge-design.md)
- [`2026-06-04-driver-command-migration-matrix.md`](2026-06-04-driver-command-migration-matrix.md)
- [`2026-06-04-media-macos-now-playing-design.md`](2026-06-04-media-macos-now-playing-design.md)
- [`2026-06-05-driver-foreground-input-design.md`](2026-06-05-driver-foreground-input-design.md)
- [`2026-06-05-driver-permission-probe-design.md`](2026-06-05-driver-permission-probe-design.md)
- [`2026-06-05-window-management-api-design.md`](2026-06-05-window-management-api-design.md)
- [`2026-06-05-window-management-api-v0.md`](2026-06-05-window-management-api-v0.md)
- [`2026-06-11-windows-driver-feasibility-and-delivery-paths.md`](2026-06-11-windows-driver-feasibility-and-delivery-paths.md)
- [`2026-06-16-tracing-driver-extraction-implementation-plan.md`](2026-06-16-tracing-driver-extraction-implementation-plan.md)
  (historical; superseded by the implemented
  [run recording contract V1 spec](../inspect/2026-07-20-auv-run-recording-contract-v1-spec.md))
- [`2026-06-18-driver-windows-v0-implementation.md`](2026-06-18-driver-windows-v0-implementation.md)
- [`2026-07-19-error-chain-inventory.md`](2026-07-19-error-chain-inventory.md)
- [`2026-07-30-overlay-interface-and-debug-commands-handoff.md`](2026-07-30-overlay-interface-and-debug-commands-handoff.md)
- [`2026-08-01-qemu-window-input-framework-research.md`](2026-08-01-qemu-window-input-framework-research.md)
- [`2026-08-05-obs-platform-capture-backends-research.md`](2026-08-05-obs-platform-capture-backends-research.md)
- [`2026-08-05-mouse-motion-streaming-design.md`](2026-08-05-mouse-motion-streaming-design.md)
- [`2026-08-06-open-source-mouse-motion-implementation-research.md`](2026-08-06-open-source-mouse-motion-implementation-research.md)
- [`2026-08-06-input-performance-evidence.md`](2026-08-06-input-performance-evidence.md)
- [`2026-08-09-orca-computer-use-comparison-note.md`](2026-08-09-orca-computer-use-comparison-note.md)
- [`2026-08-30-linux-wayland-pipewire-capture-runtime-design.md`](2026-08-30-linux-wayland-pipewire-capture-runtime-design.md)

- [`2026-09-07-overlay-host-theme.md`](2026-09-07-overlay-host-theme.md)

- [`2026-09-11-click-modifiers-contract.md`](2026-09-11-click-modifiers-contract.md)
- [`2026-09-09-background-ax-and-media-gap-review.md`](2026-09-09-background-ax-and-media-gap-review.md): Deeper background input and AX review, plus microphone/system-audio distinctions and three media capability candidates with pinned source evidence.
- [`2026-09-09-computer-use-code-and-upstream-review.md`](2026-09-09-computer-use-code-and-upstream-review.md): Follow-up source audit, dated upstream changes, correctness risks, KWWK native-core lineage, and additional component references.
- [`2026-09-09-computer-use-framework-comparison-note.md`](2026-09-09-computer-use-framework-comparison-note.md): Source-level comparison of AUV, Peekaboo, CUA and kwwk, separating native OS capabilities from frontend exposure and infrastructure.
- [`2026-09-09-computer-use-improvement-candidates.md`](2026-09-09-computer-use-improvement-candidates.md): Thirty-one separately reviewable improvements with priority, difficulty, trigger flows, AUV/upstream permalinks, and acceptance criteria; includes atomic APIs, platform models, scripting, Windows UIA, and remote Runner lifecycle.
- [`2026-09-13-wayland-background-input-research.md`](2026-09-13-wayland-background-input-research.md): Accepted scope, implementation PRs, Portal persistence, compositor alternatives, and evidence boundaries.

- [`2026-09-12-linux-portal-authorization-and-runner-reuse.md`](2026-09-12-linux-portal-authorization-and-runner-reuse.md) — first-party Portal identity, explicit authorization, SDK process ownership, local Runner reuse, Run correlation, and validation limits.

- [`2026-09-12-uinput-and-wayland-validation.md`](2026-09-12-uinput-and-wayland-validation.md)

- [`2026-09-13-mobile-use-platform-backends-research.md`](2026-09-13-mobile-use-platform-backends-research.md): Source-backed mobile agent modes, Android/iOS/Harmony backends, clone inventory, and OpenHarmony evidence boundaries.

- [`2026-09-14-iphone-phone-use-source-review.md`](2026-09-14-iphone-phone-use-source-review.md): Source review of eight iPhone phone-use projects, DeviceKit, input paths, setup dependencies, and visual-loop limits.

- [`2026-09-19-positional-targets.md`](2026-09-19-positional-targets.md): Window-bound positional targets, NetEase migration, and live validation evidence.

## Related

- Parent index: [`../INDEX.md`](../INDEX.md)
- Docs overview: [`../../../README.md`](../../../README.md)
- Shared vocabulary: [`../../../TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md)

- [`2026-09-17-click-buttons-contract.md`](2026-09-17-click-buttons-contract.md): BG-1 left/right/middle integration, API changes, and receiver validation.
