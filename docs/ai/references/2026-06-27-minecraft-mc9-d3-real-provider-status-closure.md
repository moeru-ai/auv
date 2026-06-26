# 2026-06-27 Minecraft MC-9 D3 real provider status closure

Date: 2026-06-27

Status: implemented code slice for D7 real provider status truth only.

## Scope

MC-9 D3 closes **D7 real provider status evidence** for the Minecraft offline
3DGS chain. It does **not**:

- reopen D3/D5 source inputs
- expand D11 artifact fetch
- introduce an in-repo HTTP client
- grade trainer quality or splat usefulness
- treat local `result_dir` absence as provider failure

## Responsibility split

```text
D7 (MC-9 D3) -> provider/status-command truth
D11          -> fetch / normalize / required-artifact completeness
```

## Input boundary

D7 still consumes exactly one D6 `minecraft-3dgs-training-job.json`.

Real provider lane authority:

- explicit `--training-job-status-command` when present
- stdin JSON includes `job_id`, `job_url`, `endpoint`, `token_present`,
  `job_token`, `job_backend`, `trainer_backend`, `result_dir`
- stdout JSON returns `status` and optional `message`

Local `job_status.json` via `cat` remains a **compatibility / synthetic /
backfill** path only. It is not MC-9 D3 live provider evidence.

## Status truth policy

Provider/status-command states map directly to D7 `status`:

- `queued`
- `submitted`
- `failed` with `status_reason = provider_reported_failed`
- `succeeded`
- `blocked` for launch-blocked upstream jobs, missing endpoint/token, or broken
  status-command execution/protocol

Local observation fields remain on manifest/inspect but do not rewrite D7
status:

- `result_dir_exists`
- `key_result_artifacts_present`
- `result_artifact_count`
- `result_artifacts`

When provider reports `succeeded` but local artifacts are not yet present,
D7 records:

- `status = succeeded`
- `status_message` from provider when available
- local gaps in `warnings` and D3 `known_limits`

## Historical MC-8 adapter semantics

MC-8 D2 closed the **command-adapter lane** where local result directory and
key artifact presence could force D7 into `failed` with:

- `result_directory_missing`
- `result_artifacts_missing`

Those reason codes remain readable for legacy JSON. MC-9 D3 no longer emits
them.

Reference:
`docs/ai/references/2026-06-26-minecraft-mc8-closure-gate-verdict.md`

## Output fields added in D3

- `TrainingResultRequest.job_token`
- `TrainingResultManifest.status_message`
- `TrainingResultInspectReport.status_message`
- `TrainingResultReason.provider_reported_failed`

## Live gate expectation

Given a D2-accepted D6 manifest and an explicit status command that returns a
non-blocked provider state, D7 must write a non-blocked result even when the
local trainer output directory has not been fetched yet. The inspect/terminal
surfaces must show provider status separately from local result observation.
