use std::path::Path;
use std::sync::Arc;

use auv_tracing::{AttributeValue, Attributes, Context, EventPayload, FileTracingStore, RunId, SpanSpec, configure, dispatcher};
use serde_json::{Value, json};

use super::export;

struct NamedSpan {
  name: &'static str,
  attributes: Vec<(&'static str, &'static str)>,
}

// `SpanSpec::NAME` is a constant, so each recorded span kind gets a type.
macro_rules! span_kind {
  ($type:ident, $name:literal) => {
    struct $type(NamedSpan);
    impl SpanSpec for $type {
      const NAME: &'static str = $name;
      fn attributes(&self) -> Attributes {
        let _ = self.0.name;
        Attributes::from_iter(self.0.attributes.iter().map(|(key, value)| (key.to_string(), AttributeValue::string(*value))))
      }
    }
  };
}
span_kind!(CommandSpan, "auv.invoke.command");
span_kind!(RpcSpan, "auv.runner.rpc");

macro_rules! event_kind {
  ($type:ident, $name:literal) => {
    #[derive(serde::Serialize)]
    #[serde(transparent)]
    struct $type(Value);
    impl EventPayload for $type {
      const NAME: &'static str = $name;
      const VERSION: u32 = 1;
    }
  };
}
event_kind!(Started, "auv.invoke.command_started");
event_kind!(Finished, "auv.invoke.command_finished");
event_kind!(RpcRequest, "auv.runner.rpc_request");
event_kind!(RpcFinished, "auv.runner.rpc_finished");

fn command(id: &'static str) -> CommandSpan {
  CommandSpan(NamedSpan {
    name: "auv.invoke.command",
    attributes: vec![("command_id", id), ("execution", "local")],
  })
}

fn rpc(service: &'static str, method: &'static str) -> RpcSpan {
  RpcSpan(NamedSpan {
    name: "auv.runner.rpc",
    attributes: vec![("rpc.service", service), ("rpc.method", method)],
  })
}

/// Capture evidence for a 200x100 logical capture at screen (1000, 500), with
/// the attributes `auv_cli_invoke::artifact::capture_evidence` records. The
/// body is not an image: the export only copies it.
fn capture(capture_ref: Option<&str>) -> auv_tracing::NewArtifact<futures_util::io::Cursor<Vec<u8>>> {
  let float = |value: f64| AttributeValue::float(value).unwrap();
  let mut attributes = vec![
    ("capture.bounds.x".to_string(), float(1000.0)),
    ("capture.bounds.y".to_string(), float(500.0)),
    ("capture.bounds.width".to_string(), float(200.0)),
    ("capture.bounds.height".to_string(), float(100.0)),
    ("capture.origin.space".to_string(), AttributeValue::string("screen")),
  ];
  attributes.extend(capture_ref.map(|id| ("capture.ref_id".to_string(), AttributeValue::string(id))));
  let options = auv_tracing::EmitBytesOptions::new()
    .with_purpose("auv.driver.display_capture")
    .with_content_type("image/webp")
    .with_file_extension("webp")
    .with_attributes(Attributes::from_iter(attributes));
  auv_tracing::NewArtifact::from_bytes(options, b"webp".to_vec()).unwrap()
}

fn store(path: &Path) -> auv_tracing::Dispatch {
  configure().tracing_store(Arc::new(FileTracingStore::open(path).unwrap())).build().unwrap()
}

fn sleep() {
  // Distinct start times keep the expected step order unambiguous.
  std::thread::sleep(std::time::Duration::from_millis(3));
}

fn read_trajectory(out: &Path) -> Value {
  serde_json::from_str(&std::fs::read_to_string(out.join("trajectory.json")).unwrap()).unwrap()
}

