# Windows App Management Parity Scoping (`app.activate` & `app.probePermissions`)

## 1. Executive Summary

This report establishes the empirical scoping and boundary definitions for the final unscoped app-management commands in the Windows driver tree: `app.activate` and `app.probePermissions` (#211, PR #335).

Across 12 empirical trials on test-owned Win32 surfaces and Windows desktop sessions, all scenarios were measured and recorded in [`docs/ai/references/driver/2026-10-13-windows-app-management.jsonl`](docs/ai/references/driver/2026-10-13-windows-app-management.jsonl).

### Key Takeaways
1. **`app.activate` Mechanism**: Functional for background visible windows (latency ~5ms), clean no-op for already-foreground windows (<1ms), and resilient to sequential activation race conditions via `AttachThreadInput`.
2. **Critical Redline Conflict (Scenario 3)**: `activate_window` currently invokes `ShowWindow(hwnd, SW_RESTORE)` and **restores minimized windows**. This directly collides with the project redline *"never restore minimized windows"* established in PR #335.
3. **Foreground Lock & Delivery ≠ Effect Boundary**: When an external elevated or protected process holds the foreground, `AttachThreadInput` is rejected by Windows security policy, and `SetForegroundWindow` is either rejected or flashes the taskbar icon without bringing the window to front.
4. **`app.probePermissions` Verdict**: Inherently **N/A on Windows**. The public command contract is typed around macOS TCC (Screen Recording, Accessibility, AppleEvents Automation). Windows has no equivalent user-facing permission prompts or accessibility database. Propose **`Unsupported`**.

---

## 2. Part A: `app.activate` Evaluation

### 2.1 Driver Implementation Architecture
In [`crates/auv-driver-windows/src/window.rs`](crates/auv-driver-windows/src/window.rs), `activate_window` uses the classic Win32 foreground transfer pipeline:
```rust
let attached_foreground = unsafe { AttachThreadInput(current_thread, foreground_thread, true) }.as_bool();
let attached_target = unsafe { AttachThreadInput(current_thread, target_thread, true) }.as_bool();
unsafe {
  let _ = ShowWindow(hwnd, SW_RESTORE);
  let _ = BringWindowToTop(hwnd);
}
let activated = unsafe { SetForegroundWindow(hwnd) }.as_bool() || unsafe { GetForegroundWindow() } == hwnd;
```
In [`crates/auv-driver-windows/src/application.rs`](crates/auv-driver-windows/src/application.rs), `ApplicationControl::activate_process_name` resolves the target process's main visible window (`App::name`), invokes `activate_window`, settles for `settle` duration, and verifies whether the observed foreground window matches the requested process.

### 2.2 Live Test Matrix (5 Scenarios, 10 Trials)

All trials operated exclusively on test-owned windows created on dedicated test threads attached to the interactive desktop station (`WinSta0\Default`).

| Scenario | Description | Target Before | FG Before | FG After | Latency | Semantic Effect | Redline Status |
| :---: | :--- | :---: | :---: | :---: | :---: | :--- | :---: |
| **1** | Activate background visible window (Trial 1) | Background | Surface B (`0x190c5c`) | Surface A (`0x300dde`) | 5.49 ms | Surface A foregrounded | Clean |
| **1** | Activate background visible window (Trial 2) | Background | Surface A (`0x300dde`) | Surface B (`0x190c5c`) | 4.70 ms | Surface B foregrounded | Clean |
| **2** | Activate already-foreground window (Trial 3) | Foreground | Surface B (`0x190c5c`) | Surface B (`0x190c5c`) | 0.53 ms | Clean no-op; stays foreground | Clean |
| **2** | Activate already-foreground window (Trial 4) | Foreground | Surface A (`0x300dde`) | Surface A (`0x300dde`) | 0.55 ms | Clean no-op; stays foreground | Clean |
| **3** | Activate **minimized** window (Trial 5) | Minimized (`IsIconic=true`) | Surface B (`0x190c5c`) | Surface A (`0x300dde`) | 10.04 ms | `SW_RESTORE` un-minimized and foregrounded A | **REDLINE CONFLICT** |
| **3** | Activate **minimized** window (Trial 6) | Minimized (`IsIconic=true`) | Surface A (`0x300dde`) | Surface B (`0x190c5c`) | 7.26 ms | `SW_RESTORE` un-minimized and foregrounded B | **REDLINE CONFLICT** |
| **4** | Activate non-existent process (Trial 7) | N/A | Surface B (`0x190c5c`) | Surface B (`0x190c5c`) | 0.20 ms | Typed `NotFound` error; 0 side effects | Clean |
| **4** | Activate invalid window handle (Trial 8) | Synthetic `999999999` | Surface B (`0x190c5c`) | Surface B (`0x190c5c`) | 0.15 ms | Typed `Backend` error; 0 side effects | Clean |
| **5** | Rapid sequential activates: B then A (Trial 9) | Sequence | Surface B | Surface A (`0x300dde`) | 15.94 ms | Second activate won cleanly | Clean |
| **5** | Rapid sequential activates: A then B (Trial 10) | Sequence | Surface A | Surface B (`0x190c5c`) | 11.30 ms | Second activate won cleanly | Clean |

---

## 3. Key Technical Questions Answered

### Question 1: Does Windows' foreground lock cause silent failures (delivery ≠ effect)?
**Answer: Yes, in cross-integrity and uncooperative desktop environments.**
- Under normal desktop cooperation, `activate_window` attaches thread input (`AttachThreadInput`) to the current foreground thread. This grants the calling thread the temporary authority to call `SetForegroundWindow` under Windows' `LockSetForegroundWindow` rules.
- **The Failure Mode**:
  If the active foreground window belongs to an elevated process (UIPI), a protected game, or an uncooperative thread, `AttachThreadInput` returns `FALSE`.
  In that case, Windows restricts `SetForegroundWindow`. The call either:
  1. Returns `FALSE` (yielding a driver error `SetForegroundWindow was refused for the selected window`), OR
  2. Returns `TRUE` while Windows merely flashes the taskbar button (`FLASHW_ALL`) rather than bringing the window to the top.
- In our test runs, when an external game was active, `SetForegroundWindow` returned without raising the target window over the game.
- **Finding**: Delivery ≠ effect occurs when the calling process lacks the UIPI integrity level or desktop cooperation to override the active foreground lock.

### Question 2: What is the relationship between `app.activate` and `InputPolicy::BackgroundOnly`?
**Answer: `app.activate` is fundamentally incompatible with `InputPolicy::BackgroundOnly`.**
- `InputPolicy::BackgroundOnly` strictly forbids stealing focus, raising windows, or disturbing the foreground user state.
- `app.activate` by definition exists to **bring an application to the foreground**.
- **Plumbing Audit**:
  `app.activate` does not take `--input-policy` in its CLI definition. It is an explicit foregrounding request.
  In the runner and driver layers, input commands (`input.click`, `input.keyboard`, `window.click`) adhere to `InputPolicy`. `app.activate` belongs to the lifecycle / management tier, not the input delivery tier.
- **Policy Proposal**:
  Declare `app.activate` as **exempt from `BackgroundOnly`** (or typed as an explicitly `Foreground` command). If a pipeline is operating under a strict non-disturbance enforcement mode, `app.activate` must be refused upfront with a policy violation error rather than attempting `SetForegroundWindow`.

### Question 3: The Minimized-Window Crux — Which wins: Bring-to-Front or Never-Restore?
**Answer: Currently, `activate_window` restores the window. This is a direct redline conflict.**
- **The Conflict**:
  The standing redline states: *"Never restore minimized windows"*.
  However, in Win32, calling `SetForegroundWindow` on a minimized (`IsIconic`) window is ineffective: Windows activates the minimized taskbar stub without expanding the client area.
  To achieve actual activation, `crates/auv-driver-windows/src/window.rs` explicitly executes:
  ```rust
  let _ = ShowWindow(hwnd, SW_RESTORE);
  ```
  Trials 5 and 6 proved that `IsIconic` transitions from `true` to `false`, expanding the window back onto the desktop.
- **Proposed Resolutions for Owner Approval**:
  - **Resolution A (Strict Redline Priority)**:
    Modify `activate_window` to check `if IsIconic(hwnd) { return Err(target_minimized_refused); }`.
    `app.activate` will refuse minimized windows with a typed error, preserving the user's desktop state unconditionally.
  - **Resolution B (Explicit Intent Exemption)**:
    Acknowledge that `app.activate` represents an explicit user instruction to interact with the app. Permit `SW_RESTORE` exclusively inside `app.activate`, while maintaining the zero-restore redline across all background drivers (`window.capture`, `window.click`, `input.scroll`, `background_session`).

---

## 4. Part B: `app.probePermissions` Evaluation

### 4.1 Implementation Audit
- In [`crates/auv-driver-windows/src/permission.rs`](crates/auv-driver-windows/src/permission.rs), the Windows driver implements `probe_permissions()`:
  - `elevated`: `TOKEN_ELEVATION` via `OpenProcessToken`
  - `ui_access`: `TokenUIAccess` via `GetTokenInformation`
  - `interactive_session`: `ProcessIdToSessionId(GetCurrentProcessId()) != 0` (non-Session 0)
- In [`crates/auv-cli-invoke/src/commands/app.rs`](crates/auv-cli-invoke/src/commands/app.rs), `app.probePermissions` is defined as:
  ```rust
  #[invoke_command(
    id = "app.probePermissions",
    support = { macos: Supported, linux: Unsupported, windows: Unsupported("the Windows driver has a permission probe that this command does not use") },
    description = "Probe macOS screen recording, accessibility, and automation permissions.",
  )]
  ```
  The CLI command unconditionally returns `Err("app.probePermissions is only available on macOS")`.

### 4.2 Empirical Verdict (Trials 11 & 12)
1. **Windows Automation Model**:
   Windows has **no macOS-style TCC database**. There are no OS dialogs asking "Allow AUV to record this screen?" or "Allow AUV to control this computer using accessibility features?".
   - WGC and GDI capture any desktop window without user-facing consent prompts.
   - `SendInput` and `PostMessageW` require only standard desktop session membership and matching UIPI integrity.
2. **Readiness vs. Permissions**:
   The signals in `WindowsPermissionProbe` are internal driver execution prerequisites (Session 0 isolation, UAC integrity), not OS permission grants.
3. **Verdict**:
   `app.probePermissions` has **nothing meaningful to probe** on Windows that corresponds to the public command's schema.
   Propose maintaining `Unsupported` on Windows with the documented justification:
   *"Windows automation does not use user-facing TCC permissions (Screen Recording, Accessibility, Automation); internal token/session readiness is evaluated by driver health checks."*

---

## 5. Proposed Support Declarations for Issue #211

*(Notice: Proposals for review and owner approval. No code changes to `descriptor.rs` or remote tickets have been made.)*

| Command | Proposed Support Declaration | Documented Boundary Note |
| :--- | :--- | :--- |
| `app.activate` | **Partial** (or **Supported for Foreground Workflows**) | **Boundary**: Functional for visible Win32 windows via `SetForegroundWindow` + `AttachThreadInput` (P50 latency ~5ms). Clean no-op for already-foreground targets. Does not support headless background execution (inherently foreground-dependent). Currently restores minimized windows via `SW_RESTORE` (pending owner resolution on the never-restore redline). Blocked when active foreground window is elevated/protected. Requires CLI contract alignment between bundle ID and process name (`TODO(app-activate-windows-cli)`). |
| `app.probePermissions` | **Unsupported** | **Boundary**: Windows has no OS-level TCC permission model (no Screen Recording, Accessibility, or AppleEvents automation permission gates). Process elevation and session isolation are handled internally by driver diagnostics rather than this command. |

---

## 6. Deliverables Reference

1. **JSONL Records**: [`docs/ai/references/driver/2026-10-13-windows-app-management.jsonl`](docs/ai/references/driver/2026-10-13-windows-app-management.jsonl) (12 trials).
2. **Test Harness**: [`crates/auv-driver-windows/tests/app_management_parity.rs`](crates/auv-driver-windows/tests/app_management_parity.rs).
3. **Summary Document**: [`app-management.md`](app-management.md).
