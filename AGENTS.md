# AUV Agent Guide

Concise but detailed reference for contributors working on AUV. Improve code
when you touch it; avoid one-off patterns and keep the shared runtime model in
view.

## Project Mission

- AUV turns application UI workflows into command-like, inspectable,
  replayable, and eventually shortcut-like operations.
- AUV is not only a CLI wrapper and not a generic LLM agent.
- Design around reusable runtime APIs, first-party drivers, implicit run
  recording, artifact capture, replay, and inspection.
- Keep CLI, MCP, library calls, and future UI surfaces on the same execution
  model.
- Prefer explicit boundaries between runtime, drivers, recipes, command
  frontends, run storage, and reference documentation.
- Use `docs/TERMS_AND_CONCEPTS.md` as the shared vocabulary for run recording,
  inspection, trace data, artifacts, and viewer-facing APIs.
- When a design introduces or changes a core term, update
  `docs/TERMS_AND_CONCEPTS.md` instead of defining the term only inside a
  transient spec.

> Many project details are still undecided. During design and implementation,
> communicate with users frequently and clearly to avoid misunderstandings,
> premature naming decisions, and avoidable rework.

## Project Phase: Converge The Core Contract

AUV is currently converging its core contract. The important work is to make
observation, action, verification, trace data, artifacts, and inspection agree
on the same model. Prefer changes that reduce ambiguity in that model. Avoid
changes that merely make a local file look cleaner.

Good convergence work usually has one of these shapes:

- Defines or tightens a shared contract in `docs/TERMS_AND_CONCEPTS.md`,
  `src/contract.rs`, run records, artifacts, or command signals.
- Connects an existing producer to an existing consumer with typed evidence,
  for example `RecognitionResult -> CandidateRef -> action -> VerificationResult`.
- Aligns action-selection metadata with typed driver results, for example
  `ActionResolverDecision -> InputActionResult -> trace/artifact signals`.
- Fixes a reproduced bug in a narrow path and adds a regression test.
- Turns a known boundary into explicit metadata, failure layers, fallback
  reasons, or validation errors.
- Finishes an owner-approved slice without expanding it into adjacent roadmap
  work.

Poor convergence work looks like broad cleanup, TODO chasing, opportunistic
helper extraction, speculative backends, or implementing future APIs just
because a doc mentions them. If the change adds new surface area, it needs an
explicit reason tied to the current contract.

## Scope Discipline

Before editing, classify the change as one of: bug fix, test-only, docs-only,
narrow refactor, or approved feature. If none fits, ask for a smaller slice.

Some missing pieces are intentional deferrals, some are incomplete work, and
some are real bugs. Do not guess which one you are looking at. Classify the
gap from evidence, failing tests, owner instructions, or existing reference
docs before implementing anything.

Scope rules:

- Do not implement TODOs, roadmap notes, or future-phase designs unless the
  owner names that slice.
- Do not run broad repository scans and turn the findings into drive-by
  changes. Search only to understand the assigned slice.
- Do not mix unrelated cleanup into behavior changes. Mention cleanup
  opportunities as follow-up candidates instead.
- Cross-layer changes are allowed when they are the approved contract slice,
  but the dependency direction must be clear. Example: contract type -> driver
  artifact -> read-side inspector test.
- Avoid ad-hoc compatibility shims. Versioned read compatibility for existing
  run artifacts, recipes, or public records is allowed when the migration
  boundary and tests are explicit.

Owner approval means the owner named the function/module/behavior, accepted a
concrete proposal, or asked for a specific next slice. A doc mentioning a
future feature, a TODO marker, or a change that feels like "the obvious next
thing" is not approval.

When a good idea appears mid-task, do not implement it inline. Record it as a
candidate next slice with one sentence explaining why it matters, then finish
the current slice.

When you decide *not* to implement something that a reader could plausibly
expect — a field, an enum variant, a branch, an algorithm stage, an API
surface, a fallback path, or any other surface that has been considered and
intentionally left out for the current slice — leave a `TODO:` or `NOTICE:`
marker at the type or call site where the deferral lives. The goal is to
make intent visible: a future reader (Codex, Claude, the owner, a reviewer)
must be able to tell *not yet, on purpose* apart from *forgot to write it*.
A missing marker forces the next reader to guess and frequently produces
unwanted scope expansion when an agent treats the absence as a gap.

