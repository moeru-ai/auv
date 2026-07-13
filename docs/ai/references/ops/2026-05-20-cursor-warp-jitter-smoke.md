# Cursor Warp Jitter Smoke

Date: 2026-05-20

Status: local macOS smoke evidence

## Purpose

This note records the first direct test of route B for AUV cursor presentation:

```text
save real cursor position
-> CGWarpMouseCursorPosition(target)
-> CGWarpMouseCursorPosition(original)
```

The goal is not to add runtime behavior. The goal is to test whether the
underlying real-cursor warp/restore mechanism is mechanically viable before any
virtual cursor + real cursor protection design is built on top.

## Scope

Added a local-only Swift smoke script, retired with `scripts/local/`:

```text
scripts/local/cursor-warp-jitter-smoke.swift
```

The script intentionally does not:

- click
- use the overlay daemon
- wrap `click_point`
- modify recipe or presentation schema
- require Accessibility or Screen Recording permissions
- run as part of normal `cargo test`

## Smoke Command

```bash
# Retired local-only script; kept here as historical command evidence.
swift scripts/local/cursor-warp-jitter-smoke.swift \
  --delta-x 420 \
  --delta-y 0 \
  --repeat 5 \
  --interval-ms 450 \
  --settle-ms 0
```

Output:

```text
cursorWarpJitterSmoke=true
original=737.082,422.617
target=1157.082,422.617
repeats=5
intervalMs=450
settleMs=0
action=CGWarpMouseCursorPosition(target)->CGWarpMouseCursorPosition(original)
manualObservation=watch whether the real cursor visibly flashes at target before returning
sample	1	elapsedMs=1.857
sample	2	elapsedMs=0.145
sample	3	elapsedMs=0.323
sample	4	elapsedMs=0.142
sample	5	elapsedMs=0.358
final=737.000,422.000
restored=true
```

## Findings

Mechanical findings:

- Immediate warp/restore works without a click.
- The cursor position was restored to the original location within 1 logical
  pixel.
- The measured warp/restore section took roughly `0.14ms` to `1.86ms` in this
  run.

Boundary:

- The log can prove only timing and restoration.
- It cannot prove whether the one-frame visual flicker is acceptable to a human.
- The route B decision still needs manual observation on the target desktop.

## Decision State

Route B is not disproven mechanically, but it is not validated as a product
presentation strategy yet.

Next decision:

```text
If the user observes no obvious flicker:
  continue toward virtual cursor + save/restore click wrapper experiment.

If the user observes obvious flicker:
  do not advertise route B as non-disturbing.
  fall back to labeling the real cursor during AUV control.
```
