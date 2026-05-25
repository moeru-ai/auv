// File: src/skill/validate/manifest.rs
//! Skill recipe manifest validation rules.
//!
//! Performs static checks over skill recipes: identity/target, strategy taxonomy
//! parsing, declared inputs, step references to the command catalog, disturbance
//! budgets, and verification expectations.

use serde_json::Value;

use crate::catalog::default_command_catalog;
use crate::model::{AuvResult, DisturbanceClass};

use super::super::{SkillManifest, SkillStep, SkillStrategyTaxonomy, step_id, stringify_value};
use super::inline_hook::validate_skill_inline_hooks;

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

pub(super) fn validate_step_disturbance_against_command(
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
    if !ALLOWED_CATEGORIES.contains(&category) {
      return Err(format!(
        "skill {} step {step_label} mainline_exemption.category {category:?} is not one of {ALLOWED_CATEGORIES:?} (rule 1)",
        manifest.recipe_id,
      ));
    }
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
