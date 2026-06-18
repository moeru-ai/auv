# 2026-06-18 AUV execution plan — MC-5 onward

> AUV remains a skill substrate. The Minecraft sidecar is read-only truth and
> verifier. All actions go through the AUV driver. KG1/KG2/KG3 from the P0 doc
> remain in force. New for this phase — **KG4**: the realtime daemon is a
> substrate, never an agent (no perceive→decide→act loop; the session holds
> state, not goals).

Date: 2026-06-18

Status: proposed execution plan, written for slice-by-slice execution by Claude
Code on the Mac. Continues `2026-06-16-minecraft-live-mc2-mc4-closure-plan.md`
(MC-2/3/4 are live-closed) and its "Post-MC-4 sequencing" section. The planning
sandbox is Linux and cannot build the macOS crates.

## Scope classification

`docs-only`. Sequences already-decided slices into executable form. Adds no
contract, core surface, or noun by itself. Each slice names its own gate; nothing
here approves widening beyond the slice in hand.

## Where we are (entry state)

- MC-2/3/4 are **live-closed** with recorded run ids (see the closure plan).
- Post-MC-4 sequencing is decided: ① MC-5 graduation + clean PR → ② core realtime
  daemon (the unblock) → ③ MC-6 measurement (offline, parallel) → ④ MC-7 3DGS
  (parked behind a three-judge gate).
- Update after Slice A/B local implementation: MC-5 G2/G3/G4 minimal core
  shapes have landed with osu + Minecraft consumers, and the core realtime
  session substrate design note plus in-process warm-provider proof have landed.
  The `.codex-worktrees/realtime-session-substrate/` worktree remains untracked
  and should not be treated as committed project state.

## Kill gates (carried + extended)

```text
KG1  No real camera matrix => projection is not trusted.                                 (carried)
KG2  Any action via Mineflayer / MCP / mod => the line FAILS. Real driver input only.    (carried)
KG3  No mismatch-refusal evidence => a slice that should refuse is not done.              (carried)
KG4  The realtime daemon runs no perceive→decide→act loop and generates no goals.        (new)
```

## Slice A — MC-5: graduate G2/G3/G4 to core (minimal common shape)

Status: implemented locally in `feat(minecraft): graduate mc5 evidence shapes`.

osu + Minecraft are now the two real consumers the parked G-gates were waiting
for. Lift **only the generic shape** to core; both verticals then consume it.

- G3 — same-instant timestamped capture binding.
- G2 — frame / action / target / verification correlation key.
- G4 — source→screen projection basis + projected coordinate space.

Core must never learn vertical nouns (`block_id` / `chunk` / `creeper` / beatmap
/ hit-object). Design-note-first: extend
`2026-06-15-minecraft-mc5-graduation-design-closure.md` (G2/G4 still open).

Acceptance gate:

- G2/G3/G4 minimal shapes live in core: `crates/auv-driver/src/geometry.rs`
  (World/Camera space + projection basis), `crates/auv-inference-common`
  (projected `DetectionCoordinateSpace` variant), correlation key in
  `crates/auv-tracing-driver/src/trace.rs`.
- **Both** `auv-game-osu` and `auv-game-minecraft` re-pointed to consume the core
  shape — the proof it is common, not MC-shaped.
- No vertical noun in core; no third action-result schema.
- `cargo fmt --check && cargo check && cargo test && git diff --check`.

Finish, report the graduated symbols + the two re-pointed call sites, stop.

## Slice B — CORE realtime session substrate (the ② unblock; NOT an MC slice)

Status: design note and in-process substrate implemented locally; transport
remains deferred.

Realize the **minimum realtime slice** of the v0 direction
(`2026-06-10-stateful-session-daemon-js-repl-v0.md`): a warm-model, stateful
session held across calls. This is a **core-lane** capability consumed by every
vertical — it does not live in `auv-game-minecraft`.

**Step B0 (design-note-first, BLOCKING):** write a core-lane design note
(`2026-06-18-core-realtime-session-substrate-slice-design.md`) before any code,
since this touches core runtime. It must resolve one architecture fork —
transport / process model: **reuse the proven `src/inspect_server` local HTTP/WS**
(v0-aligned default, recommended) vs a separate daemon process — and pin
warm-model residency (session-scoped vs daemon-global). The existing
`.codex-worktrees/realtime-session-substrate/` worktree must not write core code
until B0 lands.

Then implement the minimum slice, reusing anchors: `Device` / `Session`
(`docs/TERMS_AND_CONCEPTS.md`), `DeviceId` / `SessionId` (`src/trace.rs`), session
already threaded through `DriverRunContext`, the `inspect_server` transport
precedent.

Red line (KG4): the session holds perceptual/spatial state only; the daemon
exposes `observe` / `act` / `verify` on request and streams observations; no
perceive→decide→act loop; no goal generation; thin frontend preserved.

Acceptance gate:

- A warm detector held across N `observe` calls **without reload** (kills the
  cold-start pain).
- A session persists posed observations across calls and answers one lookup query
  — the cheap floor (keyframe + camera pose + detector boxes; "seen? / where?").
- `observe` / `act` / `verify` reuse the existing seam unchanged; no new
  action-result schema; no agent loop.
- At least one vertical (osu or MC) drives the substrate as the second consumer.
- `cargo fmt --check && cargo check && cargo test && git diff --check`.

