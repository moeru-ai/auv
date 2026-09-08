# Targeted keyboard input and invoke failures

Status: implemented on `fix/target-input-consistency`, based on main
`37ec72b8` (PR #176). No release was published.

## Target contract

The command definition's `TargetPolicy` supplies runtime validation, command
help, and MCP `x-auv-commands` metadata (`target.required`,
`target.accepted_types`, and `target_help`). A generic top-level target option
is not evidence that every command accepts every resource type.

| Commands | Accepted target | Effect |
| --- | --- | --- |
| `input.key`, `input.typeText`, `input.pasteText` | optional application or window | Resolve the running application or exact observed window; apply the explicit input policy; post events to the owning pid. |
| `input.focusText`, `input.axFocusText` | required application | Select an AX text control and request focus. This remains separate from application/window activation. |
| `input.clickPoint` | optional application, window, display | Existing logical screen/window/display coordinate contract; `--normalized` uses target-local ratios. |
| `window.capture/findText/waitForText/clickText` | optional application | Existing window selector, including command-local title options. |
| `app.activate` | required application | Existing explicit activation operation. |
| `mediaControl.*` | forbidden | System-wide now-playing state and media controls, not application-targeted input. |
| `input.moveMouse`, screen, display-list/capture, overlay and fixture scan commands | forbidden | Their own screen/display/fixture scope; no implicit application activation. |

Bare CLI targets retain the meaning of application bundle ids. MCP uses
`application_id`, `window_id`, or `display_id`, with at most one field present.
An application target requires an already running application, but no visible
WindowServer window or exact AX window. Missing applications are never launched;
multiple running instances require an explicit window selection.

Target and policy are independent. The three CLI commands expose
`--input-policy`, also present in their generated MCP argument metadata:

| Policy | Preparation and delivery |
| --- | --- |
| `foreground-preferred` (CLI default) | Activate once, confirm the foreground pid, then deliver to that pid. Window targets additionally require exact AX window resolution, raise, and focused-window confirmation. |
| `background-only` | Validate the process/window owner and deliver to the pid without activation. No foreground fallback. |
| `background-preferred` | Currently the same no-steal behavior on this keyboard path. The existing foreground-fallback deferral is preserved explicitly. |

Rust callers select policy in `KeyboardInput`: key/paste carry an explicit
policy beside their existing options; text reuses `TypeTextOptions.policy`.
Untargeted CLI input retains foreground behavior and rejects background policy.
Background submission returning success does not prove that the app consumed it,
so it cannot automatically trigger a reliable retry of ignored input. Retrying a
partially delivered operation can duplicate text; automatic foreground fallback
remains deferred. The existing explicitly enabled `allow_clipboard_fallback`
remains available for foreground text delivery errors, with PID-bound paste.
Resolution or preparation failure exits before that fallback can run.

`InputApi::prepare_for_input` owns shared application/window preparation;
`WindowApi::prepare_for_input` delegates to it. `WindowApi::type_text` also
uses shared keyboard delivery, so ForegroundPreferred now prepares foreground
focus before typing. Target-bound keyboard delivery uses the same lifecycle and
existing process activation helper. Native functions
validate the recipient and confirm focus; there is no keyboard-only activate
implementation. Existing window preparation callers now fail for stale or
AX-inaccessible exact foreground targets rather than activating an arbitrary
window of the same application. This is an intentional safety tightening.

Focus confirmation observes a bounded predicate, not a fixed input delay or
repeated activate loop. Validation-only calls stop before activation, focus
changes, clipboard mutation, or keyboard posting.

A window target is not a control target. The caller must establish the intended
search field/first responder separately. A click on a clear button is not a
promise that the search editor retains keyboard focus. NetEaseMusic's tested
window exposes no usable AX text node; `input.focusText Search` fails explicitly.
Use a fresh visual observation and an explicit click on the editor in that case.

No global keyboard fallback is permitted after a target is supplied. Events
remain pid-bound even if foreground focus changes between preparation and
posting. This does not lock user focus or guarantee which control inside that
application receives an event. Per-control focus leases/selectors are outside
this slice; reopening that boundary requires an approved driver contract.

## Execution and verification

`InputActionResult` remains the driver-owned direct result and tracing artifact.
No parallel action-result schema was introduced.

- `attempts[].succeeded` means that the delivery API returned without reporting
  an error after submission. CGEvent posting does not acknowledge application
  consumption. It does not mean the old query was selected, text replaced,
  search completed, or playback started.
- `verified` means a separate post-action observation proved the asserted
  semantic effect. Raw keyboard, text, paste and click delivery leave it false.
  Application/window preparation is not semantic verification of typed text.
- Foreground-policy target input reports `focus_disturbance: foreground` because
  it may activate/raise the target and does not restore the previous foreground
  app. Background-policy target input reports `none`: AUV does not request focus
  changes; the receiving app can still respond to events in its own way.
  Untargeted input retains its existing foreground-focus dependency and unknown
  focus disturbance. Paste reports temporary clipboard disturbance because the
  existing snapshot/restore transaction is reused.
- An input call can succeed while the app ignores it. Callers needing semantic
  success must verify text, search state, or playback using an independent
  observation. Media `verified` is scoped to its before/after media-state test.

## Keyboard operation hierarchy

The approved keyboard model separates a key, a chord, and an ordered request:

- `PressKey`: single-key convenience. The released string shortcut syntax
  (`cmd+a`) remains accepted at the legacy entry point; new callers use PressKeys.
- `PressKeys`: one chord represented by `PressKeysOptions.keys`. Modifiers
  precede ordinary keys; ordinary keys retain their order. All keys are pressed
  and released in reverse order. Multiple ordinary keys are allowed.
- `InputKeyboard`: an ordered list of `KeyboardInput` actions: PressKeys,
  TypeText, or PasteText. Unicode typing and clipboard transactions retain their
  driver semantics; they are not converted to physical key names.

`input.key` retains legacy shortcut spelling and adds `--count`/`--interval-ms`.
`input.keys` accepts explicit positional keys. `input.keyboard --actions JSON`
accepts an array of tagged actions. MCP uses the same registered commands and
metadata. Its string-valued `inputs.keys` must contain an encoded JSON array.

```sh
auv invoke input.keys cmd shift p --target app:com.example.editor
auv invoke input.key return --count 2 --interval-ms 100 --target window:123
auv invoke input.keyboard --target app:com.netease.163music \
  --actions '[{"kind":"press","keys":["escape"],"count":3,"interval_ms":100},{"kind":"type_text","text":"hello"}]'
```

Supported physical names currently include command/cmd, shift, option/alt,
control/ctrl, return, enter, tab, delete/backspace, forwarddelete, escape/esc,
space, arrows, home/end, pageup/pagedown, F1..F20, and ANSI letters/digits/punctuation.
Uppercase/shifted punctuation adds Shift to the whole chord. Modified special keys such as `cmd+return` are now
representable, as are literal `+` keys. Unknown keys and duplicate aliases
are rejected. Physical character names use the existing ANSI key map; callers
that require Unicode or keyboard-layout-independent text must use TypeText.

A count in 1..=255 repeats the complete press/release action. Counts above one
require a positive interval; a single press requires zero interval. Interval
is a minimum wait between complete repetitions, followed by recipient/focus
checks, not an exact event timestamp. Settle applies once after the final press.
This is not a hold, OS auto-repeat, or a repeat of the whole action list.
Independent key-down/up, holds, and cancellation/release coordination remain
intentionally deferred. A started synchronous request is not cancelled merely
because the caller disconnects; callers must not treat disconnect as rollback.

The driver validates the whole list before resolving/activating the recipient.
It binds a target process once, then checks identity and applies the action's
focus policy before every action and repetition. Swift creates all down/up
events for a chord before posting any of them. Posting itself has no OS receipt
that proves control consumption. Completion is not atomic or semantic success.

On success, InputKeyboard returns one InputActionResult per action; a repeated
press includes its individual submission attempts. On failure, execution stops.
`KeyboardInputError` retains the driver cause plus `KeyboardInputProgress`:
completed action results, zero-based failed action index, and the number of
fully submitted repetitions within the failed press. A failed text/paste action
can still have partial effects that this counter cannot measure. No automatic
replay of the list occurs.

Runner errors retain their gRPC status category/message and encode
KeyboardInputProgress in Status.details. The Rust client preserves this typed
progress, and CLI/MCP include it as `failure_details.keyboard_progress`.
Completed action results still produce their normal tracing artifacts.
Callers must retain gRPC details or CLI stdout on nonzero exit. Progress is
submission evidence and does not change `verified: false`.

Evidence level: automated driver, invoke, and Runner handler regressions cover
full-list validation, chords, repetition, dry-run, stopping on native failure,
and structured progress. The macOS checks recorded below remain independent
semantic evidence for the observed workflows, not a general app support claim.

## Runner and compatibility

Targeted invoke and ordered keyboard requests on local and selected Runner paths call the same
`InputApi::input_keyboard` and existing native keyboard/clipboard abilities.
The driver-owned `InputTarget` distinguishes a running application bundle from
an observed Window. Window RPC requests retain the observed pid, so a recycled
window id cannot silently change the recipient. Application RPC requests resolve
the running instance on the Runner, without client-local window enumeration.

`InputService/InputKeyboard` executes an ordered list of typed input actions.
`PressKeys` submits one chord through that interpreter. Existing `PressKey`
remains a foreground convenience and also uses the interpreter on macOS.
The branch-only `SendTargetedKeyboardInput` was replaced before release.
Updated consumers and Runners are required for InputKeyboard/PressKeys; an old
Runner returns UNIMPLEMENTED instead of ignoring a safety target. Existing
global RPC request/response shapes and InputActionResult wire shapes are unchanged.
There is a deliberate character-key behavior change: legacy single-character
PressKey/input.key previously used Unicode injection, while the unified press
path now sends physical keys. These follow the active keyboard layout/IME;
non-ANSI literal characters require TypeText. Shortcut strings remain accepted.

Each action owns its input policy. Text has exactly one policy source,
`TypeTextOptions.policy`; there is no duplicated request-level policy. Window
identity uses the existing observed `Window.ref` and `Window.process_id`,
inside the target variant. Application targets carry no window-only pid field.
The new RPC requires an explicit target, including `foreground: true` when
global foreground input is intended. Absence is an error. Unspecified action
policy retains the existing driver `BackgroundPreferred` default; an explicit
foreground recipient requires `ForegroundPreferred`. CLI defaults to foreground
and applies its `--input-policy` option to every action.

Selected keyboard dry-runs, with or without a target, inspect the selected Runner and may create/finish a
recorded Run. They validate every action and recipient without activation,
clipboard changes, repetition waits, or delivery.

The historical `window_targeted_keyboard` delivery-path name is retained for
PID-bound events. Application scope stamps only the pid, with no window-routing
fields. The attempt message names the pid and whether a window was selected.

CLI JSON keeps the existing `failure` string and adds `failure_details` with
`code` and `message`. Runtime failure still exits nonzero and retains `run_id`
and `command_id`. MCP retains the same details and command id in its structured
error response. Target validation and target-bound driver failures preserve
categories such as `invalid_target`, `not_found`, `unsupported`, `invalid_input`
and `backend`. Legacy non-keyboard string helpers retain `command_failed`;
this migration does not infer categories by parsing human error strings.
CLI parser and frontend setup errors (selection/tracing) before execution
retain the existing stderr boundary. Structured operation failures begin at
the registered execution boundary.

Rust handler futures now return `InvokeExecutionResult` with `InvokeFailure`;
legacy command-output helpers keep their string errors and map them at the
registered handler boundary. Rust callers inspecting errors should use
`failure.code` or `failure.message` instead of treating the failure as a String.
Commands that previously ignored a target now explicitly reject it. Human
error wording is not a machine contract; use the failure code.

## macOS evidence

The initial evidence below predates the input-preparation consolidation. The
follow-up evidence under `refinement/` records the current policy-aware paths,
including background delivery without activation, application targets without
AX windows, stale-window preparation rejection, and updated Runner transport.

Evidence level: unit/contract tests plus a live probe on this machine, not a
universal application or platform support claim. Native window: NetEaseMusic,
`com.netease.163music`, pid `30200`, WindowRef `5063`.

The local evidence pack is `.auv/target-input-evidence/` in this worktree. Each
JSON contains argv, exit code, stdout and stderr. Capture outputs include the
artifact URI, file path and SHA-256 of the screenshot. A machine-local index
links the important captures without committing personal desktop images. Selected direct results are
also available in the committed [execution evidence](evidence/targeted-keyboard-results.json).

- Main after #174 and #176 still rejected the supplied app target; there was no
  later keyboard-target implementation in the fetched branches/recent PRs.
- The two initial regression tests failed on target rejection and misleading
  help before production changes. A further live regression reproduced missing
  application targets being flattened to Backend before correction to NotFound.
- With the search field selected visually but the application not reliably
  foreground, baseline global key/type calls returned success without changing
  the query. Explicit activation made the same baseline Cmd+A/type sequence
  replace the query. No modifier-event change was justified by this evidence.
  The exact originally reported concatenation was not deterministically
  reproduced with confirmed foreground focus.
- `03-old-query-capture` shows `still alive`; `06-replaced-query-capture` shows
  the complete `Arielle's Wish` after Runner key/type calls. `07-submit-search`
  and subsequent captures show the search result page and the single aethoro
  result. Foreground-preferred point input selected the single-song tab.
- After an explicit pause, the single-result row's play button was clicked.
  `26-playing-confirmed` reports source `com.netease.163music`, title
  `Arielle's Wish`, artist `aethoro`, `is_playing: true`, `playback_rate: 1`.
  An immediate readback preceded the media update; later observation, not the
  click's delivery result, established playback success. The final row-play
  recheck (`45-final-playing-confirmed`) confirms the same song/source/artist
  playing from elapsed 0 after the longer validation run.
- `30-after-clear-paste-capture` shows the first paste after clearing had no
  visible effect. `33-refocused-paste-capture` shows successful Runner paste
  after explicitly clicking the editor again. No delay workaround was added.
- All three commands rejected missing applications, missing windows, and display
  targets through both local and Runner paths with the same failure categories.
  An observed Control Center menu-bar window failed exact AX resolution before
  keyboard delivery. A read-only foreground process probe remained Ghostty
  before and after that failure and successful validation-only calls.
  The Runner test also rejects a changed observed window owner before activation.
  OS activation refusal/TCC revocation was not forced on the user's desktop;
  those native guards remain distinct from the live AX-resolution failure.
- `old-runner-rejection` records nonzero exit and `unsupported` against the
  unmodified baseline Runner, with no fallback invocation.

Validation: `cargo test`; targeted invoke/CLI/macOS driver unit suites; live
NetEase dry-run and missing-application regressions; Swift bridge generation and
SwiftPM build; Buf generation and breaking check; SDK typecheck and tests
(53 passed, 1 skipped). Buf lint reports the pre-existing MoveMouse response
naming rule violation on main; this slice does not rename that RPC's response.
Clippy completed with existing repository warnings.
Final format/check/diff results and negative-case captures are in the local
validation index.

## Preparation consolidation evidence

The current implementation was checked after the owner requested activation
reuse and separation of target scope from policy. Tests first reproduced missing
CLI policy support, stale-window preparation returning success, and legacy
WindowApi text delivery accepting an invalid zero window id. Another protocol
regression caught `input-policy` being silently dropped without its serde name;
all three keyboard commands now preserve it through CLI, library and MCP inputs.

Local and Runner probes cover background key delivery with foreground ownership
unchanged, all three input commands rejecting missing targets and display scope,
exact AX-window failure, and old-Runner UNIMPLEMENTED with no global fallback.
`com.apple.systemuiserver` had no window in the recorded visible-window list;
application-target dry-runs succeeded on both routes, and a local foreground
Escape probe also succeeded. This confirms application scope no longer depends
on exact AX or visible-window resolution. It is not an OS-activation-failure test.

The updated workflow captured a concatenated old query after the first local
Cmd+A was not effective, then confirmed complete `Arielle's Wish` after the
Runner key/type sequence. Return entered the search results page; selecting the
single-song tab and row play control was followed by independent source/title/
artist/playing verification. Clear, explicit refocus and Runner application-
scoped paste also produced the complete query. The first-shortcut observation
remains an app-control consumption uncertainty, not proof of a modifier-event
bug or a completed fix for all focus races. The owner explicitly deferred the
first Cmd+A investigation to a separate follow-up; it is outside this PR. No fixed-delay workaround was added.

Current evidence and validation logs are indexed in the machine-local
`.auv/target-input-evidence/refinement/INDEX.md`. Default Cargo tests, focused
CLI/invoke/driver suites, live preparation/owner tests, Swift bridge generation,
SwiftPM build, SDK typecheck/tests, Buf generation/breaking, format/check and
Clippy completed; Buf lint retains the existing main MoveMouse response rule.

## Host integration boundary

LobeHub must retain stdout even when the child exits nonzero. Parse the JSON
failure envelope before replacing it with a generic process error; preserve
stderr separately. Do not equate process exit 0 or an input attempt's success
with semantic completion, and do not convert `verified: false` into true.

For search: select/observe the app and editor, use an application/window target
for every keyboard call, refocus after clearing when necessary, verify the
complete query, submit, observe the result, activate its play control, and
verify source application, title, artist and playback state. Coordinate input
uses logical points, not arbitrary resized screenshot pixels.

The host's stdout-loss issue is outside AUV. This task changes no LobeHub code
and publishes no release. A candidate next slice is preserving typed failure
categories through the remaining non-keyboard helpers; that work requires its
own owner-approved producer/consumer scope.

## Keyboard hierarchy validation (2026-09-08)

[Selected execution records](evidence/keyboard-hierarchy-results.json) retain
command IDs, run IDs, typed results, and failure progress. The small
[AppKit fixture](evidence/keyboard-fixture.swift) exposes application-owned text
and submit-count readback through a JSON file; it is a validation tool, not a
new AUV app workflow. It was compiled into a temporary .app with bundle ID
`ai.moeru.auv.keyboard-validation-fixture` and received the output JSON path as
its first process argument.

- Controlled live macOS result: initial `0123456789`, three local Delete presses,
  then a Runner sequence with two Deletes, Unicode `XY`, and Return produced
  `01234XY` and exactly one submission. Application observation was independent
  of the driver results; all delivery results remained unverified.
- Ten local/Runner invalid-request probes (missing app/window, display target,
  invalid later key, zero repetition) retained the expected failure codes and
  left the fixture text/submit count unchanged. Later-action failure retained
  index 1 and no completed actions, proving that the prefix was not delivered.
- Background-only Escape to NetEaseMusic preserved the independently observed
  foreground app on both local and Runner routes.
- NetEaseMusic independently showed local triple Delete changing `0123456789`
  to `0123456`. After clearing and explicitly refocusing the search box, Runner
  Unicode input showed the complete `Arielle's Wish`.
- NetEaseMusic also showed physical letters entering IME composition and some
  mixed-action submissions with no matching text effect. A subsequent search
  submission did not establish the expected results page in this probe; page
  and playback changes were observed and concurrent interaction was not ruled
  out. These observations do not establish an event-timing, modifier, or
  activation root cause. The earlier playback workflow above is historical
  evidence, not a new end-to-end pass for this hierarchy revision.
- One clear-button click failed during activation confirmation with "target is
  not a running application", although a fresh process/window observation
  still found the target. No click was delivered on that failure; a separately
  re-resolved attempt succeeded. This false-negative observation is retained,
  not hidden behind a retry or fixed-delay patch.

Automated coverage additionally injects failure at the native event boundary
to prove that later actions stop and completed repetition/action counts survive.
Hold/down-up cancellation, IME/control-effect guarantees, and the originally
reported ineffective first Cmd+A remain follow-ups.

The CLI root also derives selected keyboard dry-run routing from the registered
OptionalKeyboard contract. A former three-command allowlist omitted new commands
and let their dry-runs succeed locally against an old Runner. A subprocess
regression using an unavailable selected endpoint reproduces that error and
requires all key/chord/sequence dry-runs to use the selected Runner. This bug
affected dry-run routing; non-dry invocations already used the selected Runner.

After the dry-run routing fix, a live unmodified main Runner returned
`unsupported`/UNIMPLEMENTED for the new InputKeyboard request, with command ID
and failure JSON preserved. It did not fall back to a local or global action.

Final checks: Rust formatting/check/default suite and the focused driver, invoke,
Runner, and Rust SDK suites passed (287 focused tests; platform probes separate).
The CLI subprocess routing regression, live stale-window preparation rejection,
and live changed-owner Runner rejection passed. Bridge generation and the native
SwiftPM build passed. SDK typecheck and tests passed (53 passed, 1 skipped).
Clippy completed with existing repository warnings. Buf generation and breaking
checks against main passed; the changed input schema formats cleanly. Repository
Buf lint still reports the existing MoveMouseStreamResponse name, and repository
format diff still reports unchanged reflection-option ordering.
