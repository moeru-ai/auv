use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use crate::catalog::default_command_catalog;
use crate::driver::{clear_stale_lock_file, describe_lock_owner};
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
  run_skill_manifest(runtime, &entry.manifest, options)
}

pub(crate) fn run_skill_manifest(
  runtime: &Runtime,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<()> {
  run_skill_manifest_recorded(runtime, manifest, options).map(|_| ())
}

pub(crate) fn run_skill_manifest_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<RunId> {
  let mut attributes = crate::recording::Attributes::new();
  attributes.insert(
    "auv.recipe.id".to_string(),
    string_attr(manifest.recipe_id.clone()),
  );
  let mut run = runtime.start_run(
    crate::recording::RunSpec::new(crate::trace::RunType::Execute, "auv.execute")
      .with_attributes(attributes),
  )?;
  let root = run.root_span();

  match run_skill_manifest_into_run(runtime, &mut run, &root, manifest, options) {
    Ok(_summary) => runtime.finish_run(
      run,
      crate::recording::RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some(format!("Executed skill {}", manifest.recipe_id)),
        failure: None,
      },
    ),
    Err(error) => finish_failed_recorded_run(
      runtime,
      run,
      error,
      format!("Skill {} failed", manifest.recipe_id),
    ),
  }
}

pub(crate) fn run_skill_manifest_into_run(
  runtime: &Runtime,
  run: &mut crate::recording::RecordingRun,
  parent: &crate::recording::SpanRef,
  manifest: &SkillManifest,
  options: SkillRunOptions,
) -> AuvResult<SkillRunSummary> {
  validate_skill_manifest_with_commands(manifest, runtime.list_commands())?;
  let mut variables = default_inputs(manifest)?;
  for (key, value) in options.overrides {
    variables.insert(key, value);
  }
  let mut top_level_signal_exports = BTreeSet::new();

  let active_max = validate_disturbance_policy(manifest, options.max_disturbance)?;
  let _lock = maybe_acquire_live_app_lock(manifest, &variables, options.dry_run)?;

  if !options.quiet {
    println!("skill: {}", manifest.recipe_id);
    println!("version: {}", manifest.version);
    println!("objective: {}", manifest.objective);
    println!(
      "target: {}",
      render_template(&manifest.target_app.bundle_id, &variables)
    );
    println!("max disturbance: {}", active_max.as_str());
  }

  for (index, step) in manifest.steps.iter().enumerate() {
    let step_id = step_id(step, index);
    let step_span = run.start_span(
      parent,
      span_record(
        "auv.recipe.step",
        BTreeMap::from([
          ("auv.recipe.step_id".to_string(), string_attr(&step_id)),
          ("auv.step.id".to_string(), string_attr(&step_id)),
          ("auv.step.index".to_string(), serde_json::json!(index)),
          ("auv.step.kind".to_string(), string_attr("recipe")),
          (
            "auv.recipe.id".to_string(),
            string_attr(manifest.recipe_id.clone()),
          ),
        ]),
      ),
    )?;

    let step_result = {
      let mut step_context = SkillStepRuntime {
        runtime,
        run,
        step_span: &step_span,
        manifest,
        dry_run: options.dry_run,
        quiet: options.quiet,
        variables: &mut variables,
        top_level_signal_exports: &mut top_level_signal_exports,
      };
      run_skill_step_into_span(&mut step_context, step, index, &step_id)
    };

    match step_result {
      Ok(()) => run.finish_span(
        &step_span,
        crate::recording::SpanFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!("Completed recipe step {step_id}")),
          failure: None,
        },
      )?,
      Err(error) => {
        if let Err(finish_error) = run.finish_span(
          &step_span,
          crate::recording::SpanFinish {
            status_code: TraceStatusCode::Error,
            summary: Some(format!("Recipe step {step_id} failed")),
            failure: Some(error.clone()),
          },
        ) {
          return Err(format!(
            "{error}; additionally failed to finish failed step span: {finish_error}"
          ));
        }
        return Err(error);
      }
    }
  }

  Ok(SkillRunSummary {
    exported_variables: variables,
  })
}

