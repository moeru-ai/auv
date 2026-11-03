use std::collections::BTreeMap;
use std::sync::Arc;

use auv_auto_loop::runtime::FakeOperationExecutor;
use rmcp::{
  ClientHandler, ServerHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo, ErrorCode},
};

use super::{McpInvokeInput, McpServer, OpRunToolRequest, core_invoke_adapters, mcp_command_inputs};
use crate::commands::op::ExecutionModeArg;

#[test]
fn default_mcp_server_accepts_its_invoke_registry_and_adapter_catalog() {
  McpServer::new(std::path::PathBuf::from(".")).expect("default MCP invoke catalogs should agree");
}

fn image_server(project_root: &std::path::Path) -> McpServer {
  let mut adapters = core_invoke_adapters().into_iter().filter(|adapter| adapter.command_id != "display.capture").collect::<Vec<_>>();
  adapters.push(super::McpInvokeAdapter::new("display.capture", |_| async {
    let capture = auv_driver::Capture {
      image: image::RgbaImage::from_pixel(24, 12, image::Rgba([42, 84, 126, 255])),
      scale_factor: 2.0,
      bounds: auv_driver::Rect::new(100.0, -50.5, 12.0, 6.0),
      origin: None,
      backend: "fixture".into(),
      fallback_reason: None,
    };
    let artifact = auv_cli_invoke::artifact::capture_evidence("test.mcp.image", &capture, None).unwrap();
    let metadata = auv_tracing::emit_artifact(artifact).await.unwrap().unwrap();
    Ok(super::McpInvokeSuccess {
      result: serde_json::json!({ "capture_ready": true }),
      artifacts: vec![metadata],
    })
  }));
  McpServer::with_registry(project_root.into(), Arc::new(auv_cli_invoke::default_registry()), adapters).unwrap()
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn mcp_pr430_describes_logical_capture_coordinates_and_explicit_fetch() {
  // ROOT CAUSE:
  //
  // If a client followed the capture instructions on a high-DPI display, it
  // divided already-logical artifact pixels by the backing scale factor again.
  //
  // Before the fix, the instructions also omitted fetch-time resizing.
  // The fix maps returned PNG pixels through the stored image dimensions.
  let description = super::invoke_tool_description(&auv_cli_invoke::default_registry());
  assert!(description.contains("logical resolution"));
  assert!(description.contains("px * source_width / width"));
  assert!(description.contains("py * source_height / height"));
  assert!(!description.contains("divide a pixel by the capture's scale factor"));
  assert!(description.contains("artifact_read_image"));
  assert!(description.contains("artifacts[].uri"));

  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  assert!(server.get_info().instructions.unwrap().contains("artifact_read_image"));
  let tools = server.tool_router.list_all();
  let image = tools.iter().find(|tool| tool.name == "artifact_read_image").unwrap();
  let image_description = image.description.as_deref().unwrap();
  assert!(image_description.contains("stored artifact dimensions"));
  assert!(image_description.contains("source_width"));
  assert!(image_description.contains("image.source_width"));
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_fetch_dimensions_distinguish_stored_and_native_capture_pixels() {
  use base64::Engine as _;

  let directory = tempfile::tempdir().unwrap();
  let server = image_server(directory.path());
  let result = call_invoke(server.clone(), serde_json::json!({ "command_id": "display.capture" })).await;
  let artifact = &result.structured_content.as_ref().unwrap()["artifacts"][0];
  assert_eq!(artifact["content_type"], "image/webp");
  assert_eq!(artifact["attributes"]["image.source_width"], 24);
  assert_eq!(artifact["attributes"]["image.source_height"], 12);
  assert_eq!(artifact["attributes"]["image.scale_factor"], 2.0);
  assert_eq!(artifact["attributes"]["capture.bounds.x"], 100.0);
  assert_eq!(artifact["attributes"]["capture.bounds.y"], -50.5);
  let uri: auv_tracing::ArtifactUri = artifact["uri"].as_str().unwrap().parse().unwrap();
  let result = server
    .artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
      run_id: uri.run_id().to_string(),
      artifact_id: uri.artifact_id().to_string(),
      max_width: 6,
      max_height: 6,
    }))
    .await
    .unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let dimensions: serde_json::Value = serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
  assert_eq!(dimensions["source_width"], 12);
  assert_eq!(dimensions["source_height"], 6);
  assert_eq!(dimensions["width"], 6);
  assert_eq!(dimensions["height"], 3);
  let bytes = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
  assert_eq!(image.dimensions(), (6, 3));
  assert_eq!(image.get_pixel(3, 1).0, [42, 84, 126]);
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_reads_high_entropy_4k_capture_through_production_artifact_pipeline() {
  use base64::Engine as _;

  // ROOT CAUSE:
  //
  // If a high-entropy 4K capture produced more than 16 MiB of lossless WebP,
  // the reader refused it before the requested output resizing could run.
  //
  // Before the fix, a successful capture could not be fetched even at 512x512.
  // The fix admits this production encoder output within a bounded source budget.
  let directory = tempfile::tempdir().unwrap();
  let mut adapters = core_invoke_adapters().into_iter().filter(|adapter| adapter.command_id != "display.capture").collect::<Vec<_>>();
  adapters.push(super::McpInvokeAdapter::new("display.capture", |_| async {
    let mut image = image::RgbaImage::new(3840, 2160);
    let mut state = 0x1234_5678_u32;
    for pixel in image.pixels_mut() {
      for channel in &mut pixel.0[..3] {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *channel = (state >> 24) as u8;
      }
      pixel.0[3] = 255;
    }
    let capture = auv_driver::Capture {
      image,
      scale_factor: 1.0,
      bounds: auv_driver::Rect::new(0.0, 0.0, 3840.0, 2160.0),
      origin: None,
      backend: "fixture".into(),
      fallback_reason: None,
    };
    let artifact = auv_cli_invoke::artifact::capture_evidence("test.mcp.high_entropy_4k", &capture, None).unwrap();
    let metadata = auv_tracing::emit_artifact(artifact).await.unwrap().unwrap();
    Ok(super::McpInvokeSuccess {
      result: serde_json::json!({ "capture_ready": true }),
      artifacts: vec![metadata],
    })
  }));
  let server = McpServer::with_registry(directory.path().into(), Arc::new(auv_cli_invoke::default_registry()), adapters).unwrap();
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  let server_handle = tokio::spawn(async move {
    server.serve(server_transport).await.unwrap().waiting().await.unwrap();
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  let result = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: serde_json::json!({ "command_id": "display.capture" }).as_object().cloned(),
    })
    .await
    .unwrap();
  let output = result.structured_content.unwrap();
  assert_eq!(output["status"], "completed");
  assert!(output.get("recording_failure").is_none());
  let artifact = &output["artifacts"][0];
  assert_eq!(artifact["content_type"], "image/webp");
  let source_bytes = artifact["byte_length"].as_u64().unwrap();
  eprintln!("PR #430 high-entropy 3840x2160 capture: {source_bytes} source bytes");
  assert!(source_bytes > 16 * 1024 * 1024);
  assert_eq!(std::fs::metadata(artifact["file_path"].as_str().unwrap()).unwrap().len(), source_bytes);
  let uri: auv_tracing::ArtifactUri = artifact["uri"].as_str().unwrap().parse().unwrap();
  let result = client
    .call_tool(CallToolRequestParam {
      name: "artifact_read_image".into(),
      arguments: serde_json::json!({
        "run_id": uri.run_id(), "artifact_id": uri.artifact_id(),
        "max_width": 512, "max_height": 512
      })
      .as_object()
      .cloned(),
    })
    .await
    .unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let dimensions: serde_json::Value = serde_json::from_str(wire["content"][0]["text"].as_str().unwrap()).unwrap();
  assert_eq!(dimensions["source_width"], 3840);
  assert_eq!(dimensions["source_height"], 2160);
  assert_eq!(dimensions["width"], 512);
  assert_eq!(dimensions["height"], 288);
  let bytes = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  assert!(bytes.len() <= 8 * 1024 * 1024);
  assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgb8().dimensions(), (512, 288));
  client.cancel().await.unwrap();
  server_handle.await.unwrap();
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn mcp_pr430_bounds_concurrent_reads_after_waiter_cancellation() {
  use rmcp::handler::server::wrapper::Parameters;
  use std::time::Duration;

  // ROOT CAUSE:
  //
  // If callers flooded image reads, every request queued another blocking job.
  // Cancelling its async waiter did not cancel the underlying blocking job.
  //
  // Before the fix, neither submitted work nor detached work had an admission cap.
  // The fix keeps two shared slots owned by their blocking jobs until completion.
  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(1).build().unwrap();
  runtime.block_on(async {
    let (release, blocked) = std::sync::mpsc::channel();
    let (started, ready) = tokio::sync::oneshot::channel();
    let blocker = tokio::task::spawn_blocking(move || {
      started.send(()).unwrap();
      let _ = blocked.recv_timeout(Duration::from_secs(10));
    });
    ready.await.unwrap();
    let server = McpServer::new(std::path::PathBuf::from("unused")).unwrap();
    let request = || {
      Parameters(super::ArtifactReadImageRequest {
        run_id: "10000000-0000-4000-8000-000000000001".into(),
        artifact_id: "10000000-0000-4000-8000-000000000002".into(),
        max_width: 8,
        max_height: 8,
      })
    };
    let mut readers = Vec::new();
    for _ in 0..2 {
      let reader = server.clone();
      let input = request();
      let (started, ready) = tokio::sync::oneshot::channel();
      readers.push(tokio::spawn(async move {
        started.send(()).unwrap();
        reader.artifact_read_image(input).await
      }));
      ready.await.unwrap();
    }
    assert!(readers.iter().all(|reader| !reader.is_finished()));
    let mut excess = Vec::new();
    for _ in 0..32 {
      let reader = server.clone();
      let input = request();
      excess.push(tokio::spawn(async move {
        let error = tokio::time::timeout(Duration::from_secs(2), reader.artifact_read_image(input))
          .await
          .expect("excess reads must be rejected without queuing another blocking job")
          .unwrap_err();
        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert!(error.message.contains("busy"));
      }));
    }
    for reader in excess {
      reader.await.unwrap();
    }
    for reader in readers {
      reader.abort();
      assert!(reader.await.unwrap_err().is_cancelled());
    }
    let error = tokio::time::timeout(Duration::from_secs(2), server.artifact_read_image(request())).await.unwrap().unwrap_err();
    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert!(error.message.contains("busy"), "cancelled waiters must not release their queued workers' slots");
    release.send(()).unwrap();
    blocker.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
      loop {
        let error = server.artifact_read_image(request()).await.unwrap_err();
        if error.code == ErrorCode::INVALID_PARAMS {
          assert!(error.message.contains("not available"));
          break;
        }
        assert!(error.message.contains("busy"));
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("completed workers must release their slots for later requests");
  });
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_rejects_image_requests_cancelled_before_admission() {
  let server = McpServer::new(std::path::PathBuf::from("unused")).unwrap();
  let cancellation = auv_cli_invoke::InvokeCancellation::new();
  cancellation.cancel();
  let error = super::MCP_REQUEST_CANCELLATION
    .scope(
      cancellation,
      server.artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
        run_id: "10000000-0000-4000-8000-000000000001".into(),
        artifact_id: "10000000-0000-4000-8000-000000000002".into(),
        max_width: 8,
        max_height: 8,
      })),
    )
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
  assert!(error.message.contains("cancelled"));
}

