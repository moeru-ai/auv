# Windows GUI workflow: evidence levels

Status: **all three routes passed two observation and two functional rounds on
one isolated GitHub-hosted Windows desktop** at source commit
`178433f28189ed4ea5251d19d5e2b13f15800084`. This is evidence for that Windows
Server 2025 environment, not a claim about all Windows machines or setups.

The scenario itself lives in
[`evals/auv-base/platforms/desktop-windows/tasks/gui_workflow.rs`](../../../../evals/auv-base/platforms/desktop-windows/tasks/gui_workflow.rs)
and is documented in the
[platform README](../../../../evals/auv-base/platforms/desktop-windows/README.md).

## Verified (2026-10-24, base `main@739fe399311ae72b77ffeea530b231c3bb6c34ea`)

| Check | Result |
| --- | --- |
| `cargo fmt --check` | pass |
| `cargo check --locked -p auv-base-evals --bins --tests` | pass |
| `cargo test --locked -p auv-base-evals --lib desktop_windows` | 13 passed, 0 failed (includes the workflow's bounded-CLI-timeout test `an_unresponsive_cli_cannot_wait_forever`, which spawns a real sleeping subprocess and asserts the 100 ms deadline) |
| CLI surface the scenario drives (`serve --local-socket`, `run create/stop/get --outcome --endpoint --json`, `--run`, `invoke --no-overlay --store-root`, `input.scroll --settle-ms`) | verified present in the `739fe399` clap definitions |
| `--observe-only` code review | sends no input: it stops after `window.capture` and two `screen.findText` calls; click, typing, scrolling, submit, the decoy control and pointer/foreground restoration are all unreachable in that mode |
| Record contract: `functional_passed` semantics | `observation_passed`/`functional_passed` cover only the scenario body; the overall `passed` additionally requires the quiet tally, runner and fixture cleanup, and pointer/foreground restoration. Stated at the summary write site and in the platform README; consumers must gate on `passed` |
| Record contract: setup failures | a fixture failure after `workflow.jsonl` is created writes a `setup_failed` entry plus a `passed: false` summary whose structured `cleanup` fields separate verified receipts from unknown drop-only attempts and resources never created; when the record medium is usable and normal error handling can run, the file ends with an explicit outcome (a forced process kill or an unwritable volume can still truncate it). Global misconfigurations before the record exists still stop the run with a process error. Statically verified, with two no-desktop unit tests: a daemon-spawn failure records `not_created` resources, and a half-started daemon (spawned process that never becomes a daemon) is stopped with a receipt and recorded `verified` with `daemon_exited: true` |
| Lifecycle boundaries (from review) | `Runner::start` returns the half-started daemon for a receipt-bearing stop instead of dropping it unrecorded; on a post-spawn setup failure the foreground is explicitly handed back through the still-living receiver (recorded with the receiver's answer) before the receiver is closed, with `Restore::drop` kept as backstop; `spawn_detached_with` terminates the freshly spawned receiver process on every early return after process creation (the piped `single --piped` path keeps the same gap, marked with a NOTICE as a deferred slice). The foreground-restore ordering and the detached-spawn termination are statically reviewed, not unit-tested: both need a real receiver window, which no desktop-free test may create |

The runner route's daemon call chain was adapted to the current CLI surface:
the daemon binds a per-run private named pipe via `serve --local-socket <name>`
(the pre-#395 `serve --listen` flag no longer exists), while clients keep dialing
the same pipe through the `npipe://./pipe/<name>` endpoint URI the parser
accepts. The daemon's default local auth method stays `os-user`; the harness
neither supplies nor bypasses credentials. Unmerged PR #406 (`fix(auv-cli):
carry --credential-stdin through selected invokes; keep the SDK owner endpoint a
URI`) touches the invoke runner plumbing but changes no endpoint format or
`--run` behavior for the credential-free path this scenario uses; it is a
monitored dependency, not a blocker.

## Follow-up after review

The workflow now passes a `--within LEFT,TOP,RIGHT,BOTTOM` fence to local and
Runner `screen.clickText`. Each path checks the **fresh OCR selection** before
pointer delivery instead of relying only on the workflow's post-click check.
The direct Rust route checks the same rectangle. This adds a narrow optional
Invoke CLI input; older binaries without `--within` cannot run the new workflow.
The fence is not an atomic OS focus/occlusion guarantee, so functional runs
still require an isolated desktop. The static checks in the table above predate
this follow-up; its later desktop evidence is recorded below.

## Isolated-desktop runs (2026-10-27)

The three routes (`rust`, `invoke`, `runner`) each execute the same record-entry
workflow and produce their own `workflow.jsonl`; one route's success never
stands in for another's. On 2026-10-24 the available desktop had the user's own applications running
(browser, messaging, music player, game launcher), so **no workflow command was
executed on that machine** — not even `--observe-only`, which still creates
topmost receiver windows, captures the whole display as its OCR source, and
installs the desktop witness hooks. Running it would have interfered with the
user's session and produced inconclusive evidence the moment the user moved the
mouse.

On 2026-10-27, a separate GitHub-hosted `windows-2025` runner checked out the
exact source commit above. Its preflight recorded Windows build 26100, session
2, `UserInteractive=true`, an accessible input desktop, and one 1024×768
display. The only listed foreground-capable processes before the run were
Explorer and the hosted runner's Windows Terminal. The runner was separate from
the user's desktop. The [workflow run](https://github.com/moeru-ai/auv/actions/runs/40435530446)
contains preflight/build logs and the
[`workflow.jsonl` artifact](https://github.com/moeru-ai/auv/actions/runs/40435530446/artifacts/12704745253)
(SHA-256 `53770f9e9227e1701905cac3cb843383781e4438618c60176be5b565c604881a`,
retained by GitHub until 2026-11-10). The commands were equivalent to the
following, using the built release evaluation binaries and a debug `auv.exe`,
in this order and with separate fresh output directories:

```sh
just eval windows workflow --output <fresh> --auv <auv.exe> --rounds 2 --observe-only
just eval windows workflow --output <fresh> --auv <auv.exe> --rounds 2
```

| Route | Observation rounds 1 / 2 | Functional rounds 1 / 2 |
| --- | --- | --- |
| Direct Rust | passed / passed | passed / passed |
| Local Invoke | passed / passed | passed / passed |
| Selected Runner | passed / passed | passed / passed |

Each of the twelve final summaries reports `passed: true`, with zero quiet
violations and zero inconclusive quiet observations. Observation runs report
`no_global_input: true`. Every functional run verified the receiver's complete
text selection, exact typed text, a scroll from line 0 to 10, exactly one
submission with the intended text, two captures with matching painted state and
window identity, and both wrong-focus and wrong-window rejections before key
dispatch. Both OCR queries were found on the first attempt in every run. All
functional summaries report pointer and foreground restored; fixture cleanup is
verified for every route and mode. The Runner cleanup receipts in both modes
show `run_stop_exit: 0`, `owned_tree_kill_exit: 0` and `daemon_exited: true`.
The functional phase itself expects injected input and foreground changes; its
phase-wide witness can therefore report `violated` even when the separately
required quiet operations and overall summary pass.

### Failed baseline and selection fix

An earlier [run](https://github.com/moeru-ai/auv/actions/runs/40433335220) of
`225f53021b18ddef81758733861d17d0b83a5651` passed all six observation rounds
but failed all six functional rounds: the intended text was inserted into the
original caption instead of replacing it. In a separate
[diagnostic run](https://github.com/moeru-ai/auv/actions/runs/40434562900),
the receiver recorded Ctrl-down, A-down, A-up and Ctrl-up while its focused
native single-line EDIT still reported an empty selection. `Home` followed by
`Shift+End` selected the complete caption in all six rounds; the fixed scenario
reads `EM_GETSEL` and refuses to type unless the expected selection is present.
The diagnostic run is not the exact final code: it included an extra Ctrl+A
probe. The twelve final results above are from the later exact-source run.

The Windows driver's `type_text` and `paste_text` `replace_existing` paths also
send Ctrl+A and Backspace. This observation raises a separate compatibility
risk for that production behavior on this kind of control; those paths were not
exercised as `replace_existing` in these workflow runs and need their own
reproduction and fix assessment.

## Evidence level summary

| Route | Implemented | Statically verified | Desktop-verified on the stated runner |
| --- | --- | --- | --- |
| Direct Rust | yes | yes | 2 observation + 2 functional rounds passed |
| CLI Invoke | yes | yes | 2 observation + 2 functional rounds passed |
| Selected Runner | yes (adapted to `--local-socket`) | yes | 2 observation + 2 functional rounds passed |

This does not establish behavior on the user's Windows desktop, on other display
scales or layouts, or under concurrent user activity. Focus, window position and
occlusion can still change between checks and OS input delivery.

## User interruption review

The original acceptance gate could pass after a physical key event.
The ordinary quiet witness reports `Held` if that event changes neither focus nor cursor position.
The workflow now rejects physical mouse or keyboard input across the full witnessed case.
It records `user_interrupted: true`, stops before later input, and does not start later rounds.
If setup fails before the witness starts, `user_interrupted` is `null`.
It also skips pointer and foreground restoration after physical input.
Cleanup still closes owned processes and removes the temporary capture store.
The input guard is not atomic with an operation already in progress.

A no-desktop regression test uses witness snapshots for both physical input types.
The test also covers observation and functional acceptance without user input.
The original test-only reproduction failed before the fix.
The isolated-desktop results above apply to commit `178433f`.
They do not validate the later interruption changes on a live desktop.

## Receiver cleanup review

A detached receiver can start before its handshake fails.
The old setup summary recorded `not_created` for that failure.
The summary now records `unknown` because the early termination has no receipt.
The receiver also verifies process exit after forced termination.
A no-desktop test opens a wait-only process handle that cannot terminate the process.
The original test-only reproduction returned success before the fix.

## Runner cleanup review

If `taskkill` failed to end the daemon, the old cleanup waited without a deadline.
The tree-kill command and daemon exit wait now have deadlines.
The harness attempts root-only termination if the tree kill fails or the daemon remains alive.
The cleanup receipt records that fallback and does not claim that the descendants exited.
A no-desktop test used a substitute command that reported success without terminating the daemon.
The original test-only reproduction waited for the sleeping process and failed.
