# Archived Candidate-Action Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the unused archived `candidate-action` execution path from CLI, MCP, runtime, recording, and inspect surfaces while keeping the reusable `candidate_promotion` gate.

**Architecture:** Treat `candidate-action` as a removed archived vertical, not as a supported compatibility surface. Delete the public entrypoints first, then remove the command implementation, artifact readers, inspect projections, viewer panels, and now-unreferenced support modules. Keep `src/candidate_promotion.rs` because AGENTS.md identifies it as a reusable promotion/gating seam.

**Tech Stack:** Rust 2024, Cargo workspace, serde-backed run artifacts, in-repo inspect server HTML.

---

## File Map

- Delete: `src/candidate_action_command.rs`
- Delete: `src/candidate_action_decision.rs`
- Delete: `src/ax_recognition.rs`
- Delete: `src/candidate_promotion_recording.rs`
- Delete after no remaining production references: `src/action_resolver_decision.rs`
- Keep: `src/candidate_promotion.rs`
- Modify: `src/lib.rs` to remove deleted module declarations
- Modify: `src/runtime.rs` to remove `run_candidate_action_command`
- Modify: `src/cli.rs` to remove `candidate-action run` parsing, enum variant, help notes, dispatch, and tests
- Modify: `src/mcp.rs` to remove `candidate_action_run`, its request shape, parser helpers, imports, and tests
- Modify: `src/run_read.rs` to remove candidate-action, candidate-promotion artifact compatibility, and action-transition readers tied to candidate-action artifacts
- Modify: `src/inspect.rs` to remove candidate-action/promotion/action-transition text rendering
- Modify: `src/inspect_server/mod.rs` to remove candidate-action/promotion/action-transition JSON fields and tests
- Modify: `src/inspect_server_viewer.html` to remove action-transition and candidate-action client panels/tests
- Modify: `docs/ai/references/INDEX.md` after checking whether this repository index lists every new reference document manually

## Task 1: Remove MCP Candidate-Action Tool

**Files:**
- Modify: `src/mcp.rs`

- [ ] **Step 1: Write the failing MCP surface test**

  Replace the existing tool-list assertion that currently requires `candidate_action_run` with an assertion that rejects it:

  ```rust
  assert!(
    !tool_names.contains(&"candidate_action_run"),
    "candidate_action_run is an archived vertical and must not be exposed through MCP"
  );
  ```

  Keep existing assertions for generic MCP tools such as run inspection.

- [ ] **Step 2: Run the focused MCP test and confirm it fails**

  Run:

  ```sh
  cargo test -p auv-cli mcp -- --nocapture
  ```

  Expected: FAIL because `candidate_action_run` is still registered or because removed imports have not been cleaned up yet.

- [ ] **Step 3: Remove MCP candidate-action imports**

  In `src/mcp.rs`, delete:

  ```rust
  use crate::candidate_action_command::CandidateActionCommandRequest;
  use crate::candidate_action_decision::CandidateActionKind;
  ```

- [ ] **Step 4: Remove the MCP tool method**

  Delete the whole `candidate_action_run` tool method:

  ```rust
  #[tool(
    description = "Run the archived consent-gated candidate-action command through the shared runtime. M0 evidence tool only: direct query/role target, no planner, no model proposer, no consent minting by MCP."
  )]
  async fn candidate_action_run(
    &self,
    Parameters(req): Parameters<CandidateActionRunRequest>,
  ) -> Result<CallToolResult, McpError> {
    let runtime = self.runtime(req.inspect.store_root.clone())?;
    let request = req.into_command_request().map_err(invalid_params)?;
    let output = runtime
      .run_candidate_action_command(request)
      .map_err(invalid_params)?;

    json_result(serde_json::json!({
      "run_id": output.run_id.as_str(),
      "run_dir": output.run_dir.display().to_string(),
      "status": output.value.status.as_str(),
      "proposal_artifact_id": output.value.proposal_artifact_id,
      "promotion_artifact_id": output.value.promotion_artifact_id,
      "decision_artifact_id": output.value.decision_artifact_id,
      "execution_artifact_id": output.value.execution_artifact_id,
      "promotion_refusals": output.value.promotion_refusals,
    }))
  }
  ```

