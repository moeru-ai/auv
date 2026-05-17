use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::driver::{clear_stale_lock_file, describe_lock_owner};
use crate::model::{AuvResult, DisturbanceClass, ExecutionTarget, InvokeRequest, InvokeResult};
use crate::runtime::Runtime;

#[derive(Clone, Debug, Deserialize)]
pub struct SkillManifest {
  pub recipe_id: String,
  pub version: String,
  #[serde(default)]
  pub status: String,
  #[serde(default)]
  pub platform: String,
  pub target_app: SkillTargetApp,
  pub objective: String,
  #[serde(default)]
  pub inputs: BTreeMap<String, SkillInputSpec>,
  #[serde(default)]
  pub preconditions: Vec<String>,
  #[serde(default)]
  pub disturbance_policy: SkillDisturbancePolicy,
  #[serde(default)]
  pub steps: Vec<SkillStep>,
  #[serde(default)]
  pub verification: SkillVerification,
  #[serde(default)]
  pub known_limits: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillTargetApp {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub bundle_id: String,
  #[serde(default)]
  pub display_mode: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillInputSpec {
  #[serde(rename = "type", default)]
  pub kind: String,
  #[serde(default)]
  pub default: Option<Value>,
  #[serde(default)]
  pub note: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillDisturbancePolicy {
  #[serde(default)]
  pub max_disturbance: String,
  #[serde(default)]
  pub declared_classes: Vec<String>,
  #[serde(default)]
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillStep {
  #[serde(default)]
  pub id: String,
  #[serde(default)]
  pub command_id: String,
  #[serde(default)]
  pub disturbance: SkillStepDisturbance,
  #[serde(default)]
  pub expect: SkillStepExpect,
  #[serde(default)]
  pub args: BTreeMap<String, Value>,
  #[serde(default)]
  pub purpose: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillStepDisturbance {
  #[serde(default)]
  pub classes: Vec<String>,
  #[serde(default)]
  pub max: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillStepExpect {
  #[serde(default)]
  pub output_must_contain: Vec<String>,
  #[serde(default)]
  pub output_must_not_contain: Vec<String>,
  #[serde(default)]
  pub artifact_count_at_least: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillVerification {
  #[serde(default)]
  pub expected_signals: Vec<String>,
  #[serde(default)]
  pub success_criteria: Vec<String>,
  #[serde(default)]
  pub non_goals: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SkillCatalogEntry {
  pub manifest: SkillManifest,
  pub path: PathBuf,
}

pub struct SkillCatalog {
  entries: Vec<SkillCatalogEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SkillCaseMatrix {
  pub skill_id: String,
  pub version: String,
  #[serde(default)]
  pub status: String,
  #[serde(default)]
  pub cases: Vec<SkillCase>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillCase {
  #[serde(default)]
  pub case_id: String,
  #[serde(default)]
  pub status: String,
  #[serde(default)]
  pub inputs: BTreeMap<String, String>,
  #[serde(default)]
  pub disturbance: String,
  #[serde(default)]
  pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SkillCaseMatrixEntry {
  pub matrix: SkillCaseMatrix,
  pub path: PathBuf,
}

pub struct SkillCaseMatrixCatalog {
  entries: Vec<SkillCaseMatrixEntry>,
}

#[derive(Clone, Debug, Default)]
pub struct SkillRunOptions {
  pub dry_run: bool,
  pub max_disturbance: Option<DisturbanceClass>,
  pub overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct SkillCaseRunOptions {
  pub dry_run: bool,
  pub max_disturbance: Option<DisturbanceClass>,
  pub only_case_ids: Vec<String>,
  pub include_nonvalidated: bool,
}

struct LiveAppSkillLock {
  path: PathBuf,
}

impl Drop for LiveAppSkillLock {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

impl SkillCatalog {
  pub fn discover(project_root: &Path) -> AuvResult<Self> {
    let recipes_root = project_root.join("recipes");
    if !recipes_root.exists() {
      return Ok(Self {
        entries: Vec::new(),
      });
    }

    let mut entries = Vec::new();
    collect_skill_entries(&recipes_root, &mut entries)?;
    entries.sort_by(|left, right| left.manifest.recipe_id.cmp(&right.manifest.recipe_id));
    Ok(Self { entries })
  }

  pub fn entries(&self) -> &[SkillCatalogEntry] {
    &self.entries
  }

  pub fn resolve(&self, project_root: &Path, query: &str) -> AuvResult<&SkillCatalogEntry> {
    let candidate = Path::new(query);
    if candidate.exists() {
      let absolute = fs::canonicalize(candidate)
        .map_err(|error| format!("failed to canonicalize skill path {query}: {error}"))?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "skill path {} is not a registered recipe manifest",
            absolute.display()
          )
        });
    }

    let project_relative = project_root.join(query);
    if project_relative.exists() {
      let absolute = fs::canonicalize(&project_relative).map_err(|error| {
        format!(
          "failed to canonicalize project-relative skill path {}: {error}",
          project_relative.display()
        )
      })?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "skill path {} is not a registered recipe manifest",
            absolute.display()
          )
        });
    }

    self
      .entries
      .iter()
      .find(|entry| entry.manifest.recipe_id == query)
      .ok_or_else(|| {
        format!("unknown skill {query}; use `auv-cli skill list` to inspect the current catalog")
      })
  }

  pub fn resolve_recipe_id(&self, recipe_id: &str) -> AuvResult<&SkillCatalogEntry> {
    self
      .entries
      .iter()
      .find(|entry| entry.manifest.recipe_id == recipe_id)
      .ok_or_else(|| format!("unknown skill {recipe_id}; use `auv-cli skill list`"))
  }
}

impl SkillCaseMatrixCatalog {
  pub fn discover(project_root: &Path) -> AuvResult<Self> {
    let recipes_root = project_root.join("recipes");
    if !recipes_root.exists() {
      return Ok(Self {
        entries: Vec::new(),
      });
    }

    let mut entries = Vec::new();
    collect_case_matrix_entries(&recipes_root, &mut entries)?;
    entries.sort_by(|left, right| left.matrix.skill_id.cmp(&right.matrix.skill_id));
    Ok(Self { entries })
  }

  pub fn entries(&self) -> &[SkillCaseMatrixEntry] {
    &self.entries
  }

  pub fn resolve(&self, project_root: &Path, query: &str) -> AuvResult<&SkillCaseMatrixEntry> {
    let candidate = Path::new(query);
    if candidate.exists() {
      let absolute = fs::canonicalize(candidate)
        .map_err(|error| format!("failed to canonicalize case-matrix path {query}: {error}"))?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "case-matrix path {} is not a registered matrix manifest",
            absolute.display()
          )
        });
    }

    let project_relative = project_root.join(query);
    if project_relative.exists() {
      let absolute = fs::canonicalize(&project_relative).map_err(|error| {
        format!(
          "failed to canonicalize project-relative case-matrix path {}: {error}",
          project_relative.display()
        )
      })?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "case-matrix path {} is not a registered matrix manifest",
            absolute.display()
          )
        });
    }

    self
      .entries
      .iter()
      .find(|entry| entry.matrix.skill_id == query)
      .ok_or_else(|| format!("unknown case matrix {query}; use `auv-cli skill cases list`"))
  }
}

pub fn run_skill(
  runtime: &Runtime,
  entry: &SkillCatalogEntry,
  options: SkillRunOptions,
) -> AuvResult<()> {
  let manifest = &entry.manifest;
  validate_skill_manifest(manifest)?;
  let mut variables = default_inputs(manifest)?;
  for (key, value) in options.overrides {
    variables.insert(key, value);
  }

  let active_max = validate_disturbance_policy(manifest, options.max_disturbance)?;
  let _lock = maybe_acquire_live_app_lock(manifest, &variables, options.dry_run)?;

  println!("skill: {}", manifest.recipe_id);
  println!("version: {}", manifest.version);
  println!("objective: {}", manifest.objective);
  println!(
    "target: {}",
    render_template(&manifest.target_app.bundle_id, &variables)
  );
  println!("max disturbance: {}", active_max.as_str());

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_id = if step.id.is_empty() {
      format!("step-{}", index + 1)
    } else {
      step.id.clone()
    };
    let request = build_invoke_request(step, &variables)?;
    let step_max = parse_step_max(step)?;
    let step_classes = if step.disturbance.classes.is_empty() {
      "none".to_string()
    } else {
      step.disturbance.classes.join(", ")
    };
    print_step_preview(
      index + 1,
      manifest.steps.len(),
      &step_id,
      &request,
      step_max,
      &step_classes,
    );

    if options.dry_run {
      continue;
    }

    let result = runtime.invoke(request)?;
    print_invoke_result(&result);
    enforce_step_expectations(&step_id, step, &result, &variables)?;
    export_step_variables(&step_id, &result, &mut variables);
    enforce_invoke_success(&result)?;
  }

