# Remove Bundle Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the active SkillBundle execution/export/verification surface while leaving recipe execution for the next PR.

**Architecture:** This is PR1 of runtime legacy retirement. The CLI stops exposing `skill bundle ...`, `Runtime` stops resolving bundle-backed commands, root startup stops discovering `bundles/`, and `src/bundle/**` plus checked-in `bundles/` are deleted. Recipe catalog and `skill run` remain untouched for PR2.

**Tech Stack:** Rust 2024, current root `auv-cli` crate, existing hand-written CLI parser in `src/cli.rs`, `cargo test`, `cargo check`, `git diff --check`.

---

## File Structure

Delete:

- `bundles/native-app-skill-tree.v0.json`
- `bundles/game-slay-the-spire.v0.json`
- `src/bundle/catalog.rs`
- `src/bundle/export.rs`
- `src/bundle/model.rs`
- `src/bundle/mod.rs`
- `src/bundle/paths.rs`
- `src/bundle/render.rs`
- `src/bundle/tests.rs`
- `src/bundle/validate.rs`

Modify:

- `src/cli.rs`: remove `SkillBundle*` command variants, help text, parser, and tests for bundle CLI.
- `src/main.rs`: remove bundle catalog startup and match arms.
- `src/lib.rs`: remove `pub mod bundle`, `SkillBundleCatalog` import, and runtime construction argument.
- `src/runtime.rs`: remove bundle catalog storage, bundle command lookup, bundle command execution, bundle helper types, and bundle-specific tests.

Do not modify in this PR:

- `src/skill/**`
- `recipes/**`
- `src/catalog.rs`
- `src/driver/**`
- `src/recorded_operation.rs`
- `crates/auv-*` app command crates

## Task 0: Prepare The Branch

**Files:**

- No file changes.

- [ ] **Step 1: Start from a clean updated `main`**

Run:

```bash
git switch main
git pull --ff-only
git status --short
```

Expected:

- `git status --short` prints no tracked or untracked changes.

- [ ] **Step 2: Create the implementation branch**

Run:

```bash
git switch -c refactor/remove-bundle-execution
```

Expected:

- The current branch is `refactor/remove-bundle-execution`.

## Task 1: Remove Bundle CLI Parsing And Help

**Files:**

- Modify: `src/cli.rs`

- [ ] **Step 1: Write failing parser tests for removed bundle commands**

Add these tests in the existing `#[cfg(test)] mod tests` in `src/cli.rs`:

```rust
#[test]
fn parse_skill_bundle_commands_are_removed() {
  let error = parse_cli(&[
    "skill".to_string(),
    "bundle".to_string(),
    "list".to_string(),
  ])
  .expect_err("skill bundle should be removed");

  assert!(
    error.contains("skill bundle has been removed"),
    "unexpected error: {error}"
  );
}

#[test]
fn help_text_no_longer_lists_skill_bundle_commands() {
  let help = help_text();

  assert!(!help.contains("skill bundle"));
  assert!(help.contains("auv-cli skill run"));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test --lib skill_bundle_commands
```

Expected:

- `parse_skill_bundle_commands_are_removed` fails because `parse_cli` still returns `CliCommand::SkillBundleList`.
- `help_text_no_longer_lists_skill_bundle_commands` fails because help still contains `skill bundle`.

- [ ] **Step 3: Remove bundle command variants**

In `src/cli.rs`, delete these `CliCommand` variants:

```rust
  SkillBundleList,
  SkillBundleShow {
    query: String,
  },
  SkillBundleCoverage {
    query: String,
  },
  SkillBundleVerify {
    query: String,
  },
  SkillBundleExport {
    query: String,
    output_dir: String,
  },
  SkillBundlePackageVerify {
    package_dir: String,
  },
```

- [ ] **Step 4: Remove bundle help lines and notes**

In `help_text()`, delete these usage lines:

