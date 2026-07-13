# MCP External Consumer Read Chain Evidence Pack

Date: 2026-06-11

Status: M0 evidence pack for the goal "蒸馏闭环交给外部 agent".

## Scope

This pack records the first successful external-agent read chain through the
thin MCP frontend:

```text
external agent
  -> MCP bundle_list
  -> MCP skill_list
  -> MCP invoke steam.library.list.v0
  -> MCP run_inspect
  -> shared runtime run + artifact store
```

The purpose is to prove that MCP is a frontend over the shared AUV runtime
path, not a second executor or strategy layer.

## Evidence Files

All copied files live under:

```text
docs/ai/references/evidence/2026-06-11-mcp-read-chain/
```

| File | Meaning |
|---|---|
| `codex-auv-read-chain.txt` | Human-readable successful external-agent MCP transcript. |
| `codex-auv-read-chain-session.jsonl` | Full Codex session JSONL transcript for the successful external-agent run. |
| `codex-auv-external-consumer-failed.txt` | Earlier failed attempt where the first MCP tool call was cancelled; kept as negative evidence. |
| `m0-consent-refusal-transcript.txt` | Human-readable MCP transcript for the required no-consent refusal, readiness-blocked validator history, and final human-approved semantic match. |

## Successful Run

The successful chain invoked:

```json
{
  "command_id": "steam.library.list.v0",
  "dry_run": false,
  "inputs": {},
  "target": {},
  "inspect": {}
}
```

MCP returned:

```text
run_id=run_1781117601264_19519_0
status=completed
artifact=artifacts/artifact_0001_steam-library-list.json
role=steam-library-list
summary=Listed 7 installed Steam app(s) through auv-steam local appmanifest grounding.
```

The corresponding inspect output includes:

```text
command.resolved resolved steam.library.list.v0 -> fixture.observe.steam_library_list
driver.backend backend=steam.local_appmanifest.library-list
artifact.captured artifact_0001 kind=steam-library-list
```

This proves:

- `steam.library.list.v0` is present in the shared command catalog.
- MCP `invoke` calls `Runtime::invoke`, not the `auv-steam` CLI.
- The command produced a standard run id.
- The command persisted a structured artifact.
- The same run is inspectable through MCP `run_inspect`.

## Negative Evidence

The earlier transcript `codex-auv-external-consumer-failed.txt` records a real
failed external-agent attempt:

```text
mcp__auv_temp.bundle_list -> user cancelled MCP tool call
```

The chain stopped immediately and did not invent later results. This is kept
because failures are evidence for frontend behavior and should not be silently
discarded.

## M0 Consent / Refusal Debt

The current M0 goal also asks for a consent/refusal pair through MCP:

```text
external agent via MCP calls one action-class command with no consent
  -> honest refusal is recorded
external agent via MCP calls the same class with owner consent
  -> granted execution + semantic verify are recorded
```

This is now completed in this pack.

The original blocker was contractual, not accidental:

- `docs/ai/references/session-api/2026-06-11-mcp-frontend-surface-v0.md` explicitly says
  MCP V0 does not expose general `candidate-action` tools.
- The current MCP V0 surface exposes generic `invoke`, but command-catalog
  pointer actions such as `debug.clickPoint` do not carry the
  candidate-action consent gate.
- Therefore using MCP `invoke` on existing pointer commands would not produce
  the required no-consent refusal.

The selected resolution is:

```text
Expose MCP candidate_action_run as an archived M0 evidence tool only.
```

This tool maps to `Runtime::run_candidate_action_command`, accepts only direct
`query` / `role` / `action` targeting, and does not expose the model proposer
path. It is not a new product lane and must not be used as the route for future
distillation workflow work.

The archived evidence contains three real MCP calls:

1. `candidate_action_run` without consent:
   - run: `run_1781166836225_70347_0`
   - status: `promotion_refused`
   - refusal: `permission_missing`
2. `candidate_action_run` with explicit owner-approved consent before the
   activation-settle fix:
   - run: `run_1781167666368_87561_0`
   - status: `blocked_not_ready`
   - blocker: `frontmost window 4823 does not match target window 19747`
   - purpose: preserve validator blocker history as evidence
3. `candidate_action_run` with explicit owner-approved consent after the
   activation-settle fix:
   - run: `run_1781167855870_93899_0`
   - status: `executed_single_action`
   - verification: `semantic_match executed=true state_changed=true semantic_matched=true`

The final state is:

```text
read-chain evidence: complete
consent/refusal surface path: implemented
consent/refusal run evidence: complete
M0 debt: complete
```

## Validation

Before the read-chain evidence pack was first written:

- `cargo fmt --check` passed.
- `cargo check` passed.
- `cargo test mcp --quiet` passed.
- `cargo run --quiet -- list-commands` showed `steam.library.list.v0`.

Before the consent/refusal evidence was added:

- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cargo test candidate_action_command::tests:: --quiet` passed.
- `cargo test mcp --quiet` passed.
- `cargo build` passed.

Full-suite validation should be rerun before any merge or push that includes
the MCP frontend code.
