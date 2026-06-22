# auv-driver-windows v0 Implementation Reference

Date: 2026-06-18

Status: implemented; validation example available; NetEase cross-platform
smoke deferred.

## Summary

`auv-driver-windows` implements the public `auv-driver` API surface for the
Windows desktop. It mirrors the macOS driver's module shape and session API so
shared consumers — `auv-netease-music`, MCP frontends, and any future
cross-platform runtime path — can stay backend-agnostic.

The crate exposes all capabilities through a session facade (`WindowsDriverSession`)
using the same `DisplayApi / WindowApi / VisionApi / InputApi / ClipboardApi /
PermissionApi / AccessibilityApi` naming the macOS driver uses.

## Implemented Capabilities

`WINDOWS_DESKTOP_CAPABILITIES` lists the string tokens recorded on the driver
descriptor. Every token below is wired end-to-end in the current v0:

| Capability string | Module | Notes |
| --- | --- | --- |
| `desktop.recognize-image-text` | `ocr`, `vision` | `Windows.Media.Ocr` WinRT engine over a caller-supplied RGBA buffer |
| `desktop.find-image-text` | `vision` | Searches OCR output for a text pattern |
| `desktop.list-displays` | `capture` | `xcap::Monitor::all()` |
| `desktop.capture-display` | `capture` | `xcap` monitor capture; `backend = "xcap.windows"` |
| `desktop.capture-region` | `capture` | Monitor crop; `backend = "xcap.windows"` |
| `desktop.capture-window` | `capture` | Win32 GDI `PrintWindow`; `backend = "printwindow.windows"` |
| `desktop.list-windows` | `window` | `EnumWindows`; DWM extended frame bounds with `GetWindowRect` fallback |
| `control.click-point` | `input` | Foreground `SendInput`; `InputDeliveryPath::ForegroundSystemEvents` |
| `control.scroll-point` | `input` | Foreground `SendInput` wheel event |
| `control.type-text` | `input` | Unicode key events via `SendInput` |
| `control.press-key` | `input` | Virtual-key + modifier events via `SendInput` |
| `control.copy` | `input` | `Ctrl+C` via `SendInput` |
| `control.paste` | `input` | `Ctrl+V` via `SendInput` |
| `clipboard.snapshot` | `clipboard` | Win32 clipboard text snapshot |
| `clipboard.restore` | `clipboard` | Win32 clipboard text restore |
| `clipboard.set-text` | `clipboard` | Win32 clipboard text set |
| `desktop.probe-permissions` | `permission` | UAC elevation, UIAccess privilege, interactive session |
| `desktop.capture-ax-tree` | `accessibility` | Microsoft UI Automation control-view tree walker with read-only ValuePattern text; depth ≤ 40, nodes ≤ 2 000 |
| `control.focus-ax-node` | `accessibility` | Resolves a recent snapshot path and focuses the UIA element with `SetFocus` |
| `control.select-ax-node` | `accessibility` | Selects through `SelectionItemPattern` with `InvokePattern` fallback |

Window mutation (`move_to`, `resize`, `set_frame`, `minimize`, `restore`,
`zoom`) is wired via `SetWindowPos`/`ShowWindow` and produces
`WindowMutationResult` with before/after frame and state. It is not represented
as a capability string yet, mirroring the macOS driver's current shape.

## Module Map

| Module | Public exports | Responsibility |
| --- | --- | --- |
| `driver` | `WindowsDriver`, `WindowsDriverSession` | Implements `Driver` / `DriverSession`; `id = "windows.desktop"` |
| `descriptor` | `WindowsDriverDescriptor`, `WINDOWS_DESKTOP_CAPABILITIES`, `windows_driver_descriptor` | Capability constants and descriptor factory |
| `session` | `DisplayApi`, `WindowApi`, `VisionApi`, `InputApi`, `ClipboardApi`, `PermissionApi`, `AccessibilityApi` | Session facade methods; thin wiring to capability modules |
| `ocr` | `OcrError`, `recognize_text_in_rgba` | `Windows.Media.Ocr` WinRT integration |
| `vision` | `OcrMatch`, `OcrMatches`, `recognize_text_in_capture`, `find_text_in_capture` | Wraps OCR into capture-coordinate-relative results |
| `capture` | `list_displays`, `capture_display`, `capture_region`, `capture_window` | `xcap` display/region capture; GDI `PrintWindow` for window capture |
|`window` | `list_windows`, `resolve_window` | `EnumWindows` enumeration; title/pid/class/exe selector resolution |
| `input` | `click_at`, `scroll_at`, `type_text`, `press_key`, `copy`, `paste` | Foreground `SendInput` with `InputActionResult` disturbance reporting |
| `clipboard` | `snapshot`, `restore`, `set_text` | Win32 clipboard text operations |
| `mutation` | `mutate_window` | `SetWindowPos`/`ShowWindow` with before/after frame verification |
| `permission` | `WindowsPermissionProbe`, `probe` | UAC elevation, UIAccess, interactive-session token queries |
| `accessibility` | `AxNode`, `AxTreeSnapshot`, `snapshot_window` | UIA COM tree walker producing a flat ordered node list |
| `error` | (internal) | `DriverError` constructors: `backend`, `invalid_input`, `not_found` |

