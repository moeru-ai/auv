---
inclusion: always
---
# Subagent Concurrency Safety

Qualifies the "ALWAYS use parallel Task execution" advice in the agent
orchestration rule: parallelism is for reading, never for writing.

## Concurrent subagents run on Haiku

- Every parallel / concurrent subagent runs on the Haiku model — the smallest,
  fastest tier (see the performance rule: Haiku for worker agents in
  multi-agent systems). Read-only fan-out is exactly that workload.
- Reserve larger models (Sonnet / Opus) for the single main agent: synthesis,
  judgement calls, and all writes.
- If a probe needs deeper reasoning, run it in the main agent after the Haiku
  probes return — never silently upgrade a concurrent subagent to a larger
  model.

## Read-many, write-one

- Concurrent subagents are READ-ONLY: search, read, analyze, verify, inventory.
- Never delegate mutations to concurrent subagents — no file writes, edits,
  deletes, moves, renames, `git` state changes, or other state-changing
  commands.
- All mutations serialize through the single main agent, applied one at a time
  after verification. Never run two writers against overlapping files or scope
  in parallel.
- Fan out breadth (many read-only probes) freely; fanning out mutation is
  forbidden.

## Findings are unverified until confirmed

- Treat every concurrent subagent report as an UNVERIFIED claim, not fact.
- Before acting on "X is dead / unused / safe to delete / an island", the main
  agent independently confirms (grep + read + build), then acts.
- A low reference count is a lead, not a verdict. Public / serde / wire
  surfaces, same-name-different-concept types, and intentional deferrals
  routinely look "unused" but are not. Report the counting method, not a bare
  number.

## Why

Parallel writers race, produce conflicting edits, and expand scope with no
single owner of the diff. Read-wide / write-narrow keeps the change reviewable
and the convergence discipline intact.
