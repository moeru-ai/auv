# Remove Skill Recipe Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** implemented on branch `refactor/remove-skill-recipe-chain`.

**Goal:** Remove active `skill`/JSON recipe/case-matrix execution and discovery from the root AUV CLI/MCP/runtime lane.

**Architecture:** This PR removes the old `skill` surface instead of preserving a compatibility registry. User-facing `skill ...`, app distillation/validation recipe generation, scan recipe hooks, `src/skill/**`, and `recipes/**` are removed together so no production path can discover or execute JSON recipes. `app probe`, `app analyze`, `invoke`, inspect, MCP invoke, and candidate-action remain buildable because they are not recipe execution surfaces.

**Implementation commits:**

- `cd7f080` / `860a71e`: remove skill/app recipe CLI surfaces.
- `2a16f68`: remove root CLI skill dispatch.
- `4a390c7`: remove MCP skill tools.
- `5f51eaa`: remove scroll-scan recipe hooks.
- `d0fbf67`: remove app recipe distillation schema.
- `37e2e37`: delete `src/skill/**` and `recipes/**`.

**Tech Stack:** Rust 2024, root `auv-cli` crate, `rmcp`, serde JSON, existing CLI parser tests, existing Rust unit tests.

---

## File Structure

Modify:

- `src/cli.rs`: remove `CliCommand::Skill*`, remove `AppDistill` and `AppValidate` active parser variants because they produce or consume recipe/case-matrix artifacts, remove `skill` help text, add parser-level removal errors for `skill ...`, `app distill`, and `app validate`.
- `src/main.rs`: remove `SkillCatalog`/`SkillCaseMatrixCatalog` startup discovery and all `CliCommand::Skill*`, `AppDistill`, and `AppValidate` dispatch arms.
- `src/mcp.rs`: remove `skill_list`, `skill_show`, `SkillShowRequest`, and `SkillCatalog` import/helper.
- `src/scroll_scan/mod.rs`: remove recipe hook fields and execution paths; keep scan behavior without hooks and return explicit errors if old hook flags are supplied by the CLI parser.
- `src/app/mod.rs`, `src/app/analysis.rs`, `src/app/report.rs`, `src/app/tests.rs`: remove recipe/case-matrix generation and validation paths by deleting active distill/validate behavior, while keeping app probe/analyze.
- `src/lib.rs`: remove `pub mod skill` after all production imports are gone.
- `README.md`, `AGENTS.md`, `CLAUDE.md`, `docs/ai/references/**`, `docs/superpowers/specs/2026-06-11-skill-recipe-removal-sequence-design.md`: update active guidance away from `skill`, recipes, and case matrices.

Delete:

- `src/skill/**`
- `recipes/**`

Do not modify in this PR:

- `src/catalog.rs`
- `src/runtime.rs` except to remove imports or tests that only refer to deleted skill behavior
- root `src/driver/**`
- `invoke` command behavior
- `list-commands` behavior

---

### Task 1: Remove CLI Skill And App Recipe-Producing Surfaces

**Files:**

- Modify: `src/cli.rs`
- Test: `src/cli.rs`

- [ ] **Step 1: Replace skill parser coverage with removal tests**

In `src/cli.rs`, replace tests that expect `CliCommand::SkillRun`, `CliCommand::SkillCasesRun`, `SkillList`, `SkillShow`, `SkillCasesList`, `SkillCasesShow`, or `SkillCasesReport` with these tests:

