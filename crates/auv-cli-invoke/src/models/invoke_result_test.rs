use std::path::PathBuf;
use std::str::FromStr;

use auv_tracing::{ArtifactId, ArtifactMetadata, ArtifactPurpose, ArtifactUri, Attributes, ByteLength, ContentType, RunId, Sha256Digest};

use super::*;
use crate::{InvokeCommandOutput, default_registry};

// https://github.com/moeru-ai/auv/pull/429#discussion_r4389303193
#[test]
fn guarded_recapture_cause_reaches_cli_json_pr_429() {
  use auv::client::runner::{CapabilityError, ClickPreconditionFailureReason};
  let registry = default_registry();
  let command = registry.resolve("input.clickPoint").unwrap();
  for (code, expected) in [
    (tonic::Code::PermissionDenied, "permission_denied"),
    (tonic::Code::Unavailable, "backend"),
    (tonic::Code::NotFound, "not_found"),
  ] {
    let source = auv_api_proto::click_precondition::status_with_cause(
      "cap",
      ClickPreconditionFailureReason::CheckUnavailable,
      &tonic::Status::new(code, "recapture failed"),
    );
    let failure = crate::InvokeFailure::from(CapabilityError::CapturePrecondition {
      capture_id: "cap".into(),
      reason: ClickPreconditionFailureReason::CheckUnavailable,
      cause_code: Some(code),
      source,
    });
    let result = InvokeResult::from_command_result(RunId::new(), command, Err(failure));
    let output = result
      .render_to_string(InvokeOutputOptions {
        json: true,
        ..Default::default()
      })
      .unwrap();
    let json: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(json["failure_details"]["code"], "recapture_required");
    assert_eq!(json["failure_details"]["cause_code"], expected);
    assert!(json["failure"].as_str().unwrap().contains("recapture failed"));
  }
}

#[test]
fn primary_artifact_is_directly_openable_in_human_and_json_output() {
  let registry = default_registry();
  let command = registry.resolve("display.capture").expect("capture command");
  let run_id = RunId::from_str("019f8b1e-4b2d-7a00-8f00-0000000000ab").expect("run id");
  let uri = ArtifactUri::from_ids(run_id, ArtifactId::new());
  let metadata = ArtifactMetadata::new(
    uri.clone(),
    ArtifactPurpose::new("auv.driver.display_capture"),
    ContentType::new("image/png"),
    Some("png".to_string()),
    ByteLength::new(3).expect("length"),
    Sha256Digest::new([7; 32]),
    Attributes::empty(),
  );
  let file_path = PathBuf::from("/tmp/auv-artifacts/capture.png");
  let output = InvokeCommandOutput::from_result(&serde_json::json!({ "captured": true })).expect("result").with_artifacts([metadata]);
  let result = InvokeResult::from_command_result(run_id, command, Ok(output)).with_artifact_paths([(uri, file_path.clone())]);

  let human = result.render_to_string(InvokeOutputOptions::default()).expect("human output");
  assert!(human.contains("Artifacts"));
  assert!(human.contains("auv.driver.display_capture"));
  assert!(human.contains(file_path.to_str().expect("UTF-8 fixture path")));
  assert!(human.contains("auv://runs/"));

  let json = result
    .render_to_string(InvokeOutputOptions {
      json: true,
      ..InvokeOutputOptions::default()
    })
    .expect("JSON output");
  let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
  assert_eq!(value["artifacts"][0]["purpose"], "auv.driver.display_capture");
  assert_eq!(value["artifacts"][0]["content_type"], "image/png");
  assert_eq!(value["artifacts"][0]["file_extension"], "png");
  assert_eq!(value["artifacts"][0]["file_path"], file_path.to_str().expect("UTF-8 fixture path"));
}

#[test]
fn typed_failure_keeps_legacy_message_and_machine_readable_reason() {
  let registry = default_registry();
  let command = registry.resolve("input.key").unwrap();
  let failure = crate::InvokeFailure::from(auv_driver::DriverError::NotFound {
    target: "window:missing".into(),
  });
  let output = InvokeResult::from_command_result(RunId::new(), command, Err(failure));
  let json: serde_json::Value = serde_json::from_str(
    &output
      .render_to_string(InvokeOutputOptions {
        json: true,
        ..Default::default()
      })
      .unwrap(),
  )
  .unwrap();
  assert_eq!(json["status"], "failed");
  assert_eq!(json["command_id"], "input.key");
  assert_eq!(json["failure"], "window:missing was not found");
  assert_eq!(json["failure_details"]["code"], "not_found");
  assert_eq!(json["failure_details"]["message"], json["failure"]);
}
