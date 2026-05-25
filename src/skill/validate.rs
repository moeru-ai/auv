use std::collections::BTreeMap;

use serde_json::Value;

use crate::catalog::default_command_catalog;
use crate::model::{AuvResult, DisturbanceClass};

use super::{
  SkillCaseMatrix, SkillInlineHook, SkillInvocation, SkillManifest, SkillStep,
  SkillStrategyTaxonomy, SkillVerification, step_id, stringify_value,
};

pub(crate) fn validate_skill_manifest(manifest: &SkillManifest) -> AuvResult<()> {
  let command_catalog = default_command_catalog();
  validate_skill_manifest_with_commands(manifest, command_catalog.all())
}

pub(crate) fn validate_skill_manifest_with_commands(
  manifest: &SkillManifest,
  command_catalog: &[crate::model::CommandSpec],
) -> AuvResult<()> {
  validate_skill_identity(manifest)?;
  validate_skill_target_app(manifest)?;
  validate_skill_strategy(manifest)?;
  validate_skill_inputs(manifest)?;
  validate_skill_steps(manifest, command_catalog)?;
  validate_skill_inline_hooks(manifest, command_catalog)?;
  validate_skill_disturbance_budget(manifest)?;
  validate_skill_mainline_compliance(manifest)?;
  validate_skill_verification(manifest)?;
  Ok(())
}

const SCROLL_SCAN_INLINE_HOOK_STAGES: &[&str] = &[
  "per_page_after_observe",
  "per_list_item_candidate",
  "on_stop_candidate",
];

const SCROLL_SCAN_HOOK_RETURN_SCHEMA: &str = "auv.scan.hook_decision.v0";

pub(crate) fn build_inline_scan_hook_manifest(
  parent: &SkillManifest,
  hook_name: &str,
) -> AuvResult<Option<SkillManifest>> {
  let Some(hook) = parent.hooks.get(hook_name) else {
    return Ok(None);
  };
  Ok(Some(synthesize_inline_scan_hook_manifest(
    parent, hook_name, hook,
  )?))
}

fn synthesize_inline_scan_hook_manifest(
  parent: &SkillManifest,
  hook_name: &str,
  hook: &SkillInlineHook,
) -> AuvResult<SkillManifest> {
  validate_inline_scan_hook_contract(&parent.recipe_id, hook_name, hook)?;
  Ok(SkillManifest {
    recipe_id: format!("{}.hook.{}", parent.recipe_id, hook_name),
    version: parent.version.clone(),
    status: "inline-sub-recipe".to_string(),
    platform: parent.platform.clone(),
    target_app: parent.target_app.clone(),
    strategy: parent.strategy.clone(),
    invocation: SkillInvocation {
      kind: "sub_recipe".to_string(),
      host: "scroll_scan".to_string(),
      stage: hook_name.to_string(),
      context_schema: hook.input_schema.clone(),
      return_schema: hook.return_schema.clone(),
    },
    objective: format!(
      "Inline scroll-scan hook {hook_name} synthesized from {}",
      parent.recipe_id
    ),
    inputs: BTreeMap::new(),
    preconditions: vec![format!(
      "Synthetic inline hook derived from parent recipe {}.",
      parent.recipe_id
    )],
    disturbance_policy: parent.disturbance_policy.clone(),
    steps: hook.steps.clone(),
    verification: SkillVerification {
      expected_signals: vec!["last.scan.hook.decision".to_string()],
      success_criteria: vec![format!(
        "Inline hook {hook_name} may emit last.scan.hook.* signals for scroll-scan orchestration."
      )],
      non_goals: vec![
        "This synthetic inline hook manifest exists only to reuse the shared recipe step runner."
          .to_string(),
      ],
    },
    hooks: BTreeMap::new(),
    known_limits: BTreeMap::from([(
      "context".to_string(),
      "input_schema is explicit, but current scroll-scan hook execution still injects scalar scan.* overrides rather than one typed context object.".to_string(),
    )]),
  })
}