```text
  auv-cli skill bundle list
  auv-cli skill bundle show <bundle-id-or-path>
  auv-cli skill bundle coverage <bundle-id-or-path>
  auv-cli skill bundle verify <bundle-id-or-path>
  auv-cli skill bundle export <bundle-id-or-path> <output-dir>
  auv-cli skill bundle package verify <package-dir>
```

Also replace this note:

```text
  - `skill run`, `skill bundle ...`, and `invoke` are the active AUV core entrypoints. Prefer them when working on product-facing runtime behavior.
```

with:

```text
  - `skill run` is a temporary JSON recipe compatibility entrypoint pending runtime legacy retirement; app-local Rust commands are the active workflow direction.
```

- [ ] **Step 5: Replace bundle parser with a removal error**

In `parse_skill`, replace:

```rust
    "bundle" => parse_skill_bundle(arguments),
```

with:

```rust
    "bundle" => Err(
      "skill bundle has been removed; use app-local Rust commands instead".to_string(),
    ),
```

Then delete the entire `parse_skill_bundle` function.

- [ ] **Step 6: Remove stale bundle parser tests**

Delete the existing test named:

```rust
fn parse_skill_bundle_coverage_command()
```

Keep the two tests added in Step 1.

- [ ] **Step 7: Run focused CLI tests**

Run:

```bash
cargo test --lib cli::tests
```

Expected:

- All `cli::tests` pass.

- [ ] **Step 8: Commit CLI parser removal**

Run:

```bash
git add src/cli.rs
git commit -m "refactor(cli): remove skill bundle commands"
```

## Task 2: Remove Bundle Startup And Main Dispatch

**Files:**

- Modify: `src/main.rs`

- [ ] **Step 1: Remove bundle imports**

In `src/main.rs`, delete:

```rust
use auv_cli::bundle::{
  SkillBundleCatalog, export_bundle, render_bundle_package_coverage, verify_bundle,
  verify_exported_bundle_package_standalone,
};
```

- [ ] **Step 2: Stop discovering the bundle catalog**

Delete this line:

```rust
  let bundle_catalog = SkillBundleCatalog::discover(&project_root)?;
```

Keep these lines:

```rust
  let runtime_version = env!("CARGO_PKG_VERSION").to_string();
  let skill_catalog = SkillCatalog::discover(&project_root)?;
  let case_matrix_catalog = SkillCaseMatrixCatalog::discover(&project_root)?;
```

`runtime_version`, `skill_catalog`, and `case_matrix_catalog` are still used by `skill run` / `skill cases` until PR2.

- [ ] **Step 3: Remove bundle match arms**

Delete these `match command` arms:

```rust
    CliCommand::SkillBundleList => {
      for entry in bundle_catalog.entries() {
        println!("{}", entry.manifest.metadata.id);
        println!("  {}", entry.manifest.metadata.name);
        if !entry.manifest.metadata.status.is_empty() {
          println!("  status: {}", entry.manifest.metadata.status);
        }
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillBundleShow { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read bundle manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render bundle manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
    CliCommand::SkillBundleCoverage { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      print!(
        "{}",
        render_bundle_package_coverage(entry, &skill_catalog, &case_matrix_catalog, &project_root,)?
      );
    }
    CliCommand::SkillBundleVerify { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      verify_bundle(
        &project_root,
        &runtime_version,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
      )?;
      println!("bundle: {}", entry.manifest.metadata.id);
      println!("status: verified");
      println!("path: {}", entry.path.display());
    }
    CliCommand::SkillBundleExport { query, output_dir } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      verify_bundle(
        &project_root,
        &runtime_version,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
      )?;
      export_bundle(
        &project_root,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
        PathBuf::from(output_dir),
      )?;
      println!("bundle: {}", entry.manifest.metadata.id);
      println!("status: exported");
    }
    CliCommand::SkillBundlePackageVerify { package_dir } => {
      let package_root = PathBuf::from(package_dir);
      let bundle_id = verify_exported_bundle_package_standalone(&package_root)?;
      println!("bundle: {}", bundle_id);
      println!("status: verified");
      println!("package: {}", package_root.display());
    }
```

