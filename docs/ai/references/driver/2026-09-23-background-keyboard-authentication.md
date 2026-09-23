# Background keyboard event authentication

Date: 2026-09-23. Classification: approved feature, followed by owner-requested test-only validation. Status: authentication implemented; Chrome/Electron compatibility matrix failing.

Update 2026-09-24: The [held-key receiver follow-up](2026-09-24-keyboard-hold-contract.md#macos-receiver-validation)
adds persistent-sender controls and real timed/release receipts. Basic holds pass;
held modifiers do not compose with ordinary presses, and background menu/emoji
failures remain. Those results do not replace the historical CLI matrix below.

The owner approved adding the keyboard authentication mechanism discussed in the
[BG-2 comparison](2026-09-23-background-delivery-project-comparison.md). This slice
changes process-targeted keyboard submission for text, keys, and combinations in
the macOS native boundary. Mouse double posting and multi-click counts are separate.

## Delivery contract

Prepare an authentication message for each already-stamped keyboard event, then
submit it once through `SLEventPostToPid`. Missing symbols, missing factory selector,
or authentication preparation failure retain the existing public `postToPid`
submission. There is no second submission after a SkyLight post. Foreground
submission remains the session event tap. `InputActionResult` continues to describe
submission; authentication does not set `verified` or imply toolkit support.

This uses an all-or-nothing authenticated path rather than unauthenticated SkyLight
delivery. It retains the existing behavior on hosts without the complete private
capability. The producer does not add a public toolkit heuristic or authentication
status schema in this slice; a concrete consumer would be needed for that extension.

## Native boundary evidence

CUA provides the authentication factory and setter reference, including the missing
factory selector on macOS 14:
[fixed source](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L308-L351).

On macOS 26.3 (25D2125), arm64, a standalone no-post probe resolved the factory,
`SLEventGetEventRecord`, and setter. The native getter copies into a caller-owned
buffer: `(CGEventRef, void *, uint32_t) -> CGError`, not a pointer-returning getter.
Native arm64 instructions compare the third argument with 248, copy the record into
the second argument, and return zero on success. Event field 50 reports 248 on the
keyboard fixture. A copied record successfully created an authentication message.
The factory object's inspected ivars contain values and Objective-C objects (including
`NSData` for the signature), with no borrowed record pointer. This avoids CUA's
guesses at the event object's internal pointer offsets.

Probe commands were `swift /tmp/auv-keyboard-auth/record.swift` and disassembly of
the getter's in-process instruction bytes with Capstone. Initial isolated signature
and Foundation-size probes failed: the getter asserts on a wrong buffer length,
and `NSGetSizeAndAlignment` rejects the record's bitfield encoding. Neither probing
method belongs in production. The implementation must validate the reported record
size before calling the getter and must not parse the record's private layout.

## Implementation

`EventPosting.swift` owns symbol resolution, Objective-C signature validation,
record copying, message lifetime, and the single authenticated submission. It
shares the existing `SLEventPostToPid` binding with the mouse implementation.
`Keyboard.swift` calls it after stamping the target for every process-targeted
event. No Rust/Swift wire declarations or public operation parameters changed.

An autorelease pool bounds each message's lifetime. The record buffer and message
remain alive through attachment and posting. A reported record length other than
248 rejects the authenticated path before the size-sensitive getter is called.
There is no raw access to offsets inside the CGEvent object.

## Validation and evidence boundaries

- `scripts/generate-swift-bridge`: passed; generated files remain ignored.
- `swift build` in the native macOS package: passed, with existing OCR warnings.
- `cargo check`: passed.
- `cargo test`: passed, 84 tests and 1 ignored in the workspace's default CLI
  members. This command is not the whole-workspace test suite.
- `cargo test -p auv-driver-macos -p auv-driver-common --lib`: 156 passed, 5 ignored.
- `cargo test -p auv-driver-macos --test keyboard_authentication -- --include-ignored --nocapture`:
  native contract and AppKit receiver tests pass on macOS 26.3 (25D2125), arm64.
- Formatting and diff checks passed.

The no-input native test injects the external copy, factory, attachment, and post
operations. It asserts their order and exactly one private submission, rejects a
missing symbol/class/selector or incompatible method signature, and checks that
copy/factory failure never posts. It also executes the real native copy, factory,
and attachment while replacing only the final post. This proves preparation on
the tested host without sending input to an unrelated application.

[Native and AppKit test output](evidence/2026-09-23-background-keyboard-authentication/appkit-and-native-tests.log)
records the final receiver run. The independent AppKit NSTextView receiver observed `A猫B`, with one down and one
up for each of the three text/key inputs, correct Shift flags, balanced modifier
transitions, the expected window, and `active:false` throughout receipt. Its test
is opt-in because it opens a bounded GUI fixture and requires Accessibility.

A separate live Chrome 153.0.8010.53 probe used a new temporary browser profile and
a localhost page with a focused textarea. Its DOM receiver logged background
Unicode text `A猫` exactly once, with the previously foreground application
preserved. The driver still returned `verified:false`. See
[Chrome receipts and comparisons](evidence/2026-09-23-background-keyboard-authentication/chromium.json).

This does **not** establish universal Chromium keyboard support or a measured
improvement over the public route. One immediate `Shift+B` request failed to reach
the page; a later run received it. The native comparison also saw missing
zero-dwell physical keys, and receipt after an 8 ms gap in both public and
authenticated examples. These probes do not isolate timing as the only cause.
No timing changes were made as part of authentication.