// https://github.com/moeru-ai/auv/pull/430
#[test]
fn mcp_pr430_wire_cancellation_keeps_queued_worker_admission_until_completion() {
  use std::time::Duration;

  // Witness the real MCP handler returning after a cancellation notification,
  // independently of the client's immediate local cancellation result.
  struct Witness {
    server: McpServer,
    completed: Arc<tokio::sync::Notify>,
  }
  impl ServerHandler for Witness {
    fn get_info(&self) -> rmcp::model::ServerInfo {
      self.server.get_info()
    }

    async fn call_tool(
      &self,
      request: CallToolRequestParam,
      context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
      let result = self.server.call_tool(request, context).await;
      self.completed.notify_one();
      result
    }
  }

  let runtime = tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(1).build().unwrap();
  runtime.block_on(async {
    let (release, blocked) = std::sync::mpsc::channel();
    let (started, ready) = tokio::sync::oneshot::channel();
    let blocker = tokio::task::spawn_blocking(move || {
      started.send(()).unwrap();
      let _ = blocked.recv_timeout(Duration::from_secs(10));
    });
    ready.await.unwrap();
    let server = McpServer::new(std::path::PathBuf::from("unused")).unwrap();
    let completed = Arc::new(tokio::sync::Notify::new());
    let witness = Witness {
      server: server.clone(),
      completed: completed.clone(),
    };
    let (server_transport, client_transport) = tokio::io::duplex(65_536);
    let server_handle = tokio::spawn(async move {
      witness.serve(server_transport).await.unwrap().waiting().await.unwrap();
    });
    let client = TestClient.serve(client_transport).await.unwrap();
    let arguments = serde_json::json!({
      "run_id": "10000000-0000-4000-8000-000000000001",
      "artifact_id": "10000000-0000-4000-8000-000000000002"
    });
    let request = client
      .send_cancellable_request(
        rmcp::model::CallToolRequest::new(CallToolRequestParam {
          name: "artifact_read_image".into(),
          arguments: arguments.as_object().cloned(),
        })
        .into(),
        rmcp::service::PeerRequestOptions::no_options(),
      )
      .await
      .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
      while server.image_read_slots.available_permits() != 1 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("the wire request must reserve one image slot");
    client
      .notify_cancelled(rmcp::model::CancelledNotificationParam {
        request_id: request.id.clone(),
        reason: Some("test cancellation".into()),
      })
      .await
      .unwrap();
    assert!(matches!(request.await_response().await, Err(rmcp::service::ServiceError::Cancelled { .. })));
    tokio::time::timeout(Duration::from_secs(2), completed.notified())
      .await
      .expect("server cancellation must not wait for the blocked worker pool");
    assert_eq!(server.image_read_slots.available_permits(), 1, "the queued closure still owns its slot after the handler returns");
    release.send(()).unwrap();
    blocker.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
      while server.image_read_slots.available_permits() != 2 {
        tokio::task::yield_now().await;
      }
    })
    .await
    .expect("the cancelled worker must release its slot after it is drained");
    let error = client
      .call_tool(CallToolRequestParam {
        name: "artifact_read_image".into(),
        arguments: arguments.as_object().cloned(),
      })
      .await
      .unwrap_err();
    assert!(matches!(error, rmcp::service::ServiceError::McpError(error) if error.code == ErrorCode::INVALID_PARAMS));
    client.cancel().await.unwrap();
    server_handle.await.unwrap();
  });
}

