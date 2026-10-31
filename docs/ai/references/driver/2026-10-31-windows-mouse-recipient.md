# Windows foreground mouse recipient

Status: native bug reproduced; fix compiled, corrected native replay pending.
Classification: bug fix with application-owned regression coverage.

The owner goal requires input to stop when its target loses foreground, disappears,
or becomes unavailable. The accepted keyboard slice already retains window/process identity
and latches foreground loss during a batch. Window-targeted foreground clicks still call
global mouse delivery after an activation attempt. They do not currently open that recipient guard.

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

This slice targets a foreground click batch. Posted background mouse messages remain a separate policy.
Foreground scrolling/held motion and local OCR-click cancellation-context installation need their own
app-owned interruption/parity cases. The current slice does not claim those paths share the new click admission.
It does not bind a semantic UI element to screen coordinates, make `GetForegroundWindow` plus `SendInput`
atomic, or create a pre-admission HWND generation receipt. Coordinate/frame checks need their own
producer/consumer evidence; the retired #429 screenshot predicate remains retired.
Root-close fault coverage needs a terminal receiver receipt before process exit and is marked at the case list.

## Validation

Local Windows compilation and Clippy passed with existing warnings. No local desktop input was run.
Local input units passed 28 tests with one ignored, Invoke units passed 167 with one ignored,
and all six existing evaluation units passed. These do not prove native click delivery.
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
No native correctness claim follows from compilation; corrected counts, progress, cancellation, and cleanup remain unverified.
