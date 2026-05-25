// File: src/main.rs
mod cli;
mod xtask;

use std::env;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use auv_cli::app::{analyze_app_probe, distill_app_analysis, probe_app, validate_app_distillation};
use auv_cli::bundle::{
  SkillBundleCatalog, export_bundle, render_bundle_package_coverage, verify_bundle,
  verify_exported_bundle_package_standalone,
};
use auv_cli::model::RunStatus;
use auv_cli::scroll_scan::{
  ScanRegion, ScanTarget, ScanWindowRegionOptions, StopPolicy, scan_window_region,
};
use auv_cli::skill::{
  SkillCaseMatrixCatalog, SkillCatalog, render_skill_case_matrix_report, run_skill,
  run_skill_case_matrix,
};
use auv_cli::{build_default_runtime, build_runtime_with_store_root};
use cli::{CliCommand, InspectClientOptions, help_text, parse_cli};

#[tokio::main]
async fn main() {
  if let Err(error) = run().await {
    eprintln!("error: {error}");
    process::exit(1);
  }
}

async fn run() -> Result<(), String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  let command = parse_cli(&arguments)?;
  let project_root =
    env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let CliCommand::XtaskGenerateSwiftBridge = &command {
    let output = xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    println!("output: {}", output.display());
    return Ok(());
  }

  if let CliCommand::InspectServe {
    host,
    port,
    store_root,
    write,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let store = auv_cli::store::LocalStore::new(store_root.clone())?;
    let recorder = Arc::new(auv_cli::run_recording::BroadcastRunRecorder::new(1024));
    let token = resolve_inspect_serve_write_token(write)?;
    let config = auv_cli::inspect_server::InspectServeConfig {
      host: host.clone(),
      port: *port,
      store_root: Some(store_root.clone()),
      write: auv_cli::inspect_server::InspectWriteConfig {
        enabled: write.enabled || token.is_some(),
        token,
        no_token: write.no_token,
      },
    };
    auv_cli::inspect_server::serve(store, recorder, config).await?;
    return Ok(());
  }

  let runtime_version = env!("CARGO_PKG_VERSION").to_string();
  let skill_catalog = SkillCatalog::discover(&project_root)?;
  let bundle_catalog = SkillBundleCatalog::discover(&project_root)?;
  let case_matrix_catalog = SkillCaseMatrixCatalog::discover(&project_root)?;

  match command {
    CliCommand::Help => {
      print!("{}", help_text());
    }
    CliCommand::XtaskGenerateSwiftBridge => unreachable!("xtask is handled before runtime setup"),
    CliCommand::ListCommands => {
      let runtime = build_default_runtime(project_root.clone())?;
      for command in runtime.list_commands() {
        println!(
          "{} -> {}.{}",
          command.id, command.driver_id, command.operation
        );
        println!("  {}", command.summary);
        println!(
          "  disturbance: {} (max: {})",
          command
            .disturbance_classes
            .iter()
            .map(|class| class.as_str())
            .collect::<Vec<_>>()
            .join(", "),
          command.max_disturbance.as_str()
        );
      }
    }
    CliCommand::ListDrivers => {
      let runtime = build_default_runtime(project_root.clone())?;
      for driver in runtime.list_drivers() {
        println!("{}", driver.id);
        println!("  {}", driver.summary);
        println!("  capabilities: {}", driver.capabilities.join(", "));
        println!("  donor boundary: {}", driver.donor_boundary);
      }
    }
    CliCommand::AppProbe {
      bundle_id,
      output_dir,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let probe = probe_app(
        &project_root,
        &runtime,
        &bundle_id,
        output_dir.map(PathBuf::from),
      )?;
      println!("app: {}", probe.app.bundle_id);
      println!("status: captured");
      println!("probe: {}", probe.output_dir.join("probe.json").display());
      println!("steps: {}", probe.steps.len());
    }
    CliCommand::AppAnalyze { query } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let output = analyze_app_probe(&runtime, &PathBuf::from(query))?;
      println!("app: {}", output.analysis.app_identity.bundle_id);
      println!("status: analyzed");
      println!("analysis: {}", output.analysis_path.display());
      println!("report: {}", output.report_path.display());
      println!(
        "annotations: {}",
        output.analysis.annotation_candidates.len()
      );
    }
    CliCommand::AppDistill { query, output_dir } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let output = distill_app_analysis(
        &runtime,
        &PathBuf::from(query),
        output_dir.map(PathBuf::from),
      )?;
      println!("app: {}", output.distillation.app_identity.bundle_id);
      println!("status: distilled");
      println!("distillation: {}", output.distillation_path.display());
      println!("report: {}", output.report_path.display());
      println!("candidates: {}", output.distillation.candidates.len());
    }
    CliCommand::AppValidate { query } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let output = validate_app_distillation(&runtime, &PathBuf::from(query))?;
      println!("app: {}", output.validation.app_identity.bundle_id);
      println!("status: assessed");
      println!("validation: {}", output.validation_path.display());
      println!("report: {}", output.report_path.display());
      println!("candidates: {}", output.validation.candidates.len());
    }
    CliCommand::Invoke { request, inspect } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let result = runtime.invoke(request)?;
      println!("runId: {}", result.run_id);
      println!("status: {}", result.status.as_str());
      println!("output: {}", result.output_summary);
      for artifact in &result.artifact_paths {
        println!("artifact: {}", artifact.display());
      }

      if let Some(failure) = &result.failure_message {
        return Err(format!(
          "{} (inspect with `auv-cli inspect {}`)",
          failure, result.run_id
        ));
      }

      if result.status == RunStatus::Failed {
        return Err(format!("run {} finished in failed state", result.run_id));
      }
    }
    CliCommand::Inspect { run_id } => {
      let runtime = build_default_runtime(project_root.clone())?;
      print!("{}", runtime.inspect(&run_id)?);
    }
    CliCommand::InspectServe { .. } => {
      unreachable!("inspect serve is handled before runtime setup")
    }
    CliCommand::ScanWindowRegion {
      target,
      region,
      max_pages,
      max_scrolls,
      direction,
      scroll_amount,
      settle_ms,
      min_confidence,
      max_observations,
      per_page_after_observe_recipe,
      per_list_item_candidate_recipe,
      on_stop_candidate_recipe,
    } => {
      let runtime = build_default_runtime(project_root.clone())?;
      let region = parse_scan_region_arg(&region)?;
      let run_id = scan_window_region(
        &runtime,
        ScanWindowRegionOptions {
          target: ScanTarget {
            application_id: Some(target),
            window_title: None,
            region,
          },
          stop_policy: StopPolicy::UntilEnd {
            max_pages,
            max_scrolls,
            no_progress_limit: 2,
          },
          direction,
          scroll_amount,
          settle_ms,
          min_confidence,
          max_observations,
          per_page_after_observe_recipe,
          per_page_after_observe_inline_hook: None,
          per_list_item_candidate_recipe,
          per_list_item_candidate_inline_hook: None,
          on_stop_candidate_recipe,
          on_stop_candidate_inline_hook: None,
        },
      )?;
      println!("runId: {run_id}");
      println!("status: scanned");
    }
    CliCommand::SkillList => {
      for entry in skill_catalog.entries() {
        println!("{}", entry.manifest.recipe_id);
        println!("  {}", entry.manifest.objective);
        println!(
          "  strategy: {}/{}/{} -> {}",
          entry.manifest.strategy.family,
          entry.manifest.strategy.grounding,
          entry.manifest.strategy.activation,
          entry.manifest.strategy.verification_contract
        );
        if let Ok(taxonomy_id) = entry.manifest.strategy.taxonomy_id() {
          println!("  strategy taxonomy: {}", taxonomy_id);
        }
        if !entry.manifest.status.is_empty() {
          println!("  status: {}", entry.manifest.status);
        }
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillShow { query } => {
      let entry = skill_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read skill manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render skill manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
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
    CliCommand::SkillCasesList => {
      for entry in case_matrix_catalog.entries() {
        println!("{}", entry.matrix.skill_id);
        if !entry.matrix.status.is_empty() {
          println!("  status: {}", entry.matrix.status);
        }
        println!("  cases: {}", entry.matrix.cases.len());
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillCasesShow { query } => {
      let entry = case_matrix_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read case-matrix manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render case-matrix manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
    CliCommand::SkillCasesReport { query } => {
      let matrix_entry = case_matrix_catalog.resolve(&project_root, &query)?;
      let skill_entry = skill_catalog.resolve_recipe_id(&matrix_entry.matrix.skill_id)?;
      print!(
        "{}",
        render_skill_case_matrix_report(skill_entry, matrix_entry)?
      );
    }
    CliCommand::SkillCasesRun {
      query,
      dry_run,
      max_disturbance,
      only_case_ids,
      include_nonvalidated,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let entry = case_matrix_catalog.resolve(&project_root, &query)?;
      run_skill_case_matrix(
        &runtime,
        &skill_catalog,
        entry,
        auv_cli::skill::SkillCaseRunOptions {
          dry_run,
          max_disturbance,
          only_case_ids,
          include_nonvalidated,
        },
      )?;
    }
    CliCommand::SkillRun {
      query,
      dry_run,
      max_disturbance,
      overrides,
      inspect,
    } => {
      let runtime = build_runtime_for_inspect(&project_root, &inspect)?;
      let entry = skill_catalog.resolve(&project_root, &query)?;
      run_skill(
        &runtime,
        entry,
        auv_cli::skill::SkillRunOptions {
          dry_run,
          max_disturbance,
          overrides,
          quiet: false,
        },
      )?;
    }
  }

  Ok(())
}

fn parse_scan_region_arg(raw: &str) -> Result<ScanRegion, String> {
  let values = raw
    .split(',')
    .map(|value| value.trim().parse::<f64>())
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| format!("invalid --region ratios: {error}"))?;
  if values.len() != 4 {
    return Err("--region must contain four comma-separated ratios".to_string());
  }
  Ok(ScanRegion {
    left_ratio: values[0],
    top_ratio: values[1],
    right_ratio: values[2],
    bottom_ratio: values[3],
  })
}

fn resolve_store_root(project_root: &Path, explicit: Option<&String>) -> PathBuf {
  explicit
    .map(PathBuf::from)
    .unwrap_or_else(|| auv_cli::default_project_store_root(project_root.to_path_buf()))
}

fn resolve_inspect_serve_write_token(
  write: &cli::InspectServeWriteOptions,
) -> Result<Option<String>, String> {
  if write.token.is_some() && write.token_file.is_some() {
    return Err("--write-token cannot be combined with --write-token-file".to_string());
  }

  if let Some(token) = &write.token {
    return normalize_write_token("--write-token", token.clone()).map(Some);
  }

  if let Some(path) = &write.token_file {
    let token = std::fs::read_to_string(path)
      .map_err(|error| format!("failed to read write token file {path}: {error}"))?
      .trim()
      .to_string();
    return normalize_write_token("--write-token-file", token).map(Some);
  }

  if write.enabled && !write.no_token {
    let token = format!(
      "session-{}-{}",
      std::process::id(),
      auv_cli::model::now_millis()
    );
    return normalize_write_token("generated write token", token).map(Some);
  }

  Ok(None)
}

fn normalize_write_token(source: &str, token: String) -> Result<String, String> {
  if token.trim().is_empty() {
    Err(format!("{source} resolved to an empty write token"))
  } else {
    Ok(token)
  }
}

fn build_runtime_for_inspect(
  project_root: &Path,
  inspect: &InspectClientOptions,
) -> Result<auv_cli::runtime::Runtime, String> {
  let server_target = if should_try_server_write(inspect) {
    if let Some((url, token)) = resolve_inspect_server_target(inspect)? {
      Some((url, token))
    } else if inspect.require_server_write {
      return Err(
        "inspect server write is required but no inspect server is configured".to_string(),
      );
    } else if matches!(inspect.server_write, cli::InspectWriteSetting::Enabled) {
      eprintln!("warning: inspect server write requested but no inspect server is configured");
      None
    } else {
      None
    }
  } else {
    None
  };

  let local_write_enabled = should_write_local(inspect);
  let store_root = if local_write_enabled {
    resolve_store_root(project_root, inspect.store_root.as_ref())
  } else {
    temp_runtime_store_root()
  };
  let store = auv_cli::store::LocalStore::new(store_root.clone())?;
  let mut recorders: Vec<Arc<dyn auv_cli::run_recording::RunRecorder>> = Vec::new();

  if let Some((url, token)) = server_target {
    recorders.push(Arc::new(
      auv_cli::run_recording::InspectServerRunRecorder::new(
        url,
        token,
        inspect.require_server_write,
      ),
    ));
  }

  let recorder: Arc<dyn auv_cli::run_recording::RunRecorder> = match recorders.len() {
    0 => Arc::new(auv_cli::run_recording::NoopRunRecorder),
    1 => recorders.remove(0),
    _ => Arc::new(auv_cli::run_recording::CompositeRunRecorder::new(recorders)),
  };
  let recording = auv_cli::run_recording::RunRecordingBackend::new(store, recorder)
    .with_local_snapshot_write_enabled(local_write_enabled)
    .with_temporary_store_cleanup(!local_write_enabled);
  Ok(
    build_runtime_with_store_root(project_root.to_path_buf(), store_root)?
      .with_recording(recording),
  )
}

fn should_write_local(inspect: &InspectClientOptions) -> bool {
  !matches!(inspect.local_write, cli::InspectWriteSetting::Disabled)
}

fn should_try_server_write(inspect: &InspectClientOptions) -> bool {
  inspect.require_server_write
    || !matches!(inspect.server_write, cli::InspectWriteSetting::Disabled)
}

fn resolve_inspect_server_target(
  inspect: &InspectClientOptions,
) -> Result<Option<(String, Option<String>)>, String> {
  let explicit_token = resolve_client_token(inspect)?;
  if let Some(url) = &inspect.server_url {
    return Ok(Some((url.clone(), explicit_token)));
  }
  let Some(session) = read_discovered_inspect_session(inspect)? else {
    return Ok(None);
  };
  if !session.write_enabled {
    return Ok(None);
  }
  if !is_local_inspect_url(&session.url) {
    if inspect.require_server_write {
      return Err(format!(
        "inspect server write is required but discovered inspect server URL is not local: {}",
        session.url
      ));
    }
    eprintln!(
      "warning: ignoring discovered inspect server with non-local URL: {}",
      session.url
    );
    return Ok(None);
  }
  Ok(Some((session.url, explicit_token.or(session.write_token))))
}

fn read_discovered_inspect_session(
  inspect: &InspectClientOptions,
) -> Result<Option<auv_cli::run_recording::InspectServerSession>, String> {
  match auv_cli::run_recording::read_inspect_session() {
    Ok(session) => Ok(session),
    Err(error) if inspect.require_server_write => Err(error),
    Err(error) => {
      eprintln!("warning: ignoring inspect server session descriptor: {error}");
      Ok(None)
    }
  }
}

fn is_local_inspect_url(raw: &str) -> bool {
  let Ok(url) = reqwest::Url::parse(raw) else {
    return false;
  };
  match url.host_str() {
    Some(host) if host.eq_ignore_ascii_case("localhost") => true,
    Some(host) => host
      .parse::<std::net::IpAddr>()
      .is_ok_and(|address| address.is_loopback()),
    None => false,
  }
}

fn resolve_client_token(inspect: &InspectClientOptions) -> Result<Option<String>, String> {
  if let Some(token) = &inspect.server_token {
    return normalize_write_token("--inspect-server-token", token.clone()).map(Some);
  }
  if let Some(path) = &inspect.server_token_file {
    let token = std::fs::read_to_string(path)
      .map_err(|error| format!("failed to read inspect server token file {path}: {error}"))?
      .trim()
      .to_string();
    return normalize_write_token("--inspect-server-token-file", token).map(Some);
  }
  Ok(None)
}

fn temp_runtime_store_root() -> PathBuf {
  std::env::temp_dir().join(format!(
    "auv-runtime-store-{}-{}",
    std::process::id(),
    auv_cli::model::now_millis()
  ))
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::sync::Mutex;

  use super::*;

  static ENV_LOCK: Mutex<()> = Mutex::new(());

  #[test]
  fn inspect_serve_write_token_rejects_token_and_token_file_conflict() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: Some("secret".to_string()),
      token_file: Some("token.txt".to_string()),
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("conflicting token sources reject");

    assert!(error.contains("--write-token"));
    assert!(error.contains("--write-token-file"));
  }

  #[test]
  fn inspect_serve_write_token_rejects_empty_token_file() {
    let path = std::env::temp_dir().join(format!(
      "auv-empty-write-token-{}.txt",
      auv_cli::model::now_millis()
    ));
    fs::write(&path, " \n\t").expect("token file should write");
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: None,
      token_file: Some(path.display().to_string()),
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("empty token file should reject");

    assert!(error.contains("empty"));
    let _ = fs::remove_file(path);
  }

  #[test]
  fn inspect_serve_write_token_rejects_empty_explicit_token() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: Some(String::new()),
      token_file: None,
      no_token: false,
    };

    let error =
      resolve_inspect_serve_write_token(&write).expect_err("empty explicit token should reject");

    assert!(error.contains("empty"));
  }

  #[test]
  fn inspect_serve_write_token_generates_non_empty_session_token() {
    let write = cli::InspectServeWriteOptions {
      enabled: true,
      token: None,
      token_file: None,
      no_token: false,
    };

    let token = resolve_inspect_serve_write_token(&write)
      .expect("generated token should resolve")
      .expect("write-enabled serve should generate a token");

    assert!(token.starts_with("session-"));
    assert!(!token.is_empty());
  }

  #[test]
  fn inspect_server_target_prefers_explicit_url_and_token_file() {
    let path = std::env::temp_dir().join(format!(
      "auv-client-write-token-{}.txt",
      auv_cli::model::now_millis()
    ));
    fs::write(&path, "secret\n").expect("token file should write");
    let inspect = InspectClientOptions {
      server_url: Some("http://127.0.0.1:9876/".to_string()),
      server_token_file: Some(path.display().to_string()),
      ..InspectClientOptions::default()
    };

    let target = resolve_inspect_server_target(&inspect).expect("explicit target should resolve");

    let _ = fs::remove_file(path);
    assert_eq!(
      target,
      Some((
        "http://127.0.0.1:9876/".to_string(),
        Some("secret".to_string())
      ))
    );
  }

  #[test]
  fn required_inspect_server_write_rejects_missing_target() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-missing-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
  }

  #[test]
  fn required_missing_server_with_local_write_disabled_does_not_leave_temp_store() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-missing-required-server-no-local-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let prefix = format!("auv-runtime-store-{}-", std::process::id());
    let before = temp_runtime_store_entries(&prefix);
    let inspect = InspectClientOptions {
      local_write: cli::InspectWriteSetting::Disabled,
      server_write: cli::InspectWriteSetting::Enabled,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write without target should fail"),
      Err(error) => error,
    };
    let after = temp_runtime_store_entries(&prefix);

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("inspect server write is required"));
    assert_eq!(after, before);
  }

  #[test]
  fn optional_inspect_server_write_ignores_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-malformed-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let runtime = build_runtime_for_inspect(&root, &inspect);

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(runtime.is_ok());
  }

  #[test]
  fn required_inspect_server_write_rejects_malformed_discovered_session() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-required-malformed-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(&session_path, "not json").expect("malformed session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: true,
      ..InspectClientOptions::default()
    };

    let error = match build_runtime_for_inspect(&root, &inspect) {
      Ok(_) => panic!("required server write should reject malformed session"),
      Err(error) => error,
    };

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert!(error.contains("failed to parse inspect session"));
  }

  #[test]
  fn default_discovery_ignores_non_local_session_url() {
    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-remote-inspect-session-{}",
      auv_cli::model::now_millis()
    ));
    fs::create_dir_all(&root).expect("session dir should write");
    let session_path = root.join("session.json");
    fs::write(
      &session_path,
      serde_json::to_string(&auv_cli::run_recording::InspectServerSession {
        url: "http://203.0.113.7:8765".to_string(),
        store_root: root.display().to_string(),
        write_enabled: true,
        write_token: Some("secret".to_string()),
        pid: 123,
        started_at_millis: 456,
      })
      .expect("session should encode"),
    )
    .expect("session should write");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&session_path, fs::Permissions::from_mode(0o600))
        .expect("session file permissions should change");
    }
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &session_path);
    }
    let inspect = InspectClientOptions {
      server_write: cli::InspectWriteSetting::Default,
      require_server_write: false,
      ..InspectClientOptions::default()
    };

    let target =
      resolve_inspect_server_target(&inspect).expect("optional discovery should ignore remote URL");

    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = fs::remove_dir_all(root);
    assert_eq!(target, None);
  }

  fn temp_runtime_store_entries(prefix: &str) -> Vec<String> {
    let mut entries = fs::read_dir(std::env::temp_dir())
      .expect("temp dir should read")
      .filter_map(|entry| {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        name.starts_with(prefix).then_some(name)
      })
      .collect::<Vec<_>>();
    entries.sort();
    entries
  }
}
