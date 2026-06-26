# MC-8 D4 adapter live closure

Date: 2026-06-26

## Summary

MC-8 D4 ran the hardened D6 -> D7 -> D11 -> D12 trainer-side adapter chain
against the existing MC-7 accepted-only training launch plan. This is an adapter
live closure, not a cloud-provider training closure and not a model-quality gate.

The run used explicit command adapters to exercise the same JSON/stdin/stdout
contracts that a real remote backend must implement:

```text
D6 launch job -> D7 collect status/result -> D11 fetch normalized artifacts -> D12 inspect consumer
```

No local `ns-train` process was started, no trained splat quality was evaluated,
and no renderer/model preview was attempted.

## Input lineage

The D4 run consumed the existing MC-7 live closure launch plan:

- training launch plan: `.tmp/mc7-live/closure/training-launch/minecraft-3dgs-training-launch-plan.json`
- source scene packet: `.tmp/mc7-live/closure/scene-packet/run.json`
- source training package: `.tmp/mc7-live/closure/training-package/run.json`
- source run count: `6`
- frames/images: `6 / 6`
- compatibility view: `nerfstudio`
- trainer backend: `nerfstudio.splatfacto`

## Adapter commands

The live adapter scripts were written under `.tmp/mc8-d4-hardening/bin/`:

- submit adapter: `.tmp/mc8-d4-hardening/bin/submit.py`
- status adapter: `.tmp/mc8-d4-hardening/bin/status.py`
- artifact fetch adapter: `.tmp/mc8-d4-hardening/bin/fetch.py`

The status adapter explicitly consumed the D2 stdin request and returned a
message containing the job id. The artifact fetch adapter materialized normalized
artifacts under the `normalized_result_dir` supplied on stdin.

## Recorded runs

The final recorded pass wrote local run-store evidence with inspect server write
disabled:

- D6 submit run: `run_1782474146691_75094_0`
- D7 status/result run: `run_1782474150531_75179_0`
- D11 local-result artifact fetch run: `run_1782474152595_75586_0`
- D11 command-materialized artifact fetch run: `run_1782474155519_75839_0`

The working evidence directory is `.tmp/mc8-d4-recorded/`.

## Gate results

### D6 submit adapter

Command output recorded:

- `remoteJobStatus: submitted`
- `jobBackend: remote`
- `readinessBlocker: none`
- `job_id: mc8-d4-hardening-job`
- `job_url: https://mc8-live.example.invalid/api/jobs/mc8-d4-hardening-job`

Artifacts:

- `.tmp/mc8-d4-recorded/training-job/minecraft-3dgs-training-job.json`
- `.tmp/mc8-d4-recorded/training-job/minecraft-3dgs-training-job-inspect.json`
- `.tmp/mc8-d4-recorded/training-job/mc7-training-job-runbook.md`

### D7 status/result adapter

Command output recorded:

- `remoteResultStatus: succeeded`
- `statusReason: none`
- `resultStateInterpretation: result_state_matches_current_artifacts`
- `jobId: mc8-d4-hardening-job`
- `resultDir: .tmp/mc7-live/closure/training-launch/trainer-output/nerfstudio-splatfacto`

The inspect report warning proves the explicit status command received stdin and
read the job id:

- `status-command-saw-job_id=mc8-d4-hardening-job`

Artifacts:

- `.tmp/mc8-d4-recorded/training-result/minecraft-3dgs-training-result.json`
- `.tmp/mc8-d4-recorded/training-result/minecraft-3dgs-training-result-inspect.json`
- `.tmp/mc8-d4-recorded/training-result/mc7-training-result-runbook.md`

### D11 local-result artifact fetch

Command output recorded:

- `fetchStatus: succeeded`
- `sourceResultStatus: succeeded`
- `fetchReason: none`
- `requiredArtifactsPresent: true`
- `normalizedArtifactCount: 3`

Artifacts:

- `.tmp/mc8-d4-recorded/training-result-artifacts-local/minecraft-3dgs-training-result-artifact-manifest.json`
- `.tmp/mc8-d4-recorded/training-result-artifacts-local/minecraft-3dgs-training-result-artifact-inspect.json`
- `.tmp/mc8-d4-recorded/training-result-artifacts-local/normalized-result/config.yml`
- `.tmp/mc8-d4-recorded/training-result-artifacts-local/normalized-result/nerfstudio_models/`
- `.tmp/mc8-d4-recorded/training-result-artifacts-local/normalized-result/job_status.json`

### D11 command-materialized artifact fetch

This branch deliberately used a copied D7 result manifest whose `result_dir` was
not locally readable, forcing the MC-8 D3 artifact fetch adapter path.

Command output recorded:

- `fetchStatus: succeeded`
- `sourceResultStatus: succeeded`
- `fetchReason: none`
- `requiredArtifactsPresent: false`
- `normalizedArtifactCount: 3`

The inspect report warning proves the command-materialized path was exercised:

- `artifact-fetch-command-materialized-normalized-result`
- `source result directory was not locally readable; MC-8 D3 artifact fetch command materialized normalized artifacts`

Artifacts:

- `.tmp/mc8-d4-recorded/training-result-artifacts-command/minecraft-3dgs-training-result-artifact-manifest.json`
- `.tmp/mc8-d4-recorded/training-result-artifacts-command/minecraft-3dgs-training-result-artifact-inspect.json`
- `.tmp/mc8-d4-recorded/training-result-artifacts-command/normalized-result/config.yml`
- `.tmp/mc8-d4-recorded/training-result-artifacts-command/normalized-result/nerfstudio_models/`
- `.tmp/mc8-d4-recorded/training-result-artifacts-command/normalized-result/job_status.json`

## D12 read-side verification

`auv-cli inspect` was run for all four recorded runs. The D12 consumer rendered
paired manifest/report summaries and normalized artifact rows.

Key observed lines:

- D6 inspect: `MC-7 Training Jobs:` with `paired_report_artifact=artifact_0002`
- D7 inspect: `MC-7 Training Results:` with `status=succeeded` and `status_reason=n/a`
- D11 local inspect: `MC-7 Training Result Artifacts:` with `normalized_artifacts=3`, `fetch_status=succeeded`, and `normalized_artifact_count=3`
- D11 command inspect: `normalized_artifact kind=config`, `normalized_artifact kind=models_directory`, and `normalized_artifact kind=status_snapshot`

Saved inspect text outputs:

- `.tmp/mc8-d4-recorded/inspect-run_1782474146691_75094_0.txt`
- `.tmp/mc8-d4-recorded/inspect-run_1782474150531_75179_0.txt`
- `.tmp/mc8-d4-recorded/inspect-run_1782474152595_75586_0.txt`
- `.tmp/mc8-d4-recorded/inspect-run_1782474155519_75839_0.txt`

## Verdict

MC-8 D4 closes the adapter live gate for the hardened command-adapter chain:

- D6 accepted a JSON stdin submit adapter and produced a submitted job manifest.
- D7 used an explicit status command over local snapshots and passed JSON stdin.
- D11 copied local trainer result artifacts and also exercised the remote fetch
  command path when the source result directory was unavailable.
- D12 read-side inspection consumed the resulting trainer-side lineage and
  normalized artifact rows.

This does not prove a provider-backed remote training service has run. The next
slice for a real provider would replace the local scripts with provider-specific
commands or a provider adapter while keeping the same persisted artifact roles.

## Validation already completed before D4

The hardening commit `c638c99` was validated with:

```sh
cargo fmt --check
cargo test -p auv-game-minecraft
cargo test --bin auv-cli
git diff --check
cargo check
cargo test
```

The D4 live pass then exercised the post-hardening runtime path through the CLI
commands listed above.
