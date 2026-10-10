# Windows background session: five windows, no foreground

Request: the plan "Sustained multi-window computer use — final requirements and
implementation plan" (requirements R1 to R7, three phases), filed as epic
[#327](https://github.com/moeru-ai/auv/issues/327). Related: #230 (the frozen single-operation
baseline), #275 (WGC health cache), #328 and #330 (the live overlay, which visualizes and never
proves success).

Goal: one Windows driver session that operates up to five windows the user is not looking at,
for a long time, without taking the foreground, moving the real cursor or sending global input,
and that says plainly what it could not do.

Status: on branch `feat/windows-background-session`. Evidence level `live-validated` on one
machine, for the windows and applications named in [Evidence](#evidence); not `supported`.
The public claim is in [`SUPPORT_MATRIX.md`](../../../SUPPORT_MATRIX.md).

## Contract

```text
caller discovers windows once and attaches up to five (BackgroundSession::attach)
  -> every operation re-validates its target with one handle probe, never an enumeration
  -> click / scroll: a posted WM_* message to the control under the point
       (no SendInput, no activation, no cursor move, no foreground route to fall back to)
  -> capture: the target's frame stream, newest frame only
  -> typed Outcome: done | skipped_minimized | target_lost | unsupported | failed
  -> delivery is not effect: whether the application acted is a separate check
```

`WindowsDriverSession::background()` opens a `BackgroundSession`. Its vocabulary is in
[`TERMS_AND_CONCEPTS.md`](../../../TERMS_AND_CONCEPTS.md#background-session).

| Operation | Answer |
| --- | --- |
| `attach(&Window)` | A `TargetId`, or `AttachError` (full at `MAX_TARGETS` = 5, already attached, gone). Caches handle, process, thread and class. |
| `click(id, WindowPoint, ClickRequest)` | `Done { Delivery }` with `InputActionResult` (`WindowTargetedMouse`, `verified: false`) and the frame the point was resolved against. A modifier the posted route cannot carry is `Unsupported`. |
| `scroll(id, WindowPoint, Scroll)` | The same, with `WindowTargetedWheel`. |
| `type_text`, `press_keys` | Always `Unsupported`, nothing sent (`TODO(windows-targeted-keyboard)`). |
| `capture(id)` | `Done { FrameRead::Fresh { capture, frame_number, followed } }` when the window changed since the last read, `Done { FrameRead::Unchanged { frame_number } }` (no pixel copy) when it did not. |
| `refresh()` | Every target's state, without operating on any. |
| `stats()` | Probes, deliveries, fresh and unchanged reads, streams opened, reopened and resized, frames the compositor delivered. |

Before each operation the session probes its target:

- **Gone** (the handle no longer names a window, or names one of another process, or a window of
  another class): `target_lost` with a `LostReason`. Final: a reused handle must never be mistaken
  for the target. Its stream is closed.
- **Minimized**: `skipped_minimized`. Windows stops rendering a minimized window, so nothing sent
  to it could be seen. The session never restores it.
- **Moved or resized**: points are resolved against the live frame and the delivery reports
  `followed`.

The redlines of the plan are structural here, not conventions: the session has no activation
call, no `SendInput` path and no input mode, so "never fall back to the foreground" cannot be
violated by a caller's options.

### Frame stream

Each target gets its own Windows Graphics Capture session, drained on every `FrameArrived`: the
stream keeps only the newest frame on the GPU and counts the frames the compositor delivers. A
read copies pixels only when the counter moved since the last read, so a window that stands still
costs one handle probe and no copy. Switching between targets never renegotiates a capture
session.

When the captured size changes, the stream stops with `StreamError::Resized` and the session opens
a new one (`streams_resized`); a window that keeps changing size answers `failed` after three
replacements in one read. Recreating the frame pool in place was measured to keep the old origin
after DWM changed the window's frame bounds, so frames no longer lined up with the bounds they were
reported with (see [Findings](#stream-frames-misaligned-after-a-resize)).

Reading is polling: `capture` answers at once, and a caller that waits for an effect reads again
after a short sleep (the harness sleeps 250 µs). A blocking wait for the next frame is deferred
(`TODO(background-session-wait-for-frame)`).

## Results by requirement

All figures are from the committed [evidence](#evidence), recorded on 2026-10-11 while the user
worked on the same desktop. "Held" means the witness saw no change in the foreground window,
the cursor or the input stream around an operation; "inconclusive" means the foreground or
cursor changed while the user's own hardware input explains it; "violated" means a change that
nothing the user did explains, or any injected input.

- **R1, five background windows without focus theft.** `five` ran 425 operations over five
  receivers that were fully covered (`occluded_fraction` 1.0), all through one session: 372
  held, 53 inconclusive (all `cursor_moved` while the user moved the mouse), 0 violated. The
  foreground window was the same before and after every operation and every gap between
  operations, and no receiver took the foreground after its warm-up. `faults` ran 9,550
  operations for ten minutes with 0 violated and no violated gap; the foreground changed in 32
  gaps between operations, each explained by the user's own input. The
  three `bench` runs held the foreground for all 3,756, 3,841 and 3,507 rounds; their violated
  rounds are under R2. In the four `split` runs the foreground moved three times, each time to
  another program's window and never to a receiver
  ([finding](#foreground-changes-the-witness-cannot-attribute)).
- **R2, ten minutes without degradation.** Three runs of `bench`, five receivers each, every round a
  click, an observation until a frame showed it, a scroll and another observation per window:

  | Run | Ops/min, each minute | Last / first | Click P50 | Observe P50 / P95 | Witness |
  | --- | --- | --- | --- | --- | --- |
  | `bench-10min.jsonl` | 7,406 to 7,725 | 0.97 | 0.021 to 0.022 ms | 14.1 to 14.7 / 16.2 to 17.5 ms | 3,390 held, 364 inconclusive, 2 violated (injected keys, minute 9) |
  | `bench-10min-first.jsonl` | 7,456 to 7,852 | 1.01 | 0.022 to 0.023 ms | 13.1 to 14.3 / 16.4 to 17.7 ms | 2,861 held, 979 inconclusive, 1 violated (`cursor_moved`, minute 1) |
  | `bench-10min-kinds.jsonl` | 6,888 to 7,227 | 1.00 | 0.020 to 0.039 ms | 13.8 to 14.9 / 19.9 to 23.8 ms | 2,305 held, 1,200 inconclusive, 2 violated (injected keys, minute 6) |

  Every one of the 111,040 observations showed its action. Private memory settled from minute 4
  (61 to 63 MB), handles stayed at 514 to 530, GDI objects at 4. The throughput criterion holds in
  all three runs, yet each run exits `FAIL`: the witness failed 5 of the 11,104 rounds, and one
  violated round fails a run. None of the five changed the foreground window. In two
  runs another program pressed and released one key through injected input; the session's input
  path cannot inject, and the source is unknown (see
  [Injected key presses during `bench`](#injected-key-presses-during-bench)). In the
  third the cursor moved without any mouse input in the hooks' record, in a build that did not
  yet record cursor positions.

  Four more ten-minute runs split the round into click only and scroll only (`split-*.jsonl`,
  alternating, to test the finding): 6,120 to 7,140 operations a minute scrolling and 7,535 to
  9,116 clicking, last / first 0.95 and 1.03 (scroll) and 1.03 and 0.84 (click), and all 152,030
  observations showed their action. `split-4-click.jsonl` is the one run of seven that fell by
  more than 10%: from minute 7 it ran at about 7,700 operations a minute instead of about 8,800,
  while click and observe latency did not change (observe P50 10.9 to 12.6 ms throughout). The
  lost time is in the part of a round the bench does not time: reading each receiver's counters
  over its pipe and judging the desktop. The same minute brought a shell flyout, three injected
  key presses and a working-set trim of the harness (52 to 44 MB); the cause is not established.
  Two `split` runs passed the witness, the only `bench` runs that did.

  A single operation costs 0.02 ms to deliver; seeing its
  effect costs 12.3 ms P50 (P95 13.3 ms) in `five`, which is the receiver repainting and the
  compositor delivering a frame on a 180 Hz display. The frozen Warm Fast baseline (P50
  8.20 ms) measures a different operation, a QQ Music replay, and was not re-optimized; a 2
  megapixel frame read costs 2.2 ms P50.
- **R3, observing windows nobody looks at.** All five receivers were fully covered, yet every
  observation in `five`, `bench` and `faults` decoded the receiver's own state from a frame of
  the covered window. The session reads frames while windows are covered, and returns
  `skipped_minimized` for a minimized window instead of restoring it: in `faults`, window 4
  answered `skipped_minimized` 209 times and stayed minimized until the harness restored it.
  Gaps between operations are judged as well as the operations themselves.
- **R4, no rediscovery and no redundant capture.** Each run discovers its windows with one
  enumeration and never again (`window_enumerations_since_warmup` is 0 in every `bench`
  minute). A target that has not changed costs a handle probe and no copy: `five` read five
  idle windows 1,573,815 times in two seconds with no pixel copy, and the first `bench` run
  answered 787,247 reads `Unchanged` against 38,459 fresh frames.
- **R5, closed, killed, moved and resized windows.** In `faults`, the window closed at 5:00
  answered `target_lost` (`window_destroyed`) from its next operation on, 1,005 times, and the
  process killed at 6:00 did the same 814 times; windows 0 and 3 answered `done` in all 1,910
  rounds. The move at 7:00 and the resize at 7:30 were followed: the operation reports
  `followed` with the old and new frames, the stream was replaced after the size change
  (`streams_resized`), and every later click on that window still showed in a frame.
- **R6, typed answers, no silent fallback.** Keyboard operations answer `unsupported` and send
  nothing (`type_text` 20 of 20 in `five`; key press and text in `single`); a scroll that needs
  the foreground answers `unsupported`, and a click with Alt answers `invalid_input`. Every
  delivery reports `window_targeted_mouse` or `window_targeted_wheel`, `verified: false` and no
  foreground attempt, which `single`, `five` and `bench` check on every operation. Whether the application acted
  is a separate measurement, from the receiver's own counters and from frames.
- **R7, support matrix.** [`SUPPORT_MATRIX.md`](../../../SUPPORT_MATRIX.md#windows-background-session-by-application)
  has a row for the background session and a table per application, linked to this evidence.

## Findings

### Foreground: a launch grant, not the input path

A posted click makes a classic push button call `SetFocus`, which activates its top-level window
inside the application's own input queue. Whether that window then becomes the system's
foreground window is decided by Windows from the receiving process. What the receivers showed:

- **After a window's first activation, nothing took the foreground.** About 148,600 clicks and 122,400 scrolls after
  the warm-up, across `five`, the seven `bench` runs and `faults`, were each followed by a check of the
  receiver's own counter, and none took the foreground.
- **The first activation of a freshly started process sometimes did.** Windows can let a process
  it has just started take the foreground once. In these runs it happened to piped receivers
  (descendants of the foreground application) and to receivers started with the shell as parent,
  with and without user input since the start, and in other runs to none of them:

  | Runs | Piped receivers (children of the harness) | Detached receivers (shell as parent) |
  | --- | --- | --- |
  | 2026-10-10, development runs | 2 of 2 | 11 of 16 |
  | 2026-10-10, first full set | 2 of 2 | 3 of 21 (all three in one `bench` warm-up, no user input since the start) |
  | 2026-10-11, committed evidence | 0 of 2 | 0 of 26 |

  Counted from each receiver's own `foreground_taken` on its first click. The development runs
  are not in the evidence folder.

  The grant is spent on first use; whether a process gets one could not be predicted from
  anything the harness observes. Real applications launched by the survey showed the same thing:
  Calculator and Edge took the foreground during some launches and not others.
- **`EVENT_SYSTEM_FOREGROUND` is not a foreground change.** The event fires naming the receiver on
  every internal activation even when the foreground window does not change; the witness judges
  on `GetForegroundWindow` at the time of each event.
- **A raise without activation does not pass the foreground window.** `SetWindowPos(HWND_TOP,
  SWP_NOACTIVATE)` from the harness left Edge below a maximized foreground window. A background
  target is covered in practice, which is what the evaluations measure.

Consequence for the session: no takeover was seen after a window had been activated once, so a
window the user has already used did not show it; an application the agent launches itself may
take the foreground on the first posted click that activates it. The delivery result does not observe this yet
(`TODO(background-session-foreground-grant)`).

The receivers protect the user from it: a receiver that finds itself the foreground window hands
it back at once, goes back under ordinary windows if it was placed there, and counts it
(`foreground_taken`, `foreground_yielded`). Because a hand-back can be too quick for the desktop
witness, every scenario judges takeovers on the receiver's own counter, warms each receiver up
with one click before measuring, and fails on any takeover after the warm-up.

### Injected key presses during `bench`

The witness saw keys pressed and released through injected input in three of the seven committed
`bench` runs:

| Run | Operations | Minutes | Injected key presses |
| --- | --- | --- | --- |
| `bench-10min.jsonl` | click and scroll | 10 | 1 (minute 9) |
| `bench-10min-first.jsonl` | click and scroll | 10 | 0 |
| `bench-10min-kinds.jsonl` | click and scroll | 10 | 1 (minute 6) |
| `split-1-scroll.jsonl`, `split-3-scroll.jsonl` | scroll only | 20 | 0 |
| `split-2-click.jsonl` | click only | 10 | 0 |
| `split-4-click.jsonl` | click only | 10 | 3 (minute 7, within 11 s) |
| `faults-10min.jsonl` | click only, paced | 10 | 0 |
| `quiet.jsonl` | nothing of AUV running | 30 | 0 |

An earlier `bench` run on 2026-10-10, not in the evidence folder, saw one press in minute 7. The
`split` runs alternated scroll, click, scroll, click while the user worked.

What every press had in common:

- The injector left no tag (`dwExtraInfo` 0) and did not run at a lower integrity level. With UAC
  off every process here runs at high integrity, so the second fact narrows nothing.
- Neither the foreground window nor the cursor changed.
- Where the kind was recorded, the key was `other`. In `bench-10min-kinds.jsonl` that meant
  anything but a modifier, an input-method key or an unassigned key such as a hotkey tool sends to
  mask a released Alt or Win key. `split-4-click.jsonl` also ruled out text, editing and
  navigation, function and media keys, which leaves keys such as Apps, Print Screen, Pause, the
  lock keys and reserved or vendor codes. The witness does not record which key it was.

What differed:

- In the click-and-scroll runs, press and release were 90 to 120 ms apart, 3.0 to 4.5 s after the
  user's last physical input, with no window event near them.
- In `split-4-click.jsonl`, press and release were 0 to 2 ms apart. Two of the three came 3 and
  31 ms after `EVENT_SYSTEM_FOREGROUND` named a Windows shell flyout
  (`XamlExplorerHostIslandWindow`, another process) while the cursor was on the taskbar, 2 s
  after the user's last input; the third came 12 s after it.

Nothing on the session's input path injects input. `BackgroundSession` posts window messages
through `background_input`, and neither it, the harness nor the receivers call `SendInput`,
`keybd_event` or `SetCursorPos`. A posted message never reaches the low-level hooks, so the event
is not one of the session's own messages. What the runs say about the source:

- Scrolling is not the trigger: the scroll-only runs sent 66,740 scrolls, twice the rate of the
  click-and-scroll runs, and saw no injected key.
- Whether the session matters at all is open. All 3 episodes came in the 60 minutes of runs that
  clicked and none in the 50 minutes that did not (the scroll-only runs and `quiet`). If the
  presses had nothing to do with the session, that split would happen about one time in six
  ((60/110)³), so these runs cannot tell a trigger in the session from a rare outside source.
- The shell flyouts in `split-4-click.jsonl` suggest the Windows shell, or a tool hooked into it,
  reacting to the user's own use of the taskbar; nothing here shows it.

Until it is explained, a key press that reaches the user's foreground window while a session
runs remains an open risk to the redline that the harness must not disturb the user's desktop,
and the witness keeps failing such a run. Candidate next steps, none done here:

- Run on a desktop nobody uses, a virtual machine or a second interactive session. Every press so
  far came while the user was working, and it would also remove the changes below.
- Record the virtual key for keys of kind `other`. None of them carries text.
- An ETW trace of the input stack to name the injecting process.

### Foreground changes the witness cannot attribute

The witness credits a foreground change to the user only when a physical click or key press came
within 2 s before it. In `split-3-scroll.jsonl` it failed three rounds in which the foreground
moved to another program's window: twice to a shell flyout with the cursor at the bottom edge of
the display 300 and 420 ms after the user moved the mouse (taskbar flyouts open on hover), and
once to a Chromium-family window (`Chrome_WidgetWin_1`) 5.3 s after the user's last input. No
receiver was involved: none took the foreground in any run (`receiver_takeovers` 0, `is_target`
false). These rounds fail the run as measured; they are not takeovers by the session.

### One-shot WGC capture returns an older frame

`capture_window_wgc` reads the frame pool in order (two buffers) from one global capture session.
After an action the first read returned the window as it was before the action, and reads
converged after two or three calls; switching targets tore the session down. In `five`, after a burst of clicks the first one-shot
read showed the latest state in 1 of 10 trials, against 10 of 10 for the frame stream. In
`freshness` the first one-shot read after a click showed the settled state in 1 of 6 rounds (3 of
6 in an earlier run), and `PrintWindow` in 6 of 6. This path
is unchanged (`TODO(wgc-capture-latest-frame)`); sessions use the frame stream, and `single`
judges its capture-change check on `PrintWindow` and only records the one-shot WGC result.

### Stream frames misaligned after a resize

For a window created and never moved, DWM reports the full window rectangle as its frame bounds;
after its first move it reports the visible frame, 7 px narrower on each side. A WGC frame pool
recreated at the new size kept the old origin, so frames were offset by the invisible border.
`geometry` measures the left edge of the visible frame in each frame after a move, a move back, a
resize and another move. With the stream reopened on each size change, that edge sat at x = 0 in
every frame after the first move (x = 7 at the start, where the bounds were still the full
rectangle), so frames and bounds line up.

### PrintWindow pixels and bounds disagree

`PrintWindow` returns the full window rectangle's pixels while the capture reports the visible
frame's bounds, so a caller mapping pixels to the screen is off by the invisible border:
`geometry` got 380x300 and 460x340 pixels for bounds of 366x293 and 446x333. This is the existing
`TODO(windows-window-capture-dpi)`, unchanged here.

### Applications

The [support matrix](../../../SUPPORT_MATRIX.md#windows-background-session-by-application)
has the table; the details:

The `apps` survey launches each application itself, in a fresh profile where it has one, places
its window under a cover window the harness owns, and then works only through the session. It
judges effect from frames and, for the Edge test page, from the page's own counts in its window
title. It never operates on a window the user had open: an application whose process is
already running is skipped, and Notepad is left out because it can reopen the user's unsaved tabs.

- **Character Map** (classic Win32, owner-drawn grid): captures are live. Posted clicks reach the
  grid, but only 1 of 4 changed a frame. The cause was not investigated.
- **Calculator** (UWP, XAML content inside `ApplicationFrameHost`): captures are live. UI
  Automation located the digit buttons and the posted clicks were delivered there, but the
  display stayed 0: the XAML content does not act on posted mouse messages. UI Automation
  `Invoke` would be a different route, not part of this work.
- **Edge, default flags**: Chromium stops painting a window that its native occlusion tracking
  finds covered, so frames show the page as it was last painted. In this run the page had never
  been painted (none of its background color in any frame); in an earlier run it had painted
  before it was covered. The page still handles posted clicks without painting: its title
  counted 3 of 4, while no frame changed. The click it lost was the first after launch, on a page
  that had never painted; in the earlier run, where it had painted, it counted 4 of 4.
- **Edge with `--disable-features=CalculateNativeWinOcclusion`**: frames follow the page, and
  each of 4 clicks showed in a frame 11 to 17 ms after delivery (16 to 34 ms in an earlier run).
- **Scroll in Edge**: no posted wheel scrolled a covered Edge page, with or without occlusion
  tracking (0 of 4 each), as the 2026-10-06 scroll probe found for Chromium that is not the
  foreground window. REVIEW: Chromium may redirect a wheel to the window under the point
  (`RerouteMouseWheel`, `ui/base/win/mouse_wheel_util.cc`), which for a covered window belongs to
  another process. This was not confirmed.
- **Launch**: none of the four launches in this run took the foreground. In earlier runs
  Calculator and Edge took it during some launches. The survey records each launch's foreground
  events; an application that still holds the foreground once its window has appeared fails the
  survey and is closed.
- **Saved frames**: Edge's toolbar is cropped out, and its implicit sign-in to the Windows
  account (`msImplicitSignin`, found in `msedge.dll`) is disabled, so no account picture is
  saved.

## Limits and deferred work

- Keyboard input has no background route on Windows; a session answers `unsupported`
  (`TODO(windows-targeted-keyboard)`).
- A window DWM cloaks (on another virtual desktop) is treated as live
  (`TODO(windows-background-cloaked)`).
- No blocking wait for the next frame (`TODO(background-session-wait-for-frame)`).
- The delivery result does not observe a foreground takeover
  (`TODO(background-session-foreground-grant)`).
- The one-shot WGC capture can return an older frame (`TODO(wgc-capture-latest-frame)`).
- An unidentified program injected key presses in three of the seven committed `bench` runs
  ([finding](#injected-key-presses-during-bench)), and the foreground of a desktop the user is
  working on changes in ways the witness cannot attribute
  ([finding](#foreground-changes-the-witness-cannot-attribute)); only two `bench` runs passed the
  witness. One run of seven lost more than 10% of its throughput for a reason not established
  (R2).
- Not evidenced: UIPI (this machine runs with UAC off, every process at high integrity), WPF,
  WinForms, Qt, Electron, Java and game windows, display scaling other than 100%, more than one
  display, virtual desktops, remote or locked sessions.
- Epic #327 lists throughput and latency targets, long-session context and semantic success for
  arbitrary applications as non-goals, while the plan's R2 to R5 ask for them. This work followed
  the plan; the epic's text was not changed.

## Evidence

Machine: Windows 11 Pro 10.0.29648, one 2560x1440 display at 100% scaling and 180 Hz, UAC off
(every process at high integrity), `ForegroundLockTimeout` at its maximum. All runs on
2026-10-11, in sequence, while the user worked on the same desktop; the witness excuses a
foreground or cursor change only when the user's own input explains it, and never excuses
injected input. `single` through `faults` and `bench-10min-first` ran from one build. Later
builds changed only the evaluation harness: `bench-10min` recorded more about each violation,
`quiet` was added, `bench-10min-kinds` recorded the kind of each injected key, and the `split`
runs added `--ops` and a finer key kind. The session's Windows code is the same in all of them.

```text
cargo build --release -p auv-base-evals --bin win32-receiver --bin background-session-eval
target/release/background-session-eval <scenario> --output evals/auv-base/results/<name>
```

Files in [`evidence/2026-10-10-windows-background-session/`](evidence/2026-10-10-windows-background-session/):

| File | Scenario | What it shows |
| --- | --- | --- |
| `single-detached.jsonl` | `single` | One covered receiver started with the shell as parent: every operation of the driver's window input, with the desktop judged around each (`invariants`) and the receiver's own account of what it received (`effect`). Unsupported routes answer typed errors. |
| `single-piped.jsonl` | `single --piped` | The same with a receiver that is a child of the harness, the control for the launch grant. |
| `five.jsonl` | `five` | Five covered receivers in one `BackgroundSession`: click, observe, scroll, observe and `type_text` per window per round, the static-window, no-rediscovery, burst and large-frame checks. Operation records after round 1 are kept only for the last round and for any operation not judged `held`; the summary counts all of them. |
| `bench-10min.jsonl` | `bench --minutes 10` | Per-minute throughput, latency percentiles, effects, desktop verdicts, receiver takeovers, session counters and process gauges; a `desktop_violation` record for any round the witness failed, and an `interrupted` record if a receiver was minimized or closed from outside, which ends the run. |
| `bench-10min-first.jsonl` | `bench --minutes 10` | The run before, from a build whose violation record did not yet carry cursor positions. |
| `bench-10min-kinds.jsonl` | `bench --minutes 10` | The run after, whose violation records give the kind of each injected key. |
| `split-1-scroll.jsonl` to `split-4-click.jsonl` | `bench --minutes 10 --ops scroll` / `--ops click` | Four runs in a row, alternating scroll only and click only, with the finer key kinds; `split-runs.txt` has their verdicts. |
| `faults-10min.jsonl` | `faults --minutes 10` | Ten minutes over five receivers with a close at 5:00, a kill at 6:00, a move at 7:00, a resize at 7:30, a minimize at 8:00 and a restore at 9:00. Every operation is judged and counted in the summary; the records keep the first three answers of each kind per window, then one in a hundred, and every operation or gap between operations that the witness failed. Gaps judged inconclusive are counted by reason (`gap_findings_by_verdict`), not listed. |
| `apps.jsonl`, `apps-*.png` | `apps` | The application survey; one saved frame per application, cropped to the application's own content. |
| `probe.txt` | `probe` | Which receivers took the foreground on their first activation, piped and detached. |
| `freshness.txt` | `freshness` | Whether the one-shot WGC and `PrintWindow` captures show a click's effect on the first read. |
| `geometry.txt` | `geometry` | Stream frames against session bounds across moves and a resize. |
| `quiet.txt`, `quiet.jsonl` | `quiet --minutes 30` | Only the witness, with no receiver and no session, while the user worked: no injected input in 30 minutes. |
| `runs.txt` | all | Exit code, duration and verdict of each run, in order; `bench` there is `bench-10min-first.jsonl`, `bench-rerun` is `bench-10min.jsonl`. |

The full per-operation logs stay in `evals/auv-base/results/` and are not committed.