fn validate_skill_inline_hooks(
  manifest: &SkillManifest,
  command_catalog: &[crate::model::CommandSpec],
) -> AuvResult<()> {
  for hook_name in manifest.hooks.keys() {
    let inline_manifest = build_inline_scan_hook_manifest(manifest, hook_name)?.expect(
      "hook key came from manifest.hooks; inline scan hook synthesis should always return Some",
    );
    validate_skill_manifest_with_commands(&inline_manifest, command_catalog).map_err(|error| {
      format!(
        "skill {} inline hook {} is invalid: {error}",
        manifest.recipe_id, hook_name
      )
    })?;
  }
  Ok(())
}

fn validate_inline_scan_hook_contract(
  recipe_id: &str,
  hook_name: &str,
  hook: &SkillInlineHook,
) -> AuvResult<()> {
  if !SCROLL_SCAN_INLINE_HOOK_STAGES.contains(&hook_name) {
    return Err(format!(
      "skill {} declares unsupported inline hook {}; allowed stages: {}",
      recipe_id,
      hook_name,
      SCROLL_SCAN_INLINE_HOOK_STAGES.join(", ")
    ));
  }
  if hook.input_schema.trim().is_empty() {
    return Err(format!(
      "skill {} inline hook {} must declare a non-empty input_schema",
      recipe_id, hook_name
    ));
  }
  if hook.return_schema.trim().is_empty() {
    return Err(format!(
      "skill {} inline hook {} must declare a non-empty return_schema",
      recipe_id, hook_name
    ));
  }
  if hook.return_schema != SCROLL_SCAN_HOOK_RETURN_SCHEMA {
    return Err(format!(
      "skill {} inline hook {} return_schema {} does not match required {}",
      recipe_id, hook_name, hook.return_schema, SCROLL_SCAN_HOOK_RETURN_SCHEMA
    ));
  }
  if hook.steps.is_empty() {
    return Err(format!(
      "skill {} inline hook {} must declare at least one step",
      recipe_id, hook_name
    ));
  }
  Ok(())
}

/// Phase 3 Rule 1 from
/// `docs/ai/references/2026-05-22-phase-3-mainline-acceptance.md`:
/// `debug.smartPress` is a discovery vehicle, not a production
/// default. Allowed in `macos.demo.*` recipes (presentation
/// surface) or in any recipe where the step declares an explicit
/// `mainline_exemption: { reason, category }` opt-in. The exemption
/// must be a non-empty reason and a known category
/// (`discovery` | `experiment` | `reverification`); the audit doc
/// catalogues every active exemption so the opt-out cannot become a
/// silent default.
pub(crate) fn validate_skill_mainline_compliance(manifest: &SkillManifest) -> AuvResult<()> {
  const RESTRICTED_COMMANDS: &[&str] = &["debug.smartPress"];
  const DEMO_PREFIX: &str = "macos.demo.";
  const ALLOWED_CATEGORIES: &[&str] = &["discovery", "experiment", "reverification"];

  let in_demo_namespace = manifest.recipe_id.starts_with(DEMO_PREFIX);

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_label = step_id(step, index);
    let is_restricted = RESTRICTED_COMMANDS
      .iter()
      .any(|cmd| *cmd == step.command_id);
    if !is_restricted {
      continue;
    }
    if in_demo_namespace {
      continue;
    }

    let Some(exemption) = step.mainline_exemption.as_ref() else {
      return Err(format!(
        "skill {} step {step_label} uses {} outside the macos.demo.* namespace without a step-level mainline_exemption. \
         See docs/ai/references/2026-05-22-phase-3-mainline-acceptance.md rule 1 — either move the recipe to macos.demo.* or declare an explicit exemption with reason + category.",
        manifest.recipe_id, step.command_id,
      ));
    };

    let reason = exemption.reason.trim();
    if reason.is_empty() {
      return Err(format!(
        "skill {} step {step_label} mainline_exemption.reason must be non-empty (rule 1)",
        manifest.recipe_id,
      ));
    }
    let category = exemption.category.trim();
    if !ALLOWED_CATEGORIES.iter().any(|c| *c == category) {
      return Err(format!(
        "skill {} step {step_label} mainline_exemption.category {category:?} is not one of {ALLOWED_CATEGORIES:?} (rule 1)",
        manifest.recipe_id,
      ));
    }
  }
  Ok(())
}

