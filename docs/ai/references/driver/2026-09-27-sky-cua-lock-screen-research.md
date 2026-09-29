# Codex and CUA lock-screen evidence (2026-09-27)

Status: public-source research only. This note does not establish an AUV or
Codex login-screen capability. Local installation paths, package versions,
installation status, and binary inspection observations are outside this
public evidence record.

## Public evidence

| Surface | Evidence | Boundary |
| --- | --- | --- |
| Codex locked use | A [public Codex issue](https://github.com/openai/codex/issues/32913) reports a Computer Use request failing while a Mac was locked, despite an active remote turn. | A user report describes one failure, not a general support contract or a successful login-screen capture/input test. |
| CUA host macOS driver | [`capture.rs`](https://github.com/trycua/cua/blob/02fdd98ad59a00752bdccea396164cdefbe2afb5/libs/cua-driver/rust/crates/platform-macos/src/capture.rs) invokes `/usr/sbin/screencapture`; [`skylight.rs`](https://github.com/trycua/cua/blob/02fdd98ad59a00752bdccea396164cdefbe2afb5/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs) sends input to a PID; [`check_permissions.rs`](https://github.com/trycua/cua/blob/02fdd98ad59a00752bdccea396164cdefbe2afb5/libs/cua-driver/rust/crates/platform-macos/src/tools/check_permissions.rs) checks Accessibility and Screen Recording. | These are existing-session paths. The cited files do not establish host login-screen capture or input. Permission checks are not lock-screen behavior tests. |
| CUA virtual-machine console | [`VNCClient.swift`](https://github.com/trycua/cua/blob/02fdd98ad59a00752bdccea396164cdefbe2afb5/libs/lume/src/VNC/VNCClient.swift) handles RFB framebuffer, pointer, and key events; [`vnc.py`](https://github.com/trycua/cua/blob/02fdd98ad59a00752bdccea396164cdefbe2afb5/libs/python/cua-sandbox/cua_sandbox/transport/vnc.py) exposes screenshot and input over VNC. | A virtual console is a separate boundary from the host user's GUI session. This source does not prove every guest lock or login condition. |

Keep an already logged-in locked session, an OS login screen with no logged-in
user, and FileVault preboot unlock separate when assessing AUV. The public
evidence above does not show that ordinary in-session capture, accessibility,
or PID-targeted input can operate the host login screen.
