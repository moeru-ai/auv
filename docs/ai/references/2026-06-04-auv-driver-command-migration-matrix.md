# AUV Driver Command Migration Matrix

Date: 2026-06-04

This matrix tracks migration from legacy command routing to `auv-driver` typed capabilities.

Legend:

- `already-bridged`: legacy handler already calls typed `auv-driver` / `auv-driver-macos` APIs enough for the first bridge phase.
- `migrate`: useful command; change handler internals to call typed driver APIs.
- `driver-gap`: useful command; first define the missing atomic typed capability or result mapping in `auv-driver` / `auv-driver-macos`, without adding a separate primitive API layer.
- `delete`: historical command or recipe path; do not preserve.
- `defer`: valid capability but outside the first bridge batch; leave a code marker.

| Command | Operation | Bucket | Status | Evidence | Next action |
| --- | --- | --- | --- | --- | --- |
| `debug.captureDisplay` | `capture_display` | Capture | `already-bridged` | Root handler resolves legacy selectors and calls `typed::session::capture_display_bridge`, backed by `MacosDriverSession::display().capture()`. | Keep selector compatibility tests around `display_ref` vs `display_id`. |
| `debug.captureRegion` | `capture_region` | Capture | `already-bridged` | Root handler keeps legacy coordinate-space resolution and calls `typed::session::capture_region_bridge`, backed by `MacosDriverSession::display().capture_region()`. | Keep coordinate contract regression coverage. |
| `debug.captureWindow` | `capture_window` | Capture | `already-bridged` | Uses `capture_window_with_typed_session`. | Keep as regression anchor. |
| `debug.listDisplays` | `list_displays` | Window/Capture | `already-bridged` | Root handler calls `typed::session::list_displays_bridge`, backed by `MacosDriverSession::display().list()`. | Keep display descriptor JSON compatibility. |
| `debug.listWindows` | `list_windows` | Window | `already-bridged` | Unfiltered root handler calls `typed::session::list_windows_bridge`, backed by `MacosDriverSession::window().list()`. App-filtered calls remain legacy compat to preserve candidate metadata. | Add typed app-scoped listing before removing app-filter legacy path. |
| `debug.clickWindowPoint` | `click_window_point` | Input | `already-bridged` | Uses `click_window_point_bridge`. | Keep and test typed bridge signals. |
| `debug.scrollWindowRegion` | `scroll_window_region` | Input | `already-bridged` | Uses `scroll_window_point_bridge`. | Keep and test typed bridge signals. |
| `debug.typeText` | `type_text` | Input | `driver-gap` | Uses root System Events helper. | Add typed foreground text input. |
| `debug.pressKey` | `press_key` | Input | `driver-gap` | Uses root System Events helper. | Add typed key press input. |
| `debug.pasteTextPreserveClipboard` | `paste_text_preserve_clipboard` | Input | `already-bridged` | Uses typed paste bridge with legacy fallback. | Add deferral marker for unsupported submit keys. |
| `debug.captureAxTree` | `capture_ax_tree` | AX | `defer` | Calls `auv-driver-macos::native::ax_tree` directly; first bridge batch is focused on capture/window/input commands. | Leave code marker that AX capture is deferred until owner approves a typed `auv-driver` AX capability. |
| `debug.findWindowText` | `find_window_text` | Vision | `migrate` | Uses typed window capture path but OCR remains root adapter. | Rewire only through existing `auv-driver-macos` typed OCR/capture capability; do not introduce a new primitive API. |
| `music.search.results` | `music_search_results` | Domain | `delete` | Root music domain path; NetEase has domain crate. | Delete with old recipe phase after app-local music commands cover it. |
| `music.result.play` | `music_result_play` | Domain | `delete` | Root music domain path; NetEase has domain crate. | Delete with old recipe phase after app-local music commands cover it. |
| `music.validate.candidate.liveness` | `music_validate_candidate_liveness` | Domain | `delete` | Root music domain path; NetEase has domain crate. | Delete with old recipe phase after app-local music commands cover it. |
| `debug.overlay*` | `overlay_*` | Overlay | `defer` | Visual-only presentation, not input backend. | Keep separate from `auv-driver` bridge. |
