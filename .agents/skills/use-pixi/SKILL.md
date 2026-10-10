---
name: use-pixi
description: Use when explaining, creating, or changing a Pixi workspace, including manifests, dependencies, platforms, targets, features, environments, tasks, lockfiles, editable Python packages, CI commands, or IDE interpreter setup.
---

# Use Pixi

Use Pixi as the command and environment boundary for this repository.

## Inspect before editing

1. Run `pixi --version`, `pixi info`, and `pixi task list` when available.
2. Locate both `pixi.toml` and `pyproject.toml`. Pixi discovers an explicit `--manifest-path` first, then a local `pixi.toml`, then a local `pyproject.toml`, then parent manifests. A sibling `pixi.toml` therefore wins over `pyproject.toml`.
3. Read the selected manifest, its `workspace.platforms`, targets, features, environments, tasks, and editable path dependencies.
4. Read `pixi.lock` when reproducibility or CI behavior matters. Do not hand-edit the lockfile.

## Choose the dependency owner

- Put Python itself and command-line/system packages supplied by conda-forge under Pixi `dependencies`.
- Put Python distributions resolved from PyPI under `pypi-dependencies` or `[project.dependencies]`, according to the repository's existing ownership boundary.
- Represent a local Python project as `{ path = ".", editable = true }` or another relative editable path in Pixi.
- Use `target.<platform>` for dependencies limited to one supported platform.
- Use a feature when a dependency set is optional and reusable; compose features into named environments.
- Do not use bare `pip install` inside a Pixi-managed repository. Use `pixi add`, a manifest edit, and a new lock instead.

## Understand the model

- A workspace declares channels and every platform solved into `pixi.lock`.
- A target narrows configuration to a platform such as `linux-64`, `win-64`, or `osx-arm64`.
- A feature groups optional dependencies, tasks, activation, and targets.
- An environment selects features and produces an installable prefix under `.pixi/envs/` by default; named or detached configuration may select another prefix.
- A task is a reproducible command executed inside an environment.
- The lockfile records exact conda and PyPI resolutions for every declared platform.

## Make and verify changes

Use the smallest applicable commands:

```bash
pixi add PACKAGE
pixi add --pypi PACKAGE
pixi remove PACKAGE
pixi remove --pypi PACKAGE
pixi update PACKAGE
pixi install
pixi run TASK
pixi shell  # interactive use only
pixi task list
pixi lock --check
```

When adding or removing a dependency, use `--feature FEATURE` for a named feature and `--platform PLATFORM` for a platform-specific table. Combine the selectors only when both scopes apply.

For local, intentional manifest changes, run `pixi lock`, the relevant `pixi run` task, and `pixi lock --check`. Inspect the manifest diff and lockfile diff before reporting success.

Keep the lockfile immutable in CI:

```bash
pixi install --locked
pixi run --locked TASK
```

`--locked` prevents lockfile updates; it does not make environment installation or task execution read-only. Inspect task definitions before running them in any no-write workflow.

CI must not run `pixi lock`, because that command mutates the lockfile.

## IDE boundary

Run `pixi install` before selecting an interpreter. In AUV, `.pixi/envs/default` only carries Ruff and ty; select the named environment that owns a package (under `.pixi/envs/`) as the VS Code `python.defaultInterpreterPath` folder when analyzing or debugging it. Locate named or detached environments using the `prefix` reported by `pixi info --json`, but never commit the returned absolute machine path. Documentation, automation, and terminal commands should use `pixi run`; Windows and POSIX interpreter paths differ.

## Safeguards

- Keep all declared platforms solvable; do not validate only the current host.
- Do not add CUDA or another platform-only dependency to unconditional common dependencies.
- Do not remove or regenerate unrelated environments.
- Do not claim success until the lockfile is current and the requested task passes.

## AUV workspace

- Pixi is required only for Python or model work. Do not ask Rust, JavaScript, Protobuf, or documentation contributors to install it.
- The root `pixi.toml` owns Python tooling; Rust, pnpm, and Buf keep their own toolchains. Never add them to `pixi.toml`.
- Python package metadata owns runtime dependencies. Edit the package `pyproject.toml`, then run `pixi lock`.
- Keep the `default` environment to Ruff and ty with no `python`, solvable on `linux-64`, `osx-arm64`, and `win-64`. Do not add a workspace-wide Python pin.
- Give each Python package its own feature that pins `python` and its dependencies, and compose it into a named environment. Do not share a `solve-group` between packages that need different Python versions.
- For a package on another Python version, add a directory `ruff.toml` that extends the root one with its own `target-version`, and run ty in that package's environment.
- Run `pixi run --locked check` before handoff when Python files or Pixi metadata change.