#[tokio::test]
async fn mcp_image_tool_reads_only_images_returned_by_its_server() {
  use base64::Engine as _;

  let directory = tempfile::tempdir().unwrap();
  let server = image_server(directory.path());
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await.unwrap();
    service.waiting().await.unwrap();
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  let tools = client.list_all_tools().await.unwrap();
  let tool = tools.iter().find(|tool| tool.name == "artifact_read_image").expect("image tool must be discoverable");
  let schema = serde_json::to_value(&tool.input_schema).unwrap();
  assert_eq!(schema["additionalProperties"], false);
  assert_eq!(
    schema["properties"].as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
    ["artifact_id", "max_height", "max_width", "run_id"]
  );
  assert_eq!(schema["properties"]["max_width"]["default"], 2048);
  assert_eq!(schema["properties"]["max_height"]["default"], 2048);
  let invoke = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: serde_json::json!({ "command_id": "display.capture" }).as_object().cloned(),
    })
    .await
    .unwrap();
  let wire = serde_json::to_value(&invoke).unwrap();
  assert!(wire["content"].as_array().unwrap().iter().all(|content| content["type"] != "image"));
  let output = invoke.structured_content.unwrap();
  let mut artifact = output["artifacts"][0].clone();
  artifact.as_object_mut().unwrap().remove("file_path");
  let metadata: auv_tracing::ArtifactMetadata = serde_json::from_value(artifact).unwrap();
  let ids = serde_json::json!({ "run_id": metadata.uri().run_id(), "artifact_id": metadata.uri().artifact_id() });
  let fetched = client
    .call_tool(CallToolRequestParam {
      name: "artifact_read_image".into(),
      arguments: ids.as_object().cloned(),
    })
    .await
    .unwrap();
  let fetched = serde_json::to_value(fetched).unwrap();
  assert_eq!(fetched["content"][1]["type"], "image");
  assert_eq!(fetched["content"][1]["mimeType"], "image/png");
  let bytes = base64::engine::general_purpose::STANDARD.decode(fetched["content"][1]["data"].as_str().unwrap()).unwrap();
  let pixels = image::load_from_memory(&bytes).unwrap().to_rgb8();
  assert_eq!(pixels.dimensions(), (12, 6));
  assert_eq!(pixels.get_pixel(0, 0).0, [42, 84, 126]);
  let mut invalid = ids.clone();
  invalid["path"] = "C:/private.png".into();
  let error = client
    .call_tool(CallToolRequestParam {
      name: "artifact_read_image".into(),
      arguments: invalid.as_object().cloned(),
    })
    .await
    .unwrap_err();
  assert!(matches!(error, rmcp::service::ServiceError::McpError(error) if error.code == ErrorCode::INVALID_PARAMS));

  // The same store and known IDs do not grant access through another server instance.
  let other_server = McpServer::new(directory.path().into()).unwrap();
  let (other_transport, other_client_transport) = tokio::io::duplex(65_536);
  let other_handle = tokio::spawn(async move {
    let service = other_server.serve(other_transport).await.unwrap();
    service.waiting().await.unwrap();
  });
  let other_client = TestClient.serve(other_client_transport).await.unwrap();
  let error = other_client
    .call_tool(CallToolRequestParam {
      name: "artifact_read_image".into(),
      arguments: ids.as_object().cloned(),
    })
    .await
    .unwrap_err();
  assert!(matches!(error, rmcp::service::ServiceError::McpError(error) if error.code == ErrorCode::INVALID_PARAMS));

  other_client.cancel().await.unwrap();
  other_handle.await.unwrap();
  client.cancel().await.unwrap();
  server_handle.await.unwrap();
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_catalog_poison_preserves_completed_invoke_and_image_receipt() {
  use auv_tracing::{ArtifactId, ArtifactMetadata, ArtifactUri, Attributes, ByteLength, RunId, Sha256Digest};

  // ROOT CAUSE:
  //
  // If catalog registration encountered a poisoned lock, invoke discarded an
  // already-completed command and successful tracing flush as INVALID_PARAMS.
  //
  // Before the fix, the completed result and its image receipt were lost.
  // The fix recovers the catalog and returns both without repeating the command.
  let directory = tempfile::tempdir().unwrap();
  let server = image_server(directory.path());
  let store = auv_tracing::FileTracingStore::open(directory.path().join("poison")).unwrap();
  let metadata = ArtifactMetadata::new(
    ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
    "test.poison".into(),
    "image/png".into(),
    Some("invalid/extension".into()),
    ByteLength::new(1).unwrap(),
    Sha256Digest::new([0; 32]),
    Attributes::default(),
  );
  // An invalid extension makes Path::set_extension panic under the catalog lock.
  assert!(std::panic::catch_unwind(|| server.image_artifacts.register(&[metadata], &store)).is_err());
  let result = call_invoke(server.clone(), serde_json::json!({ "command_id": "display.capture" })).await;
  assert_ne!(result.is_error, Some(true));
  let output = result.structured_content.unwrap();
  assert_eq!(output["status"], "completed");
  assert!(output.get("recording_failure").is_none());
  let uri: ArtifactUri = output["artifacts"][0]["uri"].as_str().unwrap().parse().unwrap();
  let image = server
    .artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
      run_id: uri.run_id().to_string(),
      artifact_id: uri.artifact_id().to_string(),
      max_width: 12,
      max_height: 6,
    }))
    .await
    .unwrap();
  assert_ne!(image.is_error, Some(true));
  assert_eq!(serde_json::to_value(image).unwrap()["content"][1]["type"], "image");
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_image_storage_failure_is_internal_error() {
  // ROOT CAUSE:
  //
  // If a registered file disappeared, a valid image read became INVALID_PARAMS
  // because every reader failure crossed the handler boundary as a String.
  //
  // Before the fix, callers could not distinguish server storage failure.
  // The fix preserves the server error category through artifact_read_image.
  let directory = tempfile::tempdir().unwrap();
  let server = image_server(directory.path());
  let result = call_invoke(server.clone(), serde_json::json!({ "command_id": "display.capture" })).await;
  let output = result.structured_content.unwrap();
  let artifact = &output["artifacts"][0];
  std::fs::remove_file(artifact["file_path"].as_str().unwrap()).unwrap();
  let uri: auv_tracing::ArtifactUri = artifact["uri"].as_str().unwrap().parse().unwrap();
  let error = server
    .artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
      run_id: uri.run_id().to_string(),
      artifact_id: uri.artifact_id().to_string(),
      max_width: 12,
      max_height: 6,
    }))
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
  assert!(error.message.contains("cannot be opened"));
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_flush_failure_preserves_result_without_advertising_image_receipts() {
  // ROOT CAUSE:
  //
  // If recording flush failed after an artifact write, invoke returned its
  // receipt even though the image catalog deliberately refused to register it.
  //
  // Before the fix, a newly advertised image could not be fetched.
  // The fix preserves the command result and recording failure without receipts.
  struct FailingFlush;
  impl auv_tracing::TraceExporter for FailingFlush {
    fn export(&self, _: auv_tracing::TraceRecord) -> auv_tracing::BoxFuture<'_, Result<(), auv_tracing::ExportError>> {
      Box::pin(async { Ok(()) })
    }

    fn flush(&self) -> auv_tracing::BoxFuture<'_, Result<(), auv_tracing::ExportError>> {
      Box::pin(async { Err(auv_tracing::ExportError::new(auv_tracing::ErrorCode::new("test.mcp.flush_failed"))) })
    }
  }

  let directory = tempfile::tempdir().unwrap();
  let store_root = directory.path().join(".auv").join("store");
  let store = Arc::new(auv_tracing::FileTracingStore::open(&store_root).unwrap());
  let authority = super::McpFrontendAuthority {
    dispatch: auv_tracing::configure().tracing_store(store.clone()).exporter(Arc::new(FailingFlush)).build().unwrap(),
    store: store.clone(),
  };
  let mut server = image_server(directory.path());
  server.invoke_dispatch = Arc::new(move |_| Ok(authority.clone()));
  let result = call_invoke(server.clone(), serde_json::json!({ "command_id": "display.capture" })).await;
  assert_ne!(result.is_error, Some(true));
  let output = result.structured_content.unwrap();
  assert_eq!(output["status"], "completed");
  assert_eq!(output["result"]["capture_ready"], true);
  assert!(!output["recording_failure"].as_str().unwrap().is_empty());
  assert!(output.get("artifacts").is_none(), "receipts must be omitted after a recording flush failure");

  // The file write and store flush succeeded; only the telemetry flush failed.
  let metadata = auv_tracing::FileTracingStore::read_records(&store_root)
    .unwrap()
    .map(Result::unwrap)
    .find_map(|stored| match stored.record {
      auv_tracing::TraceRecord::Artifact { metadata, .. } => Some(metadata),
      _ => None,
    })
    .unwrap();
  assert_eq!(std::fs::metadata(store.artifact_path(&metadata)).unwrap().len(), metadata.byte_length().get());
  let error = server
    .artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
      run_id: metadata.uri().run_id().to_string(),
      artifact_id: metadata.uri().artifact_id().to_string(),
      max_width: 12,
      max_height: 6,
    }))
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
  assert!(error.message.contains("not available"));
}

