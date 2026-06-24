# 2026-06-24 Minecraft MC-6 to MC-7 handoff

Date: 2026-06-24

Classification label: `implementation handoff`.

Purpose: freeze the exact state at the end of the MC-6 closure slice and make
the next MC-7 slice resumable without reopening MC-6 status, rerunning live
Minecraft collection, or drifting back into stale "MC-6 is still open" plan
text.

## Status note

This document is a **historical transition handoff**, not the authoritative
current-state note for MC-7.

Read boundary:

- this file preserves the exact reasoning and resume shape at the moment MC-6
  closed and MC-7 was about to open
- current accepted MC-6 truth lives in
  `docs/ai/references/2026-06-24-minecraft-mc6-dual-gate-closure-reference.md`
- current accepted MC-7 D2 truth lives in
  `docs/ai/references/2026-06-24-minecraft-mc7-d2-accepted-only-scene-packet-inspect-reference.md`

Current repo truth at commit time:

- `68c2605 feat(minecraft): close mc6 dual-gate sweep` was the handoff draft
  point recorded below
- `1c0787e feat(auv-game-minecraft): add mc7 scene packet inspect report`
  already landed the D2 code slice that this handoff recommended

Do not read this file as saying MC-7 D2 is still undone. Read it as the
preserved bridge between MC-6 closure and the now-landed D2 implementation.

## Historical handoff snapshot

Live repo truth at handoff draft time:

- branch: `main`
- head commit at draft time: `68c2605 feat(minecraft): close mc6 dual-gate sweep`
- local branch status at draft time: `main...origin/main`

MC-6 status at handoff:

- **closed under the current dual-gate contract**
- Gate 1 geometry: already passed on the fresh 2026-06-24
  `window.capture` lineage
- Gate 2 completeness: passed on the fresh 9-run live sweep with
  `--require-real-source`

Do not reopen MC-6 just because older notes still contain the historical fail
state. Those older notes are still valid as historical evidence, not as the
current status.

## MC-6 closure artifacts

Current accepted Gate 2 closure artifacts:

- sample-build run:
  `.auv/runs/run_1782284483150_79654_0/artifacts/artifact_0001_texture_sweep_samples.json`
- eval run:
  `.auv/runs/run_1782284485217_79709_0/artifacts/artifact_0002_texture_sweep_report.json`
- local eval copy during the closure slice:
  `.tmp/mc6-live-a-20260624/eval/texture_sweep_report.json`

Final report reading:

- `actual_resource_pack_count = 3`
- `noise_refusal_exercised = true`
- `passed = true`

Per-profile closure rows:

- `file/auv-mc6-flat-color`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 159.244`
- `file/auv-mc6-repetitive`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 183.035`
- `file/auv-mc6-rich`
  - `sample_count = 2`
  - `refused_noise_count = 1`
  - `duration_seconds = 201.762`

Read boundary:

- this is a **coverage proof**
- this is **not** a richer geometry metric proof
- `pose_error_p95_px = 0.0` remains the intentional bridge-only v0 semantics

## MC-6 docs that now carry the accepted truth

Read these first before touching any MC-6/MC-7 status wording:

- `docs/ai/references/2026-06-24-minecraft-mc6-dual-gate-closure-reference.md`
- `docs/ai/references/2026-06-24-minecraft-mc7-d2-accepted-only-scene-packet-inspect-reference.md`
- `docs/ai/references/2026-06-24-minecraft-mc6-canonical-staging-artifact.md`
- `docs/ai/references/2026-06-24-minecraft-mc6-canonical-clean-rebuild-fail-record.md`
- `docs/ai/references/2026-06-19-minecraft-mc6-texture-sweep-gate-verdict.md`

Interpretation rule:

- the `2026-06-19` verdict and the clean fail-record remain historical
  fail/projection-debug evidence
- the `2026-06-24` dual-gate closure reference is the current truth

## MC-7 status at handoff

At handoff draft time, MC-7 was **already opened**, but only as an offline
inspect-artifact lane.

What already exists in code:

- CLI command surface:
  `auv-cli minecraft export-3dgs-scene-packet`
- implementation:
  `crates/auv-game-minecraft/src/scene_packet.rs`
- command wiring / artifact persistence:
  `src/minecraft.rs`
- CLI parse coverage:
  `src/cli.rs`

