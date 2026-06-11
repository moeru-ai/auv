# MCP Frontend Surface V0

Date: 2026-06-11

Status: M0 surface note under the goal "同一执行模型从条款变成事实"

## Goal Placement

This note covers only the MCP frontend surface.

It depends on core commands that already exist or are accepted for landing in
core first. The first external-agent consumer proof must include
`steam.library.list.v0`, but this document does not redefine Steam capability
ownership — it only defines how MCP consumes the shared runtime.

## Identity

AUV does not implement an agent.

The MCP server is a thin frontend over the same runtime/library path used by the
CLI. It is not a planner, not a strategy layer, and not a second executor.

The agent must send an explicit tool name and an explicit command id.

## Transport

Transport: stdio MCP server.

Why first:

- cheapest real attachment path for Claude/Codex/Cowork-style clients
- no new daemon/service tier in the first slice
- preserves the frontend-only boundary

## First Exposed Tool Surface

Expose only:

- `bundle_list`
- `bundle_show`
- `skill_list`
- `skill_show`
- `invoke`
- `run_inspect`

Do not expose in V0:

- `candidate-action` tools
- raw driver operations
- direct store write APIs
- `app probe` / `app analyze` / `app distill` / `app validate`
- scan surfaces
- inspect server mutation endpoints

## Tool Mapping

### `bundle_list`

Maps to:

- `SkillBundleCatalog::discover(project_root)`
- `entries()`

Returns a structured list of bundle id / name / status / path.

### `bundle_show`

Maps to:

- `SkillBundleCatalog::discover(project_root)`
- `resolve(project_root, query)`
- raw manifest JSON payload

### `skill_list`

Maps to:

- `SkillCatalog::discover(project_root)`
- `entries()`

Returns recipe id / objective / status / path / strategy taxonomy metadata.

### `skill_show`

Maps to:

- `SkillCatalog::discover(project_root)`
- `resolve(project_root, query)`
- raw manifest JSON payload

### `invoke`

Maps to:

- `build_default_runtime(project_root)` or equivalent runtime builder
- `Runtime::invoke(InvokeRequest)`

The request must carry:

- `command_id`
- explicit `target.application_id` when needed
- explicit `inputs`
- explicit `dry_run`

This is the only execution entrypoint in V0.

The first required command path for the external-consumer goal is:

- `steam.library.list.v0`

The read-chain proof should then include one existing StS read command.

### `run_inspect`

Maps to:

- `Runtime::inspect(run_id)`

V0 returns the current text inspect rendering rather than inventing a second
inspect schema for MCP.

## Consent / Disturbance / Refusal

MCP must not weaken runtime policy.

- consent-gated commands remain consent-gated
- disturbance limits remain identical to CLI/library callers
- refusal remains a valid and required evidence path
- the server must not mint consent on behalf of the caller

Acceptance evidence must include:

- one granted consent-gated action path
- one refusal path with no consent

## Error Semantics

The server must preserve runtime errors as-is, wrapped only by the MCP transport
error envelope.

Examples:

- `unknown command ...`
- `ambiguous command ...`
- validation failures
- consent refusal
- disturbance refusal
- driver/runtime failures

The server must not:

- reinterpret errors into natural language
- retry with modified parameters
- switch commands automatically

## Provenance

If frontend provenance is recorded, it must go through the normal run contract.

Proposed run attribute:

- `frontend = cli | mcp | library`

If this field is not cheap to land cleanly, defer it. Do not create an MCP-only
side channel beside the run store.

## Acceptance For This Note

This note is done when:

- the exposed V0 MCP tools are explicit
- the not-exposed surface is explicit
- each tool maps to an existing runtime/library path
- consent/refusal behavior is preserved as a hard boundary
- `steam.library.list.v0` is named as the first required command in the agent read chain

## Next Slice Candidates

1. Land `steam.library.list.v0` into the shared core command catalog and runtime.
2. After that, re-land the stdio MCP server against the now-stable read path.
3. Run a real agent transcript: list -> invoke `steam.library.list.v0` -> invoke StS read -> inspect.
4. Add the granted/refusal action pair only after the read chain is stable.
