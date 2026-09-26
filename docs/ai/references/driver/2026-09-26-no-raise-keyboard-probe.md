# Keyboard input with no-raise activation and explicit focus restoration

Date: 2026-09-26. Classification: test-only experiment and evidence.
Production keyboard delivery is unchanged.

An external test controller can establish native key-window state, call AUV's
existing PID-targeted keyboard APIs, and restore the original receiver's focus
without an observed change in the two test windows' order or system foreground
identity. The complete record-based sequence passed 27/27 bounded receiver
trials. CUA's `activate_without_raise` record pair alone did not solve the
select-all or Chromium emoji failures.

This is an experimental focus transition, not a claim of zero focus disturbance
or a newly supported production `BackgroundOnly` route. The target becomes
internally active/key while the anchor becomes inactive/non-key. Concurrent
human keyboard input, multi-window applications, other Spaces, minimized windows,
crash recovery, and other OS versions were not validated.

## Fixture and input path

The [fixture](../../../../evals/auv-base/platforms/desktop-macos/tasks/focus_without_raise.rs)
launches two independent Swift/AppKit applications: an `NSTextView` target with a
Select All menu item and an anchor application representing the user's current
foreground. It also launches isolated Electron and Chrome receivers in turn.
Neither receiver contains method overrides or artificial Chromium focus hooks.

The actual keyboard input is sent by the existing persistent Rust
`keyboard-receiver-sender` through the public AUV library. Each select-all trial
holds Cmd+A for 200 ms, releases it, then types `Z` into initial `replace me` text.
The other cases insert `😀` and `A猫` into an empty control. The positive no-raise
sequence uses the existing authenticated PID route for all these keys and text.

`agent-browser` 0.38.0 connects to the isolated Chrome profile through CDP. It
reads the resulting value, selection and `document.hasFocus()` and captures
screenshots; it does not generate the tested input. Its independent reads agreed
with all 36 Chrome receipts in the repeated matrix.

Native/DOM receivers record text, selection and focus changes. The controller
samples NSWorkspace's front PID, `_SLPSGetFrontProcess` PSN and target/anchor
window order at a requested 10 ms cadence, retaining state changes. Stage
snapshots supplement these samples. These are bounded observations: sub-sample
transitions are not excluded, and only the two fixture windows' relative order
is asserted. No physical keyboard concurrency claim follows from an unchanged
foreground PID.

Environment: macOS 26.3 (25D2125), arm64; Chrome 153.0.8010.53; Electron 44.4.4 /
Chromium 152.0.7977.130; ABC input source. The prior application and input source
are restored by the harness. All receivers and agent-browser sessions are owned
by this fixture and are closed afterward.

## Three separate state transitions

CUA's pinned [activate_without_raise implementation](https://github.com/trycua/cua/blob/605d358a48ad938a41b384a8f23909153eb73f78/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L492)
posts 248-byte records to the old and new processes. `record[0x08] = 0x0D`;
`record[0x8A]` distinguishes loss (`0x02`) from gain (`0x01`); the requested window
ID is at `0x3C..0x40`. The test ports this recipe without calling set-front.
Both native posts returned zero, and the Swift receiver became active, but its
window remained non-key. The web receivers also retained `document.hasFocus() ==
false`. Activation alone was insufficient.

