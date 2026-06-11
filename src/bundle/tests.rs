// File: src/bundle/tests.rs
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::model::now_millis;
use crate::skill::{SkillCaseMatrixCatalog, SkillCatalog};

use super::catalog::SkillBundleCatalog;
use super::export::{export_bundle, read_json_file, verify_exported_bundle_package_standalone};
use super::model::{
  ExportedBundlePackageManifest, SkillBundleCatalogEntry, SkillBundleCommand, SkillBundleManifest,
  SkillBundleMember, SkillBundleMemberCoverageSummary, SkillBundleMetadata, SkillBundleTarget,
  SkillBundleVerification, SkillBundleVersions,
};
use super::paths::sanitized_bundle_package_name;
use super::validate::{
  validate_bundle_target_application_scope, verify_bundle, verify_bundle_metadata_version,
};

fn unique_test_suffix() -> String {
  static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);
  format!(
    "{}-{}-{}",
    now_millis(),
    std::process::id(),
    TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
  )
}

#[test]
fn bundle_catalog_discovers_bundle_manifests_only() {
  let root = env::temp_dir().join(format!("auv-bundle-catalog-{}", now_millis()));
  fs::create_dir_all(root.join("bundles/test")).expect("temp bundles dir should exist");
  fs::write(
    root.join("bundles/test/example.v0.json"),
    r#"{
        "apiVersion": "auv.ai/v1alpha1",
        "kind": "SkillBundle",
        "metadata": { "id": "test.bundle.v0", "name": "Test Bundle", "version": "0.1.0" },
        "members": []
      }"#,
  )
  .expect("bundle manifest should write");
  fs::write(root.join("bundles/test/readme.txt"), "ignored").expect("non-json file should write");

  let catalog = SkillBundleCatalog::discover(&root).expect("catalog should load");
  assert_eq!(catalog.entries().len(), 1);
  assert_eq!(catalog.entries()[0].manifest.metadata.id, "test.bundle.v0");

  let _ = fs::remove_dir_all(root);
}

#[test]
fn bundle_catalog_resolves_by_id() {
  let root = env::temp_dir().join(format!("auv-bundle-resolve-{}", now_millis()));
  fs::create_dir_all(root.join("bundles/test")).expect("temp bundles dir should exist");
  fs::write(
    root.join("bundles/test/example.v0.json"),
    r#"{
        "apiVersion": "auv.ai/v1alpha1",
        "kind": "SkillBundle",
        "metadata": { "id": "test.bundle.v0", "name": "Test Bundle", "version": "0.1.0" },
        "members": []
      }"#,
  )
  .expect("bundle manifest should write");

  let catalog = SkillBundleCatalog::discover(&root).expect("catalog should load");
  let entry = catalog
    .resolve(&root, "test.bundle.v0")
    .expect("bundle should resolve");
  assert_eq!(entry.manifest.metadata.name, "Test Bundle");

  let _ = fs::remove_dir_all(root);
}

fn bundle_entry_with(
  metadata_version: &str,
  target_application: &str,
  member_app_bundle_ids: &[&str],
) -> SkillBundleCatalogEntry {
  SkillBundleCatalogEntry {
    manifest: SkillBundleManifest {
      api_version: "auv.ai/v1alpha1".to_string(),
      kind: "SkillBundle".to_string(),
      metadata: SkillBundleMetadata {
        id: "test.bundle.v0".to_string(),
        name: "Test Bundle".to_string(),
        version: metadata_version.to_string(),
        status: "working".to_string(),
      },
      target: SkillBundleTarget {
        application_family: "native-macos-apps".to_string(),
        platform: "macOS".to_string(),
      },
      commands: vec![SkillBundleCommand {
        id: "test.bundle.command".to_string(),
        recipe_id: "test.recipe.0".to_string(),
        summary: "Bundle command fixture".to_string(),
      }],
      versions: SkillBundleVersions {
        auv: ">=0.0.1, <0.1.0".to_string(),
        target_application: target_application.to_string(),
      },
      members: member_app_bundle_ids
        .iter()
        .enumerate()
        .map(|(index, app_bundle_id)| SkillBundleMember {
          recipe_id: format!("test.recipe.{index}"),
          case_matrix_id: format!("test.recipe.{index}.cases"),
          role: "sample".to_string(),
          validated_case_ids: vec!["baseline".to_string()],
          candidate_case_ids: Vec::new(),
          contract: "verifyAxText".to_string(),
          evidence_refs: Vec::new(),
          app_bundle_id: (*app_bundle_id).to_string(),
          target_application: String::new(),
          coverage_summary: SkillBundleMemberCoverageSummary::default(),
        })
        .collect(),
      verification: SkillBundleVerification {
        expected_signals: vec!["signal".to_string()],
        success_criteria: vec!["criteria".to_string()],
        non_goals: Vec::new(),
      },
      known_limits: Vec::new(),
    },
    path: PathBuf::from("/tmp/test-bundle.json"),
  }
}

