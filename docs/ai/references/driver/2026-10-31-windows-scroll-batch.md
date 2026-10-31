# Windows complete scroll batch lifetime

Status: test-only reproduction prepared; unchanged-production native replay pending.
Classification: test-only reproduction before a scroll batch bug fix.
Base: PR #449, `05b583359fd79ab9ab526598db7f64ad917392ff`.

## Contract and reproduced boundary

The owner goal requires a target lease across input steps and typed delivery progress
when a recipient is lost or a batch is cancelled. A single foreground scroll now holds
its recipient through delivery and settle. Timed and live scroll still open that guard
per sample. Another foreground or an ABA return between samples can therefore become
a new admission. The common executor also formats a later error into `Backend`, losing
its original category and typed input receipt.

## Application-owned reproduction

`background-session-eval window-scroll-batch --output DIR` requires an isolated desktop
and a fresh directory. It uses the existing owned Win32 receivers, desktop witness,
Driver capabilities, capture and cleanup checks. Desktop behavior runs only in hosted CI.

The task requests a one-second 400-pixel motion, or a 400-pixel/second live stream
with a one-second lease, at four samples per second. After the first completed Driver
sample, the progress callback pauses the actor until A acknowledges its first wheel.
It then asks A's GUI thread to move foreground to B, return to A, minimize, or close.
This fault occurs after the old sample guard has dropped. It uses no timer to guess
whether the application consumed input. The new fixture command is `apply_mouse_fault`.
Mouse-close keeps the existing terminal GUI receipt before destruction.

Success requires application scroll movement. Every loss case must return `TargetLost`
with submitted progress and exactly one A wheel, while B remains unchanged.
An idle stream also loses foreground before any wheel; it must return `TargetLost`
with `NotDelivered` rather than an empty successful stream.
The 22 prior mouse cases and fifteen keyboard cases remain regression gates.

## Planned ownership boundary

The Windows adapter must retain one desktop/native admission and recipient for the
whole foreground-preferred motion or stream. Nested scroll samples may reuse only
that same recipient, without activation or resetting accumulated native progress.
The shared executor keeps its scheduling and quantization. Its wait boundary must
allow the platform adapter to notice recipient loss while waiting, including idle
streams and final settle. Public wait-method naming is provisional until implemented.
Typed partial failure must preserve the original cause and conservatively retain
submitted or uncertain effects, including cancellation between samples.

Background-preferred path selection and posted-partial fallback remain separate.
Frame/coordinate freshness and atomic native input binding are not claimed here.
TODO(windows-held-mouse-recipient): drag owns a common-coordinator hold/release
worker and cannot simply nest another desktop reservation. Complete its interruption,
lock-order and owned-release contract in the next owner-named input-lease slice.
