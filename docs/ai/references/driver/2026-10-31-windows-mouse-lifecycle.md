# Windows mouse recipient lifetime

Status: native baseline reproduced; fix implemented, corrected replay pending.
Classification: bug fix with application-owned regression coverage.
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

## Implementation

Foreground-preferred single-call scroll validates its point, deltas, and settle clock range
before activation. It shares the existing click recipient boundary: desktop coordinator first,
native submission gate second, retained process/window identity and loss latch until watcher shutdown.
The scroll keeps that guard through native delivery and the requested settle.
Cancellation and target loss during settle retain `InputActionProgress` from the accepted native prefix.
Refusal before delivery retains `NotDelivered`. The ten prior click cases exercise the shared boundary.

Background-only posting and background-preferred fallback retain their previous policy.
Posted failure needs its own partial-delivery contract before that fallback can share the new admission;
the deferral is marked at the policy site. Sampled/streamed scroll still admits each call separately.
Its complete motion lease, and held drag motion, remain separate requirements of the full owner goal.

The corrected replay adds six scroll admission/cancellation cases: invalid point, invalid delta,
out-of-range settle, pre-cancel, observed owner mismatch, and cancellation after the application
acknowledges its first wheel. Invalid settle is excluded from the unchanged baseline because its
old unbounded sleep would prevent safe completion of the reproduction.

## Baseline evidence

The [unchanged-production replay](https://github.com/moeru-ai/auv/actions/runs/41190986120)
ran source `7140627c37efbd0b6332d8bbb4620f839182a5cd`,
workflow `f0216e0019ffc6dd7d2b24de02ecc0b931bb6657`, job `124507562240`.
Artifact `13038663749` matched SHA-256
`8e48d371e844d186c1acef4e44750a4b43a70b944dfe2909ca3ddb721d2e2922` after download.
All eleven click cases, scroll success, and all fifteen keyboard cases passed.
The four scroll-loss cases each advanced A from line 0 to line 3, left B at line 0,
then incorrectly returned Driver success without target-loss progress.
Click-close and scroll-close both supplied the terminal GUI receipt; all 32 mouse receivers
exited with destroyed HWNDs. Zero physical input, released modifiers, and desktop restoration passed.
Reviewed ABA and alternate-receiver captures agree with the application scroll state.

A local public-boundary unit also reproduced the missing pre-cancel `NotDelivered` receipt
before the production fix. Local compilation is not corrected native acceptance.