#[test]
fn bundle_metadata_version_must_be_semver() {
  let entry = bundle_entry_with("not-a-version", "", &["com.example.notes"]);
  let error = verify_bundle_metadata_version(&entry).expect_err("metadata version should fail");
  assert!(error.contains("metadata.version"));
}

#[test]
fn bundle_target_application_scope_rejects_multi_app_bundle() {
  let entry = bundle_entry_with(
    "0.1.0",
    ">=1.0.0, <2.0.0",
    &["com.example.notes", "com.example.editor"],
  );
  let error = validate_bundle_target_application_scope(&entry)
    .expect_err("multi-app bundle targetApplication should fail");
  assert!(error.contains("spans multiple appBundleId values"));
}

#[test]
fn bundle_target_application_scope_allows_empty_for_multi_app_bundle() {
  let entry = bundle_entry_with("0.1.0", "", &["com.example.notes", "com.example.editor"]);
  let scope = validate_bundle_target_application_scope(&entry)
    .expect("empty targetApplication should be allowed");
  assert!(scope.is_none());
}

#[test]
fn bundle_target_application_scope_allows_single_app_bundle() {
  let entry = bundle_entry_with("0.1.0", ">=1.0.0, <2.0.0", &["com.example.notes"]);
  let scope = validate_bundle_target_application_scope(&entry)
    .expect("single-app targetApplication should be allowed")
    .expect("scope should exist");
  assert_eq!(scope.0, "com.example.notes");
  assert!(scope.1.matches(&semver::Version::parse("1.4.0").unwrap()));
}

fn copy_fixture_file(source_project_root: &Path, temp_project_root: &Path, relative: &str) {
  let source = source_project_root.join(relative);
  let destination = temp_project_root.join(relative);
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).expect("destination parent should exist");
  }
  fs::copy(&source, &destination).expect("fixture file should copy");
}

fn write_temp_file(temp_project_root: &Path, relative: &str, contents: &str) {
  let destination = temp_project_root.join(relative);
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).expect("destination parent should exist");
  }
  fs::write(&destination, contents).expect("temp fixture file should write");
}

fn write_temp_bundle_manifest(
  temp_project_root: &Path,
  evidence_refs: &[&str],
  app_bundle_id: &str,
  contract: &str,
  member_target_application: &str,
  bundle_target_application: &str,
) {
  let bundle_path = temp_project_root.join("bundles/test/evidence-check.v0.json");
  if let Some(parent) = bundle_path.parent() {
    fs::create_dir_all(parent).expect("bundle parent should exist");
  }
  let value = serde_json::json!({
    "apiVersion": "auv.ai/v1alpha1",
    "kind": "SkillBundle",
    "metadata": {
      "id": "test.evidence.check.v0",
      "name": "Evidence Check Bundle",
      "version": "0.1.0",
      "status": "working"
    },
    "target": {
      "applicationFamily": "native-macos-apps",
      "platform": "macOS"
    },
    "versions": {
      "auv": ">=0.0.1, <0.1.0",
      "targetApplication": bundle_target_application
    },
    "members": [
      {
        "recipeId": "macos.textedit.create_and_verify_text.v0",
        "caseMatrixId": "macos.textedit.create_and_verify_text.v0",
        "role": "native-text-sample",
        "validatedCaseIds": ["textedit-marker-baseline"],
        "candidateCaseIds": [],
        "contract": contract,
        "appBundleId": app_bundle_id,
        "targetApplication": member_target_application,
        "coverageSummary": {
          "activationStatus": "validated",
          "semanticSelectionStatus": "not-applicable",
          "validatedClaims": ["Example editor marker insertion and AX text verification are validated on macOS."],
          "boundaryClaims": ["This member is a native text sample, not a music-selection skill."]
        },
        "evidenceRefs": evidence_refs,
      }
    ],
    "verification": {
      "expectedSignals": ["evidence contract"],
      "successCriteria": ["evidence refs must be present"],
      "nonGoals": [],
    },
    "knownLimits": []
  });
  fs::write(
    &bundle_path,
    serde_json::to_string_pretty(&value).expect("bundle manifest should serialize"),
  )
  .expect("bundle manifest should write");
}

