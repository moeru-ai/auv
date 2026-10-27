# Windows desktop evaluations

Opt-in evaluations of the Windows driver acting on windows the user is not looking at.
They need an interactive desktop and open small windows of their own. Background scenarios
request no global input and independently record foreground, pointer and input disturbances;
an application's launch can still activate a window and is a separate finding. `foreground`
and the full `workflow` scenario deliberately use the real pointer and keyboard on owned windows.

```text
test-objects/win32/receiver.rs   A native Win32 window in a process of its own: a push button, a
                                 scrollable edit and a label, built from the classic controls real
                                 applications use. It reports what the application itself processed
                                 and paints its click count and scroll line as a bit strip, so a
                                 capture can be decoded and compared with the truth.
test-objects/web/receiver.html   A page for the browser rows of the application survey. It puts its
                                 own click count and scroll offset in its title, which the browser
                                 copies into the window title.
tasks/watch.rs                   Independent witness of the desktop: low-level hooks count injected
                                 and physical input, a WinEvent hook records foreground changes.
                                 It keeps no key codes, characters or coordinates; for each
                                 foreground change it keeps the owning process and window class,
                                 for each injected event the injector's tag.
tasks/receiver_process.rs        Starts receivers, detached from this process tree by default.
tasks/records.rs                 One JSON object per line.
tasks/background.rs              `single`: one background window.
tasks/sustained.rs               `five`, `bench` and `faults`: five windows through one session.
tasks/probe.rs                   `probe`, `freshness` and `geometry`: diagnostics behind the
                                 findings; `quiet`: the witness alone, for injected input from
                                 other programs.
tasks/window_commands.rs         `window`: the `window.*` invoke commands, as the real `auv` binary, against a
                                 covered receiver and judged against what the receiver knows.
tasks/screen_commands.rs         `screen`: `screen.findText` and `screen.waitForText`, as the real `auv` binary, against an
                                 uncovered receiver, placed against the controls that draw the text.
tasks/foreground_commands.rs     `foreground`: `display.*`, `screen.captureRegion`, `input.moveMouse`,
                                 `input.clickPoint`, `input.drag`, the keyboard commands and
                                 `input.pasteText`, as the real `auv` binary, judged by the
                                 receiver's text box, key log and drag pad (`--foreground-input`).
tasks/apps.rs                    `apps`: real applications, one per toolkit, covered and in the
                                 background.
tasks/background_session.rs      The runner binary, `background-session-eval`.
```

## Run

```sh
just eval windows single   --output evals/auv-base/results/windows-single
just eval windows five     --output evals/auv-base/results/windows-five
just eval windows bench    --output evals/auv-base/results/windows-bench --minutes 10
just eval windows faults   --output evals/auv-base/results/windows-faults --minutes 10
just eval windows probe    --output evals/auv-base/results/windows-probe
just eval windows freshness --output evals/auv-base/results/windows-freshness
just eval windows geometry --output evals/auv-base/results/windows-geometry
just eval windows apps     --output evals/auv-base/results/windows-apps
just eval windows quiet    --output evals/auv-base/results/windows-quiet --minutes 30
just eval windows window   --output evals/auv-base/results/windows-window --auv target/debug/auv.exe
just eval windows screen   --output evals/auv-base/results/windows-screen --auv target/debug/auv.exe
just eval windows foreground --output evals/auv-base/results/windows-foreground --auv target/debug/auv.exe
just eval windows workflow --output evals/auv-base/results/windows-workflow --auv target/debug/auv.exe --rounds 2
just eval windows workflow --output evals/auv-base/results/windows-observation --auv target/debug/auv.exe --rounds 2 --observe-only
```

