use std::path::PathBuf;

use crate::model::AuvResult;
use crate::runtime::Runtime;
use crate::trace::{RunId, TraceStatusCode, string_attr};

#[path = "recipe_observer.rs"]
pub mod observer;

use super::{
  SkillManifest, SkillRunOptions, finish_failed_recorded_run,
  run_skill_manifest_into_run_with_reporter,
};
use observer::{NoopRecipeRunReporter, RecipeRunReporter};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkillRecipeOrigin {
  CatalogPath(PathBuf),
  Inline,
}

#[derive(Clone, Debug)]
pub struct SkillRecipe {
  manifest: SkillManifest,
  origin: SkillRecipeOrigin,
}

impl SkillRecipe {
  pub fn from_manifest(manifest: SkillManifest, origin: SkillRecipeOrigin) -> Self {
    Self { manifest, origin }
  }

  pub fn manifest(&self) -> &SkillManifest {
    &self.manifest
  }

  pub fn origin(&self) -> &SkillRecipeOrigin {
    &self.origin
  }

  pub fn recipe_id(&self) -> &str {
    &self.manifest.recipe_id
  }

  pub fn run_with(
    &self,
    runner: &SkillRecipeRunner<'_>,
    options: SkillRunOptions,
  ) -> AuvResult<RunId> {
    runner.run(self, options)
  }
}

pub struct SkillRecipeRunner<'a> {
  runtime: &'a Runtime,
  reporter: Box<dyn RecipeRunReporter + 'a>,
}

impl<'a> SkillRecipeRunner<'a> {
  pub fn new(runtime: &'a Runtime) -> Self {
    Self {
      runtime,
      reporter: Box::new(NoopRecipeRunReporter),
    }
  }

  pub fn with_reporter(mut self, reporter: Box<dyn RecipeRunReporter + 'a>) -> Self {
    self.reporter = reporter;
    self
  }

  pub fn run_manifest(
    &self,
    manifest: &SkillManifest,
    options: SkillRunOptions,
  ) -> AuvResult<RunId> {
    let recipe = SkillRecipe::from_manifest(manifest.clone(), SkillRecipeOrigin::Inline);
    self.run(&recipe, options)
  }

  pub fn run(&self, recipe: &SkillRecipe, options: SkillRunOptions) -> AuvResult<RunId> {
    let manifest = recipe.manifest();
    let mut trace = RecipeTraceRecorder::start(self.runtime, recipe)?;
    let root = trace.root();
    let result = run_skill_manifest_into_run_with_reporter(
      self.runtime,
      trace.run_mut(),
      &root,
      manifest,
      options,
      self.reporter.as_ref(),
    );

    match result {
      Ok(_summary) => trace.finish_success(manifest, self.reporter.as_ref()),
      Err(error) => trace.finish_failure(manifest, error),
    }
  }
}

struct RecipeTraceRecorder<'a> {
  runtime: &'a Runtime,
  run: crate::run_builder::RecordingRun,
  root: crate::run_builder::SpanRef,
}

impl<'a> RecipeTraceRecorder<'a> {
  fn start(runtime: &'a Runtime, recipe: &SkillRecipe) -> AuvResult<Self> {
    let mut attributes = crate::run_builder::Attributes::new();

    attributes.insert(
      "auv.recipe.id".to_string(),
      string_attr(recipe.recipe_id().to_string()),
    );

    let run = runtime.start_run(
      crate::run_builder::RunSpec::new(crate::trace::RunType::Execute, "auv.execute")
        .with_attributes(attributes),
    )?;
    let root = run.root_span();

    Ok(Self { runtime, run, root })
  }

  fn root(&self) -> crate::run_builder::SpanRef {
    self.root.clone()
  }

  fn run_mut(&mut self) -> &mut crate::run_builder::RecordingRun {
    &mut self.run
  }

  fn finish_success(
    self,
    manifest: &SkillManifest,
    reporter: &dyn RecipeRunReporter,
  ) -> AuvResult<RunId> {
    let run_id = self.runtime.finish_run(
      self.run,
      crate::run_builder::RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some(format!("Executed skill {}", manifest.recipe_id)),
        failure: None,
      },
    )?;

    reporter.recipe_finished(&manifest.recipe_id, &run_id);

    Ok(run_id)
  }

  fn finish_failure(self, manifest: &SkillManifest, error: String) -> AuvResult<RunId> {
    finish_failed_recorded_run(
      self.runtime,
      self.run,
      error,
      format!("Skill {} failed", manifest.recipe_id),
    )
  }
}
