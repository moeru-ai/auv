# Dual Cursor Notes Demo Boundary — 2026-05-21

Status: boundary / de-promotion note

## Purpose

Record the current truth for
`macos.demo.dual_cursor_press_notes.v0` so the repo stops treating it as a
validated presentation baseline when the live desktop facts do not support
that claim.

## What was observed

Hands-off replay:

```bash
cargo run --quiet -- skill cases run \
  macos.demo.dual_cursor_press_notes.v0 \
  --case notes-dual-cursor-demo \
  --inspect-server-write true \
  --require-inspect-server-write
```

Recorded failed run:

- `run_1779371986165_16879_0`

Observed failure:

- `create-note` failed at `debug.axPressButton`
- driver failure message:
  `no matching AX-pressable node found for query 新建备忘录`

Live AX inspection of Notes during the same evaluation showed:

- the toolbar still contained a `新建备忘录` button
- but that button was disabled / not currently AX-pressable in the live app
  state

So the failure was not a viewer bug and not a runtime signal-loss bug. The
demo's own preconditions were too soft for the current Notes UI state.

## Human interference rule

During later replay attempts on the same day, the operator explicitly touched
the desktop:

- switched into Notes
- copied / pasted manually

Those runs are contaminated and must not be used as promotion evidence.

For this demo class, evidence is valid only when the desktop is hands-off for
the entire replay.

## Repo truth change

Because of the failed hands-off replay and the contaminated follow-up runs:

- `notes-dual-cursor-demo` should not remain `validated`
- the demo should be treated as `candidate`
- README/docs should say "intended visual" rather than "current baseline"

## What this does NOT mean

- It does **not** mean the inspect viewer is broken.
- It does **not** mean the overlay implementation is disproven.
- It does **not** mean `debug.axPressButton` is globally broken.

It means this specific Notes presentation demo is not currently robust against
live Notes state and therefore cannot honestly be marketed as validated.

## Next honest follow-up

If this demo is to be promoted again, it needs one of:

1. a reproducible Notes pre-state that guarantees `新建备忘录` is AX-pressable
2. a recipe pre-step that normalizes Notes into that state
3. a narrower contract that stops promising note creation and demonstrates
   only an already-available AX focus / overlay interaction

Until one of those is proven with a new hands-off run, keep the demo at
candidate status.
