# Windows keyboard recipient protection

Status: approved feature under implementation. Win32 text delivery has native evidence. Expanded acceptance remains pending.

## Existing contract and scope

Use the existing `InputTarget::Window` and `InputPolicy::ForegroundPreferred` batch API. Preserve explicit global `InputTarget::Foreground` behavior. Application identifiers, posted keyboard messages, and window-targeted holds remain unsupported until their existing contract gaps are resolved.

Validate the complete request before IO. A dry run validates request shape without activating a window or installing hooks. Actual delivery requires the window's observed PID, a live matching owner/thread/class, visibility, and a non-minimized, non-cloaked window.

Require the caller's window to be foreground at admission, then retain a guard for the complete batch. Revalidate before every native `SendInput` submission, including replacement shortcuts, Unicode fragments, paste shortcuts, and submit keys. Detected foreign or ambiguous foreground events and root-window destruction invalidate the guard permanently. Returning to the original foreground cannot revive a lost guard. No action is retried and no loss causes automatic reactivation.

The guard observes a window recipient, not a text control. The caller establishes the first responder. Application state remains the semantic verifier. A foreground check and `SendInput` are separate OS operations; this feature cannot claim atomic recipient binding or replace the closed #429 capture predicate.

## Failure and lifecycle

Reuse `KeyboardInputError` for completed actions/repetitions and `InputActionProgress` for native submission facts. A failed action with accepted native input must not become `not_delivered`. Preserve uncertainty and stop; never undo application text or replay the action automatically.

Introduce `target_lost` for an admitted recipient that loses its identity, availability, or foreground validity. Keep that cause through Invoke, Runner rich errors, and tracing. Caller cancellation must stop later submissions while the blocking worker retains its reservation until it returns.

Window admission failures carry explicit `not_delivered` progress. Accepted native input can be only a prefix of one text, paste, or repeated-press action. Batch counts and native progress therefore remain separate.

Runner packs `InputActionFailure` and `KeyboardInputProgress` as distinct `Any` details inside one `google.rpc.Status`. `InputActionFailure.reason = TARGET_LOST` requires `FailedPrecondition`. A known reason does not require known delivery. Rust exposes `ClientErrorKind::TargetLost`; Invoke emits `target_lost`. JS exposes `inputFailureReason(error)` and `keyboardInputProgress(error)` alongside `inputActionProgress(error)`.

NOTICE: This changes the experimental keyboard error framing from raw `KeyboardInputProgress` bytes to standard rich-error details. Upgrade Runner and SDK together. Readers leave legacy raw details unknown and retain the original RPC error. They do not infer zero delivery.

Windows Runner delivery uses the existing blocking-input cancellation boundary. macOS and Linux keep their delivery-thread contracts. Unrepresentable wait durations fail batch preflight before any earlier action can submit input.

Release only AUV-owned input state after a short native submission. Cleanup may need key releases after recipient loss; it must preserve the original failure receipt. Hook registration, message dispatch, unhooking, and desktop attachment belong to one owned watcher thread. Startup failure and shutdown must not detach that worker.

## Acceptance

The owned receiver acknowledges fault arming before input and changes foreground after the first key release. Independent application text/key logs, native window state, and the desktop witness determine the result.

- The explicit global control must demonstrate that unbound text can continue to another owned foreground receiver.
- A valid window batch types the complete text into its intended receiver.
- Text delivery stops after foreground loss, A-to-B-to-A, minimization, or window destruction. The alternate receiver must receive no text key messages.
- Repeated presses stop with an exact completed-press count. Combination cleanup releases only owned keys; releases may reach the new foreground.
- Paste restores clipboard text and stops the following action after takeover. Required modifier releases are separate from another key-down.
- Cancellation after application acknowledgement stops later characters. Pre-cancelled, foreign-foreground, and wrong-owner admission submit no input.
- Failure records preserve partial submission; all owned processes/windows exit and desktop restoration has separate receipts.

Native tests run only on the isolated hosted Windows desktop. Local validation compiles native fixtures and runs pure tests.

## Native evidence

All accepted runs used one isolated interactive display at 1024 by 768 and 96 DPI. The desktop OS was Windows build 26100, image `20261021.278.1`, session 2. Each artifact was downloaded and checked against its GitHub SHA-256 digest.

| Revision | Run and artifact | Result |
| --- | --- | --- |
| Baseline `beef622247b71b419ab7fdf5608b09d2282870e0` | [40941402936](https://github.com/moeru-ai/auv/actions/runs/40941402936), artifact `12925984296` | Global control produced A=`a`, B=`bc`; all five Window cases were Unsupported. This is an accepted feature baseline. |
| Producer `af374c5bce7a45112a56a8999387ec64fade7356` | [41087448481](https://github.com/moeru-ai/auv/actions/runs/41087448481), artifact `12992935468` | All six cases passed. A valid Window batch produced `abc`; four fault cases stopped after one submitted character. B received no keys in protected cases. |

Baseline artifact SHA-256: `6fb975001a8362e909e34ff301641212d75810f59a2f84d7e99d703e44a88231`.
Producer artifact SHA-256: `5e5de560323caee6fe03fbeae29ecae7644d5a6b6904a94743323c9eb86ee8ab`.

Both accepted runs recorded 12 successful receiver cleanups, no physical input, and restored foreground and pointer state. An earlier `02d1425a` rehearsal lacked B's typing first responder. It is not an accepted baseline. The fixture repaired that setup before the baseline above.

The expanded evaluator contains 15 cases for text, repeated presses, combinations, paste, cancellation, and admission rejection. Its current source has compilation and pure projection checks; native acceptance is pending. The earlier six-case result does not validate these added cases.

## Limits and follow-up gates

The admission tuple retains a live process handle, HWND, owner thread, and class during one batch. It does not identify same-owner HWND reuse before admission. A versioned identity producer/consumer contract remains deferred at the code site.

Serialization covers native submissions within this process. It does not provide exclusive ownership against other processes or physical input. The caller still owns control focus and semantic verification.

Microsoft documents that [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) inserts an input array into the global stream. Its signature has no window recipient. [Out-of-context WinEvent delivery](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) is asynchronous. A guard check and submission therefore cannot prove atomic recipient binding. This is an inference from those API boundaries, not a stronger Windows guarantee.

TODO(windows-keyboard-recipient): complete expanded native acceptance, selected-Runner cancellation, Invoke/MCP failure records, and cleanup fault injection before claiming those evidence levels. Real applications, 125/150% desktop DPI, window-targeted holds, and Application targets remain outside this Win32 slice. This work does not replace the retired #429 screenshot predicate.