- [ ] **Step 4: Run a focused compile check**

Run:

```bash
cargo check
```

Expected:

- This may still fail because `src/lib.rs` and `src/runtime.rs` still import `bundle`. The expected failure should not mention missing `CliCommand::SkillBundle*` variants in `src/main.rs`.

- [ ] **Step 5: Commit main dispatch removal**

If `cargo check` fails only due remaining bundle imports outside `src/main.rs`, commit:

```bash
git add src/main.rs
git commit -m "refactor(cli): stop dispatching bundle commands"
```

If `cargo check` reports an issue in `src/main.rs`, fix that issue before committing.

## Task 3: Remove Bundle Catalog From Runtime Construction

**Files:**

- Modify: `src/lib.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Remove bundle import and discovery from `src/lib.rs`**

In `src/lib.rs`, delete:

```rust
pub mod bundle;
```

Delete:

```rust
use bundle::SkillBundleCatalog;
```

In `build_runtime_with_store_root`, replace:

```rust
  let bundles = SkillBundleCatalog::discover(&project_root)?;
  let skills = SkillCatalog::discover(&project_root)?;
  let drivers = default_driver_registry();
  Ok(Runtime::new_with_catalogs(
    project_root,
    commands,
    bundles,
    skills,
    drivers,
    store,
  ))
```

with:

```rust
  let skills = SkillCatalog::discover(&project_root)?;
  let drivers = default_driver_registry();
  Ok(Runtime::new_with_catalogs(
    project_root,
    commands,
    skills,
    drivers,
    store,
  ))
```

- [ ] **Step 2: Remove bundle fields from `Runtime`**

In `src/runtime.rs`, delete:

```rust
use crate::bundle::{SkillBundleCatalog, SkillBundleCatalogEntry, SkillBundleCommand};
```

In `Runtime`, delete:

```rust
  bundles: SkillBundleCatalog,
```

In `Runtime::new`, delete the bundle discovery block:

```rust
    let bundles = SkillBundleCatalog::discover(&project_root).unwrap_or_else(|error| {
      panic!(
        "failed to discover bundle catalog from {}: {error}",
        project_root.display()
      )
    });
```

Change the call from:

```rust
    Self::new_with_catalogs(project_root, commands, bundles, skills, drivers, store)
```

to:

```rust
    Self::new_with_catalogs(project_root, commands, skills, drivers, store)