// https://github.com/moeru-ai/auv/pull/430
#[tokio::test]
async fn mcp_pr430_explicit_fixture_and_store_paths_are_not_workspace_confined() {
  use base64::Engine as _;

  // This records the existing invoke authority boundary. Image receipt checks
  // do not confine a trusted command's fixture or store paths to project_root.
  let directory = tempfile::tempdir().unwrap();
  let project = directory.path().join("project");
  let fixture = directory.path().join("fixture");
  let store_root = directory.path().join("store");
  std::fs::create_dir(&project).unwrap();
  std::fs::create_dir(&fixture).unwrap();
  let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/testdata/scan/temporal/single_frame_v0");
  for name in ["manifest.json", "frame-0001.png"] {
    std::fs::copy(source.join(name), fixture.join(name)).unwrap();
  }
  let server = McpServer::new(project.clone()).unwrap();
  let result = call_invoke(
    server.clone(),
    serde_json::json!({ "command_id": "scan.frame", "store_root": store_root, "inputs": { "fixture-dir": fixture } }),
  )
  .await;
  let output = result.structured_content.unwrap();
  assert_eq!(output["status"], "completed");
  assert!(output.get("recording_failure").is_none());
  assert!(store_root.join("records.jsonl").is_file());
  assert!(!project.join(".auv").exists());
  let artifact = output["artifacts"].as_array().unwrap().iter().find(|artifact| artifact["content_type"] == "image/png").unwrap();
  let path = std::path::Path::new(artifact["file_path"].as_str().unwrap());
  assert!(path.starts_with(store_root.join("artifacts")));
  let uri: auv_tracing::ArtifactUri = artifact["uri"].as_str().unwrap().parse().unwrap();
  let result = server
    .artifact_read_image(rmcp::handler::server::wrapper::Parameters(super::ArtifactReadImageRequest {
      run_id: uri.run_id().to_string(),
      artifact_id: uri.artifact_id().to_string(),
      max_width: 8,
      max_height: 8,
    }))
    .await
    .unwrap();
  let wire = serde_json::to_value(result).unwrap();
  let bytes = base64::engine::general_purpose::STANDARD.decode(wire["content"][1]["data"].as_str().unwrap()).unwrap();
  let actual = image::load_from_memory(&bytes).unwrap().to_rgba8();
  let expected = image::open(source.join("frame-0001.png")).unwrap().to_rgba8();
  assert_eq!(actual, expected);
}

