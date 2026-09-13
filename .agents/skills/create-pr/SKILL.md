---
name: create-pr
description: Prepare or update AUV pull requests with source-backed API change disclosures, before-and-after usage tables, and migration notes. Also use for retrospective API audits of specified PRs or revisions.
---

# Create PR

Make the PR useful to both reviewers and downstream callers. Explain the final
behavior, show exact calling changes, and preserve enough source context to audit
those claims later. Follow the repository's AGENTS.md and PR template.

## Establish the comparison

- Identify the repository, PR or branch, base, and head. Record resolved commit
  SHAs in the disclosure. Use the actual PR diff, not every change since `main`.
- For an open PR, compare its merge base with its head. For a merged or historical
  change, identify the reviewed or landed revision range and state which one you
  used. Do not infer historical behavior from today's working tree. A commit in
  Git is not proof of merge or release; use “head revision” unless that status
  was verified.
- For stacked PRs, distinguish changes introduced here from changes inherited
  from the base. State dependencies and merge order. Recheck the base after an
  upstream merge; do not claim independence merely because a PR has its own URL.
- Start with changed files, then inspect definitions, exports, and real callers
  at both revisions. Use `git show`, `git diff`, and targeted `rg` searches.
  PR prose and commit messages are context, not proof of an API contract.

## Inventory caller-visible changes

Trace each changed contract across the layers that actually expose it: shared
Rust types, driver capabilities, `auv` APIs, CLI arguments, Runner/Proto, and SDKs.
Do not scan unrelated modules or invent obligations for untouched layers.

Classify findings as added, removed, changed, or internal-only. Include:

- Exported functions, methods, types, enum variants, fields, and re-exports.
- Parameter order and types, required arguments, defaults, builder options,
  return values, error categories, and serialization shapes.
- Accepted aliases and values, platform guards, identity or authorization
  requirements, and resource reuse when existing calls behave differently.

Check these distinctions before describing a migration:

- A new path or re-export is not a removal if the old path still exists.
- A `pub(crate)` helper rename is not a public API break.
- A new public type does not mean execution methods accept that type. Read their
  parameter types; show actual accepted inputs, not an intended future API.
- Source compatibility, wire compatibility, and runtime behavior are different.
  A defaulted wire field can still break Rust struct literals. An older peer can
  ignore a new field without implementing the requested behavior.
- The ability to parse or compile an input does not prove native delivery.
- An unchanged signature can require migration when defaults, return semantics,
  lifecycle, or supported platforms change.

Report removals explicitly, including when none were found in the inspected
public surfaces. Mention unchanged APIs only when readers could reasonably
mistake them for removed or changed APIs. Do not list every untouched method.

## Write the disclosure

Lead with the concrete problem and resulting behavior. Use an English
Conventional Commit title with the relevant scope. Use `chore(docs): ...` for
primarily documentation changes, as required by this repository. Keep committed
PR text in clear, simple English unless the user requests another language.

For API changes, include a Markdown table with side-by-side inline code:

| Surface / change | Before | After | Caller action |
|---|---|---|---|
| Rust method — changed | Exact old call | Exact new call | Required migration |
| Public type — added | Not available | Exact construction or parsing | Optional use or new requirement |
| Entry point — removed | Exact old call | Supported replacement, or none | Required migration |

Replace the illustrative rows with verified findings; omit categories with no
rows. Put short code in backticks inside cells. Escape pipe characters as needed
for Markdown tables. Put longer compilable examples in fenced blocks below the
table and refer to them from the relevant row. Do not put fenced blocks inside
table cells. Define receiver types or setup when a snippet would otherwise be
ambiguous. Label partial snippets and placeholders; do not imply they were run.

Then add only the migration notes that matter:

- List added APIs with usable examples, and removed APIs with replacements.
  The comparison table can serve as this inventory; do not duplicate it.
- State required edits versus optional adoption. Preserve platform-specific
  examples, setup prerequisites, and meaningful before/after behavior differences.
- Name client/Runner version requirements when supported by the code. Do not
  invent released versions or compatibility negotiation.
- Link each group of claims to commit-pinned source definitions, callers, tests,
  or durable contract notes. Include both revisions when explaining a removal or
  signature change. Local revision/path references are acceptable for offline
  drafts; replace them with permalinks when publishing to GitHub.
- Report validation commands and outcomes at the tested revision. Distinguish
  compilation, automated tests, live receiver evidence, and pending CI. Do not
  claim examples were executed when only their signatures were inspected.

For changes with no caller-visible impact, use a short explicit statement and
relevant validation instead of an empty table. For a retrospective audit, produce
the same disclosure with the historical comparison range; do not create a new PR
or rewrite old PRs unless requested. Store a requested durable audit under the
owning `docs/ai/references/<responsibility>/` directory.

## Deliver within the requested scope

Before publishing, compare the final diff with the table and migration notes.
Update obsolete rows when scope changes; do not append contradictory follow-ups.
Use a structured body argument or `gh ... --body-file` to preserve code and
newlines. Read the published body back to verify it reflects the intended text.

Creating or updating a PR must follow the user's existing authorization. A draft
or audit request alone does not authorize publishing, merging, or changing other
PRs. When publication is already requested, complete the preparation and publish
without an extra confirmation. Keep unrelated working-tree changes out of the PR.
The skill does not require raw logs, screenshots, or temporary probe scripts to
be committed; concise validation results and source links are sufficient unless
the task specifically requires those artifacts.
