# Windows foreground mouse recipient

Status: scoped native acceptance passed; PR #448 is under review and unmerged.
Classification: bug fix with application-owned regression coverage.

The owner goal requires input to stop when its target loses foreground, disappears,
or becomes unavailable. The accepted keyboard slice already retains window/process identity
and latches foreground loss during a batch. At the baseline, window-targeted foreground clicks called
global mouse delivery after an activation attempt. They did not open that recipient guard.

## Reproduction boundary

`background-session-eval window-mouse --output DIR` requires
`AUV_ISOLATED_DESKTOP_EVAL=1` and a fresh directory. It launches two owned Win32 receivers
at the same position. The existing receiver's button and counters are the application oracle.

The Driver requests three foreground-preferred clicks with a 400 ms interval.
After the first button release, receiver A can move foreground to B, return to A after 50 ms,
or minimize itself. The fault runs after the button processes its release and synthesizes no input.
The keyboard fixture uses the same native fault action; its trigger and receipt remain independent.

| Case | Required result | Application oracle |
|---|---|---|
| Success | Completed input | A receives three clicks; B receives none |
| Foreground takeover | Target lost with submitted prefix | A receives one click; B receives none |
| Foreground ABA | Target lost, without revival on return | A receives one click; B receives none |
| Minimize | Target lost with submitted prefix | A receives one click; B receives none; A is minimized |
| Invalid count/point/interval | Not delivered | No input, foreground change, or pointer change |
| Pre-cancelled | Not delivered | No input, foreground change, or pointer change |
| Observed owner mismatch | Target lost, not delivered | No input, foreground change, or pointer change |
| Cancel after first click | Cancelled with submitted prefix | A acknowledges one click before cancellation; B receives none |

The task keeps raw receiver states, native fault receipts, Driver results, desktop witness counts,
and captures. Cleanup verifies receiver process exit and HWND destruction.
Foreground handback and pointer restoration are controller teardown, separate from delivery.
The first run must execute against unchanged production code before a fix is attempted.

## Implementation boundary

The fix reuses the existing private recipient guard and its retained process, identity checks,
foreground-loss latch, submission receipts, cancellation, and watcher lifetime.
Do not add another input mutex or a parallel action-result schema.
Lock ordering preserves the existing desktop coordinator before the native submission gate.
Click count, finite point, and interval range are validated before admission or activation.
Admission opens the guard and permits at most one initial activation. The guard owns the native gate until watcher shutdown.
Its receipt records mouse disturbance as temporary, while the existing keyboard clipboard policy remains unchanged.
Errors after accepted submission retain `InputActionProgress`; they do not claim zero delivery.
Successful output retains a foreground-admission attempt followed by the native delivery attempt.
If native insertion returns a short click count, cleanup releases only the accepted downs.
Those releases use the existing cleanup boundary, bypass admission, and preserve the uncertain
submission receipt. A fake native-boundary regression first reproduced cleanup refusal after
admission loss, then passed after the fix. This is not a claim that CI forced a real short `SendInput` return.

This slice targets a foreground click batch. Posted background mouse messages remain a separate policy.
Foreground scrolling/held motion and local OCR-click cancellation-context installation need their own
app-owned interruption/parity cases. The current slice does not claim those paths share the new click admission.
It does not bind a semantic UI element to screen coordinates, make `GetForegroundWindow` plus `SendInput`
atomic, or create a pre-admission HWND generation receipt. Coordinate/frame checks need their own
producer/consumer evidence; the retired #429 screenshot predicate remains retired.
Root-close fault coverage needs a terminal receiver receipt before process exit and is marked at the case list.

## Validation

Local Windows compilation and Clippy passed with existing warnings. No local desktop input was run.
Local input units and native-boundary models passed 34 tests with one ignored, Invoke units passed 167 with one ignored,
and all six existing evaluation units passed. These do not prove native click delivery.
All-feature Driver compilation and the new evaluation command's help also passed.
The first CI preparation stopped before input because a shallow checkout lacked the parent used for production-diff validation.
That task is not native behavior evidence. Fetching the parent corrected the preparation boundary.