/// Phase 3 Rule 2 from
/// `docs/ai/references/2026-05-22-phase-3-mainline-acceptance.md`:
/// any recipe that uses `debug.smartPress` in any step cannot have
/// `status == "validated"` cases. The promotion path for a smart-
/// press recipe is candidate -> evidence -> spawn a non-smart child
/// fixed to whichever strategy actually works.
pub(crate) fn validate_smart_press_case_status(
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
) -> AuvResult<()> {
  let uses_smart_press = manifest
    .steps
    .iter()
    .any(|step| step.command_id == "debug.smartPress");
  if !uses_smart_press {
    return Ok(());
  }
  for case in &matrix.cases {
    if case.status.trim() == "validated" {
      return Err(format!(
        "case matrix {} case {} is status=validated, but recipe {} uses debug.smartPress (rule 2 — smart-press recipes cannot host validated cases; promote to a non-smart child recipe instead). \
         See docs/ai/references/2026-05-22-phase-3-mainline-acceptance.md.",
        matrix.skill_id, case.case_id, manifest.recipe_id,
      ));
    }
  }
  Ok(())
}

/// Enforce that every step's declared `disturbance.max` and class set
/// respects the recipe's `disturbance_policy.max_disturbance` budget and
/// the policy's `declared_classes` list.
///
/// Phase 3 #4: this turns the recipe budget from documentation into a
/// load-time constraint so `skill list`, `skill cases run --dry-run`,
/// and bundle verify all catch violations before any driver call.
pub(crate) fn validate_skill_disturbance_budget(manifest: &SkillManifest) -> AuvResult<()> {
  let recipe_max = if manifest
    .disturbance_policy
    .max_disturbance
    .trim()
    .is_empty()
  {
    return Err(format!(
      "skill {} must declare disturbance_policy.max_disturbance",
      manifest.recipe_id
    ));
  } else {
    DisturbanceClass::parse(&manifest.disturbance_policy.max_disturbance).map_err(|error| {
      format!(
        "skill {} has invalid disturbance_policy.max_disturbance {}: {error}",
        manifest.recipe_id, manifest.disturbance_policy.max_disturbance
      )
    })?
  };

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_label = step_id(step, index);
    let step_max = parse_step_max(step).map_err(|error| {
      format!(
        "skill {} step {step_label} has invalid disturbance.max {}: {error}",
        manifest.recipe_id, step.disturbance.max
      )
    })?;
    if step_max > recipe_max {
      return Err(format!(
        "skill {} step {step_label} declares disturbance.max {} above recipe budget {}",
        manifest.recipe_id,
        step_max.as_str(),
        recipe_max.as_str()
      ));
    }
    for class in &step.disturbance.classes {
      let parsed = DisturbanceClass::parse(class).map_err(|error| {
        format!(
          "skill {} step {step_label} has invalid disturbance class {}: {error}",
          manifest.recipe_id, class
        )
      })?;
      if parsed > step_max {
        return Err(format!(
          "skill {} step {step_label} declares class {} above its own max {}",
          manifest.recipe_id,
          class,
          step_max.as_str()
        ));
      }
      if !manifest.disturbance_policy.declared_classes.is_empty()
        && !manifest
          .disturbance_policy
          .declared_classes
          .iter()
          .any(|declared| declared == class)
      {
        return Err(format!(
          "skill {} step {step_label} uses class {} not declared in disturbance_policy.declared_classes",
          manifest.recipe_id, class
        ));
      }
    }
  }

  Ok(())
}

