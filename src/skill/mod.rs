// File: src/skill/mod.rs
//! Skill (recipe) loading, validation, and execution.
//!
//! A skill manifest is a declarative recipe: inputs + ordered steps (commands)
//! with disturbance budgets and verification expectations. This module validates
//! manifests/case matrices, then executes them by calling into `Runtime` while
//! recording a structured run tree.
//!
//! Boundary: skills orchestrate commands; they do not implement platform UI
//! automation (drivers do), and they are not a high-level planner.

mod case_matrix;
mod recipe;
mod validate;

pub(crate) use case_matrix::run_skill_case_matrix_into_run;
pub use case_matrix::{render_skill_case_matrix_report, run_skill_case_matrix};
pub use recipe::{
  SkillRecipe, SkillRecipeOrigin, SkillRecipeRunner,
  observer::{
    ConsoleRecipeRunReporter, NoopRecipeRunReporter, RecipeRunReporter, RecipeStartedReport,
    RecipeStepReport,
  },
};
pub(crate) use validate::{
  build_inline_scan_hook_manifest, parse_step_max, validate_case_matrix_against_skill,
  validate_case_matrix_manifest, validate_disturbance_policy, validate_skill_manifest,
  validate_skill_manifest_with_commands,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::driver::macos::support::runtime::{clear_stale_lock_file, describe_lock_owner};
use crate::model::{
  AuvResult, DisturbanceClass, ExecutionTarget, InvokeRequest, InvokeResult, now_millis,
};
use crate::runtime::Runtime;
use crate::trace::{RunId, TraceStatusCode, new_span_id, string_attr};

#[derive(Clone, Debug, Deserialize)]
pub struct SkillManifest {
  pub recipe_id: String,
  pub version: String,
  #[serde(default)]
  pub status: String,
  #[serde(default)]
  pub platform: String,
  pub target_app: SkillTargetApp,
  #[serde(default)]
  pub strategy: SkillStrategy,
  #[serde(default)]
  pub invocation: SkillInvocation,
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
  pub hooks: BTreeMap<String, SkillInlineHook>,
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

#[derive(Clone, Debug, Deserialize, Default, PartialEq, Eq)]
pub struct SkillStrategy {
  #[serde(default)]
  pub family: String,
  #[serde(default)]
  pub grounding: String,
  #[serde(default)]
  pub activation: String,
  #[serde(default, rename = "verificationContract")]
  pub verification_contract: String,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq, Eq)]
pub struct SkillInvocation {
  #[serde(default)]
  pub kind: String,
  #[serde(default)]
  pub host: String,
  #[serde(default)]
  pub stage: String,
  #[serde(default)]
  pub context_schema: String,
  #[serde(default)]
  pub return_schema: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkillStrategyTaxonomy {
  pub family: SkillStrategyFamily,
  pub grounding: SkillGrounding,
  pub activation: SkillActivation,
  pub verification_contract: SkillVerificationContract,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillStrategyFamily {
  SearchEntry,
  ResultSelection,
  Playback,
  NativeText,
  WindowAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillGrounding {
  AxTextInput,
  OcrAnchor,
  VisualRow,
  AxText,
  WindowPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillActivation {
  ClipboardSubmit,
  PointerClick,
  PointerDoubleClick,
  PointerRowActivation,
  PointerFocusClipboardPaste,
  /// Phase 2 + Phase 3 #2: the whole activation chain reaches the
  /// target via AX (AXUIElementPerformAction + AXUIElementSetAttribute
  /// for focus) and only uses the clipboard for marker insertion. No
  /// step warps the macOS cursor.
  AxPerformActionClipboardPaste,
  /// Phase 3 #5: `debug.smartPress`. Tries the AX path
  /// (`AXUIElementPerformAction`) first; if that fails and
  /// `allow_pointer_fallback` is not disabled, falls back to a
  /// real pointer click. The actual strategy each invocation took
  /// is recorded in `signals.smartPress.strategy`
  /// (`ax-action` | `pointer-click`); the taxonomy label only says
  /// "this recipe is allowed to take either path".
  SmartPress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillVerificationContract {
  CaptureEvidence,
  VerifyImageText,
  VerifyNowPlayingTitle,
  VerifyAxText,
}

impl SkillStrategy {
  pub fn taxonomy(&self) -> AuvResult<SkillStrategyTaxonomy> {
    let family = SkillStrategyFamily::parse(&self.family)?;
    let grounding = SkillGrounding::parse(&self.grounding)?;
    let activation = SkillActivation::parse(&self.activation)?;
    let verification_contract = SkillVerificationContract::parse(&self.verification_contract)?;
    Ok(SkillStrategyTaxonomy {
      family,
      grounding,
      activation,
      verification_contract,
    })
  }

  pub fn taxonomy_id(&self) -> AuvResult<String> {
    Ok(self.taxonomy()?.taxonomy_id())
  }
}

impl SkillStrategyTaxonomy {
  pub fn taxonomy_id(&self) -> String {
    format!(
      "{}.{}.{}.{}",
      self.family.as_str(),
      self.grounding.as_str(),
      self.activation.as_str(),
      self.verification_contract.as_str()
    )
  }

  fn allowed() -> &'static [SkillStrategyTaxonomy] {
    const ALLOWED: &[SkillStrategyTaxonomy] = &[
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::SearchEntry,
        grounding: SkillGrounding::AxTextInput,
        activation: SkillActivation::ClipboardSubmit,
        verification_contract: SkillVerificationContract::CaptureEvidence,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::ResultSelection,
        grounding: SkillGrounding::OcrAnchor,
        activation: SkillActivation::PointerClick,
        verification_contract: SkillVerificationContract::CaptureEvidence,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::Playback,
        grounding: SkillGrounding::OcrAnchor,
        activation: SkillActivation::PointerDoubleClick,
        verification_contract: SkillVerificationContract::VerifyImageText,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::Playback,
        grounding: SkillGrounding::VisualRow,
        activation: SkillActivation::PointerRowActivation,
        verification_contract: SkillVerificationContract::VerifyNowPlayingTitle,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::NativeText,
        grounding: SkillGrounding::AxText,
        activation: SkillActivation::PointerFocusClipboardPaste,
        verification_contract: SkillVerificationContract::VerifyAxText,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::NativeText,
        grounding: SkillGrounding::AxText,
        activation: SkillActivation::AxPerformActionClipboardPaste,
        verification_contract: SkillVerificationContract::VerifyAxText,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::WindowAction,
        grounding: SkillGrounding::WindowPoint,
        activation: SkillActivation::PointerClick,
        verification_contract: SkillVerificationContract::CaptureEvidence,
      },
      SkillStrategyTaxonomy {
        family: SkillStrategyFamily::WindowAction,
        grounding: SkillGrounding::WindowPoint,
        activation: SkillActivation::SmartPress,
        verification_contract: SkillVerificationContract::CaptureEvidence,
      },
    ];
    ALLOWED
  }

  fn is_allowed(&self) -> bool {
    Self::allowed().contains(self)
  }

  fn allowed_taxonomy_ids() -> String {
    Self::allowed()
      .iter()
      .map(Self::taxonomy_id)
      .collect::<Vec<_>>()
      .join(", ")
  }
}

impl SkillStrategyFamily {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "search-entry" => Ok(Self::SearchEntry),
      "result-selection" => Ok(Self::ResultSelection),
      "playback" => Ok(Self::Playback),
      "native-text" => Ok(Self::NativeText),
      "window-action" => Ok(Self::WindowAction),
      other => Err(format!(
        "strategy.family {} is unsupported; allowed values: search-entry, result-selection, playback, native-text, window-action",
        other
      )),
    }
  }

  fn as_str(&self) -> &'static str {
    match self {
      Self::SearchEntry => "search-entry",
      Self::ResultSelection => "result-selection",
      Self::Playback => "playback",
      Self::NativeText => "native-text",
      Self::WindowAction => "window-action",
    }
  }
}

impl SkillGrounding {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "ax-text-input" => Ok(Self::AxTextInput),
      "ocr-anchor" => Ok(Self::OcrAnchor),
      "visual-row" => Ok(Self::VisualRow),
      "ax-text" => Ok(Self::AxText),
      "window-point" => Ok(Self::WindowPoint),
      other => Err(format!(
        "strategy.grounding {} is unsupported; allowed values: ax-text-input, ocr-anchor, visual-row, ax-text, window-point",
        other
      )),
    }
  }

  fn as_str(&self) -> &'static str {
    match self {
      Self::AxTextInput => "ax-text-input",
      Self::OcrAnchor => "ocr-anchor",
      Self::VisualRow => "visual-row",
      Self::AxText => "ax-text",
      Self::WindowPoint => "window-point",
    }
  }
}

