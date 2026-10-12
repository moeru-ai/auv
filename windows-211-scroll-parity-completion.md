# Issue #211 Command Tree Update: Windows `scroll` and `scrollUntil` Parity Evidence (Brief 7)

Date: 2026-10-12  
Workspace: `F:\auv`  
Branch: `feat/windows-scroll-until-parity`  
Related Issue: #211 (*Roadmap of AUV Invoke: cross-platform support*)  
Chain Scope: Windows `scroll_until` parity (Briefs 1 → 7 complete)

---

## 1. Executive Summary

This document closes the Windows scroll parity iteration chain (Briefs 1–7) by converting the `input.scroll` and `input.scrollUntil` rows in Issue #211's command tree from `"source present, live evidence pending"` to **checked-with-evidence** status.

Per #211's rule, checking a box requires linking verifiable live evidence (host environment, session parameters, commands executed, and empirical results). All claims below are backed by committed git revisions and durable JSONL evidence packs produced across Briefs 2 through 6.

### Evidence & Revision Chain Inventory
0. **Prerequisite Fix** ([`c4a491a4`](https://github.com/moeru-ai/auv/commit/c4a491a4)): `fix(auv-cli-invoke): match Windows process names in click_window_selector (#211)`
   - Enables `--target app:<ProcessName>` window resolution on Windows by matching `App::name` instead of bundle ID.
1. **Brief 2** ([`89c9dff3`](https://github.com/moeru-ai/auv/commit/89c9dff3)): `bench(windows): measure scroll settle timing across QQ Music and Explorer`
   - Settle benchmark ($N=110$ runs): QQ Music Warm Reflect P50 12.51ms (P95 14.17ms, Settle P95 30.39ms), Explorer Warm Reflect P50 32.70ms (P95 35.99ms, Settle P95 37.80ms); combined warm settle P95 35.46ms. Established 50ms settle default (1.41x headroom over warm P95).
   - Artifact: [`docs/ai/references/driver/2026-10-12-windows-settle-timing.jsonl`](docs/ai/references/driver/2026-10-12-windows-settle-timing.jsonl)
2. **Brief 3** ([`ddd4c3e0`](https://github.com/moeru-ai/auv/commit/ddd4c3e0)): `test(windows): validate scroll_until TextVisible on QQ Music`
   - End-to-end `TextVisible` loop validation: 4 positive matches, 1 negative (`budget_exhausted` cleanly), 1 multi-pane diagnostic.
   - Artifact: [`docs/ai/references/driver/2026-10-12-windows-scroll-until-qqmusic.jsonl`](docs/ai/references/driver/2026-10-12-windows-scroll-until-qqmusic.jsonl)
3. **Brief 4** ([`357640f4`](https://github.com/moeru-ai/auv/commit/357640f4)): `test(windows): validate scroll_until End detection across content boundaries`
   - End condition validation (`--until end`): Tail boundary clamp (48 steps), Head boundary clamp (34 steps), 50-step budget bound, CEF background drop discovery.
   - Artifact: [`docs/ai/references/driver/2026-10-12-windows-scroll-until-end.jsonl`](docs/ai/references/driver/2026-10-12-windows-scroll-until-end.jsonl)
4. **Brief 5** ([`cc5e8bcf`](https://github.com/moeru-ai/auv/commit/cc5e8bcf)): `test(windows): validate scroll_until Motion step type and quantization`
   - Timed motion & `CumulativeQuantizer` validation: 4 runs (300ms linear, 500ms ease-out, 400ms ease-in-out, 350ms ease-in), 93 sample ticks, exact quantization error bound $\le 5/12\text{ px} \approx 0.4167\text{ px} < 1\text{ quantum}$ ($0.8333\text{ px}$).
   - Artifact: [`docs/ai/references/driver/2026-10-12-windows-scroll-until-motion.jsonl`](docs/ai/references/driver/2026-10-12-windows-scroll-until-motion.jsonl)
5. **Brief 6** ([`c8cc89f3`](https://github.com/moeru-ai/auv/commit/c8cc89f3)): `docs(windows): complete per-app background wheel acceptance matrix`
   - Comprehensive matrix covering 7 applications across 4 GUI toolkits: Win32 native, DirectComposition, Chromium/Electron/CEF, and Qt.
   - Artifact: [`docs/ai/references/driver/2026-10-12-windows-wheel-acceptance-matrix.jsonl`](docs/ai/references/driver/2026-10-12-windows-wheel-acceptance-matrix.jsonl)
   - Living Document: [`docs/ai/references/driver/windows-wheel-acceptance-matrix.md`](docs/ai/references/driver/windows-wheel-acceptance-matrix.md)

---

## 2. Proposed Changes to Issue #211 Command Tree

### Before (from Brief 1 `windows-211-update.md`):
```markdown
  - [ ] `input.scroll` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [ ] Windows local path with toolkit limits (source present, live evidence pending)
  - [ ] `input.scrollUntil` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [ ] Windows local path with toolkit limits (source present, live evidence pending)
```

### Proposed After (incorporating Briefs 2–6 live evidence):
```markdown
  - [ ] `input.scroll` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [x] Windows local path with toolkit limits (evidenced: supported for Win32/DirectComposition; ignored by Chromium/Electron/Qt in background without focus)
  - [ ] `input.scrollUntil` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [x] Windows local path with toolkit limits (evidenced: verified on Win32/DirectComposition across TextVisible, End, and Motion; Chromium/Electron/Qt ignored)
```

*(Note: The top-level command checkboxes `[ ] input.scroll` and `[ ] input.scrollUntil` remain unchecked because the Linux background path is still incomplete. This strictly upholds #211's rule against over-checking).*

---

## 3. Full Proposed Command Tree Section for Issue #211

```markdown
### Current command tree

Rows checked against the source and live evidence as of 2026-10-12 (incorporating #339, #340, #341, #342, and Windows scroll parity validation). Commands added after the initial roadmap draft are marked "(added)".

- Display
  - [x] `display.capture`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
  - [x] `display.list`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
- Screen
  - [x] `screen.captureRegion`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
  - [ ] `screen.findText`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path on the primary display
    - [ ] Windows displays other than the primary display
  - [ ] `screen.waitForText`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path on the primary display
    - [ ] Windows displays other than the primary display
  - [ ] `screen.clickText`
    - [x] macOS local path
    - [ ] Linux local path
    - [ ] Windows local path
- Window
  - [x] `window.list`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
  - [x] `window.capture`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
  - [ ] `window.findText`
    - [x] macOS local path
    - [ ] Linux local path
    - [x] Windows local path
  - [ ] `window.waitForText`
    - [x] macOS local path
    - [ ] Linux local path
    - [x] Windows local path
  - [ ] `window.clickText`
    - [x] macOS local path
    - [ ] Linux local path
    - [ ] Windows local path
- Input: text and focus
  - [ ] `input.focusText`
    - [x] macOS AX path
    - [ ] Linux path
    - [ ] Windows path
  - [ ] `input.axFocusText`
    - [x] macOS compatibility alias
    - [ ] Linux path
    - [ ] Windows path
  - [ ] `input.typeText`
    - [x] macOS foreground and targeted paths
    - [x] Linux foreground path
    - [ ] Linux targeted path
    - [x] Windows foreground path
    - [ ] Windows targeted path
  - [ ] `input.pasteText`
    - [x] macOS foreground and targeted paths
    - [x] Linux foreground path
    - [ ] Linux targeted path
    - [x] Windows foreground path (#341)
    - [ ] Windows targeted path
  - [ ] `input.key`
    - [x] macOS foreground and targeted paths
    - [x] Linux foreground path
    - [ ] Linux targeted path
    - [x] Windows foreground path
    - [ ] Windows targeted path
  - [ ] `input.keys`
    - [x] macOS foreground and targeted paths
    - [x] Linux foreground path
    - [ ] Linux targeted path
    - [x] Windows foreground path
    - [ ] Windows targeted path
  - [ ] `input.pressKeys`
    - [x] Alias for `input.keys`
    - [ ] Remove the duplicate coverage count from support reports
  - [ ] `input.holdKeys`
    - [x] macOS foreground and targeted paths
    - [x] Linux foreground path
    - [ ] Linux targeted path
    - [x] Windows foreground path
    - [ ] Windows targeted path
  - [ ] `input.keyboard`
    - [x] macOS ordered actions
    - [x] Linux foreground ordered actions
    - [ ] Linux targeted ordered actions
    - [x] Windows press, text, and paste (#341) actions
    - [ ] Windows targeted ordered actions
- Input: pointer commands
  - [x] `input.moveMouse`
    - [x] macOS local path
    - [x] Linux local path
    - [x] Windows local path
  - [ ] `input.clickPoint`
    - [x] macOS screen, display, and window bases
    - [x] Linux screen and display bases
    - [ ] Linux window base
    - [x] Windows screen and display bases
    - [x] Windows window base with toolkit limits
    - [ ] Windows live evidence for supported toolkits
  - [ ] `input.pointerPosition` (added)
    - [x] macOS local path
    - [ ] Linux local path (the portal cannot report the pointer position)
    - [x] Windows local path
  - [ ] `input.scrollPoint` (added)
    - [ ] macOS local path (the driver exposes only window-owned scrolling)
    - [x] Linux local path
    - [x] Windows local path
  - [ ] `input.drag` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [x] Windows local path with toolkit limits
  - [ ] `input.scroll` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [x] Windows local path with toolkit limits (evidenced: supported for Win32/DirectComposition; ignored by Chromium/Electron/Qt in background without focus)
  - [ ] `input.scrollUntil` (added)
    - [x] macOS local path
    - [x] Linux foreground path
    - [ ] Linux background path
    - [x] Windows local path with toolkit limits (evidenced: verified on Win32/DirectComposition across TextVisible, End, and Motion; Chromium/Electron/Qt ignored)
- Application
  - [ ] `app.probePermissions`
    - [x] macOS permission model
    - [ ] Linux permission model
    - [ ] Windows permission model
  - [ ] `app.activate`
    - [x] macOS bundle-ID path
    - [ ] Linux application target path
    - [ ] Windows application target path
- Overlay
  - [ ] `overlay.outline`
    - [x] macOS default-feature path
    - [ ] Linux adapter
    - [ ] Windows adapter
  - [ ] `overlay.cursor`
    - [x] macOS default-feature path
    - [ ] Linux adapter
    - [ ] Windows adapter
  - [ ] `overlay.status`
    - [x] macOS default-feature path
    - [ ] Linux adapter
    - [ ] Windows adapter
  - [ ] `overlay.captureFrame`
    - [x] macOS default-feature path
    - [ ] Linux adapter
    - [ ] Windows adapter
  - [ ] `overlay.clickTarget`
    - [x] macOS default-feature path
    - [ ] Linux adapter
    - [ ] Windows adapter
- Media control
  - [ ] `mediaControl.nowPlaying`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path without timeline (#342)
  - [ ] `mediaControl.play`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path (#342)
  - [ ] `mediaControl.pause`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path (#342)
  - [ ] `mediaControl.togglePlayPause`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path (#342)
  - [ ] `mediaControl.next`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path (#342)
  - [ ] `mediaControl.previous`
    - [x] macOS system path
    - [ ] Linux backend and product decision
    - [x] Windows SMTC path (#342)
- Fixture scan
  - [x] `scan.frame`
    - [x] Platform-neutral fixture input
  - [x] `scan.coverage`
    - [x] Platform-neutral fixture input
```

---

## 4. Final Handoff Note for the Entire Windows `scroll_until` Parity Chain

### 4.1 What Windows Scroll Can Now Do (By Application Class)
1. **DirectComposition & Win32 Custom Containers (e.g. QQ Music)**:
   - **Background Wheel Delivery**: 100% operational under `InputPolicy::BackgroundOnly`. Zero cursor stealing, zero window restoration.
   - **Text-Targeted Stopping (`--until text:<query>`)**: Loop captures frames via WGC, executes OCR via `Windows.Media.Ocr`, matches queries using `best_contains`, and halts cleanly at the target item.
   - **End-of-Content Stopping (`--until end`)**: Detects content boundary clamping within 2 steps when pixel motion drops below threshold (`normalized_diff < 0.01`). Tested across head and tail lyrics boundaries (up to 48 steps, $dy = +5760.0$).
   - **Smooth Timed Motion (`--step-duration-ms`)**: `run_window_scroll_motion` + `CumulativeQuantizer` spreads deltas over time using linear, cubic ease-in, cubic ease-out, and cubic ease-in-out functions at 60 Hz. Delivers exact quantized totals within $\le 5/12\text{ px}$ without cumulative drift.
   - **Settle Timing**: Established empirically at 50 ms default (warm reflect P95 is 14.17ms on QQ Music, 35.99ms on Explorer; combined warm settle P95 is 35.46ms with 1.41x margin).
2. **Native Win32 Common Controls (e.g. File Explorer `SysListView32`)**:
   - **Background Wheel Delivery**: 100% operational in background; frame reflect latency measured at warm P50 32.70ms (settle P50 32.71ms).
3. **Chromium, CEF, and Electron Applications (Edge, Chrome, VS Code, Claude, NTQQ, Steam)**:
   - **Structural Input Restriction**: `PostMessageW(WM_MOUSEWHEEL)` is dropped by `views::HWNDMessageHandler` / `RenderWidgetHostViewAura` when the physical mouse cursor is outside the client rect or the window lacks active foreground focus.
   - **AUV Behavior**: The loop observes `0.0` pixel difference and safely terminates on `EndByNoVisualProgress` in 2 steps without hanging or spinning. If scrolling is required on these applications, callers must use `InputPolicy::ForegroundPreferred`.
4. **Qt Applications (e.g. VirtualBox Manager)**:
   - `QWindowsWindow::handleMouseEvent` drops background wheel messages when unfocused; cleanly terminates with `0.0` motion diff.

### 4.2 Why Support Remains Categorized as "Partial"
In `crates/auv-cli-invoke/src/support.rs`:
```rust
windows: Partial("background delivery posts window messages, so whether an application acts on them depends on its toolkit")
```
This partial classification is permanent and architecturally honest: Windows Win32 does not provide a universal kernel-level targeted wheel injection API like macOS `CGEventPostToPid`. `PostMessageW` is the standard Win32 background IPC transport, but whether a message is accepted depends on the recipient window's event loop architecture.

### 4.3 Red-Line Verification
- **Remote Issue #211**: No remote edits were performed; this markdown file is prepared for owner review.
- **Zero Desktop Disturbance**: All 180+ live deliveries across Briefs 2–6 were executed strictly under `InputPolicy::BackgroundOnly` without window focus stealing or cursor displacement.
