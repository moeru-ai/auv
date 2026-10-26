# driver

Platform drivers, input, window, capture, permissions

Executable GUI evaluations live in
[`evals/auv-base`](../../../../evals/auv-base/README.md), organized by platform with
test objects, tasks, and cases. Rust runs evaluations; shared desktop receivers
use TypeScript. Raw run output stays local.

Count: **82**

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
- [`2026-08-07-wayland-capture-obs-sunshine-evidence.md`](2026-08-07-wayland-capture-obs-sunshine-evidence.md): Wayland portal/PipeWire capture lifecycle compared with OBS Studio and Sunshine at pinned commits.
- [`2026-08-09-orca-computer-use-comparison-note.md`](2026-08-09-orca-computer-use-comparison-note.md)
- [`2026-08-30-linux-wayland-pipewire-capture-runtime-design.md`](2026-08-30-linux-wayland-pipewire-capture-runtime-design.md)

- [`2026-09-07-overlay-host-theme.md`](2026-09-07-overlay-host-theme.md)
- [`2026-10-09-overlay-windows-direct2d-evidence.md`](2026-10-09-overlay-windows-direct2d-evidence.md): Windows overlay drawing moved from GDI to a Direct2D DC render target (real premultiplied alpha, antialiasing, DirectWrite labels, cursor glow, resvg SVG cursors); pixel tests and before/after screen captures.
- [`2026-10-10-windows-overlay-motion.md`](2026-10-10-windows-overlay-motion.md): Windows live overlay that follows real driver input (spring-following cursor with tilt and press, click ripples, window marks), with one colored cursor per window and caller-written status pills, its faithfulness rules, report sites, the owner-chosen rounded built-in pointer with a silhouette shadow, and frame-time, latency and per-window color evidence from a scripted harness and from real window-targeted operations.
- [`2026-10-10-windows-background-session.md`](2026-10-10-windows-background-session.md): Windows background session: up to five covered windows operated by posted click and wheel and observed through per-window frame streams, with typed outcomes (`skipped_minimized`, `target_lost`, `unsupported`), no re-discovery and no foreground route; the launch foreground grant, the one-shot WGC stale-frame and resize-alignment findings, a per-application survey (Character Map, Calculator, Edge with and without native occlusion), and single, five-window, 10-minute benchmark and fault-injection evidence.

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

