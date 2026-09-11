# Core CLI command ownership design

Date: 2026-08-04

Status: accepted and implemented. This note records the resulting ownership
contract and its intentional deferrals.

## Scope classification

This is a coordinated ownership migration across `auv-cli`, `auv`,
`auv-api-client`, `auv-api-server`, and the new `auv-daemon` crate. It removes
the pass-through `cli_frontend` module, establishes canonical operation
interfaces, and lands the accepted daemon server-SDK ownership without a
transitional compatibility layer. The owner approved this coordinated
migration as one refactor rather than a sequence of compatibility stages.

The future `auv-cli-mcp` package is an architectural consumer used to test whether
operations are genuinely reusable. This design does not approve creating that
package or implementing a new MCP surface in the current slice.

## Confirmed decisions

### One parsed command model

The root CLI will have one parsed command model. The clap-owned `RootCommand`
and its typed argument values will route directly to the corresponding command
module. The flattened `CliCommand` intermediate representation and the second
dispatch pass will be removed.

Tests should exercise either parsing behavior or a command module's stable
execution interface. They should not require a duplicate command model solely
as a test seam.

### Top-level command-family ownership

Each top-level command family is one cohesive command module. For example,
`commands/devices` owns its clap declarations, execution dispatch, private
`list`/`get`/`profiles`/pairing/trust flows, and Device-specific human and JSON
presentation. `commands/run`, `commands/runner`, and the other top-level
families follow the same rule.

Leaf subcommands are private branches inside their owning command module by
default. File size or execution order alone does not justify splitting them
into `list`, `get`, or `output` modules. A nested module is introduced only
when it hides a distinct policy, protocol, persistence, lifecycle, or reusable
domain responsibility behind its own meaningful interface.

### Narrow command execution interfaces

Each top-level command module exposes one narrow asynchronous execution entry
point and returns the root frontend's existing `Result<i32, String>` outcome.
The entry point receives its typed command arguments plus only the root-owned
values that command actually needs, such as resource selection or the project
root. Commands that do not need a value do not receive it.

The refactor will not introduce a universal `CommandContext`, `Dependencies`,
or `Deps` bag containing clients, paths, tracing, selection, or sibling
helpers. The root owns process diagnostics, parsing, help/version handling,
top-level routing, and the final error-to-exit-status conversion. Each command
module owns its execution and presentation.

### Unresolved Root Selection

The root CLI passes global Device and optional Run selection to commands as an
unresolved `RootSelection`. It does not connect to a daemon or project that
selection into `AuvContext` before routing.

This distinction preserves command-specific semantics: Device inventory can
remain useful without a live local daemon, invoke may create an implicit Run,
resource commands validate explicit leaf arguments against root selection, and
plugins eventually inherit a resolved context. Commands must delegate the
actual selector and placement rules to the owning AUV operation interface rather than
copying them locally.

### Existing operation-interface and wire-client ownership

This refactor follows the existing `TERMS_AND_CONCEPTS.md` contract rather than
opening a new ownership decision. The `auv` operation interface owns context resolution,
profile selection, discovery, resource selection, and placement.
`auv-api-client` remains an explicit remote wire client and does not read CLI
or process configuration or choose resources.

Current daemon-administration commands obtain the operation interface's gRPC control client
after selection has been resolved. The target design replaces that escape hatch
with canonical operations: core frontends do not reproduce discovery,
prefix matching, ambiguity handling, profile precedence, Device/Run
consistency, protobuf requests, or gRPC error interpretation.

### Typed configured-Device availability

The `auv` operation interface owns concurrent probing of configured Device profiles and the
classification of connection, authorization, profile, and canonical-identity
failures into typed Device availability. The operation interface returns a typed observation
that preserves the configured profile, availability, and optional live Device;
it does not return CLI status strings or rendered rows.

