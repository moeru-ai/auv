# Minecraft MC-13: Spatial Query Read-Side Inspect Consumer Design

Date: 2026-06-27

## Purpose

MC-13 closes the read-side consumer gap for MC-12 training-result spatial query
artifacts. It consumes the existing MC-12 producer JSON contract only; it does
not change MC-12 schema, add CLI commands, rerun spatial queries, wire action
paths, upgrade providers, or judge render/splat quality.

**MC-13 = MC-12 query artifact 的 read-side / inspect / generic viewer 消费闭环。**
It only answers whether query results can be read, paired, and audited in
read-side, inspect text, and the generic viewer.

## Consumed artifact roles

- `minecraft-3dgs-training-result-query`
- `minecraft-3dgs-training-result-query-inspect`

## Surfaces closed

1. **`src/run_read.rs`** — lineage extractors and narrow summary types for both
   roles, including mime/parse issue reporting.
2. **`src/inspect.rs`** — `MC-12 Training Result Spatial Query:` text section
   with business-key pairing (not artifact-path pairing).
3. **`src/inspect_server_viewer.html`** — summary cards before raw JSON for both
   roles.

## Pairing rule

Inspect reports pair to query manifests when **all** business keys match:

- `training_result_semantic_manifest_path`
- `source_training_result_artifact_manifest_path`
- `source_training_result_manifest_path`
- `source_training_job_manifest_path`
- `source_training_launch_plan_path`
- `source_scene_packet_manifest_path`
- `source_run_ids`
- `query_kind`
- `target_block` + `target_face` + `target_semantics`

**Do not** pair on artifact path, store path, or
`training_result_spatial_query_manifest_path` string equality against run-store
paths — those are unreliable in recorded runs.

Duplicate matches leave reports unpaired, matching MC-11 / D12 behavior.

## Explicit non-goals

- No MC-12 producer contract rewrite
- No rerun of `query-3dgs-training-result`
- No action / ActionResolver integration (**MC-14** — see
  `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`)
- No Gaussian-native / checkpoint-native provider (**MC-15**)
- No render preview or splat quality gate (**MC-16**)
- No entity / anchor / label query

## Deferred slices

```text
MC-14  action-facing spatial query consumer (see MC-14 design + live closure docs)
MC-15  checkpoint-native / Gaussian-native provider backend
MC-16  render inspect / holdout preview consumer
```

## Collabi coordination

Collabi writer UI at
`https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`
was reachable before prior MC slices. No callable automated check-in script or
writer token was available in this repository; coordination follows the same
manual-writer pattern recorded in MC-9 / MC-11 / MC-12 notes.

## Manual viewer smoke

1. Run MC-12 `query-3dgs-training-result` so both query artifacts are recorded
   (see MC-12 live closure for example commands).
2. Start the inspect server and open a run containing those artifacts.
3. Select each query manifest/inspect artifact and confirm the summary card
   appears above the raw JSON with `status`, `selected_backend`, `visibility`,
   and `target_block`.
4. Run `auv inspect <run_id>` and confirm `MC-12 Training Result Spatial Query:`
   renders paired manifest/inspect rows.

## Live closure

- `docs/ai/references/2026-06-27-minecraft-mc13-spatial-query-read-side-live-closure.md`

## Related references

- MC-12 producer design:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
- MC-12 live closure:
  `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-live-closure.md`
- MC-11 read-side pattern:
  `docs/ai/references/2026-06-27-minecraft-mc11-semantic-read-side-inspect-consumer-design.md`
