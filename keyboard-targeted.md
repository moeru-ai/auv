# Windows Targeted Keyboard Input Evaluation

> Status: Empirical Evaluation and Proposed Support Declarations  
> Date: 2026-10-12  
> Branch: `feat/windows-keyboard-targeted`  
> Baseline Reference: PR #335, Issue #211  
> Evidence Log: `docs/ai/references/driver/2026-10-12-windows-keyboard-targeted.jsonl` (38 trials)

---

## 1. Executive Summary

This evaluation scopes Windows support for targeted keyboard input (`input.typeText`, `input.key`, `input.keys`, `input.holdKeys`, and batch `input.keyboard` with `--target` and `--input-policy background-only`).

PR #335 declared background keyboard as `unsupported` with the design principle *"don't fake it"*. This evaluation provides the empirical evidence to assess that verdict:

1. **Delivery ≠ Semantic Effect (The #335 Principle)**:
   Calling `PostMessageW(hwnd, WM_KEYDOWN / WM_KEYUP / WM_CHAR, ...)` returns `TRUE` across all tested applications (Win32 Edit, Win32 Top-Level, and Chromium/Edge). Delivery succeeds unconditionally into the target thread's message queue. However, semantic effect diverges completely depending on control architecture and target specification.
2. **The In-App Focus Crux**:
   - A classic Win32 `Edit` control **does not require in-app keyboard focus** (`GetFocus() == hwnd`) to process direct `WM_CHAR` messages. Literal text can be appended to a background `Edit` control without activating or foregrounding the window.
   - However, this **strictly requires targeting the child `Edit` control HWND directly**. Posting to the top-level window (`hwnd_frame`) results in **zero semantic effect**, as standard top-level `DefWindowProcW` drops unrouted character messages.
   - Modern application frameworks (Chromium, Electron, WinUI 3, WPF, DirectComposition) do not expose child `Edit` HWNDs. Chromium (`Chrome_WidgetWin_1`) routes through `HWNDMessageHandler` and `RenderWidgetHost`, which **silently discard all background keyboard messages** whenever `::GetFocus() != hwnd`.
3. **The Thread Modifier State Disconnect**:
   Chords (`input.keys`, e.g., `Ctrl+A`, `Ctrl+C`) and held keys (`input.holdKeys`) **fail semantically in the background**. Posting `WM_KEYDOWN(VK_CONTROL)` places a message in the queue but **does not update the thread's asynchronous key state table** (`GetKeyState(VK_CONTROL)` remains 0). When `WM_KEYDOWN(VK_A)` arrives, the control evaluates `GetKeyState`, observes that `Ctrl` is not active, and degenerates into typing the raw letter `'a'`.
4. **Proposed Declarations**:
   - `input.typeText`: Propose **Partial: 机制已证，需 child HWND targeting（未实现）** (tested mechanism works when dispatching directly to a child Win32 `Edit` HWND, but public `auv invoke --target` currently resolves only to top-level windows via `App::name`; without driver-level child control resolution, public API invocations cannot route to child controls and are dropped by `DefWindowProcW`).
   - `input.key`: Propose `unsupported` (narrow subset works on child Win32 `Edit`, but navigation/chords fail).
   - `input.keys`: Propose `unsupported` (architectural limitation of Win32 message queue).
   - `input.holdKeys`: Propose `unsupported` (cannot sustain logical key state in background without `SendInput`).

---

## 2. Test Matrix and Empirical Results

The evaluation tested 4 commands across 3 application architectures (plus top-level targeting diagnostics) under `InputPolicy::BackgroundOnly` (zero foreground activation, zero cursor moves, zero desktop disturbance).

Total: **38 trials** recorded in `docs/ai/references/driver/2026-10-12-windows-keyboard-targeted.jsonl`.

### Matrix Summary

| Command | Win32 Child Edit (`win32-receiver` / Notepad Edit) | Win32 Top-Level HWND | Chromium / Electron (`msedge.exe` / Edge Beta) | Verdict |
| :--- | :--- | :--- | :--- | :--- |
| **`input.typeText`** | **Works** (100% semantic text entry via `WM_CHAR`) | **Fails** (0% text entry; dropped by `DefWindowProc`) | **Fails** (0% text entry; dropped by `RenderWidgetHost`) | **Partial: 机制已证，需 child HWND targeting（未实现）** |
| **`input.key`** | **Partial** (`backspace` works; `return`/`tab` ignored by single-line) | **Fails** (dropped by top-level procedure) | **Fails** (dropped by `HWNDMessageHandler`) | **Unsupported** |
| **`input.keys`** (chords) | **Fails** (degenerates to typing `'a'`, `GetKeyState==0`) | **Fails** (dropped by top-level procedure) | **Fails** (dropped by Chromium Aura focus gate) | **Unsupported** |
| **`input.holdKeys`** | **Fails** (does not hold `GetKeyState`; repeat requires focus) | **Fails** (dropped by top-level procedure) | **Fails** (dropped by Chromium Aura focus gate) | **Unsupported** |

---

## 3. Detailed Findings by Architecture

### 3.1 App 1: Notepad Anchor (Classic Win32 Edit Control)
- **Target Surface**: Authentic Win32 `Edit` control (`WS_CHILD | WS_VISIBLE | WS_BORDER | ES_AUTOHSCROLL`).
- **Delivery**: `PostMessageW` returned `TRUE` across all trials.
- **Semantic Effect**:
  - `typeText`: **Works**. Literal ASCII and Unicode strings (`"abc"`, `"hello world"`, `"123-xyz!"`) appeared verbatim in the edit buffer. `WM_GETTEXT` confirmed 100% accuracy without window activation.
  - `key` (single keys):
    - `backspace`: Succeeded when sent as `WM_CHAR(0x08)` with caret at end of text. Note: posting both `WM_KEYDOWN(VK_BACK)` and `WM_CHAR(0x08)` triggers two deletions.
    - `return`: Ignored by single-line `ES_AUTOHSCROLL` control (standard Win32 behavior).
    - `tab`: Intercepted by dialog navigation logic; did not insert `\t`.
  - `keys` (chords): **Failed**.
    - `ctrl+a`: Instead of selecting all text (`EM_GETSEL == (0, len)`), the edit control appended the character `'a'`. The text `"select_me"` became `"select_mea"`.
    - `ctrl+c`: `WM_COPY` was never triggered.
    - `shift+a`: Degenerated into lowercase `'a'`.
  - `holdKeys`: **Failed**.
    - Holding `Shift` or `Ctrl` for 200–250ms left `GetKeyState` at 0. Auto-repeat `WM_KEYDOWN` without focus did not generate auto-repeat characters.

### 3.2 App 2: Win32 Receiver (Multi-Control Win32 Environment)
- **Target Surface**: `target/release/win32-receiver.exe` running with `--sink --foreground-input` at the bottom of the Z-order (`HWND_BOTTOM`).
- **Delivery**: `PostMessageW` returned `TRUE` across all trials.
- **Semantic Effect**:
  - `typeText`: **Works**. Multi-line and single-line buffers accepted text cleanly, including Chinese Unicode (`"unicode_测试"`).
  - `key`: Consistent with App 1 (`backspace` deleted characters; `return`/`tab` ignored by single-line text box).
  - `keys` (chords): Consistent with App 1 (chords degenerated into raw character typing due to `GetKeyState == 0`).
  - `holdKeys`: Consistent with App 1 (zero modifier persistence).

### 3.3 App 3: Chromium / Electron (Microsoft Edge Beta)
- **Target Surface**: `Chrome_WidgetWin_1` (HWND `67174`) and its render child `Intermediate D3D Window` (HWND `1246580`).
- **Delivery**: `PostMessageW` returned `TRUE` across all trials (Windows placed messages into Edge's message queue).
- **Semantic Effect**: **Zero effect across all 12 trials**.
  - `typeText`: 0 characters appeared; zero visual changes.
  - `key`: 0 events processed; zero visual changes.
  - `keys`: 0 shortcuts recognized; zero visual changes.
  - `holdKeys`: 0 events processed; zero visual changes.
- **Root Cause**:
  Chromium's input stack (`ui/views/win/hwnd_message_handler.cc` and `content/browser/renderer_host/render_widget_host_view_aura.cc`) gates keyboard processing behind window focus (`::GetFocus() == hwnd`). When a window is not active or foregrounded, `WM_CHAR`, `WM_KEYDOWN`, and `WM_KEYUP` are dropped before reaching Blink or the omnibox.

### 3.4 Crux Diagnostic: Top-Level vs. Child Control Routing
- **Target Surface**: Top-level window HWNDs (`hwnd_notepad` and `hwnd_receiver`).
- **Result**: **0% semantic effect**.
- **Root Cause & Public API Seam**:
  Win32 does not automatically route top-level `WM_CHAR` or `WM_KEYDOWN` down to child controls. If an automation framework passes a top-level window handle instead of the child control HWND, the top-level procedure executes `DefWindowProcW`, which discards the message.
  Crucially, live trials proved that the message mechanism succeeds when posting directly to a child `Edit` HWND. However, the public CLI `auv invoke --target` (e.g. `--target app:notepad` via the recently landed `App::name` resolution) currently resolves only down to top-level window HWNDs. As long as `auv-driver` lacks child control inspection and targeting, the public API cannot reach the child `Edit` control and remains completely inoperative. Hence, `Partial` strictly means **"机制已证，需 child HWND targeting（未实现）"**.

---

## 4. Key Technical Questions Answered

### Question 1: Does the target Edit control need in-app keyboard focus for `WM_CHAR` to take effect?
**Answer: No, but it strictly requires child control targeting.**
- If `WM_CHAR` is posted directly to the child `Edit` control HWND, the edit control's internal window procedure (`EditWndProc`) appends the character immediately, regardless of whether `GetFocus() == hwnd_edit`.
- However, if `WM_CHAR` is posted to the top-level window HWND, it is dropped.
- In modern frameworks (Chromium, Electron, WinUI 3), there is no child `Edit` HWND; all input goes to the top-level window or swapchain child, which *does* require focus.

### Question 2: For chords (`keys`) and `holdKeys`, does the app see modifier state correctly via posted messages?
**Answer: No. This is an architectural boundary of the Win32 message queue.**
- `PostMessageW(hwnd, WM_KEYDOWN, VK_CONTROL, ...)` enqueues a message in the thread's message queue.
- It **does not update** the thread's virtual key state table (`GetKeyState` / `GetAsyncKeyState`).
- Applications and standard Win32 controls evaluate `GetKeyState(VK_CONTROL)` when processing child key events (like `'A'`).
- Because `GetKeyState` returns 0, the application does not recognize that `Ctrl` is down. It treats the subsequent key as a regular keystroke (typing `'a'`) rather than an accelerator (Select All).
- `SetKeyboardState` cannot fix this across processes: `SetKeyboardState` modifies only the calling thread's key state, not the target process's key state, unless attached via `AttachThreadInput` (which brings significant race conditions and input deadlocks).

### Question 3: Is there interference from the foreground window's keyboard state?
**Answer: Minimal for `WM_CHAR`, significant for `WM_KEYDOWN`.**
- `WM_CHAR` carries the exact UTF-16 code point in `wParam`, bypassing the foreground keyboard state and CapsLock.
- However, if an Input Method Editor (IME, e.g. Microsoft Pinyin) is active on the target thread, posted `WM_KEYDOWN` messages are converted to `VK_PROCESSKEY` (0xE5) and withheld in an IME composition buffer rather than delivered to the control.

---

## 5. Proposed Support Declarations

*Notice: These are proposals for review and owner approval. No code changes to `crates/auv-driver-windows` or `docs/SUPPORT_MATRIX.md` are made in this evaluation.*

| Command | Proposed Support Declaration | Documented Boundary Note |
| :--- | :--- | :--- |
| `input.typeText` | **Partial: 机制已证，需 child HWND targeting（未实现）** | **Boundary**: Underlying Win32 message mechanism is proven when posting directly to child `Edit` HWNDs. However, public `auv invoke --target` currently resolves only down to top-level window HWNDs (`App::name`). As long as `auv-driver` lacks child control resolution, public API delivery to top-level windows drops messages (`DefWindowProcW`). Unsupported on top-level targets and Chromium/Electron. |
| `input.key` | **Unsupported** | Single keys cannot reliably navigate or submit in the background. Return and Tab behave inconsistently across control styles. |
| `input.keys` | **Unsupported** | Win32 message queue cannot simulate modifier state (`GetKeyState`). Chords fail and degenerate into typing raw characters. |
| `input.holdKeys` | **Unsupported** | Cannot sustain logical modifier or key state in background without global input injection. |
| `input.keyboard` | **Partial** / **Unsupported** | Inherits action-level boundaries; batch chords or holds fail. |

### Final Recommendation for Issue #211
Retain PR #335's default `unsupported` declaration for targeted keyboard input unless an explicit child-control targeting contract (e.g., UI automation control resolution returning child HWNDs) is introduced. If `input.typeText` is marked `partial`, its accurate declaration is **"机制已证，需 child HWND targeting（未实现）"**: the underlying child-HWND `PostMessageW` mechanism is verified, but public `invoke --target` currently resolves only to top-level windows (the scope of the recently landed `App::name` fix). As long as `auv-driver` does not resolve child controls, the public API cannot deliver text in the background.