impl SkillActivation {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "clipboard-submit" => Ok(Self::ClipboardSubmit),
      "pointer-click" => Ok(Self::PointerClick),
      "pointer-double-click" => Ok(Self::PointerDoubleClick),
      "pointer-row-activation" => Ok(Self::PointerRowActivation),
      "pointer-focus-clipboard-paste" => Ok(Self::PointerFocusClipboardPaste),
      "ax-perform-action-clipboard-paste" => Ok(Self::AxPerformActionClipboardPaste),
      "smart-press" => Ok(Self::SmartPress),
      other => Err(format!(
        "strategy.activation {} is unsupported; allowed values: clipboard-submit, pointer-click, pointer-double-click, pointer-row-activation, pointer-focus-clipboard-paste, ax-perform-action-clipboard-paste, smart-press",
        other
      )),
    }
  }

  fn as_str(&self) -> &'static str {
    match self {
      Self::ClipboardSubmit => "clipboard-submit",
      Self::PointerClick => "pointer-click",
      Self::PointerDoubleClick => "pointer-double-click",
      Self::PointerRowActivation => "pointer-row-activation",
      Self::PointerFocusClipboardPaste => "pointer-focus-clipboard-paste",
      Self::AxPerformActionClipboardPaste => "ax-perform-action-clipboard-paste",
      Self::SmartPress => "smart-press",
    }
  }
}

impl SkillVerificationContract {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "captureEvidence" => Ok(Self::CaptureEvidence),
      "verifyImageText" => Ok(Self::VerifyImageText),
      "verifyNowPlayingTitle" => Ok(Self::VerifyNowPlayingTitle),
      "verifyAxText" => Ok(Self::VerifyAxText),
      other => Err(format!(
        "strategy.verificationContract {} is unsupported; allowed values: captureEvidence, verifyImageText, verifyNowPlayingTitle, verifyAxText",
        other
      )),
    }
  }

  fn as_str(&self) -> &'static str {
    match self {
      Self::CaptureEvidence => "capture-evidence",
      Self::VerifyImageText => "verify-image-text",
      Self::VerifyNowPlayingTitle => "verify-now-playing-title",
      Self::VerifyAxText => "verify-ax-text",
    }
  }
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
  /// Step-level opt-out for Rule 1 in
  /// `docs/ai/references/2026-05-22-phase-3-mainline-acceptance.md`.
  /// Required when a `command_id` is restricted to the
  /// `macos.demo.*` namespace by the mainline-compliance gate (today
  /// only `debug.smartPress`). The reason must be human-readable; the
  /// category limits the kind of exemption being claimed.
  #[serde(default)]
  pub mainline_exemption: Option<MainlineExemption>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct MainlineExemption {
  #[serde(default)]
  pub reason: String,
  #[serde(default)]
  pub category: String,
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
  pub signal_equals: BTreeMap<String, String>,
  #[serde(default)]
  pub signal_contains: BTreeMap<String, String>,
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

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillInlineHook {
  #[serde(default)]
  pub input_schema: String,
  #[serde(default)]
  pub return_schema: String,
  #[serde(default)]
  pub steps: Vec<SkillStep>,
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
  pub quiet: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SkillRunSummary {
  #[allow(dead_code)]
  pub exported_variables: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SkillCaseMatrixRunSummary {
  pub selected_case_count: usize,
  pub exported_variables: BTreeMap<String, String>,
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
  let recipe = SkillRecipe::from_manifest(
    entry.manifest.clone(),
    SkillRecipeOrigin::CatalogPath(entry.path.clone()),
  );
  if options.quiet {
    SkillRecipeRunner::new(runtime)
      .run(&recipe, options)
      .map(|_| ())
  } else {
    SkillRecipeRunner::new(runtime)
      .with_reporter(Box::new(ConsoleRecipeRunReporter))
      .run(&recipe, options)
      .map(|_| ())
  }
}

#[cfg(test)]
pub(crate) fn run_skill_manifest_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<RunId> {
  if options.quiet {
    SkillRecipeRunner::new(runtime).run_manifest(manifest, options)
  } else {
    SkillRecipeRunner::new(runtime)
      .with_reporter(Box::new(ConsoleRecipeRunReporter))
      .run_manifest(manifest, options)
  }
}

pub(crate) fn run_skill_manifest_into_run(
  runtime: &Runtime,
  run: &mut crate::run_builder::RecordingRun,
  parent: &crate::run_builder::SpanRef,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<SkillRunSummary> {
  let reporter: &dyn RecipeRunReporter = if options.quiet {
    &NoopRecipeRunReporter
  } else {
    &ConsoleRecipeRunReporter
  };
  run_skill_manifest_into_run_with_reporter(runtime, run, parent, manifest, options, reporter)
}

pub(super) fn run_skill_manifest_into_run_with_reporter(
  runtime: &Runtime,
  run: &mut crate::run_builder::RecordingRun,
  parent: &crate::run_builder::SpanRef,
  manifest: &SkillManifest,
  options: SkillRunOptions,
  reporter: &dyn RecipeRunReporter,
) -> AuvResult<SkillRunSummary> {
  validate_skill_manifest_with_commands(manifest, runtime.list_commands())?;
  let mut variables = default_inputs(manifest)?;
  for (key, value) in options.overrides {
    variables.insert(key, value);
  }
  let mut top_level_signal_exports = BTreeSet::new();

  let active_max = validate_disturbance_policy(manifest, options.max_disturbance)?;
  let _lock = maybe_acquire_live_app_lock(manifest, &variables, options.dry_run)?;

  report_recipe_started(reporter, manifest, &variables, active_max);

  let mut context = SkillManifestRuntime {
    runtime,
    run,
    parent,
    manifest,
    dry_run: options.dry_run,
    reporter,
    variables: &mut variables,
    top_level_signal_exports: &mut top_level_signal_exports,
  };
  for (index, step) in manifest.steps.iter().enumerate() {
    run_skill_step_recorded(&mut context, step, index)?;
  }

  Ok(SkillRunSummary {
    exported_variables: variables,
  })
}

struct SkillManifestRuntime<'a> {
  runtime: &'a Runtime,
  run: &'a mut crate::run_builder::RecordingRun,
  parent: &'a crate::run_builder::SpanRef,
  manifest: &'a SkillManifest,
  dry_run: bool,
  reporter: &'a dyn RecipeRunReporter,
  variables: &'a mut BTreeMap<String, String>,
  top_level_signal_exports: &'a mut BTreeSet<String>,
}

struct SkillStepRuntime<'a> {
  runtime: &'a Runtime,
  run: &'a mut crate::run_builder::RecordingRun,
  step_span: &'a crate::run_builder::SpanRef,
  manifest: &'a SkillManifest,
  dry_run: bool,
  reporter: &'a dyn RecipeRunReporter,
  variables: &'a mut BTreeMap<String, String>,
  top_level_signal_exports: &'a mut BTreeSet<String>,
}

fn report_recipe_started(
  reporter: &dyn RecipeRunReporter,
  manifest: &SkillManifest,
  variables: &BTreeMap<String, String>,
  active_max: DisturbanceClass,
) {
  reporter.recipe_started(RecipeStartedReport {
    recipe_id: manifest.recipe_id.clone(),
    version: manifest.version.clone(),
    objective: manifest.objective.clone(),
    target: render_template(&manifest.target_app.bundle_id, variables),
    max_disturbance: active_max,
  });
}

fn run_skill_step_recorded(
  context: &mut SkillManifestRuntime<'_>,
  step: &SkillStep,
  index: usize,
) -> AuvResult<()> {
  let step_id = step_id(step, index);
  let step_span = start_recipe_step_span(
    context.run,
    context.parent,
    context.manifest,
    &step_id,
    index,
  )?;
  let step_result = {
    let mut step_context = SkillStepRuntime {
      runtime: context.runtime,
      run: context.run,
      step_span: &step_span,
      manifest: context.manifest,
      dry_run: context.dry_run,
      reporter: context.reporter,
      variables: context.variables,
      top_level_signal_exports: context.top_level_signal_exports,
    };
    run_skill_step_into_span(&mut step_context, step, index, &step_id)
  };

  finish_recipe_step_span(context.run, &step_span, &step_id, step_result)
}

fn start_recipe_step_span(
  run: &mut crate::run_builder::RecordingRun,
  parent: &crate::run_builder::SpanRef,
  manifest: &SkillManifest,
  step_id: &str,
  index: usize,
) -> AuvResult<crate::run_builder::SpanRef> {
  run.start_span(
    parent,
    span_record(
      "auv.recipe.step",
      BTreeMap::from([
        ("auv.recipe.step_id".to_string(), string_attr(step_id)),
        ("auv.step.id".to_string(), string_attr(step_id)),
        ("auv.step.index".to_string(), serde_json::json!(index)),
        ("auv.step.kind".to_string(), string_attr("recipe")),
        (
          "auv.recipe.id".to_string(),
          string_attr(manifest.recipe_id.clone()),
        ),
      ]),
    ),
  )
}

fn finish_recipe_step_span(
  run: &mut crate::run_builder::RecordingRun,
  step_span: &crate::run_builder::SpanRef,
  step_id: &str,
  step_result: AuvResult<()>,
) -> AuvResult<()> {
  match step_result {
    Ok(()) => run.finish_span(
      step_span,
      crate::run_builder::SpanFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some(format!("Completed recipe step {step_id}")),
        failure: None,
      },
    ),
    Err(error) => {
      if let Err(finish_error) = run.finish_span(
        step_span,
        crate::run_builder::SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!("Recipe step {step_id} failed")),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!(
          "{error}; additionally failed to finish failed step span: {finish_error}"
        ));
      }
      Err(error)
    }
  }
}

