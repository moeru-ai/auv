# Phase 3 — Mainline Acceptance Rules

Date: 2026-05-22

Status: accepted as drift gate

## Why this exists

AUV's mainline arrow is:

```
probe -> analyze -> candidate/annotation -> distill -> validate -> package -> inspect/replay
```

The last few sessions have produced a lot of work that is **adjacent**
to that arrow — overlay sprites, dual-cursor animation, smartPress as
a discovery vehicle, viewer polish. None of it is wrong; some of it is
genuinely useful (the screenshot-first inspect/replay surface in
particular). But the cumulative pattern is recognizable: **debug
presentation surface grows while the validated-narrow-skill count
stays flat**. If that pattern continues, AUV becomes "a Claude
computer-use skin pack with extra tracing" instead of a recipe
distillation runtime.

This document defines the gates that keep mainline mainline. Each rule
is paired with an enforcement plan: **code-gate** (refused at
`cargo test`), **review-gate** (refused at PR/commit time), or
**doc-gate** (recorded in `docs/ai/references/` and audited).

## The rules

### Rule 1 — smartPress is not a production default

`debug.smartPress` is a discovery vehicle: it tries AX first and
falls back to pointer click when AX fails. That is genuinely useful
when **measuring** AX coverage. It is genuinely dangerous when used as
the default press step inside a product-namespace recipe, because:

- the recipe consumer cannot tell from the recipe alone which mode
  actually ran on any given invocation
- a "successful" run that fell back to pointer-click looks identical
  in status to one that pressed via AX
- "smart" wrappers in GUI automation invariably accumulate hidden
  state and become unmaintainable

**Code-gate**: any step with `command_id == "debug.smartPress"` must
satisfy ONE of:

- recipe id starts with `macos.demo.` (demo namespace = explicit
  presentation surface, not product), OR
- the step declares
  `mainline_exemption: { reason: "...", category: "discovery"|"experiment"|"reverification" }`

The exemption marker forces the recipe author to write the reason
into the JSON. The audit doc (`<date>-phase-3-mainline-audit.md`)
lists every active exemption, so checkbox-ism is bounded.

### Rule 2 — smartPress steps cannot live in validated cases

If any step in a recipe uses `debug.smartPress`, no case in the
case-matrix may have `status == "validated"`. Reason: validation
means "this recipe achieves its objective via the contract it
declares". A smartPress step doesn't declare which mode it runs.
Promoting a case to `validated` would hide that ambiguity.

A smartPress recipe's promotion path is:

```
candidate -> evidence (N hands-off replays + recorded smartPress.strategy distribution)
          -> spawn a non-smart child recipe (fixed to whichever strategy actually works)
          -> validate the child
```

**Code-gate**: `validate_skill_case_matrix_against_manifest` rejects
`status: validated` when the paired recipe contains any smartPress
step.

### Rule 3 — overlay is presentation, never proof

The overlay (dual cursor, animation, click ripple, pixel sprite) is
allowed everywhere. It is **never** evidence that the underlying
action succeeded.

- `overlayShowEvent=shown` does NOT mean "the cursor reached the
  target" — it means "the overlay window was ordered front"
- `dualCursor=true` does NOT mean "no real input happened" —
  `cursorDisturbance` is the only signal that proves that
- a smooth visual demo with no errors on screen does NOT mean the AX
  press fired — `performedAction=AXPress` in the artifact does

**Review-gate**: any case promoted to `validated` must cite at least
one **contract-shaped signal** (e.g. `cursorDisturbance=none`,
`performedAction=AXPress`, `ax.node_found=true`) — not an overlay
event or visual outcome.

### Rule 4 — QQ音乐 / 网易云 are not ad-hoc demo subjects

Music apps came into the repo as the original narrow-skill subjects
under Phase 1. They have a specific freeze-doc boundary
(Chinese requested-title semantic selection unresolved). Continuing
to add v2/v3/v4 variants whenever a new primitive lands is drift,
not progress.

**Doc-gate**: any new recipe under `recipes/macos/qqmusic/` or
`recipes/macos/netease-cloud-music/` must reference (in its
`evidenceRefs` or commit message) which Phase 1 boundary it is
moving and what evidence shape the new recipe produces. "I wanted to
try smartPress on it" is not a sufficient evidence shape.

### Rule 5 — inspect serve is read-only

