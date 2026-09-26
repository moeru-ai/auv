# Keyboard hold contract

Date: 2026-09-24. Classification: approved feature. Status: implemented for
review; receiver validation is bounded to the host and applications below.

`PressKeys` remains a completed down/up combination. `HoldKeys` owns a bounded
down/wait/up operation. `KeyDown` returns an opaque hold ID and a down
`InputActionResult`; `KeyUp` releases that ID and reports the release attempt.
The new operations reuse `InputTarget`, `InputPolicy`, key-name validation, and
`InputActionResult`. The Runner requires a positive duration or timeout no
greater than 30 seconds. An unspecified policy selects foreground for these
new RPCs.

Partial Rust call (`session` is a local driver session; imports omitted):

```rust
let input = session.input();
let target = InputTarget::Foreground;
input.hold_keys(&target, vec!["space".into()], InputPolicy::ForegroundPreferred, Duration::from_millis(800))?;
let mut held = input.key_down(&target, vec!["shift".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(5))?;
// Send related input while the key is down.
held.release()?;
```

The JS SDK exposes the Runner operations as protobuf-shaped requests:

```ts
const input = auv.runner({ runnerClass: 'auv.core.local', runId: run.id }).input
const target = { recipient: { case: 'foreground' as const, value: true } }
await input.holdKeys({ target, keys: ['space'], duration: { seconds: 0n, nanos: 800_000_000 } })
const { holdId } = await input.keyDown({ target, keys: ['shift'], timeout: { seconds: 5n } })
try {
  // Send related input to the same recipient.
}
finally {
  await input.keyUp({ holdId })
}
```

## Ownership and release

The process-wide coordinator admits one held combination at a time. It records
ownership before native posting, releases keys in reverse order after a failed
down, and retains an uncertain hold after a failed up so the caller can retry
the same ID. A later `KeyUp` for the most recently released ID returns `Noop`.
Separate processes and physical input are outside this admission scope.

Rust callers retain a `KeyboardHold` guard. Explicit `release()` reports errors;
its `Drop` makes a best-effort release. A Runner RPC transfers the hold ID out
of that guard so the key can remain down across calls. Runner teardown calls
coordinator `shutdown()` to attempt release. The coordinator is held by a
process-wide static and cannot use its own `Drop` for process-exit cleanup.
Timeout cleanup runs only while the process remains alive. A failed release is
still uncertain, rather than a guarantee that no key remains down.

`HoldKeys` leaves its own release a one-second margin before the coordinator's
fallback deadline, so its normal result describes that release transition.
Cancellation during a blocking hold attempts the same release.

## Platform delivery

macOS preserves the resolved pid/window and modifier flags through each held
transition. A separate ordinary `PressKeys` call to the same recipient inherits
held modifier flags; unrelated recipients do not. Linux supports foreground
Portal and uinput routes with the original input session retained. Windows
uses foreground `SendInput`. Target-bound keyboard input remains unsupported
on Linux and Windows in this slice.

All driver results report input delivery, not semantic success. In particular,
successful posting does not establish key auto-repeat, text editing, or menu
command acceptance by the receiving application.

## Validation boundary

Coordinator tests cover reverse release, failed-release retry, cancellation,
deadline cleanup, and modifier ownership. macOS unit tests record native
transition order and flags. The native authentication test checks preparation
and one submission without sending input to an unrelated application.
The default Cargo suite, focused driver tests, SwiftPM build, Windows target
check, Buf lint/breaking check, Runner validation, and SDK serialization tests
passed on the PR worktree.

Earlier local macOS 26.3 receiver probes with Chrome 153 and Electron 44
observed five timed/down/up/timeout/Drop cases passing 25/25 per application
and held Shift plus a separate `b` passing 5/5 after the modifier fix.
Background Cmd+A replacement and emoji insertion remained 0/5 per
application. These are bounded historical observations, not platform support
claims. Raw receipts and temporary receiver code are not part of this PR;
the separate evaluation PR owns the reproducible GUI harness.

Linux/Windows live receiver behavior, key auto-repeat, process-crash recovery,
and live Runner RPC shutdown remain unverified. A successful submission is not
a substitute for a separate receiver result.