A useful deferral marker names three things in one short comment: the gap
(what was considered but omitted), the reason it is deferred (why it does
not belong in this slice), and the trigger that would unlock it (what
condition or owner approval would re-open the decision). When the deferral
is already covered by a reference doc, the inline marker may simply cite
that doc instead of restating the rationale — for example,
`// TODO(view-memory-v1): persistence deferred, see view-parser-view-memory-v0.md`.
The rule is that *something at the code site* must point a reader to the
intentional gap.

Apply this discipline to:

- Enum variants you considered but chose not to add.
- Struct fields you considered but did not add.
- Algorithm stages, fallback paths, or recovery branches you reserved but
  did not implement.
- API surfaces you chose to keep private, omit, or stub for the current
  slice.
- Reader-side checks that mirror a producer-side guarantee (such as
  version rejection on a wire-shape field) when only the producer side
  has landed.

A deferral marker is not approval to implement the deferred work later
without owner involvement. It is documentation of an existing decision so
the decision survives the next read of the code.

## Current Contract Seam

The active macOS automation seam is:

```text
recognition / AX / candidates
  -> ActionResolver
  -> auv-driver InputActionResult
  -> OperationResult / VerificationResult / trace artifacts
```

Keep visual presentation separate from input delivery:

- `auv-overlay-macos` is a visual trust/debug layer. It may show an AUV cursor,
  a user cursor, movement, or click ripples, but it does not prove semantic
  success and must not be treated as the input backend.
- `auv-driver` / `auv-driver-macos` owns typed input delivery such as
  `WindowTargetedMouse`, `WindowTargetedKeyboard`, foreground fallback,
  disturbance metadata, attempts, and fallback reasons.
- `ActionResolver` should choose and explain the method, then consume or map
  typed driver results. Do not grow a parallel action-result schema unless a
  concrete gap is first documented.
- Verification remains separate from activation. A successful click, AX press,
  or overlay animation is not semantic success without a verification result or
  an explicit `activation_only` boundary.

## Architecture Surfaces

- **Runtime**: Owns execution semantics, implicit run recording, artifact
  persistence, and the common model used by all frontends.
- **Drivers**: Expose platform or application capabilities through narrow,
  capability-oriented APIs.
- **Recipes**: Describe reusable workflows and case matrices without becoming
  the only execution model.
- **Command frontends**: Parse and present user-facing commands; they should
  call shared runtime APIs rather than owning core behavior.
- **Run storage**: Owns durable records, artifacts, trace data, and replayable
  inputs.
- **Inspection/viewer APIs**: Read durable run data and artifacts; do not
  depend on transient CLI-only behavior.
- **Reference docs**: Record accepted terminology, design decisions, evidence,
  and implementation handoffs that should survive beyond a task thread.

## Key Path Index

- `src/runtime.rs`: implicit run execution and artifact persistence.
- `src/catalog.rs`: command catalog and default command definitions.
- `src/skill.rs`: recipe and case-matrix loading, validation, and execution.
- `src/bundle.rs`: bundle export, bundle verification, and package
  verification.
- `src/driver/macos/`: macOS driver implementation, dispatch, support, and
  tests.
- `recipes/`: executable recipe manifests and case matrices.
- `bundles/`: bundle manifests.
- `docs/TERMS_AND_CONCEPTS.md`: shared vocabulary for core AUV concepts.
- `docs/ai/references/`: durable reference notes, coverage reports, evidence
  packs, and implementation handoffs.
- `docs/ai/explanations/`: committed tutorials, explainers, walkthroughs, and
  diagrams.
- `docs/notes/<owner>/`: personal or exploratory scratch material that should
  not be committed unless explicitly requested.

## Commands

- **Build**
  - `cargo build`: compile the workspace in debug mode.
  - `cargo run`: build and run the current CLI binary locally.
- **Test**
  - `cargo test`: run Rust unit and integration tests.
- **Format and lint**
  - `cargo fmt`: format Rust code using `rustfmt`.
  - `cargo fmt --check`: verify formatting without modifying files.
  - `cargo clippy --all-targets --all-features`: run lint checks across the
    workspace.
