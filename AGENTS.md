# Repository Guidelines

## Project Mission & Design Principles

AUV turns application UI workflows into command-like, inspectable, replayable,
and eventually shortcut-like operations. It is not only a CLI wrapper and not a
generic LLM agent. Design the project around reusable runtime APIs,
first-party drivers, implicit run recording, artifact capture, replay, and
inspection.

Prefer explicit boundaries between runtime, drivers, recipes, command
frontends, run storage, and reference documentation. Keep CLI, MCP, library
calls, and future UI surfaces on the same execution model.

Use `docs/TERMS_AND_CONCEPTS.md` as the shared vocabulary for run recording,
inspection, trace data, artifacts, and viewer-facing APIs. When a design
introduces or changes a core term, update that document instead of defining the
term only inside a transient spec.

> Many project details are still undecided. During design and implementation,
> communicate with users frequently and clearly to avoid misunderstandings,
> premature naming decisions, and avoidable rework.

## Project Structure & Module Organization

Current repository structure:

- `src/runtime.rs`: implicit run execution and artifact persistence
- `src/catalog.rs`: command catalog and default command definitions
- `src/skill.rs`: recipe and case-matrix loading, validation, and execution
- `src/bundle.rs`: bundle export, bundle verification, and package verification
- `src/driver/macos/`: macOS driver implementation, dispatch, support, and tests
- `recipes/`: executable recipe manifests and case matrices
- `bundles/`: bundle manifests
- `docs/ai/references/`: durable reference notes, coverage reports, and evidence packs

The CLI is a frontend over the shared runtime, not the only architecture
surface.

## Build, Test, and Development Commands

- `cargo build`: compile the workspace in debug mode.
- `cargo run`: build and run the current CLI binary locally.
- `cargo test`: run Rust unit and integration tests.
- `cargo fmt`: format Rust code using `rustfmt`.
- `cargo clippy --all-targets --all-features`: run Rust lint checks across the
  workspace.
- `nix develop`: enter the repository development shell when using the provided
  flake.

Run formatting and tests before submitting changes that touch Rust code.

## Coding Style & Naming Conventions

Rust code uses the 2024 edition and the repository `rustfmt.toml` sets
two-space indentation (`tab_spaces = 2`). Prefer idiomatic Rust naming:
`snake_case` for functions, modules, and variables; `PascalCase` for types and
traits; `SCREAMING_SNAKE_CASE` for constants.

Keep public APIs small and documented. For design terms that are still under
discussion, mark them as provisional in docs rather than stabilizing them
through code names too early.

## Testing Guidelines

Focused unit tests live next to the code they cover with `#[cfg(test)]`.
The current repo already has Rust unit coverage for catalog, skill, bundle,
runtime, driver, and CLI behavior. Add regression tests for behavior changes
and keep them narrow.

Use `cargo test` for the full suite and include regression tests for bug fixes.

## Commit & Pull Request Guidelines

Existing commits use short Conventional Commit-style subjects such as
`chore: init` and `chore(README.md): added`. Continue using concise subjects in
the form `type(scope): summary` when a scope is useful.

Pull requests should include a short description, relevant design or issue
links, and verification commands run. Include screenshots or trace artifacts
when changing UI inspection, automation, or documentation visuals.

## Agent-Specific Instructions

Do not treat CLI as the only architecture surface. Design changes should account
for library/runtime calls, CLI frontends, run recording, replay, and inspection.
When editing design docs, preserve open questions and clearly label provisional
names so the team can review them before implementation.

For platform-native interop layers, prefer capability-oriented module names
such as `screen`, `window`, `ax_tree`, `keyboard`, `clipboard`, `app`, and
`permission`. Keep FFI and generated binding details behind narrow modules named
`native`, `binding`, or `ffi`; do not make dependency names such as
`swift_bridge` or broad terms such as `bridge` into durable public namespaces.

For macOS Swift native code, prefer `swift-bridge` for typed Rust/Swift
interop unless a design explicitly chooses a different boundary. When adding a
new Swift package or restructuring Swift sources, use Swift tooling such as
`swift package init` to create the package skeleton instead of hand-writing the
project structure from scratch; then adapt the generated manifest and sources
to the repository layout.

The macOS Swift packages use generated `swift-bridge` types that are produced by
Cargo during the real build. For SourceKit or IDE indexing, run
`hack/generate-swift-bridge` to generate ignored files under
`crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Generated/` and
`crates/auv-overlay-macos/native/swift/Sources/AuvMacosOverlayNative/Generated/`.
After generating those files, `cd crates/auv-driver-macos/native/swift && swift build`
and `cd crates/auv-overlay-macos/native/swift && swift build` are the preferred
SwiftPM-side checks that the packages and generated bridge types are visible to the
IDE. Do not commit the generated directories; regenerate them after changing
`crates/auv-driver-macos/src/native/binding.rs` or
`crates/auv-overlay-macos/src/native/binding.rs`.

## Documentation Workflow

During active design or implementation, write specs, plans, and working notes in
the locations required by the relevant skill or workflow. These in-progress
documents are the source of truth while the work is underway.

When an implementation is mostly complete, update durable reference material in
`docs/ai/references/`. Add, merge, or revise reference docs so completed and
partially completed work is discoverable outside the original plan or spec.

### Documentation Placement

Use `docs/ai/references/` only for durable project reference material: accepted
design notes, implementation handoffs, evidence packs, coverage reports, and
records that should be useful to reviewers after the original task context is
gone. Content in this directory should describe the current project state or a
clearly labeled historical decision.

Use `docs/ai/explanations/` for committed explanatory material: tutorials,
interactive explainers, conceptual walkthroughs, diagrams, and other documents
whose purpose is to teach or clarify an idea rather than record project
evidence. Prefer English for committed explanations unless the user explicitly
asks for another language. For explanatory HTML files, name the file with the
relevant module or feature name in kebab case as the prefix, followed by a
short description, for example `scroll-scan-visual-row-band.html`.

Use `docs/notes/<owner>/` for personal, exploratory, or scratch material,
including rough HTML demos, temporary investigation notes, local run logs, and
drafts that are not ready to be shared as durable project references. Do not
commit notes from `docs/notes/<owner>/` unless the user explicitly asks for
that exact material to be committed. If a note becomes generally useful, move
or rewrite it into `docs/ai/explanations/` or `docs/ai/references/` as
appropriate before committing.

## Validation Commands

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`
- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`
- `cargo run --quiet -- skill bundle list`