fn validate_skill_identity(manifest: &SkillManifest) -> AuvResult<()> {
  if manifest.recipe_id.trim().is_empty() {
    return Err("skill manifest recipe_id must not be empty".to_string());
  }
  if manifest.version.trim().is_empty() {
    return Err(format!(
      "skill {} must declare a non-empty version",
      manifest.recipe_id
    ));
  }
  semver::Version::parse(&manifest.version).map_err(|error| {
    format!(
      "skill {} has invalid version {}: {error}",
      manifest.recipe_id, manifest.version
    )
  })?;
  if manifest.objective.trim().is_empty() {
    return Err(format!(
      "skill {} must declare a non-empty objective",
      manifest.recipe_id
    ));
  }
  Ok(())
}

fn validate_skill_target_app(manifest: &SkillManifest) -> AuvResult<()> {
  if manifest.target_app.bundle_id.trim().is_empty() {
    return Err(format!(
      "skill {} must declare a non-empty target_app.bundle_id",
      manifest.recipe_id
    ));
  }
  if manifest.target_app.display_mode.trim().is_empty() {
    return Err(format!(
      "skill {} must declare a non-empty target_app.display_mode",
      manifest.recipe_id
    ));
  }
  Ok(())
}

fn validate_skill_strategy(manifest: &SkillManifest) -> AuvResult<()> {
  validate_skill_strategy_field(manifest, "family", &manifest.strategy.family)?;
  validate_skill_strategy_field(manifest, "grounding", &manifest.strategy.grounding)?;
  validate_skill_strategy_field(manifest, "activation", &manifest.strategy.activation)?;
  validate_skill_strategy_field(
    manifest,
    "verificationContract",
    &manifest.strategy.verification_contract,
  )?;
  let taxonomy = manifest
    .strategy
    .taxonomy()
    .map_err(|error| format!("skill {} {}", manifest.recipe_id, error))?;
  if !taxonomy.is_allowed() {
    return Err(format!(
      "skill {} strategy combination {} is unsupported; allowed combinations: {}",
      manifest.recipe_id,
      taxonomy.taxonomy_id(),
      SkillStrategyTaxonomy::allowed_taxonomy_ids()
    ));
  }
  Ok(())
}