struct SkillStepRuntime<'a> {
  runtime: &'a Runtime,
  run: &'a mut crate::recording::RecordingRun,
  step_span: &'a crate::recording::SpanRef,
  manifest: &'a SkillManifest,
  dry_run: bool,
  quiet: bool,
  variables: &'a mut BTreeMap<String, String>,
  top_level_signal_exports: &'a mut BTreeSet<String>,
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
  if !context.quiet {
    print_step_preview(
      index + 1,
      context.manifest.steps.len(),
      step_id,
      &request,
      step_max,
      &step_classes,
    );
  }

  if context.dry_run {
    return Ok(());
  }

  let result = context
    .runtime
    .invoke_in_span(context.run, context.step_span, request)?;
  if !context.quiet {
    print_invoke_result(&result);
  }
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
  validate_skill_disturbance_budget(manifest)?;
  validate_skill_mainline_compliance(manifest)?;
  validate_skill_verification(manifest)?;
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

pub fn run_skill_case_matrix(
  runtime: &Runtime,
  skill_catalog: &SkillCatalog,
  matrix_entry: &SkillCaseMatrixEntry,
  options: SkillCaseRunOptions,
) -> AuvResult<()> {
  let skill_entry = skill_catalog.resolve_recipe_id(&matrix_entry.matrix.skill_id)?;
  run_skill_case_matrix_inline(
    runtime,
    &skill_entry.manifest,
    &matrix_entry.matrix,
    options,
  )
}

pub(crate) fn run_skill_case_matrix_inline(
  runtime: &Runtime,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<()> {
  run_skill_case_matrix_recorded(runtime, manifest, matrix, options).map(|_| ())
}

pub(crate) fn run_skill_case_matrix_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<RunId> {
  let mut attributes = crate::recording::Attributes::new();
  attributes.insert(
    "auv.case_matrix.skill_id".to_string(),
    string_attr(matrix.skill_id.clone()),
  );
  let mut run = runtime.start_run(
    crate::recording::RunSpec::new(crate::trace::RunType::Validate, "auv.validate")
      .with_attributes(attributes),
  )?;
  let root = run.root_span();

  match run_skill_case_matrix_into_run(runtime, &mut run, &root, manifest, matrix, options) {
    Ok(selected_case_count) => runtime.finish_run(
      run,
      crate::recording::RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some(format!(
          "Validated {} selected case(s) for {}",
          selected_case_count, matrix.skill_id
        )),
        failure: None,
      },
    ),
    Err(error) => finish_failed_recorded_run(
      runtime,
      run,
      error,
      format!("Case matrix {} failed", matrix.skill_id),
    ),
  }
}

