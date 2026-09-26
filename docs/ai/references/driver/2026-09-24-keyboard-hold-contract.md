# Keyboard hold contract

Date: 2026-09-24; revalidated 2026-09-25. Classification: approved feature, with
test-only receiver follow-ups. Status: same-recipient held modifier composition
now passes macOS receiver checks; background menu and emoji limitations remain.
The [background diagnosis](#background-failure-diagnosis-2026-09-25) identifies
separate command-target and IME-focus barriers; it does not close either limitation.
The [2026-09-26 no-raise experiment](2026-09-26-no-raise-keyboard-probe.md) establishes
a test-only positive sequence with key-window records and explicit restoration;
that sequence has not been integrated into production input delivery.

`PressKeys` remains a complete down/up combination. `HoldKeys` owns a bounded
down/wait/up operation. `KeyDown` returns an opaque hold ID and a down
`InputActionResult`; `KeyUp` releases that ID and returns release evidence.
Both new operations use the existing `InputTarget`, `InputPolicy`, key-name
validation, and `InputActionResult`. An unspecified policy selects foreground
for these new RPCs. The Runner requires a positive duration or timeout no
greater than 30 seconds. Rust callers use `KeyboardHold.release()` to observe
release errors; dropping the handle attempts cleanup.

Rust driver call:

```rust
let input = session.input();
let target = InputTarget::Foreground;
input.hold_keys(&target, vec!["space".into()], InputPolicy::ForegroundPreferred, Duration::from_millis(800))?;
let mut held = input.key_down(&target, vec!["shift".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(5))?;
// Perform related input while the key is down.
held.release()?;
```

The JS SDK exposes the same Runner operations as protobuf-shaped requests:

```ts
const input = auv.runner({ runnerClass: 'auv.core.local', runId: run.id }).input
const target = { recipient: { case: 'foreground' as const, value: true } }
await input.holdKeys({ target, keys: ['space'], duration: { seconds: 0n, nanos: 800_000_000 } })
const { holdId } = await input.keyDown({ target, keys: ['shift'], timeout: { seconds: 5n } })
try {
  // Perform related input while the key is down.
}
finally {
  await input.keyUp({ holdId })
}
```

The coordinator reserves one combination per local driver process. It records
ownership before native posting, releases all keys in reverse order after a
failed down, and retains an uncertain hold after a failed up for retry by ID.
Timeout, cancellation during a blocking hold, and Runner shutdown attempt the
same release. A later `KeyUp` for the most recently known released ID returns
Noop. Separate processes and physical input are outside this admission scope.
`HoldKeys` gives its own release a one-second margin before the coordinator's
fallback deadline, so its normal result describes the release transition.

macOS keeps the original pid/window routing and modifier flags through each
transition. Linux supports foreground Portal and uinput routes with the
original input session retained; target-bound keyboard input is still
unsupported there. Windows uses foreground `SendInput`; target-bound input
remains unsupported. No platform claims receiver acceptance or key auto-repeat
from a successful submission.

Validation so far: shared coordinator tests cover reverse release, retry after
failed release, cancellation, and deadline cleanup; macOS unit tests record the
native transition sequence and modifier flags. The full default Cargo suite,
macOS SwiftPM build, Windows target check, Linux container check, Buf lint and
breaking check, Runner RPC validation, and JS request serialization passed.
The SDK typecheck passed on the updated PR worktree. The following macOS receiver tests add bounded behavior evidence;
other platforms and live Runner RPC lifecycle behavior still need native validation.

## macOS receiver validation

This section records the 2026-09-24 implementation before the modifier fix.
See [2026-09-25 revalidation](#modifier-composition-revalidation-2026-09-25) for
the current result.

The owner requested a follow-up test of the new implementation. The independent
[Chrome/Electron fixture](../../../../evals/auv-base/platforms/desktop-macos/tasks/keyboard.rs)
now accepts `--sender`, selecting a persistent
[Rust sender](../../../../evals/auv-base/platforms/desktop-macos/tasks/keyboard_sender.rs).
It calls the real library APIs without injecting DOM events. The process remains
alive between commands, so a `KeyboardHold` can span separate requests to the
fixture. This is a library test, not a Runner RPC test. The production input
implementation was not changed by this validation follow-up.

Environment: macOS 26.3 (25D2125), arm64; Chrome 153.0.8010.53; Electron 44.4.4
(Chromium 152.0.7977.130). Receivers use isolated profiles and the ABC input source,
which is restored afterward. The owner confirmed desktop use during preliminary
runs; those samples were excluded. All final trials passed their input-source and
foreground preconditions and the before/after foreground checks.

Each result below asserts text, exact input/base-key event counts, trusted events,
foreground state, and `verified:false` submission results. Timed/release cases also
assert a down-to-up receipt interval of 150–1500 ms for a requested 200 ms hold.
Separate down/up and Drop tests check an intermediate receipt containing down but
no up; timeout tests check that up arrived before the later explicit release.
These checks demonstrate a held interval and cleanup, not just eventual text.

| Case | Chrome background | Electron background | Chrome foreground | Electron foreground |
| --- | --- | --- | --- | --- |
| `hold_keys([b], 200 ms)` | 5/5 | 5/5 | 2/2 | 2/2 |
| `hold_keys([shift, b], 200 ms)` | 5/5 | 5/5 | 2/2 | 2/2 |
| Separate down / wait / release | 5/5 | 5/5 | 2/2 | 2/2 |
| 200 ms timeout / later release | 5/5 | 5/5 | 2/2 | 2/2 |
| Down / wait / Drop | 5/5 | 5/5 | 2/2 | 2/2 |
| Hold Shift / ordinary `b` / release / `c` | **0/5** | **0/5** | **0/2** | **0/2** |
| Hold Cmd+A / release / type `Z` | **0/5** | **0/5** | 2/2 | 2/2 |
| Ordinary `b`, persistent sender | 5/5 | 5/5 | 2/2 | 2/2 |
| `A猫`, persistent sender | 5/5 | 5/5 | 2/2 | 2/2 |
| `😀`, persistent sender | **0/5** | **0/5** | 2/2 | 2/2 |

These are locally observed results. Raw receiver receipts, native event logs,
and build metadata remain local and are excluded from the PR. The reproduction
commands below generate fresh validation output.

### Modifier composition failure before the fix

Every held-Shift composition trial produced `bc`, not `Bc`. The DOM trace shows:

```text
keydown Shift  shift=true
keydown b      shift=false
keyup b        shift=false
keyup Shift    shift=false
keydown c      shift=false
```

The hold itself reaches the receiver, but the subsequent ordinary press loses the
modifier. `Keyboard.swift::pressKeys` starts with empty `CGEventFlags` and derives
them solely from that request's key list. It does not consume the coordinator's
held modifier state. Sending Shift and B together through `hold_keys` works, which
separates this failure from timed down/up delivery.

This finding required ordinary input to consume held state for the same recipient,
or explicitly reject unsupported interleaving. An ordinary press must preserve
modifiers owned by an existing hold until that hold releases them. The 2026-09-25
implementation adds that state connection; increasing hold duration was not the fix.

### Remaining BG-2 boundaries

Holding Cmd+A for 200 ms still failed to replace the selection in background:
both receivers retained `replace meZ`. Foreground controls produced `Z`. Background
emoji requests produced a key event but no text insertion; foreground controls
inserted the emoji. These are still open compatibility issues. The persistent
sender's ordinary `b` and `A猫` observations are useful controls for the short-lived
CLI failures, but do not prove that process exit is the sole cause.

The rebuilt CLI was also rerun through the original nine-case background matrix:
Chrome passed **33/45**, Electron **30/45**, with no invalid preconditions or focus
failures. Emoji and select-all replacement stayed at 0/5 in both. Return and the
two-request Shift+B/`c` sequence still lost events intermittently; Electron also
lost some Shift+B and Left inputs. This implementation leaves `input.keys` on its
existing complete-press path, so adding the hold API does not fix those losses.
These small samples do not establish an improvement over the previous 30/45 and
26/45 run.

No live auto-repeat, crash recovery, Linux/Windows receiver behavior, or RPC
shutdown guarantee is established by these samples. The coordinator cancellation
test remains unit-level evidence. Five trials per background case are bounded
observations on these versions, not a universal support claim.

### Reproduction and checks

```sh
just eval build
just eval hold --electron "$electron_binary" \
  --output "$probe_root/hold-background" --repetitions 5
just eval hold --electron "$electron_binary" \
  --output "$probe_root/hold-foreground" --repetitions 2 --mode foreground
```

Use new output directories and the isolated Electron installation described in
the [authentication receiver report](2026-09-23-background-keyboard-authentication.md).
The matrices intentionally exit 1 while the listed behavior failures remain.
The follow-up also passed native SwiftPM build, `cargo test -p auv-driver-common
-p auv-driver-macos --lib` (161 passed, 5 ignored), and `cargo test -p auv-cli --lib
keyboard` (7 passed, 1 ignored). Fixture syntax and formatting checks passed.

## Modifier composition revalidation (2026-09-25)

The implementation now stores held modifier flags by native recipient and combines
them with each ordinary press's local flags. A native lock serializes state updates
with posting. Held-key release removes the flags when none remain. Unicode text
composition with held modifiers remains explicitly deferred at its native call site.

The owner requested another live check. The sender binary and generated Swift
bindings were rebuilt, and the SwiftPM build passed. The same Chrome/Electron
versions, macOS host, ABC input source, and independent DOM/native event receivers
were used. This follow-up only changes test fixtures and evidence, not production
delivery code. The fixture adds three cleanup/nesting cases and verifies Shift on
both base-key down and up events, including the unmodified key after release.

The full background run attempted five trials per case per application. Two
samples had a change between unrelated foreground applications: Chrome's fifth
ordinary `b` control and Electron's fifth timeout-Shift cleanup case. Their text
and key assertions passed, but they are retained as **invalid**, not passing
focus tests or driver failures. The table shows the original run without replacing
those records. `I` denotes one invalid trial within the denominator.

| Case | Chrome background | Electron background | Foreground, each application |
| --- | --- | --- | --- |
| Five original timed/down/up/timeout/Drop cases | 25/25 | 25/25 | 10/10 |
| Hold Shift / ordinary `b` / release / `c` -> `Bc` | **5/5** | **5/5** | **2/2** |
| Hold Shift / press Shift+B / press `c` / release / `d` -> `BCd` | **5/5** | **5/5** | **2/2** |
| Shift timeout / ordinary `b` -> `b` | 5/5 | 4/5 + I | 2/2 |
| Drop held Shift / ordinary `b` -> `b` | 5/5 | 5/5 | 2/2 |
| Ordinary `b`, persistent sender | 4/5 + I | 5/5 | 2/2 |
| `A猫`, persistent sender | 5/5 | 5/5 | 2/2 |
| Hold Cmd+A / release / type `Z` | 0/5 | 0/5 | 2/2 |
| `😀`, persistent sender | 0/5 | 0/5 | 2/2 |
| Total | 54/65 + I | 54/65 + I | **26/26** |

A separate focused background run repeated `timeout_shift_then_b` and
`persistent_press_b` once in each application: **2/2 per application**, with no
invalid samples. These additional receipts confirm both interrupted case types;
they do not replace the invalid records in the table. Reproduce that bounded
check with the sender command above
plus `--case timeout_shift_then_b --case persistent_press_b --repetitions 1`.

The regression now records this sequence in both applications:

```text
keydown Shift  shift=true
keydown B      shift=true
keyup B        shift=true
keyup Shift    shift=false
keydown c      shift=false
keyup c        shift=false
```

The missing modifier inheritance is fixed in these bounded library receiver tests.
Nested ordinary presses preserve the outer hold, and explicit release, timeout,
and Drop clear it for later input. Foreground controls pass all 26 trials in each
application. Background select-all and emoji insertion still fail all five trials
in each application, so this does not close BG-2.

All driver results remain `verified:false`; verification comes from the
separate receiver. Raw validation output is retained locally.

Use the same reproduction commands above with new output directories. The fixture
now supports repeated `--case` arguments for focused reproduction and reports
invalid samples separately. The shared/macOS Rust library suite passed again
(161 passed, 5 ignored); Rust formatting and diff checks passed. No new claim is made
about the short-lived CLI path, other platforms, or live Runner RPC behavior.

## Background failure diagnosis (2026-09-25)

Classification: test-only investigation and documentation. Production delivery
was unchanged. The original two-case library reproduction still fails: Cmd+A
then `Z` gives `replace meZ`, and emoji gives an empty string. Both applications
failed both select-all trials; Electron failed both emoji trials. Chrome failed
one valid emoji trial and had one invalid trial due to a changed input source.

### Cmd+A reaches command dispatch, but no target is resolved

An opt-in native observer inside the isolated Electron receiver recorded:

```text
sendAction=selectAll: target=(null) keyWindow=(null) mainWindow=(null) active=0 result=0
```

The event reaches an edit action, but AppKit's targetless action dispatch fails
while the application has neither a key nor a main window. This explains why the
following `Z` is inserted at the existing caret. Foreground controls record the
same action with a key/main window and `result=1`, and produce `Z`.

A causal receiver-only intervention retried that failed action with the fixture's
`RenderWidgetHostViewCocoa` as the explicit target. Replacement then succeeded
in both trials without changing the OS foreground process or document focus;
emoji still failed. This intervention modifies the receiver, not AUV, and is not
a production fix. Removing the fixture's application menu did not fix dispatch.

Chromium's [key-equivalent handler at Electron's Chromium version](https://github.com/chromium/chromium/blob/152.0.7977.130/content/app_shim_remote_cocoa/render_widget_host_view_cocoa.mm#L1344)
requires the first responder of a key window before forwarding command-key
equivalents. Direct native instrumentation established the failure in Electron;
Chrome's matching behavior and shared Chromium path support the same explanation,
but this investigation did not instrument Chrome's native action dispatch.

### Multi-unit Unicode reaches insertText, then takes the IME path

The native receiver logged the complete `😀` in `insertText:replacementRange:`,
with UTF-16 length 2. Thus the observed failure is downstream of Unicode event
construction, authentication, and native text interpretation. The earlier
possibility that this payload was simply ignored by the framework is narrowed
by this receipt.

The failure applies to more than emoji. A non-emoji supplementary character
`𐐀` and the decomposed grapheme `e\u0301` also fail. In a temporary native sender
probe, sending `AB` in one Unicode event fails, while the production-style
separate `A` and `B` events succeed. Similarly, `A猫` succeeds when sent separately
and fails as a single event. This separates UTF-16 payload length from an
emoji-specific encoding issue.

The matching Chromium source explains the boundary:

- [The Cocoa view](https://github.com/chromium/chromium/blob/152.0.7977.130/content/app_shim_remote_cocoa/render_widget_host_view_cocoa.mm#L1625)
  sends a single UTF-16 unit as a character event and longer text through
  `ImeCommitText`.
- [WidgetBase::ImeCommitText](https://github.com/chromium/chromium/blob/152.0.7977.130/third_party/blink/renderer/platform/widget/widget_base.cc#L1682)
  rejects untargeted commits when `ShouldHandleImeEvents()` is false.
- [WebFrameWidgetImpl::ShouldHandleImeEvents](https://github.com/chromium/chromium/blob/152.0.7977.130/third_party/blink/renderer/core/frame/web_frame_widget_impl.cc#L4366)
  requires widget focus for a main-frame widget. A DOM active textarea alone
  does not satisfy this condition.

A second receiver-only intervention made the view report a key window only
during its internal `windowDidBecomeKey:` notification, then immediately restored
the real getter. This propagates Chromium's internal input-focus state while
leaving the OS application inactive and its real window non-key. Emoji and
`e\u0301` then both inserted in 2/2 trials; select-all still failed in 2/2.
The OS foreground process stayed unchanged. The normal fixture correctly marks
these samples **invalid for background support**, because the intervention makes
`document.hasFocus()` true. They establish a causal diagnostic result, not a
green background-input regression test.

Merely enabling DevTools page-focus emulation did not fix the text insertion.
Sending the window notification without satisfying its key-window guard also
did not fix it. These negative controls distinguish the widget input-focus
transition from a changed DOM focus report.

### Evidence and remaining work

Adding 100 ms between native posts and a 400 ms wait before replacement did not
fix either failure in valid Electron trials. Chrome's delay-run samples were
invalid because the input source changed and are excluded. A private event source
also left both failures unchanged in both receivers. Foreground controls passed
all eight select-all, emoji, BMP, and combining-text samples in Electron.

Diagnostic receipts, native observations, temporary intervention source, and
build metadata remain local. No production receiver instrumentation was added.
Existing failing receiver cases remain the regression seam for an eventual fix.

NOTICE: Resolving these barriers requires a separately validated command-target
and text-commit path. Do not silently activate a target under `BackgroundOnly`,
split UTF-16 surrogate pairs into invalid characters, or ship receiver runtime
patches as a generic keyboard fix. Candidate next slice: evaluate a typed,
target-aware command/text route with separate semantic verification; acceptance
requires the original unmodified background receivers to pass.
