# QQ Music background control (living doc)

Goal: control QQ Music without stealing foreground focus (parity with macOS background input). Append new phases here.

## Phase 1 — SMTC + CoreAudio driver (2026-10-03)

Recon: QQ Music registers an SMTC session (`QQMusic.exe`). Probe of background search-and-play paths: (1) HWND `WM_SETTEXT`: `TXGuiFoundation` has 0 child Edit HWNDs; sending `WM_SETTEXT` only mutates the window caption bar, unable to target the DirectComposition search box. (2) UIA `ValuePattern::SetValue`: returns `0x80004001` (`E_NOTIMPL`) and actively steals foreground focus (`SetForegroundWindow`). (3) UIA `LegacyIAccessible::SetValue`: returns `0x80004001` (`E_NOTIMPL`). WGC captures occluded windows; minimized windows are not capturable (DWM suspends composition).

Shipped `media.rs`: `SmtcMediaManager` / `SmtcSession` (play, pause, toggle, next, previous, status, track metadata) + `AudioVolumeController` (per-process volume via CoreAudio; global volume untouched).

Eval: 100 ops, `GetForegroundWindow` asserted unchanged after every op — 0 focus steals. 100 records (`2026-10-03-qqmusic-background-control.jsonl`).

| Op | P50 | P95 |
|---|---|---|
| metadata query (30) | 0.24 ms | 0.43 ms |
| play / pause (20 cycles) | 0.35 / 0.37 ms | 0.51 / 0.46 ms |
| next / previous (10 rounds) | 0.20 / 0.22 ms | 0.24 / 0.24 ms |
| process volume set (10) | 1.74 ms | 2.41 ms |

Decision: P0 shipped. P1 (background search-and-play): honest-stop NO-GO (all candidate background text paths — HWND `WM_SETTEXT`, UIA `ValuePattern.SetValue`, and UIA `LegacyIAccessible.SetValue` — are unsupported by `TXGuiFoundation` or violate the zero-focus-stealing redline).
