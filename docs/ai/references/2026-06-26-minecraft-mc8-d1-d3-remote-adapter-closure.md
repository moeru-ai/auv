# MC-8 D1-D3 remote trainer adapter closure

Date: 2026-06-26

## Summary

MC-8 D1-D3 moves the MC-7 trainer-side chain from blocked-only local evidence
into explicit adapter seams for a remote training backend. The slice still does
not evaluate model quality, preview a splat, inspect checkpoints, or claim that
a real remote training job has succeeded.

The closed surface is adapter readiness:

```text
D6 launch job -> D7 collect status/result -> D11 fetch normalized artifacts
```

Each adapter is command-based and JSON-shaped so a real remote backend can be
wired without adding a cloud-specific SDK or changing the MC-7 artifact schemas.

## D1: remote submit adapter

`auv-cli minecraft launch-3dgs-training-job` can now receive explicit remote
submission configuration:

```sh
auv-cli minecraft launch-3dgs-training-job \
  --training-launch-plan <training-launch-plan.json> \
  --output-dir <dir> \
  --training-job-endpoint <url> \
  --training-job-token <token> \
  --training-job-submit-command <command>
```

The submit command receives a JSON `TrainingLaunchJobRequest` on stdin and must
write a JSON `TrainingLaunchJobSubmission` on stdout. A submitted response must
include `job_id`; otherwise the operation records `submission_failed` instead of
pretending success.

## D2: remote status/result adapter

`auv-cli minecraft collect-3dgs-training-job-result` can now receive an explicit
status command:

```sh
auv-cli minecraft collect-3dgs-training-job-result \
  --training-job-manifest <training-job.json> \
  --output-dir <dir> \
  --training-job-endpoint <url> \
  --training-job-token <token> \
  --training-job-status-command <command>
```

The status command writes a JSON status snapshot compatible with
`job_status.json`. Malformed output or command failure is recorded as
`remote_status_unavailable`; it is not treated as success.

## D3: remote artifact fetch adapter

`auv-cli minecraft fetch-3dgs-training-result-artifacts` can now receive an
artifact fetch command:

```sh
auv-cli minecraft fetch-3dgs-training-result-artifacts \
  --training-result-manifest <training-result.json> \
  --output-dir <dir> \
  --artifact-fetch-command <command>
```

The artifact fetch command receives a JSON request on stdin. It must materialize
normalized result artifacts under the provided `normalized_result_dir`:

- `config.yml`
- `nerfstudio_models/`
- optional `job_status.json`

After the command returns successfully, AUV verifies those normalized artifacts
from the filesystem and writes the existing D11 manifest / inspect outputs. If
required files are missing, the fetch is recorded as `failed` with a copy/fetch
failure reason.

## Boundaries

This slice intentionally does not:

- add a provider-specific remote job SDK;
- install or invoke local `ns-train`;
- inspect `nerfstudio_models/` internals;
- score model quality or trained splat usefulness;
- change the D5/D6/D7/D11/D12 persisted artifact roles;
- require a fresh Minecraft capture.

The follow-up MC-8 D4 adapter live gate exercised this command-adapter path
through D6, D7, D11, and D12. See
`docs/ai/references/2026-06-26-minecraft-mc8-d4-adapter-live-closure.md`
for the recorded evidence. A provider-backed remote training run remains a
separate future slice.

## Validation

Targeted validation for this slice covered:

- D1 command submission success and missing-job-id failure;
- D2 status command success and malformed-output blocked path;
- D3 artifact fetch command success when local result dir is missing;
- D3 artifact fetch command failure when required normalized artifacts are not
  materialized;
- CLI parsing for the new submit, status, and artifact fetch command flags.

Full validation should still include the repository standard commands before
committing:

```sh
cargo fmt --check
cargo check
cargo test -p auv-game-minecraft
cargo test --bin auv-cli
cargo test
git diff --check
```
