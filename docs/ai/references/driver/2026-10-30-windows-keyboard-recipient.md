# Windows keyboard recipient protection

Status: approved feature under review. Fifteen direct Win32 cases have native evidence. Live frontend parity remains pending.

## Existing contract and scope

Use the existing `InputTarget::Window` and `InputPolicy::ForegroundPreferred` batch API. Preserve explicit global `InputTarget::Foreground` behavior. Application identifiers, posted keyboard messages, and window-targeted holds remain unsupported until their existing contract gaps are resolved.

Validate the complete request before IO. A dry run validates request shape without activating a window or installing hooks. Actual delivery requires the window's observed PID, a live matching owner/thread/class, visibility, and a non-minimized, non-cloaked window.

Prepare the explicit Window's foreground once before admission, then retain a guard for the complete batch. Revalidate before every native `SendInput` submission, including replacement shortcuts, Unicode fragments, paste shortcuts, and submit keys. Detected foreign or ambiguous foreground events and root-window destruction invalidate the guard permanently. Returning to the original foreground cannot revive a lost guard. No action is retried and no loss causes automatic reactivation.

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
- Cancellation after application acknowledgement stops later characters. Pre-cancelled and wrong-owner admission submit no input.
- Initial foreground preparation activates the explicitly chosen Window once. Later takeover stops delivery without reactivation.
- Failure records preserve partial submission; all owned processes/windows exit and desktop restoration has separate receipts.

Native tests run only on the isolated hosted Windows desktop. Local validation compiles native fixtures and runs pure tests.

## Native evidence

All accepted runs used one isolated interactive display at 1024 by 768 and 96 DPI. The desktop OS was Windows build 26100, image `20261021.278.1`, session 2. Each artifact was downloaded and checked against its GitHub SHA-256 digest.