- [ ] **Step 5: Remove MCP request and parser helpers**

  Delete these items from `src/mcp.rs`:

  ```rust
  #[derive(Debug, Deserialize, Serialize, JsonSchema)]
  struct CandidateActionRunRequest {
    target_app: String,
    query: String,
    role: String,
    #[serde(default = "default_candidate_action")]
    action: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    dev_self_minted_consent: bool,
    #[serde(default)]
    human_gesture_consent: bool,
    #[serde(default = "default_human_gesture_timeout_ms")]
    human_gesture_timeout_ms: u64,
    #[serde(default)]
    granted_by: String,
    #[serde(default)]
    reveal_shortcut: Option<String>,
    #[serde(default = "default_reveal_settle_ms")]
    reveal_settle_ms: u64,
    #[serde(default = "default_stable_frames")]
    stable_frames: u32,
    #[serde(default)]
    stable_frame_delay_ms: u64,
    #[serde(default = "default_max_centroid_drift_px")]
    max_centroid_drift_px: f64,
    #[serde(default = "default_require_stable_text")]
    require_stable_text: bool,
    #[serde(default)]
    inspect: McpInspectOptions,
  }

  impl CandidateActionRunRequest {
    fn into_command_request(self) -> Result<CandidateActionCommandRequest, String>
  }

  fn parse_candidate_action(action: &str, text: Option<&str>) -> Result<CandidateActionKind, String>

  fn default_candidate_action() -> String {
    "click".to_string()
  }
  ```

  Also delete tests whose names start with:

  ```text
  candidate_action_run_request_
  ```

- [ ] **Step 6: Re-run MCP tests**

  Run:

  ```sh
  cargo test -p auv-cli mcp -- --nocapture
  ```

  Expected: PASS for MCP tests, with no `candidate_action_run` tool in the tool list.

- [ ] **Step 7: Commit the MCP removal**

  ```sh
  git add src/mcp.rs
  git commit -m "refactor(mcp): remove archived candidate-action tool"
  ```

## Task 2: Remove CLI Candidate-Action Command

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Write the failing CLI parse test**

  Replace the existing `parse_candidate_action_run_command` happy-path test with:

  ```rust
  #[test]
  fn parse_candidate_action_run_command_is_removed() {
    let error = parse_cli(&[
      "candidate-action".to_string(),
      "run".to_string(),
      "--target-app".to_string(),
      "com.apple.TextEdit".to_string(),
      "--query".to_string(),
      "Body".to_string(),
      "--role".to_string(),
      "AXTextArea".to_string(),
    ])
    .expect_err("candidate-action run should no longer parse");

    assert!(
      error.contains("unknown command candidate-action")
        || error.contains("usage:"),
      "unexpected error: {error}"
    );
  }
  ```

- [ ] **Step 2: Run the focused CLI parse test and confirm it fails**

  Run:

  ```sh
  cargo test -p auv-cli parse_candidate_action_run_command_is_removed -- --nocapture
  ```

  Expected: FAIL because `candidate-action run` still parses.

- [ ] **Step 3: Remove the candidate action import**

  Delete this import from the top of `src/cli.rs`:

  ```rust
  use auv_cli::candidate_action_decision::CandidateActionKind;
  ```

- [ ] **Step 4: Remove CLI command variant and request struct**

  Delete the `CliCommand` variant:

  ```rust
  CandidateActionRun {
    request: CandidateActionCommandRequest,
    inspect: InspectWriteRequest,
  },
  ```

  Delete the whole `CandidateActionCommandRequest` struct in `src/cli.rs`.

- [ ] **Step 5: Remove parser dispatch and parser function**

  In the top-level parser match, remove:

  ```rust
  "candidate-action" => parse_candidate_action(arguments),
  ```

  Delete the full `parse_candidate_action` function.

- [ ] **Step 6: Remove CLI help notes and dispatch arms**

  Remove the three help-note lines that describe `candidate-action run` as a frozen archived vertical.

  Remove all `CliCommand::CandidateActionRun` match arms from command execution, inspect-write planning, and tests.

- [ ] **Step 7: Delete candidate-action CLI tests**

  Delete tests whose names start with:

  ```text
  parse_candidate_action_run_command
  ```

  Keep `help_text_keeps_candidate_action_as_note_only` only long enough to rewrite it into:

  ```rust
  #[test]
  fn help_text_does_not_mention_candidate_action() {
    let help = usage();
    assert!(!help.contains("candidate-action"));
    assert!(!help.contains("candidate_action"));
  }
  ```