  Ok(())
}

fn validate_skill_manifest(manifest: &SkillManifest) -> AuvResult<()> {
  validate_skill_identity(manifest)?;
  validate_skill_target_app(manifest)?;
  validate_skill_inputs(manifest)?;
  validate_skill_steps(manifest)?;
  validate_skill_verification(manifest)?;
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

fn validate_skill_steps(manifest: &SkillManifest) -> AuvResult<()> {
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
    parse_step_max(step).map_err(|error| {
      format!(
        "skill {} step {} has invalid disturbance.max {}: {error}",
        manifest.recipe_id, step_label, step.disturbance.max
      )
    })?;

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

pub fn run_skill_case_matrix(
  runtime: &Runtime,
  skill_catalog: &SkillCatalog,
  matrix_entry: &SkillCaseMatrixEntry,
  options: SkillCaseRunOptions,
) -> AuvResult<()> {
  let skill_entry = skill_catalog.resolve_recipe_id(&matrix_entry.matrix.skill_id)?;
  let cases = select_cases(&matrix_entry.matrix, &options)?;
  let selected_case_count = cases.len();

  println!("case-matrix: {}", matrix_entry.matrix.skill_id);
  println!("version: {}", matrix_entry.matrix.version);
  if !matrix_entry.matrix.status.is_empty() {
    println!("status: {}", matrix_entry.matrix.status);
  }
  println!("selected cases: {}", cases.len());

  let mut failures = Vec::new();
  for case in cases {
    println!("case: {} [{}]", case.case_id, case.status);
    match run_skill(
      runtime,
      skill_entry,
      SkillRunOptions {
        dry_run: options.dry_run,
        max_disturbance: options.max_disturbance,
        overrides: case.inputs.clone(),
      },
    ) {
      Ok(()) => println!("case-result: ok"),
      Err(error) => {
        println!("case-result: failed");
        println!("case-error: {error}");
        failures.push((case.case_id.clone(), error));
      }
    }
  }

  if !failures.is_empty() {
    let summary = failures
      .iter()
      .map(|(case_id, error)| format!("- {case_id}: {error}"))
      .collect::<Vec<_>>()
      .join("\n");
    return Err(format!(
      "{} of {} selected case(s) failed:\n{}",
      failures.len(),
      selected_case_count,
      summary
    ));
  }

  Ok(())
}

fn collect_skill_entries(root: &Path, entries: &mut Vec<SkillCatalogEntry>) -> AuvResult<()> {
  for raw_entry in fs::read_dir(root)
    .map_err(|error| format!("failed to read skill directory {}: {error}", root.display()))?
  {
    let raw_entry =
      raw_entry.map_err(|error| format!("failed to enumerate skill directory entry: {error}"))?;
    let path = raw_entry.path();
    if path.is_dir() {
      collect_skill_entries(&path, entries)?;
      continue;
    }
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
      continue;
    }

    let raw = fs::read_to_string(&path)
      .map_err(|error| format!("failed to read skill manifest {}: {error}", path.display()))?;
    if let Ok(manifest) = serde_json::from_str::<SkillManifest>(&raw) {
      entries.push(SkillCatalogEntry {
        manifest,
        path: fs::canonicalize(&path)
          .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?,
      });
    }
  }
  Ok(())
}

fn collect_case_matrix_entries(
  root: &Path,
  entries: &mut Vec<SkillCaseMatrixEntry>,
) -> AuvResult<()> {
  for raw_entry in fs::read_dir(root).map_err(|error| {
    format!(
      "failed to read case-matrix directory {}: {error}",
      root.display()
    )
  })? {
    let raw_entry = raw_entry
      .map_err(|error| format!("failed to enumerate case-matrix directory entry: {error}"))?;
    let path = raw_entry.path();
    if path.is_dir() {
      collect_case_matrix_entries(&path, entries)?;
      continue;
    }
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
      continue;
    }
    let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
      continue;
    };
    if !file_name.contains(".cases.") {
      continue;
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
      format!(
        "failed to read case-matrix manifest {}: {error}",
        path.display()
      )
    })?;
    let matrix = serde_json::from_str::<SkillCaseMatrix>(&raw).map_err(|error| {
      format!(
        "failed to parse case-matrix manifest {}: {error}",
        path.display()
      )
    })?;
    entries.push(SkillCaseMatrixEntry {
      matrix,
      path: fs::canonicalize(&path)
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?,
    });
  }
  Ok(())
}