`commands/devices` owns CLI filters, merging or de-duplicating local daemon
Devices and configured observations, warning/error policy when the local daemon
is unavailable, and human or JSON presentation. The current
`ConfiguredDeviceProbe` and `configured_device_error_status` implementation
therefore move below the CLI seam instead of merely moving into the Device
command file.

### CLI as an interface adapter

`auv-cli` is the CLI and standard-I/O adapter over reusable operations. It owns
OS-argument parsing, typed command routing, command-specific human and JSON
presentation, standard-stream behavior, and process exit mapping. It does not
own reusable Device, Run, Runner, pairing, context, or capability semantics.

The `auv` operation interface will expose resource-specific control operations for Device,
Run, and Runner callers. Those operations own selector resolution, ambiguity,
resource-association validation, and mutations. They return typed results and
errors without printing. CLI, library, MCP, and future UI frontends can consume
the same operations.

Pairing and other daemon-facing core operations follow the same canonical
operation-interface rule. Ordinary core CLI and future MCP code call `auv`; they do not
construct protobuf requests, interpret `tonic::Status`, or call
`auv-api-client` directly. `auv-api-client` is the gRPC/protobuf transport
adapter behind domain-facing operation signatures.

The operation interface will not introduce a generic CRUD interface for daemon resources.
Each resource module keeps the rules specific to that resource, and
`auv-api-client` continues to encapsulate individual protobuf RPCs after the
operation interface has resolved context and operation intent.

Daemon-facing client operations do not transfer daemon ownership into `auv`.
The daemon implementation continues to own resource state, token and
credential rules, authorization, persistence, Runner supervision, and server
lifecycle. The `auv` operation interface may select, connect, validate client-visible
relationships, and orchestrate client-side profile behavior while invoking
those server-owned rules through `auv-api-client`.

Extension-owned generated clients may receive an operation-interface-provided routed and
authenticated transport handle for extension-specific protocols. Core
operations cannot use that escape hatch to avoid their canonical operation
interfaces.

### No core wire-type leakage

The coordinated migration removes the current core escape hatches rather than
carrying them into a second phase. Core Device, Run, Runner, Pairing, context,
and capability operations accept and return Rust domain types and typed domain
or outer-layer errors. They do not expose protobuf messages or
`tonic::Status`.

The current general-purpose `Client::grpc()` path is not used as a core
operation interface. Extension-specific generated clients may instead obtain a
purpose-named routed transport after context, routing, and authentication have
been resolved. `auv-api-client` owns generated gRPC calls and routed transport
construction; the `auv` operation adapter immediately converts their wire
responses and statuses into its public domain results and errors.

### Unified operation interface with resource-specific interfaces

`auv::Client` is the canonical entry point for shared context, selection,
authentication, backend resolution, typed results, and typed errors. It exposes
deep resource-specific interfaces for Devices, Runs, Runners, Pairing, and
other accepted domains instead of accumulating one flat method list.

The resource interfaces are not a generic CRUD abstraction. Each hides the
selection, lifecycle, validation, and failure rules specific to its resource.
This is one unified SDK/interface, not one aggregate daemon implementation or
catch-all runtime crate.

### Future MCP consumption

A future MCP frontend selects AUV operations and projects them into MCP tools.
It owns tool names, descriptions, JSON schemas, MCP request/result mapping, and
its transport lifecycle. Those protocol details do not enter `auv`.

The intended coverage is most CLI-supported operations that have reusable
programmatic semantics. CLI and MCP therefore validate the same canonical
`auv` operation interfaces, while frontend-only process operations and plugin
exposure rules still require explicit classification. This design records that
consumer requirement but does not create `auv-cli-mcp` in the current slice.

Plugin discovery and typed extension operations are distinct. `plugin.list`
may be a reusable operation, and an extension may deliberately publish typed
operations that CLI and MCP interfaces both adapt. An arbitrary `auv-<name>`
PATH executable, argv sequence, or stdout stream is not automatically promoted
into an MCP tool or canonical AUV operation.