Finish, report the warm-call count + the lookup result + the consuming vertical,
stop.

## Slice C — MC-6: spatial dataset recorder + 2.5D-baseline measurement (offline)

Offline; does **not** need the daemon; may run in parallel with A / B.

Status: design note and local recorder/measurement substrate implemented. The
design note is
`2026-06-18-minecraft-mc6-spatial-dataset-measurement-design.md`. Local code now
records `minecraft-spatial-frame` artifacts, exports MC-6 bundle manifests via
`auv-cli minecraft export-spatial-bundle <run-id> --output-dir <dir>`, and
evaluates precomputed texture-sweep samples via
`auv-cli minecraft eval-texture-sweep --samples <json> --output-dir <dir>` using
the pre-set v0 thresholds. Closure runs must add `--require-real-source`, which
rejects missing source blocks and fixture/smoke/test generators unless the
sample source cites source run ids plus bundle manifests. The sidecar now
records `resource_pack_ids` on each telemetry sample and the sweep evaluator
records both the input sample file and the report as run artifacts.

Preparation-only substrate now exists for the next live pass:
`auv-cli minecraft prepare-texture-sweep --sidecar-run-dir <dir> --output-dir <dir>`
generates the K=3 local resource-pack profiles and a runbook, and
`auv-cli minecraft build-texture-sweep-samples --bundle-manifest <bundle/run.json>... --output <samples.json>`
builds the evaluator input from real exported spatial bundles. These commands
do **not** launch Minecraft or close MC-6. The real K-pack live/offline sweep has
**not** been run yet; do not treat MC-6 as numerically closed until that table
exists from real samples whose source block cites the run ids / bundle
manifests.

Before continuing MC-6 prep, read
`2026-06-18-minecraft-mc6-run-preparation-exploration.md`. It records the
current local evidence inventory, sidecar state, and the "prepare only; do not
run live chain yet" boundary so the next pass does not rescan the same `.auv`
and sidecar state.

C1 — recorder: each run → a bundle (`screenshots/`, `spatial_frames/`,
`actions/`, `verification/`, `overlays/`, `run.json` with versions + commits).
The labeled gym, not present showmanship.

C2 — the 2.5D-baseline texture sweep, with a **numerical** acceptance gate
(subjective "2.5D looks ok" is banned — that only moves the mouth-decision to a
later time). Set the thresholds **before** running:

```text
pose error p95         <  N px        (set N before the run)
occlusion IoU          >  M           (set M before the run)
resource packs         =  K           rich → flat-color → repetitive
per-pack duration      =  T seconds
refuse-on-noise rule   defined and exercised
```

Compute 2.5D keyframe-cache pose/occlusion error vs the mod's raycast + matrix
ground-truth across the K packs. This table is the **only** input that decides
session-floor vs 2.5D vs 3DGS — by number, not argument.

Acceptance gate: bundle schema recorded + read-side visible; the sweep runs
across K packs with `--require-real-source` and emits the p95 / IoU table;
pass/fail is read off the pre-set thresholds. The table must come from real
sample provenance, not the evaluator's fixture or smoke data. Finish, report the
table, stop.

## Slice D — MC-7: 3DGS (PARKED — do not start)

Do not start. Recorded only to fix the open-gate. 3DGS opens only when all three
independent judges point "yes":

- **technical forcing** — Slice C's numbers (does 2.5D blow up on flat-color /
  repetitive textures?). Benchmark, not argument.
- **refusal-seam shape needs it** — read the contract. The live MC-4 matrix (7
  classes) needs **zero** dense photometric comparison; a "dense mismatch" class
  would be a separate, explicit contract decision.
- **market forcing** — owner product judgment about who pays for an API-denied /
  feature-poor / depth-less 3D surface. Not decidable by benchmark or advisor.

If ever opened, the first version is an **offline inspect artifact only** (P0 §8),
never in the action path until earned.

## What graduates vs what stays

```text
MAY graduate to core (Slice A / B; owner-approved design note; no vertical nouns):
  G3 same-instant capture binding · G2 correlation key · G4 projection basis + coord space
  the realtime session/daemon substrate (core by construction)

NEVER graduates (stays in the vertical crates):
  block / face / chunk / entity / inventory semantics, beatmap / hit-object semantics,
  the sidecar wire format, per-game camera quirks, the play-task policy
```

## What to avoid

- No third action-result schema beside `ActionResolverDecision` /
  `InputActionResult`.
- No vertical nouns (MC or osu) graduated to core.
- No Mineflayer / MCP / mod action path (KG2). Sidecar stays read-only truth.
- No agent loop or goal generation in the daemon (KG4).
- No Slice B code before its B0 design note; no starting Slice D at all.
- No drive-by refactor — one slice in hand, finish, report, stop.

## Fast restart checklist

Re-read before editing: `CLAUDE.md`, `AGENTS.md`,
`2026-06-14-auv-3d-minecraft-spatial-skill-p0.md`,
`2026-06-16-minecraft-live-mc2-mc4-closure-plan.md` (incl. its Post-MC-4
sequencing), `2026-06-10-stateful-session-daemon-js-repl-v0.md`.

Order: **start at Slice A**; Slice C may run in parallel (offline); Slice B is
gated on its B0 design note; Slice D is parked. Finish, report, stop, let the
owner pick next.