```rust
#[test]
fn parse_skill_commands_are_removed() {
  for args in [
    vec!["skill"],
    vec!["skill", "list"],
    vec!["skill", "show", "macos.textedit.create_and_verify_text.v0"],
    vec!["skill", "run", "recipes/macos/textedit/create-and-verify-text.v0.json"],
    vec!["skill", "cases", "list"],
    vec![
      "skill",
      "cases",
      "run",
      "recipes/macos/textedit/create-and-verify-text.cases.v0.json",
    ],
  ] {
    let args = args.into_iter().map(String::from).collect::<Vec<_>>();
    let error = parse_cli(&args).expect_err("skill command should be removed");
    assert!(
      error.contains("skill commands have been removed"),
      "unexpected error for {args:?}: {error}"
    );
  }
}

#[test]
fn parse_app_distill_and_validate_are_removed() {
  for args in [
    vec!["app", "distill", ".auv/app-probes/example/analysis.json"],
    vec!["app", "validate", ".auv/app-probes/example/distillation.json"],
  ] {
    let args = args.into_iter().map(String::from).collect::<Vec<_>>();
    let error = parse_cli(&args).expect_err("recipe-producing app command should be removed");
    assert!(
      error.contains("app recipe distillation has been removed"),
      "unexpected error for {args:?}: {error}"
    );
  }
}

#[test]
fn help_text_no_longer_lists_skill_or_recipe_app_commands() {
  let help = help_text();
  assert!(!help.contains("auv-cli skill"));
  assert!(!help.contains("skill run"));
  assert!(!help.contains("skill cases"));
  assert!(!help.contains("app distill"));
  assert!(!help.contains("app validate"));
  assert!(help.contains("auv-cli app probe"));
  assert!(help.contains("auv-cli app analyze"));
}
```

- [ ] **Step 2: Run CLI tests and verify they fail**

```bash
cargo test cli::tests::parse_skill_commands_are_removed
cargo test cli::tests::parse_app_distill_and_validate_are_removed
cargo test cli::tests::help_text_no_longer_lists_skill_or_recipe_app_commands
```

Expected before implementation: failures because old parser still accepts `skill`, `app distill`, or `app validate`, and help still lists them.

- [ ] **Step 3: Remove `CliCommand` variants and parser branches**

In `src/cli.rs`, delete these `CliCommand` variants:

```rust
  AppDistill {
    query: String,
    output_dir: Option<String>,
  },
  AppValidate {
    query: String,
  },
  SkillList,
  SkillShow {
    query: String,
  },
  SkillCasesList,
  SkillCasesShow {
    query: String,
  },
  SkillCasesReport {
    query: String,
  },
  SkillCasesRun {
    query: String,
    dry_run: bool,
    max_disturbance: Option<DisturbanceClass>,
    only_case_ids: Vec<String>,
    include_nonvalidated: bool,
    inspect: InspectClientOptions,
  },
  SkillRun {
    query: String,
    dry_run: bool,
    max_disturbance: Option<DisturbanceClass>,
    overrides: BTreeMap<String, String>,
    inspect: InspectClientOptions,
  },
```

Replace the top-level parser arms for removed commands with explicit errors:

```rust
    "skill" => Err("skill commands have been removed; use app-local Rust commands instead".to_string()),
```

In the `app` parser, make `distill` and `validate` return:

```rust
Err("app recipe distillation has been removed; use app-local Rust commands instead".to_string())
```

Delete these parser helpers after their callers are gone. Remove the complete
function bodies from their `fn ...` line through the matching closing brace:

```rust
fn parse_skill(arguments: &[String]) -> AuvResult<CliCommand>
fn parse_skill_cases(arguments: &[String]) -> AuvResult<CliCommand>
fn parse_skill_run(arguments: &[String]) -> AuvResult<CliCommand>
fn parse_skill_cases_run(arguments: &[String]) -> AuvResult<CliCommand>
```

Remove `BTreeMap` import from `src/cli.rs` if it becomes unused.

- [ ] **Step 4: Remove help text for deleted surfaces**

In `help_text()`, delete usage lines and notes for:

```text
auv-cli app distill <analysis-dir-or-analysis-json> [--output-dir <dir>]
auv-cli app validate <distill-dir-or-distillation-json>
auv-cli skill list
auv-cli skill show <skill-id-or-path>
auv-cli skill cases list
auv-cli skill cases show <matrix-id-or-path>
auv-cli skill cases report <matrix-id-or-path>
auv-cli skill cases run <matrix-id-or-path> [--case <case-id>] [--all-statuses] [--dry-run] [--max-disturbance <class>] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
auv-cli skill run <skill-id-or-path> [--dry-run] [--max-disturbance <class>] [--set key=value] [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]
skill run
skill cases run
candidate-skill distillation
generated case matrix
```

Keep `app probe` and `app analyze` help because they remain active.

- [ ] **Step 5: Run CLI tests**

Run:

```bash
cargo test cli::tests::parse_skill_commands_are_removed
cargo test cli::tests::parse_app_distill_and_validate_are_removed
cargo test cli::tests::help_text_no_longer_lists_skill_or_recipe_app_commands
```

Expected: all three pass.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "refactor(cli): remove skill recipe commands"
```

---

### Task 2: Remove Main Dispatch And Startup Skill Discovery

**Files:**

- Modify: `src/main.rs`

- [ ] **Step 1: Remove skill imports and startup discovery**

In `src/main.rs`, delete:

```rust
use auv_cli::app::{analyze_app_probe, distill_app_analysis, probe_app, validate_app_distillation};
use auv_cli::skill::{
  SkillCaseMatrixCatalog, SkillCatalog, render_skill_case_matrix_report, run_skill,
  run_skill_case_matrix,
};
```

Replace the app import with:

```rust
use auv_cli::app::{analyze_app_probe, probe_app};
```

Delete startup discovery:

```rust
  let skill_catalog = SkillCatalog::discover(&project_root)?;
  let case_matrix_catalog = SkillCaseMatrixCatalog::discover(&project_root)?;
