# 2026-06-24 Minecraft MC-7 D2 accepted-only scene-packet inspect reference

Date: 2026-06-24

Status: implemented code slice; synthetic validation closed. Historical
accepted-only lineage is no longer restorable, so any future real reference
validation requires a fresh accepted-only capture lineage.

## Scope

This note records the accepted truth for MC-7 D2:

- D2 stays on the existing `auv-cli minecraft export-3dgs-scene-packet`
  command surface.
- D2 adds a machine-readable `inspect_report.json`.
- D2 records that inspect output as a second staged artifact with role
  `minecraft-3dgs-scene-packet-inspect`.
- D2 does not add training, renderer, viewer-only CLI, or new acceptance gates.

## Input boundary

The first D2 packet is still defined as **accepted-only**. The intended real
input runs are:

- `run_1782281195384_53866_0`
- `run_1782281378426_54930_0`
- `run_1782283214391_69376_0`
- `run_1782283416142_70808_0`
- `run_1782283850920_75557_0`
- `run_1782284010123_76280_0`

`refusal-menu` runs remain side evidence and do not enter the first accepted
scene packet.

## D2 inspect output contract

The exporter now writes four files under the scene-packet output directory:

- `run.json`
- `cameras.json`
- `known_limits.json`
- `inspect_report.json`

`inspect_report.json` records:

- packet and source lineage
- counts for frames, screenshots, missing screenshots, camera records, source
  runs, and `file/*` resource-pack profiles
- resource-pack coverage rows built only from `file/*`
- anomaly indices using the 1-based packet `frame_index`
- warnings
- final known limits

## D2 anomaly semantics

These continue export and enter the inspect report:

- missing screenshot ref
- screenshot artifact ref that does not resolve in the bundle manifest
- `screen_state != in_game`
- no `file/*` resource pack
- multiple `file/*` resource packs

This still hard-fails:

- screenshot artifact resolves, but the underlying bundle file is physically
  missing and cannot be copied

## D2 wording cleanup

The stale D1 wording:

- `MC-7 D1 scene packet is 3DGS input material only; no trained splat is present`

was intentionally replaced with stage-neutral wording:

- `MC-7 scene packet is 3DGS input material only; no trained splat is present`

This prevents D2 inspect output from presenting itself as leftover D1 output.

## Validation state

Code-layer validation for D2 is expected to use synthetic temporary bundles
only. The implementation adds coverage for:

- the synthetic 6-bundle / 6-frame / 6-screenshot / 3-profile happy path
- missing screenshot anomaly continuation
- no `file/*` anomaly continuation
- multiple `file/*` anomaly continuation
- non-ingame anomaly continuation
- resolved screenshot artifact with missing file hard error

Real reference validation is currently **not runnable on the historical six
accepted runs** because that accepted-only lineage is no longer recoverable
from the cleaned local state.

## Real reference validation checklist

If a future fresh accepted-only capture is collected, the real D2 reference
pass should record:

1. the new accepted-only capture lineage used for the reference pass
2. the six accepted input bundle manifests actually used
3. the scene-packet output directory
4. the raw `inspect_report.json` readings
5. whether that output is sufficient to justify the next training/backend
   discussion

Do not invent expected real counts from fixtures or stale notes. If no fresh
accepted-only lineage exists, this note remains code-closed but real-source
unvalidated.