The browser viewer and inspect-server HTTP/WebSocket surface read run
state. It must not gain mutation/control endpoints — no "rerun this
case" button, no "press this AX node" REST call, no recipe upload
form. The viewer is for **after-the-fact replay**, not remote
control.

**Review-gate**: any PR that adds POST/PUT/PATCH/DELETE under
`inspect_server` is rejected without explicit out-of-band approval.

### Rule 6 — Phase 3 doesn't pre-emptively chase YOLO / realtime tracking

Computer-vision-based realtime UI element tracking (YOLO,
EfficientDet, real-time detector loops) is tempting because it
solves the "AX tree doesn't reach canvas elements" gap. But adding
it without a **target spec** (what should be detected), **detector
contract** (input/output shape), and **verification contract** (how
we know it detected the right thing) just replaces the AX black box
with a vision black box.

**Doc-gate**: any commit that introduces a CV-style detector must
first produce three docs under `docs/ai/references/`:
1. target spec — what UI element classes
2. detector contract — input image format, output bbox/class shape
3. verification contract — how a probe artifact proves a detection
   is correct

### Rule 7 — recipe writing requires probe provenance

A recipe should not exist before a probe artifact proves the target
surface is stable enough to recipe-ify. The historical pattern that
got us into drift this session was: someone runs a one-off
`auv-cli invoke debug.foo` that works, then writes a recipe around
it. The recipe inherits the one-off's luck.

**Provenance field (recommended for now)**:
```json
"provenance": {
  "probe_artifact_id": "artifact_..." | "phase-1-grandfathered",
  "probe_run_id": "run_..." | null,
  "probe_summary": "Notes 新建备忘录 AX-pressable across 5 hands-off probes"
}
```

**Enforcement schedule**:
- Today: **doc-gate**. Audit doc records which recipes lack
  provenance.
- Next: **code-gate** as required field once the audit reaches zero
  legacy violations OR a clear grandfather-marker is accepted.

This rule does not gate today's commit because making it a hard
schema requirement would break 11+ existing recipes. The audit
(`<date>-phase-3-mainline-audit.md`) lists every legacy recipe and
either captures a retroactive probe reference or marks it as
`phase-1-grandfathered`.

## Why these are mostly code-gates, not just docs

`docs/ai/references/archive/phase-history/2026-05-18-phase-1-freeze.md` and the Phase 2
freeze doc both said honest things about the project's boundaries.
This session still managed to:

- promote a Notes case to `validated` from a single configurator-
  assisted smoke (then watch Codex demote it on hands-off failure)
- claim multiple "pushed to main" commits that turned out to live on
  fork branches that got reset
- write a smart-press recipe with disturbance class drift that only
  Codex's hardened gate caught

Docs alone don't stop these mistakes. The agent (often me) reads the
doc, agrees with it, and then walks past it under deadline pressure.
The only durable enforcement is when `cargo test` goes red.

## The mainline-aligned next moves

This doc gates new work; it does not by itself produce skills. The
work that does sit on the mainline arrow and should be the next
priority:

1. **Probe more, recipe less.** Use `auv-cli app probe` to walk a
   target app's AX surface across realistic state variations, store
   the probe artifacts as input to future recipe candidates.
2. **Distill from probe artifacts.** Take an existing probe set and
   actually run the `auv-cli app analyze | distill | validate`
   pipeline end-to-end — the runtime exists, the recent sessions
   have not produced any new artifact through it.
3. **Re-evaluate Chinese semantic selection.** Phase 1 freeze's
   one unresolved boundary. With smartPress + overlay evidence
   available, design a discovery experiment (not a new product
   recipe) that produces evidence for whether Chinese row matching
   is feasible at all. Result goes into `evidenceRefs/`, not into a
   new validated case.

These are mainline. Visual polish, more demos, more clever
combinators are not — they belong in the demo namespace or behind
explicit exemptions.

## What this document does NOT say

- It does not say smartPress is bad. It says smartPress in a
  product-namespace default is bad.
- It does not say overlay/dual-cursor work was wasted. The screenshot-
  first inspect surface that Codex built around overlay_evidence is
  directly mainline-aligned.
- It does not say the viewer is wrong. The viewer reads runs; it
  serves the inspect/replay step at the end of the mainline arrow.
- It does not say "no more Phase 3". It says Phase 3 work that is not
  on the mainline arrow needs an exemption or a redirection.

## Sign-off

This is the gate. Future agents (including me): if a commit message
or recipe declaration would fail Rule 1–7, **stop and reframe** —
don't paper over with cleverer code.
