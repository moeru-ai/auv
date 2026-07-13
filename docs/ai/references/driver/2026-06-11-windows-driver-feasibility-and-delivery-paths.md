# Windows Driver Feasibility And Delivery Paths

Date: 2026-06-11

Status: proposed design, pending review

## Goal

Assess whether `auv-driver-windows` can support the public `auv-driver` API
surface, and define the validation shape for Windows NetEase Music as the
primary real-application smoke.

The intended v0 is not a background-only automation stack. The goal is public
API coverage with honest delivery evidence: every input action should record
which delivery path succeeded, which attempts failed, why fallback happened,
and what user-visible disturbance the path caused.

Windows NetEase Music is the primary validation target because NetEase is a
cross-platform application. The Windows validation should reuse shared
`auv-driver` contracts and NetEase app-domain logic instead of extending the
existing macOS-only proof paths.

## Non-Goals

- Do not require background-only operation for v0.
- Do not add a UIAccess worker in the first slice.
- Do not put NetEase-specific workflow logic inside `auv-driver-windows`.
- Do not copy MaaFramework's task/controller architecture wholesale.
- Do not treat the archived macOS AX copilot vertical as the Windows product
  lane.

## Public API Coverage Matrix

| AUV API | Windows feasibility | Recommended Windows path | Contract / caveat |
| --- | --- | --- | --- |
| `Driver::descriptor`, `open_local` | Easy | Local `WindowsDriverSession` | Use `PlatformKind::Windows`. |
| `display.list()` | Easy | `EnumDisplayMonitors`, DPI APIs | Normalize logical vs physical coordinates. |
| `display.capture()` | Medium | GDI/BitBlt or DXGI desktop duplication | Fill `Capture.backend` and `scale_factor`. |
| `display.capture_region()` | Medium | Desktop BitBlt / DXGI crop | Must handle virtual desktop and per-monitor DPI. |
| `window.list()` | Medium | `EnumWindows` plus UIA top-level union | Store `HWND` in `WindowRef.id`; validate stale refs before use. |
| `window.resolve()` | Medium | pid/title/class/exe/package metadata | `AppSelector::bundle` is macOS-shaped; Windows should map it to executable or package identity where possible. |
| `window.capture()` | Medium-hard | WGC, then PrintWindow, then visible-region fallback | UWP/WinUI/DirectComposition need WGC; minimized and occluded windows need explicit fallback reporting. |
| `window.to_screen_point()`, `to_window_point()` | Medium | `GetWindowRect`, DWM extended frame bounds, client-origin helpers | The coordinate basis must match the returned screenshot. |
| `vision.recognize_text_in_capture()` | Feasible | Existing OCR over `Capture` | No Windows-native OCR is required for v0. |
| `window.click()` | Hard | UIA/MSAA semantic action, PostMessage deepest child, foreground SendInput fallback | Record all attempts in `InputActionResult`. |
| `window.type_text()` | Hard | UIA ValuePattern, `WM_CHAR` to focused child, clipboard paste, foreground SendInput fallback | WPF/UWP/XAML/Chromium differ significantly. |
| `window.scroll()` | Hard | UIA ScrollPattern, `WM_MOUSEWHEEL`, foreground wheel fallback | Scroll is likely to return false success without verification. |
| `input.click_at()` | Easy | Foreground `SendInput` | Current API returns `()`, so evidence is weaker than `window.click()`. |
| `input.type_text()`, `press_key()` | Medium | Foreground `SendInput` | Should map to `ForegroundSystemEvents` when promoted to typed evidence. |
| `clipboard.snapshot()`, `restore()`, `set_text()` | Medium | Win32 clipboard APIs | Snapshot encoding can be platform-private. |
| `window.move_to()`, `resize()`, `set_frame()` | Medium | `SetWindowPos` | Natural `WindowMutationPath::PlatformNative` mapping. |
| `window.minimize()`, `restore()`, `zoom()` | Medium | `ShowWindow`, `SetWindowPlacement` | Verify before/after state or frame. |
| `permission.probe()` | Medium | Integrity level, interactive desktop, UIA/WGC availability | Current `PermissionProbe` names are macOS-shaped; Windows should document how each field maps or extend the contract later. |

## Windows Delivery Paths

Windows should be modeled as a set of delivery paths, not one generic click or
type API.

