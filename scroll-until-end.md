# Windows `scroll_until` End Detection Validation (Brief 4)

Date: 2026-10-12  
Workspace: `F:\auv`  
Branch: `feat/windows-scroll-until-parity`  
Raw evidence: [`docs/ai/references/driver/2026-10-12-windows-scroll-until-end.jsonl`](docs/ai/references/driver/2026-10-12-windows-scroll-until-end.jsonl) (6 records)

---

## 1. Executive Summary & Goal

Brief 4 validates `scroll_until` with the `--until end` condition on Windows, proving that the loop:
1. Detects when content visual motion ceases at a physical boundary.
2. Stops cleanly on motion-confirmed end of content (`EndByNoVisualProgress`), rather than spinning until budget exhaustion (`BudgetExhausted`).
3. Correctly respects the default `max_steps = 50` budget when content exceeds 50 steps without falsely declaring an early stop mid-stream.
4. Operates strictly under `InputPolicy::BackgroundOnly` across 100% of deliveries without focus stealing, window restoration, or mouse cursor disturbance.

---

## 2. Test Runs & Results Summary

| Run Type | Surface / Target | Run ID | Steps | Delivered $\Delta$ | Stop Reason | Last Motion Diff | Final Position Confirmation |
| :--- | :--- | :--- | :---: | :---: | :--- | :---: | :--- |
| **Positive End (Tail)** | QQ Music (Lyrics Bottom) | `01a12fde-4f20-773b-9918-cb1906305387` | 48 | $dy = +5760.0$ | `end_by_no_visual_progress` | `0.008357` ($< 0.01$) | **Confirmed**: Concluding lines of 《秋至的春信》 (`送你一个春天吧 / 送你告别的话 / 春枝在秋长大 / 春枝在冬开花`) followed by blank background space. Clamped at tail. |
| **Positive End (Head)** | QQ Music (Lyrics Top) | `01a12feb-37f3-771a-8fd2-436adcb2d1f3` | 34 | $dy = -4080.0$ | `end_by_no_visual_progress` | `0.000399` ($< 0.01$) | **Confirmed**: Song credits (`编曲：昆仑 / 混音：昆仑 / 封面设计：永遇乐`) and opening verse. Viewport clamped at top $0.0$. |
| **Diagnostic (Budget)** | QQ Music (Mid-Stream Up) | `01a12fea-51a1-75f1-a925-4eb1655c0c16` | 50 | $dy = -6000.0$ | `budget_exhausted` | `0.009393` (active) | **Confirmed**: 50-step budget enforced mid-scroll without hanging; active motion shift $20$ ongoing. |
| **Diagnostic (Budget)** | QQ Music (Mid-Stream Up) | `01a12fea-c488-7277-9a99-44c42bfbb1f7` | 50 | $dy = -6000.0$ | `budget_exhausted` | `0.009521` (active) | **Confirmed**: Second run identically halts at step 50 budget bound. |
| **Diagnostic (Region)** | QQ Music (Broad Region) | `01a12fe9-d357-755b-90d1-bf033155081f` | 2 | $dy = -240.0$ | `end_by_no_visual_progress` | `0.009804` ($< 0.01$) | **Analysis**: Broad region `0.4,0.2,0.5,0.6` diluted 1-line diff to $0.98\%$, tripping early stop. Tightened text region `0.52,0.24,0.22,0.50` resolved this cleanly. |
| **Negative (Ignored)** | Steam Friends List (CEF) | `01a12fe4-697d-763a-b2cf-cce616a1d24d` | 2 | $dy = +240.0$ | `end_by_no_visual_progress` | `0.000000` ($= 0.0$) | **Analysis**: Chromium/CEF drops background `WM_MOUSEWHEEL` via `PostMessageW` without focus/hover. SHA256 of capture unchanged (`1d31d014...`). |

---

## 3. Visual Confirmation of Final End Position

