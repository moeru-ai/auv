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

> Many project details are still undecided. During design and implementation,
> communicate with users frequently and clearly to avoid misunderstandings,
> premature naming decisions, and avoidable rework.

## Project Structure & Module Organization

To be implemented. The repository layout is still provisional and should be
updated once core crates, runtime modules, driver modules, tests, and durable
documentation locations are decided.

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

There is no dedicated test suite yet. Add focused unit tests near the code they
cover with `#[cfg(test)]`, and add integration tests under `tests/` once CLI or
runtime behavior becomes stable. Name tests after the behavior being verified,
for example `creates_run_for_each_invoke`.

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

## Documentation Workflow

During active design or implementation, write specs, plans, and working notes in
the locations required by the relevant skill or workflow. These in-progress
documents are the source of truth while the work is underway.

When an implementation is mostly complete, update durable reference material in
`docs/ai/references/`. Add, merge, or revise reference docs so completed and
partially completed work is discoverable outside the original plan or spec.