```

- [ ] **Step 2: Remove dispatch arms for deleted variants**

Delete the complete `match command` arms for these patterns:

```rust
CliCommand::AppDistill
CliCommand::AppValidate
CliCommand::SkillList
CliCommand::SkillShow
CliCommand::SkillCasesList
CliCommand::SkillCasesShow
CliCommand::SkillCasesReport
CliCommand::SkillCasesRun
CliCommand::SkillRun
```

Do not add replacement runtime calls. The parser returns removal errors before dispatch.

- [ ] **Step 3: Run a build check**

Run:

```bash
cargo check
```

Expected before later tasks: likely fails because `src/mcp.rs`, `src/app/**`, `src/scroll_scan/mod.rs`, and `src/lib.rs` still import `skill`. The expected success criterion for this task is that no error points to `src/main.rs` skill imports or deleted `CliCommand` variants.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor(cli): stop dispatching skill recipes"
```

---

### Task 3: Remove MCP Skill Tools

**Files:**

- Modify: `src/mcp.rs`
- Test: `src/mcp.rs`

- [ ] **Step 1: Update MCP tool listing test**

In `mcp_server_lists_and_invokes_shared_runtime`, keep existing assertions for `invoke`, `run_inspect`, and `candidate_action_run`, and add:

```rust
assert!(!tool_names.contains(&"skill_list"));
assert!(!tool_names.contains(&"skill_show"));
```

- [ ] **Step 2: Remove MCP skill catalog code**

Delete from `src/mcp.rs`:

```rust
use crate::skill::SkillCatalog;
```

Delete helper:

```rust
  fn skill_catalog(&self) -> Result<SkillCatalog, McpError> {
    SkillCatalog::discover(&self.project_root).map_err(invalid_params)
  }
```

Delete the complete MCP tool functions whose signatures start with:

```rust
  async fn skill_list(&self) -> Result<CallToolResult, McpError>

  async fn skill_show(
    &self,
    Parameters(req): Parameters<SkillShowRequest>,
  ) -> Result<CallToolResult, McpError>
```

Delete request type:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct SkillShowRequest {
  query: String,
}
```

Keep `read_manifest_value` only if another MCP tool still uses it. If no caller remains, delete it and the `internal_error` helper if that also becomes unused.

- [ ] **Step 3: Format and test**

Run:

```bash
rustfmt --edition 2024 src/mcp.rs
cargo test mcp_server_lists_and_invokes_shared_runtime
```

Expected: MCP focused test passes.

- [ ] **Step 4: Commit**

```bash
git add src/mcp.rs
git commit -m "refactor(mcp): remove skill tools"
```

---

### Task 4: Remove Scroll Scan Recipe Hooks

**Files:**

- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Modify: `src/scroll_scan/mod.rs`
- Test: `src/scroll_scan/mod.rs`, `src/cli.rs`

- [ ] **Step 1: Add parser tests for removed scan recipe hook flags**

In `src/cli.rs`, add:

```rust
#[test]
fn parse_scan_window_region_rejects_recipe_hooks() {
  for flag in [
    "--per-page-after-observe-recipe",
    "--per-list-item-candidate-recipe",
    "--on-stop-candidate-recipe",
  ] {
    let args = vec![
      "scan".to_string(),
      "window-region".to_string(),
      "--target".to_string(),
      "com.example.App".to_string(),
      "--region".to_string(),
      "0,0,1,1".to_string(),
      flag.to_string(),
      "recipes/scan/list-item-candidate-continue-hook.v0.json".to_string(),
    ];
    let error = parse_cli(&args).expect_err("recipe hook flags should be removed");
    assert!(
      error.contains("scan recipe hooks have been removed"),
      "unexpected error for {flag}: {error}"
    );
  }
}
```

- [ ] **Step 2: Remove scan hook fields from CLI command**

In `CliCommand::ScanWindowRegion`, delete:

```rust
      per_page_after_observe_recipe,
      per_list_item_candidate_recipe,
      on_stop_candidate_recipe,
```

In the scan parser, make old flags return:

```rust
Err("scan recipe hooks have been removed; typed interaction hooks will replace them".to_string())
```

- [ ] **Step 3: Remove scan hook fields from runtime options**

In `src/scroll_scan/mod.rs`, delete from `ScanWindowRegionOptions`:

```rust
  pub per_page_after_observe_recipe: Option<String>,
  pub per_page_after_observe_inline_hook: Option<crate::skill::SkillManifest>,
  pub per_list_item_candidate_recipe: Option<String>,
  pub per_list_item_candidate_inline_hook: Option<crate::skill::SkillManifest>,
  pub on_stop_candidate_recipe: Option<String>,
  pub on_stop_candidate_inline_hook: Option<crate::skill::SkillManifest>,
```

Add this comment near `ScanWindowRegionOptions`:

```rust
// TODO(tracing-interaction-hooks): recipe-backed scan hooks were removed with
// JSON recipe execution. Reintroduce hook composition only as typed Rust
// interaction hooks once `auv-tracing-interaction` owns macro-operation
// recording.
```

- [ ] **Step 4: Delete recipe hook execution helpers**

Delete these functions from `src/scroll_scan/mod.rs`:

```rust
attach_inline_scan_hooks_from_manifest
run_list_item_candidate_hooks
validate_scan_sub_recipe
run_optional_scan_hook
resolve_scan_hook_manifest
list_item_candidate_hook_overrides
hook_decision_from_variables
parse_hook_action
validate_list_item_candidate_hook_decision
```

If one of these helpers is still used for non-recipe scan behavior, keep it and remove only the `crate::skill` dependency. Use `rg -n "hook_decision_from_variables|HookDecisionRecord|run_optional_scan_hook|run_list_item_candidate_hooks" src/scroll_scan/mod.rs` to verify.

- [ ] **Step 5: Replace hook call sites with no-op typed-hook deferrals**

Where the scan loop currently calls optional hooks, remove the call and keep the scan path unchanged. Add a single comment at the removed call site:

```rust
// TODO(tracing-interaction-hooks): typed scan hooks are deferred until
// `auv-tracing-interaction`; the removed JSON recipe hook path must not be
// reintroduced.
```

- [ ] **Step 6: Update main scan dispatch**

In `src/main.rs`, remove hook fields when constructing `ScanWindowRegionOptions`.

- [ ] **Step 7: Remove or rewrite scroll-scan hook tests**

Delete tests that only validate recipe hook manifests or hook recipe execution, including tests named like:

```text
attach_inline_scan_hooks_from_manifest_injects_parent_local_hook
hook_decision_parses_exported_recipe_variables
hook_decision_prefers_structured_signal_when_present
hook_decision_rejects_mismatched_structured_page_index
hook_decision_rejects_unknown_action
list_item_candidate_hook_overrides_include_outer_scan_context
scan_loop_rejects_unimplemented_hook_actions
scan_window_region_executes_inline_list_item_hook_under_scan_run
scan_window_region_keeps_standalone_list_item_hook_recipe_compatible
```

Keep tests for scan artifact serialization, observation merging, boundary detection, and stop policies.

- [ ] **Step 8: Run focused checks**

Run:

```bash
cargo test parse_scan_window_region_rejects_recipe_hooks
cargo test scroll_scan::
cargo check
```

Expected: focused parser test and scroll-scan tests pass; no `crate::skill` import remains in `src/scroll_scan/mod.rs`.

- [ ] **Step 9: Commit**

```bash
git add src/cli.rs src/main.rs src/scroll_scan/mod.rs
git commit -m "refactor(scroll-scan): remove recipe hooks"
```

---

### Task 5: Remove App Distillation And Validation Recipe Schema

**Files:**

- Modify: `src/app/mod.rs`
- Modify: `src/app/analysis.rs`
- Modify: `src/app/report.rs`
- Modify: `src/app/tests.rs`

- [ ] **Step 1: Remove active distill/validate exports**

In `src/app/mod.rs`, delete the complete public functions whose signatures
start with:

```rust
pub fn distill_app_analysis(
pub fn validate_app_distillation(
```

Delete the complete private helpers whose signatures start with:

```rust
fn distill_app_analysis_into_run(
fn validate_app_distillation_into_run(
fn inject_promoted_candidate_runtime_inputs(
fn enforce_promoted_candidate_consumer_expectations(
fn step_references_input(
fn ensure_manifest_string_input(
fn default_distill_output_dir(
```

Delete output structs that only exist for distill/validate:

```rust
pub struct AppDistillOutput
pub struct AppValidateOutput
```

Keep `AppAnalysis`, `AppProbe`, probe/analyze functions, candidate analysis contracts, and report rendering used by `app analyze`.

- [ ] **Step 2: Remove recipe/case fields from app candidate structs**

For structs that remain in `src/app/mod.rs`, remove fields:

```rust
  pub recipe_path: PathBuf,
  pub case_matrix_path: PathBuf,
```

If a struct becomes unused after deleting distill/validate, delete the whole struct.

- [ ] **Step 3: Remove app analysis dependency on skill taxonomy types**

In `src/app/analysis.rs`, replace:

```rust
use crate::skill::{SkillCaseMatrix, SkillStrategy};
```

with app-local neutral strategy structs if needed:

```rust
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct AppStrategyShape {
  pub family: String,
  pub grounding: String,
  pub activation: String,
  pub verification_contract: String,
}
```

If functions only render candidate recipe/case values, delete those functions instead of introducing replacement types. Prefer deletion for functions named:

```text
render_*_recipe
render_*_case*
validate_*_case*
```

- [ ] **Step 4: Remove app report references to recipe/case paths**

In `src/app/report.rs`, delete report lines like:

```rust
lines.push(format!("  - recipe: `{}`", candidate.recipe_path.display()));
lines.push(format!("  - case matrix: `{}`", candidate.case_matrix_path.display()));
```

If whole report functions only render distillation/validation reports, delete them with their callers.

- [ ] **Step 5: Delete recipe/case app tests**

In `src/app/tests.rs`, delete tests that:

- deserialize `SkillManifest`
- deserialize `SkillCaseMatrix`
- write `candidate.recipe.json`
- write `candidate.cases.json`
- call `distill_app_analysis`
- call `validate_app_distillation`
- assert generated recipes pass skill validators

Keep tests for:

- probe path resolution
- analysis JSON shape
- candidate grounding that does not require recipe/case execution
- report rendering for `app analyze`
- contract shapes such as `RecognitionResult`, `CandidateRef`, `OperationResult`, and `VerificationResult`

- [ ] **Step 6: Verify no app skill imports remain**

Run:

```bash
rg -n "crate::skill|SkillManifest|SkillCaseMatrix|SkillStrategy|recipe_path|case_matrix_path|run_skill_case_matrix" src/app
```

Expected: no matches.

- [ ] **Step 7: Run app-focused tests**

Run:

```bash
cargo test app:: --lib
cargo check
```

Expected: app tests pass and no app module imports `skill`.

- [ ] **Step 8: Commit**

```bash
git add src/app
git commit -m "refactor(app): remove recipe distillation surface"
```

---

### Task 6: Delete Skill Module And Recipes Tree

**Files:**

- Modify: `src/lib.rs`
- Delete: `src/skill/**`
- Delete: `recipes/**`

- [ ] **Step 1: Delete skill module export**

In `src/lib.rs`, delete:

```rust
pub mod skill;
```

- [ ] **Step 2: Delete source directories**

Run:

```bash
git rm -r src/skill recipes
```

Expected: Git stages deletion of all skill source files and checked-in recipe files.

- [ ] **Step 3: Search for remaining skill references**

Run:

```bash
rg -n "crate::skill|auv_cli::skill|SkillManifest|SkillCaseMatrix|SkillCatalog|SkillRecipe|skill run|skill cases|recipes/" src crates README.md AGENTS.md CLAUDE.md docs/ai/references docs/superpowers/specs
```

Expected: remaining matches are only historical docs or intentional removal guidance. No match in `src/**/*.rs` or `crates/**/*.rs` should refer to deleted `skill` module types.

- [ ] **Step 4: Remove source-code references**

For any remaining source-code match from Step 3:

- Delete the code if it only served recipe execution.
- Replace user-facing text with:

```text
skill commands have been removed; use app-local Rust commands instead
```

Do not introduce a compatibility module.

- [ ] **Step 5: Run build**

Run:

```bash
cargo check
```

Expected: passes except for existing warnings unrelated to this PR. If it fails because a test imports deleted skill types, remove that test if it covered removed behavior.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs
git commit -m "refactor(skill): delete recipe engine"
```

