# Minecraft MC-11: Semantic Read-Side Inspect Consumer Design

Date: 2026-06-27

## Purpose

MC-11 closes the read-side consumer gap for MC-10 training-result semantic gate
artifacts. It consumes the existing MC-10 producer JSON contract only; it does
not change MC-10 schema, add CLI commands, run render preview, or judge trained
splat quality.

## Consumed artifact roles

- `minecraft-3dgs-training-result-semantic`
- `minecraft-3dgs-training-result-semantic-inspect`

## Surfaces closed

1. **`src/run_read.rs`** — lineage extractors and narrow summary types for both
   roles, including mime/parse issue reporting.
2. **`src/inspect.rs`** — `MC-10 Training Result Semantics:` text section with
   business-lineage pairing (not artifact-path pairing).
3. **`src/inspect_server_viewer.html`** — summary cards before raw JSON for both
   roles.

## Pairing rule

Inspect reports pair to semantic manifests when these fields match:

- `source_training_result_artifact_manifest_path`
- `source_training_result_manifest_path`
- `source_training_job_manifest_path`
- `source_training_launch_plan_path`
- `source_scene_packet_manifest_path`
- `source_run_ids`

Duplicate matches leave reports unpaired, matching D12 behavior.

## Explicit non-goals

- No MC-10 producer contract rewrite
- No render preview or checkpoint quality grading
- No trained splat consumption
- No new CLI commands

## Collabi coordination

Collabi writer UI at
`https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`
was reachable (HTTP 200) before edits. No callable automated check-in script or
writer token was available in this session; coordination followed the same
manual-writer pattern recorded in recent MC-9 closure notes.

## Manual viewer smoke

1. Run an MC-10 semantic validation command that records both semantic artifacts.
2. Start the inspect server and open a run containing those artifacts.
3. Select each semantic manifest/inspect artifact and confirm the summary card
   appears above the raw JSON with `semantic_status`, lineage paths, and
   checkpoint/warning counts.
4. Run `auv inspect <run_id>` and confirm `MC-10 Training Result Semantics:`
   renders paired manifest/inspect rows.

## Related references

- MC-10 producer design:
  `docs/ai/references/apps/minecraft/2026-06-27-minecraft-probe-10-reference.md`
- MC-10 live closure:
  `docs/ai/references/apps/minecraft/2026-06-27-minecraft-probe-10-reference.md`
- D12 read-side pattern:
  `docs/ai/references/apps/minecraft/2026-06-26-minecraft-probe-7-reference.md`