#[test]
fn mcp_handshake_names_the_auv_build_rather_than_the_transport_crate() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let info = server.get_info();

  assert_eq!(info.server_info.name, "auv");
  assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn device_entry_mcp_tools_expose_only_nonsecret_selection_and_target_fields() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let sessions = tools.iter().find(|tool| tool.name == "device_list_user_sessions").expect("Device sessions tool");
  let get_session = tools.iter().find(|tool| tool.name == "device_get_user_session").expect("Device get session tool");
  let unlock = tools.iter().find(|tool| tool.name == "device_ensure_user_session_unlocked").expect("Device unlock tool");
  let lock = tools.iter().find(|tool| tool.name == "device_ensure_user_session_locked").expect("Device lock tool");
  let session_schema = serde_json::to_value(&sessions.input_schema).unwrap();
  let get_schema = serde_json::to_value(&get_session.input_schema).unwrap();
  let unlock_schema = serde_json::to_value(&unlock.input_schema).unwrap();
  let lock_schema = serde_json::to_value(&lock.input_schema).unwrap();
  let session_properties = session_schema["properties"].as_object().unwrap();
  let get_properties = get_schema["properties"].as_object().unwrap();
  let unlock_properties = unlock_schema["properties"].as_object().unwrap();

  assert_eq!(session_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name"]);
  assert_eq!(get_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name", "session_selector"]);
  assert_eq!(get_schema["additionalProperties"], false);
  assert_eq!(unlock_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name", "session_selector", "user"]);
  assert_eq!(unlock_schema["additionalProperties"], false);
  assert_eq!(lock_schema, unlock_schema);
}

#[test]
fn mcp_disables_incidental_overlays_but_preserves_explicit_overlay_operations() {
  let incidental = mcp_command_inputs(auv_cli_invoke::InvokeNamespace::Window, pairs(&[("overlay", "true")]));
  assert_eq!(incidental.get("overlay").map(String::as_str), Some("false"));

  let explicit = mcp_command_inputs(auv_cli_invoke::InvokeNamespace::Overlay, BTreeMap::new());
  assert!(!explicit.contains_key("overlay"));
}

#[tokio::test]
async fn overlay_mcp_adapters_execute_the_shared_dry_run_commands() {
  let cases = [
    ("overlay.outline", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
    ("overlay.cursor", pairs(&[("x", "10"), ("y", "20")])),
    ("overlay.status", pairs(&[("x", "10"), ("y", "20"), ("text", "processing")])),
    ("overlay.captureFrame", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
    ("overlay.clickTarget", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
  ];
  let adapters = core_invoke_adapters();

  for (command_id, inputs) in cases {
    let adapter = adapters.iter().find(|adapter| adapter.command_id == command_id).unwrap_or_else(|| panic!("missing {command_id} adapter"));
    adapter
      .invoke(McpInvokeInput {
        target: None,
        inputs,
        dry_run: true,
        cancellation: Default::default(),
      })
      .await
      .unwrap_or_else(|error| panic!("{command_id} MCP dry run failed: {error}"));
  }
}

// https://github.com/moeru-ai/auv/actions/runs/30577666189/job/90989876962
#[tokio::test]
async fn mcp_uses_the_same_typed_range_validation_as_cli() {
  // ROOT CAUSE:
  //
  // If invalid window-point coordinates were invoked outside macOS, the
  // platform rejection won because typed coordinate validation lived inside
  // the macOS-only command body.
  //
  // Before the fix, Linux CI observed a platform error instead of the shared
  // validation error. The fix validates command inputs before platform dispatch.
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.clickPoint").expect("click-point adapter");
  let error = adapter
    .invoke(McpInvokeInput {
      target: Some(auv_cli_invoke::ExecutionTarget::Window {
        id: "window-1".to_string(),
      }),
      inputs: pairs(&[("x", "2"), ("y", "0.5"), ("relative-to", "window"), ("normalized", "true")]),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect_err("out-of-range MCP input must fail typed decoding");

  assert!(error.message.contains("within 0..=1"), "unexpected typed validation error: {error}");
}

// ROOT CAUSE:
//
// If an MCP host sent the input keys advertised in x-auv-commands, input.drag
// failed with `missing field 'start-x'` because serde renamed the positional
// fields while the metadata advertised their Clap ids.
//
// Before the fix, only undocumented hyphenated keys worked.
// The fix keeps the advertised keys (`start_x`, ...) as the decoded names.
#[tokio::test]
async fn drag_mcp_adapter_accepts_advertised_input_keys_with_typed_defaults() {
  // Drive the adapter with exactly the keys the MCP schema advertises.
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();
  let metadata = schema["x-auv-commands"].as_array().unwrap().iter().find(|command| command["id"] == "input.drag").expect("drag metadata");
  let advertised: Vec<_> = metadata["arguments"].as_array().unwrap().iter().map(|argument| argument["input_key"].as_str().unwrap()).collect();
  assert_eq!(advertised[..4], ["start_x", "start_y", "end_x", "end_y"]);
  let inputs: Vec<_> = advertised[..4].iter().copied().zip(["120", "80", "320", "80"]).collect();
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.drag").expect("drag adapter");
  let success = adapter
    .invoke(McpInvokeInput {
      target: None,
      inputs: pairs(&inputs),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect("screen-relative drag dry run should use typed defaults");

  assert_eq!(success.result["relative_to"], "screen");
  assert_eq!(success.result["button"], "left");
  assert_eq!(success.result["duration_ms"], 300);
  assert_eq!(success.result["screen_end"]["x"], 320.0);
  assert!(success.result["action"].is_null());
}

#[tokio::test]
async fn click_point_mcp_defaults_to_screen_coordinates_without_optional_inputs() {
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.clickPoint").expect("click-point adapter");
  let success = adapter
    .invoke(McpInvokeInput {
      target: None,
      inputs: pairs(&[("x", "120"), ("y", "80")]),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect("screen-relative dry run should use typed defaults");

  assert_eq!(success.result["relative_to"], "screen");
  assert_eq!(success.result["screen_point"]["x"], 120.0);
  assert!(success.result["action"].is_null());
}

fn pairs(values: &[(&str, &str)]) -> BTreeMap<String, String> {
  values.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

#[tokio::test]
async fn keyboard_mcp_rejects_display_with_typed_failure() {
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.key").unwrap();
  let error = adapter.invoke(McpInvokeInput {
    target: Some(auv_cli_invoke::ExecutionTarget::Display { id: "primary".into() }),
    inputs: pairs(&[("key", "cmd+a")]), dry_run: false, cancellation: Default::default(),
  }).await.unwrap_err();
  assert_eq!(error.code, auv_cli_invoke::FailureCode::InvalidTarget);
}

#[test]
fn mcp_target_metadata_comes_from_the_executable_definition() {
  let registry = auv_cli_invoke::default_registry();
  let keyboard = super::invoke_command_metadata(registry.resolve("input.key").unwrap());
  assert_eq!(keyboard["target"]["accepted_types"], serde_json::json!(["application", "window"]));
  assert_eq!(keyboard["target"]["required"], false);
  let media = super::invoke_command_metadata(registry.resolve("mediaControl.play").unwrap());
  assert_eq!(media["target"]["accepted_types"], serde_json::json!([]));
  let focus = super::invoke_command_metadata(registry.resolve("input.focusText").unwrap());
  assert_eq!(focus["target"]["required"], true);
}

#[test]
fn mcp_support_metadata_comes_from_the_executable_definition() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    assert_eq!(metadata["support"], serde_json::to_value(command.support).unwrap(), "support of {}", command.id);
    assert_eq!(metadata["role"], serde_json::to_value(command.role).unwrap(), "role of {}", command.id);
  }

  let click_text = super::invoke_command_metadata(registry.resolve("screen.clickText").unwrap());
  assert_eq!(click_text["support"]["linux"], serde_json::json!({ "state": "unsupported" }));
  assert_eq!(click_text["support"]["macos"], serde_json::json!({ "state": "supported" }));

  let type_text = super::invoke_command_metadata(registry.resolve("input.typeText").unwrap());
  assert_eq!(
    type_text["support"]["linux"],
    serde_json::json!({ "state": "partial", "note": "focused target only; cannot focus or raise a window" })
  );

  let press_keys = super::invoke_command_metadata(registry.resolve("input.pressKeys").unwrap());
  assert_eq!(press_keys["role"], serde_json::json!({ "kind": "alias", "canonical": "input.keys" }));
}

#[test]
fn mcp_runner_support_metadata_states_the_route_and_cells() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    let expected_route = match command.runner {
      auv_cli_invoke::RunnerSupport::NoRoute => "no_route",
      auv_cli_invoke::RunnerSupport::Direct => "direct",
      auv_cli_invoke::RunnerSupport::Cells(_) => "cells",
    };
    assert_eq!(metadata["runner_support"]["route"], serde_json::json!(expected_route), "runner route of {}", command.id);
    match command.runner_cells() {
      None => assert!(
        metadata["runner_support"].get("cells").is_none(),
        "{} has no Runner route, so it must not carry runner cells",
        command.id
      ),
      Some(cells) => assert_eq!(metadata["runner_support"]["cells"], serde_json::to_value(cells).unwrap(), "runner cells of {}", command.id),
    }
  }

  // Pins from the 2026-10-25 source audit: the cells where the Runner route
  // differs from the direct path must stay stated as different cells.
  let click_text = super::invoke_command_metadata(registry.resolve("screen.clickText").unwrap());
  assert_eq!(click_text["support"]["linux"], serde_json::json!({ "state": "unsupported" }));
  assert_eq!(click_text["runner_support"]["cells"]["linux"], serde_json::json!({ "state": "supported" }));

  let window_click_text = super::invoke_command_metadata(registry.resolve("window.clickText").unwrap());
  assert_eq!(
    window_click_text["runner_support"]["cells"]["linux"],
    serde_json::json!({ "state": "partial", "note": "foreground portal delivery only; background_only input policy is rejected" })
  );

  let drag = super::invoke_command_metadata(registry.resolve("input.drag").unwrap());
  assert_eq!(
    drag["runner_support"]["cells"]["macos"],
    serde_json::json!({ "state": "partial", "note": "foreground-preferred input policy is refused; DragMouse carries no window policy" })
  );

  // The direct macOS refusal hints a selected Runner may expose it remotely; the
  // Runner-side ScrollPoint handler refuses it for the same driver fact, so the
  // audited cells stay equal and the declaration is the direct shorthand.
  let scroll_point = super::invoke_command_metadata(registry.resolve("input.scrollPoint").unwrap());
  assert_eq!(scroll_point["runner_support"]["route"], serde_json::json!("direct"));
  assert_eq!(
    scroll_point["runner_support"]["cells"]["macos"],
    serde_json::json!({ "state": "unsupported", "note": "the driver exposes only window-owned scrolling" })
  );

  let type_text = super::invoke_command_metadata(registry.resolve("input.typeText").unwrap());
  assert_eq!(type_text["runner_support"]["route"], serde_json::json!("direct"));
  assert_eq!(type_text["runner_support"]["cells"], type_text["support"]);

  // Fixtures state no route at all, not "unsupported platforms": dispatch
  // refuses them before any platform judgment.
  let frame = super::invoke_command_metadata(registry.resolve("scan.frame").unwrap());
  assert_eq!(frame["runner_support"], serde_json::json!({ "route": "no_route" }));
}

// ROOT CAUSE:
//
// If the CLI help and the MCP `x-auv-commands` metadata rendered support from
// different sources, one surface could claim a path the other denies; the
// screen.clickText Linux gap (direct path refuses, Runner route works) is
// exactly a cell the two surfaces must state identically.
//
// The fix renders both surfaces from one registry declaration. This test
// proves the words a user reads in CLI help equal the JSON a model reads.
#[test]
fn cli_help_and_mcp_metadata_render_runner_support_from_the_same_registry_cells() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    let help = auv_cli_invoke::render_command_help(command);

    let direct = help_block_cells(&help, "Platform support:").expect("direct cells are listed");
    assert_eq!(
      direct,
      metadata_cells(&metadata["support"]),
      "direct cells of {} must agree between CLI help and MCP metadata",
      command.id
    );

    match metadata["runner_support"]["route"].as_str().unwrap() {
      "no_route" => {
        assert!(help.contains("no Runner route; run it without a selected Run or Device"), "fixture note of {}:\n{help}", command.id);
        assert!(metadata["runner_support"].get("cells").is_none(), "{} must not carry runner cells", command.id);
      }
      route => {
        let cells = metadata["runner_support"]["cells"].clone();
        if route == "direct" {
          assert!(help.contains("same as the direct local path above"), "direct-route note of {}:\n{help}", command.id);
          assert_eq!(cells, metadata["support"], "{}: a direct route repeats the direct cells", command.id);
        } else {
          let runner = help_block_cells(&help, "Selected Runner support:").expect("runner cells are listed");
          assert_eq!(runner, metadata_cells(&cells), "runner cells of {} must agree between CLI help and MCP metadata", command.id);
        }
      }
    }
  }
}

#[test]
fn invoke_description_states_runner_support_where_it_differs_from_direct() {
  let registry = auv_cli_invoke::default_registry();
  let description = super::invoke_tool_description(&registry);

  let click_text = description_command_block(&description, "screen.clickText");
  assert!(
    click_text.contains("\n  runner support: macOS supported; Linux supported; Windows partial"),
    "screen.clickText block:\n{click_text}"
  );

  let type_text = description_command_block(&description, "input.typeText");
  assert!(!type_text.contains("runner support:"), "equal cells stay covered by the support line:\n{type_text}");

  let frame = description_command_block(&description, "scan.frame");
  assert!(frame.contains("no Runner route"), "fixture block:\n{frame}");
}

/// The per-platform cells one help block lists, as MCP `(state, note)` pairs.
/// Help renders each cell as `<label> <word>` plus `: <note>` when the cell
/// has one, so the note is everything after the word's separating space.
fn help_block_cells<'a>(help: &'a str, header: &'a str) -> Option<Vec<(String, Option<&'a str>)>> {
  let mut lines = help.lines().skip_while(|line| *line != header);
  lines.next()?;
  let labels = ["macOS", "Linux", "Windows"];
  let mut cells = Vec::new();
  for line in lines {
    let mut tokens = line.trim_start().splitn(2, char::is_whitespace);
    let Some(label) = tokens.next() else { break };
    if !labels.contains(&label) {
      break;
    }
    let rest = tokens.next().unwrap_or_default().trim_start();
    let mut rest_tokens = rest.splitn(2, char::is_whitespace);
    let word = rest_tokens.next().unwrap_or_else(|| panic!("support word missing in {line:?}"));
    let state = match word.trim_end_matches(':') {
      "yes" => "supported",
      "partial" => "partial",
      "no" => "unsupported",
      "?" => "not_evidenced",
      other => panic!("unknown support word {other:?} in {line:?}"),
    };
    cells.push((state.to_string(), rest_tokens.next()));
  }
  (cells.len() == labels.len()).then_some(cells)
}

fn metadata_cells(cells: &serde_json::Value) -> Vec<(String, Option<&str>)> {
  ["macos", "linux", "windows"]
    .iter()
    .map(|key| {
      let cell = &cells[key];
      let state = cell["state"].as_str().unwrap_or_else(|| panic!("missing {key} cell")).to_string();
      let note = cell.get("note").and_then(|note| note.as_str());
      (state, note)
    })
    .collect()
}

fn description_command_block(description: &str, id: &str) -> String {
  description
    .split("\n\n")
    .find(|block| block.starts_with(&format!("{id}: ")))
    .unwrap_or_else(|| panic!("{id} block missing from invoke description"))
    .to_string()
}

#[test]
fn keyboard_sequence_metadata_exposes_repeat_arguments_and_target_contract() {
  let registry = auv_cli_invoke::default_registry();
  let keys = super::invoke_command_metadata(registry.resolve("input.keys").unwrap());
  assert_eq!(keys["target"]["accepted_types"], serde_json::json!(["application", "window"]));
  let arguments = keys["arguments"].as_array().unwrap();
  assert_eq!(arguments.iter().find(|arg| arg["input_key"] == "keys").unwrap()["repeated"], true);
  assert!(arguments.iter().any(|arg| arg["input_key"] == "count"));
  assert!(arguments.iter().any(|arg| arg["input_key"] == "interval-ms"));
  let sequence = super::invoke_command_metadata(registry.resolve("input.keyboard").unwrap());
  assert_eq!(sequence["target"], keys["target"]);
}

/// Server whose `op_run` executor is a counting fake, so no test drives real automation
/// (on Windows the default executor would).
fn op_run_server() -> (McpServer, Arc<FakeOperationExecutor>) {
  let fake = Arc::new(FakeOperationExecutor::new());
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap().with_op_executor(fake.clone());
  (server, fake)
}

fn op_run_request(operation: &str, mode: ExecutionModeArg) -> OpRunToolRequest {
  let fixture = match operation {
    "example.prepare_fast" => "example-prepare-fast.json",
    _ => "example-prepare.json",
  };
  OpRunToolRequest {
    operation: operation.to_string(),
    mode,
    file: crate::commands::op::tests::example_operation_file(fixture),
  }
}

#[derive(Debug, Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[test]
fn op_run_tool_schema_requires_an_explicit_mode_without_default() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let op_run = tools.iter().find(|tool| tool.name == "op_run").expect("op_run tool must be registered");
  let schema = serde_json::to_value(&op_run.input_schema).unwrap();

  let required = schema["required"].as_array().expect("required list").iter().filter_map(|name| name.as_str()).collect::<Vec<_>>();
  assert!(required.contains(&"operation"));
  assert!(required.contains(&"mode"), "mode must be required in the schema: {schema}");
  assert!(required.contains(&"file"), "file must be required: auv-cli ships no built-in operations: {schema}");
  assert_eq!(schema["additionalProperties"], false);

  // rmcp emits draft-07, so the enum lives behind a `$ref` into `definitions`.
  let mode_reference = schema["properties"]["mode"]["$ref"].as_str().expect("mode must reference the ExecutionModeArg enum");
  let mode = schema["definitions"][mode_reference.rsplit('/').next().unwrap()].clone();
  assert_eq!(mode["enum"], serde_json::json!(["fast", "verified"]));
  assert!(mode.get("default").is_none(), "mode must have no default: {mode}");
  assert!(schema["properties"]["mode"].get("default").is_none());
}

#[tokio::test]
async fn execute_op_run_fast_succeeds_without_confirmation() {
  let (server, fake) = op_run_server();

  let result = server.execute_op_run(op_run_request("example.prepare_fast", ExecutionModeArg::Fast)).await.unwrap();

  assert_eq!(result.is_error, Some(false));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "success");
  assert_eq!(output["execution_mode"], "fast");
  assert_eq!(output["confirmed"], false, "Fast must never be confirmed:true");
  assert_eq!(output["reason_code"], "EXACT_KEY_MATCH");
  assert_eq!(fake.calls_count(), 1);
}

#[tokio::test]
async fn execute_op_run_verified_is_confirmed_when_the_gate_passes() {
  let (server, _fake) = op_run_server();

  let result = server.execute_op_run(op_run_request("example.prepare", ExecutionModeArg::Verified)).await.unwrap();

  assert_eq!(result.is_error, Some(false));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "success");
  assert_eq!(output["execution_mode"], "verified");
  assert_eq!(output["confirmed"], true);
  assert_eq!(output["reason_code"], "GATE_PASSED");
}

#[tokio::test]
async fn execute_op_run_verified_gate_failure_is_an_unconfirmed_tool_error() {
  let (server, fake) = op_run_server();
  fake.set_verified_should_pass(false);

  let result = server.execute_op_run(op_run_request("example.prepare", ExecutionModeArg::Verified)).await.unwrap();

  assert_eq!(result.is_error, Some(true));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "failed");
  assert_eq!(output["confirmed"], false, "a failed gate must never report confirmed:true");
  assert_eq!(output["reason_code"], "GATE_FAILED");
}

#[tokio::test]
async fn execute_op_run_mode_mismatch_is_rejected_with_zero_driver_calls() {
  let (server, fake) = op_run_server();

  // example.prepare is Verified-only, so requesting Fast must be refused before the executor runs.
  let result = server.execute_op_run(op_run_request("example.prepare", ExecutionModeArg::Fast)).await.unwrap();

  assert_eq!(result.is_error, Some(true));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "failed");
  assert_eq!(output["confirmed"], false);
  assert_eq!(output["reason_code"], "MODE_MISMATCH_ESCALATE_VLM");
  assert_eq!(fake.calls_count(), 0, "a mode mismatch must cause zero driver side effects");
}