fn validate_skill_strategy_field(
  manifest: &SkillManifest,
  field_name: &str,
  value: &str,
) -> AuvResult<()> {
  if value.trim().is_empty() {
    return Err(format!(
      "skill {} must declare a non-empty strategy.{}",
      manifest.recipe_id, field_name
    ));
  }
  if !value
    .chars()
    .all(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_' | '.'))
  {
    return Err(format!(
      "skill {} strategy.{} {} contains unsupported characters",
      manifest.recipe_id, field_name, value
    ));
  }
  Ok(())
}

fn validate_skill_inputs(manifest: &SkillManifest) -> AuvResult<()> {
  for (key, spec) in &manifest.inputs {
    if key.trim().is_empty() {
      return Err(format!(
        "skill {} has an input with an empty key",
        manifest.recipe_id
      ));
    }
    if spec.kind.trim().is_empty() {
      return Err(format!(
        "skill {} input {} must declare a non-empty type",
        manifest.recipe_id, key
      ));
    }
    if let Some(default) = &spec.default {
      stringify_value(default).map_err(|error| {
        format!(
          "skill {} input {} has an invalid default value: {error}",
          manifest.recipe_id, key
        )
      })?;
    }
  }
  Ok(())
}

fn validate_skill_steps(
  manifest: &SkillManifest,
  command_catalog: &[crate::model::CommandSpec],
) -> AuvResult<()> {
  if manifest.steps.is_empty() {
    return Err(format!(
      "skill {} must declare at least one step",
      manifest.recipe_id
    ));
  }

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_label = if step.id.trim().is_empty() {
      format!("step-{}", index + 1)
    } else {
      step.id.clone()
    };
    if step.command_id.trim().is_empty() {
      return Err(format!(
        "skill {} step {} must declare a non-empty command_id",
        manifest.recipe_id, step_label
      ));
    }
    let Some(command) = command_catalog
      .iter()
      .find(|command| command.id == step.command_id)
    else {
      return Err(format!(
        "skill {} step {} references unknown command_id {}",
        manifest.recipe_id, step_label, step.command_id
      ));
    };
    if step.disturbance.max.trim().is_empty() {
      return Err(format!(
        "skill {} step {} must declare disturbance.max",
        manifest.recipe_id, step_label
      ));
    }
    if step.disturbance.classes.is_empty() {
      return Err(format!(
        "skill {} step {} must declare disturbance.classes",
        manifest.recipe_id, step_label
      ));
    }
    for class in &step.disturbance.classes {
      DisturbanceClass::parse(class).map_err(|error| {
        format!(
          "skill {} step {} has invalid disturbance class {}: {error}",
          manifest.recipe_id, step_label, class
        )
      })?;
    }
    validate_step_arg_implied_disturbance(&manifest.recipe_id, &step_label, step)?;
    validate_step_disturbance_against_command(&manifest.recipe_id, &step_label, step, command)?;

    for key in step.args.keys() {
      if key.trim().is_empty() {
        return Err(format!(
          "skill {} step {} has an empty arg key",
          manifest.recipe_id, step_label
        ));
      }
    }
  }
  Ok(())
}

fn validate_step_arg_implied_disturbance(
  recipe_id: &str,
  step_label: &str,
  step: &SkillStep,
) -> AuvResult<()> {
  if step_arg_bool(step, "activate_target_before_capture") {
    let declares_foreground = step
      .disturbance
      .classes
      .iter()
      .any(|class| class == DisturbanceClass::ForegroundApp.as_str());
    if !declares_foreground {
      return Err(format!(
        "skill {} step {} sets activate_target_before_capture=true but does not declare disturbance class foreground_app",
        recipe_id, step_label
      ));
    }

    let step_max = parse_step_max(step).map_err(|error| {
      format!(
        "skill {} step {} has invalid disturbance.max {}: {error}",
        recipe_id, step_label, step.disturbance.max
      )
    })?;
    if step_max < DisturbanceClass::ForegroundApp {
      return Err(format!(
        "skill {} step {} sets activate_target_before_capture=true but disturbance.max {} is below foreground_app",
        recipe_id,
        step_label,
        step_max.as_str()
      ));
    }
  }

  Ok(())
}

fn step_arg_bool(step: &SkillStep, key: &str) -> bool {
  match step.args.get(key) {
    Some(Value::Bool(value)) => *value,
    Some(Value::String(value)) => value.trim().eq_ignore_ascii_case("true"),
    _ => false,
  }
}

fn validate_step_disturbance_against_command(
  recipe_id: &str,
  step_label: &str,
  step: &SkillStep,
  command: &crate::model::CommandSpec,
) -> AuvResult<()> {
  let step_max = parse_step_max(step).map_err(|error| {
    format!(
      "skill {} step {} has invalid disturbance.max {}: {error}",
      recipe_id, step_label, step.disturbance.max
    )
  })?;
  if step_max > command.max_disturbance {
    return Err(format!(
      "skill {} step {} uses disturbance.max {} above command {} max {}",
      recipe_id,
      step_label,
      step_max.as_str(),
      command.id,
      command.max_disturbance.as_str()
    ));
  }

  for class in &step.disturbance.classes {
    let parsed = DisturbanceClass::parse(class).map_err(|error| {
      format!(
        "skill {} step {} has invalid disturbance class {}: {error}",
        recipe_id, step_label, class
      )
    })?;
    if !command.disturbance_classes.contains(&parsed) {
      return Err(format!(
        "skill {} step {} uses disturbance class {} not declared by command {}",
        recipe_id, step_label, class, command.id
      ));
    }
  }

  Ok(())
}

