# Windows foreground mouse recipient

Status: test-only reproduction prepared; native behavior is not accepted yet.

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

The task keeps raw receiver states, native fault receipts, Driver results, desktop witness counts,
and captures. Cleanup verifies receiver process exit and HWND destruction.
Foreground handback and pointer restoration are controller teardown, separate from delivery.
The first run must execute against unchanged production code before a fix is attempted.

## Planned implementation boundary

Reuse the existing private recipient guard and its retained process, identity checks,
foreground-loss latch, submission receipts, cancellation, and watcher lifetime.
Do not add another input mutex or a parallel action-result schema.
Lock ordering must preserve the existing desktop coordinator before the native submission gate.
Errors after accepted submission must retain `InputActionProgress`; they must not claim zero delivery.

This slice targets a foreground click batch. Posted background mouse messages remain a separate policy.
It does not bind a semantic UI element to screen coordinates, make `GetForegroundWindow` plus `SendInput`
atomic, or create a pre-admission HWND generation receipt. Coordinate/frame checks need their own
producer/consumer evidence; the retired #429 screenshot predicate remains retired.
Root-close fault coverage needs a terminal receiver receipt before process exit and is marked at the case list.

## Validation

Local Windows compilation and Clippy passed with existing warnings. No local desktop input was run.
Native success/failure, click counts, target-loss progress, ABA refusal, and cleanup remain unverified.
