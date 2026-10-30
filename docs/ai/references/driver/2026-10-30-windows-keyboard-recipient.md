# Windows keyboard recipient protection

Status: approved feature under implementation; native acceptance pending.

## Existing contract and scope

Use the existing `InputTarget::Window` and `InputPolicy::ForegroundPreferred` batch API. Preserve explicit global `InputTarget::Foreground` behavior. Application identifiers, posted keyboard messages, and window-targeted holds remain unsupported until their existing contract gaps are resolved.

Validate the complete request before IO. A dry run validates request shape without activating a window or installing hooks. Actual delivery requires the window's observed PID, a live matching owner/thread/class, visibility, and a non-minimized, non-cloaked window.

Prepare foreground once, then retain a guard for the complete batch. Revalidate before every native `SendInput` submission, including replacement shortcuts, Unicode fragments, paste shortcuts, and submit keys. Foreign or ambiguous foreground events and root-window destruction invalidate the guard permanently. Returning to the original foreground cannot revive it. No action is retried and no loss causes automatic reactivation.

The guard observes a window recipient, not a text control. The caller establishes the first responder. Application state remains the semantic verifier. A foreground check and `SendInput` are separate OS operations; this feature cannot claim atomic recipient binding or replace the closed #429 capture predicate.

## Failure and lifecycle

Reuse `KeyboardInputError` for completed actions/repetitions and `InputActionProgress` for native submission facts. A failed action with accepted native input must not become `not_delivered`. Preserve uncertainty and stop; never undo application text or replay the action automatically.

Introduce `target_lost` for an admitted recipient that loses its identity, availability, or foreground validity. Keep that cause through Invoke, Runner rich errors, and tracing. Caller cancellation must stop later submissions while the blocking worker retains its reservation until it returns.

Release only AUV-owned input state after a short native submission. Cleanup may need key releases after recipient loss; it must preserve the original failure receipt. Hook registration, message dispatch, unhooking, and desktop attachment belong to one owned watcher thread. Startup failure and shutdown must not detach that worker.

## Acceptance

The owned receiver acknowledges fault arming before input and changes foreground after the first key release. Independent application text/key logs, native window state, and the desktop witness determine the result.

- The explicit global control must demonstrate that unbound text can continue to another owned foreground receiver.
- A valid window batch types the complete text into its intended receiver.
- Foreground loss, A-to-B-to-A, minimization, and window destruction stop later submissions. The alternate receiver must receive no key messages.
- Failure records preserve partial submission; all owned processes/windows exit and desktop restoration has separate receipts.

Native tests run only on the isolated hosted Windows desktop. Local validation compiles native fixtures and runs pure tests.

TODO(windows-keyboard-recipient): actual native acceptance, error projections, and the complete guarded producer remain pending. This note records implementation requirements, not completed support.
