# Invoke Uses The Local Runner By Default

Status: **proposal, decisions taken with the owner on 2026-11-04**; not
implemented. Names marked *provisional* may still change.

## Problem

`auv invoke <command>` without `--device`, `--device-id` or `--run` runs the
command on the direct local path: the driver runs inside the short-lived CLI
process (`crates/auv-cli/src/commands/invoke.rs:81`, `RootSelection::is_empty`
in `crates/auv/src/selection.rs:83`). MCP uses the same rule
(`crates/auv-cli/src/commands/mcp.rs:453`). With a selection, the call goes
through the daemon to the long-lived `auv.core.local` Runner. So the two paths
differ:

| | Direct path (today's default) | Local Runner |
| --- | --- | --- |
| State across calls (macOS maximize record, AX connections, portal sessions, a future window cache) | lost after each call | kept while the Runner lives |
| Trace store | `<cwd>/.auv/store`, a new Run id per call | the daemon store |
| macOS TCC attribution | the process that launched `auv` | the daemon process |
| Linux portal restore tokens | user state dir `…/portal` | `<store>/runner-state/auv.core.local/portal` |
| Windows session | wherever `auv` was launched (Session 0 over SSH: no desktop) | the daemon's session |

The JS SDK always uses a daemon. The window-management design already records
the cost of the split: a maximize and a restore in two plain CLI calls do not
pair.

## Decisions

1. **Plain `auv invoke` and `auv mcp` use the local Runner.** If no usable
   daemon exists, the CLI starts one (rules below). Same for MCP.
2. **`--no-daemon`** (*provisional*; chosen over `--direct` / `--in-process`
   for the least mental cost: it says what is skipped) keeps today's direct
   path. Commands with no Runner route (`scan.frame`, `scan.coverage`,
   `RunnerSupport::NoRoute`) and dry runs that cannot resolve a selection yet
   (`TODO(selected-invoke-dry-run)`) keep running directly without the flag.
3. **Traces go to the daemon store.** The CLI prints the Run id and the store
   path. `<cwd>/.auv/store` is used only with `--no-daemon`.
4. **An auto-started daemon exits 5 minutes after the last call**, together
   with its Runner.
5. **It registers in local discovery**, so other CLI calls and SDK clients of
   the same user (`connect({ discover: true })`) share it.
6. **`AUV_ENDPOINT` (or an explicit endpoint) wins and never auto-starts.**
7. **Windows: `auv setup windows-helper install` also registers the user's
   logon task** that runs the daemon in the interactive session; the CLI in
   Session 0 (e.g. SSH) triggers that task instead of starting a daemon that
   cannot reach the desktop. The owner lifted the earlier "no daemon
   autostart / no daemon service" deferral on 2026-11-04.

## Resolution order

```text
explicit endpoint / AUV_ENDPOINT        -> use it; never start anything
--device / --device-id / --run          -> unchanged (selected route)
--no-daemon or a NoRoute command        -> direct path (today's behavior)
reachable registered local daemon       -> use it
otherwise                               -> auto-start (below), then use it
```

A daemon that is registered but unreachable is never a reason to fall back to
the direct path silently (`TERMS_AND_CONCEPTS.md` "AUV operation interface": connection
and capability failures never cause implicit local fallback). It is either recovered (stale
socket, below) or the call fails with the reason.

## Auto-start

### macOS and Linux

- Spawn `auv serve` detached from the terminal (`setsid`; stdio to the daemon
  log under the store), registered, local listener only.
- Store root: the user's AUV state dir (`ProjectDirs("ai","moeru","auv")`),
  **not** the cwd-relative default `<cwd>/.auv/store`
  (`crates/auv-cli/src/commands/serve.rs:142`), so the daemon's store does not
  depend on where the first call ran.
- Wait until the discovery descriptor names a reachable endpoint (bounded,
  e.g. 10 s), then route.
- Concurrent first calls: the discovery lock (`api-server.lock`,
  `crates/auv-daemon/src/discovery.rs:24-54`) lets one daemon win; the losers
  wait for its descriptor instead of failing.
- `TODO(daemon-user-service-macos-linux)`: a launchd user agent / systemd
  `--user` unit (`auv daemon start|status|stop` in `TERMS_AND_CONCEPTS.md`)
  would give macOS a stable TCC identity (`auv`, not the launching terminal).
  Not in this slice.

### Windows

| Situation | Behavior |
| --- | --- |
| Interactive session (local console or RDP) | Spawn `auv serve` with `DETACHED_PROCESS` / new process group, registered, as above |
| Session 0 (SSH, services) and the logon task is installed | `schtasks /run` the task (it runs in the user's interactive session), wait for discovery, route |
| Session 0 and no task | Fail: name `auv setup windows-helper install` and `--no-daemon` |

`auv setup windows-helper install` (existing command, needs administrator)
additionally registers, for the **current user**:

- a scheduled task (*provisional* name `AUV Daemon`) that starts at that
  user's logon, runs only when the user is logged on (interactive session) and
  with **limited** privileges — never the elevated token of the setup process,
  which would break the Helper's caller-SID == target-SID rule;
- action: the `auv.exe` that ran setup, using a stable path (for Scoop the
  `scoop\apps\auv\current\auv.exe` junction, not a versioned directory) and
  `serve` with the user store, registered, local named pipe only;
- the same 5-minute idle exit; the CLI re-triggers the task on demand.

`uninstall` removes the service and the task; `status` reports both.
`TODO(windows-daemon-task-without-admin)`: a non-administrator path that
registers only the task. luoling-windows-11 already runs a hand-made task
`AuvDaemon` (`auv serve --listen npipe://./pipe/auv-owner --listen
http://0.0.0.0:9847`); replacing it needs the owner's go-ahead.

## Security boundary

The boundary stays **same OS user**, as documented: "Local Unix-socket access
is owner-checked same-user authority, not an open or anonymous daemon mode"
(`../session-api/2026-08-11-auv-js-sdk-spec.md`), and process isolation within one user is
not an authorization boundary (`../session-api/2026-07-31-device-run-runner-aggregated-api-design.md`).
A same-user process could already run `auv invoke` itself, so an auto-started
daemon grants it nothing new. It does make a daemon exist without an explicit
`auv serve`, so the auto-started daemon is narrowed:

- **Local listener only.** No remote listener, so pairing tokens that a
  same-user caller could create (`local_owner`) have no listener to use.
- **Authentication `os-user`** (the `auv serve` default,
  `crates/auv-cli/src/commands/serve.rs:56-57`): Unix peer uid must equal the
  socket owner (`crates/auv-api-server/src/server.rs:370-374`); Windows pipe
  DACL allows only SYSTEM and the user's SID, remote clients rejected
  (`crates/auv-api-server/src/windows_pipe.rs:28-46`). Other users cannot
  connect. Other logon sessions of the same Windows user are not excluded.
- **Fix: socket permission window.** `bind_unix` binds, then `chmod 0600`, with
  no umask control (`server.rs:450-458`), so the socket briefly has umask
  permissions. Create it in a `0700` directory or tighten the umask first.
- **Fix: discovery directory mode.** It is set to `0700` only when created
  (`crates/auv-daemon/src/discovery.rs:18-22`). Refuse or repair a
  pre-existing directory that other users can write.
- **Stale socket after a crash or `SIGKILL`.** A registered daemon refuses an
  existing path (`server.rs:447`). Before auto-starting, if the descriptor's
  endpoint refuses connections and the socket file is owned by the user,
  remove it and its descriptor, then start. Never remove a live endpoint.

## Lifecycle

- Today `--daemon-idle-timeout` counts only "no live Runners"
  (`crates/auv-api-server/src/server.rs:334-352`) and the local Runner idles 5
  minutes on its own, so a 5-minute daemon timeout would exit about 10 minutes
  after the last call. For auto-started daemons, idle starts at the **last
  admitted request** (any RPC, including Runner traffic proxied by the
  daemon); at 5 minutes the daemon drains and stops the Runner, then exits.
- Explicit `auv serve` daemons (users' own, `auv.service` on neko-gpu-1, the
  SDK's private daemons) keep today's semantics; the new idle rule applies
  only to daemons the CLI or the logon task starts.
- Graceful stop on SIGINT/SIGTERM/SIGHUP stays (`crates/auv-api-server/src/termination.rs`).

## Platform state

- **Linux portal grants.** The direct path keeps restore tokens in the user
  state dir; the Runner keeps them under the store
  (`crates/auv-daemon/src/daemon/runner_provider.rs:99`). The auto-started
  daemon uses the user state dir as its store, and the local Runner's portal
  state root must be the same directory the direct path uses, so switching
  does not ask for portal consent again. `auv doctor --portal-*` writes there.
- **macOS TCC.** A daemon spawned from a terminal is still attributed to that
  terminal (`setsid` does not change the responsible process). The first
  terminal or app that starts the shared daemon then owns the grants for
  everyone who shares it. `auv doctor` must say which process the daemon's
  grants belong to (`crates/auv-cli/src/commands/doctor.rs:138` already warns
  about launch-host attribution).
- **Windows session.** Covered by the auto-start table.

## Callers

| Caller | Today | After |
| --- | --- | --- |
| `auv invoke …` (no selection) | direct path | local Runner (auto-start) |
| `auv --no-daemon invoke …` | — | direct path |
| `auv mcp` tools | direct path without selection | local Runner (auto-start) |
| JS `startAuv()` (LobeHub, AIRI) | private daemon: launcher token, `--no-register`, never reads discovery (`js/packages/sdk/src/node/daemon.ts:218,267`) | unchanged |
| JS `connect({ discover: true })` | finds a registered daemon | may now find the auto-started one (intended sharing) |
| AIRI: `auv --credential-stdin invoke … --store-root <root>` with `AUV_ENDPOINT` set to its private daemon (`apps/stage-tamagotchi/src/main/services/airi/computer-use/runtime.ts:72-90`) | runs on the **direct path** (the endpoint is ignored without a selection) | runs on its own daemon's Runner (rule 6). Behavior change: traces in that daemon's store, state kept across calls. Must be tested in AIRI before release |
| LobeHub Desktop: `startAuv({ platforms: { macos: { helperApp } } })` (`lobehub-auv` README) | private daemon | unchanged. Desktop code not checked locally (sparse checkout) |

## Tests to change

`crates/auv-cli/src/commands/mcp_test.rs:1321` (no selection runs the local
adapter), `crates/auv-cli/tests/root_cli.rs:1088,1138,1190`, the invoke runner
tests that assert the direct path. New tests: resolution order (endpoint wins,
`--no-daemon`, NoRoute), stale-socket recovery, idle-from-last-request,
Windows Session 0 behavior (unit-level), concurrent auto-start.

## Alternatives considered

- **Use a daemon only if one is running, else the direct path.** Rejected:
  behavior (trace location, TCC identity, portal grants) would depend on
  whether a daemon happens to run, and it blurs the "no implicit local
  fallback" rule.
- **AuvHelper starts the daemon in the console session.** The LocalSystem
  Helper already starts a one-shot worker in the user's console session via
  `WTSQueryUserToken` (`crates/auv-driver-windows/src/device_session.rs`).
  Rejected for now: it widens a LocalSystem service (it would have to launch
  only the installed `auv.exe` with fixed arguments) and works only where the
  Helper is installed. The logon task needs no new privileged operation.
- **A Windows SCM service for the daemon.** Rejected: a service runs in
  Session 0 without the user's desktop and breaks the Helper's same-SID rule.
- **Keep the direct path and persist its state to disk** (e.g. the maximize
  record). Rejected: it duplicates Runner state handling and still loses AX
  connections, portal sessions and the future window cache.

## Out of scope

- launchd / systemd user services (`TODO(daemon-user-service-macos-linux)`).
- A per-session daemon on Windows for users with several logon sessions.
- Changing `startAuv` to share a registered daemon (its own deferred
  "connect-or-start" NOTICE).
