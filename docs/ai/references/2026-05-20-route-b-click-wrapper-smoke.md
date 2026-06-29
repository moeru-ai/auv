# Route B Click Wrapper Smoke

Date: 2026-05-20

Status: local macOS smoke evidence

## Purpose

This note records the first local smoke for route B:

```text
visual-only virtual cursor
-> save real cursor
-> CGEvent click at target
-> restore real cursor
-> hide virtual cursor
```

The goal is to test the core experience assumption behind "virtual cursor +
real cursor protection" before adding any runtime wrapper, recipe presentation
field, or trace integration.

## Scope

Added a local-only Swift smoke script, retired with `scripts/local/`:

```text
scripts/local/route-b-click-wrapper-smoke.swift
```

The script creates its own small target window, so the click does not hit the
user's current application. It intentionally does not:

- modify AUV runtime behavior
- wrap `debug.clickPoint`
- change recipe or presentation schema
- write trace events
- require Accessibility or Screen Recording permissions
- click a user application window

## Smoke Command

```bash
# Retired local-only script; kept here as historical command evidence.
swift scripts/local/route-b-click-wrapper-smoke.swift \
  --delta-x 420 \
  --delta-y 0 \
  --pre-click-ms 500 \
  --post-click-ms 700 \
  --label AUV
```

Output:

```text
routeBClickWrapperSmoke=true
initialCursor=373.082,589.309
clickTarget=793.082,589.309
overlayIgnoresMouseEvents=true
targetWindowFrame=698.082,340.691,190.000,104.000
preClickMs=500
postClickMs=700
manualObservation=watch whether the real cursor visibly flashes at target while the virtual AUV cursor remains visible
savedCursor=373.082,589.309
finalCursor=373.000,589.000
restored=true
clickDelivered=true
clickCount=1
targetWindowClickLocation=95.082,52.691
warpClickRestoreElapsedMs=18.384
```

## Findings

Mechanical findings:

- The visual-only overlay was configured to ignore mouse events.
- The wrapped CGEvent click was delivered to the target view.
- The target view recorded exactly one click.
- The real cursor was restored to the saved position within 1 logical pixel.

Timing finding:

- The measured warp/click/restore section took `18.384ms` in this run.
- That is close to one 60fps frame, so the subjective flicker question remains
  real. The implementation is mechanically viable, but not yet presentation-
  validated.

## Related Correction

The existing `debug.clickPoint` and `debug.scrollPoint` reports previously said:

```text
cursorAfter=target
```

That was false because the Swift click/scroll scripts already restore the
cursor with `CGWarpMouseCursorPosition(originalLocation)` in `defer`.

The correct report value is now:

```text
cursorAfter=restored-to-original
```

## Decision State

Route B is mechanically viable for a local smoke:

```text
virtual overlay visible
real click delivered
real cursor restored
```

It is not yet validated as a product presentation mode. The unresolved decision
is human-visible flicker. If the `18ms` warp/click/restore path looks acceptable
on the user's desktop, the next cut can be a debug-only wrapped click command.
If it looks distracting, AUV should prefer labeling the real cursor rather than
claiming a non-disturbing virtual cursor.