fn validate_skill_verification(manifest: &SkillManifest) -> AuvResult<()> {
  if manifest.verification.expected_signals.is_empty() {
    return Err(format!(
      "skill {} must declare verification.expected_signals",
      manifest.recipe_id
    ));
  }
  if manifest.verification.success_criteria.is_empty() {
    return Err(format!(
      "skill {} must declare verification.success_criteria",
      manifest.recipe_id
    ));
  }
  Ok(())
}

pub(crate) fn validate_disturbance_policy(
  manifest: &SkillManifest,
  requested_max: Option<DisturbanceClass>,
) -> AuvResult<DisturbanceClass> {
  let recipe_max = if manifest.disturbance_policy.max_disturbance.is_empty() {
    DisturbanceClass::Pointer
  } else {
    DisturbanceClass::parse(&manifest.disturbance_policy.max_disturbance)?
  };
  let active_max = requested_max.unwrap_or(recipe_max);
  if active_max > recipe_max {
    return Err(format!(
      "requested max disturbance {:?} exceeds skill max disturbance {:?}",
      active_max.as_str(),
      recipe_max.as_str()
    ));
  }

  for step in &manifest.steps {
    let step_id = step_id(step, 0);
    let step_max = parse_step_max(step)?;
    if step_max > active_max {
      return Err(format!(
        "step {step_id:?} requires disturbance {:?}, above allowed max {:?}",
        step_max.as_str(),
        active_max.as_str()
      ));
    }
    for class in &step.disturbance.classes {
      let parsed = DisturbanceClass::parse(class)?;
      if parsed > step_max {
        return Err(format!(
          "step {step_id:?} declares class {class:?} above its own max {:?}",
          step_max.as_str()
        ));
      }
      if !manifest.disturbance_policy.declared_classes.is_empty()
        && !manifest
          .disturbance_policy
          .declared_classes
          .iter()
          .any(|declared| declared == class)
      {
        return Err(format!(
          "step {step_id:?} uses class {class:?} not declared by skill policy"
        ));
      }
    }
  }

  Ok(active_max)
}

pub(crate) fn validate_case_matrix_manifest(matrix: &SkillCaseMatrix) -> AuvResult<()> {
  if matrix.skill_id.trim().is_empty() {
    return Err("case matrix skill_id must not be empty".to_string());
  }
  if matrix.version.trim().is_empty() {
    return Err(format!(
      "case matrix {} must declare a non-empty version",
      matrix.skill_id
    ));
  }
  semver::Version::parse(&matrix.version).map_err(|error| {
    format!(
      "case matrix {} has invalid version {}: {error}",
      matrix.skill_id, matrix.version
    )
  })?;
  if matrix.status.trim().is_empty() {
    return Err(format!(
      "case matrix {} must declare a non-empty status",
      matrix.skill_id
    ));
  }
  if matrix.cases.is_empty() {
    return Err(format!(
      "case matrix {} must declare at least one case",
      matrix.skill_id
    ));
  }

  let mut seen_case_ids = std::collections::BTreeSet::new();
  for (index, case) in matrix.cases.iter().enumerate() {
    let case_label = if case.case_id.trim().is_empty() {
      format!("case-{}", index + 1)
    } else {
      case.case_id.clone()
    };
    if case.case_id.trim().is_empty() {
      return Err(format!(
        "case matrix {} has a case with an empty case_id",
        matrix.skill_id
      ));
    }
    if !seen_case_ids.insert(case.case_id.clone()) {
      return Err(format!(
        "case matrix {} contains duplicate case_id {}",
        matrix.skill_id, case.case_id
      ));
    }
    if case.status.trim().is_empty() {
      return Err(format!(
        "case matrix {} case {} must declare a non-empty status",
        matrix.skill_id, case_label
      ));
    }
    if case.disturbance.trim().is_empty() {
      return Err(format!(
        "case matrix {} case {} must declare a non-empty disturbance",
        matrix.skill_id, case_label
      ));
    }
    DisturbanceClass::parse(&case.disturbance).map_err(|error| {
      format!(
        "case matrix {} case {} has invalid disturbance {}: {error}",
        matrix.skill_id, case_label, case.disturbance
      )
    })?;

    for key in case.inputs.keys() {
      if key.trim().is_empty() {
        return Err(format!(
          "case matrix {} case {} has an empty input key",
          matrix.skill_id, case_label
        ));
      }
    }
  }

  Ok(())
}

