# Frontend Convention V0

Date: 2026-06-11

Status: M0 convention note for the goal "同一执行模型从条款变成事实"

## Why This Exists

AUV currently has a drift problem on the consumer side.

Several product crates prove useful capability, but they do not automatically
enter the shared AUV runtime/run/store/inspect model. That means the statement
in `AGENTS.md` — CLI, MCP, library calls, and future UI surfaces should share
one execution model — is not yet true in product code.

This note defines the convention that makes it true without pulling strategy
logic into core.

## Identity

AUV does not implement an agent.

A product crate is allowed to contain:

- domain capability logic
- app-family specific strategy logic
- thin frontend CLI output and presentation

AUV core contains:

- runtime execution
- run recording
- artifact persistence
- inspect/read-side surfaces
- shared command catalog
- shared disturbance / consent / refusal semantics

## Rule

New product capability should default to this shape:

```text
product crate library capability
  -> core command catalog entry
  -> runtime invoke
  -> run/artifact/store/inspect
  -> optional thin frontend(s): CLI / MCP / library adapter
```

Not this shape:

```text
product crate CLI
  -> direct local logic
  -> no run
  -> no store
  -> no inspect
```

## Dependency Direction

The required dependency direction is:

```text
auv-cli/core -> product crate library
```

Allowed:

- `auv-cli` depends on `auv-steam` library APIs
- core command implementation calls product crate library functions
- product crate binary reuses the same library function as the core command

Not allowed:

- core copies product crate logic
- product crate CLI output becomes command contract
- product crate binary becomes the only executable path for a capability
- MCP or future UI frontend calling product CLI text instead of library/runtime

## Thin Frontend Definition

A thin frontend may:

- parse user/client arguments
- map arguments into typed runtime/library requests
- format output for its own transport
- surface the original runtime/library errors

A thin frontend must not:

- keep a parallel executor
- keep a parallel store
- add planner or natural-language parsing
- auto-retry with modified arguments
- silently weaken consent or disturbance policy
- redefine the command contract around table/text presentation

## Product Crate Convention

For product crates such as `auv-steam`:

- the library crate is the capability owner
- the binary crate is a presentation shell
- the first-class reusable unit is a typed library function/result, not a table
- if the library shape is not reusable enough for core invoke, fix the library
  instead of duplicating its logic in core

## First API-Signal Example

`steam.library.list.v0` is the first intended API-grade command on the AUV
signal ladder.

Signal ladder now becomes:

```text
API/file read
  -> AX
  -> OCR
  -> detector/inference
```

Principle: strongest available signal wins, but every signal still lands in the
same runtime/run/store/inspect model.

That means API/file reads are not outside AUV core; they are one valid producer
shape that should be consumable through the same invoke surface.

## Required Outcome For `steam.library.list.v0`

The core command must:

- be present in the shared command catalog
- be invokable through `auv-cli invoke steam.library.list.v0`
- produce a standard run id
- persist artifacts and/or structured evidence through the existing store
- be inspectable through `auv-cli inspect <run-id>`
- call `auv-steam` library code rather than reimplementing `steamlocate`

The `auv-steam` binary should remain useful, but only as a thin frontend over
that same capability.

## MCP Relationship

MCP is a frontend, not a capability source.

The first MCP external-consumer proof should call the same core command that CLI
calls. MCP is not where the Steam library capability is invented; MCP is where
that capability proves it is consumable by a real external agent.

## Acceptance For This Note

This convention is accepted when:

- the dependency direction is explicit
- thin frontend boundaries are explicit
- product-crate CLI presentation is explicitly kept out of command contracts
- `steam.library.list.v0` is named as the first API-grade example
- MCP is explicitly framed as a frontend over the same runtime path

## Next Slice Candidates

1. Land `steam.library.list.v0` through core invoke using `auv-steam` library code.
2. Narrow the `auv-steam` binary to a presentation shell over the same library call.
3. Resume the MCP frontend slice only after the Steam command exists in the core runtime.