Each recipe builds in release mode first, because the latency figures need it. Choose a fresh
output directory every time. Records stay in the ignored `results/`. `bench --ops click` or
`--ops scroll` sends only one of the two operations per window.
`window` runs a built `auv` (`cargo build -p auv-cli`; a debug build is enough) and removes the
invoke store it records into, because `window.list` records every window on the desktop.
`screen` reads the whole primary display, so each command keeps a screenshot of the desktop as its OCR
source. The runner removes that store however it ends and never opens it. It stops before running
any command when another window covers the receiver, because text under a window cannot be read.
`screen` and `foreground` start their receiver topmost (`--topmost`). A detached receiver holds no
foreground right, so it opens under the foreground window, and Windows ignores a later
`HWND_TOPMOST` for it; only `WS_EX_TOPMOST` at creation works.
`foreground` moves the real pointer and sends real keys, so run it only on a desktop you can leave
alone for a minute. It checks that every point it aims at belongs to its receiver, waits until the
mouse and keyboard have been idle for two seconds before each command that acts, sends keys only
while its own text box has the focus, and puts the pointer and the foreground back at the end. Its
text box has no input method by default, because an active IME composes key presses instead of
typing them; `--keep-ime` leaves the desktop's input method on to reproduce that, and records the
text instead of judging it.
`foreground` also pastes, so it borrows the clipboard. It takes a snapshot of the clipboard (every
format the driver can copy) before it starts and gives it back however it ends. Before the paste
steps it puts its own text on the clipboard, then checks after each paste that the clipboard holds
that text again. Its records hold only whether the clipboard came back, never what was on it.

## Complete workflows

`workflow` runs the same owned Win32 record-entry workflow through direct Rust APIs, local
Invoke, and a real daemon-owned Runner selected by `--run`: resolve and capture the window,
OCR the input caption, click, verify foreground and control focus, select and type text,
scroll a separate control, OCR-click Submit, then check the actual submitted record and
decode the painted state strip from a final capture. Receiver setup starts with a nonzero
calibration count; it is never counted as delivered input. Every real submission must add
exactly one click. The fixture's `submitted_text` is copied from the control by `BN_CLICKED`.

No input action is retried. OCR readiness gets at most three observations. Before dispatch,
OCR must select a single candidate inside the expected owned control, the user must have
been idle for two seconds, and keyboard input requires the correct foreground and control
focus. Every workflow `screen.clickText` also passes `--within LEFT,TOP,RIGHT,BOTTOM`:
local Invoke and selected Runner reject a re-resolved OCR action point outside this owned
control before sending input, and the direct Rust route checks the same fence. The returned
click point is still checked independently. Wrong-focus and wrong-window controls
must reject keys before dispatch and leave both applications' text/key logs unchanged.
These checks are not an atomic OS focus lease; a concurrent desktop change can still race
the precheck. Foreground actions are expected to change input/pointer state and are recorded
separately from operations required to be quiet. A quiet violation or inconclusive operation
prevents overall acceptance. Pointer and foreground restoration are checked after the full run.
The summary's `functional_passed` (and `--observe-only`'s `observation_passed`) covers only the
scenario body; the overall `passed` additionally requires the quiet tally, runner and fixture
cleanup, and pointer and foreground restoration. Consumers must gate on `passed`: a true
`functional_passed` beside a false `passed` is a workflow whose steps ran but whose desktop or
cleanup contract failed. A fixture or environment failure after the record file exists is
written as a `setup_failed` entry plus a `passed: false` summary whose structured `cleanup`
fields separate verified receipts from unknown drop-only attempts and resources never created.
So, whenever the record medium is usable and normal error handling can run, `workflow.jsonl`
ends with an explicit outcome; a forced process kill or an unwritable volume can still truncate
it. Global misconfigurations (output directory, missing `auv`, temp space) still stop the run
with a process error before any route runs.
The click fence does not make OCR and native input atomic: a window may move, focus may
change, or an occluder may appear after the final check. All functional workflow runs must
therefore still happen in a dedicated, isolated Windows session with no user applications open.