#[tokio::test]
async fn execute_op_run_unreadable_custom_file_is_invalid_params() {
  let (server, fake) = op_run_server();
  let request = OpRunToolRequest {
    file: std::path::PathBuf::from("does-not-exist.operation.json"),
    ..op_run_request("example.prepare_fast", ExecutionModeArg::Fast)
  };

  let error = server.execute_op_run(request).await.expect_err("an unreadable file must fail the call");

  assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
  assert_eq!(fake.calls_count(), 0);
}

// Drives the request through the real MCP transport and tool router, so argument deserialization
// is the code under test rather than a hand-built `OpRunToolRequest`.
#[tokio::test]
async fn op_run_over_mcp_rejects_a_missing_mode_or_file_before_scheduling() {
  let (server, fake) = op_run_server();
  let (server_transport, client_transport) = tokio::io::duplex(16_384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();

  let listed = client.list_all_tools().await.unwrap();
  assert!(listed.iter().any(|tool| tool.name == "op_run"), "tools/list must include op_run");

  let file = crate::commands::op::tests::example_operation_file("example-prepare-fast.json");
  let bad_arguments = [
    serde_json::json!({ "operation": "example.prepare_fast", "file": file }),
    serde_json::json!({ "operation": "example.prepare_fast", "mode": null, "file": file }),
    serde_json::json!({ "operation": "example.prepare_fast", "mode": "turbo", "file": file }),
    serde_json::json!({ "operation": "example.prepare_fast", "mode": "fast", "file": file, "unexpected": true }),
    serde_json::json!({ "operation": "example.prepare_fast", "mode": "fast" }),
  ];
  for arguments in bad_arguments {
    let outcome = client
      .call_tool(CallToolRequestParam {
        name: "op_run".into(),
        arguments: arguments.as_object().cloned(),
      })
      .await;
    match outcome {
      Err(rmcp::service::ServiceError::McpError(error)) => {
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{arguments} must be rejected as invalid params")
      }
      other => panic!("{arguments} must fail closed with invalid_params, got {other:?}"),
    }
  }
  assert_eq!(fake.calls_count(), 0, "rejected requests must never reach the executor");

  let accepted = client
    .call_tool(CallToolRequestParam {
      name: "op_run".into(),
      arguments: serde_json::json!({ "operation": "example.prepare_fast", "mode": "fast", "file": file }).as_object().cloned(),
    })
    .await
    .unwrap();
  assert_eq!(accepted.structured_content.expect("structured output")["confirmed"], false);
  assert_eq!(fake.calls_count(), 1);

  client.cancel().await.unwrap();
  server_handle.await.unwrap().unwrap();
}

/// Core adapters with `display.list` replaced by one that counts local executions.
fn counting_display_list_server(selection: auv::selection::RootSelection) -> (McpServer, Arc<std::sync::atomic::AtomicUsize>) {
  let local_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let counter = Arc::clone(&local_calls);
  let mut adapters = core_invoke_adapters().into_iter().filter(|adapter| adapter.command_id != "display.list").collect::<Vec<_>>();
  adapters.push(super::McpInvokeAdapter::new("display.list", move |_input| {
    let counter = Arc::clone(&counter);
    async move {
      counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      Ok(super::McpInvokeSuccess::empty())
    }
  }));
  let store_root = tempfile::tempdir().unwrap().keep();
  let server = McpServer::with_registry(store_root, Arc::new(auv_cli_invoke::default_registry()), adapters)
    .unwrap()
    .with_default_selection(selection);
  (server, local_calls)
}

async fn call_invoke(server: McpServer, arguments: serde_json::Value) -> rmcp::model::CallToolResult {
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: arguments.as_object().cloned(),
    })
    .await
    .expect("invoke reports Device failures as tool errors, not protocol errors")
}