fn run_skill_step_into_span(
  context: &mut SkillStepRuntime<'_>,
  step: &SkillStep,
  index: usize,
  step_id: &str,
) -> AuvResult<()> {
  let request = build_invoke_request(step, context.variables)?;
  let step_max = parse_step_max(step)?;
  let step_classes = if step.disturbance.classes.is_empty() {
    "none".to_string()
  } else {
    step.disturbance.classes.join(", ")
  };
  context.reporter.step_started(
    step_id,
    &request,
    RecipeStepReport {
      index,
      total: context.manifest.steps.len(),
      max_disturbance: step_max,
      disturbance_classes: step_classes,
    },
  );

  if context.dry_run {
    return Ok(());
  }

  let result = context
    .runtime
    .invoke_in_span(context.run, context.step_span, request)?;
  context.reporter.step_finished(step_id, &result);
  enforce_step_expectations(step_id, step, &result, context.variables)?;
  export_step_variables(
    step_id,
    &result,
    context.variables,
    context.top_level_signal_exports,
  );
  enforce_invoke_success(&result)?;
  Ok(())
}

pub(crate) fn finish_failed_recorded_run(
  runtime: &Runtime,
  run: crate::run_builder::RecordingRun,
  error: String,
  summary: String,
) -> AuvResult<RunId> {
  if let Err(finish_error) = runtime.finish_run(
    run,
    crate::run_builder::RunFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(summary),
      failure: Some(error.clone()),
    },
  ) {
    return Err(format!(
      "{error}; additionally failed to persist failed run: {finish_error}"
    ));
  }
  Err(error)
}

