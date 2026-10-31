# Windows complete scroll batch lifetime

Status: scoped native acceptance passed; unmerged implementation prepared for review.
Classification: bug fix with application-owned regression coverage.
Base: PR #449, `05b583359fd79ab9ab526598db7f64ad917392ff`.

## Contract and reproduced boundary

The owner goal requires a target lease across input steps and typed delivery progress
when a recipient is lost or a batch is cancelled. A single foreground scroll now holds
its recipient through delivery and settle. At the baseline, timed and live scroll
opened that guard per sample. Another foreground or an ABA return between samples
could therefore become a new admission. The common executor also formatted a later
error into `Backend`, losing its original category and typed input receipt.

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

## Implemented ownership boundary

The Windows adapter retains one desktop/native admission and recipient for the
whole foreground-preferred motion or stream. Nested scroll samples reuse only
that same recipient, without activation or resetting accumulated native progress.
The shared executor keeps its scheduling and quantization. The provided
`WindowInput::wait_scroll(deadline)` method defaults to input-cancellable waiting.
The Windows override reuses recipient waiting, so loss wakes idle streams and
final settle as well as moving samples. Existing primary method signatures stay unchanged.

Partial failure now retains `DriverError::InputAction` with the original cause.
An earlier accepted sample prevents a later zero-delivery refusal from erasing
the prefix. A native failure without a receipt stays uncertain. Delta context
moves from a flattened backend message into a failed action-attempt message.
Cancellation between samples retains a submitted prefix.

Background-preferred path selection and posted-partial fallback remain separate.
Frame/coordinate freshness and atomic native input binding are not claimed here.
TODO(windows-held-mouse-recipient): drag owns a common-coordinator hold/release
worker and cannot simply nest another desktop reservation. Complete its interruption,
lock-order and owned-release contract in the next owner-named input-lease slice.

## Baseline evidence

The [unchanged-production replay](https://github.com/moeru-ai/auv/actions/runs/41201512555)
ran source `4ea2559831d1a15635f11d2df4306a24493359bb`,
workflow `5ae43cfc100c70e060ad8a7a4ca7f2bc83ae4796`, job `124538072063`.
Artifact `13042717896` matched SHA-256
`fed9d2ff376ab57f2930fc2d887fc5e2c8f30dc3e6d1e0cf0fc6ef99422ee08a` after download.

Both success cases passed. Motion takeover and ABA still returned success after
three and four A wheels. Live takeover and ABA returned success after two and
three A wheels. Minimize and close stopped at one wheel but lost the typed cause
and prefix. The idle stream lost foreground and still returned empty success.
Thus nine of eleven batch cases failed. All 22 batch receivers exited with
destroyed HWNDs; physical input stayed zero and desktop restoration passed.
Selected recipient/alternate images agree with the extra scroll counts.
The same job passed the existing 22 mouse cases, fifteen keyboard cases, and eight pure regressions.

Two local regressions first failed for typed target loss and cancellation after
accepted delivery, then passed with the common fix. All 110 common units and both
new public-boundary pre-cancel units passed.
The corrected task adds refusal, cancellation, final-settle loss, and nested
retarget rejection cases for 27 total batch cases.

## Corrected native acceptance

The [corrected replay](https://github.com/moeru-ai/auv/actions/runs/41206480036)
passed at source `ad0441bc26243cf6a5ac31a6c964588ad4e893f1`,
workflow `ac5cabcfed1c9cbed3c75b73cb15573006a2b5cd`, job `124552467771`.
Artifact `13044666273` matched SHA-256
`a32b3eb13991101a0194b0ce32bee03740b7ac9ee3b7ea37b9c353dc8d33bbd0` after download.
The [permanent manifest](evidence/2026-10-31-windows-scroll-batch/acceptance.json)
retains source, environment, binary hashes, application state, typed receipts,
terminal close state, capture hashes, and visual-review groups.

All 27 batch cases passed. Takeover, ABA, minimize, and close stopped after one
acknowledged wheel in both motion and stream, with no B input or automatic revival.
Cancellation after the prefix retained submitted progress. Empty-stream loss and
cancellation retained `NotDelivered`. Ten admission/refusal cases produced no native
input or pointer/foreground change. Final-settle loss retained the accepted prefix,
and a nested attempt to change the recipient was refused without B input.

The same source passed all 22 prior mouse cases, fifteen keyboard cases, and
34 selected pure regressions. Independent audit checked all 128 receiver exits
and HWND destructions, zero physical input, released modifiers, and desktop restoration.
Eight minimized/closed capture refusals cover the new batch and prior mouse matrices.
The task did not restore those windows to obtain pixels.

Visual review covers fifty batch and forty prior mouse captures. Fifty-nine
exactly match previously reviewed hashes; the remaining 31 share eleven new
image contents, each inspected against the application counters. The manifest
records the prior run/path/hash matches and new representative groups.
Offline audit rejected the baseline before accepting the corrected source.

Local checks passed: all 110 common units, two new pre-cancel units, 34 Windows
input units with one ignored, and 167 Invoke units with one ignored. All-target
and all-feature Driver checks, Clippy with existing warnings, both command helps,
formatting, and diff checks passed. No local desktop input was used.
This native evidence used build 26100, image `20261021.278.1`, interactive session 2,
one 1024 by 768 display, and measured 96 DPI. It does not accept the remaining
background, drag, coordinate, scaled-DPI, or full frontend requirements of the owner goal.
