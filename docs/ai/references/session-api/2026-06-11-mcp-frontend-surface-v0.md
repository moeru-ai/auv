# MCP Frontend Surface V0

Date: 2026-06-11

Status: M0 surface note; read-chain and consent/refusal evidence completed.

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

Expose the read-chain tools:

- `bundle_list`
- `bundle_show`
- `skill_list`
- `skill_show`
- `invoke`
- `run_inspect`

Expose one archived M0 evidence tool:

- `candidate_action_run`

This tool exists only to repay the M0 consent/refusal evidence debt. It calls
the existing `Runtime::run_candidate_action_command` path and accepts only
direct `query` / `role` / `action` targeting. It does not expose the model
proposer path, does not parse natural language, and does not mint consent.

Do not expose in V0:

- general-purpose `candidate-action` tooling
- candidate-action model proposer controls
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

### `candidate_action_run`

Maps to:

- `Runtime::run_candidate_action_command`

This is an archived vertical recovery path exposed only for the M0
consent/refusal pair. It is not a new product lane and must not be used as the
route for future distillation workflow work.

Constraints:

- direct target only: `target_app`, `query`, `role`, `action`
- no `intent` / model proposer inputs
- no natural-language planning
- no MCP-side retry or parameter mutation
- no MCP-side consent minting

No-consent calls must reach the existing refusal-first candidate-promotion
gate. Consent-granted calls must supply an explicit consent source accepted by
the existing command path; MCP only transports that request.

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
- `candidate_action_run` is limited to the archived M0 evidence path and does
  not become a general candidate-action or proposer surface

## Completed Evidence

1. `steam.library.list.v0` landed in the shared core command catalog and runtime.
2. The stdio MCP server landed as a thin frontend over the shared runtime path.
3. A real external-consumer transcript invoked `steam.library.list.v0` through
   MCP and inspected the resulting run.
4. The granted/refusal action pair ran through `candidate_action_run`:
   - no consent refused with `permission_missing`
   - owner-approved consent executed and produced `semantic_match`

The detailed run ids and transcript snippets are in
`docs/ai/references/session-api/2026-06-11-mcp-read-chain-evidence-pack.md`.
