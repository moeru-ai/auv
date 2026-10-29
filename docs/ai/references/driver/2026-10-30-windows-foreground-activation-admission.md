# Windows foreground activation admission

Status: all three native regressions reproduced before repair. Scoped repair passed isolated Windows acceptance.

Classification: bug fix from the owner's Windows Phase 2 input-delivery goal.
Base: `main@d9d2bdcc6a27da0c1a70a17c4b265c56cbd0f7f3`.
The [#429 closure decision](https://github.com/moeru-ai/auv/pull/429#issuecomment-6481267529)
explicitly identifies activation-failure refusal as useful independently of the rejected screenshot predicate.
This work does not depend on #429's capture store, RPC, pixels, or input precondition.

## Reproduction and intended behavior

Before the repair, `WindowApi` foreground click, scroll, and drag recorded an activation failure as an attempt, then continued with global input.
Global `SendInput` has no window recipient.
A different window can therefore receive input aimed at a target that could not be activated.

Three ignored native regressions use the public Driver APIs and the existing foreground button receiver.
Each creates and destroys an owned target window whose old rectangle overlaps the still-live receiver.
The receiver must hold the foreground before the measured operation.
The record retains the Driver result, received mouse-button/wheel messages, pointer positions, and final foreground check.
Each operation must fail, deliver no messages to the receiver, and leave the pointer and foreground unchanged.
The fixture attempts pointer/foreground restoration and owned-window cleanup during teardown, including on an assertion failure.
The tests run only when explicitly selected on an isolated interactive desktop.
Local validation compiles them without execution.

The repair returns the original activation error before global click, wheel, or drag delivery.
Successful activation retains the existing `InputAttempt` and delivery result.
Posted-message delivery remains unchanged.
For scrolling, every selected `ForegroundHid` candidate requires successful activation, including an explicit candidate under `BackgroundPreferred`.
No public method, command, RPC, SDK, result schema, or new retry is needed.

## Boundaries

This is an activation-error boundary, not a cross-call input lease or a screenshot predicate.
Successful activation does not prevent another process from taking foreground afterward.
The common `InputPreparationLease` currently holds only a restored flag; it does not provide an exact Windows recipient guard.
The macOS preparation path also refuses an unimplemented focus guard.
Those placeholders cannot establish an existing lease implementation to copy.
Recipient identity, continuous focus-loss admission, and replacement state subscriptions need their own contract and evidence.

Activation can itself change presentation before it returns an error; this repair does not promise activation rollback.
The closed-target regression measures the separate claim that no global input follows the activation failure.

## Native reproduction

The [test-only native run](https://github.com/moeru-ai/auv/actions/runs/40920335771)
used source `e1010ab3bac156819b366c8e23b68fbb5444e19e` and workflow `d24bf7a12029ae294662eeec4328bc37e7aac7d1`.
All three calls returned successful foreground-delivery results after recording failed activation attempts.
The independent receiver confirmed the following events:

| Operation | Receiver messages | Shared pointer before / after |
| --- | --- | --- |
| Click | `WM_LBUTTONDOWN`, `WM_LBUTTONUP` | `(512, 384)` / `(250, 250)` |
| Scroll | `WM_MOUSEWHEEL` | `(512, 384)` / `(250, 250)` |
| Drag | `WM_LBUTTONDOWN`, `WM_LBUTTONUP` | `(512, 384)` / `(250, 250)` |

Each named regression failed its required-refusal assertion.
The workflow required all three exact failures and successful Driver results; setup errors cannot count as reproduction.
The [raw artifact](https://github.com/moeru-ai/auv/actions/runs/40920335771/artifacts/12917647940)
matched SHA-256 `c3ea698398e7536b7932cd93c6d8bd55027805982150bf28aadf7880a18bf09f` after download.
Retention ends 2026-11-12.

## Repair acceptance

The [repaired native run](https://github.com/moeru-ai/auv/actions/runs/40921807996)
used source `8656b95aa42ee38f91d02b29eb67f9d92bf2bc3b` and workflow `874c111f6ece18196b66d014d69fbe68bc93d3c9`.
The three negative regressions are unchanged from the test-only reproduction; the later test change adds a positive control.

| Check | Result and evidence boundary |
| --- | --- |
| Closed-target click, scroll, drag | All returned the original `Backend` activation error, with no receiver messages. Pointer stayed at `(512, 384)` and the receiver retained foreground. |
| Valid-window click, scroll, drag | All reached the owned receiver through the public WindowApi routes, with their expected button/wheel messages. Results retained two successful attempts and `verified: false`. |
| Existing global-input control | All 18 button/count/modifier cases passed their exact message and flag assertions. |
| Complete native test binary | Five tests passed, with none failed or ignored in this explicit run. |
| Local validation | Affected Driver and workspace all-target checks, native-test compilation without execution, formatting, and whitespace checks passed. |

Both native runs recorded Windows Server 2025 build 26100, hosted image `20261021.278.1`, interactive session 2, and an accessible input desktop.
Each had one 1024 by 768 display; the owned per-monitor-v2 DPI probe reported 96 DPI and restored its own context.
Independent inspection of the downloaded repair logs confirmed all three empty receiver receipts, unchanged pointer/foreground checks, three valid-window receipts, and 18 existing control receipts.
The [repair artifact](https://github.com/moeru-ai/auv/actions/runs/40921807996/artifacts/12919061447)
matched SHA-256 `7d381a995b6c57b523da0610256f1c992bd86402db778f341e0cb9918384ad37` after download.
Retention ends 2026-11-12.

The receipt scope is the operation interval. There is no dedicated full-desktop teardown receipt in this test binary.
Standard PR CI is separate from the isolated run.
This evidence does not complete the input-lease contract, general toolkit compatibility, the DPI coordinate matrix, or live MCP parity.