- [ ] **Step 8: Run focused CLI tests**

  Run:

  ```sh
  cargo test -p auv-cli parse_candidate_action -- --nocapture
  cargo test -p auv-cli help_text_does_not_mention_candidate_action -- --nocapture
  ```

  Expected: first command reports no matching tests or only the removal test passes; second command PASS.

- [ ] **Step 9: Commit CLI removal**

  ```sh
  git add src/cli.rs
  git commit -m "refactor(cli): remove archived candidate-action command"
  ```

## Task 3: Remove Runtime Entrypoint and Module Declarations

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Remove runtime method**

  Delete the entire method from `impl Runtime`:

  ```rust
  pub fn run_candidate_action_command(
    &self,
    request: crate::candidate_action_command::CandidateActionCommandRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<
      crate::candidate_action_command::CandidateActionCommandOutput,
    >,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.command",
      ),
      "Consent-gated candidate action command",
      |context| {
        crate::candidate_action_command::execute_candidate_action_command(context, &request)
      },
    )
  }
  ```

- [ ] **Step 2: Remove now-unused runtime import**

  Run `rg -n "RunType" src/runtime.rs`. When the deleted method was the only non-test use, remove `RunType` from the `use auv_tracing_driver::trace` import list. Keep imports required by other runtime tests.

- [ ] **Step 3: Remove deleted module declarations**

  In `src/lib.rs`, delete:

  ```rust
  mod action_resolver_decision;
  pub mod ax_recognition;
  pub mod candidate_action_command;
  pub mod candidate_action_decision;
  pub mod candidate_promotion_recording;
  ```

  Keep:

  ```rust
  pub mod candidate_promotion;
  ```

- [ ] **Step 4: Run a compile check**

  Run:

  ```sh
  cargo check
  ```

  Expected: FAIL with references from `run_read`, `inspect`, or `inspect_server` to deleted modules. Those failures define the next tasks.

- [ ] **Step 5: Commit runtime and lib cleanup after later tasks compile**

  Do not commit this task until Tasks 4-6 compile. Then run:

  ```sh
  git add src/runtime.rs src/lib.rs
  git commit -m "refactor(runtime): remove archived candidate-action entrypoint"
  ```

## Task 4: Remove Run-Read Candidate-Action Compatibility

**Files:**
- Modify: `src/run_read.rs`

- [ ] **Step 1: Remove imports for deleted artifact types**

  Delete imports of:

  ```rust
  use crate::action_resolver_decision::ActionResolverDecision;
  use crate::candidate_action_decision::{
    CandidateActionDecisionArtifact, CandidateActionExecutionArtifact,
  };
  use crate::candidate_promotion::{CandidatePromotion, PromotionProjection, PromotionRefusal};
  use crate::candidate_promotion_recording::CandidatePromotionArtifact;
  ```

  After deleting the candidate-promotion lineage reader in this task, run `rg -n "CandidatePromotion|PromotionProjection|PromotionRefusal" src/run_read.rs`. Remove the `crate::candidate_promotion` import when the search shows no remaining active use.

- [ ] **Step 2: Remove candidate-action and promotion artifact role constants**

  Delete constants for:

  ```rust
  CANDIDATE_ACTION_DECISION_ARTIFACT_ROLE
  CANDIDATE_ACTION_EXECUTION_ARTIFACT_ROLE
  ```

  Delete the candidate-promotion artifact role constant if it is only used by removed candidate-promotion lineage extraction.

- [ ] **Step 3: Remove public run-read lineage structs and enums**

  Delete these read-side types:

  ```rust
  CandidatePromotionLineageStatus
  CandidatePromotionLineage
  CandidateActionDecisionLineageStatus
  CandidateActionDecisionLineage
  CandidateActionExecutionLineageStatus
  CandidateActionExecutionClosureState
  CandidateActionExecutionLineage
  ActionResolverDecisionProjection
  ActionTransitionLineageStatus
  ActionTransitionLineage
  LegacyCandidateActionExecutionArtifact
  ```

- [ ] **Step 4: Remove list/extract functions**

  Delete these functions:

  ```rust
  list_candidate_promotion_lineage
  list_candidate_action_decision_lineage
  list_candidate_action_execution_lineage
  list_action_transition_lineage
  extract_candidate_promotion_lineage
  extract_candidate_action_decision_lineage
  extract_candidate_action_execution_lineage
  extract_action_transition_lineage
  ```

