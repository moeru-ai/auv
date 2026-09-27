# Background keyboard event authentication

Date: 2026-09-23. Classification: approved feature. Status: authentication
implemented; receiver acceptance varies by action and application.

The owner approved adding the keyboard authentication mechanism discussed in
the [BG-2 comparison](2026-09-23-background-delivery-project-comparison.md).
It changes process-targeted keyboard submission for text, keys, and
combinations in the macOS native boundary. Mouse posting and multi-click
counts are separate concerns.

## Delivery contract

The native code stamps each process-targeted keyboard event, prepares an
authentication message, and submits the event once through `SLEventPostToPid`.
Missing symbols, a missing factory selector, or preparation failure select
the existing public `CGEvent.postToPid` route. There is no second submission
after a SkyLight post. Foreground input still uses the session event tap.

The fallback preserves behavior on hosts without the complete private
capability. The public API does not expose a toolkit heuristic or an
authentication-status field. `InputActionResult` describes submission and
keeps `verified: false` until a separate receiver check establishes an effect.

## Native boundary

CUA supplies the authentication factory and setter reference, including the
missing factory selector on macOS 14: [pinned source](https://github.com/trycua/cua/blob/d1a01f8580d5963702427b9e110fbcd98c39fac3/libs/cua-driver/rust/crates/platform-macos/src/input/skylight.rs#L308-L351).

On macOS 26.3 arm64, a standalone no-post probe resolved the factory,
`SLEventGetEventRecord`, and setter. The native getter copies into a
caller-owned buffer with signature `(CGEventRef, void *, uint32_t) -> CGError`.
The observed record length was 248 bytes. A wrong buffer length caused an
assertion in the native getter, so the implementation rejects other lengths
before calling it. It does not parse private CGEvent offsets.

`EventPosting.swift` owns symbol resolution, Objective-C signature checks,
record copying, message lifetime, and single submission. `Keyboard.swift`
calls it after stamping the recipient. An autorelease pool bounds the
message lifetime; the record and message remain alive through attachment and
posting. No Rust/Swift wire declarations or public operation parameters
changed for authentication.

## Validation boundary

- `scripts/generate-swift-bridge`, native macOS `swift build`, `cargo check`,
  default `cargo test`, and focused driver library tests passed.
- `cargo test -p auv-driver-macos --test keyboard_authentication` passed on
  macOS 26.3 arm64. Its injected boundary asserts copy, factory, attachment,
  and exactly one private post in order. Missing API, ABI mismatch, copy
  failure, and factory failure cannot post. It also exercises native
  preparation while intercepting the final post.
- Earlier local AppKit and Chrome/Electron probes observed accepted Unicode
  input and several physical-key combinations, as well as missing events.
  The reproducible GUI harness lives under
  [`evals/auv-base`](../../../../evals/auv-base/README.md). Raw receipts and
  temporary probes stay local.

The historical Chrome/Electron background matrix had 30/45 and 26/45
complete cases respectively on one macOS 26.3 host. Both applications
failed all five background emoji insertions and Cmd+A replacement trials.
Some foreground controls also lost events. This does not establish a
universal Chromium keyboard capability or isolate authentication as the
cause of the failures. No timing, Unicode, or menu-routing fix is included
in this slice.

NOTICE: Background command-target resolution and Unicode IME focus remain
open. A future owner-approved input contract must define those behaviors
before production delivery can claim semantic text-editing success.

## Historical GUI receiver validation

The independent AppKit NSTextView receiver observed `A猫B`, with one down and one
up for each of the three text/key inputs, correct Shift flags, balanced modifier
transitions, the expected window, and `active:false` throughout receipt. Its test
is opt-in because it opens a bounded GUI fixture and requires Accessibility.

A separate live Chrome 153.0.8010.53 probe used a new temporary browser profile and
a localhost page with a focused textarea. Its DOM receiver logged background
Unicode text `A猫` exactly once, with the previously foreground application
preserved. The driver still returned `verified:false`.

This does **not** establish universal Chromium keyboard support or a measured
improvement over the public route. One immediate `Shift+B` request failed to reach
the page; a later run received it. The native comparison also saw missing
zero-dwell physical keys, and receipt after an 8 ms gap in both public and
authenticated examples. These probes do not isolate timing as the only cause.
No timing changes were made as part of authentication.

An additional AppKit menu fixture tested Cmd+A followed by `Z` after initial `ABC`.
Both public and authenticated posting produced `ABCZ`, rather than replacing the
selection. This is a negative baseline observation, not an authentication-only
regression or a passing replacement claim.
The committed AppKit receiver test asserts the observed text/Shift sequence; it
does not assert that background menu selection works.

## Chrome and Electron receiver matrix

The owner requested real Chrome and sample Electron tests after implementation.
The committed [receiver harness](../../../../evals/auv-base/platforms/desktop-macos/tasks/keyboard.rs)
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

The harness exits **1** for these failing matrices. These are locally observed
results. Raw receipts, CLI output, build metadata, and native event logs remain
local and are excluded from the PR; the reproduction commands below generate
fresh output in a temporary directory.

Reproduce from the repository root on a macOS GUI session with Accessibility
permission and Chrome installed. Each invocation briefly opens its own receivers,
restores the previous foreground application/input source, and removes its profiles:

```sh
just eval build
probe_root="$(mktemp -d)"
npm install --prefix "$probe_root" --no-audit --no-fund electron@44.4.4
node "$probe_root/node_modules/electron/install.js"
just eval keyboard \
  --electron "$probe_root/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron" \
  --output "$probe_root/background" --repetitions 5
just eval keyboard \
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