| Windows path | AUV mapping | Good fit | Known risk |
| --- | --- | --- | --- |
| UIA `Invoke`, `Toggle`, `SelectionItem`, `ExpandCollapse` | `AxPress` for v0; later rename or add a neutral semantic path | Buttons, menu items, tabs, list items | Custom canvases and games often have no useful semantic node. |
| UIA `ValuePattern.SetValue` / `RangeValuePattern.SetValue` | `AxSetValue` | Text fields, sliders, range controls | UIA providers can be slow or incomplete. |
| UIA `ScrollPattern` | `AxScroll` | Accessible lists and panes | Coverage varies by framework. |
| MSAA `IAccessible` | `AxPress` or future `AccessibilityAction` | VCL/SAL and older desktop UI stacks | Older API with weaker structured semantics. |
| `PostMessage(WM_MOUSEMOVE/WM_*BUTTON*)` | `WindowTargetedMouse` | Traditional Win32 child HWND controls | Chromium, WPF, GTK, and others may silently drop messages. |
| `PostMessage(WM_CHAR/WM_KEYDOWN/UP)` | `WindowTargetedKeyboard` | Win32 edit controls, RichEdit, Scintilla-like targets | Accelerators, modifier combos, and UIPI are fragile. |
| Foreground `SendInput` | `ForegroundSystemEvents` | General fallback when target accepts only system input | Foreground and cursor disturbance must be recorded. |
| Synthetic pointer/touch injection | `WindowTargetedMouse` or a future explicit path | Potential background path for WPF/Chromium/GTK | Complex and should be deferred until there is evidence. |
| Clipboard paste | `ClipboardPaste` | Text input fallback | Clipboard disturbance must be recorded. |
| Windows Graphics Capture | `Capture.backend = "windows_graphics_capture"` | UWP, WinUI, DirectComposition, occluded rendered surfaces | Minimized windows still have no rendered content. |
| PrintWindow | `Capture.backend = "print_window"` | Traditional GDI/Win32 windows | Can return black or stale content. |
| GDI/ScreenDC/DXGI crop | Backend-specific capture labels | Visible foreground/desktop fallback | Occlusion can capture the covering window instead of the target. |

## Reference Findings

### Cua

The `trycua/cua` repository provides the strongest reference for Windows
delivery-path routing. If the implementation environment does not already have
this repository available locally, clone a current copy before using it as an
implementation reference.

Useful findings:

- The Windows backend uses UIA/MSAA, `PostMessage`, foreground `SendInput`,
  synthetic pointer/touch injection, and multiple capture paths.
- Cua's dispatch model distinguishes `background`, `foreground`, and `auto`.
- Framework heuristics are explicit: Chromium/Electron, WPF, GTK, VCL/SAL,
  UWP/XAML/WinUI each change which delivery paths are likely to work.
- It detects UIPI/integrity failures before reporting success for
  `PostMessage` paths.
- It uses WGC for DirectComposition/UWP/WinUI capture where PrintWindow can
  return black.
- It has a separate UIAccess-oriented worker, but this should not be part of
  AUV's first Windows slice.

### MaaFramework

The `MaaXYZ/MaaFramework` repository is most useful as a capture/input backend
catalog. If the implementation environment does not already have this
repository available locally, clone a current copy before using it as an
implementation reference.

Useful findings:

- Maa's Win32 control unit separates public controller shape, capture backend,
  input backend, capability flags, and cleanup.
- Capture methods include GDI, WGC FramePool, DXGI desktop duplication,
  window-cropped DXGI, PrintWindow, and ScreenDC.
- Maa probes allowed capture methods and chooses a working/fast backend.
- Input methods include `SendMessage`, `PostMessage`, `SendInput`-style
  foreground paths, legacy mouse/key events, and cursor/window-position
  variants.
- Maa does not appear to carry Cua-style framework heuristics; it is more
  method-configurable than framework-aware.

The AUV takeaway is to combine both lessons: use Cua-style routing heuristics
for action selection, and Maa-style backend catalog/probing for capture and
input capability reporting.

## Application Adoption Boundary

`auv-driver-windows` should detect platform and target-stack facts:

- `HWND`, pid, title, class name, executable path, package identity, and root
  owner window.
- DPI context, window frame, DWM extended frame bounds, client rect, and
  screenshot coordinate origin.
- Foreground state, minimized/cloaked/visible state, and stale handle status.
- UIA/MSAA availability and useful pattern coverage.
- Process integrity level, UIPI risk, UIAccess availability, and interactive
  desktop/session status.
- Capture support and backend capability, including WGC availability and
  PrintWindow black/stale fallback.

`auv-driver-windows` should not know NetEase-specific workflow decisions such
as search query, playlist region, result verification, or player-region OCR.
Those belong in `auv-netease-music`, recipes, or higher app-domain layers.

Framework heuristics belong in a Windows action resolver or delivery-policy
module, not scattered through every low-level backend. Examples include:

- Chromium/Electron: avoid trusting posted mouse and modifier-combo keys.
- WPF: avoid trusting posted pointer events; text may require focused or
  foreground delivery.
- GTK: posted button clicks are often unreliable.
- VCL/SAL: prefer MSAA and avoid UIA tree hangs; accelerator keys may require
  foreground/system input.
