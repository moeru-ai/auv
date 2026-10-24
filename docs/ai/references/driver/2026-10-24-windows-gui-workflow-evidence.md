# Windows GUI workflow: evidence levels

Status: **static verification complete, functional runs pending**. The `workflow`
scenario is implemented and compiles; no desktop run of it has happened yet, so
its per-route acceptance is **null** and no workflow.jsonl evidence exists. This
document records what has been verified and what remains blocked, so the
scenario's claims stay at their actual evidence level.

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
| Record contract: setup failures | a fixture failure after `workflow.jsonl` is created writes a `setup_failed` entry plus a `passed: false` summary whose structured `cleanup` fields separate verified receipts from unknown drop-only attempts and resources never created; when the record medium is usable and normal error handling can run, the file ends with an explicit outcome (a forced process kill or an unwritable volume can still truncate it). Global misconfigurations before the record exists still stop the run with a process error. Statically verified, including the no-desktop unit test `a_setup_failure_is_recorded_with_truthful_cleanup_and_not_passed` |

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

## Blocked: isolated-desktop functional runs

The three routes (`rust`, `invoke`, `runner`) each execute the same record-entry
workflow and must each produce their own `workflow.jsonl` evidence; one route's
success never stands in for another's. The execution machine available on
2026-10-24 was an interactive desktop with the user's own applications running
(browser, messaging, music player, game launcher), so **no workflow command was
executed there at all** — not even `--observe-only`, which still creates
topmost receiver windows, captures the whole display as its OCR source, and
installs the desktop witness hooks. Running it would have interfered with the
user's session and produced inconclusive evidence the moment the user moved the
mouse.

Required to unlock (run in that order, each with a fresh output directory, on a
dedicated isolated Windows session with no user applications open):

```sh
just eval windows workflow --output <fresh> --auv <auv.exe> --rounds 2 --observe-only
just eval windows workflow --output <fresh> --auv <auv.exe> --rounds 2
```

Acceptance per route: summary `passed:true` with `quiet_violated == 0` and
`quiet_inconclusive == 0`; for the functional run also `cursor_restored`,
`foreground_restored`, `handed`, verified wrong-focus and wrong-window
rejections, and a runner-route `runner_cleanup` receipt
(`run_stop_exit == 0 && owned_tree_kill_exit == 0 && daemon_exited == true`).

## Evidence level summary

| Route | Implemented | Statically verified | Desktop-verified |
| --- | --- | --- | --- |
| Direct Rust | yes | yes | **null** |
| CLI Invoke | yes | yes | **null** |
| Selected Runner | yes (adapted to `--local-socket`) | yes | **null** |

Until the blocked runs happen, the workflow scenario must not be cited as
desktop-verified end-to-end regression coverage.