fn default_inputs(manifest: &SkillManifest) -> AuvResult<BTreeMap<String, String>> {
  let mut resolved = BTreeMap::new();
  for (key, spec) in &manifest.inputs {
    if let Some(default) = &spec.default {
      resolved.insert(key.clone(), stringify_value(default)?);
    }
  }
  Ok(resolved)
}

fn select_cases<'a>(
  matrix: &'a SkillCaseMatrix,
  options: &SkillCaseRunOptions,
) -> AuvResult<Vec<&'a SkillCase>> {
  let selected = if options.only_case_ids.is_empty() {
    matrix
      .cases
      .iter()
      .filter(|case| options.include_nonvalidated || case.status == "validated")
      .collect::<Vec<_>>()
  } else {
    matrix
      .cases
      .iter()
      .filter(|case| {
        options
          .only_case_ids
          .iter()
          .any(|wanted| wanted == &case.case_id)
      })
      .collect::<Vec<_>>()
  };

  if selected.is_empty() {
    let reason = if options.only_case_ids.is_empty() {
      "no matching validated cases found"
    } else {
      "no matching cases found for requested case ids"
    };
    return Err(format!("{reason} in matrix {}", matrix.skill_id));
  }

  Ok(selected)
}

fn validate_disturbance_policy(
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

fn parse_step_max(step: &SkillStep) -> AuvResult<DisturbanceClass> {
  if step.disturbance.max.is_empty() {
    Ok(DisturbanceClass::None)
  } else {
    DisturbanceClass::parse(&step.disturbance.max)
  }
}

fn maybe_acquire_live_app_lock(
  manifest: &SkillManifest,
  variables: &BTreeMap<String, String>,
  dry_run: bool,
) -> AuvResult<Option<LiveAppSkillLock>> {
  if dry_run || manifest.target_app.display_mode != "live-desktop" {
    return Ok(None);
  }

  let bundle_id = render_template(&manifest.target_app.bundle_id, variables);
  if bundle_id.trim().is_empty() {
    return Ok(None);
  }

  let timeout_ms = std::env::var("AUV_RECIPE_LOCK_TIMEOUT_MS")
    .ok()
    .and_then(|raw| raw.parse::<u64>().ok())
    .unwrap_or(10_000);
  let path = PathBuf::from(format!(
    "/tmp/auv-live-app-{}.lock",
    sanitize_lock_component(&bundle_id)
  ));
  let started = Instant::now();
  loop {
    match OpenOptions::new().create_new(true).write(true).open(&path) {
      Ok(mut handle) => {
        writeln!(handle, "pid={}", std::process::id()).ok();
        writeln!(handle, "skill={}", manifest.recipe_id).ok();
        writeln!(handle, "bundleId={bundle_id}").ok();
        return Ok(Some(LiveAppSkillLock { path }));
      }
      Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
        clear_stale_lock_file(&path)?;
        if started.elapsed() > Duration::from_millis(timeout_ms) {
          let owner = describe_lock_owner(&path).unwrap_or_else(|_| "unknown owner".to_string());
          return Err(format!(
            "timed out waiting for live-app skill lock for {bundle_id:?} after {timeout_ms} ms ({owner}; path={})",
            path.display()
          ));
        }
        thread::sleep(Duration::from_millis(50));
      }
      Err(error) => {
        return Err(format!(
          "failed to acquire live-app skill lock for {bundle_id:?}: {error}"
        ));
      }
    }
  }
}