### Run 1: Lyrics Tail End (`01a12fde-4f20-773b-9918-cb1906305387`)
- **Delivered Delta**: $dy = +5760.0$ over 48 steps.
- **Artifact**: `01a12fde-9a41-75ec-bfd6-2646a2643fb9.webp`.
- **Visual Evidence**:
  - Displays the final lines of the song:
    ```text
    送你一个春天吧
    送你告别的话
    春枝在秋长大
    春枝在冬开花
    ```
  - Below the final line `春枝在冬开花`, there is completely blank gradient background space.
  - Continued scroll steps deliver wheel ticks, but the lyrics view cannot scroll down further; motion diff drops to `0.008357` ($< 0.01$), triggering `EndByNoVisualProgress`.

### Run 2: Lyrics Head End (`01a12feb-37f3-771a-8fd2-436adcb2d1f3`)
- **Delivered Delta**: $dy = -4080.0$ over 34 steps.
- **Artifact**: `01a12feb-7f43-7186-95a6-70a494a39332.webp`.
- **Visual Evidence**:
  - Displays the opening metadata and intro lines:
    ```text
    秋至的春信 VIP
    昆仑 / 在与东

    编曲：昆仑
    混音：昆仑
    封面设计：永遇乐
    让我送你一个春天吧
    开满热烈的花
    ```
  - The scroll position is clamped to the absolute top ($0.0$).
  - Over steps 33 and 34, `normalized_diff = 0.000399`, completing `end_by_no_visual_progress` with 2 consecutive confirmations.

---

## 4. Key Architectural Discoveries

### 1. Motion Detection Mechanics & Region Scope
- `auv-scan` computes motion via `compare_viewport_pixels`:
  $$\text{no\_motion} \iff \text{normalized\_diff} < \text{no\_motion\_threshold}\ (0.01)$$
  Stopping requires `no_motion_streak >= no_motion_confirmations` (default: 2 consecutive steps).
- **Invariant Reminder**: `no_motion_threshold = 0.01` is a shared parameter across platforms (macOS & Windows). It must NOT be modified in code for Windows work without dedicated multi-platform alignment.
- **Region Best Practice**: In multi-pane or sparsely animated applications, `--region` is critical. If a scrollable pane occupies a small fraction of the compared region, the normalized diff per step drops below $1\%$, causing premature termination. Specifying `--region` around the scrolling pane ensures clean detection of both active movement and true boundary stops.

### 2. Windows Background Delivery Capability Boundaries
- **Native / Win32 / DirectComposition**: QQ Music's DirectComposition/CEF container accepts background `WM_MOUSEWHEEL` via `PostMessageW` without focus.
- **Chromium / CEF Event Router**: Evaluated across three applications (`steamwebhelper.exe`, `msedge.exe`, `QQ.exe`). In all three, `PostMessageW(WM_MOUSEWHEEL)` is received at the Win32 window layer, but Chromium's internal event router drops the wheel event if the window does not have physical keyboard focus or mouse hover. As a result, pixel movement is exactly $0.000000$, and the loop stops immediately after 2 steps. This is an important Windows driver capability boundary to document.

---

## 5. Handoff Note for Brief 5

- **Brief 5 Goal**: Validate `scroll_until` with `ScrollUntilStep::Motion` (interpolated multi-frame motion delivery with easing and step duration) on Windows.
- **Key Baseline for Brief 5**:
  - All runs in Briefs 3 & 4 used `ScrollUntilStep::Instant` (`--step-duration-ms 0`).
  - Brief 5 will exercise `--step-duration-ms` and `--easing` (e.g. `linear`, `ease-in-out`), validating that smooth continuous wheel delivery works through the Windows driver and that frame capture / settle timing coordinates cleanly with the motion curve.
  - Settle duration should continue to use the Brief 2 recommended default ($50\text{ ms}$).
  - Use QQ Music as anchor surface with `--region 0.52,0.24,0.22,0.50` (or `0.4,0.2,0.5,0.6`).
