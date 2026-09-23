# Keyboard hold contract

Date: 2026-09-24. Classification: approved feature, with a test-only receiver
follow-up. Status: implementation prepared for review; the held-modifier composition
failure reproduced below was corrected and rerun against Chrome and Electron.

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
remains unsupported. The macOS receiver observations below are bounded to the
tested versions. Submission alone does not establish receiver acceptance or
key auto-repeat.

Validation so far: shared coordinator tests cover reverse release, retry after
failed release, cancellation, and deadline cleanup; macOS unit tests record the
native transition sequence and modifier flags. The full default Cargo suite,
macOS SwiftPM build, Windows target check, Linux container check, Buf lint and
breaking check, Runner RPC validation, and JS request serialization passed.
The SDK request tests and typecheck passed in the isolated PR worktree. The
following macOS receiver tests add bounded behavior evidence;
other platforms and live Runner RPC lifecycle behavior still need native validation.

## macOS receiver validation

The owner requested a follow-up test of the new implementation. The independent
[Chrome/Electron fixture](../../../../crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py)
now accepts `--sender`, selecting a persistent
[Rust sender](../../../../crates/auv-driver-macos/tests/fixtures/chromium_keyboard/sender.rs).
It calls the real library APIs without injecting DOM events. The process remains
alive between commands, so a `KeyboardHold` can span separate requests to the
fixture. This is a library test, not a Runner RPC test. The receiver fixture
itself does not modify the production input path.

Environment: macOS 26.3 (25D2125), arm64; Chrome 153.0.8010.53; Electron 44.4.4
(Chromium 152.0.7977.130). Receivers use isolated profiles and the ABC input source,
which is restored afterward. The owner confirmed desktop use during preliminary
runs; those samples were excluded. All final trials passed their input-source and
foreground preconditions and the before/after foreground checks.

The pre-fix results below assert text, exact input/base-key event counts, trusted events,
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

Evidence: [background summary](evidence/2026-09-24-keyboard-hold-validation/hold-background/summary.json),
[foreground control](evidence/2026-09-24-keyboard-hold-validation/hold-foreground/summary.json),
and [selected receiver event samples](evidence/2026-09-24-keyboard-hold-validation/held-shift-before-after.json).
[Build identity](evidence/2026-09-24-keyboard-hold-validation/build.json) records
the revision and working-tree binary/source hashes.

### Held-modifier composition regression and fix

Before the fix, every held-Shift composition trial produced `bc`, not `Bc`.
The DOM trace showed:

```text
keydown Shift  shift=true
keydown b      shift=false
keyup b        shift=false
keyup Shift    shift=false
keydown c      shift=false
```

The hold reached the receiver, but the subsequent ordinary press lost the
modifier. `Keyboard.swift::pressKeys` started with empty `CGEventFlags` and
derived them solely from that request's key list. Sending Shift and B together
through `hold_keys` already worked, separating this failure from timed delivery.

`Keyboard.swift` now retains held modifier flags per recipient. It serializes a
complete `pressKeys` submission with held-key transitions and combines the held
flags with that press's local flags. Releasing a held modifier clears the retained
flag. Unicode text events still have separate semantics; their composition with
a held modifier remains deferred at the native call site.

The isolated PR worktree includes the authenticated macOS event posting path
used by the receiver validation. Without that path, a control run using the old
public `postToPid` submission failed all five Chrome hold/release cases in one round;
therefore the authenticated path is part of this PR's macOS behavior and evidence.
After the composition fix, five new background trials passed all five bounded
hold/release cases and the held-Shift/ordinary-`b` case in both Chrome and
Electron (5/5 per case and receiver). The latter produced `Bc` after release.
[Post-fix summary](evidence/2026-09-24-keyboard-hold-validation/hold-composition-fix/summary.json)
and the [before/after event samples](evidence/2026-09-24-keyboard-hold-validation/held-shift-before-after.json)
record the counts and representative observations. The fixture exits 1 because the separately noted
select-all replacement and emoji cases still fail.

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
26/45 run. See the [CLI summary](evidence/2026-09-24-keyboard-hold-validation/cli-background/summary.json)
and the reproducible receiver fixture.

No live auto-repeat, crash recovery, Linux/Windows receiver behavior, or RPC
shutdown guarantee is established by these samples. The coordinator cancellation
test remains unit-level evidence. Five trials per background case are bounded
observations on these versions, not a universal support claim.

### Reproduction and checks

```sh
scripts/generate-swift-bridge
cargo build -p auv-cli
cargo build -p auv-driver-macos --example keyboard-receiver-sender
python3 crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py \
  --electron "$electron_binary" \
  --sender target/debug/examples/keyboard-receiver-sender \
  --output "$probe_root/hold-background" --repetitions 5
python3 crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py \
  --electron "$electron_binary" \
  --sender target/debug/examples/keyboard-receiver-sender \
  --output "$probe_root/hold-foreground" --repetitions 2 --mode foreground
```

Use new output directories and the isolated Electron installation described in
the [authentication receiver report](2026-09-23-background-keyboard-authentication.md).
The matrices intentionally exit 1 while the listed behavior failures remain.
The follow-up also passed native SwiftPM build, `cargo test -p auv-driver-common
-p auv-driver-macos --lib`, and `cargo test -p auv-cli --lib keyboard`.
Fixture syntax and formatting checks passed.
