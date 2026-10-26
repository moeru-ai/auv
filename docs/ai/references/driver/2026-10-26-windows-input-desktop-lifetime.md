# Windows input desktop handle lifetime

PR: [#425](https://github.com/moeru-ai/auv/pull/425).
Evidence level: deterministic lifecycle tests and compilation.

`DesktopState` owns handles from `OpenInputDesktop`.
The original handle from `GetThreadDesktop` remains borrowed.
Cleanup restores the original desktop before it closes the active handle.
Failed closes remain in a separate pending list, including failures without an active binding.
Pending failures stop new probes until a retry succeeds.

After a successful restore, the binding ends regardless of the close result.
A later attachment therefore records its own original desktop.
The mock API rejects closes of active, borrowed, or previously closed handles.
The native destructor belongs to the TLS owner, so mock state never calls Win32 during destruction.

## API boundary

`ensure_input_desktop()` keeps its original `fn() -> ()` signature and best-effort behavior.
The new `desktop::try_ensure_input_desktop()` returns `Result<(), DesktopError>`.
Window enumeration and D3D initialization return attachment failures through the existing driver error.
The WGC health worker stops on attachment failure.
The existing worker guard records its exit and wakes waiters.

CLI syntax and Protobuf messages do not change.
Runner callers can receive an attachment error instead of data from the wrong desktop.

## Cleanup limits

Windows can refuse restoration after a thread creates windows or hooks.
The state retains its binding after this failure.
At TLS destruction, a diagnostic reports any remaining failure.
The process retains unresolved handles until process exit.
This behavior does not establish unconditional leak freedom.

A worker or reaper lifetime requires a separate approved slice.
The inline `NOTICE:` records this boundary.
See the [SetThreadDesktop restrictions](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setthreaddesktop).

## Evidence

Three new regressions failed at head `7d424e60` before the repair.
The repaired cases cover pending probes, intermediate closes, and a new original desktop after restoration.
A repeated-cycle case covers 100 owned handles and rejects duplicate closes.

- `cargo test --locked -p auv-driver-windows`: passed, including 184 library tests. Nine library tests remained ignored.
- `cargo check --locked`: passed on Windows.
- `cargo check --locked -p auv-driver-windows --lib --target x86_64-unknown-linux-gnu`: passed.
- `cargo test --locked`: passed, including 157 CLI tests. One CLI test remained ignored.
- `cargo fmt --check` and `git diff --check`: passed.

The isolated desktop smoke remains ignored.
These results do not establish behavior during a real secure-desktop transition.