`--observe-only` exercises resolution, owned-window capture, OCR coordinates and Runner
lifecycle, with no input, decoy activation, clipboard access or pointer/foreground restoration.
Its `observation_passed` is separate from desktop quietness; `functional_passed` remains null.
It cannot pass the full workflow or its wrong-focus/wrong-window controls. Choose a fresh
output directory; an existing directory is rejected. Temporary desktop captures are removed
on normal/error return. Run stop and private daemon process-tree termination are recorded
before store removal; serve logs remain outside the temporary store. No default daemon or
paired Device configuration is changed. If the harness process itself is force-killed, its
detached receiver windows can outlive it and must be closed by hand in that session.

The [2026-10-24 evidence](../../../../docs/ai/references/driver/2026-10-24-windows-gui-workflow-evidence.md)
records measured effects and the per-route runs behind this evaluation.

## How to read a result

- **Held, inconclusive, violated.** Each operation is judged from what the desktop did around it
  and between it and the next one. `violated` means injected input, or a foreground or cursor
  change that the user's own input does not explain. `inconclusive` means the foreground or
  cursor changed while the user's hardware input was also active, which cannot be attributed to
  the driver; injected input is still checked. Run with your hands off for the cleanest result.
  A violation names who took the foreground (a target's process, the harness, or another
  process) and lists injected events, so one caused by another program can be told apart.
- **A bench interrupted from outside ends early.** Nothing in `bench` minimizes or closes a
  receiver. When one answers `skipped_minimized` or `target_lost`, someone else did it (for
  example the user's show-desktop), so the run stops and records an `interrupted` entry with the
  user's input, injected input and foreground changes from the three seconds before it.
- **Delivery is not effect.** The receiver reports what it processed. A click counts only when its
  button reported `BN_CLICKED`; an observation counts only when the decoded frame equals the
  receiver's own state.
- **An effect check must be able to see the effect.** A covered Chromium window stops painting, so
  its frames show "no change" whatever the page did. Judge an effect by the application's own
  state (a receiver's counts, a page title the page sets) or by frames from a window known to
  paint, and pair a "no effect" result with a foreground control at the same point. A frame hash
  from a window that may not paint is not evidence of a drop.
- **Runs act only on windows they created.** Every Windows eval and every probe or bench outside
  `evals/` works on receivers it starts or applications it launches itself, never on a window the
  user has open: no input or activation there, and no records of the user's window titles, tabs,
  tracks or clipboard. A run that can only act on the user's own session, such as media control
  of what they are playing, needs the owner's approval for that run. `apps` skips an application
  whose process is already running for this reason.
- **Receivers start detached.** Whether a window may take the foreground is decided from its
  process. A receiver that is a descendant of the foreground application can keep a right that an
  independently launched application does not, so the harness starts receivers with the shell as
  their parent, as an application the user launched would be. `single --piped` starts one the
  other way, as the control that shows the difference.
- **A fresh receiver may take the foreground once.** Windows can let a process it has just
  launched take the foreground on its first activation. A receiver that finds itself the
  foreground window hands it back at once, counts it (`foreground_taken`, `foreground_yielded`)
  and, if it was placed under ordinary windows, goes back there, since becoming the foreground
  window raised it. Every scenario clicks each receiver once before the measured run and records
  what happened in `warm_up`, so a launch grant is never charged to the session; any takeover
  after that fails the run, even though the receiver handed the foreground back.
- **Applications are a finding, not a pass condition.** `apps` passes when the desktop was never
  disturbed. What each application did with the input is recorded per operation: whether the
  frames changed, judged against a control wait with nothing sent, and, for the Edge page, the
  page's own counts. An application is skipped when the user already runs it, because a launch
  could reuse their window. Edge runs in a fresh profile with implicit sign-in turned off, and its
  saved frames leave out the toolbar. Notepad is not surveyed, because it can reopen the user's
  unsaved documents.
- **Unsupported is a result.** Background keyboard input has no route on Windows. The scenarios
  check that it is answered `unsupported` and that nothing reached the foreground.