---

### Task 7: Update Documentation And Active Guidance

**Files:**

- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`
- Modify: `docs/ai/references/2026-06-10-rust-orchestration-recipes-bundles-retirement.md`
- Modify: `docs/ai/references/2026-06-10-recipe-bundle-retirement-inventory.md`
- Modify: `docs/ai/references/2026-06-10-auv-cli-invoke-catalog-removal.md`
- Modify: any docs found by search

- [ ] **Step 1: Update active command guidance**

Search:

```bash
rg -n "skill run|skill cases|recipes/|JSON recipe|case matrix|SkillManifest|SkillCaseMatrix|bundle surface retired|fallback" README.md AGENTS.md CLAUDE.md docs/ai/references docs/superpowers/specs
```

In active docs, replace claims that users should run recipes with:

```text
JSON recipe and case-matrix execution has been removed. Use app-local Rust
commands and typed driver APIs for active workflow automation.
```

Historical docs may keep old wording only if the section is explicitly marked
historical or archived.

- [ ] **Step 2: Update validation command lists**

Remove commands that no longer exist from active validation sections:

```text
cargo run --quiet -- skill cases list
cargo run --quiet -- skill bundle list
cargo run --quiet -- skill run recipes/macos/textedit/create-and-verify-text.v0.json
cargo run --quiet -- skill cases run recipes/macos/textedit/create-and-verify-text.cases.v0.json
```

Replace with removal checks:

```text
cargo run --quiet -- skill run recipes/macos/textedit/create-and-verify-text.v0.json
cargo run --quiet -- skill cases list
```

Both should be documented as expected failures.

- [ ] **Step 3: Update new sequence spec status**

In `docs/superpowers/specs/2026-06-11-skill-recipe-removal-sequence-design.md`, change:

```text
Status: proposed owner-update spec
```

to:

```text
Status: implementation planned
```

- [ ] **Step 4: Run doc diff check**

Run:

```bash
git diff --check
```

Expected: passes.

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md CLAUDE.md docs/ai/references docs/superpowers/specs
git commit -m "docs: retire active skill recipe guidance"
```