## Delivery Paths

All input in v0 is `InputDeliveryPath::ForegroundSystemEvents` via `SendInput`.
No background/UIA pattern actions or `PostMessage` paths are wired yet.

Key disturbance metadata recorded per action:

| Field | Value in v0 |
| --- | --- |
| `foreground_disturbance` | `Temporary` (click/scroll) or `Unknown` (type/press) |
| `cursor_disturbance` | `Unknown` (type/press) or `None` (click has absolute coordinate) |
| `clipboard_disturbance` | `Permanent` for paste; `None` otherwise |

## Capture Backends

| Path | Backend tag | Use case |
| --- | --- | --- |
| Display / region | `"xcap.windows"` | Full-screen or region grabs; subject to occlusion |
| Window | `"printwindow.windows"` | Single-window capture without compositing the full desktop; can return black for minimized or DirectComposition windows |

TODO(windows-capture-wgc): WGC (`Windows.Graphics.Capture`) for UWP/WinUI/
DirectComposition surfaces where `PrintWindow` returns black is deferred until
an owner-approved capture slice.

## Permission Probe

`WindowsPermissionProbe` surfaces three process-level signals:

- `elevated`: process token has administrator elevation (affects UIPI).
- `ui_access`: process holds the UIAccess privilege (bypasses UIPI without elevation).
- `interactive_session`: process runs in the interactive desktop session (Session 0 services cannot reach the user desktop).

Each signal is `PermissionStatus::Granted / Missing / Unknown`.

TODO(windows-readiness-assessment): a target-aware readiness assessment
combining this probe with window foreground/frame-drift checks (mirroring
macOS `assess_readiness`) is deferred until an owner-approved slice.

## Accessibility Tree

`snapshot_window` walks the UIA control-view tree for a single window via COM
`IUIAutomation`. Output is an `AxTreeSnapshot`: a flat, depth-first ordered
list of `AxNode` items carrying:

- `depth`, `path` (slash-joined child index chain from root)
- `control_type`, `name`, optional ValuePattern `value`, `automation_id`,
  `class_name`, `focused`
- `bounds` (screen-space `Rect` from UIA bounding-rectangle edges)

Traversal limits: depth ≤ 40, node count ≤ 2 000.

Read-only ValuePattern text, path-targeted `SetFocus`, and typed
`SelectionItemPattern`/`InvokePattern` result activation are exposed for Apple
Music search. UIA ValuePattern writes remain deferred until an owner-approved
consumer needs them.

## Deferrals

| Area | Deferral reason |
| --- | --- |
| WGC window/UWP capture | Needs owner-approved capture slice; PrintWindow covers common cases |
| UIA ValuePattern writes | Read-only values, focus, selection, and invocation are available; value mutation still needs an approved consumer (see `TODO(windows-ax-value-write)`) |
| Target-aware readiness assessment | Needs Windows equivalents for macOS app-bundle/frontmost concepts |
| UIAccess worker process | Deferred beyond v0 per feasibility design |
| Framework heuristics (Chromium/WPF/GTK routing) | Deferred to a Windows action resolver or delivery-policy slice |
| `PostMessage` / `WM_CHAR` delivery paths | Deferred; foreground `SendInput` is the sole v0 input path |
| NetEase Windows cross-platform live smoke | `auv-netease-music` still constructs `MacosDriver` directly; Windows session wiring deferred |

## Validation

The `validate` example (`crates/auv-driver-windows/examples/validate.rs`)
exercises every capability against the live desktop and prints inspectable
output. Run from the repo root:

```text
cargo run -p auv-driver-windows --example validate -- <command> [args]
```

Commands: `displays`, `windows`, `permissions`, `resolve <title-substr>`,
`capture-screen [out.png]`, `capture-window <substr> [out]`, `ocr <substr>`,
`ax <substr>`, `coords <substr>`, `clipboard`, `type <text>`, `press <key>`,
`click <x> <y>`, `scroll <x> <y> <delta_y>`, `move <substr> <x> <y>`,
`resize <substr> <w> <h>`, `minimize <substr>`, `unminimize <substr>`.

## Relationship to Feasibility Design

The feasibility doc
(`2026-06-11-windows-driver-feasibility-and-delivery-paths.md`) defined the
coverage matrix and delivery-path taxonomy. This reference records what
actually landed in v0 against that matrix:

- All "Easy" and most "Medium" items from the feasibility matrix are implemented.
- `window.click()`, `window.type_text()`, and `window.scroll()` are delivered
  as foreground `SendInput` only (no UIA pattern or `PostMessage` path yet).
- `window.resolve()` supports title/pid/exe/class selectors; `AppSelector::bundle`
  maps to package identity on Windows where available.
- `vision.recognize_text_in_capture()` uses the native `Windows.Media.Ocr`
  engine over a provided capture buffer.
- `permission.probe()` surfaces the three Windows-specific readiness signals
  described in the feasibility matrix.
