# Collabi Workflow

This reference exists so humans and agents have one repo-local place to look
before editing AUV with shared-state coordination.

## Read Before Editing

Review:

- `AGENTS.md`
- `CLAUDE.md`
- `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/collaboration-map`
- `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/claims`
- `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/audit?limit=20&order=desc`

Summarize:

1. active slices already in progress
2. whether your intended path overlaps another slice
3. the one narrow slice you intend to take

If there is path overlap or an active conflicting claim, stop before editing.

## Writer Entry Point

Human-facing writer console:

- `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`

Do not describe Collabi as API-only. The writer console is part of the actual
human check-in flow.

## Write Boundary

Default assumption: read-only.

Only perform writer/check-in steps when the human has explicitly granted write
access for the current slice.

Without explicit write access:

- do not fabricate a claim
- do not pretend check-in happened
- do not say the slice is visible in Collabi

Instead, report:

- intended session title
- intended scope and file paths
- evidence that should be attached

## Suggested Check-In Payload

Minimum useful fields for the writer flow:

- repo: `moeru-ai/auv`
- slice classification: `bug fix` / `test-only` / `docs-only` / `narrow refactor` / `approved feature`
- narrow scope summary
- touched paths
- validation plan

## Finish State

When the slice finishes, record:

- what changed
- validation evidence
- final state: `done` or `blocked`
- next recommended slice, without starting it automatically