| Revision | Run and artifact | Result |
| --- | --- | --- |
| Baseline `beef622247b71b419ab7fdf5608b09d2282870e0` | [40941402936](https://github.com/moeru-ai/auv/actions/runs/40941402936), artifact `12925984296` | Global control produced A=`a`, B=`bc`; all five Window cases were Unsupported. This is an accepted feature baseline. |
| Producer `af374c5bce7a45112a56a8999387ec64fade7356` | [41087448481](https://github.com/moeru-ai/auv/actions/runs/41087448481), artifact `12992935468` | All six cases passed. A valid Window batch produced `abc`; four fault cases stopped after one submitted character. B received no keys in protected cases. |
| Expanded source `264143c4f0e6f8afe76269ffa4cb1d6445c65357` | [41101183228](https://github.com/moeru-ai/auv/actions/runs/41101183228), artifact `12999121492` | All 15 cases passed. Text, repeated presses, combinations, paste, cancellation, wrong-owner rejection, and one initial activation have direct Win32 receipts. |

Baseline artifact SHA-256: `6fb975001a8362e909e34ff301641212d75810f59a2f84d7e99d703e44a88231`.
Producer artifact SHA-256: `5e5de560323caee6fe03fbeae29ecae7644d5a6b6904a94743323c9eb86ee8ab`.
Expanded artifact SHA-256: `0857064873c27e7e163ff6d87d6cf59c13ac051a0fd514b05068381931412a31`.

The first two accepted runs recorded 12 successful receiver cleanups each. The expanded run recorded 30. All recorded no physical input and restored foreground and pointer state. An earlier `02d1425a` rehearsal lacked B's typing first responder. It is not an accepted baseline. The fixture repaired that setup before the baseline above.

The expanded run checked exact injected-key counts. Repeated-press and combination takeover each retained one completed press and submitted native progress. Paste takeover restored clipboard text and stopped the following text action. Cancellation after A acknowledged `a` stopped later characters. Pre-cancelled and wrong-owner cases injected zero keys with explicit `not_delivered` progress. Initial preparation activated A once; later takeover cases left B foreground without reactivation.

B received no keys or text in all 14 protected cases. The global control alone produced B=`bc`. Clipboard text and modifier-state readbacks passed for every case. These checks do not establish restoration of every possible clipboard format or native short-insertion behavior.

The first expanded run at `bd219a46`, [41098663509](https://github.com/moeru-ai/auv/actions/runs/41098663509), passed 14 cases and failed one evaluator assertion. That assertion incorrectly required an initially foreign foreground to be refused. The existing contract and producer allow one initial activation before admission. Its records showed A=`abc`, B empty, and six injected keys. The corrected case tests that preparation separately and requires later takeover cases to leave B foreground. The native producer is unchanged by this correction.
Its artifact `12998200328` matched SHA-256 `bf3debf6af222baf96c137b86b0be958493af4959db175404c55b3018e7c7a2b`.

## Frontend acceptance

The evaluation command `window-keyboard-routes --auv <AUV_EXE> --output <FRESH_DIR>`
compares direct Driver, local Invoke, selected Runner, local MCP, and selected-Runner MCP.
It requires `AUV_ISOLATED_DESKTOP_EVAL=1` and runs only on an isolated desktop.
The command reuses the workflow's private Runner lifecycle and the locked rmcp client.
MCP admission checks the actual handshake, invoke tool, and a dry run without input.
It never substitutes CLI delivery for a failed MCP route.

Twelve cases cover success and takeover on each route, plus cancellation on both MCP routes.
Cancellation starts after the application acknowledges its first character.
The evaluator then checks for late input over 1,400 ms, beyond the remaining repeat intervals.
This finite check does not prove an unlimited quiet period or fabricate a native delivery receipt.
Application state, injected-key counts, desktop events, owned-window PNGs, and frontend stores remain separate channels.
The PNGs require independent visual review; capture success alone is not a text oracle.

The first [isolated frontend task](https://github.com/moeru-ai/auv/actions/runs/41109370775),
source `8cab8a76`, passed 11 of 12 cases.
Artifact `13003487852` matched SHA-256
`6e5d58a19a7d9cd1982cfd92dc3907af55fbc4017b513f865fc60a4fb32cae0b` after download.
The failed local-MCP cancellation case acknowledged `a`, then received `aaa`
and six injected key events. Its durable command record reported success.
Independent PNG review confirmed `aaa` in A and no text in B.
Selected-Runner MCP stopped at `a` and two key events.
All 24 receiver cleanups, both MCP exits, owned Runner cleanup,
and pointer/foreground restoration passed, with no physical input.

Windows local keyboard execution now uses the existing cancellable blocking-input boundary.
It retains the inner keyboard error so native progress and batch progress survive ordinary failures.
macOS/Linux keep their existing delivery-thread contracts.
A canceled request can lose its final wire reply; absent progress remains unknown.
The repair passed compilation and 288 unit tests.
The [replay at source `925e29f5`](https://github.com/moeru-ai/auv/actions/runs/41111375084)
passed all 12 frontend cases and the original 15 direct-driver cases.
Workflow `eeec28657ce901d43a171775b1af891549c676bf` checked out the exact source
and retained hashes for the CLI, evaluator, and receiver binaries.
Artifact `13004865694` matched SHA-256
`e89d081a0c40c0b1a4e470a0d9aaee96f3696efa9511b59ca7191ce2ad3da7e7` after download.

All five routes produced `aaa` on success and stopped at `a` after takeover.
Both MCP cancellation cases stopped at `a`, with two injected key events.
Independent review checked all 24 PNGs against application state and desktop events.
Four frontend stores preserved matching `target_lost`, `submitted`, and one completed press.
Each success artifact's body length and SHA-256 matched its durable metadata.
The two canceled requests retained `auv.frontend.cancelled`, without a final command result.
Those final delivery receipts remain unknown; their prior dry-run results are separate spans.

All 54 receiver cleanups, both normal MCP exits, and owned Runner cleanup passed.
Both matrices restored foreground and pointer state, with zero physical input and no input retries.
The host remained build 26100, image `20261021.278.1`, session 2,
one 1024 by 768 display, and 96 DPI.
This proves the stated Win32 keyboard cases, not a complete application workflow or another DPI level.

The previous Windows CI run failed in
[`abandoned_hold_releases_without_another_request`](https://github.com/moeru-ai/auv/actions/runs/41103991540/job/124228286676).
A controlled backend gate reproduced the test's invalid assumption: recording `up`
can precede completion of native release and reservation cleanup.
The repair changes test synchronization, not the mouse controller.
All 106 common-driver unit tests passed after that repair.

Five hook lifecycle models now cover first-registration failure, second-registration failure,
failed partial cleanup, failed normal cleanup, and successful shutdown.
They assert both removal attempts, registration-thread ownership, retained errors, and worker completion.
The private `Hooks` boundary changes no public API or WinEvent policy.
The tests use fake registrations and never install native callbacks.
Their Windows test target compiles; execution and the full native replay are pending.
These are fault models, not proof that a real Win32 API failure was forced.

## Limits and follow-up gates

The admission tuple retains a live process handle, HWND, owner thread, and class during one batch. It does not identify same-owner HWND reuse before admission. A versioned identity producer/consumer contract remains deferred at the code site.

Serialization covers native submissions within this process. It does not provide exclusive ownership against other processes or physical input. The caller still owns control focus and semantic verification.

Microsoft documents that [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) inserts an input array into the global stream. Its signature has no window recipient. [Out-of-context WinEvent delivery](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook) is asynchronous. A guard check and submission therefore cannot prove atomic recipient binding. This is an inference from those API boundaries, not a stronger Windows guarantee.

TODO(windows-keyboard-recipient): complete native hook startup and cleanup fault injection before claiming those evidence levels. Real applications, complete focus/edit/save workflows, 125/150% desktop DPI, window-targeted holds, and Application targets remain outside this Win32 slice. This work does not replace the retired #429 screenshot predicate.