---

### Task 8: Final Verification And PR Preparation

**Files:**

- No planned edits unless verification finds a source reference missed earlier.

- [ ] **Step 1: Run full verification**

Run:

```bash
cargo check
cargo test
git diff --check
cargo run --quiet -- skill run recipes/macos/textedit/create-and-verify-text.v0.json
cargo run --quiet -- skill cases list
cargo run --quiet -- app probe com.apple.TextEdit --output-dir /tmp/auv-app-probe-smoke
cargo run --quiet -- list-commands
```

Expected:

- `cargo check`: passes.
- `cargo test`: passes.
- `git diff --check`: passes.
- `skill run recipes/macos/textedit/create-and-verify-text.v0.json`: exits non-zero with `skill commands have been removed`.
- `skill cases list`: exits non-zero with `skill commands have been removed`.
- `app probe ...`: may fail on machines without macOS permissions or TextEdit state; if it fails for environment/permission reasons, record the exact error in the PR instead of hiding it.
- `list-commands`: still works in this PR because catalog redesign is PR3.

- [ ] **Step 2: Verify no active recipe files remain**

Run:

```bash
find recipes -maxdepth 3 -type f
```

Expected: command fails because `recipes` no longer exists, or prints nothing if a tombstone directory remains intentionally empty.

- [ ] **Step 3: Verify no source skill module references remain**