fn temp_bundle_project_root() -> PathBuf {
  let source_project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let temp_project_root =
    env::temp_dir().join(format!("auv-bundle-evidence-{}", unique_test_suffix()));
  copy_fixture_file(
    &source_project_root,
    &temp_project_root,
    "recipes/macos/textedit/create-and-verify-text.v0.json",
  );
  copy_fixture_file(
    &source_project_root,
    &temp_project_root,
    "recipes/macos/textedit/create-and-verify-text.cases.v0.json",
  );
  write_temp_file(
    &temp_project_root,
    "docs/ai/references/2026-05-17-qqmusic-narrow-skill-coverage.md",
    "temporary evidence fixture for bundle verification tests\n",
  );
  temp_project_root
}

#[test]
fn export_bundle_writes_self_consistent_package() {
  let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
  let case_matrix_catalog =
    SkillCaseMatrixCatalog::discover(&project_root).expect("case matrix catalog should load");
  let bundle_catalog =
    SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
  let entry = bundle_catalog
    .resolve(&project_root, "native.app.skill-tree.v0")
    .expect("bundle should resolve");

  let output_dir =
    env::temp_dir().join(format!("auv-bundle-export-verify-{}", unique_test_suffix()));
  export_bundle(
    &project_root,
    &skill_catalog,
    &case_matrix_catalog,
    entry,
    output_dir.clone(),
  )
  .expect("bundle export should succeed");

  let package_root = output_dir.join(sanitized_bundle_package_name(&entry.manifest.metadata.id));
  let bundle_id = verify_exported_bundle_package_standalone(&package_root)
    .expect("exported bundle package should standalone-verify");
  assert_eq!(bundle_id, entry.manifest.metadata.id);

  let package_manifest: ExportedBundlePackageManifest = serde_json::from_value(
    read_json_file(&package_root.join("package.json")).expect("package manifest should read"),
  )
  .expect("package manifest should parse");
  for member in &package_manifest.members {
    assert!(member.package_dir.starts_with("members/"));
    assert!(!member.package_dir.contains(" -> "));
    assert!(!member.strategy.family.is_empty());
    assert!(!member.strategy.grounding.is_empty());
    assert!(!member.strategy.activation.is_empty());
    assert!(!member.strategy.verification_contract.is_empty());
  }

  let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn export_bundle_rejects_missing_evidence_refs() {
  let project_root = temp_bundle_project_root();
  write_temp_bundle_manifest(
    &project_root,
    &["docs/ai/references/missing-evidence.md"],
    "",
    "verifyAxText",
    "",
    "",
  );

  let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
  let case_matrix_catalog =
    SkillCaseMatrixCatalog::discover(&project_root).expect("case catalog should load");
  let bundle_catalog =
    SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
  let entry = bundle_catalog
    .resolve(&project_root, "test.evidence.check.v0")
    .expect("bundle should resolve");
  let output_dir = env::temp_dir().join(format!(
    "auv-bundle-export-missing-{}",
    unique_test_suffix()
  ));

  let error = export_bundle(
    &project_root,
    &skill_catalog,
    &case_matrix_catalog,
    entry,
    output_dir,
  )
  .expect_err("missing evidence refs should fail export");
  assert!(error.contains("missing evidence ref"));

  let _ = fs::remove_dir_all(project_root);
}

#[test]
fn standalone_package_verify_requires_exported_evidence_refs() {
  let project_root = temp_bundle_project_root();
  write_temp_bundle_manifest(
    &project_root,
    &["docs/ai/references/2026-05-17-qqmusic-narrow-skill-coverage.md"],
    "",
    "verifyAxText",
    "",
    "",
  );

  let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
  let case_matrix_catalog =
    SkillCaseMatrixCatalog::discover(&project_root).expect("case catalog should load");
  let bundle_catalog =
    SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
  let entry = bundle_catalog
    .resolve(&project_root, "test.evidence.check.v0")
    .expect("bundle should resolve");
  let output_dir =
    env::temp_dir().join(format!("auv-bundle-export-verify-{}", unique_test_suffix()));

  export_bundle(
    &project_root,
    &skill_catalog,
    &case_matrix_catalog,
    entry,
    output_dir.clone(),
  )
  .expect("bundle export should succeed");

  let package_root = output_dir.join(sanitized_bundle_package_name(&entry.manifest.metadata.id));
  let exported_evidence = package_root
    .join("members")
    .join(sanitized_bundle_package_name(
      "macos.textedit.create_and_verify_text.v0",
    ))
    .join("evidence")
    .join(sanitized_bundle_package_name(
      "docs/ai/references/2026-05-17-qqmusic-narrow-skill-coverage.md",
    ));
  fs::remove_file(&exported_evidence).expect("evidence export should be removable");

  let error = verify_exported_bundle_package_standalone(&package_root)
    .expect_err("missing exported evidence refs should fail standalone verify");
  assert!(error.contains("missing"));

  let _ = fs::remove_dir_all(output_dir);
  let _ = fs::remove_dir_all(project_root);
}

#[test]
fn verify_bundle_does_not_probe_installed_app_versions() {
  let project_root = temp_bundle_project_root();
  write_temp_bundle_manifest(
    &project_root,
    &["docs/ai/references/2026-05-17-qqmusic-narrow-skill-coverage.md"],
    "com.example.DoesNotExist",
    "verifyAxText",
    ">=1.0.0",
    "",
  );

  let runtime_version = "0.0.1";
  let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
  let case_matrix_catalog =
    SkillCaseMatrixCatalog::discover(&project_root).expect("case catalog should load");
  let bundle_catalog =
    SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
  let entry = bundle_catalog
    .resolve(&project_root, "test.evidence.check.v0")
    .expect("bundle should resolve");

  verify_bundle(
    &project_root,
    runtime_version,
    &skill_catalog,
    &case_matrix_catalog,
    entry,
  )
  .expect("bundle verification should not depend on installed app versions");

  let _ = fs::remove_dir_all(project_root);
}

#[test]
fn verify_bundle_rejects_member_contract_mismatch_with_recipe_strategy() {
  let project_root = temp_bundle_project_root();
  write_temp_bundle_manifest(
    &project_root,
    &["docs/ai/references/2026-05-17-qqmusic-narrow-skill-coverage.md"],
    "com.example.editor",
    "verifyNowPlayingTitle",
    ">=1.20.0, <2.0.0",
    "",
  );

  let runtime_version = "0.0.1";
  let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
  let case_matrix_catalog =
    SkillCaseMatrixCatalog::discover(&project_root).expect("case catalog should load");
  let bundle_catalog =
    SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
  let entry = bundle_catalog
    .resolve(&project_root, "test.evidence.check.v0")
    .expect("bundle should resolve");

  let error = verify_bundle(
    &project_root,
    runtime_version,
    &skill_catalog,
    &case_matrix_catalog,
    entry,
  )
  .expect_err("bundle verification should fail on contract mismatch");
  assert!(error.contains("verificationContract"));

  let _ = fs::remove_dir_all(project_root);
}
