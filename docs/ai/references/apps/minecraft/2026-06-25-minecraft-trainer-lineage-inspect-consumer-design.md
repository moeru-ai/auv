# 2026-06-25 Minecraft MC-7 D8 trainer-lineage inspect consumer design

Date: 2026-06-25

Status: implemented code slice target; D8 consumes existing D5/D6/D7 JSON artifacts through read-side, text inspect, and the inspect viewer. It is not a trainer execution or quality gate.

## Scope

MC-7 D8 is defined as the first unified trainer-side inspect consumer for:

- D5 `minecraft-3dgs-training-launch-plan` and `minecraft-3dgs-training-launch-inspect`
- D6 `minecraft-3dgs-training-job` and `minecraft-3dgs-training-job-inspect`
- D7 `minecraft-3dgs-training-result` and `minecraft-3dgs-training-result-inspect`

It does not add CLI commands, change D5/D6/D7 write contracts, parse runbook Markdown, or inspect trained splat quality.

## Read-side contract

D8 treats the D5/D6/D7 manifests and inspect reports as the authoritative read sources. It adds run-read lineage summaries beside the D4 training-package reader:

- artifact lineage and role/path metadata
- parsed manifest/report summary when JSON is readable
- issue text when MIME, file, or JSON parsing fails

Pairing between manifest and inspect artifacts is based on lineage fields, not artifact filenames or runbook paths.

## Inspect text and viewer behavior

Text inspect adds three sections after `MC-7 Training Packages:`:

- `MC-7 Training Launches:`
- `MC-7 Training Jobs:`
- `MC-7 Training Results:`

The inspect viewer recognizes the six D5/D6/D7 JSON artifact roles and renders lightweight summary cards before the raw JSON preview. The original artifact link/download surface remains unchanged.

## Non-goals

D8 deliberately does not close:

- real-source trainer validation
- remote job execution correctness beyond D6/D7 recorded status
- trained splat quality
- downstream 3D viewer or model consumption

If future work needs quality evaluation or splat consumption, it should open a separate D9-style slice rather than extending D8.

## Compatibility note

D8 consumes the existing D5/D6/D7 schema as-is. Any historical naming awkwardness in those schemas is displayed neutrally by the read side and should not be corrected inside this consumer slice.