pub(crate) fn validate_case_matrix_against_skill(
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
) -> AuvResult<()> {
  if matrix.skill_id != manifest.recipe_id {
    return Err(format!(
      "case matrix {} does not match skill {}",
      matrix.skill_id, manifest.recipe_id
    ));
  }

  let recipe_max = if manifest.disturbance_policy.max_disturbance.is_empty() {
    DisturbanceClass::Pointer
  } else {
    DisturbanceClass::parse(&manifest.disturbance_policy.max_disturbance).map_err(|error| {
      format!(
        "skill {} has invalid disturbance_policy.max_disturbance {}: {error}",
        manifest.recipe_id, manifest.disturbance_policy.max_disturbance
      )
    })?
  };

  for case in &matrix.cases {
    let case_disturbance = DisturbanceClass::parse(&case.disturbance).map_err(|error| {
      format!(
        "case matrix {} case {} has invalid disturbance {}: {error}",
        matrix.skill_id, case.case_id, case.disturbance
      )
    })?;
    if case_disturbance > recipe_max {
      return Err(format!(
        "case matrix {} case {} uses disturbance {} above skill max {}",
        matrix.skill_id,
        case.case_id,
        case_disturbance.as_str(),
        recipe_max.as_str()
      ));
    }

    for key in case.inputs.keys() {
      if !manifest.inputs.contains_key(key) {
        return Err(format!(
          "case matrix {} case {} references unknown input {}",
          matrix.skill_id, case.case_id, key
        ));
      }
    }

    for (input_key, spec) in &manifest.inputs {
      if spec.default.is_none() && !case.inputs.contains_key(input_key) {
        return Err(format!(
          "case matrix {} case {} is missing required input {}",
          matrix.skill_id, case.case_id, input_key
        ));
      }
    }
  }

  validate_smart_press_case_status(manifest, matrix)?;

  Ok(())
}

pub(crate) fn validate_case_matrix_against_skill_with_commands(
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  command_catalog: &[crate::model::CommandSpec],
) -> AuvResult<()> {
  validate_case_matrix_against_skill(manifest, matrix)?;

  for step in &manifest.steps {
    let Some(command) = command_catalog
      .iter()
      .find(|command| command.id == step.command_id)
    else {
      return Err(format!(
        "skill {} step {} references unknown command_id {}",
        manifest.recipe_id,
        if step.id.trim().is_empty() {
          &step.command_id
        } else {
          &step.id
        },
        step.command_id
      ));
    };
    let step_label = if step.id.trim().is_empty() {
      step.command_id.clone()
    } else {
      step.id.clone()
    };
    validate_step_disturbance_against_command(&manifest.recipe_id, &step_label, step, command)?;
  }

  Ok(())
}

pub(crate) fn parse_step_max(step: &SkillStep) -> AuvResult<DisturbanceClass> {
  if step.disturbance.max.is_empty() {
    Ok(DisturbanceClass::None)
  } else {
    DisturbanceClass::parse(&step.disturbance.max)
  }
}
