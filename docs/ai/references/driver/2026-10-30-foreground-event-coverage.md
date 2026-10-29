# Windows foreground event coverage

Status: diagnostic implementation; native evidence pending. This does not accept an input lease or replace the closed #429 design.

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

## Platform references

- [SetWinEventHook](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwineventhook): out-of-context callbacks are asynchronous and require a message loop.
- [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput): delivery uses the system input stream and has no HWND argument.