Run:

```bash
rg -n "crate::skill|auv_cli::skill|SkillManifest|SkillCaseMatrix|SkillCatalog|SkillRecipe|run_skill" src crates
```

Expected: no matches.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git diff --stat origin/main...HEAD
git diff --name-status origin/main...HEAD
```

Expected: deletions include `src/skill/**` and `recipes/**`; modifications include CLI, MCP, app, scroll-scan, docs.

- [ ] **Step 5: Push branch and open PR**

Use branch:

```bash
git switch -c refactor/remove-skill-recipe-chain
git push -u origin refactor/remove-skill-recipe-chain
```

Open PR with title:

```text
refactor(auv): remove skill recipe chain
```

PR body:

```markdown
## Summary
- Remove active `skill` CLI/MCP surfaces and JSON recipe/case-matrix execution.
- Delete `src/skill/**` and `recipes/**` as active source.
- Remove recipe-backed scroll scan hooks and app recipe distillation/validation surfaces.

## Verification
- `cargo check`
- `cargo test`
- `git diff --check`
- `cargo run --quiet -- skill run recipes/macos/textedit/create-and-verify-text.v0.json` (expected removal error)
- `cargo run --quiet -- skill cases list` (expected removal error)
- `cargo run --quiet -- list-commands`

## Follow-ups
- Redesign `invoke` and `list-commands` around app-local Rust commands.
- Extract recording to `auv-tracing-driver`.
- Delete remaining runtime/root driver compatibility.
```