The [unchanged-production baseline](https://github.com/moeru-ai/auv/actions/runs/41187048131)
ran source `e39b9cdf27b9615577ac2d41f3da49f44bfbae31` under workflow `902415fa`.
Artifact `13036219915` matched SHA-256
`12b39e88b6cda58b2f2d2e221d87e8e9535997b2481dd4b7c7c3817bf95843d5` after download.

| Baseline case | A clicks | B clicks | Driver result |
|---|---:|---:|---|
| Success | 3 | 0 | Success |
| Takeover | 1 | 2 | Success, no target-loss progress |
| ABA | 3 | 0 | Success, batch revived after return |
| Minimize | 1 | 2 | Success, no target-loss progress |

Independent review confirmed the alternate receiver's two-click images and the ABA recipient's three-click image.
All 15 existing keyboard cases passed after fixture extraction, with 30 verified receiver exits/destructions.
All eight mouse receivers exited and their HWNDs were destroyed.
The corrected replay adds five zero-delivery admission cases and cancellation after an acknowledged click to the original four.
The first corrected replays ([source `62c18086`](https://github.com/moeru-ai/auv/actions/runs/41188726649)
and [source `88769ca7`](https://github.com/moeru-ai/auv/actions/runs/41188916764)) passed nine mouse cases,
all 15 keyboard cases, and the five watcher models. The later replay also passed the owned cleanup model.
Only pre-cancel failed: receiver counts and desktop witness confirmed zero delivery, but the outer
desktop reservation returned a plain cancellation error without `NotDelivered` progress.
Artifact `13037539594` matched SHA-256
`ded0f3df7093d1791322c3535a105891dd7b8880ba769e0213e1ff3bc2e71498` on download.
All 20 mouse receivers exited with destroyed HWNDs, and pointer/foreground were restored.

A public-boundary unit reproduced the missing pre-cancel receipt before production changes.
Foreground click now leaves desktop admission to its existing delivery boundary, which stamps refusal.
Posted background click keeps its outer reservation. The next section records corrected native acceptance.

## Native acceptance

The [receipt-corrected replay](https://github.com/moeru-ai/auv/actions/runs/41189403167)
passed at source `5f666614fc8a70f9b12f3405b59d136d03c67f3e`,
workflow `67d191ee149e7c5c2ceacc7238823abef949b2fa`, job `124502875300`.
Artifact `13038700569` matched SHA-256
`3fcf90deaa85445ddf553317859aa028220f058385a8a6d5ea4e20a9756e124a` on download.
The [permanent manifest](evidence/2026-10-31-windows-mouse-recipient/acceptance.json)
retains source, environment, binary hashes, per-case receipts, and capture hashes.

All ten mouse cases passed. Success delivered three clicks to A and none to B.
Takeover, ABA, minimize, and cancellation after acknowledgement each stopped after one A click,
with no B click and a submitted-prefix receipt. The five pre-delivery refusal cases emitted no
mouse or keyboard input and preserved foreground/pointer state. Pre-cancel now retains its cause
and `NotDelivered` receipt; owner mismatch retains `TargetLost`.

The same job passed all 15 existing keyboard cases, five watcher lifecycle models,
the owned-release model, and the pre-cancel unit. All 50 owned receivers exited with destroyed HWNDs.
No physical input or held modifiers were detected; foreground and pointer restoration passed.
All 19 mouse captures were reviewed against application counters. The minimized recipient's capture
was explicitly refused without restoring the window. The offline acceptance check rejected the earlier
missing-receipt run before accepting this corrected run.

This evidence used build 26100, image `20261021.278.1`, interactive session 2,
one 1024 by 768 display, and a measured 96-DPI window. It does not cover 125% or 150% scaling,
real short `SendInput` returns, other frontends' cancellation propagation, or the full Windows goal.
