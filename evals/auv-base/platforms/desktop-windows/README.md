# Windows desktop evaluations

Opt-in evaluations of the Windows driver acting on windows the user is not looking at.
They need an interactive desktop and open small windows of their own; they never take the
foreground, move the pointer or send input outside the windows they own, and they check that
independently of what the driver reports.

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
```

Each recipe builds in release mode first, because the latency figures need it. Choose a fresh
output directory every time. Records stay in the ignored `results/`. `bench --ops click` or
`--ops scroll` sends only one of the two operations per window.

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