// ROOT CAUSE:
//
// If `auv --device-id <guest> mcp serve` received an invoke call, the root
// selection was never passed to the MCP frontend, so the command ran on the
// local host instead (an OSWorld eval captured the operator's Mac screen).
//
// Before the fix, MCP invoke always executed the local adapter.
// The fix routes a selected invoke through the same remote context as the CLI.
#[tokio::test]
async fn mcp_invoke_with_a_root_device_selection_never_runs_locally() {
  let selection = auv::selection::RootSelection {
    credential: None,
    device_name: None,
    device_id: Some("0000000000000000000000000000000000000000000000000000000000000000".into()),
    run_id: None,
  };
  let (server, local_calls) = counting_display_list_server(selection);

  let result = call_invoke(server, serde_json::json!({ "command_id": "display.list" })).await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0, "a selected Device must not fall back to local execution");
  assert_eq!(result.is_error, Some(true), "an unreachable selected Device is a tool error: {result:?}");
}

#[tokio::test]
async fn mcp_invoke_device_fields_select_a_device_per_call() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(
    server,
    serde_json::json!({ "command_id": "display.list", "device_id": "0000000000000000000000000000000000000000000000000000000000000000" }),
  )
  .await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
  assert_eq!(result.is_error, Some(true), "{result:?}");
}

#[tokio::test]
async fn mcp_invoke_without_a_selection_still_runs_the_local_adapter() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(server, serde_json::json!({ "command_id": "display.list" })).await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
  assert_ne!(result.is_error, Some(true), "{result:?}");
}

// Pins the shared dry-run selection rule with MCP on both sides: a dry run
// that needs no Device stays local (the control plane cannot resolve a
// selection without creating a Run), while a keyboard dry run still resolves
// the selected Device because keyboard validation depends on its driver. The
// CLI applies the same rule in `commands::invoke::execute`.
#[tokio::test]
async fn mcp_invoke_dry_run_with_a_device_selection_runs_locally() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(
    server,
    serde_json::json!({
      "command_id": "display.list",
      "device_id": "0000000000000000000000000000000000000000000000000000000000000000",
      "dry_run": true,
    }),
  )
  .await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 1, "a dry run that needs no Device must not resolve one");
  assert_ne!(result.is_error, Some(true), "{result:?}");
}