fn build_invoke_request(
  step: &SkillStep,
  variables: &BTreeMap<String, String>,
) -> AuvResult<InvokeRequest> {
  let mut target = ExecutionTarget::default();
  let mut inputs = BTreeMap::new();
  for (key, value) in &step.args {
    let rendered = render_value(value, variables)?;
    if key == "target" {
      target.application_id = Some(rendered);
    } else {
      inputs.insert(key.clone(), rendered);
    }
  }

  Ok(InvokeRequest {
    command_id: step.command_id.clone(),
    target,
    inputs,
  })
}

fn render_value(value: &Value, variables: &BTreeMap<String, String>) -> AuvResult<String> {
  match value {
    Value::String(raw) => Ok(render_template(raw, variables)),
    other => stringify_value(other),
  }
}

fn stringify_value(value: &Value) -> AuvResult<String> {
  match value {
    Value::Null => Ok(String::new()),
    Value::Bool(raw) => Ok(if *raw { "true" } else { "false" }.to_string()),
    Value::Number(raw) => Ok(raw.to_string()),
    Value::String(raw) => Ok(raw.clone()),
    Value::Array(_) | Value::Object(_) => Err(format!(
      "unsupported JSON value in skill manifest; expected scalar, got {}",
      value
    )),
  }
}

