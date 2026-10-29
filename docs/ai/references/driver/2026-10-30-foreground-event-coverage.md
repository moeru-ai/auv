# Windows foreground event coverage

Status: 60 native diagnostic cases recorded; ambiguous witness verdict corrected with a failing-then-passing unit regression. This does not accept an input lease or replace the closed #429 design.

## Purpose

The Windows input-recipient slice needs to stop when its target loses foreground. A direct foreground query can miss an A-to-B-to-A transition. An asynchronous event names a window, but the foreground can differ when the callback runs.

The existing desktop witness records both values. This probe adds controlled cases and independent receiver state to its records. It changes no Driver, Invoke, Runner, or MCP operation.

## Cases

The probe starts three detached Win32 receivers on an isolated interactive desktop:

- A keeps foreground during the stable control.
- The Driver activates B, a direct OS query confirms B, and the Driver immediately returns to A. The independent hook thread records the intervening events.
- A remains the intended foreground while one background-only click reaches C. C reports its button count, activation count, and whether it took foreground.

C starts after A becomes foreground. Its existing test-fixture behavior hands foreground back to the window present at startup. This keeps any takeover and return within the owned A/C pair. A takeover still appears in C's counters and in the event receipts.

The command records the before, middle, immediate, and collected desktop states. It also records callback-named windows, callback-time foreground, and application state. The event `process` field belongs to the callback-time foreground process.

```powershell
cargo build --locked -p auv-base-evals --bin win32-receiver --bin background-session-eval
target/debug/background-session-eval foreground-events --output <fresh-directory> --rounds 10
```

Run this command only on an isolated interactive test desktop. It activates owned windows. It stops further cases when physical input appears or an unrelated window holds foreground. No failed action is retried.

## Acceptance and limits

The probe checks the actual B state and return to A. Each background case must increment C's independent button count once. Global injected-input counters and the shared pointer must remain unchanged during each case. Cleanup records process exit and destruction of every owned HWND.

An event that names C without an application-reported takeover is a diagnostic finding. It does not prove that C was never foreground between reads. A callback that names B after foreground returned to A remains evidence of an event, with separate callback-time state.

Collection waits 150 ms for diagnostics. This is not an OS delivery deadline or a synchronization barrier. The witness has a bounded ring and millisecond receipt timestamps. It does not record the event's generation time. Counts cannot establish complete transition history or prove stability when no event appears.

`complete` means the requested diagnostic cases completed. Foreground and pointer restoration have separate receipt fields. No keyboard input, control-focus lease, click predicate, UIA identity guarantee, or third-party application support is claimed.

TODO(windows-input-target-lease): define a recipient and loss contract before production input consumes event history. These diagnostic receipts cannot provide atomic validation and global `SendInput` delivery.

## Native receipts

The [isolated run](https://github.com/moeru-ai/auv/actions/runs/40931339602) checked source `46b8aab607e5662f1413bb7ecfbb251b2551f820` with workflow `decf6837bef864d44f0406b1af44afa37a4a2386`.
The [artifact](https://github.com/moeru-ai/auv/actions/runs/40931339602/artifacts/12923502019) is `windows-foreground-events-46b8aab6`; ZIP SHA-256 is `db1f0c100bb67790b536446c04859d66be18fca5cab5e980040b93cecad64e6a`.

The host was Windows build 26100, image `20261021.278.1`, session 2, one 1024-by-768 display, and 96 DPI. Input-desktop access and an interactive session were checked before fixture creation.

| Case | Count | Recorded result |
| --- | ---: | --- |
| Stable A | 20 | Completed control spans |
| A-to-B-to-A | 20 | Direct middle queries confirmed B; all 20 histories named B |
| Background click to C | 20 | Independent C button count increased once per case |

Two background cases show why named and current foreground must stay separate:

- Round 0: C reported one takeover and handback. Callbacks directly read C and then A, although both surrounding snapshots read A.
- Round 1: C's activation and focus counts increased, but its takeover count did not. One callback named C while reading A. This does not prove that C was never foreground.

No controlled ABA case had only A in its callback-time queries. Delayed ABA is covered by a modelled unit regression, not claimed as a native reproduction.

Global injected-key, injected-mouse, physical-key, and physical-mouse counts stayed zero. All three process exits and HWND destructions were confirmed. The final pointer and foreground matched their original values. No input action was retried.

The [first rehearsal](https://github.com/moeru-ai/auv/actions/runs/40929771761) stopped after three cases because C handed foreground to its startup host. Its [artifact](https://github.com/moeru-ai/auv/actions/runs/40929771761/artifacts/12921089989) remains available with SHA-256 `414099bfc2de5f3c68988a1376700c12db694e01fd6dca18a02ccdd16575a076`. Starting C after A corrected the fixture handback target; the stop checks and takeover counters stayed intact.

## Witness verdict correction

The previous `Snapshot::judge` filtered events only by callback-time foreground. A named B event whose callback read A could therefore produce `Held`, including a modelled delayed A-to-B-to-A history.

The regression failed at test-only source `4375b5c440100d00e68083810b1d9fce2526687b`: actual `Held`, expected `Inconclusive`. Repaired source `b207fd8accaea3a38daa3aed056af7f790eca0c6` passes all 11 witness unit tests.

Named/current disagreement now records `inconclusive` with reason `foreground_event_ambiguous`. Direct evidence of an unexplained takeover or injected input still produces `violated`. Existing evaluation consumers already count inconclusive verdicts separately.

This changes evaluation JSON verdicts, not production Driver results. The native diagnostic reads snapshots and timelines without calling `judge`; its raw collection path is unchanged by the policy correction. The artifact proves the recorded cases; the unit tests prove the new verdict rule.

## Platform references

- [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook): out-of-context callbacks are asynchronous and require a message loop.
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput): delivery uses the system input stream and has no HWND argument.