- [ ] **Step 5: Remove helper functions tied only to deleted lineage**

  Delete helper functions whose only callers were removed in Steps 3-4:

  ```rust
  candidate_promotion_lineage_entry
  candidate_action_decision_lineage_entry
  malformed_candidate_action_decision_lineage
  candidate_action_execution_lineage_entry
  malformed_candidate_action_execution_lineage
  candidate_action_execution_closure_state
  read_candidate_action_execution_for_transition
  read_candidate_action_decision_artifact
  action_transition_verification_projection
  legacy_action_transition_verification_projection
  classify_action_transition_lineage
  action_transition_lineage_entry
  legacy_action_transition_lineage_entry
  malformed_action_transition_lineage
  classify_candidate_promotion_lineage
  classify_candidate_action_decision_lineage
  classify_candidate_action_execution_lineage
  candidate_action_side_effect_string
  candidate_action_execution_side_effect_string
  promotion_decision_summary
  promotion_refusal_string
  projection_kind
  consent_scope_string
  consent_provenance_string
  consent_grade_string
  consent_action_string
  ```

- [ ] **Step 6: Delete run-read tests for removed artifact compatibility**

  Delete tests named:

  ```text
  candidate_promotion_lineage_extracts_ready_and_error_states
  candidate_action_decision_lineage_extracts_decide_only_and_error_states
  candidate_action_execution_lineage_extracts_activation_only_and_error_states
  action_transition_lineage_surfaces_plan_delivery_mismatch_from_l8b
  action_transition_lineage_marks_legacy_missing_decision_as_partial
  ```

  Delete helper fixtures used only by those tests:

  ```rust
  candidate_action_decision_artifact
  candidate_action_execution_artifact
  candidate_action_execution_with_semantic_artifact
  candidate_action_activation_verification
  candidate_action_semantic_verification
  candidate_promotion_artifact
  ```

- [ ] **Step 7: Run run-read tests**

  Run:

  ```sh
  cargo test -p auv-cli run_read -- --nocapture
  ```

  Expected: PASS for remaining run-read tests and no references to candidate-action artifact readers.

- [ ] **Step 8: Commit run-read cleanup**

  ```sh
  git add src/run_read.rs
  git commit -m "refactor(run-read): remove candidate-action artifact readers"
  ```

## Task 5: Remove Text Inspect Candidate-Action Sections

**Files:**
- Modify: `src/inspect.rs`

- [ ] **Step 1: Remove imports for deleted run-read types**

  Delete imports of:

  ```rust
  CandidatePromotionLineage
  CandidatePromotionLineageStatus
  CandidateActionDecisionLineage
  CandidateActionDecisionLineageStatus
  CandidateActionExecutionLineage
  CandidateActionExecutionLineageStatus
  CandidateActionExecutionClosureState
  ActionTransitionLineage
  ActionTransitionLineageStatus
  ```

- [ ] **Step 2: Remove list wrapper functions**

  Delete wrappers:

  ```rust
  list_candidate_promotion_lineage
  list_candidate_action_decision_lineage
  list_candidate_action_execution_lineage
  list_action_transition_lineage
  ```

- [ ] **Step 3: Remove lineage collection from `inspect_run`**

  Remove local variables that call the deleted list functions. Remove their arguments from the function that renders the inspect text body.

- [ ] **Step 4: Remove render sections and status helpers**

  Delete render functions and helpers for:

  ```text
  candidate promotion lineage
  candidate action decision lineage
  candidate action execution lineage
  action transition lineage
  ```

  Delete status rendering helpers:

  ```rust
  render_candidate_promotion_status
  render_candidate_action_decision_status
  render_candidate_action_execution_status
  render_candidate_action_execution_closure_state
  render_action_transition_status
  ```

- [ ] **Step 5: Update inspect snapshot-style tests**

  Remove expected output lines containing:

  ```text
  Candidate promotion lineage
  Candidate action decision lineage
  Candidate action execution lineage
  Action transition lineage
  candidate-action
  candidate_promotion
  ```