fn render_template(raw: &str, variables: &BTreeMap<String, String>) -> String {
  let mut rendered = raw.to_string();
  for (key, value) in variables {
    let pattern = format!("${{{key}}}");
    rendered = rendered.replace(&pattern, value);
  }
  rendered
}

fn print_step_preview(
  index: usize,
  total: usize,
  step_id: &str,
  request: &InvokeRequest,
  step_max: DisturbanceClass,
  step_classes: &str,
) {
  let mut command = vec![
    "auv-cli".to_string(),
    "invoke".to_string(),
    request.command_id.clone(),
  ];
  if let Some(target) = &request.target.application_id {
    command.push("--target".to_string());
    command.push(target.clone());
  }
  for (key, value) in &request.inputs {
    command.push(format!("--{key}"));
    command.push(value.clone());
  }
  println!(
    "[{index}/{total}] {step_id} (disturbance max={}; classes={step_classes}) -> {}",
    step_max.as_str(),
    command.join(" ")
  );
}

fn print_invoke_result(result: &InvokeResult) {
  println!("runId: {}", result.run_id);
  println!("status: {}", result.status.as_str());
  println!("output: {}", result.output_summary);
  for artifact in &result.artifact_paths {
    println!("artifact: {}", artifact.display());
  }
}

fn export_step_variables(
  step_id: &str,
  result: &InvokeResult,
  variables: &mut BTreeMap<String, String>,
) {
  let prefix = format!("step_{}", sanitize_step_component(step_id));
  variables.insert(format!("{prefix}_run_id"), result.run_id.clone());
  variables.insert(
    format!("{prefix}_status"),
    result.status.as_str().to_string(),
  );
  variables.insert(format!("{prefix}_output"), result.output_summary.clone());
  variables.insert(
    format!("{prefix}_artifact_count"),
    result.artifact_paths.len().to_string(),
  );

  let mut image_paths = Vec::new();
  for (index, artifact) in result.artifact_paths.iter().enumerate() {
    let rendered = artifact.display().to_string();
    variables.insert(format!("{prefix}_artifact_{index}"), rendered.clone());
    if is_image_artifact(&rendered) {
      image_paths.push(rendered);
    }
  }

  if let Some(last) = result.artifact_paths.last() {
    variables.insert(
      format!("{prefix}_artifact_last"),
      last.display().to_string(),
    );
  }

  if let Some(first_image) = image_paths.first() {
    variables.insert(format!("{prefix}_artifact_image_0"), first_image.clone());
  }
  if let Some(last_image) = image_paths.last() {
    variables.insert(format!("{prefix}_artifact_image_last"), last_image.clone());
  }
}