- [`2026-09-18-held-input-design.md`](2026-09-18-held-input-design.md): Held mouse input contract: shared logical mouse state, ordered admission, cleanup, three-platform adapters, and validation boundaries.
- [`2026-09-23-held-input-native-validation.md`](2026-09-23-held-input-native-validation.md): BG-1 Windows background/RDP and Linux uinput receipts, Portal protocol validation, reproducible tests, and remaining live evidence gaps.
- [`2026-09-24-keyboard-hold-contract.md`](2026-09-24-keyboard-hold-contract.md): Timed and cross-call keyboard holds, the held-modifier fix, and receiver-backed diagnosis of background command-target and Unicode IME-focus failures on 2026-09-25.
- [`2026-09-26-no-raise-keyboard-probe.md`](2026-09-26-no-raise-keyboard-probe.md): Swift/Electron/Chrome experiments with no-raise activation, key-window records, explicit restoration, and independent agent-browser receipts; test-only evidence.
- [`2026-09-18-held-input-project-research.md`](2026-09-18-held-input-project-research.md): Pinned-source comparison of held-input APIs, cleanup lifecycles, and independent observation.
- [`2026-09-19-positional-targets.md`](2026-09-19-positional-targets.md): Window-bound positional targets, NetEase migration, and live validation evidence.
- [`2026-09-23-background-delivery-project-comparison.md`](2026-09-23-background-delivery-project-comparison.md): BG-2 source comparison of CUA, KWWK, locally bundled OpenAI Sky, and MaaFramework; transport selection, click counts, and evidence limits.
- [`2026-09-23-background-keyboard-authentication.md`](2026-09-23-background-keyboard-authentication.md): Authenticated macOS keyboard submission, guarded public fallback, native contract tests, and AppKit/Chrome/Electron receiver evidence, including failing compatibility cases.
- [`2026-09-27-macos-lock-screen-research.md`](2026-09-27-macos-lock-screen-research.md): macOS locked session, LoginWindow, and FileVault preboot boundaries; Apple APIs and remote-host source evidence.
- [`2026-09-27-windows-linux-login-screen-research.md`](2026-09-27-windows-linux-login-screen-research.md): Windows secure-desktop and Linux greeter capture/input source review.
- [`2026-09-27-sky-cua-lock-screen-research.md`](2026-09-27-sky-cua-lock-screen-research.md): Public Codex issue and open-source CUA paths, with lock-screen evidence limits.
- [`2026-09-27-lock-and-login-screen-deep-research.md`](2026-09-27-lock-and-login-screen-deep-research.md): Pinned-source follow-up on secure desktop APIs, session workers, greeter capture, AUV Runner placement, and credential boundaries.
- [`2026-10-01-linux-screenshot-fallback.md`](2026-10-01-linux-screenshot-fallback.md): Partial-image regression, single-output coordinate checks, and live GNOME Screenshot evidence.
- [`2026-10-01-linux-window-capture-reference-validation.md`](2026-10-01-linux-window-capture-reference-validation.md): Exact AT-SPI reference resolution, current-frame crops, before/after checks, and live regression evidence.
- [`2026-10-03-windows-driver-latency-baseline.md`](2026-10-03-windows-driver-latency-baseline.md): Windows 驱动延迟基线实测（capture_display/capture_window/SendInput/OCR 共 2,100 样本实测分布）与 WGC 立项决策依据。
- [`2026-10-03-wgc-driver-implementation.md`](2026-10-03-wgc-driver-implementation.md): Windows 驱动 WGC v1 截图后端实施、正确性验收测试与延迟实测报告（1440p P50 达 10.75ms，相比 GDI 提升 17x）。
- [`2026-10-03-wgc-gpu-load-degradation.md`](2026-10-03-wgc-gpu-load-degradation.md): GPU 负载下 WGC vs GDI 延迟与退化比实测报告（RTX 4070 Ti 100% 满载下 WGC P50 为 11.28ms，退化比 1.05x，新帧率 100%）。
- [`2026-10-06-scroll-motion-design.md`](2026-10-06-scroll-motion-design.md): Timed scroll (timing functions, cumulative quantization, `ScrollWindowPointMotion`, `input.scroll --duration-ms`), Linux uinput high-resolution wheel, live velocity control (`StreamScroll`, SDK `scrollWith` generators), scroll-until (`ScrollUntil` with observations and client predicates, `input.scrollUntil`, shared viewport pixel motion), and live evidence on macOS, Windows, and Linux (Linux with the #246 capture fix).
- [`2026-10-06-capture-references-and-positions-design.md`](2026-10-06-capture-references-and-positions-design.md)
  (Part A implemented: captures stay in the Runner's capture store and travel
  as `CaptureRef`s, `GetCaptureImage` is the only pixel transfer; Part B
  proposed: one `Position`-based coordinate model)
- [`2026-10-06-scroll-delta-contract.md`](2026-10-06-scroll-delta-contract.md): Scroll delta convention (logical px, positive = down) and its implementation across drivers, `ScrollWindowPoint`, `input.scroll`, and the JS SDK; Playwright/CUA comparison; macOS Chrome/Electron and Windows Edge evidence (occlusion backgrounding blocks macOS background scroll; Windows posted wheel reaches Chromium only in the foreground; Linux portal and uinput fixed to discrete 120 px notches on GNOME).
- [`2026-10-07-verified-fast-path-spike.md`](2026-10-07-verified-fast-path-spike.md): QQ 音乐 Step 3 验证快慢路径触发条件单变量实测（间隔/生命周期/播放位置/事件时序 4 维度 96 样本实测分布）、交替极限环机理与 NO-GO 裁决依据。
- [`2026-10-09-pointer-position.md`](2026-10-09-pointer-position.md): Pointer, logical mouse and cursor kept apart; `GetPointerPosition`, `ScrollPoint` (scroll without a target window) and typed `key_down`/`key_up`; deferred logical-mouse reads and `WatchPointer`/`WatchMice` streams; macOS evidence.
- [`2026-10-12-windows-paste-text.md`](2026-10-12-windows-paste-text.md): Windows `paste_text` for `input.pasteText` and `input.keyboard`: delayed clipboard rendering as the consumption boundary (the text is handed over only when a program asks for it, and the user's clipboard comes back right after the foreground application reads it), exclusion from clipboard history, cloud and monitors, a fail-and-restore result when nothing reads it in 2 s, the VirtualBox shared-clipboard and IME-composition findings, and live receiver evidence.
- [`2026-10-12-windows-media-control.md`](2026-10-12-windows-media-control.md): `mediaControl.*` on Windows: the platform-neutral now-playing contract (`now-playing-v0`, the command verification rule) moved to `auv-driver-common::media` with `auv-media-macos` re-exporting it, the SMTC current-session mapping onto it, controls that poll the same session until the effect shows or 3 s pass, live evidence for all six commands against QQ Music, and the finding that a skip's after-state can read not playing while the app switches tracks.
- [`2026-10-12-windows-scroll-until-background.md`](2026-10-12-windows-scroll-until-background.md): Posted-wheel `scroll_until` on the user's QQ Music through `auv invoke` (text 4 of 4, head and tail end stops, four timed motions), the `measure-settle` bench and why the 400 ms settle default stays, the `--region` requirement for multi-pane windows, and why the uncontrolled no-motion runs on Edge, Claude, NTQQ, Steam and VirtualBox carry no claim.
- [`2026-10-12-windows-targeted-keyboard-probe.md`](2026-10-12-windows-targeted-keyboard-probe.md): Raw posted-keyboard probe, not command evidence: `WM_CHAR` reaches a classic `Edit` child but not its top-level window, posted modifiers leave `GetKeyState` unchanged, the records' contradictions, and what a targeted route would need; the keyboard cells stay foreground-only.
- [`2026-10-13-windows-app-activation-probe.md`](2026-10-13-windows-app-activation-probe.md): Driver trials of window activation and the permission probe on two test-owned windows, why `app.activate` and `app.probePermissions` stay `Unsupported`, and corrections to the original document (the foreground-lock claim, `skipped_minimized`, "Resolution B adopted").
- [`2026-10-14-windows-window-clicktext.md`](2026-10-14-windows-window-clicktext.md): Direct invoke route for `window.clickText` on Windows under `InputPolicy::BackgroundOnly`, button OCR sizing fixes, coordinate translation verification (PR #323), typed `NotFound` negative path, and test-owned Win32 receiver evidence.
- [qqmusic-background-control.md](qqmusic-background-control.md): Living document tracking QQ Music background control without stealing foreground focus across SMTC, CoreAudio, and UIA phases.
- [windows-capture-parity.md](windows-capture-parity.md): Living document tracking Windows capture latency parity with macOS ScreenCaptureKit across baseline, WGC v1, and GPU-load degradation phases.
- [`2026-10-14-osu-phase0-s1a-report.md`](2026-10-14-osu-phase0-s1a-report.md): osu! Windows 注入基准与即兴演奏器 Phase 0（执行器骨架、高精度 QPC 混合自旋、P99 0.525ms <= 16.6ms 门禁、50/50 边缘对账）与 S1 Part A（真实谱面 Tear Rain Normal 独立官方对账、4ms 轨迹与交替按键计划合成）验证报告。
- [`2026-10-26-osu-bench-closeout.md`](2026-10-26-osu-bench-closeout.md): Windows osu! bench / sight-read player line closeout: shipped SendInput bench, night runs live evidence, not-executed phases, post-merge review follow-ups, and archival.
- [`2026-10-19-linux-portal-eis-input.md`](2026-10-19-linux-portal-eis-input.md): Linux RemoteDesktop portal input through EIS (absolute pointer, buttons, discrete scroll, compositor XKB keyboard), the held-button deadline regression in the shared mouse coordinator and its fix, live GNOME Wayland evidence, and the ablation results.
- [`2026-10-20-linux-window-list-latency.md`](2026-10-20-linux-window-list-latency.md): `window.list` stage-walk latency (17,289 a11y calls, 9 s) reduced to 683 calls by gating the GNOME Shell origin repair and walking both window containers; measurements, ablation, boundaries, and the remaining bounds-only-walk TODO.
- [`2026-10-20-linux-live-desktop-matrix.md`](2026-10-20-linux-live-desktop-matrix.md): live Linux desktop evidence behind the matrix cells: window list/capture/findText, display capture fallback, PaperWM and occlusion boundaries.
- [`2026-10-20-linux-window-text-ocr.md`](2026-10-20-linux-window-text-ocr.md): `window.findText` and `window.waitForText` wired on Linux through AT-SPI resolve, display crop, and Tesseract; visible-area and PaperWM boundaries, no support-matrix claim.
- [`2026-10-20-windows-screen-clicktext.md`](2026-10-20-windows-screen-clicktext.md): Windows local screen OCR click, foreground boundary and owned-receiver evidence.


- [`2026-10-26-windows-input-desktop-lifetime.md`](2026-10-26-windows-input-desktop-lifetime.md): Input desktop ownership, pending close retries, native TLS destruction, attachment errors, and lifecycle test evidence.
- [`2026-10-26-windows-operation-preflight.md`](2026-10-26-windows-operation-preflight.md): Explicit step admission, declared gate thresholds, title changes, timeout limits, and executor test evidence.

## Related

- Parent index: [`../INDEX.md`](../INDEX.md)
- Docs overview: [`../../../README.md`](../../../README.md)
- Shared vocabulary: [`../../../TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md)

- [`2026-09-17-click-buttons-contract.md`](2026-09-17-click-buttons-contract.md): BG-1 left/right/middle integration, API changes, and receiver validation.