- [ ] **Step 6: Run inspect tests**

  Run:

  ```sh
  cargo test -p auv-cli inspect -- --nocapture
  ```

  Expected: PASS with inspect output no longer mentioning candidate-action lineage sections.

- [ ] **Step 7: Commit text inspect cleanup**

  ```sh
  git add src/inspect.rs
  git commit -m "refactor(inspect): remove candidate-action lineage output"
  ```

## Task 6: Remove Inspect Server Candidate-Action Fields and Viewer Panels

**Files:**
- Modify: `src/inspect_server/mod.rs`
- Modify: `src/inspect_server_viewer.html`

- [ ] **Step 1: Remove server imports for deleted lineage types**

  In `src/inspect_server/mod.rs`, delete imports of `CandidatePromotionLineage` and candidate-action artifact types. Remove test-only imports of `ActionResolverDecision`, `CandidateActionDecisionArtifact`, `CandidateActionExecutionArtifact`, and `CandidatePromotionArtifact`.

- [ ] **Step 2: Remove candidate-action JSON extraction**

  In the run serialization path, delete calls to:

  ```rust
  extract_candidate_promotion_lineage
  extract_candidate_action_decision_lineage
  extract_candidate_action_execution_lineage
  extract_action_transition_lineage
  ```

  Remove corresponding fields from the response struct:

  ```rust
  candidate_promotion_lineage
  candidate_action_decision_lineage
  candidate_action_execution_lineage
  action_transition_lineage
  ```

- [ ] **Step 3: Remove server tests for deleted JSON fields**

  Delete assertions that access:

  ```text
  run["candidate_promotion_lineage"]
  run["candidate_action_decision_lineage"]
  run["candidate_action_execution_lineage"]
  run["action_transition_lineage"]
  ```

  Delete fixture builders that exist only to create candidate-action or candidate-promotion artifacts for those assertions.

- [ ] **Step 4: Remove viewer action-transition panel code**

  In `src/inspect_server_viewer.html`, delete functions:

  ```javascript
  clearActionTransitionLineage
  hasActionTransitionLineage
  renderActionTransitionLineageCard
  renderActionTransitionLineage
  selfTestActionTransitionLineage
  ```

  Remove calls to those functions from active-run render, clear, and self-test flows.

- [ ] **Step 5: Remove viewer references to deleted JSON fields**

  Remove JavaScript references to:

  ```text
  action_transition_lineage
  candidate_action_decision_lineage
  candidate_action_execution_lineage
  candidate_promotion_lineage
  ```

  Keep any existing test that asserts the viewer must not reference removed fields, and extend it to cover all four field names.

- [ ] **Step 6: Run inspect server tests**

  Run:

  ```sh
  cargo test -p auv-cli inspect_server -- --nocapture
  ```

  Expected: PASS with no server JSON or viewer references to candidate-action fields.

- [ ] **Step 7: Commit inspect server cleanup**

  ```sh
  git add src/inspect_server/mod.rs src/inspect_server_viewer.html
  git commit -m "refactor(inspect-server): remove candidate-action panels"
  ```

## Task 7: Delete Archived Candidate-Action Support Modules