fn enforce_invoke_success(result: &InvokeResult) -> AuvResult<()> {
  if let Some(failure) = &result.failure_message {
    return Err(format!(
      "{} (inspect with `auv-cli inspect {}`)",
      failure, result.run_id
    ));
  }
  if result.status != crate::model::RunStatus::Completed {
    return Err(format!("run {} finished in failed state", result.run_id));
  }
  Ok(())
}

fn enforce_step_expectations(
  step_id: &str,
  step: &SkillStep,
  result: &InvokeResult,
  variables: &BTreeMap<String, String>,
) -> AuvResult<()> {
  for needle in &step.expect.output_must_contain {
    let rendered = render_template(needle, variables);
    if !result.output_summary.contains(&rendered) {
      return Err(format!(
        "step {step_id:?} output did not contain required marker {rendered:?}: {}",
        result.output_summary,
      ));
    }
  }
  for needle in &step.expect.output_must_not_contain {
    let rendered = render_template(needle, variables);
    if result.output_summary.contains(&rendered) {
      return Err(format!(
        "step {step_id:?} output contained forbidden marker {rendered:?}: {}",
        result.output_summary,
      ));
    }
  }
  if let Some(minimum) = step.expect.artifact_count_at_least {
    if result.artifact_paths.len() < minimum {
      return Err(format!(
        "step {step_id:?} produced {} artifacts, below required minimum {}",
        result.artifact_paths.len(),
        minimum
      ));
    }
  }
  Ok(())
}

fn sanitize_step_component(raw: &str) -> String {
  let lowered = raw.trim().to_lowercase().replace('-', "_");
  let collapsed = lowered
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || character == '_' {
        character
      } else {
        '_'
      }
    })
    .collect::<String>();
  collapsed
    .split('_')
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join("_")
}

fn sanitize_lock_component(raw: &str) -> String {
  raw
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character
      } else {
        '-'
      }
    })
    .collect::<String>()
    .split('-')
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join("-")
}

fn is_image_artifact(path: &str) -> bool {
  let lowered = path.to_ascii_lowercase();
  lowered.ends_with(".png") || lowered.ends_with(".jpg") || lowered.ends_with(".jpeg")
}

