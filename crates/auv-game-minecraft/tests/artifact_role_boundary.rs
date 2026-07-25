use std::fs;

use auv_game_minecraft::dataset::{PROJECTION_BUNDLE_ROLE, SPATIAL_FRAME_BUNDLE_ROLE, directory_for_role};
use auv_game_minecraft::{
  BundleArtifactId, SourceArtifactUri, SourceAuthorityId, SourceRunId, SourceRunReference, SourceRunRevision, SpatialBundleDirectory,
  SpatialBundleInputs, SpatialBundleSourceArtifact, export_spatial_bundle,
};

#[test]
fn spatial_bundle_manifest_roles_are_owned_by_dataset_contract() {
  assert_eq!(SPATIAL_FRAME_BUNDLE_ROLE, "minecraft-spatial-frame");
  assert_eq!(PROJECTION_BUNDLE_ROLE, "minecraft-projection");
  assert_eq!(directory_for_role("auv.minecraft.spatial_frame"), None);
  assert_eq!(directory_for_role("auv.minecraft.projection"), None);

  let temp = tempfile::tempdir().expect("temp dir");
  let source_dir = temp.path().join("source");
  let output_dir = temp.path().join("bundle");
  fs::create_dir_all(&source_dir).expect("source dir");
  let spatial_frame = source_dir.join("spatial-frame.json");
  let projection = source_dir.join("projection.json");
  fs::write(&spatial_frame, b"{}").expect("spatial frame");
  fs::write(&projection, b"{}").expect("projection");
  let source_run = source_run_reference();
  let spatial_frame_uri = source_artifact_uri(&source_run, 1);
  let projection_uri = source_artifact_uri(&source_run, 2);

  let output = export_spatial_bundle(SpatialBundleInputs {
    output_dir,
    source_run,
    exported_at_millis: 123,
    artifacts: vec![
      SpatialBundleSourceArtifact {
        source_artifact_uri: spatial_frame_uri,
        bundle_artifact_id: BundleArtifactId::new("bundle-000001").expect("bundle artifact id"),
        role: SPATIAL_FRAME_BUNDLE_ROLE.to_string(),
        source_file: spatial_frame,
        screenshot_bundle_artifact_id: None,
      },
      SpatialBundleSourceArtifact {
        source_artifact_uri: projection_uri,
        bundle_artifact_id: BundleArtifactId::new("bundle-000002").expect("bundle artifact id"),
        role: PROJECTION_BUNDLE_ROLE.to_string(),
        source_file: projection,
        screenshot_bundle_artifact_id: None,
      },
    ],
  })
  .expect("bundle export");

  assert_eq!(output.manifest.counts.spatial_frames, 2);
  assert_eq!(
    output.manifest.artifacts.iter().map(|artifact| artifact.role.as_str()).collect::<Vec<_>>(),
    [SPATIAL_FRAME_BUNDLE_ROLE, PROJECTION_BUNDLE_ROLE]
  );
  assert!(output.manifest.artifacts.iter().all(|artifact| artifact.directory == SpatialBundleDirectory::SpatialFrames));
  assert!(
    output.manifest.artifacts.iter().all(|artifact| !artifact.role.starts_with("auv.minecraft.")),
    "bundle roles must not contain canonical auv-tracing purposes"
  );

  let manifest_json = serde_json::to_value(&output.manifest).expect("serialize bundle manifest");
  assert_eq!(manifest_json["exported_at_millis"], 123);
  assert!(manifest_json["source_run"].get("authority_id").is_some());
  assert!(manifest_json["source_run"].get("run_id").is_some());
  assert_eq!(manifest_json["source_run"]["through_revision"], 7);
  for fabricated_field in [
    "source_operation",
    "source_run_type",
    "source_status",
    "generated_at_millis",
    "auv_git_commit",
    "exporter_git_commit",
  ] {
    assert!(manifest_json["source_run"].get(fabricated_field).is_none(), "source_run contains fabricated field {fabricated_field}");
  }
  assert_eq!(manifest_json["artifacts"][0]["bundle_artifact_id"], "bundle-000001");
  assert!(manifest_json["artifacts"][0]["source_artifact_uri"].as_str().is_some_and(|uri| uri.starts_with("auv://runs/")));
  assert!(!manifest_json.to_string().contains("artifact://"));
}

#[test]
fn bundle_artifact_id_rejects_empty_string_sentinels() {
  assert!(BundleArtifactId::new("").is_err());
  assert!(serde_json::from_str::<BundleArtifactId>(r#""""#).is_err());
}

fn source_run_reference() -> SourceRunReference {
  SourceRunReference {
    authority_id: SourceAuthorityId::new("authority-1").expect("source authority"),
    run_id: SourceRunId::new("00000000-0000-0000-0000-000000000001").expect("source run"),
    through_revision: SourceRunRevision::new(7).expect("source revision"),
  }
}

fn source_artifact_uri(source_run: &SourceRunReference, index: u64) -> SourceArtifactUri {
  SourceArtifactUri::new(format!("auv://runs/{}/artifacts/00000000-0000-0000-0000-{index:012}", source_run.run_id))
    .expect("source artifact URI")
}
