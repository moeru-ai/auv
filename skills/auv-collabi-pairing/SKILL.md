---
name: auv-collabi-pairing
description: Use when working in /Users/liuziheng/https-github-com-moeru-ai-auv and coordination must go through Collabi shared state. Before editing, read AGENTS.md, CLAUDE.md, Collabi overview, claims, and audit; stop on overlapping claim or scope/path risk; keep the slice narrow; and, if explicit writer access is granted, use writer.html to check in before editing and add evidence when finishing.
---

# AUV Collabi Pairing

Use this skill only when you are working in:

- `/Users/liuziheng/https-github-com-moeru-ai-auv`
- a paired or multi-agent AUV slice where shared state must go through Collabi

Do not use this skill for general coding or for repos outside the AUV checkout.

## Start-Of-Task Workflow

Before editing:

1. Read the repo rules:
   - `/Users/liuziheng/https-github-com-moeru-ai-auv/AGENTS.md`
   - `/Users/liuziheng/https-github-com-moeru-ai-auv/CLAUDE.md`
2. Read the current shared state:
   - `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/overview`
   - `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/claims`
   - `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/audit?limit=20&order=desc`
   - relevant `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/api/sessions/<id>` details if the overview or audit already points at one
3. Summarize three things before you propose edits:
   - active work already in progress
   - whether your intended repo, scope, or path overlaps another active slice
   - the one narrow slice you recommend taking

If another active claim overlaps your intended repo, scope, or path, stop and report it before editing.

## Execution Rules

- Stay inside one narrow AUV slice.
- Classify the slice before editing: `bug fix`, `test-only`, `docs-only`, `narrow refactor`, or `approved feature`.
- Do not broaden scope because of TODOs, roadmap notes, nearby cleanup, or "obvious next steps".
- Follow AUV architecture and seam rules from `AGENTS.md` and `CLAUDE.md`; do not restate them from memory.
- Treat Collabi as shared handoff state for humans and agents.

## Collabi Write Boundary

Default mode is read-only.

Only write to Collabi when the human explicitly grants write access or provides the writer/check-in flow. If explicit write access was not granted:

- do not invent credentials
- do not call write endpoints
- do not pretend the task was checked in

Instead, report what should be checked in.

If explicit write access was granted, use the existing Collabi flow before editing:

- `writer.html`: `https://collabi-airi-cu-free-01.koreacentral.cloudapp.azure.com/writer.html`
- or the writer flow described in `references/workflow.md`

## Validation Defaults

For behavior changes, run:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

When the slice touches CLI, recipes, or bundles, also run:

- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`
- `cargo run --quiet -- skill bundle list`

## Finish-Of-Task Workflow

When you finish:

1. Summarize what changed.
2. State what was validated.
3. State whether the slice is `done` or `blocked`.
4. List the next recommended step, but do not start it automatically.
5. If write access was granted, add evidence and complete or update the Collabi session state.

If you need concrete commands, writer usage, or check-in examples, read `references/workflow.md`.