fn step_id(step: &SkillStep, fallback_index: usize) -> String {
  if step.id.is_empty() {
    format!("step-{}", fallback_index + 1)
  } else {
    step.id.clone()
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use serde_json::json;

  use super::{
    SkillCaseMatrixCatalog, SkillCatalog, SkillManifest, default_inputs, export_step_variables,
    is_image_artifact, render_template, render_value, sanitize_lock_component,
    validate_skill_manifest,
  };
  use crate::model::{InvokeResult, RunStatus, now_millis};

  #[test]
  fn default_inputs_extract_scalar_defaults() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" },
        "click_count": { "type": "integer", "default": 2 }
      },
      "steps": []
    }))
    .expect("manifest should deserialize");

    let defaults = default_inputs(&manifest).expect("defaults should extract");
    assert_eq!(defaults.get("query").expect("query default"), "aa");
    assert_eq!(
      defaults.get("click_count").expect("click_count default"),
      "2"
    );
  }

  #[test]
  fn render_value_substitutes_step_artifact_placeholders() {
    let rendered = render_value(
      &json!("${step_capture_evidence_artifact_image_0}"),
      &BTreeMap::from([(
        "step_capture_evidence_artifact_image_0".to_string(),
        "/tmp/example.png".to_string(),
      )]),
    )
    .expect("template should render");
    assert_eq!(rendered, "/tmp/example.png");
  }

  #[test]
  fn export_step_variables_captures_image_artifacts() {
    let mut variables = BTreeMap::new();
    export_step_variables(
      "capture-evidence",
      &InvokeResult {
        run_id: "run_1".to_string(),
        status: RunStatus::Completed,
        output_summary: "ok".to_string(),
        artifact_paths: vec![
          PathBuf::from("/tmp/report.txt"),
          PathBuf::from("/tmp/evidence.png"),
        ],
        failure_message: None,
      },
      &mut variables,
    );

    assert_eq!(
      variables
        .get("step_capture_evidence_artifact_image_0")
        .expect("image artifact should export"),
      "/tmp/evidence.png"
    );
  }

  #[test]
  fn skill_catalog_discovers_recipe_manifests_only() {
    let root = env::temp_dir().join(format!("auv-skill-catalog-{}", now_millis()));
    fs::create_dir_all(root.join("recipes/test")).expect("temp recipes dir should exist");
    fs::write(
      root.join("recipes/test/example.v0.json"),
      r#"{
        "recipe_id": "test.example.v0",
        "version": "0.1.0",
        "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
        "objective": "test",
        "steps": []
      }"#,
    )
    .expect("manifest should write");
    fs::write(root.join("recipes/test/cases.json"), r#"{"cases":[]}"#)
      .expect("case file should write");

    let catalog = SkillCatalog::discover(&root).expect("catalog should load");
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].manifest.recipe_id, "test.example.v0");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn case_matrix_catalog_discovers_cases_manifests_only() {
    let root = env::temp_dir().join(format!("auv-case-matrix-{}", now_millis()));
    fs::create_dir_all(root.join("recipes/test")).expect("temp recipes dir should exist");
    fs::write(
      root.join("recipes/test/example.v0.json"),
      r#"{
        "recipe_id": "test.example.v0",
        "version": "0.1.0",
        "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
        "objective": "test",
        "steps": []
      }"#,
    )
    .expect("manifest should write");
    fs::write(
      root.join("recipes/test/example.cases.v0.json"),
      r#"{
        "skill_id": "test.example.v0",
        "version": "0.1.0",
        "cases": [{ "case_id": "baseline", "status": "validated" }]
      }"#,
    )
    .expect("matrix should write");

    let catalog = SkillCaseMatrixCatalog::discover(&root).expect("catalog should load");
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].matrix.skill_id, "test.example.v0");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn is_image_artifact_recognizes_common_extensions() {
    assert!(is_image_artifact("/tmp/example.png"));
    assert!(is_image_artifact("/tmp/example.JPG"));
    assert!(!is_image_artifact("/tmp/example.txt"));
  }

  #[test]
  fn render_template_leaves_unknown_placeholders_in_place() {
    let rendered = render_template(
      "artifact=${missing}",
      &BTreeMap::from([("query".to_string(), "aa".to_string())]),
    );
    assert_eq!(rendered, "artifact=${missing}");
  }

  #[test]
  fn sanitize_lock_component_collapses_non_alphanumeric_segments() {
    assert_eq!(
      sanitize_lock_component("com.tencent.QQMusicMac"),
      "com-tencent-QQMusicMac"
    );
    assert_eq!(
      sanitize_lock_component("  weird / bundle id  "),
      "weird-bundle-id"
    );
  }

  #[test]
  fn validate_skill_manifest_accepts_minimal_valid_recipe() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["pointer"]
      },
      "steps": [{
        "id": "step-1",
        "command_id": "debug.captureScreen",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    validate_skill_manifest(&manifest).expect("manifest should validate");
  }

  #[test]
  fn validate_skill_manifest_rejects_empty_steps() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "objective": "test",
      "steps": [],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error = validate_skill_manifest(&manifest).expect_err("empty steps should fail");
    assert!(error.contains("at least one step"));
  }

  #[test]
  fn validate_skill_manifest_rejects_invalid_version() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "not-a-version",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "objective": "test",
      "steps": [{
        "command_id": "debug.captureScreen",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error = validate_skill_manifest(&manifest).expect_err("invalid version should fail");
    assert!(error.contains("invalid version"));
  }
}
