# 2026-06-25 Minecraft MC-7 D7 training-result collection design

Date: 2026-06-25

Status: implemented code slice; D7 consumes D6 job manifests plus recorded
trainer-output evidence and writes a result manifest, inspect report, and
runbook. It still does not grade model quality.

## Scope

MC-7 D7 is defined as:

- consume one D6 `minecraft-3dgs-training-job.json`
- read remote-status/result evidence for that job
- write one result manifest JSON
- write one result inspect JSON
- write one manual runbook Markdown
- do not evaluate splat quality
- do not widen trainer/backend coverage

## Input boundary

D7 reads only the D6 job manifest as its canonical input. To keep that contract
real, the D6 manifest now carries:

- `status`
- `job_id`
- `job_url`
- `readiness_blocker`

D7 does not reopen D3 or D5 source artifacts directly.

## Current backend-neutral result contract

The current D7 result consumer uses the D6 `suggested_output_dir` as the
trainer-output directory and reads:

- `<suggested_output_dir>/job_status.json`
- `<suggested_output_dir>/config.yml`
- `<suggested_output_dir>/nerfstudio_models/`

This is intentionally a backend-neutral handoff, not a claim that all future
remote backends must use the exact same transport forever.

## Status policy

D7 reports one of:

- `queued`
- `submitted`
- `blocked`
- `failed`
- `succeeded`

Reason codes are narrow:

- `missing_configuration`
- `missing_authentication`
- `launch_blocked`
- `remote_status_unavailable`
- `result_directory_missing`
- `result_artifacts_missing`

This keeps the slice focused on result-collection truth rather than trainer
quality.

## Output shape

The canonical D7 output directory is:

```text
minecraft-3dgs-training-result.json
minecraft-3dgs-training-result-inspect.json
mc7-training-result-runbook.md
```

Three artifact roles are staged:

- `minecraft-3dgs-training-result`
- `minecraft-3dgs-training-result-inspect`
- `minecraft-3dgs-training-result-runbook`

## Boundary

D7 closes “result has or has not come back” for the current remote-training
contract. It does not close:

- splat quality
- downstream splat consumption
- real-source trainer benchmarking

Those belong in later slices.