### Daemon server SDK

`auv-daemon` denotes the crate/SDK that encapsulates the daemon's server-side
state, control semantics, supervision, routing, and lifecycle. It is not the
daemon management client interface; client-facing operations remain available
through `auv` and use `auv-api-client` for remote transport.

CLI and MCP interfaces may both host or start a daemon by invoking
`auv-daemon`. The ability to start the process does not transfer daemon state
or lifecycle implementation into either frontend.

The current migration creates `auv-daemon` and moves the complete accepted
server ownership into it: Device, Run, and Runner state and control semantics;
pairing persistence; Runner provider and supervisor behavior; routing,
draining, and reusable bind/serve/shutdown lifecycle. `auv-api-server` retains
protocol serving and adapters. `auv-cli` parses `serve` arguments and invokes
the server SDK.

This is not a file-only extraction. The migration does not retain a second
daemon owner, a temporary pass-through compatibility crate, or an intermediate
CLI-owned implementation.

### CLI observable compatibility

Except for the explicitly approved live pairing-administration change, the
coordinated migration preserves CLI command names, aliases, flags, defaults,
human and JSON output, exit behavior, daemon-unavailable policy, Root Selection
conflicts, short-ID behavior, and invoke Run/tracing lifecycle.

The approved exception removes pairing-store CLI access and stopped-daemon
enable, disable, and unpair behavior in favor of live operations. Internal Rust
interfaces, modules, crates, and tests may be restructured without compatibility
shims. The flattened `CliCommand` and core wire-type escape hatches are removed.

### Invoke ownership exception

`auv-cli-invoke` retains its current crate ownership and reusable invoke
behavior during this migration. It is not renamed to `auv-invoke`, absorbed by
`auv`, or replaced with a compatibility wrapper. `commands/invoke` continues
to consume it while owning the root CLI adapter behavior assigned elsewhere in
this design.

How a future `auv-cli-mcp` interface consumes or motivates restructuring invoke
is deliberately deferred until that interface is designed. The implementation
must leave a TODO at the invoke consumption seam that cites this decision and
names future MCP design as the trigger for reopening it.

### Shared presentation primitives

Command modules may share protocol-neutral CLI presentation mechanisms through
`auv-cli-common`, such as table rendering, terminal/color detection, and a
general output mode. Resource-specific rows, JSON projections, empty-state
messages, warnings, and output policy remain in the owning command module.

Resource semantics are not presentation helpers. Canonical compact ID display,
selector behavior, and typed enum names belong to the `auv` domain model.
The migration does not create `commands/common`, `frontend_utils`, or another
miscellaneous CLI helper layer for these concerns.

### Typed resource identities and selectors

Device, Run, Runner, and RunnerClass identities are distinct domain value
objects even when their underlying wire encodings have similar shapes. A
canonical ID is validated at its construction, configuration, deserialization,
or wire-conversion boundary, preserves its complete value, and provides the
accepted compact display without CLI string manipulation.

User-entered names and unambiguous short prefixes are selectors, not partially
valid resource IDs. Resource-specific `auv` operations resolve selectors into
the corresponding typed IDs. A Device, Run, Runner, or RunnerClass ID cannot be
substituted for another resource's identity by type accident.

### Resource-specific typed errors

Device, Run, Runner, and Pairing interfaces expose resource-specific typed
errors for selector failures, ambiguity, invalid resource state, association
conflicts, authorization, and rejected operations. Shared outer client and
context errors represent transport, discovery, profile, and protocol failures.

Public error variants do not expose `tonic::Status`, although implementations
retain source chains for diagnostics. CLI and future MCP interfaces map the same
typed errors into their own text, exit, or structured-tool behavior. The
migration does not invent a global catch-all error enum, error-code registry,
retry, or backoff policy without a concrete recovery requirement.

### Explicit frontend projections