```

Change `Runtime::new_with_catalogs` from:

```rust
  pub fn new_with_catalogs(
    project_root: PathBuf,
    commands: CommandCatalog,
    bundles: SkillBundleCatalog,
    skills: SkillCatalog,
    drivers: DriverRegistry,
    store: LocalStore,
  ) -> Self {
    Self {
      project_root,
      commands,
      bundles,
      skills,
      drivers,
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }
```

to:

```rust
  pub fn new_with_catalogs(
    project_root: PathBuf,
    commands: CommandCatalog,
    skills: SkillCatalog,
    drivers: DriverRegistry,
    store: LocalStore,
  ) -> Self {
    Self {
      project_root,
      commands,
      skills,
      drivers,
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }
```

- [ ] **Step 3: Update all `Runtime::new_with_catalogs` call sites**

Use:

```bash
rg -n "new_with_catalogs|runtime_with_bundle_catalog|SkillBundleCatalog" src
```

For each call to `Runtime::new_with_catalogs`, remove the bundle argument.

Example shape:

```rust
Runtime::new_with_catalogs(
  project_root,
  commands,
  skills,
  drivers,
  store,
)
```

- [ ] **Step 4: Run compile check and record expected runtime failures**

Run:

```bash
cargo check
```

Expected:

- It may still fail because bundle command execution helper functions and tests still reference removed bundle types.
- It should not fail in `src/lib.rs` due `SkillBundleCatalog`.

- [ ] **Step 5: Commit runtime constructor simplification**

If compile errors are limited to bundle command execution helpers/tests in `src/runtime.rs` and deleted `src/bundle` exports, commit:

```bash
git add src/lib.rs src/runtime.rs
git commit -m "refactor(runtime): remove bundle catalog construction"
```

If errors include changed `Runtime::new_with_catalogs` call sites outside bundle tests, update those call sites before committing.

## Task 4: Remove Bundle-Backed Invoke From Runtime

**Files:**

- Modify: `src/runtime.rs`

- [ ] **Step 1: Remove bundle resolution from `invoke_in_span`**

Find the current command resolution block in `Runtime::invoke_in_span` that has this shape:

```rust
    let bundle_command = self.resolve_bundle_command(&command_id)?;
    let direct_command = self.commands.resolve(&command_id);

    match (bundle_command, direct_command) {
      (Some(bundle_command), Some(direct_command)) => Err(format!(
        "ambiguous command {command_id}; matched both direct command {} and bundle command {}",
        direct_command.id,
        render_bundle_command_match(&bundle_command)
      )),
      (Some(bundle_command), None) => {
        self.invoke_bundle_command_in_span(run, parent, request, bundle_command)
      }
      (None, Some(command)) => self.invoke_direct_command_in_span(run, parent, request, command),
      (None, None) => Err(format!(
        "unknown command {command_id}; use `list-commands` or `auv-cli skill bundle list` to inspect available entries"
      )),
    }
```

Replace it with:

```rust
    let direct_command = self.commands.resolve(&command_id);

    match direct_command {
      Some(command) => self.invoke_direct_command_in_span(run, parent, request, command),
      None => Err(format!(
        "unknown command {command_id}; use `list-commands` to inspect available entries"
      )),
    }
```

- [ ] **Step 2: Delete bundle execution helpers**

Delete these items from `src/runtime.rs`:

```rust
fn invoke_bundle_command_in_span(...)
fn resolve_bundle_command(...)
struct ResolvedBundleCommand<'a> { ... }
fn bundle_command_attributes(...)
fn render_bundle_command_match(...)
```

Delete only the bundle-related helpers. Keep direct driver invoke helpers and recording helpers.

- [ ] **Step 3: Remove bundle runtime tests**

Delete tests that are specifically about bundle-backed invoke:

```rust
fn invoke_rejects_ambiguous_bundle_and_direct_command_id()
fn invoke_bundle_command_dry_run_executes_recipe_path()
```

Delete helper functions that only support those tests:

```rust
fn runtime_with_bundle_catalog(...)
fn runtime_bundle_project_root(...)
fn write_bundle_recipe(...)
fn bundle_catalog_with_command(...)
```

If a helper is used by non-bundle tests, keep it and remove only the bundle parameter.

- [ ] **Step 4: Add a replacement unknown-command test**

Add or update a runtime test so the removed bundle hint is covered:

```rust
#[test]
fn invoke_unknown_command_points_to_list_commands_only() {
  let project_root = temp_dir("unknown-command-no-bundle-hint");
  let runtime = Runtime::new_with_catalogs(
    project_root.clone(),
    CommandCatalog::new(Vec::new()),
    skill_catalog_for_project(&project_root),
    DriverRegistry::new(Vec::new()),
    LocalStore::new(temp_dir("unknown-command-no-bundle-store")).expect("store should create"),
  );
  let request = InvokeRequest {
    command_id: "missing.command".to_string(),
    dry_run: false,
    target: ExecutionTarget::None,
    label: None,
    inputs: BTreeMap::new(),
    inspect: Default::default(),
  };

  let error = runtime
    .invoke(request)
    .expect_err("unknown command should fail");

  assert!(error.contains("unknown command missing.command"));
  assert!(error.contains("list-commands"));
  assert!(!error.contains("skill bundle"));
}
```

This uses the existing `skill_catalog_for_project` runtime-test helper. Do not
add a new public `SkillCatalog` constructor only for this test.

- [ ] **Step 5: Run runtime tests**

Run:

```bash
cargo test --lib runtime::tests
```

Expected:

- All `runtime::tests` pass.

- [ ] **Step 6: Commit bundle-backed invoke removal**

Run:

```bash
git add src/runtime.rs
git commit -m "refactor(runtime): remove bundle-backed invoke"
```

## Task 5: Delete Bundle Module And Manifests

**Files:**

- Delete: `src/bundle/**`
- Delete: `bundles/**`

- [ ] **Step 1: Delete active bundle files**

Run:

```bash
git rm -r src/bundle bundles
```

Expected:

- `src/bundle/*` files are staged for deletion.
- `bundles/native-app-skill-tree.v0.json` and `bundles/game-slay-the-spire.v0.json` are staged for deletion.

- [ ] **Step 2: Check for remaining production imports**

Run:

```bash
rg -n "crate::bundle|auv_cli::bundle|SkillBundle|skill bundle|bundle-backed|bundles/" src crates Cargo.toml
```

Expected remaining hits:

- None in production code.
- If hits remain in tests for removed behavior, delete those tests.

- [ ] **Step 3: Keep historical docs unchanged**

Do not edit old reference docs in this PR solely to remove historical mentions of bundles. The active documentation update already lives in:

```text
docs/superpowers/specs/2026-06-11-runtime-legacy-retirement-design.md
docs/ai/references/2026-06-10-rust-orchestration-recipes-bundles-retirement.md
docs/ai/references/2026-06-10-recipe-bundle-retirement-inventory.md
```

- [ ] **Step 4: Run full compile and tests**

Run:

```bash
cargo check
cargo test
git diff --check
```

Expected:

- `cargo check` passes.
- `cargo test` passes or fails only in unrelated pre-existing tests. If failures occur, inspect them before claiming success.
- `git diff --check` passes.

- [ ] **Step 5: Commit deleted bundle files**

Run:

```bash
git add -u
git commit -m "refactor(bundle): remove active bundle surface"
```

## Task 6: Final PR Verification And Summary

**Files:**

- No new files expected.

- [ ] **Step 1: Run absence checks**

Run:

```bash
rg -n "SkillBundle|SkillBundleCatalog|skill bundle|crate::bundle|auv_cli::bundle|bundles/" src crates Cargo.toml
```

Expected:

- No hits in `src`, `crates`, or `Cargo.toml`.

- [ ] **Step 2: Confirm recipes remain for PR2**

Run:

```bash
test -d recipes
rg -n "SkillCatalog::discover|skill run|skill cases" src/cli.rs src/main.rs src/runtime.rs src/skill
```

Expected:

- `recipes` directory still exists.
- Hits remain for `skill run`, `skill cases`, and `SkillCatalog::discover`. These are intentionally deferred to PR2.

- [ ] **Step 3: Run final verification**

Run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

Expected:

- All commands pass.
- If full `cargo fmt --check` fails due pre-existing unrelated formatting, run:

```bash
cargo fmt --check --package auv-cli
```

and record the full-workspace formatter failure separately in the PR summary.

- [ ] **Step 4: Push branch and open PR**

Confirm the current branch:

```bash
git branch --show-current
```

Expected:

```text
refactor/remove-bundle-execution
```

Push the branch:

```bash
git push -u origin refactor/remove-bundle-execution
```

Open a PR with this summary:

```markdown
## Summary
- Remove `skill bundle ...` CLI parsing, help, and dispatch.
- Remove bundle catalog discovery from root startup and Runtime construction.
- Remove bundle-backed invoke resolution.
- Delete active `src/bundle/**` and checked-in `bundles/**` manifests.

## Deferred
- JSON recipe and case-matrix execution remain for the next PR.
- `skill run` and `skill cases` remain for the next PR.
- Root catalog/runtime/driver removal remain later phases of runtime legacy retirement.

## Verification
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`
```

- [ ] **Step 5: Do not start PR2 in the same branch**

Stop after opening the bundle-removal PR. PR2 starts from updated `main` after this PR merges.