- **Development shell**
  - `nix develop`: enter the repository development shell when using the
    provided flake.

## Development Practices

- Do not treat CLI as the only architecture surface.
- Design changes should account for library/runtime calls, CLI frontends, run
  recording, replay, and inspection.
- Favor clear module boundaries; shared behavior belongs behind runtime,
  driver, storage, recipe, or inspection boundaries rather than local one-off
  helpers.
- Keep runtime entrypoints lean; move reusable policy, validation, recording,
  replay, and persistence behavior into the modules that own those decisions.
- Before planning or writing new utilities, commands, driver helpers, recipe
  helpers, or artifact builders, search for existing internal implementations
  first.
- If the logic could become shared project infrastructure, propose the shared
  boundary instead of adding a one-off local copy.
- When modifying code, check for small, minimal progressive refactors that make
  the touched area clearer.
- Keep changes scoped to the task; do not start broad cleanup or compatibility
  work unless the current change depends on it.
- Do not add backward-compatibility guards by default. If extended support is
  needed, document the compatibility boundary, why it matters, and the expected
  post-refactor shape before adding extra branches.

## Rust Style

- Rust code uses the 2024 edition.
- The repository `rustfmt.toml` sets two-space indentation
  (`tab_spaces = 2`).
- Prefer idiomatic Rust naming:
  - `snake_case` for functions, modules, and variables.
  - `PascalCase` for types and traits.
  - `SCREAMING_SNAKE_CASE` for constants.
- Keep public APIs small and documented.
- For design terms that are still under discussion, mark them as provisional in
  docs rather than stabilizing them through code names too early.
- Avoid hardcoded Unix, macOS, or Windows path literals unless the path is
  truly platform-defined.
- Prefer `Path`, `PathBuf`, component joins, and path-safe arguments over string
  concatenation.
- Do not move every literal into a constant. One-time or two-time values should
  remain near usage when locality is clearer.
- For retry, timeout, backoff, geometry tolerance, and limit values, avoid one
  broad constant that silently governs unrelated behavior.

## Naming & Comments

- Prefer names that rely on the module boundary for context instead of
  repeating product, platform, protocol, or transport prefixes inside every
  symbol.
- A well-named module should let exported functions use short action-first
  names; repeat the larger context only when the symbol crosses a boundary where
  that context is no longer obvious.
- Name functions after the domain operation they perform, not after the
  implementation layer that happens to contain them.
- Use nouns for resolved domain concepts and verbs for transformations or side
  effects.
- When a function derives a policy, configuration, selector, or trace decision
  from an event or request, name the domain result explicitly so callers
  understand what decision is being made.
- Avoid names that encode multiple layers of ownership into one symbol.
- If a name needs several qualifiers to be understandable, reconsider the module
  boundary or introduce a clearer local concept.
- Use dependency injection only at real external boundaries: filesystem,
  process, clock, environment, OS APIs, accessibility services, window servers,
  model runtimes, network, cache, and feature gates.
- Do not introduce `Dependencies` or `Deps` structs for internal functions that
  only call sibling helpers or forward parameters.
- Add clear, concise comments for utilities, parsing, OS interaction,
  accessibility traversal, geometry/math, FFI, scheduling, artifact persistence,
  and architectural functions when the intent, invariant, or platform
  constraint is not obvious from names and local context.
- Avoid comments that repeat what the code already says.
- When moving, refactoring, fixing, or updating code, keep existing comments
  intact and move them with the code.
- If a comment is truly unnecessary, replace it with a clearer comment or remove
  it only when the surrounding code fully preserves the original context.
- Use markers:
  - `TODO:` follow-up work that is intentionally left for later.
  - `REVIEW:` concerns that need another maintainer or design pass.
  - `NOTICE:` magic numbers, workarounds, platform quirks, generated-code
    constraints, or important external context.
- When using a workaround, add a `NOTICE:` comment explaining why it is needed,
  the root cause, and the removal condition.
- If a workaround was validated through SwiftPM, SourceKit, generated bridge
  output, platform documentation, or dependency source, include the relevant
  file, command, issue, or URL in code-formatted text.

## Testing Practices

- Focused unit tests live next to the code they cover with `#[cfg(test)]`.
- The current repo already has Rust unit coverage for catalog, skill, bundle,
  runtime, driver, and CLI behavior.
