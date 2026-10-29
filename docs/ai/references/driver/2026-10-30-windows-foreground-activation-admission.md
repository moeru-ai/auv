# Windows foreground activation admission

Status: all three native regressions reproduced before repair. Repair is implemented; native acceptance is pending.

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
The fixture restores pointer and foreground state during teardown, including on an assertion failure.
The tests run only when explicitly selected on an isolated interactive desktop.
Local validation compiles them without execution.

The repair returns the original activation error before global click, wheel, or drag delivery.
Successful activation retains the existing `InputAttempt` and delivery result.
Background input policies remain unchanged.
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

## Acceptance still required

- Run the unchanged native regressions against the repair.
- Run the added positive control through all three public WindowApi routes.
- Run the existing foreground button/count/modifier receipt control on the same isolated desktop.
- Compile affected targets, check formatting and whitespace, and record exact source and artifact digests.

No native behavior claim follows from local compilation.
