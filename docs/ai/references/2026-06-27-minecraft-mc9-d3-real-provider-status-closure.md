# 2026-06-27 Minecraft MC-9 D3 real provider status closure

Date: 2026-06-27

Status: implemented code slice with fresh local live evidence for D6 submit,
D7 provider-status truth, and D11 command-materialized normalized-artifact
fetch.

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

## Fresh local live evidence

Fresh local live evidence was recorded on 2026-06-27 under
`.tmp/mc9-d3-live/`.

Recorded runs:

- D5 launch-prep reference run: `run_1782493609356_36354_0`
- D6 real-provider submit run: `run_1782493613903_36423_0`
- D7 real-provider status run: `run_1782493616372_36652_0`
- D11 command-materialized artifact fetch run: `run_1782493728710_37763_0`

Observed D6 submit facts:

- `status = submitted`
- `accepted_by_provider = true`
- `submission_recorded_at_millis = 1782493613989`
- `job_id = mc9-d3-live-job`
- `job_url = https://mc9-live.example.invalid/api/jobs/mc9-d3-live-job`

Observed D7 provider-status facts:

- `status = succeeded`
- `status_reason = null`
- `status_message = provider-status-saw-job_id=mc9-d3-live-job token=present`
- `result_dir_exists = false`
- `key_result_artifacts_present = false`

This is the key D3 proof: provider-reported success remained `succeeded` even
though the local result directory had not been fetched yet.

Observed D11 command-materialized fetch facts:

- `fetch_status = succeeded`
- `fetch_reason = null`
- `source_result_dir_exists = false`
- `required_artifacts_present = false`
- `normalized_artifact_count = 3`

The `required_artifacts_present = false` value is expected in this branch. It
describes the **source** D7 manifest's local-result observation, not the
freshly materialized normalized output. The normalized output still contained:

- `normalized-result/config.yml`
- `normalized-result/nerfstudio_models/`
- `normalized-result/job_status.json`

Saved inspect text snapshots:

- `.tmp/mc9-d3-live/inspect/d6.txt`
- `.tmp/mc9-d3-live/inspect/d7.txt`
- `.tmp/mc9-d3-live/inspect/d11.txt`

Saved manifest / inspect artifacts:

- `.tmp/mc9-d3-live/training-job/minecraft-3dgs-training-job.json`
- `.tmp/mc9-d3-live/training-job/minecraft-3dgs-training-job-inspect.json`
- `.tmp/mc9-d3-live/training-result/minecraft-3dgs-training-result.json`
- `.tmp/mc9-d3-live/training-result/minecraft-3dgs-training-result-inspect.json`
- `.tmp/mc9-d3-live/training-result-artifacts/minecraft-3dgs-training-result-artifact-manifest.json`
- `.tmp/mc9-d3-live/training-result-artifacts/minecraft-3dgs-training-result-artifact-inspect.json`

Operational note:

- local inspect-server writes to `127.0.0.1:8765` were unavailable during this
  pass, but local run-store records and local artifact outputs were written
  successfully.