- Add regression tests for behavior changes and keep them narrow.
- Use `cargo test` for the full suite and include regression tests for bug
  fixes.
- For any investigated bug or issue, try to reproduce it first with a test-only
  reproduction before changing production code.
- Prefer a unit test. If that is not possible, use the smallest higher-level
  automated test or recorded fixture that can still reproduce the behavior.
- When a regression test maps to an external report, include the tracker
  identifier in the test name when practical.
- Add the report link as a comment directly above the regression test.
- For local investigations without a tracker, leave a short root-cause comment
  when it would help future maintainers understand why the test exists.
- Test through stable public behavior.
- Do not create new exports, dependency bags, wrapper services, or alternate
  code paths only to make private implementation details mockable.
- Avoid smoke-only tests for behavior changes.
- Assert the observable output, trace record, artifact, replay input, command
  result, or platform-facing call that would have caught the original failure.
- Prefer explicit assertions over overly broad table-driven tests.
- Use tables when the cases share the same behavior and the table improves
  readability; keep one-off or highly distinct scenarios as separate tests with
  clear names.

Use this root-cause block format in regression tests when relevant:

```rust
// ROOT CAUSE:
//
// If <condition>, <unexpected behavior> happened because <reason>.
//
// Before the fix, <old behavior>.
// The fix keeps <new invariant or behavior>.
```

## Module Design

- Prefer deep modules over shallow modules.
- A module should hide a meaningful decision: policy, persistence boundary,
  protocol/schema contract, scheduling semantics, replay semantics, artifact
  layout, driver capability, platform permission rule, or lifecycle concern.
- Do not split code by execution order alone.
- A module boundary should represent a stable responsibility that can be
  understood without reading all sibling files.
- Keep cohesive domain flows together until there is proven pressure to split.
- A 200-400 line cohesive Rust module is preferable to several shallow modules
  that pass the same context or options through each other.
- Before creating a new service, trait, context object, or dependency struct,
  verify that it adds policy, validation, state, retry/error handling, IO
  isolation, a reusable abstraction, or a platform boundary.
- If a new abstraction does not add those things, keep the logic as a private
  helper or inline it.
- Before defining a new reusable primitive — a struct, enum, type alias,
  constant, or shared helper — search the workspace (owning/domain crate and
  dependencies) for one that already carries the concept. Search by name
  (rg/grep) and by shape (ast-grep, else rg/grep), not exact name alone.
- Reuse an existing primitive when it fits; import a dependency's instead of
  copying it. A primitive that re-expresses another's shape and meaning is a
  duplicate — delete it and reuse the original.
- If one almost fits (missing a derive, method, or variant), extend it in its
  owning crate; do not fork a local variant.
- Add a new primitive only for a genuinely distinct concept, never an ad-hoc
  copy. Same-shape primitives are allowed only when meanings differ on purpose
  (ScreenPoint vs WindowPoint) and the name shows it; place a reusable primitive
  in its domain crate, and keep one private only when local and not a
  shared-concept duplicate.
- Avoid pass-through services and traits that only forward arguments to another
  layer with a similar signature.
- Adjacent layers should expose different abstractions:
  - CLI frontends parse and present.
  - Runtime APIs execute and record.
  - Drivers expose capabilities.
  - Storage owns persistence semantics.
  - Inspectors read durable run data.
- Do not extract tiny one-call helper functions just to name an implementation
  step, reduce line count, reduce nesting, or create a test seam.
- Extract a helper only when it is reused by multiple production call sites,
  hides non-trivial branching, IO, parsing, normalization, error policy, or
  lifecycle logic, or names a stable domain concept.
- Keep reusable domain contracts and rendering/building logic in the crate or
  module that owns that domain.
- Runtime entrypoints should wire dependencies and call those boundaries instead
  of inlining large reusable contracts.
- Prefer early returns and simple control flow when it improves readability.
- Do not introduce pass-through helpers or shallow modules solely to reduce
  indentation.

## Platform-Native Interop

- Prefer capability-oriented module names for platform-native interop layers,
  such as `screen`, `window`, `ax_tree`, `keyboard`, `clipboard`, `app`, and
  `permission`.
- Keep FFI and generated binding details behind narrow modules named `native`,
  `binding`, or `ffi`.