- UWP/XAML/WinUI: use UIA patterns and WGC before Win32 message/capture paths.

## NetEase Cross-Platform Validation

Windows NetEase Music should be the primary real-app E2E validation target,
but the validation must not deepen the old macOS-only path.

Current state:

- `auv-netease-music` already has platform-neutral result records, parser
  diagnostics, view-parser records, delivery-path fields, and OCR-driven
  product logic.
- Live execution currently constructs `MacosDriver` directly and returns
  `only supported on macOS` for non-macOS targets.
- Some helpers, such as AX tree corroboration, are genuinely macOS-only and
  should remain behind narrow `cfg(target_os = "macos")` gates.

Required validation direction:

- Introduce a small NetEase live-driver boundary before adding Windows logic.
  The workflow should depend on shared capabilities: resolve window, capture
  window, OCR/find text, click window point/text, type or paste text, scroll,
  and report delivery paths.
- Keep shared NetEase workflow logic compiled on Windows when it only depends
  on `auv-driver` types.
- Keep platform session construction behind `cfg`: macOS opens
  `MacosDriver`; Windows should later open `WindowsDriver`.
- Preserve `delivery_path`, `fallback_reason`, capture backend labels, and
  verification artifacts in NetEase outputs.

Primary E2E flow:

1. Resolve the Windows NetEase Music window by Windows metadata, not by macOS
   bundle id. Accept title, pid, executable, package id, or class-derived
   evidence.
2. Capture the window and record the selected backend.
3. Search for a stable query such as `AURORA Cure For Me`.
4. Enter text through the best available path, allowing foreground fallback.
5. OCR the result list and locate the expected title and artist.
6. Activate the visible result through `window.click`, recording
   `InputActionResult`.
7. Verify the bottom player region by OCR over a captured image.

Foreground fallback is acceptable for this E2E, but it must be reported as
foreground fallback. A foreground action must not be recorded as a background
success.

Failures should identify the failed layer:

- window resolution
- capture backend
- OCR / recognition
- text delivery
- click delivery
- playback verification

## Supporting Diagnostic Validation

Before relying on NetEase, the Windows driver should have smaller diagnostic
checks so failures can be isolated:

- A simple Win32 text target, such as Notepad, for keyboard/text delivery.
- Calculator or Settings for UWP/XAML UIA invoke and WGC capture.
- A Chromium/Electron target for known `PostMessage` drop behavior.
- A simple visible window for WGC, PrintWindow, and BitBlt capture comparison.

These diagnostics are not substitutes for NetEase. They prevent the NetEase
E2E from becoming the only place where driver regressions can be understood.

## Recommended V0 Slice

1. Add a `WindowsDriver` crate and session facade that mirrors the macOS
   grouped API shape while depending only on `auv-driver`.
2. Implement window/display enumeration and resolution with stable Windows
   target metadata.
3. Implement capture with WGC, PrintWindow, and visible-region fallback. Always
   fill backend and fallback reason.
4. Implement foreground `SendInput` for global input helpers.
5. Implement `window.click` and `window.type_text` with UIA/PostMessage first
   where safe, and foreground fallback where requested by policy.
6. Implement window mutation through `SetWindowPos`, `ShowWindow`, and
   `SetWindowPlacement`, returning `WindowMutationResult`.
7. Implement readiness/permission probes for integrity, UIA availability,
   interactive desktop, WGC availability, and target-window stability.
8. Refactor `auv-netease-music` live workflows to consume a shared
   driver-session boundary so Windows can reuse the app workflow.

## Open Contract Notes

- `PermissionProbe` currently contains macOS-shaped field names. Windows v0
  can map these conservatively, but a later contract slice should introduce
  platform-neutral capability probe records.
- `InputDeliveryPath::Ax*` names are macOS-shaped but currently usable for
  semantic accessibility actions. A later contract slice should rename or add
  neutral accessibility delivery paths.
- Some facade helpers return `()` instead of `InputActionResult`; this weakens
  evidence. Do not expand that issue inside the first Windows slice unless a
  touched call site needs it.
- UIAccess worker support is a later capability slice because it changes
  installation, signing, process launch, and security assumptions.

## Validation And Acceptance

Docs-only validation for this spec:

- `git diff --check`

Future implementation validation:

- Unit tests for Windows target metadata normalization and stale `HWND`
  rejection.
- Unit tests for delivery-path selection from target facts.
- Result-shape tests for `InputActionResult`, `WindowMutationResult`, and
  `Capture`.
- Compilation checks proving shared NetEase workflow logic no longer depends
  directly on `auv-driver-macos`.
- Manual/live smoke tests for Notepad, Calculator/Settings, Chromium/Electron,
  and Windows NetEase Music.
