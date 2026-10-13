# Windows app activation and permission probe: driver trials

Status: probe record for #350 ([#211](https://github.com/moeru-ai/auv/issues/211)). It replaces
`app-management.md`, which #350 merged at the repository root (recoverable at `dc38e238`). The
[consolidation review](../invoke-cli/2026-10-13-windows-parity-consolidation-review.md) has the
full reasoning.

> **Migration notice:** the activation contract replaced `ApplicationControl::activate_process_name`
> and `ProcessActivationResult` with `WindowsDriverSession::applications().activate`, which returns
> the shared `ApplicationActivationResult`, and wired `app.activate` on Windows (cell `Supported`).
> The harness now calls the new API. The trials below are kept as recorded.

The trials call the driver directly: `WindowApi::activate`,
`ApplicationControl::activate_process_name` and `probe_permissions`. They do not run
`auv invoke app.activate` or `app.probePermissions`, which both refuse on Windows. The harness is
the ignored live test `crates/auv-driver-windows/tests/app_management_parity.rs`. Its targets are
two windows that the test creates ("Surface A" and "Surface B").

Records: [`evidence/2026-10-13-windows-app-activation-probe/trials.jsonl`](evidence/2026-10-13-windows-app-activation-probe/trials.jsonl),
12 trials on one Windows 11 machine on 2026-10-13.

## Cells: unchanged

- **`app.activate` stays `Unsupported`.** Its note, "the Windows driver can activate a process by
  name, which this command does not use", is accurate. Wiring needs the result-type decision that
  `TODO(app-activate-windows-cli)` names (`crates/auv-cli-invoke/src/commands/app.rs`): the
  command returns the bundle-ID shaped `ApplicationActivationResult`, and the Windows driver
  returns `ProcessActivationResult`.
  - Once there is a shared activation result, the cell can be `Supported`.
  - The foreground lock and the restore of a minimized window are runtime limits for the support
    matrix, not gaps in the path. macOS activation can be refused at runtime too.
  - `--target app:<name>` would mean the executable name, as it already does for the window and
    input commands.
- **`app.probePermissions` stays `Unsupported`.** The shared `PermissionProbe` has four macOS
  fields (`auv-driver-common/src/permission.rs`). `WindowsPermissionProbe` has three different
  ones: elevation, UIAccess and an interactive session. Linux, which projects its probe onto the
  shared shape, is also `Unsupported`.
  - A cross-platform readiness command would be a new contract, not this cell.
  - Windows does have a permission gate that matters for input. Under UIPI, a process cannot post
    to a window of higher integrity, and the readiness probe exists for that reason. So "Windows
    has no permission model" should not appear in public text.

## Trials

| Trials | What ran | Result |
| --- | --- | --- |
| 1, 2 | Activate a visible window under the other surface | Foreground moved to the target, in 4.7 and 5.5 ms |
| 3, 4 | Activate the window that is already the foreground | No change, in 0.5 ms |
| 5, 6 | Activate a minimized window | `ShowWindow(SW_RESTORE)` restored it, and it became the foreground |
| 7 | Activate a process name that does not exist | Error "main visible window was not found", foreground unchanged |
| 8 | Activate a handle that is not a window | `Backend` error "SetForegroundWindow was refused for the selected window", foreground unchanged |
| 9, 10 | Activate B then A at once, and A then B | The second call won both times |
| 11, 12 | `probe_permissions` twice | The same readings both times (elevated, no UIAccess, interactive session) |

- **Trial 8 answers a dead handle with `Backend`, not `NotFound`.** `activate_window` could check
  `IsWindow` first.
- **Trial 12's error text is not observed.** "app.probePermissions is only available on macOS" is
  a literal in the test, not the output of a run of the command.
- **Trials 5 and 6 label the restore `REDLINE_CONFLICT`.** That label came before the rule below,
  under which an activation may restore.
- **The game in trial 9.** Before trial 9 the foreground window was not a test surface. It was the
  user's game, and the trial took the foreground from it. No record says the owner allowed that.
  A Windows eval or probe now acts only on windows it created
  (`evals/auv-base/platforms/desktop-windows/README.md`). The harness now names only its own
  surfaces in its records (#353).

## Corrections to the original document

- **"An external game held the foreground against activation."** No record shows that. The only
  trial with the game in the foreground, trial 9, shows activation winning. Windows does limit
  `SetForegroundWindow` (the foreground lock, and UIPI against a higher-integrity foreground), but
  these trials did not observe the limit.
- **"Background channels return `skipped_minimized`."** Only the Rust background session does
  ([background session](../../../TERMS_AND_CONCEPTS.md#background-session)). The other paths
  differ:
  - direct `window.capture` refuses with `InvalidInput`;
  - direct posted clicks and wheels do not check for a minimized window at all;
  - `input.wheel` is not a command.
- **"Resolution B adopted".** The original document said this, but it was not an owner decision.
  The owner confirmed that on 2026-10-13. What the owner accepted that day is the rule as the
  consolidation review restated it: a rule about input mode, not about "explicit user intent". It
  is in [input mode](../../../TERMS_AND_CONCEPTS.md#input-mode).