- Do not make dependency names such as `swift_bridge` or broad terms such as
  `bridge` into durable public namespaces.

### macOS Swift Packages

- For macOS Swift native code, prefer `swift-bridge` for typed Rust/Swift
  interop unless a design explicitly chooses a different boundary.
- When adding a new Swift package or restructuring Swift sources, use Swift
  tooling such as `swift package init` to create the package skeleton instead
  of hand-writing the project structure from scratch.
- Adapt the generated manifest and sources to the repository layout after
  SwiftPM creates the skeleton.
- The macOS Swift packages use generated `swift-bridge` types that are produced
  by Cargo during the real build.
- For SourceKit or IDE indexing, run `hack/generate-swift-bridge` to generate
  ignored files under:
  - `crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Generated/`
  - `crates/auv-overlay-macos/native/swift/Sources/AuvMacosOverlayNative/Generated/`
- After generating those files, use SwiftPM-side checks so generated bridge
  types are visible to the IDE:
  - `cd crates/auv-driver-macos/native/swift && swift build`
  - `cd crates/auv-overlay-macos/native/swift && swift build`
- Do not commit generated directories.
- Regenerate bridge files after changing:
  - `crates/auv-driver-macos/src/native/binding.rs`
  - `crates/auv-overlay-macos/src/native/binding.rs`
- When a change touches Rust/Swift FFI declarations or Swift native source
  files, rerun `hack/generate-swift-bridge` before validating.
- Run the SwiftPM build for every touched Swift package.
- Treat SwiftPM and SourceKit errors as real compatibility issues even when
  `cargo check` succeeds.
- Keep availability guards, generated bridge types, and IDE-visible imports
  current.

## Documentation Workflow

- During active design or implementation, write specs, plans, and working notes
  in the locations required by the relevant skill or workflow.
- In-progress documents are the source of truth while the work is underway.
- When an implementation is mostly complete, update durable reference material
  in `docs/ai/references/`.
- Add, merge, or revise reference docs so completed and partially completed work
  is discoverable outside the original plan or spec.
- When editing design docs, preserve open questions and clearly label
  provisional names so the team can review them before implementation.

### Documentation Placement

- Use `docs/ai/references/` only for durable project reference material:
  accepted design notes, implementation handoffs, evidence packs, coverage
  reports, and records that should be useful to reviewers after the original
  task context is gone.
- Content in `docs/ai/references/` should describe the current project state or
  a clearly labeled historical decision.
- Use `docs/ai/explanations/` for committed explanatory material: tutorials,
  interactive explainers, conceptual walkthroughs, diagrams, and documents
  whose purpose is to teach or clarify an idea rather than record project
  evidence.
- Prefer English for committed explanations unless the user explicitly asks for
  another language.
- For explanatory HTML files, name the file with the relevant module or feature
  name in kebab case as the prefix, followed by a short description, for example
  `scroll-scan-visual-row-band.html`.
- Use `docs/notes/<owner>/` for personal, exploratory, or scratch material,
  including rough HTML demos, temporary investigation notes, local run logs, and
  drafts that are not ready to be shared as durable project references.
- Do not commit notes from `docs/notes/<owner>/` unless the user explicitly
  asks for that exact material to be committed.
- If a note becomes generally useful, move or rewrite it into
  `docs/ai/explanations/` or `docs/ai/references/` as appropriate before
  committing.

## PR / Workflow Tips

- Run formatting and tests before submitting changes that touch Rust code.
- Use concise Conventional Commit-style subjects, such as `chore: init` and
  `chore(README.md): added`.
- Prefer `type(scope): summary` when a scope is useful.
- When a commit primarily changes one crate, use the exact crate name as the
  Conventional Commit scope, for example `feat(auv-netease-music): ...`.
- Pull requests should include:
  - A short description.
  - Relevant design or issue links.
  - Verification commands run.
  - Screenshots or trace artifacts when changing UI inspection, automation, or
    documentation visuals.
- Summarize changes, how they were tested, and follow-ups.
- Keep changes scoped.
- Improve legacy code you touch when the improvement is local and directly
  supports the task.
- Avoid one-off patterns.

## Validation Commands

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`
- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`
- `cargo run --quiet -- skill bundle list`