#[test]
fn a_local_capture_then_click_exports_ordered_steps_with_a_mapped_pointer() {
  let root = tempfile::tempdir().unwrap();
  let dispatch = store(root.path());
  let run_id = RunId::new();
  let context = dispatcher::with_default(&dispatch, || Context::root(run_id));
  context.in_scope(|| {
    let span = auv_tracing::start_span(command("display.capture"));
    span.in_scope(|| {
      auv_tracing::emit_event!(Started(json!({ "command_id": "display.capture", "inputs": {}, "dry_run": false })));
      drop(auv_tracing::emit_artifact(capture(None)));
      auv_tracing::emit_event!(Finished(json!({ "command_id": "display.capture", "status": "succeeded", "result": {} })));
    });
    drop(span);
    sleep();
    let span = auv_tracing::start_span(command("input.clickPoint"));
    span.in_scope(|| {
      auv_tracing::emit_event!(Started(
        json!({ "command_id": "input.clickPoint", "inputs": { "x": "1100", "y": "560", "button": "right" }, "dry_run": false })
      ));
      auv_tracing::emit_event!(Finished(json!({
        "command_id": "input.clickPoint",
        "status": "succeeded",
        "result": { "relative_to": "screen", "screen_point": { "x": 1100.0, "y": 560.0 } },
      })));
    });
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let out = root.path().join("export");
  let exported = export(&[root.path().to_path_buf()], &[run_id], &out).unwrap();
  assert_eq!((exported.steps, exported.images, exported.skipped_records), (2, 1, 0));
  let trajectory = read_trajectory(&out);
  assert_eq!(trajectory["schema_version"], "ATIF-v1.8");
  assert_eq!(trajectory["session_id"], run_id.to_string());
  let steps = trajectory["steps"].as_array().unwrap();
  assert_eq!(steps.iter().map(|step| step["step_id"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2]);
  assert_eq!(steps[0]["tool_calls"][0]["function_name"], "display.capture");

  let capture_image = &steps[0]["observation"]["results"][0]["content"][1];
  assert_eq!(capture_image["type"], "image");
  assert_eq!(capture_image["source"]["media_type"], "image/webp");
  let image_path = capture_image["source"]["path"].as_str().unwrap();
  assert!(out.join(image_path).is_file(), "the referenced image is copied next to the trajectory");

  let click = &steps[1];
  assert_eq!(click["tool_calls"][0]["arguments"]["button"], "right");
  assert_eq!(
    click["tool_calls"][0]["extra"]["pointer"],
    json!({ "x": 100.0, "y": 60.0, "kind": "right_click", "image": image_path }),
    "screen (1100, 560) is pixel (100, 60) of the capture at (1000, 500)"
  );
  assert_eq!(click["observation"]["results"][0]["source_call_id"], click["tool_calls"][0]["tool_call_id"]);
  assert_eq!(click["extra"]["auv"]["before_image"], image_path);
  assert!(click["extra"]["latency_ms"].as_f64().unwrap() >= 0.0);
}

#[test]
fn sdk_calls_become_steps_and_remote_command_calls_stay_inside_their_command() {
  let root = tempfile::tempdir().unwrap();
  let frontend = store(root.path());
  let runner_path = root.path().join("runner-traces").join("runner_1");
  let runner = store(&runner_path);
  let run_id = RunId::new();

  // A CLI command routed to the Runner: its RPC nests under the command span.
  let command_span =
    dispatcher::with_default(&frontend, || Context::root(run_id)).in_scope(|| auv_tracing::start_span(command("display.capture")));
  let command_span_id = *command_span.id().unwrap();
  command_span.in_scope(|| {
    auv_tracing::emit_event!(Started(json!({ "command_id": "display.capture", "inputs": {}, "dry_run": false })));
  });
  let remote =
    dispatcher::with_default(&runner, || Context::from_remote(auv_tracing::RemoteContext::new(run_id, Some(command_span_id))).unwrap());
  let nested = remote.in_scope(|| auv_tracing::start_span(rpc("auv.api.driver.v1.CaptureService", "CaptureDisplay")));
  nested.in_scope(|| drop(auv_tracing::emit_artifact(capture(Some("cap-1")))));
  drop(nested);
  command_span.in_scope(|| {
    auv_tracing::emit_event!(Finished(json!({ "command_id": "display.capture", "status": "succeeded", "result": {} })));
  });
  drop(command_span);
  sleep();

  // An SDK call with only the daemon-injected Run context.
  let sdk = dispatcher::with_default(&runner, || Context::from_remote(auv_tracing::RemoteContext::new(run_id, None)).unwrap());
  let call = sdk.in_scope(|| auv_tracing::start_span(rpc("auv.api.driver.v1.InputService", "ClickPoint")));
  call.in_scope(|| {
    auv_tracing::emit_event!(RpcRequest(
      json!({ "service": "auv.api.driver.v1.InputService", "method": "ClickPoint", "message": { "position": {} } })
    ));
    auv_tracing::emit_event!(RpcFinished(json!({
      "service": "auv.api.driver.v1.InputService",
      "method": "ClickPoint",
      "grpc_status": 0,
      "completed": true,
      "response": { "message": { "screenPoint": { "x": 1010.0, "y": 505.0 } } },
      "request_messages": 1,
      "response_messages": 1,
    })));
  });
  drop(call);
  futures_executor::block_on(frontend.flush()).unwrap();
  futures_executor::block_on(runner.flush()).unwrap();

  let out = root.path().join("export");
  let exported = export(&[root.path().to_path_buf()], &[run_id], &out).unwrap();
  assert_eq!(exported.steps, 2, "the nested RPC is part of its command, not a step");
  let trajectory = read_trajectory(&out);
  let steps = trajectory["steps"].as_array().unwrap();
  assert_eq!(steps[0]["tool_calls"][0]["function_name"], "display.capture");
  assert_eq!(steps[0]["observation"]["results"][0]["content"][1]["type"], "image", "a remote command shows its Runner's capture");
  assert_eq!(steps[1]["tool_calls"][0]["function_name"], "auv.api.driver.v1.InputService/ClickPoint");
  assert_eq!(steps[1]["tool_calls"][0]["arguments"], json!({ "position": {} }));
  assert_eq!(steps[1]["tool_calls"][0]["extra"]["pointer"]["kind"], "click");
  assert_eq!(steps[1]["tool_calls"][0]["extra"]["pointer"]["x"], 10.0);
  assert_eq!(steps[1]["extra"]["auv"]["rpc"], "auv.api.driver.v1.InputService/ClickPoint");
}

#[test]
fn an_unknown_run_is_an_error_not_an_empty_trajectory() {
  let root = tempfile::tempdir().unwrap();
  let dispatch = store(root.path());
  dispatcher::with_default(&dispatch, || Context::root(RunId::new())).in_scope(|| drop(auv_tracing::start_span(command("display.capture"))));
  futures_executor::block_on(dispatch.flush()).unwrap();
  let error = export(&[root.path().to_path_buf()], &[RunId::new()], &root.path().join("out")).unwrap_err();
  assert!(error.contains("has no recorded commands"), "{error}");
}

#[test]
fn a_torn_final_line_is_skipped_and_counted() {
  let root = tempfile::tempdir().unwrap();
  let dispatch = store(root.path());
  let run_id = RunId::new();
  dispatcher::with_default(&dispatch, || Context::root(run_id)).in_scope(|| drop(auv_tracing::start_span(command("input.moveMouse"))));
  futures_executor::block_on(dispatch.flush()).unwrap();
  use std::io::Write as _;
  std::fs::OpenOptions::new().append(true).open(root.path().join("records.jsonl")).unwrap().write_all(b"{\"version\":2,\"rec").unwrap();

  let exported = export(&[root.path().to_path_buf()], &[run_id], &root.path().join("out")).unwrap();
  assert_eq!((exported.steps, exported.skipped_records), (1, 1));
  let trajectory = read_trajectory(&root.path().join("out"));
  assert_eq!(trajectory["steps"][0]["extra"]["auv"]["status"], "unfinished");
}

#[test]
fn one_run_per_call_merges_into_one_trajectory_in_time_order() {
  // Without an explicit Run, each `auv invoke` or MCP call records its own Run.
  let root = tempfile::tempdir().unwrap();
  let dispatch = store(root.path());
  let first = RunId::new();
  let second = RunId::new();
  let unrelated = RunId::new();
  for (run_id, command_id) in [
    (first, "input.moveMouse"),
    (unrelated, "display.list"),
    (second, "input.typeText"),
  ] {
    dispatcher::with_default(&dispatch, || Context::root(run_id)).in_scope(|| drop(auv_tracing::start_span(command(command_id))));
    sleep();
  }
  futures_executor::block_on(dispatch.flush()).unwrap();

  let out = root.path().join("out");
  assert_eq!(export(&[root.path().to_path_buf()], &[second, first], &out).unwrap().steps, 2);
  let trajectory = read_trajectory(&out);
  let steps = trajectory["steps"].as_array().unwrap();
  assert_eq!(steps[0]["tool_calls"][0]["function_name"], "input.moveMouse");
  assert_eq!(steps[0]["extra"]["auv"]["run_id"], first.to_string());
  assert_eq!(steps[1]["extra"]["auv"]["run_id"], second.to_string());
  assert_eq!(trajectory["extra"]["auv"]["run_ids"], json!([second.to_string(), first.to_string()]));
}

#[test]
fn a_drag_records_where_it_started_and_ended_on_the_screenshot() {
  let root = tempfile::tempdir().unwrap();
  let dispatch = store(root.path());
  let run_id = RunId::new();
  let context = dispatcher::with_default(&dispatch, || Context::root(run_id));
  context.in_scope(|| {
    let span = auv_tracing::start_span(command("display.capture"));
    span.in_scope(|| drop(auv_tracing::emit_artifact(capture(None))));
    drop(span);
    sleep();
    let span = auv_tracing::start_span(command("input.drag"));
    span.in_scope(|| {
      auv_tracing::emit_event!(Finished(json!({
        "command_id": "input.drag",
        "status": "succeeded",
        "result": { "screen_start": { "x": 1010.0, "y": 520.0 }, "screen_end": { "x": 1150.0, "y": 590.0 } },
      })));
    });
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let out = root.path().join("out");
  export(&[root.path().to_path_buf()], &[run_id], &out).unwrap();
  let pointer = &read_trajectory(&out)["steps"][1]["tool_calls"][0]["extra"]["pointer"];
  assert_eq!(pointer["kind"], "drag");
  assert_eq!((pointer["x"].as_f64(), pointer["y"].as_f64()), (Some(150.0), Some(90.0)));
  assert_eq!(pointer["start"], json!({ "x": 10.0, "y": 20.0 }));
}

#[test]
fn listing_counts_the_steps_an_export_would_write() {
  // A Runner call nested under a command is part of that command's step, so
  // the listing must not count it separately.
  let root = tempfile::tempdir().unwrap();
  let frontend = store(root.path());
  let runner = store(&root.path().join("runner-traces").join("runner_1"));
  let run_id = RunId::new();
  let command_span =
    dispatcher::with_default(&frontend, || Context::root(run_id)).in_scope(|| auv_tracing::start_span(command("display.list")));
  let remote = dispatcher::with_default(&runner, || {
    Context::from_remote(auv_tracing::RemoteContext::new(run_id, Some(*command_span.id().unwrap()))).unwrap()
  });
  drop(remote.in_scope(|| auv_tracing::start_span(rpc("auv.api.driver.v1.DisplayService", "ListDisplays"))));
  drop(command_span);
  futures_executor::block_on(frontend.flush()).unwrap();
  futures_executor::block_on(runner.flush()).unwrap();

  let runs = super::runs(&[root.path().to_path_buf()]).unwrap();
  assert_eq!(runs.iter().map(|(run, _, steps)| (*run, *steps)).collect::<Vec<_>>(), [(run_id, 1)]);
}