The `no_raise_key` variant adds the pair of records used by CUA's
[make_exact_window_key](https://github.com/trycua/cua/blob/605d358a48ad938a41b384a8f23909153eb73f78/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L619):
event kinds `0x01`, `0x02`, byte `0x3A = 0x10`, and bytes `0x20..0x30 = 0xFF`.
Crucially, this experiment omits that CUA function's separate set-front call.
This combination established key-window/page-input focus and allowed the existing
authenticated keyboard route to perform both semantic operations.

Restoration is a third operation. CUA's raw-click wrapper
[conditionally restores from NSWorkspace's front PID](https://github.com/trycua/cua/blob/605d358a48ad938a41b384a8f23909153eb73f78/libs/cua-driver/rust/crates/platform-macos/src/tools/click.rs#L1216).
In this keyboard experiment, NSWorkspace and WindowServer continued to name the
anchor even after AppKit deactivated it. That condition did not run restoration,
and the anchor stayed internally unfocused. This observation is about the ported
conditional wrapper, not an end-to-end failure claim against the whole CUA tool.

The successful test restoration explicitly sends a loss record to the receiver
that was activated, a gain record to the original anchor, and the key-window pair
to the original anchor window. It does not resolve the defocus recipient from
the still-unchanged WindowServer front PSN. The anchor then becomes active/key,
and the target loses key/page focus.

```text
capture original process and window
  -> send old-app loss + target-app gain records
  -> send target key-window record pair
  -> existing AUV PID-targeted keyboard input
  -> observe receiver result
  -> send target loss + original-app gain records
  -> send original key-window record pair
  -> verify restored receiver focus
```

## Repeated results

Three trials per cell. The repeated matrix has 108 trials and no invalid
input-source/foreground preconditions. Counts below assert exact text; focus,
restoration, and ordering assertions are separately retained in each receipt.

| Setup | Case | Swift/AppKit | Electron | Chrome + agent-browser |
| --- | --- | --- | --- | --- |
| Original background | Select all + replace | 0/3 | 0/3 | 0/3 |
| Original background | Emoji | 3/3 | 0/3 | 0/3 |
| Original background | `A猫` | 3/3 | 3/3 | 3/3 |
| CUA no-raise records only | Select all + replace | 0/3 | 0/3 | 0/3 |
| CUA no-raise records only | Emoji | 3/3 | 0/3 | 0/3 |
| CUA no-raise records only | `A猫` | 3/3 | 3/3 | 3/3 |
| No-raise + key-window records | Select all + replace | **3/3** | **3/3** | **3/3** |
| No-raise + key-window records | Emoji | **3/3** | **3/3** | **3/3** |
| No-raise + key-window records | `A猫` | **3/3** | **3/3** | **3/3** |
| No-raise + actual AUV background click | Select all + replace | 3/3 | 3/3 | 3/3 |
| No-raise + actual AUV background click | Emoji | 3/3 | 3/3 | 3/3 |
| No-raise + actual AUV background click | `A猫` | **2/3** | 3/3 | 3/3 |

All 27 `no_raise_key` trials passed every check: exact text, unchanged tracked
window order, unchanged sampled foreground identities, restored anchor focus,
target unfocused afterward, and unchanged anchor text. Chrome additionally passed
the independent agent-browser comparison. The successful record path requires
neither an input-field click nor an unauthenticated key event in these fixtures.

The actual-click variant supplies an alternative way to establish key-window
state, but its third Swift BMP trial stopped updating the receiver receipt after
the click and inserted no text. The last receipt still showed the target active
and key after attempted restoration. The sample remains a failure; stale receipt
state cannot establish whether restoration itself was processed. A mouse tracking
loop is a possible explanation, not an established root cause. Investigating that
pointer delivery issue is a separate candidate slice.

Two preliminary matrices are retained as controls. The first (`pilot`) calls its
ordinary-activation-plus-PID route `foreground`; it must not be confused with
the later full `ForegroundPreferred` keyboard control. That first route inserted
emoji but failed select-all in each receiver. In `controls`, full foreground
delivery passed all nine cases, and the no-raise/key-window path worked with both
authenticated AUV chords and a CUA-style unauthenticated chord. The latter is a
transport comparison, not required by the final positive sequence. The preliminary
no-raise modes deliberately use the conditional restoration that exposed the
anchor-focus defect; they do not count as successful complete transactions.

## Reproduce

Use the isolated Electron installation described in the keyboard authentication
report and build the existing sender first. The Rust task compiles its standalone
Swift fixtures with `swiftc`; no production Swift package or FFI was changed.

```sh
just eval build
just eval focus \
  --electron "$electron_binary" --output "$new_probe_directory" \
  --repetitions 3 --mode baseline --mode no_raise \
  --mode no_raise_key --mode no_raise_click --restore records
```

For a focused assertion of the positive experimental chain, select only
`--mode no_raise_key --restore records --require-success`. The flag makes failed
text, posture or restoration assertions exit 1. The mixed diagnostic matrix
normally exits 0 when collection completes, since it includes expected failing
controls. An input-source change during a run is invalid; GUI use should be kept away
from the fixture until it finishes.

These are locally observed results. Raw receipts, native/DOM observations,
screenshots, and build metadata remain local and are excluded from the PR.
The Rust task writes fresh validation output to the requested output directory.
The original probe passed Swift compilation, Python/JS syntax checks, and
`git diff --check`. The current runner has since moved to Rust with TypeScript
web/Electron receivers; see the [evaluation suite](../../../../evals/auv-base/README.md)
for current build commands and migration validation.

NOTICE: This test controller is not a driver implementation. Production integration
would need explicit focus-disturbance semantics, exact-target admission, restoration
on errors/cancellation, and receiver verification. These remain deferred until an
owner-approved implementation slice; the existing keyboard API cannot describe
the external test controller's focus changes through its input result alone.