pub(crate) fn span_record(
  name: impl Into<String>,
  attributes: crate::run_builder::Attributes,
) -> crate::trace::SpanRecordV1Alpha1 {
  crate::trace::SpanRecordV1Alpha1 {
    api_version: crate::trace::SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.into(),
    state: crate::trace::TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
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
    validate_case_matrix_manifest(&matrix)
      .map_err(|error| format!("invalid case-matrix manifest {}: {error}", path.display()))?;
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

pub(crate) fn stringify_value(value: &Value) -> AuvResult<String> {
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

fn export_step_variables(
  step_id: &str,
  result: &InvokeResult,
  variables: &mut BTreeMap<String, String>,
  top_level_signal_exports: &mut BTreeSet<String>,
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

  for (key, value) in &result.signals {
    let signal_key = format!("{prefix}_signal_{}", sanitize_step_component(key));
    variables.entry(signal_key).or_insert_with(|| value.clone());
    if is_top_level_hook_signal(key)
      && (!variables.contains_key(key) || top_level_signal_exports.contains(key))
    {
      variables.insert(key.clone(), value.clone());
      top_level_signal_exports.insert(key.clone());
    }
  }
}

fn is_top_level_hook_signal(key: &str) -> bool {
  matches!(
    key,
    "last.scan.hook.decision" | "last.scan.hook.action" | "last.scan.hook.reason"
  )
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
  let available_markers = render_signal_markers(&result.signals);
  for needle in &step.expect.output_must_contain {
    let rendered = render_template(needle, variables);
    if !available_markers.contains(&rendered) {
      return Err(format!(
        "step {step_id:?} signals did not contain required marker {rendered:?}: {}",
        render_signal_marker_summary(&available_markers),
      ));
    }
  }
  for needle in &step.expect.output_must_not_contain {
    let rendered = render_template(needle, variables);
    if available_markers.contains(&rendered) {
      return Err(format!(
        "step {step_id:?} signals contained forbidden marker {rendered:?}: {}",
        render_signal_marker_summary(&available_markers),
      ));
    }
  }
  if let Some(minimum) = step.expect.artifact_count_at_least
    && result.artifact_paths.len() < minimum
  {
    return Err(format!(
      "step {step_id:?} produced {} artifacts, below required minimum {}",
      result.artifact_paths.len(),
      minimum
    ));
  }
  for (signal_key, expected_value) in &step.expect.signal_equals {
    let rendered_key = render_template(signal_key, variables);
    let rendered_value = render_template(expected_value, variables);
    let actual_value = result.signals.get(&rendered_key).ok_or_else(|| {
      format!(
        "step {step_id:?} signals did not include required key {rendered_key:?}: {}",
        render_signal_marker_summary(&available_markers),
      )
    })?;
    if actual_value != &rendered_value {
      return Err(format!(
        "step {step_id:?} signal {rendered_key:?} expected exact value {rendered_value:?}, got {actual_value:?}",
      ));
    }
  }
  for (signal_key, expected_fragment) in &step.expect.signal_contains {
    let rendered_key = render_template(signal_key, variables);
    let rendered_fragment = render_template(expected_fragment, variables);
    let actual_value = result.signals.get(&rendered_key).ok_or_else(|| {
      format!(
        "step {step_id:?} signals did not include required key {rendered_key:?}: {}",
        render_signal_marker_summary(&available_markers),
      )
    })?;
    if !actual_value.contains(&rendered_fragment) {
      return Err(format!(
        "step {step_id:?} signal {rendered_key:?} did not contain required fragment {rendered_fragment:?}: {actual_value:?}",
      ));
    }
  }
  Ok(())
}

fn render_signal_markers(signals: &BTreeMap<String, String>) -> Vec<String> {
  signals
    .iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

fn render_signal_marker_summary(markers: &[String]) -> String {
  if markers.is_empty() {
    "no structured signals".to_string()
  } else {
    markers.join(", ")
  }
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

pub(crate) fn step_id(step: &SkillStep, fallback_index: usize) -> String {
  if step.id.is_empty() {
    format!("step-{}", fallback_index + 1)
  } else {
    step.id.clone()
  }
}

#[cfg(test)]
mod tests {
  use std::collections::{BTreeMap, BTreeSet};
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use serde_json::json;

  use super::case_matrix::run_skill_case_matrix_recorded;
  use super::{
    RecipeRunReporter, RecipeStartedReport, RecipeStepReport, SkillCaseMatrix,
    SkillCaseMatrixCatalog, SkillCatalog, SkillManifest, SkillRecipe, SkillRecipeOrigin,
    SkillRecipeRunner, SkillRunOptions, SkillRunSummary, SkillStep,
    build_inline_scan_hook_manifest, default_inputs, enforce_step_expectations,
    export_step_variables, is_image_artifact, render_template, render_value,
    run_skill_manifest_into_run, run_skill_manifest_recorded, sanitize_lock_component,
    validate_case_matrix_against_skill, validate_case_matrix_manifest, validate_skill_manifest,
    validate_skill_manifest_with_commands,
  };
  use crate::catalog::{CommandCatalog, default_command_catalog};
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::{
    AuvResult, CommandSpec, DriverCall, DriverDescriptor, DriverResponse, InvokeRequest,
    InvokeResult, RunStatus, now_millis,
  };
  use crate::store::LocalStore;
  use crate::trace::{RunId, SpanId};

  struct SkillSuccessDriver;

  impl Driver for SkillSuccessDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.skill.driver",
        summary: "Test skill driver",
        capabilities: &["test.skill"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
      let mut signals = BTreeMap::from([
        ("outcome".to_string(), "ok".to_string()),
        ("query".to_string(), "driver query".to_string()),
        (
          "step_first_run_id".to_string(),
          "driver overwritten run id".to_string(),
        ),
        ("last.scan.hook.action".to_string(), "continue".to_string()),
        (
          "last.scan.hook.reason".to_string(),
          "test driver signal".to_string(),
        ),
        (
          "last.scan.hook.decision".to_string(),
          serde_json::json!({
            "hook_name": "per_list_item_candidate",
            "page_index": 0,
            "action": "continue",
            "reason": "test driver structured signal",
            "annotations": ["structured fixture annotation"],
            "evidence": ["artifacts/fixture-overlay.json"]
          })
          .to_string(),
        ),
      ]);
      if let Some(context) = call.inputs.get("context") {
        signals.insert("echo.context".to_string(), context.clone());
      }

      Ok(DriverResponse {
        summary: format!("ok {}", call.operation),
        backend: Some("test.backend".to_string()),
        signals,
        notes: vec!["outcome=ok".to_string()],
        artifacts: vec![],
      })
    }
  }

  #[derive(Clone, Default)]
  struct CapturingRecipeRunReporter {
    events: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
  }

  impl CapturingRecipeRunReporter {
    fn events(&self) -> Vec<String> {
      self
        .events
        .lock()
        .expect("events lock should not be poisoned")
        .clone()
    }
  }

  impl RecipeRunReporter for CapturingRecipeRunReporter {
    fn recipe_started(&self, event: RecipeStartedReport) {
      self
        .events
        .lock()
        .expect("events lock should not be poisoned")
        .push(format!("recipe_started:{}", event.recipe_id));
    }

    fn step_started(&self, step_id: &str, _request: &InvokeRequest, _step: RecipeStepReport) {
      self
        .events
        .lock()
        .expect("events lock should not be poisoned")
        .push(format!("step_started:{step_id}"));
    }

    fn step_finished(&self, step_id: &str, _result: &InvokeResult) {
      self
        .events
        .lock()
        .expect("events lock should not be poisoned")
        .push(format!("step_finished:{step_id}"));
    }

    fn recipe_finished(&self, recipe_id: &str, _run_id: &RunId) {
      self
        .events
        .lock()
        .expect("events lock should not be poisoned")
        .push(format!("recipe_finished:{recipe_id}"));
    }
  }

  #[test]
  fn default_inputs_extract_scalar_defaults() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
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
  fn skill_manifest_parses_sub_recipe_invocation_contract() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "scan.fixture.list_item_candidate_continue.v0",
      "version": "0.1.0",
      "target_app": { "bundle_id": "fixture://scan-hook", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "invocation": {
        "kind": "sub_recipe",
        "host": "scroll_scan",
        "stage": "per_list_item_candidate",
        "context_schema": "auv.scan.list_item_candidate.scalar_context.v0",
        "return_schema": "auv.scan.hook_decision.v0"
      },
      "objective": "test",
      "steps": []
    }))
    .expect("manifest should deserialize");

    assert_eq!(manifest.invocation.kind, "sub_recipe");
    assert_eq!(manifest.invocation.host, "scroll_scan");
    assert_eq!(manifest.invocation.stage, "per_list_item_candidate");
  }

  #[test]
  fn skill_manifest_parses_inline_scan_hook_block() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.inline-hook.parent",
      "version": "0.1.0",
      "target_app": { "bundle_id": "fixture://scan-hook", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "capture",
        "command_id": "debug.captureDisplay",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }],
      "hooks": {
        "per_list_item_candidate": {
          "input_schema": "auv.scan.list_item_candidate_context.v0",
          "return_schema": "auv.scan.hook_decision.v0",
          "steps": [{
            "id": "return-hook-decision",
            "command_id": "debug.fixtureObserve",
            "disturbance": {
              "classes": ["none"],
              "max": "none"
            },
            "args": {
              "target": "fixture://scan-hook",
              "hook_action": "continue",
              "hook_reason": "inline hook continued"
            }
          }]
        }
      },
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let hook = manifest
      .hooks
      .get("per_list_item_candidate")
      .expect("hook should parse");
    assert_eq!(hook.input_schema, "auv.scan.list_item_candidate_context.v0");
    assert_eq!(hook.return_schema, "auv.scan.hook_decision.v0");
    assert_eq!(hook.steps.len(), 1);
    assert_eq!(hook.steps[0].command_id, "debug.fixtureObserve");
  }

  #[test]
  fn build_inline_scan_hook_manifest_synthesizes_sub_recipe_contract() {
    let manifest = inline_hook_parent_manifest();

    let hook_manifest = build_inline_scan_hook_manifest(&manifest, "per_list_item_candidate")
      .expect("hook synthesis should succeed")
      .expect("hook manifest should exist");

    assert_eq!(
      hook_manifest.recipe_id,
      "test.inline-hook.parent.hook.per_list_item_candidate"
    );
    assert_eq!(hook_manifest.invocation.kind, "sub_recipe");
    assert_eq!(hook_manifest.invocation.host, "scroll_scan");
    assert_eq!(hook_manifest.invocation.stage, "per_list_item_candidate");
    assert_eq!(
      hook_manifest.invocation.context_schema,
      "auv.scan.list_item_candidate_context.v0"
    );
    assert_eq!(
      hook_manifest.invocation.return_schema,
      "auv.scan.hook_decision.v0"
    );
    assert_eq!(hook_manifest.steps.len(), 1);
    validate_skill_manifest_with_commands(&hook_manifest, default_command_catalog().all())
      .expect("synthetic hook manifest should validate");
  }

  #[test]
  fn validate_skill_manifest_rejects_inline_hook_with_unknown_stage() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.inline-hook.parent",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture://scan-hook", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "capture",
        "command_id": "debug.captureDisplay",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }],
      "hooks": {
        "before_scan": {
          "input_schema": "auv.scan.list_item_candidate_context.v0",
          "return_schema": "auv.scan.hook_decision.v0",
          "steps": [{
            "id": "return-hook-decision",
            "command_id": "debug.fixtureObserve",
            "disturbance": {
              "classes": ["none"],
              "max": "none"
            }
          }]
        }
      },
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error =
      validate_skill_manifest(&manifest).expect_err("unknown inline hook stage should fail");
    assert!(error.contains("unsupported inline hook"));
    assert!(error.contains("before_scan"));
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
    let mut top_level_signal_exports = BTreeSet::new();
    export_step_variables(
      "capture-evidence",
      &InvokeResult {
        run_id: "run_1".to_string(),
        producer_span_id: SpanId::new("0000000000000001"),
        status: RunStatus::Completed,
        output_summary: "ok".to_string(),
        signals: BTreeMap::new(),
        artifacts: vec![],
        artifact_paths: vec![
          PathBuf::from("/tmp/report.txt"),
          PathBuf::from("/tmp/evidence.png"),
        ],
        failure_message: None,
      },
      &mut variables,
      &mut top_level_signal_exports,
    );

    assert_eq!(
      variables
        .get("step_capture_evidence_artifact_image_0")
        .expect("image artifact should export"),
      "/tmp/evidence.png"
    );
  }

  #[test]
  fn skill_run_summary_exposes_exported_variables() {
    let summary = SkillRunSummary {
      exported_variables: BTreeMap::from([(
        "last.scan.hook.action".to_string(),
        "continue".to_string(),
      )]),
    };

    assert_eq!(
      summary.exported_variables.get("last.scan.hook.action"),
      Some(&"continue".to_string())
    );
  }

  #[test]
  fn run_skill_manifest_summary_includes_signal_exported_variables() {
    let project_root = temp_dir("summary-signal-project");
    let store_root = temp_dir("summary-signal-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest = two_step_manifest();
    let mut run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let root = run.root_span();

    let summary = run_skill_manifest_into_run(
      &runtime,
      &mut run,
      &root,
      &manifest,
      SkillRunOptions {
        dry_run: false,
        max_disturbance: None,
        overrides: BTreeMap::new(),
        quiet: false,
      },
    )
    .expect("skill should run");

    assert_eq!(
      summary.exported_variables.get("last.scan.hook.action"),
      Some(&"continue".to_string())
    );
    assert_eq!(
      summary.exported_variables.get("last.scan.hook.reason"),
      Some(&"test driver signal".to_string())
    );
    assert!(
      summary
        .exported_variables
        .contains_key("last.scan.hook.decision")
    );
    assert_eq!(
      summary.exported_variables.get("query"),
      Some(&"default query".to_string())
    );
    assert_ne!(
      summary.exported_variables.get("step_first_run_id"),
      Some(&"driver overwritten run id".to_string())
    );
    assert_eq!(
      summary.exported_variables.get("step_first_signal_query"),
      Some(&"driver query".to_string())
    );
    assert_eq!(
      summary
        .exported_variables
        .get("step_first_signal_step_first_run_id"),
      Some(&"driver overwritten run id".to_string())
    );
    assert!(
      summary
        .exported_variables
        .contains_key("step_first_signal_last_scan_hook_decision")
    );

    let _ = runtime.finish_run(
      run,
      crate::run_builder::RunFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some("test finished".to_string()),
        failure: None,
      },
    );
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn later_step_can_consume_exported_signal_template_variable() {
    let project_root = temp_dir("signal-template-project");
    let store_root = temp_dir("signal-template-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.signal.template.flow",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture.app", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "prove step signal export can be consumed by later args",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }, {
        "id": "second",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "args": {
          "context": "${step_first_signal_last_scan_hook_decision}"
        },
        "expect": {
          "signal_contains": {
            "echo.context": "\"hook_name\":\"per_list_item_candidate\""
          }
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");
    let mut run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let root = run.root_span();

    let summary = run_skill_manifest_into_run(
      &runtime,
      &mut run,
      &root,
      &manifest,
      SkillRunOptions {
        dry_run: false,
        max_disturbance: None,
        overrides: BTreeMap::new(),
        quiet: false,
      },
    )
    .expect("skill should run");

    assert!(
      summary
        .exported_variables
        .contains_key("step_first_signal_last_scan_hook_decision")
    );

    let _ = runtime.finish_run(
      run,
      crate::run_builder::RunFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some("test finished".to_string()),
        failure: None,
      },
    );
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn qqmusic_play_search_result_candidate_recipe_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/qqmusic/play-search-result-candidate.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
  }

  #[test]
  fn qqmusic_play_search_result_candidate_case_matrix_aligns_with_recipe() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let recipe_path = root.join("recipes/macos/qqmusic/play-search-result-candidate.v0.json");
    let matrix_path = root.join("recipes/macos/qqmusic/play-search-result-candidate.cases.v0.json");
    let recipe_raw = fs::read_to_string(&recipe_path).expect("recipe file should read");
    let matrix_raw = fs::read_to_string(&matrix_path).expect("matrix file should read");
    let manifest: SkillManifest = serde_json::from_str(&recipe_raw).expect("recipe should parse");
    let matrix: SkillCaseMatrix = serde_json::from_str(&matrix_raw).expect("matrix should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    validate_case_matrix_manifest(&matrix).expect("matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("matrix should align");
  }

  #[test]
  fn qqmusic_music_result_play_recipe_requires_structured_candidate_ref_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/qqmusic/music.result.play.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "play-candidate")
      .expect("play-candidate step should exist");
    assert_eq!(
      step.expect.signal_equals.get("candidate.input_mode"),
      Some(&"candidate_ref".to_string())
    );
  }

  #[test]
  fn qqmusic_open_search_submit_query_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/qqmusic/open-search-submit-query.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-search-input")
      .expect("focus-search-input step should exist");
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn qqmusic_select_result_anchor_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/qqmusic/select-result-anchor.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-search-input")
      .expect("focus-search-input step should exist");
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn textedit_create_and_verify_text_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/textedit/create-and-verify-text.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-body")
      .expect("focus-body step should exist");
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn notes_create_and_verify_note_v0_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/notes/create-and-verify-note.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-body")
      .expect("focus-body step should exist");
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn notes_create_and_verify_note_v1_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/notes/create-and-verify-note.v1.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-body")
      .expect("focus-body step should exist");
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn notes_create_and_verify_note_v2_recipe_requires_ax_focus_contract() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/notes/create-and-verify-note.v2.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-body")
      .expect("focus-body step should exist");
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
    assert_eq!(
      step.expect.signal_equals.get("cursorDisturbance"),
      Some(&"none".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("focusMechanism"),
      Some(&"ax-attribute".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("setAttribute"),
      Some(&"AXFocused".to_string())
    );
  }

  #[test]
  fn qqmusic_search_ocr_anchor_recipe_requires_query_focus_consumer() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/qqmusic/search-ocr-anchor.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-search-input")
      .expect("focus-search-input step should exist");
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn netease_play_visible_anchor_recipe_requires_expected_consumer_signals() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/netease-cloud-music/play-visible-anchor.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let click_search_box = manifest
      .steps
      .iter()
      .find(|step| step.id == "click-search-box")
      .expect("click-search-box step should exist");
    assert_eq!(
      click_search_box
        .expect
        .signal_equals
        .get("clickWindowPoint.consumer"),
      Some(&"relative-point".to_string())
    );
    let double_click_result = manifest
      .steps
      .iter()
      .find(|step| step.id == "double-click-result")
      .expect("double-click-result step should exist");
    assert_eq!(
      double_click_result
        .expect
        .signal_equals
        .get("clickWindowText.consumer"),
      Some(&"query".to_string())
    );
  }

  #[test]
  fn dual_cursor_press_notes_recipe_requires_ax_focus_overlay_contract() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("recipes/macos/demo/dual-cursor-press-notes.v0.json");
    let raw = fs::read_to_string(&path).expect("recipe file should read");
    let manifest: SkillManifest = serde_json::from_str(&raw).expect("recipe should parse");

    validate_skill_manifest(&manifest).expect("recipe should validate");
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-body")
      .expect("focus-body step should exist");
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
    assert_eq!(
      step.expect.signal_equals.get("cursorDisturbance"),
      Some(&"none".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("focusMechanism"),
      Some(&"ax-attribute".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("setAttribute"),
      Some(&"AXFocused".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("overlayPresentation"),
      Some(&"dual-cursor-visual-only".to_string())
    );
    assert_eq!(
      step.expect.signal_equals.get("dualCursor"),
      Some(&"true".to_string())
    );
  }

  #[test]
  fn enforce_step_expectations_reads_structured_signals() {
    let step: SkillStep = serde_json::from_value(json!({
      "id": "verify-text",
      "command_id": "test.skill.invoke",
      "disturbance": {
        "classes": ["none"],
        "max": "none"
      },
      "expect": {
        "output_must_contain": [
          "targetText=${target_text}",
          "matchedRole=AXTextArea"
        ],
        "output_must_not_contain": [
          "timedOut=true"
        ],
        "artifact_count_at_least": 1
      }
    }))
    .expect("step should deserialize");
    let result = InvokeResult {
      run_id: "run_2".to_string(),
      producer_span_id: SpanId::new("0000000000000002"),
      status: RunStatus::Completed,
      output_summary: "human summary only".to_string(),
      signals: BTreeMap::from([
        ("targetText".to_string(), "hello".to_string()),
        ("matchedRole".to_string(), "AXTextArea".to_string()),
        ("timedOut".to_string(), "false".to_string()),
      ]),
      artifacts: vec![],
      artifact_paths: vec![PathBuf::from("/tmp/evidence.txt")],
      failure_message: None,
    };

    enforce_step_expectations(
      "verify-text",
      &step,
      &result,
      &BTreeMap::from([("target_text".to_string(), "hello".to_string())]),
    )
    .expect("signals should satisfy expectation");
  }

  #[test]
  fn enforce_step_expectations_no_longer_falls_back_to_output_summary() {
    let step: SkillStep = serde_json::from_value(json!({
      "id": "verify-text",
      "command_id": "test.skill.invoke",
      "disturbance": {
        "classes": ["none"],
        "max": "none"
      },
      "expect": {
        "output_must_contain": ["targetText=hello"]
      }
    }))
    .expect("step should deserialize");
    let error = enforce_step_expectations(
      "verify-text",
      &step,
      &InvokeResult {
        run_id: "run_3".to_string(),
        producer_span_id: SpanId::new("0000000000000003"),
        status: RunStatus::Completed,
        output_summary: "targetText=hello".to_string(),
        signals: BTreeMap::new(),
        artifacts: vec![],
        artifact_paths: vec![],
        failure_message: None,
      },
      &BTreeMap::new(),
    )
    .expect_err("summary-only markers should no longer pass");

    assert!(error.contains("no structured signals"));
  }

  #[test]
  fn enforce_step_expectations_supports_signal_equals_and_contains() {
    let step: SkillStep = serde_json::from_value(json!({
      "id": "verify-text",
      "command_id": "test.skill.invoke",
      "disturbance": {
        "classes": ["none"],
        "max": "none"
      },
      "expect": {
        "signal_equals": {
          "clipboard.restored": "true"
        },
        "signal_contains": {
          "ax.matched_text": "${target_text}"
        }
      }
    }))
    .expect("step should deserialize");
    let result = InvokeResult {
      run_id: "run_4".to_string(),
      producer_span_id: SpanId::new("0000000000000004"),
      status: RunStatus::Completed,
      output_summary: "human summary only".to_string(),
      signals: BTreeMap::from([
        ("clipboard.restored".to_string(), "true".to_string()),
        (
          "ax.matched_text".to_string(),
          "prefix hello suffix".to_string(),
        ),
      ]),
      artifacts: vec![],
      artifact_paths: vec![],
      failure_message: None,
    };

    enforce_step_expectations(
      "verify-text",
      &step,
      &result,
      &BTreeMap::from([("target_text".to_string(), "hello".to_string())]),
    )
    .expect("signal checks should pass");
  }

  #[test]
  fn enforce_step_expectations_reports_missing_signal_key() {
    let step: SkillStep = serde_json::from_value(json!({
      "id": "verify-text",
      "command_id": "test.skill.invoke",
      "disturbance": {
        "classes": ["none"],
        "max": "none"
      },
      "expect": {
        "signal_equals": {
          "ax.node_found": "true"
        }
      }
    }))
    .expect("step should deserialize");
    let error = enforce_step_expectations(
      "verify-text",
      &step,
      &InvokeResult {
        run_id: "run_5".to_string(),
        producer_span_id: SpanId::new("0000000000000005"),
        status: RunStatus::Completed,
        output_summary: String::new(),
        signals: BTreeMap::from([("clipboard.restored".to_string(), "true".to_string())]),
        artifacts: vec![],
        artifact_paths: vec![],
        failure_message: None,
      },
      &BTreeMap::new(),
    )
    .expect_err("missing signal should fail");

    assert!(error.contains("ax.node_found"));
  }

  #[test]
  fn failed_recipe_step_finishes_step_span_with_error() {
    let project_root = temp_dir("failed-step-project");
    let store_root = temp_dir("failed-step-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let mut manifest = two_step_manifest();
    manifest.steps[0].expect.output_must_contain = vec!["missing-marker".to_string()];
    let mut run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let root = run.root_span();

    let error = run_skill_manifest_into_run(
      &runtime,
      &mut run,
      &root,
      &manifest,
      SkillRunOptions {
        dry_run: false,
        max_disturbance: None,
        overrides: BTreeMap::new(),
        quiet: true,
      },
    )
    .expect_err("missing marker should fail the step");

    assert!(error.contains("missing-marker"));
    let run_id = runtime
      .finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: crate::trace::TraceStatusCode::Error,
          summary: Some("test failed as expected".to_string()),
          failure: Some(error),
        },
      )
      .expect("run should finish");
    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    let step_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.recipe.step")
      .expect("step span should exist");
    assert_eq!(step_span.status_code, crate::trace::TraceStatusCode::Error);
    assert!(step_span.failure.is_some());

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn skill_recipe_runs_with_runner_from_any_manifest_source() {
    let project_root = temp_dir("recipe-object-project");
    let store_root = temp_dir("recipe-object-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let recipe = SkillRecipe::from_manifest(two_step_manifest(), SkillRecipeOrigin::Inline);

    let run_id = recipe
      .run_with(
        &SkillRecipeRunner::new(&runtime),
        SkillRunOptions {
          dry_run: false,
          max_disturbance: None,
          overrides: BTreeMap::new(),
          quiet: true,
        },
      )
      .expect("recipe object should run");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(
      canonical.run.attributes.get("auv.recipe.id"),
      Some(&json!("test.recorded.skill"))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn skill_recipe_runner_reports_recipe_lifecycle_and_records_trace() {
    let project_root = temp_dir("runner-skill-project");
    let store_root = temp_dir("runner-skill-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest = two_step_manifest();
    let reporter = CapturingRecipeRunReporter::default();

    let run_id = SkillRecipeRunner::new(&runtime)
      .with_reporter(Box::new(reporter.clone()))
      .run_manifest(
        &manifest,
        SkillRunOptions {
          dry_run: false,
          max_disturbance: None,
          overrides: BTreeMap::new(),
          quiet: true,
        },
      )
      .expect("runner should execute recipe");

    assert_eq!(
      reporter.events(),
      vec![
        "recipe_started:test.recorded.skill",
        "step_started:first",
        "step_finished:first",
        "step_started:second",
        "step_finished:second",
        "recipe_finished:test.recorded.skill",
      ]
    );

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
    assert_eq!(canonical.spans[0].name, "auv.execute");
    assert_eq!(
      canonical
        .spans
        .iter()
        .filter(|span| span.name == "auv.recipe.step")
        .count(),
      2
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_skill_manifest_records_one_execute_run() {
    let project_root = temp_dir("recorded-skill-project");
    let store_root = temp_dir("recorded-skill-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest = two_step_manifest();

    let run_id = run_skill_manifest_recorded(
      &runtime,
      &manifest,
      SkillRunOptions {
        dry_run: false,
        max_disturbance: None,
        overrides: BTreeMap::new(),
        quiet: false,
      },
    )
    .expect("recorded skill should run");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
    assert_eq!(canonical.spans[0].name, "auv.execute");
    assert_eq!(
      canonical
        .spans
        .iter()
        .filter(|span| span.name == "auv.recipe.step")
        .count(),
      2
    );
    let first_step_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.recipe.step")
      .expect("first recipe step span should be recorded");
    assert_eq!(
      first_step_span.attributes.get("auv.step.id"),
      Some(&json!("first"))
    );
    assert_eq!(
      first_step_span.attributes.get("auv.step.index"),
      Some(&json!(0))
    );
    assert_eq!(
      first_step_span.attributes.get("auv.step.kind"),
      Some(&json!("recipe"))
    );
    assert_eq!(
      first_step_span.attributes.get("auv.recipe.id"),
      Some(&json!(manifest.recipe_id))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_skill_case_matrix_records_validate_run_with_nested_execute() {
    let project_root = temp_dir("recorded-case-project");
    let store_root = temp_dir("recorded-case-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest = two_step_manifest();
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.recorded.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {},
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    let run_id = run_skill_case_matrix_recorded(
      &runtime,
      &manifest,
      &matrix,
      super::SkillCaseRunOptions::default(),
    )
    .expect("recorded matrix should run");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(canonical.run.run_type, crate::trace::RunType::Validate);
    assert_eq!(canonical.spans[0].name, "auv.validate");
    assert!(canonical.spans.iter().any(|span| span.name == "auv.case"));
    let execute_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.execute")
      .expect("execute span should exist");
    assert!(
      canonical
        .spans
        .iter()
        .filter(|span| span.name == "auv.recipe.step")
        .all(|span| span.parent_span_id.as_ref() == Some(&execute_span.span_id))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_skill_case_matrix_summary_keeps_last_case_exported_variables() {
    let project_root = temp_dir("recorded-case-summary-project");
    let store_root = temp_dir("recorded-case-summary-store");
    let runtime = skill_test_runtime(project_root.clone(), store_root.clone());
    let manifest = two_step_manifest();
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.recorded.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {},
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");
    let mut run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Validate,
        "auv.validate",
      ))
      .expect("run should start");
    let root = run.root_span();

    let summary = super::case_matrix::run_skill_case_matrix_into_run(
      &runtime,
      &mut run,
      &root,
      &manifest,
      &matrix,
      super::SkillCaseRunOptions::default(),
    )
    .expect("case matrix should run");

    assert_eq!(summary.selected_case_count, 1);
    assert_eq!(
      summary.exported_variables.get("step_first_signal_query"),
      Some(&"driver query".to_string())
    );
    assert!(
      summary
        .exported_variables
        .contains_key("step_first_signal_last_scan_hook_decision")
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
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
        "strategy": {
          "family": "native-text",
          "grounding": "ax-text",
          "activation": "pointer-focus-clipboard-paste",
          "verificationContract": "verifyAxText"
        },
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
        "strategy": {
          "family": "native-text",
          "grounding": "ax-text",
          "activation": "pointer-focus-clipboard-paste",
          "verificationContract": "verifyAxText"
        },
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
        "status": "active-case-matrix",
        "cases": [{
          "case_id": "baseline",
          "status": "validated",
          "disturbance": "none"
        }]
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
      sanitize_lock_component("com.example.music"),
      "com-example-music"
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
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["none", "pointer"]
      },
      "steps": [{
        "id": "step-1",
        "command_id": "debug.captureDisplay",
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
  fn validate_skill_manifest_accepts_window_action_smart_press_taxonomy() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.smart-press.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "window-action",
        "grounding": "window-point",
        "activation": "smart-press",
        "verificationContract": "captureEvidence"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "Run" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["foreground_app", "pointer", "none"]
      },
      "steps": [{
        "id": "smart-press",
        "command_id": "debug.smartPress",
        "disturbance": {
          "classes": ["foreground_app", "pointer"],
          "max": "pointer"
        },
        "args": {
          "target": "app",
          "query": "${query}",
          "overlay": "true",
          "allow_pointer_fallback": "true"
        },
        "mainline_exemption": {
          "reason": "Phase 3 #5 cross-app smartPress discovery vehicle; recipe lives outside macos.demo.* for taxonomy-test coverage.",
          "category": "discovery"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    validate_skill_manifest(&manifest).expect("smart-press manifest should validate");
  }

  fn smart_press_manifest_template(
    recipe_id: &str,
    exemption: Option<serde_json::Value>,
  ) -> SkillManifest {
    let mut step = json!({
      "id": "smart-press",
      "command_id": "debug.smartPress",
      "disturbance": {
        "classes": ["foreground_app", "pointer"],
        "max": "pointer"
      },
      "args": {
        "target": "app",
        "query": "${query}",
        "overlay": "true"
      }
    });
    if let Some(exemption) = exemption {
      step
        .as_object_mut()
        .expect("step should be object")
        .insert("mainline_exemption".to_string(), exemption);
    }
    serde_json::from_value(json!({
      "recipe_id": recipe_id,
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "window-action",
        "grounding": "window-point",
        "activation": "smart-press",
        "verificationContract": "captureEvidence"
      },
      "objective": "test",
      "inputs": { "query": { "type": "string", "default": "Run" } },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["foreground_app", "pointer", "none"]
      },
      "steps": [step],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize")
  }

  #[test]
  fn validate_skill_manifest_rejects_smart_press_in_product_namespace_without_exemption() {
    let manifest = smart_press_manifest_template("macos.qqmusic.play.v9", None);
    let error = validate_skill_manifest(&manifest)
      .expect_err("smart-press in product namespace without exemption should fail");
    assert!(
      error.contains("mainline_exemption"),
      "unexpected error: {error}"
    );
    assert!(error.contains("rule 1"), "unexpected error: {error}");
  }

  #[test]
  fn validate_skill_manifest_accepts_smart_press_with_explicit_exemption() {
    let manifest = smart_press_manifest_template(
      "macos.qqmusic.play.v9",
      Some(json!({
        "reason": "Phase 3 #6 controlled experiment on whether QQ音乐 play control is AX-pressable",
        "category": "experiment"
      })),
    );
    validate_skill_manifest(&manifest)
      .expect("smart-press in product namespace with valid exemption should validate");
  }

  #[test]
  fn validate_skill_manifest_rejects_smart_press_exemption_with_unknown_category() {
    let manifest = smart_press_manifest_template(
      "macos.qqmusic.play.v9",
      Some(json!({
        "reason": "this should fail because the category is not in the allow-list",
        "category": "because-i-said-so"
      })),
    );
    let error =
      validate_skill_manifest(&manifest).expect_err("unknown exemption category should fail");
    assert!(error.contains("category"), "unexpected error: {error}");
    assert!(error.contains("rule 1"), "unexpected error: {error}");
  }

  #[test]
  fn validate_case_matrix_rejects_validated_case_on_smart_press_recipe() {
    let manifest = smart_press_manifest_template("macos.demo.smart.v9", None);
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "macos.demo.smart.v9",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "demo-validated",
        "status": "validated",
        "inputs": { "query": "Run" },
        "disturbance": "pointer"
      }]
    }))
    .expect("matrix should deserialize");
    let error = validate_case_matrix_against_skill(&manifest, &matrix)
      .expect_err("validated case on smart-press recipe should fail");
    assert!(error.contains("rule 2"), "unexpected error: {error}");
    assert!(
      error.contains("debug.smartPress"),
      "unexpected error: {error}"
    );
  }

  #[test]
  fn validate_case_matrix_accepts_candidate_case_on_smart_press_recipe() {
    let manifest = smart_press_manifest_template("macos.demo.smart.v9", None);
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "macos.demo.smart.v9",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "demo-candidate",
        "status": "candidate",
        "inputs": { "query": "Run" },
        "disturbance": "pointer"
      }]
    }))
    .expect("matrix should deserialize");
    validate_case_matrix_against_skill(&manifest, &matrix)
      .expect("candidate case on smart-press recipe should validate");
  }

  #[test]
  fn validate_skill_manifest_rejects_unknown_command_id() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
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
        "command_id": "debug.doesNotExist",
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

    let catalog = default_command_catalog();
    let error = validate_skill_manifest_with_commands(&manifest, catalog.all())
      .expect_err("unknown command should fail");
    assert!(error.contains("unknown command_id"));
  }

  #[test]
  fn validate_skill_manifest_rejects_step_disturbance_above_command_max() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
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
        "command_id": "debug.listWindows",
        "disturbance": {
          "classes": ["pointer"],
          "max": "pointer"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let catalog = default_command_catalog();
    let error = validate_skill_manifest_with_commands(&manifest, catalog.all())
      .expect_err("step disturbance above command max should fail");
    assert!(error.contains("above command"));
  }

  #[test]
  fn validate_skill_manifest_rejects_step_class_not_declared_by_command() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "window-action",
        "grounding": "window-point",
        "activation": "smart-press",
        "verificationContract": "captureEvidence"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "Run" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["foreground_app", "keyboard", "pointer"]
      },
      "steps": [{
        "id": "smart-press",
        "command_id": "debug.smartPress",
        "disturbance": {
          "classes": ["foreground_app", "keyboard", "pointer"],
          "max": "pointer"
        },
        "args": {
          "target": "app",
          "query": "${query}"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let catalog = default_command_catalog();
    let error = validate_skill_manifest_with_commands(&manifest, catalog.all())
      .expect_err("step class not declared by command should fail");
    assert!(error.contains("not declared by command"));
    assert!(error.contains("debug.smartPress"));
    assert!(error.contains("keyboard"));
  }

  #[test]
  fn validate_skill_manifest_rejects_hidden_foreground_capture_disturbance_class() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "foreground_app",
        "declared_classes": ["none", "foreground_app"]
      },
      "steps": [{
        "id": "capture",
        "command_id": "debug.captureDisplay",
        "disturbance": {
          "classes": ["none"],
          "max": "foreground_app"
        },
        "args": {
          "target": "app",
          "activate_target_before_capture": true
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error = validate_skill_manifest(&manifest)
      .expect_err("hidden foreground capture disturbance should fail");
    assert!(error.contains("activate_target_before_capture=true"));
    assert!(error.contains("foreground_app"));
  }

  #[test]
  fn validate_skill_manifest_rejects_hidden_foreground_capture_disturbance_max() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "foreground_app",
        "declared_classes": ["none", "foreground_app"]
      },
      "steps": [{
        "id": "capture",
        "command_id": "debug.captureDisplay",
        "disturbance": {
          "classes": ["foreground_app"],
          "max": "none"
        },
        "args": {
          "target": "app",
          "activate_target_before_capture": "true"
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error = validate_skill_manifest(&manifest)
      .expect_err("foreground capture disturbance max should fail");
    assert!(error.contains("activate_target_before_capture=true"));
    assert!(error.contains("below foreground_app"));
  }

  #[test]
  fn validate_skill_manifest_rejects_step_disturbance_above_recipe_budget() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "keyboard",
        "declared_classes": ["foreground_app", "keyboard", "pointer"]
      },
      "steps": [{
        "id": "step-1",
        "command_id": "debug.pressButton",
        "disturbance": {
          "classes": ["foreground_app", "keyboard", "pointer"],
          "max": "pointer"
        },
        "args": { "target": "${app_id}", "query": "Run" }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error =
      validate_skill_manifest(&manifest).expect_err("step above recipe budget should fail");
    assert!(
      error.contains("above recipe budget"),
      "unexpected error: {error}"
    );
    assert!(error.contains("pointer"));
    assert!(error.contains("keyboard"));
  }

  #[test]
  fn validate_skill_manifest_rejects_step_class_above_step_max() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["foreground_app", "keyboard", "pointer"]
      },
      "steps": [{
        "id": "step-1",
        "command_id": "debug.pressButton",
        "disturbance": {
          "classes": ["foreground_app", "keyboard", "pointer"],
          "max": "keyboard"
        },
        "args": { "target": "${app_id}", "query": "Run" }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize");

    let error = validate_skill_manifest(&manifest).expect_err("class above step max should fail");
    assert!(
      error.contains("above its own max"),
      "unexpected error: {error}"
    );
  }

  #[test]
  fn validate_skill_manifest_rejects_class_not_in_declared_classes() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
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
        "command_id": "debug.captureDisplay",
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

    let error =
      validate_skill_manifest(&manifest).expect_err("class not in declared_classes should fail");
    assert!(
      error.contains("not declared in disturbance_policy.declared_classes"),
      "unexpected error: {error}"
    );
  }

  #[test]
  fn validate_skill_manifest_rejects_empty_steps() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
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
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "steps": [{
        "command_id": "debug.captureDisplay",
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

  #[test]
  fn validate_case_matrix_manifest_accepts_minimal_valid_matrix() {
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {
          "query": "aa"
        },
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    validate_case_matrix_manifest(&matrix).expect("matrix should validate");
  }

  #[test]
  fn validate_case_matrix_manifest_rejects_duplicate_case_ids() {
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "disturbance": "none"
      }, {
        "case_id": "baseline",
        "status": "validated",
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    let error = validate_case_matrix_manifest(&matrix).expect_err("duplicate ids should fail");
    assert!(error.contains("duplicate case_id"));
  }

  #[test]
  fn validate_case_matrix_against_skill_rejects_unknown_inputs() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string" }
      },
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "command_id": "debug.captureDisplay",
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
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {
          "query": "aa",
          "unknown": "value"
        },
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    let error = validate_case_matrix_against_skill(&manifest, &matrix)
      .expect_err("unknown input should fail");
    assert!(error.contains("unknown input"));
  }

  #[test]
  fn validate_case_matrix_against_skill_rejects_missing_required_inputs() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string" },
        "title": { "type": "string" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["pointer"]
      },
      "steps": [{
        "command_id": "debug.captureDisplay",
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
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "test.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {
          "query": "aa"
        },
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    let error = validate_case_matrix_against_skill(&manifest, &matrix)
      .expect_err("missing required input should fail");
    assert!(error.contains("missing required input"));
  }

  #[test]
  fn validate_case_matrix_against_skill_rejects_mismatched_skill_id() {
    let manifest: SkillManifest = serde_json::from_value(json!({
      "recipe_id": "test.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "app", "display_mode": "live-desktop" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test",
      "inputs": {
        "query": { "type": "string", "default": "aa" }
      },
      "disturbance_policy": {
        "max_disturbance": "pointer",
        "declared_classes": ["pointer"]
      },
      "steps": [{
        "command_id": "debug.captureDisplay",
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
    let matrix: SkillCaseMatrix = serde_json::from_value(json!({
      "skill_id": "other.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {
          "query": "aa"
        },
        "disturbance": "none"
      }]
    }))
    .expect("matrix should deserialize");

    let error = validate_case_matrix_against_skill(&manifest, &matrix)
      .expect_err("mismatched skill id should fail");
    assert!(error.contains("does not match skill"));
  }

  fn two_step_manifest() -> SkillManifest {
    serde_json::from_value(json!({
      "recipe_id": "test.recorded.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture.app", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test recorded skill execution",
      "inputs": {
        "query": { "type": "string", "default": "default query" }
      },
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "expect": {
          "output_must_contain": ["outcome=ok"]
        }
      }, {
        "id": "second",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "expect": {
          "output_must_contain": ["outcome=ok"]
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize")
  }

  fn inline_hook_parent_manifest() -> SkillManifest {
    serde_json::from_value(json!({
      "recipe_id": "test.inline-hook.parent",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture://scan-hook", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test parent manifest with inline hook",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "capture",
        "command_id": "debug.captureDisplay",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        }
      }],
      "hooks": {
        "per_list_item_candidate": {
          "input_schema": "auv.scan.list_item_candidate_context.v0",
          "return_schema": "auv.scan.hook_decision.v0",
          "steps": [{
            "id": "return-hook-decision",
            "command_id": "debug.fixtureObserve",
            "disturbance": {
              "classes": ["none"],
              "max": "none"
            },
            "args": {
              "target": "fixture://scan-hook",
              "hook_action": "continue",
              "hook_reason": "inline hook continued",
              "hook_name": "${scan.hook.name}",
              "hook_stage": "${scan.hook.stage}",
              "hook_page_index": "${scan.page_index}"
            }
          }]
        }
      },
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    }))
    .expect("manifest should deserialize")
  }

  fn skill_test_runtime(project_root: PathBuf, store_root: PathBuf) -> crate::runtime::Runtime {
    crate::runtime::Runtime::new(
      project_root,
      CommandCatalog::new(vec![CommandSpec {
        id: "test.skill.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test skill invoke",
        driver_id: "test.skill.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(SkillSuccessDriver)]),
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }
}