**Files:**
- Delete: `src/candidate_action_command.rs`
- Delete: `src/candidate_action_decision.rs`
- Delete: `src/ax_recognition.rs`
- Delete: `src/candidate_promotion_recording.rs`
- Delete: `src/action_resolver_decision.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Verify remaining production references**

  Run:

  ```sh
  rg -n "candidate_action_command|candidate_action_decision|candidate_promotion_recording|ax_recognition|action_resolver_decision|CandidateAction|candidate-action|candidate_action|ActionResolverDecision" src crates
  ```

  Expected: only comments or docs unrelated to production code remain. If production references remain, finish Tasks 1-6 before continuing.

- [ ] **Step 2: Delete the support modules**

  Run:

  ```sh
  git rm src/candidate_action_command.rs src/candidate_action_decision.rs src/ax_recognition.rs src/candidate_promotion_recording.rs src/action_resolver_decision.rs
  ```

- [ ] **Step 3: Verify `src/lib.rs` exposes only retained modules**

  Ensure `src/lib.rs` contains:

  ```rust
  pub mod candidate_promotion;
  ```

  Ensure it does not contain:

  ```rust
  mod action_resolver_decision;
  pub mod ax_recognition;
  pub mod candidate_action_command;
  pub mod candidate_action_decision;
  pub mod candidate_promotion_recording;
  ```

- [ ] **Step 4: Run compile check**

  Run:

  ```sh
  cargo check
  ```

  Expected: PASS, or FAIL only on doc/test references that still mention deleted symbols. Remove those references in the same task before committing.

- [ ] **Step 5: Commit module deletion**

  ```sh
  git add src/lib.rs
  git commit -m "refactor: delete archived candidate-action modules"
  ```

## Task 8: Clean Documentation References and Index

**Files:**
- Modify: `docs/ai/references/INDEX.md`
- Modify: `docs/TERMS_AND_CONCEPTS.md` after checking whether it claims the removed path is current

- [ ] **Step 1: Search for active-roadmap references to candidate-action**

  Run:

  ```sh
  rg -n "candidate-action|candidate_action|ActionResolverDecision|candidate_promotion_recording|ax_recognition" docs/ai/references docs/TERMS_AND_CONCEPTS.md
  ```

- [ ] **Step 2: Update the references index**

  If `docs/ai/references/INDEX.md` manually lists current references, add both removal docs:

  ```markdown
  - [Archived Candidate-Action Removal Spec](2026-07-07-archived-candidate-action-removal-spec.md)
  - [Archived Candidate-Action Removal Implementation Plan](2026-07-07-archived-candidate-action-removal-plan.md)
  ```

  Do not rewrite unrelated index entries.

- [ ] **Step 3: Leave historical references intact**

  Do not edit old handoff documents only because they mention candidate-action. Historical docs may keep historical names.

- [ ] **Step 4: Update current terminology only if it claims the removed path exists**

  If `docs/TERMS_AND_CONCEPTS.md` claims candidate-action decision/execution artifacts are current runtime surfaces, change that sentence to state:

  ```markdown
  The archived candidate-action decision/execution artifacts were removed from the current runtime surface on 2026-07-07. Historical handoff documents may still mention them as prior evidence.
  ```

- [ ] **Step 5: Commit docs cleanup**

  ```sh
  git add docs/ai/references/INDEX.md docs/TERMS_AND_CONCEPTS.md docs/ai/references/runtime/2026-07-07-archived-candidate-action-removal-plan.md
  git commit -m "docs: plan archived candidate-action removal"
  ```

## Task 9: Final Verification

**Files:**
- No planned source edits

- [ ] **Step 1: Check for removed symbols**

  Run:

  ```sh
  rg -n "candidate_action_command|candidate_action_decision|candidate_promotion_recording|ax_recognition|action_resolver_decision|CandidateActionRun|candidate_action_run|candidate-action run|ActionResolverDecision" src crates
  ```

  Expected: no matches in production code. Matches in archived docs are acceptable only outside `src` and `crates`.

- [ ] **Step 2: Confirm retained promotion seam still tests**

  Run:

  ```sh
  cargo test -p auv-cli candidate_promotion:: -- --nocapture
  ```

  Expected: PASS. This confirms `src/candidate_promotion.rs` remains valid after deleting the archived vertical.

- [ ] **Step 3: Run required validation commands**

  Run:

  ```sh
  cargo fmt --check
  cargo check
  cargo test
  git diff --check
  cargo run --quiet -- invoke --help
  ```

  Expected: all commands PASS.

- [ ] **Step 4: Confirm CLI help no longer advertises candidate-action**

  Run:

  ```sh
  cargo run --quiet -- --help | rg "candidate-action|candidate_action"
  ```

  Expected: no output and exit code 1 from `rg`.

- [ ] **Step 5: Summarize the final diff**

  Run:

  ```sh
  git status --short
  git diff --stat
  ```

  Expected: source deletions for archived candidate-action modules, source edits in CLI/MCP/runtime/read-side/inspect surfaces, and the new plan document.

## Self-Review Notes

- Spec coverage: Tasks 1-3 remove MCP, CLI, and runtime entrypoints. Tasks 4-6 remove read-side and inspect compatibility. Task 7 deletes now-unreferenced support modules. Task 8 handles reference docs. Task 9 validates the removal.
- Placeholder scan: no task uses open-ended implementation placeholders; each task names concrete files, symbols, commands, and expected outcomes.
- Type consistency: deleted symbols are consistently named with their current Rust identifiers. `src/candidate_promotion.rs` is explicitly retained throughout the plan.
