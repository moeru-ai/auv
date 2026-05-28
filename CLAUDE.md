# CLAUDE.md

Claude-specific operating guidance for AUV. `AGENTS.md` is authoritative;
this file is a short operating card for staying inside the current project
phase.

## Current Mode

AUV is converging its core contract. Optimize for making observation, action,
verification, artifacts, trace records, and inspection agree. Do not optimize
for local prettiness, broad cleanup, or filling every missing piece.

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
`InputActionResult`.

## Start Every Task By Classifying It

Use one label before editing:

- `bug fix`: a reproduced defect with a narrow fix and regression test.
- `test-only`: coverage for existing behavior.
- `docs-only`: clarification with no runtime behavior change.
- `narrow refactor`: behavior-preserving cleanup required by the assigned slice.
- `approved feature`: owner named or approved the behavior/module to add.

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

Also run command-list smoke checks when command catalog, recipes, bundles, or
CLI behavior changes. Docs-only edits can skip Cargo validation, but say so in
the summary.
