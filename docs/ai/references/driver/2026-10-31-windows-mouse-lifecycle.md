# Windows mouse recipient lifetime

Status: test-only reproduction prepared; unchanged-production native run pending.
Classification: test-only reproduction before a foreground scroll bug fix.
Base: PR #448, `9810bd254f752619291977c73c7d586ffea99f42`.

## Contract

The owner goal requires window input to retain its recipient, stop on target loss,
and preserve submitted or uncertain effects. Foreground click batches already use
the recipient guard. Foreground scroll still activates once and calls global
scroll delivery, then sleeps for the requested settle duration without that guard.
A window closing after a native click also needs a terminal application receipt.
The controller cannot ask an exited process for its final counters.

## Application-owned reproduction

The existing `window-mouse` task adds a click-close case and five scroll cases.
The scrolling edit is focused through the already accepted Driver click path
before witness accounting and before a fault is armed. A single downward wheel
must advance its first visible line and increment its raw wheel count once.
The application can then move foreground to B, return to A, minimize A, or close A.
It runs the fault after the original control processes the input message.

The receiver's `arm_mouse_fault` accepts an optional `trigger` of `click` or `wheel`.
Omission retains the original click trigger. Keyboard fault behavior stays unchanged.
For an armed mouse-close fault, the GUI thread emits a `closing` event containing
the same application state as its normal state query, plus the native fault receipt.
It writes this event while the controls still exist, immediately before destruction.
The controller reads it without sending another request to the dying process.
Owned process exit and HWND destruction are checked separately.

Scroll success requires an application effect and Driver success. Scroll takeover,
ABA, minimize, and close require `TargetLost` with a submitted prefix during settle,
one A wheel, advanced A scroll position, and unchanged B scroll state.
Click-close requires one A click, zero B clicks, a terminal receipt, and target-loss progress.
The ten prior click cases and fifteen keyboard cases remain regression gates.
All desktop input runs only on hosted isolated interactive desktops.

## Limits

This slice does not make foreground checks and native input atomic or provide
frame/coordinate freshness. Closed and minimized windows must be refused by capture,
not restored to produce evidence. Cancellation after a wheel and invalid/pre-cancel
scroll admission are required for the corrected replay after the baseline reproduction.

TODO(windows-held-mouse-recipient): held motion has a separate common-coordinator
release worker and needs its own interruption/owned-release evidence before sharing
the guard. The owner goal names that follow-up; it is not accepted by this scroll slice.
No general toolkit, scaled-DPI, or cross-frontend cancellation claim follows from this fixture.
