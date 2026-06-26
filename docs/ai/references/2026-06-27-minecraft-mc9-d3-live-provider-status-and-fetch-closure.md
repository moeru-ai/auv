# MC-9 D3 live provider-status and fetch closure

Date: 2026-06-27

## Summary

This note records the first fresh local live pass after the MC-9 D3 code slice
landed.

The pass exercised:

```text
D5 launch prep reference
-> D6 real provider submit
-> D7 real provider status truth
-> D11 command-materialized normalized artifact fetch
```

This is still not a cloud-provider training-quality gate. It is a local live
closure for:

- provider acceptance recording on D6
- provider-status truth on D7
- command-materialized normalized artifact fetch on D11

## Input lineage

The pass reused the accepted-only MC-7 closure package:

- training package: `.tmp/mc7-live/closure/training-package/run.json`
- source scene packet: `.tmp/mc7-live/closure/scene-packet/run.json`
- source run count: `6`
- trainer backend: `nerfstudio.splatfacto`

Fresh working directory:

- `.tmp/mc9-d3-live/`

## Recorded runs

- D5 launch-prep reference run: `run_1782493609356_36354_0`
- D6 real-provider submit run: `run_1782493613903_36423_0`
- D7 real-provider status run: `run_1782493616372_36652_0`
- D11 command-materialized fetch run: `run_1782493728710_37763_0`

Saved inspect text snapshots:

- `.tmp/mc9-d3-live/inspect/d6.txt`
- `.tmp/mc9-d3-live/inspect/d7.txt`
- `.tmp/mc9-d3-live/inspect/d11.txt`

## Gate results

### D6 real-provider submit

Observed facts:

- `status = submitted`
- `accepted_by_provider = true`
- `submission_recorded_at_millis = 1782493613989`
- `job_id = mc9-d3-live-job`
- `job_url = https://mc9-live.example.invalid/api/jobs/mc9-d3-live-job`

Artifacts:

- `.tmp/mc9-d3-live/training-job/minecraft-3dgs-training-job.json`
- `.tmp/mc9-d3-live/training-job/minecraft-3dgs-training-job-inspect.json`
- `.tmp/mc9-d3-live/training-job/mc7-training-job-runbook.md`

### D7 provider-status truth

Observed facts:

- `status = succeeded`
- `status_reason = null`
- `status_message = provider-status-saw-job_id=mc9-d3-live-job token=present`
- `result_dir_exists = false`
- `key_result_artifacts_present = false`
- terminal interpretation:
  `provider_status_recorded_local_results_not_yet_observed`

This is the core MC-9 D3 proof. The provider status remained `succeeded` even
though the local result directory had not been fetched yet.

Artifacts:

- `.tmp/mc9-d3-live/training-result/minecraft-3dgs-training-result.json`
- `.tmp/mc9-d3-live/training-result/minecraft-3dgs-training-result-inspect.json`
- `.tmp/mc9-d3-live/training-result/mc7-training-result-runbook.md`

### D11 command-materialized normalized fetch

Observed facts:

- `fetch_status = succeeded`
- `fetch_reason = null`
- `source_result_status = succeeded`
- `source_result_dir_exists = false`
- `required_artifacts_present = false`
- `normalized_artifact_count = 3`

Warnings prove the command-materialized path was exercised:

- `mc9-d3-fetch-command-materialized-normalized-result`
- `source result directory was not locally readable; MC-8 D3 artifact fetch command materialized normalized artifacts`

The `required_artifacts_present = false` field is expected here. It refers to
the **source** D7 manifest's local-result observation. It does not mean the
normalized fetch failed.

Normalized outputs were present:

- `.tmp/mc9-d3-live/training-result-artifacts/normalized-result/config.yml`
- `.tmp/mc9-d3-live/training-result-artifacts/normalized-result/nerfstudio_models/`
- `.tmp/mc9-d3-live/training-result-artifacts/normalized-result/job_status.json`

Artifacts:

- `.tmp/mc9-d3-live/training-result-artifacts/minecraft-3dgs-training-result-artifact-manifest.json`
- `.tmp/mc9-d3-live/training-result-artifacts/minecraft-3dgs-training-result-artifact-inspect.json`

## Verdict

MC-9 D3 is now live-closed for the local provider-status lane:

- D6 records real submit acceptance facts
- D7 records provider status truth independently from local result presence
- D11 can immediately follow and materialize normalized artifacts from a
  source-result-missing branch without rewriting D7 status semantics

This note still does **not** claim:

- cloud-provider execution quality
- trained splat usefulness
- checkpoint semantic validation
- viewer / renderer quality

## Operational note

Local inspect-server writes to `http://127.0.0.1:8765` were unavailable during
this pass. That did not block the local run-store evidence or local artifact
outputs used in this closure note.