pub(crate) fn run_skill_case_matrix_into_run(
  runtime: &Runtime,
  run: &mut crate::recording::RecordingRun,
  parent: &crate::recording::SpanRef,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<usize> {
  validate_case_matrix_against_skill_with_commands(manifest, matrix, runtime.list_commands())?;
  let cases = select_cases(matrix, &options)?;
  let selected_case_count = cases.len();

  println!("case-matrix: {}", matrix.skill_id);
  println!("version: {}", matrix.version);
  if !matrix.status.is_empty() {
    println!("status: {}", matrix.status);
  }
  println!("selected cases: {}", cases.len());

  let mut failures = Vec::new();
  for case in cases {
    println!("case: {} [{}]", case.case_id, case.status);
    let case_span = run.start_span(
      parent,
      span_record(
        "auv.case",
        BTreeMap::from([("auv.case.id".to_string(), string_attr(&case.case_id))]),
      ),
    )?;
    let execute_span = run.start_span(
      &case_span,
      span_record(
        "auv.execute",
        BTreeMap::from([(
          "auv.recipe.id".to_string(),
          string_attr(manifest.recipe_id.clone()),
        )]),
      ),
    )?;

    let case_result = run_skill_manifest_into_run(
      runtime,
      run,
      &execute_span,
      manifest,
      SkillRunOptions {
        dry_run: options.dry_run,
        max_disturbance: options.max_disturbance,
        overrides: case.inputs.clone(),
        quiet: false,
      },
    );

    match case_result {
      Ok(_summary) => {
        run.finish_span(
          &execute_span,
          crate::recording::SpanFinish {
            status_code: TraceStatusCode::Ok,
            summary: Some(format!("Executed skill {}", manifest.recipe_id)),
            failure: None,
          },
        )?;
        run.finish_span(
          &case_span,
          crate::recording::SpanFinish {
            status_code: TraceStatusCode::Ok,
            summary: Some(format!("Case {} passed", case.case_id)),
            failure: None,
          },
        )?;
        println!("case-result: ok");
      }
      Err(error) => {
        let finish_error =
          finish_case_spans_after_error(run, &execute_span, &case_span, manifest, case, &error);
        println!("case-result: failed");
        println!("case-error: {error}");
        failures.push((
          case.case_id.clone(),
          finish_error
            .map(|_| error.clone())
            .unwrap_or_else(|finish_error| format!("{error}; {finish_error}")),
        ));
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

  Ok(selected_case_count)
}

pub fn render_skill_case_matrix_report(
  skill_entry: &SkillCatalogEntry,
  matrix_entry: &SkillCaseMatrixEntry,
) -> AuvResult<String> {
  let command_catalog = default_command_catalog();
  validate_case_matrix_against_skill_with_commands(
    &skill_entry.manifest,
    &matrix_entry.matrix,
    command_catalog.all(),
  )?;

  let manifest = &skill_entry.manifest;
  let matrix = &matrix_entry.matrix;

  let mut by_status = BTreeMap::<String, usize>::new();
  let mut by_disturbance = BTreeMap::<String, usize>::new();
  for case in &matrix.cases {
    *by_status.entry(case.status.clone()).or_insert(0) += 1;
    *by_disturbance.entry(case.disturbance.clone()).or_insert(0) += 1;
  }

  let target_app_display = if manifest.target_app.name.trim().is_empty() {
    manifest.target_app.bundle_id.clone()
  } else if manifest.target_app.bundle_id.trim().is_empty() {
    manifest.target_app.name.clone()
  } else {
    format!(
      "{} ({})",
      manifest.target_app.name.trim(),
      manifest.target_app.bundle_id.trim()
    )
  };

  let mut output = String::new();
  output.push_str(&format!("# Skill Case Report: {}\n\n", matrix.skill_id));
  output.push_str(&format!("- skill version: `{}`\n", manifest.version));
  output.push_str(&format!("- matrix version: `{}`\n", matrix.version));
  output.push_str(&format!("- matrix status: `{}`\n", matrix.status));
  output.push_str(&format!("- target app: `{}`\n", target_app_display));
  output.push_str(&format!(
    "- strategy family: `{}`\n",
    manifest.strategy.family
  ));
  output.push_str(&format!(
    "- strategy grounding: `{}`\n",
    manifest.strategy.grounding
  ));
  output.push_str(&format!(
    "- strategy activation: `{}`\n",
    manifest.strategy.activation
  ));
  output.push_str(&format!(
    "- strategy verification contract: `{}`\n",
    manifest.strategy.verification_contract
  ));
  if let Ok(taxonomy_id) = manifest.strategy.taxonomy_id() {
    output.push_str(&format!("- strategy taxonomy: `{}`\n", taxonomy_id));
  }
  output.push_str(&format!("- objective: {}\n", manifest.objective.trim()));
  output.push_str(&format!(
    "- max disturbance: `{}`\n",
    if manifest.disturbance_policy.max_disturbance.is_empty() {
      "pointer"
    } else {
      &manifest.disturbance_policy.max_disturbance
    }
  ));
  output.push_str(&format!("- case count: `{}`\n\n", matrix.cases.len()));

  output.push_str("## Status Counts\n\n");
  for (status, count) in &by_status {
    output.push_str(&format!("- `{}`: `{}`\n", status, count));
  }
  output.push_str("\n## Disturbance Counts\n\n");
  for (disturbance, count) in &by_disturbance {
    output.push_str(&format!("- `{}`: `{}`\n", disturbance, count));
  }

  output.push_str("\n## Cases\n\n");
  for case in &matrix.cases {
    output.push_str(&format!("### {} [{}]\n\n", case.case_id, case.status));
    output.push_str(&format!("- disturbance: `{}`\n", case.disturbance));
    if !case.inputs.is_empty() {
      output.push_str("- inputs:\n");
      for (key, value) in &case.inputs {
        output.push_str(&format!("  - `{}` = `{}`\n", key, value));
      }
    }
    if !case.notes.is_empty() {
      output.push_str("- notes:\n");
      for note in &case.notes {
        output.push_str(&format!("  - {}\n", note));
      }
    }
    if let (Some(requested_title), Some(target_title)) = (
      case.inputs.get("requested_title"),
      case.inputs.get("target_title"),
    ) && !requested_title.trim().is_empty()
      && requested_title != target_title
    {
      output.push_str("- verification gap:\n");
      output.push_str(&format!("  - requested_title = `{}`\n", requested_title));
      output.push_str(&format!("  - verified_target = `{}`\n", target_title));
      output.push_str(
        "  - this case validates the current activation path, not semantic target-title selection.\n",
      );
    }
    output.push('\n');
  }

  output.push_str("## Verification Contract\n\n");
  output.push_str("- expected signals:\n");
  for signal in &manifest.verification.expected_signals {
    output.push_str(&format!("  - {}\n", signal));
  }
  output.push_str("- success criteria:\n");
  for criterion in &manifest.verification.success_criteria {
    output.push_str(&format!("  - {}\n", criterion));
  }
  output.push_str("- non-goals:\n");
  for non_goal in &manifest.verification.non_goals {
    output.push_str(&format!("  - {}\n", non_goal));
  }

  Ok(output)
}

fn finish_failed_recorded_run(
  runtime: &Runtime,
  run: crate::recording::RecordingRun,
  error: String,
  summary: String,
) -> AuvResult<RunId> {
  if let Err(finish_error) = runtime.finish_run(
    run,
    crate::recording::RunFinish {
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

fn finish_case_spans_after_error(
  run: &mut crate::recording::RecordingRun,
  execute_span: &crate::recording::SpanRef,
  case_span: &crate::recording::SpanRef,
  manifest: &SkillManifest,
  case: &SkillCase,
  error: &str,
) -> AuvResult<()> {
  run.finish_span(
    execute_span,
    crate::recording::SpanFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(format!("Skill {} failed", manifest.recipe_id)),
      failure: Some(error.to_string()),
    },
  )?;
  run.finish_span(
    case_span,
    crate::recording::SpanFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(format!("Case {} failed", case.case_id)),
      failure: Some(error.to_string()),
    },
  )
}

fn span_record(
  name: impl Into<String>,
  attributes: crate::recording::Attributes,
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

fn step_id(step: &SkillStep, fallback_index: usize) -> String {
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

  use super::{
    SkillCaseMatrix, SkillCaseMatrixCatalog, SkillCatalog, SkillManifest, SkillRunOptions,
    SkillRunSummary, SkillStep, default_inputs, enforce_step_expectations, export_step_variables,
    is_image_artifact, render_template, render_value, run_skill_case_matrix_recorded,
    run_skill_manifest_into_run, run_skill_manifest_recorded, sanitize_lock_component,
    validate_case_matrix_against_skill, validate_case_matrix_manifest, validate_skill_manifest,
    validate_skill_manifest_with_commands,
  };
  use crate::catalog::{CommandCatalog, default_command_catalog};
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::{
    AuvResult, CommandSpec, DriverCall, DriverDescriptor, DriverResponse, InvokeResult, RunStatus,
    now_millis,
  };
  use crate::store::LocalStore;

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
      Ok(DriverResponse {
        summary: format!("ok {}", call.operation),
        backend: Some("test.backend".to_string()),
        signals: BTreeMap::from([
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
        ]),
        notes: vec!["outcome=ok".to_string()],
        artifacts: vec![],
      })
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
        status: RunStatus::Completed,
        output_summary: "ok".to_string(),
        signals: BTreeMap::new(),
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
      .start_run(crate::recording::RunSpec::new(
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
        .get("last.scan.hook.decision")
        .is_some()
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
        .get("step_first_signal_last_scan_hook_decision")
        .is_some()
    );

    let _ = runtime.finish_run(
      run,
      crate::recording::RunFinish {
        status_code: crate::trace::TraceStatusCode::Ok,
        summary: Some("test finished".to_string()),
        failure: None,
      },
    );
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
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
      status: RunStatus::Completed,
      output_summary: "human summary only".to_string(),
      signals: BTreeMap::from([
        ("targetText".to_string(), "hello".to_string()),
        ("matchedRole".to_string(), "AXTextArea".to_string()),
        ("timedOut".to_string(), "false".to_string()),
      ]),
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
        status: RunStatus::Completed,
        output_summary: "targetText=hello".to_string(),
        signals: BTreeMap::new(),
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
      status: RunStatus::Completed,
      output_summary: "human summary only".to_string(),
      signals: BTreeMap::from([
        ("clipboard.restored".to_string(), "true".to_string()),
        (
          "ax.matched_text".to_string(),
          "prefix hello suffix".to_string(),
        ),
      ]),
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
        status: RunStatus::Completed,
        output_summary: String::new(),
        signals: BTreeMap::from([("clipboard.restored".to_string(), "true".to_string())]),
        artifact_paths: vec![],
        failure_message: None,
      },
      &BTreeMap::new(),
    )
    .expect_err("missing signal should fail");

    assert!(error.contains("ax.node_found"));
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

  fn skill_test_runtime(project_root: PathBuf, store_root: PathBuf) -> crate::runtime::Runtime {
    crate::runtime::Runtime::new(
      project_root,
      CommandCatalog::new(vec![CommandSpec {
        id: "test.skill.invoke",
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