#[tokio::test]
async fn mcp_invoke_keyboard_dry_run_still_selects_the_device() {
  // The rule under test decides whether this very command dispatches locally,
  // so the local `input.key` adapter itself must be the counter. A
  // `display.list` counter would stay at zero even if a broken selection rule
  // ran `input.key` locally and the run failed on its missing `key` argument,
  // which would let both assertions below pass with the rule broken.
  let local_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let counter = Arc::clone(&local_calls);
  let adapters = core_invoke_adapters()
    .into_iter()
    .filter(|adapter| adapter.command_id != "input.key")
    .chain([super::McpInvokeAdapter::new("input.key", move |_input| {
      let counter = Arc::clone(&counter);
      async move {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(super::McpInvokeSuccess::empty())
      }
    })])
    .collect::<Vec<_>>();
  let store_root = tempfile::tempdir().unwrap().keep();
  let server = McpServer::with_registry(store_root, Arc::new(auv_cli_invoke::default_registry()), adapters).unwrap();

  let result = call_invoke(
    server,
    serde_json::json!({
      "command_id": "input.key",
      "device_id": "0000000000000000000000000000000000000000000000000000000000000000",
      "dry_run": true,
    }),
  )
  .await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0, "a keyboard dry run must validate on the selected Device, not locally");
  assert_eq!(result.is_error, Some(true), "an unreachable selected Device is a tool error: {result:?}");
}

// Pins the #428 MCP presentation contract for selection failures: the caller
// gets the structured failed form with a typed failure_details.code instead of
// a bare protocol error. The specific code depends on the reachable daemon
// state, so only the typed shape is asserted.
#[tokio::test]
async fn mcp_invoke_selection_failure_keeps_typed_failure_details() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(
    server,
    serde_json::json!({ "command_id": "display.list", "device_id": "0000000000000000000000000000000000000000000000000000000000000000" }),
  )
  .await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
  assert_eq!(result.is_error, Some(true), "{result:?}");
  let output = serde_json::to_value(result.structured_content.expect("selection failures present the structured failed form")).unwrap();
  assert_eq!(output["status"], "failed");
  assert_eq!(output["command_id"], "display.list");
  assert!(output["run_id"].is_string(), "the failed presentation still carries its run id: {output}");
  let details = &output["failure_details"];
  assert!(details["code"].as_str().is_some_and(|code| !code.is_empty()), "failure_details.code must stay typed: {output}");
  assert_eq!(output["failure"], details["message"], "{output}");
}

// ROOT CAUSE:
//
// If a model client listed MCP tools, it rendered only standard schema keys,
// so the `x-auv-commands` argument metadata never reached the model. OSWorld
// agents guessed `inputs` keys (`text` instead of `query`) and list encodings
// (`"CTRL+L"` instead of a JSON array string) in about 100 failed calls.
//
// The fix renders the command catalog with inputs and examples into the
// `invoke` description.
#[tokio::test]
async fn invoke_description_lists_inputs_and_example_calls() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let (server_transport, client_transport) = tokio::io::duplex(1 << 20);
  tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  let tools = client.list_all_tools().await.unwrap();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool");
  let description = invoke.description.as_deref().unwrap_or_default();

  assert!(description.contains("  inputs.query (required)"), "{description}");
  // OSWorld agents on Linux copied the macOS example id `com.google.Chrome`
  // into most Chrome episodes; the preamble says ids differ by platform.
  assert!(description.contains("run `window.list` instead of guessing"), "{description}");
  // The same agents clicked display.capture points with an app target, which
  // the window basis shifted by the window origin.
  assert!(description.contains("Send a screen point without `target`, or with `target` and"), "{description}");
  assert!(description.contains(r#"  example: {"command_id":"input.pressKeys","inputs":{"keys":"[\"cmd\",\"shift\",\"p\"]"}}"#), "{description}");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();
  let wait = schema["x-auv-commands"].as_array().unwrap().iter().find(|command| command["id"] == "screen.waitForText").unwrap();
  assert_eq!(wait["examples"][0]["call"], serde_json::json!({ "command_id": "screen.waitForText", "inputs": { "query": "Ready" } }));
}

#[test]
fn invoke_tool_request_deserialization_handles_dry_run_and_rejects_unknown_fields() {
  use super::InvokeToolRequest;

  // 1. dry_run: true correctly decodes
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
    "dry_run": true,
  })).expect("dry_run: true should decode");
  assert_eq!(req.command_id, "test.cmd");
  assert!(req.dry_run);

  // 2. dry_run: false correctly decodes
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
    "dry_run": false,
  })).expect("dry_run: false should decode");
  assert!(!req.dry_run);

  // 3. Omitting dry_run preserves existing default semantics (false)
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
  })).expect("omitting dry_run should decode");
  assert!(!req.dry_run);

  // 4. inputs with arbitrary command-level keys is accepted
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "input.typeText",
    "inputs": { "text": "hello", "custom_key": "custom_val" },
    "dry_run": true,
  })).expect("inputs map should allow arbitrary command keys");
  assert_eq!(req.inputs.get("text").map(String::as_str), Some("hello"));
  assert_eq!(req.inputs.get("custom_key").map(String::as_str), Some("custom_val"));

  // 5. camelCase dryRun is rejected as an unknown field
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "dryRun": true,
  })).expect_err("dryRun should be rejected");
  assert!(err.to_string().contains("unknown field `dryRun`"), "error was: {err}");

  // 6. camelCase storeRoot is rejected
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "storeRoot": "/tmp",
  })).expect_err("storeRoot should be rejected");
  assert!(err.to_string().contains("unknown field `storeRoot`"), "error was: {err}");

  // 7. Other unknown top-level fields are rejected
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "unexpected_field": true,
  })).expect_err("unexpected_field should be rejected");
  assert!(err.to_string().contains("unknown field `unexpected_field`"), "error was: {err}");
}

#[test]
fn invoke_tool_schema_rejects_additional_properties_matching_serde_contract() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool must be registered");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();

  assert_eq!(schema["additionalProperties"], false, "invoke input schema must deny additionalProperties to match Serde contract: {schema}");
}

#[tokio::test]
async fn invoke_over_mcp_rejects_unknown_top_level_arguments_before_dispatch() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();

  let bad_arguments = [
    serde_json::json!({ "command_id": "display.list", "dryRun": true }),
    serde_json::json!({ "command_id": "display.list", "storeRoot": "/custom/path" }),
    serde_json::json!({ "command_id": "display.list", "unexpected": true }),
    serde_json::json!({ "command_id": "display.list", "dry_run": true, "extra": "forbidden" }),
  ];

  for arguments in bad_arguments {
    let outcome = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: arguments.as_object().cloned(),
      })
      .await;

    match outcome {
      Err(rmcp::service::ServiceError::McpError(error)) => {
        assert_eq!(
          error.code,
          ErrorCode::INVALID_PARAMS,
          "{arguments} must fail closed with INVALID_PARAMS, got: {error:?}"
        );
      }
      other => panic!("{arguments} must fail closed with McpError(INVALID_PARAMS), got: {other:?}"),
    }
  }

  assert_eq!(
    local_calls.load(std::sync::atomic::Ordering::SeqCst),
    0,
    "rejected invoke requests with unknown fields must never reach the executor/adapter"
  );

  // Legitimate invoke with dry_run: true must succeed and dispatch
  let accepted = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: serde_json::json!({ "command_id": "display.list", "dry_run": true }).as_object().cloned(),
    })
    .await
    .expect("valid invoke with dry_run: true should be accepted");

  assert_ne!(accepted.is_error, Some(true), "valid invoke must not report an error: {accepted:?}");
  assert_eq!(
    local_calls.load(std::sync::atomic::Ordering::SeqCst),
    1,
    "valid invoke must dispatch to the adapter exactly once"
  );

  client.cancel().await.unwrap();
  server_handle.await.unwrap().unwrap();
}
