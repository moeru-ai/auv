# CLAUDE.md

Claude-specific operating guidance for AUV. `AGENTS.md` is authoritative;
this file is a short operating card for staying inside the current project
phase.

## Current Mode

AUV is now exploring its next-generation spatial substrate: 3D grounding,
offline 3DGS inspect artifacts, and view-dependent verification evidence for
future 3D surfaces that do not expose truth directly. The former SkillBundle
surface remains retired; do not reintroduce bundle execution, export, or
verification as compatibility. Do not treat one archived macOS AX proof as the
product center, and do not spend active roadmap budget polishing the archived
vertical.

Current validation boundary:

- One real macOS AX copilot vertical is proven and archived.
- That proof is specific to local TextEdit-style execution and remains a
  recoverable reference, not the current roadmap center.
- New work may extend AUV through the approved spatial-substrate lane described
  in `docs/ai/references/2026-06-18-auv-mc5-onward-execution-plan.md`,
  especially the owner-opened MC-7 offline 3DGS inspect-artifact path.
- Core-lane stabilization work may continue in parallel, but this document now
  treats it as a separate convergence lane rather than the active exploration
  center.

Convergence evidence:

- The archived AX copilot proof lives in
  `docs/archive/verticals/ax-copilot/2026-06-09-auv-macos-ax-copilot-mvp-evidence-pack.md`.
- The active execution boundary for this phase is
  `docs/ai/references/2026-06-18-auv-mc5-onward-execution-plan.md`.
- The owner-opened 3DGS design note lives in
  `docs/ai/references/2026-06-18-minecraft-mc7-offline-3dgs-inspect-artifact-design.md`.

Current seam to preserve:

```text
recognition / AX / candidates
  -> ActionResolver
  -> auv-driver InputActionResult
  -> OperationResult / VerificationResult / trace artifacts
```

`auv-overlay-macos` is visual-only presentation. `auv-driver` /
`auv-driver-macos` own input delivery and disturbance reporting. Do not create
a third action-result schema beside `ActionResolverDecision` and
`InputActionResult`. `candidate_promotion` remains part of AUV core as a
reusable promotion/gating seam even while `candidate-action` is frozen.

## Start Every Task By Classifying It

Use one label before editing:

- `bug fix`: a reproduced defect with a narrow fix and regression test.
- `test-only`: coverage for existing behavior.
- `docs-only`: clarification with no runtime behavior change.
- `narrow refactor`: behavior-preserving cleanup required by the assigned slice.
- `approved feature`: owner named or approved the behavior/module to add.
- `substrate research`: owner-approved exploration of 3D spatial grounding,
  offline 3DGS inspect artifacts, or next-generation verification evidence.

If the task does not fit one label, ask for a smaller slice.

## Scope Defaults

- Work on one focused outcome. Multi-file changes are fine only when the files
  are part of the same approved dependency chain.
- Do not chase TODOs, roadmap notes, or future APIs unless the owner names that
  slice.
- Do not add drive-by renames, helper extraction, comment polish, or unrelated
  compatibility fallbacks while touching code.
- New durable public namespaces, traits, modules, commands, or contract fields
  need owner-approved design. Private helpers are fine when they serve the
  current slice and have narrow tests.
- Cross-layer changes need an explicit dependency direction, for example:
  `contract -> driver artifact -> read-side inspector test`.

## Approval Boundary

Approved:

- The owner named the function, module, behavior, or document to change.
- The owner accepted a concrete proposal.
- The owner asked for one specific next slice.

Not approved:

- A doc or TODO mentions a future feature.
- The change follows naturally from the previous slice.
- The repo scan found a smell.
- The owner said "you decide" without naming a chain of follow-ups. Treat that
  as one focused slice, then stop.

## Completion Behavior

After a slice:

1. State what changed and what was validated.
2. List follow-up candidates as observations, not as already-started work.
3. Stop and wait for the owner to choose.

Do not chain into the next slice without explicit confirmation.

## Validation

For behavior changes, run:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

Also run command-list smoke checks when command catalog, runtime frontends, or
CLI behavior changes. Docs-only edits can skip Cargo validation, but say so in
the summary.
