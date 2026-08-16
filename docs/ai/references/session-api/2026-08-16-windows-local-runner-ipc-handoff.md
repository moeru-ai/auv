# Windows local Runner IPC handoff

Status: implemented and tested on Windows on 2026-08-16.

This slice adds both Windows local process boundaries. It covers the public
daemon listener and the private IPC for daemon-owned executable Runners. It
does not add a Windows release artifact or app-specific remote operations.

## Platform transport

The public daemon endpoint and the private Runner endpoint have different
trust boundaries.

```text
CLI or SDK
  -> daemon endpoint
     -> selected Device and Run
        -> daemon-owned Runner process
           -> platform Driver or app-owned operation
```

On Linux and macOS, local callers use a Unix socket by default. The daemon
passes one connected Unix stream to each executable Runner as file descriptor
3.

On Windows, local callers use an owner-scoped named pipe by default. The pipe
DACL grants access to the owner and LocalSystem. The daemon also rejects remote
pipe clients. Discovery publishes an `npipe://./pipe/...` endpoint.

The daemon creates one private named-pipe instance for each executable Runner.
It passes the unadvertised pipe name in `AUV_RUNNER_IPC_PIPE`. The Runner
connects before it serves gRPC. An Ephemeral Runner stops after its route body
is dropped. A Run-affine `UnlessShutdown` Runner remains ready for reuse until
an explicit stop or daemon shutdown.

Both paths use the same Health, Reflection, RunnerClass, Device, Run, and route
contracts. The transport difference is private to daemon supervision.

## Local use

Build AUV with the repository toolchain, then start the foreground daemon:

```powershell
cargo +1.91.0 build -p auv-cli
target\debug\auv.exe serve
```

Windows publishes a unique `npipe://./pipe/auv-...` endpoint. Linux and macOS
publish a local Unix endpoint. The default command does not bind TCP. A second
terminal can use discovery without repeating the endpoint:

```powershell
target\debug\auv.exe devices list
target\debug\auv.exe invoke display.list --json
```

Use `--listen http://127.0.0.1:0` only when a test requires TCP. Port zero is
resolved before the discovery file is published.

## Paired Device use

A non-loopback listener requires a pairing store. Keep a named-pipe listener
for owner administration. On the Windows Device host, run:

```text
auv serve --listen npipe://./pipe/auv-owner --listen http://0.0.0.0:9848 --pairing-store <PATH>
auv devices pair --endpoint npipe://./pipe/auv-owner create-token
```

On the controller, consume that token and select the saved Device:

```text
auv devices pair --endpoint http://DEVICE_IP:9848 connect --token <TOKEN> --label "Windows PC" --profile windows-pc
auv --device "Windows PC" invoke display.list --json
```

The `invoke` frontend runs on the controller. The typed Driver operation runs
inside `auv.core.local` on the selected Device.

The current remote listener is plain `http://` with bearer authentication. Use
it only on a trusted network or through an encrypted tunnel. Do not expose it
directly to the public internet.

## Evidence

The following automated boundaries passed on Windows:

- `auv serve` routed Runner Health and `invoke display.list` through the
  first-party named-pipe Runner.
- The default Windows daemon served its control API through an owner-scoped
  named pipe. Rust and Node clients connected without a TCP listener.

The tests are in `crates/auv-cli/tests/root_cli.rs`. This is automated
transport and placement evidence. It is not live app-input evidence.