`auv` returns typed domain results; those result layouts are not automatically
the CLI JSON contract or a future MCP schema. Each command module explicitly
projects domain results into its existing table, human text, and JSON output.
A future MCP interface owns a separate explicit MCP projection.

Domain types derive serialization only when the domain, storage, or another
accepted interface requires it. Frontend convenience alone does not couple a
domain struct's fields to a public CLI or MCP representation.

### Final `auv-cli` source boundary

The root source retains `cli.rs` for root parsing, diagnostics, one routing
pass, and exit mapping; `commands/` for all user-command declarations,
execution, and presentation; `runner/` for the internal process role dispatched
before the Tokio runtime; and `xtask.rs` for repository-only development work.

`cli_frontend.rs` is deleted. Top-level plugin execution moves into
`commands/plugin` while reusable selection moves to `auv`. The existing MCP
serve implementation moves into `commands/mcp`; creating `auv-cli-mcp` remains
deferred. CLI-owned daemon hosting moves to `auv-daemon`, with serve argument
adaptation in `commands/serve`. No equivalent top-level frontend helper replaces
the deleted modules.

### Acceptance evidence and delivery

The coordinated migration may proceed in dependency order and keep progressive
compile/test feedback, but its delivered state contains no temporary public
compatibility layer. Required evidence covers:

- typed Device, Run, Runner, Pairing, selector, identity, error, and configured
  availability behavior through `auv`;
- daemon state, pairing authority, Runner lifecycle, and server lifecycle
  through `auv-daemon`;
- domain/wire conversion, authentication, and RPC adapters across
  `auv-api-client` and `auv-api-server`;
- existing root subprocess, table/JSON, exit, selection-conflict,
  daemon-unavailable, and invoke lifecycle behavior through `auv-cli`;
- live enable, disable, unpair, and shared paired-Device administration across
  listener types;
- deletion of `cli_frontend`, core gRPC escape-hatch use, CLI pairing-store
  access, duplicated selector logic, and raw core protobuf/tonic interfaces.

The final workspace runs the repository validation commands. The implementation
does not create Git commits unless the owner requests them separately.

### Live pairing administration

The current `auv devices enable`, `disable`, and `unpair` implementations
directly administer the daemon's local `PairingStore` while the daemon is
stopped. That path is removed by the coordinated migration.

`auv-daemon` becomes the sole pairing-store owner and implements live enable,
disable, unpair, token, enrollment, and credential-revocation operations.
`auv-api-server` maps the approved operations to protocol services,
`auv-api-client` implements their wire calls, and `auv` exposes domain-facing
typed operations. CLI and future MCP interfaces do not open pairing files.

The existing CLI pairing-store option and stopped-daemon administration
behavior are removed rather than retained as compatibility. The local owner
and every active paired Device bearer have equal authority to create tokens,
enable or disable Devices, unpair Devices, and revoke credentials. Pairing does
not add an administrator role; `PairDevice` remains token-authenticated.

## Implemented result

- `auv-cli` parses once and routes directly into cohesive command-family
  modules; `cli_frontend` and the second command representation are gone.
- `auv` owns typed Device, Run, Runner, Pairing, configured-profile
  observation, selection, capability results, and client-visible error policy.
  Core CLI commands no longer construct daemon protobuf requests or interpret
  tonic statuses, and public operation errors retain their transport source
  chain without exposing it as a business-facing type.
- `auv-daemon` is the concrete server SDK for state, pairing persistence,
  Runner providers and supervision, routing, discovery publication, and
  bind/serve/shutdown lifecycle.
- `auv-api-server` is a protocol adapter over injected control and pairing
  contracts. It contains no concrete daemon store or supervisor, including in
  its test build.
- Pairing administration is live through the daemon API. Local-owner and
  active paired-bearer callers share the approved administration authority;
  `auv::pairing` also exposes typed credential revocation.
- `auv-cli-invoke` remains in place with the required MCP-triggered ownership
  TODO at its CLI consumption seam; no `auv-cli-mcp` crate was created.
