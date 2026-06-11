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

This is **not yet completed** in this pack.

The blocker is contractual, not accidental:

- `docs/ai/references/2026-06-11-mcp-frontend-surface-v0.md` explicitly says
  MCP V0 does not expose `candidate-action` tools.
- The current MCP V0 surface exposes generic `invoke`, but command-catalog
  pointer actions such as `debug.clickPoint` do not carry the
  candidate-action consent gate.
- Therefore using MCP `invoke` on existing pointer commands would not produce
  the required no-consent refusal.

Do not mark M0 complete until this is resolved by an owner-approved surface
decision. The likely choices are:

1. Add a narrow consent-gated action command to the shared catalog, then let MCP
   `invoke` exercise it without creating a candidate-action-specific tool.
2. Revise the MCP surface note to allow a narrowly scoped archived
   `candidate-action` recovery tool only for the M0 consent/refusal debt.
3. Change the M0 acceptance wording so the consent/refusal pair is recorded via
   CLI/library instead of MCP.

Until one of those is approved, the correct state is:

```text
read-chain evidence: complete
consent/refusal debt: blocked
M0 overall: incomplete
```

## Validation

Before this evidence pack was written:

- `cargo fmt --check` passed.
- `cargo check` passed.
- `cargo test mcp --quiet` passed.
- `cargo run --quiet -- list-commands` showed `steam.library.list.v0`.

Full-suite validation should be rerun before any merge or push that includes
the MCP frontend code.