An additional AppKit menu fixture tested Cmd+A followed by `Z` after initial `ABC`.
Both public and authenticated posting produced `ABCZ`, rather than replacing the
selection. This is a negative baseline observation, not an authentication-only
regression or a passing replacement claim. See
[menu comparison](evidence/2026-09-23-background-keyboard-authentication/appkit-menu-comparison.json).
The committed AppKit receiver test asserts the observed text/Shift sequence; it
does not assert that background menu selection works.

## Chrome and Electron receiver matrix

The owner requested real Chrome and sample Electron tests after implementation.
The committed [receiver harness](../../../../crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py)
launches each application with a new temporary profile and the same localhost
textarea receiver. It addresses only the window belonging to the process it
started. Electron uses a sandboxed renderer without Node integration and a normal
Edit menu. Its main process also records `before-input-event`, independently of
the DOM event logger. No browser automation or JavaScript input injection supplies
the tested keystrokes; all inputs go through the built AUV CLI.

The final runs used macOS 26.3 arm64, Chrome **153.0.8010.53**, and Electron
**44.4.4** with Chromium **152.0.7977.130**. The harness temporarily selected the
ABC keyboard source, restored it afterward, and blurred/reset the textarea before
each case to end prior IME composition. Earlier foreground runs using the active
IME produced `insertCompositionText` spanning cases and are excluded from this
matrix. An earlier run whose foreground precondition prevented submission is also
excluded. These controls do not alter the native inter-event timing.

Evidence level: repeated live receiver observations on one host and these versions,
not a toolkit support guarantee. Each cell is complete passes / trials. A pass
requires expected text, exact input-event count, balanced base-key down/up counts
(except the menu case), trusted received events, the expected foreground state,
and driver `verified:false`.

| Case | Chrome background | Electron background | Chrome foreground control | Electron foreground control |
| --- | --- | --- | --- | --- |
| Unicode `A猫` | 5/5 | 4/5 | 2/2 | 2/2 |
| Emoji `😀` | 0/5 | 0/5 | 1/2 | 1/2 |
| Physical `b` | 4/5 | 4/5 | 2/2 | 2/2 |
| Shift+B | 5/5 | 5/5 | 2/2 | 0/2 |
| `x`, count 3, interval 50 ms | 5/5 | 5/5 | 2/2 | 2/2 |
| Cmd+A, then `Z` replacing `replace me` | 0/5 | 0/5 | 2/2 | 2/2 |
| Left, then `X` inside `ab` | 4/5 | 4/5 | 2/2 | 1/2 |
| Return after `ab` | 4/5 | 2/5 | 1/2 | 2/2 |
| Shift+B, then unmodified `c` | 3/5 | 2/5 | 2/2 | 1/2 |
| **Total** | **30/45** | **26/45** | **16/18** | **13/18** |

All 90 background trials completed submission and passed the foreground checks:
the observed frontmost PID was unchanged before/after each case, and received DOM
events reported `document.hasFocus() == false`. This is bounded observation, not
continuous OS focus monitoring. All driver results remained `verified:false`.
Some failed trials had no events; others had missing key-up events or unchanged
text. Correct text alone did not count as a complete pass.

Both background receivers failed every emoji insertion and select-all replacement
trial. Some emoji requests reached `keydown` without a text insertion. Replacement
left either `replace me` or `replace meZ`. The foreground controls successfully
replaced the selection in both applications, proving that the receiver and Edit
behavior work in that condition. Intermittent key loss also occurred in foreground
controls, so the observations cannot attribute all failures to authentication or
background posting. No new timing, Unicode, or menu routing fix is included here.

The harness exits **1** for these failing matrices. Raw receipts, CLI results,
versions, and native Electron event logs are retained:

- [Background summary](evidence/2026-09-23-background-keyboard-authentication/chrome-electron-abc/background/summary.json)
  and the reproducible [receiver fixture](../../../../crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py).
- [Foreground control summary](evidence/2026-09-23-background-keyboard-authentication/chrome-electron-abc/foreground/summary.json)
  and sibling receipt files.
- [Build identity](evidence/2026-09-23-background-keyboard-authentication/chrome-electron-abc/build.json)
  includes the base revision and hashes of the tested binary and native source;
  authentication was an uncommitted working-tree change.

Reproduce from the repository root on a macOS GUI session with Accessibility
permission and Chrome installed. Each invocation briefly opens its own receivers,
restores the previous foreground application/input source, and removes its profiles:

```sh
cargo build -p auv-cli
probe_root="$(mktemp -d)"
npm install --prefix "$probe_root" --no-audit --no-fund electron@44.4.4
node "$probe_root/node_modules/electron/install.js"
python3 crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py \
  --electron "$probe_root/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron" \
  --output "$probe_root/background" --repetitions 5
python3 crates/auv-driver-macos/tests/fixtures/chromium_keyboard/run.py \
  --electron "$probe_root/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron" \
  --output "$probe_root/foreground" --repetitions 2 --mode foreground
```

## Candidate next slice

Use these failing receivers to isolate physical-key timing, supplementary Unicode
insertion, and background menu dispatch before defining compatibility policy. The
test-only follow-up reproduces these gaps but does not fix them. Production changes
remain deferred from the approved authentication slice; the key-sequence call site
has a matching TODO. Mouse dual posting and the two-click compatibility cap remain
unchanged. BG-2 is not closed by authentication or by passing native contract tests.
