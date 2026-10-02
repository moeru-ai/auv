# QQ Music background control (living doc)

Goal: control QQ Music without stealing foreground focus (parity with macOS background input). Append new phases here.

## Phase 1 — SMTC + CoreAudio driver (2026-10-03)

Recon: QQ Music registers an SMTC session (`QQMusic.exe`). Its UIA search box rejects `ValuePattern::SetValue` (`E_NOTIMPL`) and its UIA provider steals focus when called — UIA `SetValue` is banned for this app. WGC captures occluded windows; minimized windows are not capturable (DWM suspends composition).

Shipped `media.rs`: `SmtcMediaManager` / `SmtcSession` (play, pause, toggle, next, previous, status, track metadata) + `AudioVolumeController` (per-process volume via CoreAudio; global volume untouched).

Eval: 100 ops, `GetForegroundWindow` asserted unchanged after every op — 0 focus steals. 100 records (`2026-10-03-qqmusic-background-control.jsonl`).

| Op | P50 | P95 |
|---|---|---|
| metadata query (30) | 0.22 ms | 0.48 ms |
| play / pause (20 cycles) | 0.34 / 0.37 ms | 0.48 / 0.42 ms |
| next / previous (10 rounds) | 0.22 / 0.23 ms | 0.26 / 0.28 ms |
| process volume set (10) | 1.65 ms | 3.69 ms |

Decision: P0 shipped. P1 (UIA search-and-play): open.
