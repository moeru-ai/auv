use super::*;

fn executable(path: impl Into<PathBuf>) -> RunnerRuntime {
  RunnerRuntime::Executable(ExecutableRunnerRuntime {
    executable: path.into(),
    arguments: vec!["serve-runner".to_string()],
    working_directory: None,
    environment: BTreeMap::new(),
  })
}

fn custom_config(runtime: RunnerRuntime) -> RunnerProviderConfig {
  RunnerProviderConfig {
    runner_class: "example.runner.music".to_string(),
    runtime,
  }
}

fn write_config(directory: &Path, name: &str, config: &serde_json::Value) -> PathBuf {
  let path = directory.join(name);
  std::fs::write(&path, serde_json::to_vec_pretty(config).expect("encode provider config")).expect("write provider config");
  path
}

#[test]
fn load_json_preserves_bare_and_absolute_executables() {
  let directory = tempfile::tempdir().expect("provider fixture directory");
  let bare = custom_config(executable("runner-on-path"));
  let bare_path = write_config(directory.path(), "bare.json", &serde_json::to_value(&bare).unwrap());
  assert_eq!(RunnerProviderConfig::load_json(bare_path).unwrap(), bare);

  let absolute_path = directory.path().join("not-required-to-exist");
  let absolute = custom_config(executable(absolute_path));
  let absolute_config = write_config(directory.path(), "absolute.json", &serde_json::to_value(&absolute).unwrap());
  assert_eq!(RunnerProviderConfig::load_json(absolute_config).unwrap(), absolute);
}

#[test]
fn load_json_resolves_relative_executable_from_manifest_directory_without_admission() {
  let directory = tempfile::tempdir().expect("provider fixture directory");
  let config = custom_config(executable("bin/custom-runner"));
  let mut config = config;
  let RunnerRuntime::Executable(runtime) = &mut config.runtime else {
    panic!("fixture uses executable runtime")
  };
  runtime.working_directory = Some(PathBuf::from("runner-work"));
  runtime.environment.insert("RUNNER_MODE".to_string(), "test".to_string());
  let config_path = write_config(directory.path(), "provider.json", &serde_json::to_value(&config).unwrap());

  let loaded = RunnerProviderConfig::load_json(config_path).expect("load relative provider");
  let RunnerRuntime::Executable(runtime) = loaded.runtime else {
    panic!("fixture uses executable runtime")
  };
  assert_eq!(runtime.executable, directory.path().join("bin/custom-runner"));
  assert_eq!(runtime.arguments, ["serve-runner"]);
  assert_eq!(runtime.working_directory, Some(directory.path().join("runner-work")));
  assert_eq!(runtime.environment.get("RUNNER_MODE").map(String::as_str), Some("test"));
}

#[test]
fn config_accepts_forward_compatible_unknown_fields() {
  let directory = tempfile::tempdir().expect("provider fixture directory");
  let config_path = write_config(
    directory.path(),
    "provider.json",
    &serde_json::json!({
      "runner_class": "example.runner.music",
      "future_provider_field": true,
      "runtime": {
        "type": "executable",
        "config": {
          "executable": "runner-on-path",
          "arguments": [],
          "future_runtime_field": "accepted"
        }
      }
    }),
  );

  let loaded = RunnerProviderConfig::load_json(config_path).expect("unknown fields are ignored by serde");
  assert_eq!(loaded.runner_class, "example.runner.music");
  assert_eq!(loaded.runtime, executable("runner-on-path").with_arguments(Vec::new()));
}

#[test]
fn registry_keeps_runtime_selection_without_preflighting_it() {
  let executable_config = custom_config(executable("missing-runner-on-path"));
  let remote_config = RunnerProviderConfig {
    runner_class: "example.runner.remote".to_string(),
    runtime: RunnerRuntime::RemoteGrpc(RemoteGrpcRunnerRuntime {
      endpoint: "not prevalidated until connect".to_string(),
    }),
  };

  let registry =
    RunnerProviderRegistry::build_with_first_party(None, vec![executable_config, remote_config]).expect("selection does not perform IO");
  assert!(matches!(registry.get("example.runner.music").unwrap().runtime, RunnerRuntime::Executable(_)));
  assert!(matches!(registry.get("example.runner.remote").unwrap().runtime, RunnerRuntime::RemoteGrpc(_)));
}

#[test]
fn registry_rejects_invalid_reserved_and_duplicate_runner_classes() {
  let invalid = RunnerProviderConfig {
    runner_class: String::new(),
    runtime: executable("runner"),
  };
  assert!(matches!(
    RunnerProviderRegistry::build_with_first_party(None, vec![invalid]),
    Err(RunnerProviderConfigError::InvalidRunnerClass(_))
  ));

  let reserved = RunnerProviderConfig {
    runner_class: LOCAL_RUNNER_CLASS.to_string(),
    runtime: executable("runner"),
  };
  assert!(matches!(
    RunnerProviderRegistry::build_with_first_party(None, vec![reserved]),
    Err(RunnerProviderConfigError::ReservedRunnerClass(class)) if class == LOCAL_RUNNER_CLASS
  ));

  let first = custom_config(executable("first"));
  let duplicate = custom_config(executable("second"));
  assert!(matches!(
    RunnerProviderRegistry::build_with_first_party(None, vec![first, duplicate]),
    Err(RunnerProviderConfigError::DuplicateRunnerClass(class)) if class == "example.runner.music"
  ));
}

#[test]
fn first_party_runtimes_are_registered_under_daemon_owned_classes() {
  let registry = RunnerProviderRegistry::build_with_first_party(Some(executable("auv")), Vec::new()).unwrap();
  assert!(registry.get(LOCAL_RUNNER_CLASS).is_some());
  assert_eq!(registry.values().count(), 1);
}

trait RuntimeTestExt {
  fn with_arguments(self, arguments: Vec<String>) -> Self;
}

impl RuntimeTestExt for RunnerRuntime {
  fn with_arguments(mut self, arguments: Vec<String>) -> Self {
    let RunnerRuntime::Executable(runtime) = &mut self else {
      panic!("test helper requires executable runtime")
    };
    runtime.arguments = arguments;
    self
  }
}