At handoff draft time, current MC-7 scope was still D1:

- scene packet export from real MC spatial bundles
- inspect artifact only
- no trained splat
- no action-path dependency
- no refusal-taxonomy change

This means the next slice is **not** "invent MC-7 from zero". The scene-packet
exporter already exists. The next useful slice must start from that fact.

Current-state note:

- that recommended next slice has since landed as MC-7 D2 in `1c0787e`
- do not reopen D1-vs-D2 planning from this file alone; confirm against the
  D2 reference note first

## Historical next-slice recommendation

Yes — **the next serious Minecraft slice should be MC-7**, not another MC-6
rebuild.

But be precise about which MC-7:

- **recommended next slice: MC-7 D2**
- meaning: stay offline and inspect-artifact-first
- do **not** jump straight to training-runtime plumbing unless you first prove
  the exported scene packet is the right input shape

## Why MC-7 next is the correct order

Because MC-6 has already done the one thing it needed to do:

- it produced a passing real-source table
- it no longer blocks forward motion
- it no longer justifies more closure churn

So if the goal is to continue the Minecraft spatial-memory line, the next
non-fake move is to use the real MC-6/MC-7 bundle lineage as input for the next
offline representation artifact.

## Recommended MC-7 D2 shape

The next slice should answer one narrow question:

> Can the current MC-7 scene packet be inspected and judged as a viable 3DGS
> input object from real closure-grade bundle inputs?

Recommended D2 scope:

1. take the fresh MC-6 closure bundles, not stale fixture bundles
2. export an MC-7 scene packet from those real bundles
3. record coverage / frame-count / missing-screenshot / camera-count facts in a
   durable inspect note
4. if useful, add a tiny read-side summary artifact or report for the scene
   packet
5. do **not** add training, splat rendering, or new runtime dependencies in the
   same slice

Good D2 outputs:

- a real scene packet exported from the 2026-06-24 closure bundles
- a durable reference note saying whether the packet shape is enough for the
  next training/backend decision
- maybe a narrow manifest/read-side summary artifact if inspection is too manual

Bad D2 scope creep:

- bundling in trainer selection
- remote job orchestration
- expected-view rendering that pretends to be a real splat renderer
- action-path integration
- contract changes to refusal semantics

## Historical starting recommendation

Use the fresh MC-6 closure bundle lineage, not the historical three-run staging
lineage.

The immediate first move in the next slice should be:

1. re-export the 9-run closure bundles if needed from the accepted bridge runs
2. feed those bundle manifests into
   `auv-cli minecraft export-3dgs-scene-packet`
3. inspect the resulting packet shape and missing-data profile

If the next agent wants one question answered first, it should be:

- does MC-7 D2 consume all 9 bundles, or should it consume only the accepted
  bundle subset for the first real packet?

My recommendation:

- start with the **accepted-only subset** first for the first D2 truth pass
- then decide explicitly whether refusal-menu frames belong in the scene packet
  contract or only in side evidence

Reason:

- MC-7 is about representation input quality, not about replaying the MC-6
  refusal rule
- mixing refusal-menu frames into the first packet without an explicit contract
  decision is avoidable ambiguity

## What not to forget

- keep using Chinese in user-facing summaries unless requested otherwise
- verify live git state before claiming a new slice is "done"
- keep MC-7 offline/read-side only unless the owner explicitly widens scope
- do not let stale docs drag the next slice back into "MC-6 might still be open"

## Resume checklist

When resuming from this handoff:

1. re-open this file first
2. verify `git status --short --branch`
3. re-read:
   - `docs/ai/references/2026-06-24-minecraft-mc6-dual-gate-closure-reference.md`
   - `docs/ai/references/2026-06-18-minecraft-mc7-offline-3dgs-inspect-artifact-design.md`
   - `docs/ai/references/2026-06-18-auv-mc5-onward-execution-plan.md`
4. verify the existing MC-7 command surface in:
   - `src/cli.rs`
   - `src/minecraft.rs`
   - `crates/auv-game-minecraft/src/scene_packet.rs`
5. open MC-7 as D2 scene-packet inspection on real closure-grade bundle inputs

## Bottom line at handoff draft time

If someone asks "下一刀是不是 MC-7":

- **yes**

If someone asks "是哪种 MC-7":

- **MC-7 D2: real-bundle scene-packet inspection, still offline, still not training-first**
