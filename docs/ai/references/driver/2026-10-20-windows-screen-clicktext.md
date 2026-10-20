# Windows screen.clickText through local Invoke

Status: implemented and exercised against an owned native Win32 receiver.
Base: `11d9e256` on `main`. This closes `TODO(invoke-screen-click-text-windows)`.

## Behavior and boundary

`auv invoke screen.clickText "Continue"` now reaches the existing Windows
primary-display capture, system OCR and foreground `click_at` path. No Windows
Driver implementation, protobuf schema or Runner lifecycle was changed. macOS
keeps its path; Linux remains explicitly unsupported.

The support cell is `Partial("primary display only; foreground pointer input")`.
This command moves the real pointer and may activate the window under the match.
It does not provide background input, target activation or a focus lease.
`--target` is rejected before desktop access. Missing text returns typed
`NotFound` without input. A dry run is validation only.

The handler and typed helper share cancellation checks before capture and
between capture, OCR and click. Synchronous Driver calls retain their own
timeout; cancellation cannot interrupt a call already in progress. Results keep
OCR matches, the selected screen point and `InputActionResult`, with the existing
OCR-source and input-action tracing paths. Delivery evidence remains distinct
from application-effect verification.

## Regression evidence

The initial Windows dry-run regression failed with the old typed `Unsupported`
response before the production change. Afterward, the Invoke package passed
129 unit tests (one unrelated ignored test), 12 integration tests and one macro
integration test. Added coverage checks Windows admission, cancellation before
desktop access, target rejection and the advertised foreground/primary limits.

The existing opt-in `foreground` evaluation now also exercises `screen.clickText`.
It uses the receiver's own button geometry to refuse any OCR match outside the
owned control, waits for physical input to stop, then sends one click. Readiness
may be observed up to three times, 200 ms apart; a delivered click is never
retried. No-match behavior is checked through both typed failure and independent
receiver/watch state.

Selected results are in [summary.json](evidence/2026-10-20-windows-screen-clicktext/summary.json).
Raw records stay local under `reports/windows-gui-20261020/`.

| Run | Button effect | Missing query | Quiet desktop witness | Restoration |
| --- | --- | --- | --- | --- |
| `1792494934-8168` | 7 → 8, foreground path | `not_found`, 8 → 8, no injected input | 7 held, 0 inconclusive, 0 violated | pointer, foreground, clipboard restored |
| `1792495037-24940` | 7 → 8, foreground path | `not_found`, 8 → 8, no injected input | 2 held, 5 inconclusive, 0 violated | pointer, foreground, clipboard restored |

The second run observed 296 physical mouse events; its inconclusive intervals
are retained and do not prove desktop quietness. The first run had no physical
mouse or key events. Acting commands intentionally move the pointer/send keys;
their raw background-invariant verdicts are retained, and are not advertised as
background passes. The harness separately checks that input stays on its owned
window. Driver `verified:false` is not promoted to semantic success: the button's
processed-click counter provides the effect evidence.

Two earlier preflights found no owned caption and stopped before sending
`screen.clickText`. Their records remain local. The exact OCR miss cause was not
established; changing a caption alone is not presented as a Driver fix. Only
the owned window is retained if a preflight needs image diagnosis; full desktop
OCR artifacts are removed by the existing store guard.

## Reproduce

Use an interactive Windows desktop and fresh output directories. The foreground
scenario briefly uses the real pointer/keyboard on its own window and restores
the previous pointer, foreground and rich clipboard on exit.

```powershell
cargo test --locked -p auv-cli-invoke
cargo build --locked -p auv-cli -p auv-base-evals --bin auv --bin background-session-eval --bin win32-receiver
target/debug/background-session-eval.exe foreground --auv target/debug/auv.exe --output reports/windows-screen-clicktext-new
```

For the recorded runs, `CARGO_TARGET_DIR=F:/auv-targets/windows-gui` and temporary
build files were placed on F: because C: was low on space. This is a local build
choice, not a project requirement. Antigravity CLI was invoked for scoped review,
but returned no usable final review; it is not an acceptance source.
