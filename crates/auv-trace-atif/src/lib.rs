//! ATIF projection of recorded AUV Runs: a standalone tool, not part of the
//! `auv` CLI. ATIF belongs to its consumers (vieval, Harbor); AUV's own
//! contract is the trace store this crate reads.
//!
//! Harbor's Agent Trajectory Interchange Format (ATIF v1.8,
//! `rfcs/0001-trajectory-format.md` in `github.com/harbor-framework/harbor`)
//! is a lossy, presentation-oriented view of AUV trace records: one agent step
//! per action, the action as a tool call, captures as image parts of the
//! observation. It reads durable stores only, never a live frontend.
//!
//! Steps come from two producers:
//!
//! - `auv.invoke.command` spans (CLI, MCP). Runner RPC spans under a command
//!   span belong to that step.
//! - `auv.runner.rpc` spans with no recorded command parent (`@auv-js/sdk`):
//!   each such call is its own step.
//!
//! The model's message, reasoning, and token usage are not AUV's to know; an
//! eval merges them into the exported steps by `extra.auv.span_id`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use auv_tracing::{ArtifactMetadata, AttributeValue, FileTracingStore, RunId, SpanId, Timestamp, TraceRecord};
use serde_json::{Value, json};

const SCHEMA_VERSION: &str = "ATIF-v1.8";

// NOTICE(atif-record-names): these are the producers' record names, read as
// a durable contract: `auv_cli_invoke::trace` (command span and events) and
// `auv-cli`'s `runner::recording` (Runner RPC span and events). This tool does
// not depend on those crates, which pull in every platform driver; rename
// them together.
const COMMAND_SPAN: &str = "auv.invoke.command";
const COMMAND_STARTED: &str = "auv.invoke.command_started";
const COMMAND_FINISHED: &str = "auv.invoke.command_finished";
const RPC_SPAN: &str = "auv.runner.rpc";
/// Observation text stays readable; the full result is in the trace store.
const MAX_OBSERVATION_TEXT: usize = 2000;

/// What one export wrote.
#[derive(Debug)]
pub struct Exported {
  pub trajectory: PathBuf,
  pub steps: usize,
  pub images: usize,
  /// Lines that could not be read, such as a torn final line after a crash.
  pub skipped_records: usize,
}

/// Exports `run_ids` from `stores` as one `<out>/trajectory.json` plus the
/// images it references under `<out>/images/`.
///
/// Several Runs merge into one trajectory in time order: without an explicit
/// Run, every `auv invoke` and MCP `invoke` call records its own Run, so an
/// agent episode driven that way is one Run per step.
///
/// Each store's `runner-traces/*` sub-stores (written by daemon-owned Runners)
/// are read too, so the daemon's store root covers SDK-driven Runs.
pub fn export(stores: &[PathBuf], run_ids: &[RunId], out: &Path) -> Result<Exported, String> {
  let mut loaded = Loaded::default();
  for store in expand_stores(stores)? {
    loaded.read(&store, run_ids)?;
  }
  let steps = loaded.steps();
  let runs = run_ids.iter().map(ToString::to_string).collect::<Vec<_>>();
  if steps.is_empty() {
    return Err(format!(
      "Run {} has no recorded commands or Runner calls in {}",
      runs.join(", "),
      stores.iter().map(|store| store.display().to_string()).collect::<Vec<_>>().join(", ")
    ));
  }
  let images = out.join("images");
  std::fs::create_dir_all(&images).map_err(|error| format!("failed to create {}: {error}", images.display()))?;
  let mut copied = 0;
  let mut previous_image: Option<&Image> = None;
  let mut atif_steps = Vec::with_capacity(steps.len());
  for (index, step) in steps.iter().enumerate() {
    for image in &step.images {
      let target = images.join(image.file_name());
      std::fs::copy(&image.path, &target).map_err(|error| format!("failed to copy {}: {error}", image.path.display()))?;
      copied += 1;
    }
    atif_steps.push(step.to_atif(index + 1, previous_image));
    if let Some(last) = step.images.last() {
      previous_image = Some(last);
    }
  }
  let trajectory = json!({
    "schema_version": SCHEMA_VERSION,
    // One session per export; each step names its own Run in `extra.auv`.
    "session_id": runs[0],
    "agent": { "name": "auv", "version": env!("CARGO_PKG_VERSION") },
    "steps": atif_steps,
    "extra": { "auv": { "run_ids": runs, "skipped_records": loaded.skipped } },
  });
  let path = out.join("trajectory.json");
  let text = serde_json::to_string_pretty(&trajectory).map_err(|error| format!("failed to encode trajectory: {error}"))?;
  std::fs::write(&path, text).map_err(|error| format!("failed to write {}: {error}", path.display()))?;
  Ok(Exported {
    trajectory: path,
    steps: steps.len(),
    images: copied,
    skipped_records: loaded.skipped,
  })
}

fn expand_stores(stores: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
  let mut expanded = Vec::new();
  for store in stores {
    if store.join("records.jsonl").is_file() {
      expanded.push(store.clone());
    }
    let runners = store.join("runner-traces");
    if let Ok(entries) = std::fs::read_dir(&runners) {
      let mut nested =
        entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.join("records.jsonl").is_file()).collect::<Vec<_>>();
      nested.sort();
      expanded.extend(nested);
    }
  }
  if expanded.is_empty() {
    return Err(format!(
      "no trace store found in {}",
      stores.iter().map(|store| store.display().to_string()).collect::<Vec<_>>().join(", ")
    ));
  }
  Ok(expanded)
}

#[derive(Default)]
struct Loaded {
  spans: HashMap<SpanId, Span>,
  skipped: usize,
}

struct Span {
  run_id: RunId,
  name: String,
  /// The local parent, or the remote caller's span for a Runner call.
  parent: Option<SpanId>,
  started_at: Timestamp,
  ended_at: Option<Timestamp>,
  attributes: auv_tracing::Attributes,
  events: Vec<(String, Value)>,
  images: Vec<Image>,
  /// Load order, which breaks ties between equal start times.
  order: usize,
}

#[derive(Clone)]
struct Image {
  path: PathBuf,
  metadata: ArtifactMetadata,
  occurred_at: Timestamp,
}

impl Image {
  fn file_name(&self) -> String {
    format!("{}.{}", self.metadata.uri().artifact_id(), self.metadata.file_extension().unwrap_or("bin"))
  }

  fn relative_path(&self) -> String {
    format!("images/{}", self.file_name())
  }

  fn float(&self, key: &str) -> Option<f64> {
    match self.metadata.attributes().get(key)? {
      AttributeValue::F64(value) => Some(value.get()),
      AttributeValue::I64(value) => Some(*value as f64),
      _ => None,
    }
  }

  /// Maps a logical screen point onto this image's pixels. Evidence images are
  /// stored at logical resolution, so the offset from the capture's screen
  /// origin is the pixel position.
  fn pixel(&self, point: (f64, f64)) -> Option<(f64, f64)> {
    let (x, y) = (point.0 - self.float("capture.bounds.x")?, point.1 - self.float("capture.bounds.y")?);
    let (width, height) = (self.float("capture.bounds.width")?, self.float("capture.bounds.height")?);
    (x >= 0.0 && y >= 0.0 && x <= width && y <= height).then_some((x, y))
  }
}

impl Loaded {
  fn read(&mut self, store: &Path, run_ids: &[RunId]) -> Result<(), String> {
    let records = FileTracingStore::read_records(store).map_err(|error| format!("failed to read {}: {error}", store.display()))?;
    let mut pending_events = Vec::new();
    let mut pending_images = Vec::new();
    for record in records {
      let record = match record {
        Ok(record) => record.record,
        Err(error) => {
          eprintln!("warning: skipping {} {error}", store.display());
          self.skipped += 1;
          continue;
        }
      };
      if !run_ids.contains(&record.run_id()) {
        continue;
      }
      match record {
        TraceRecord::SpanStarted {
          run_id,
          span_id,
          parent_span_id,
          remote_span_id,
          name,
          started_at,
          attributes,
        } => {
          let order = self.spans.len();
          self.spans.insert(
            span_id,
            Span {
              run_id,
              name: name.as_str().to_string(),
              parent: parent_span_id.or(remote_span_id),
              started_at,
              ended_at: None,
              attributes,
              events: Vec::new(),
              images: Vec::new(),
              order,
            },
          );
        }
        TraceRecord::SpanEnded {
          span_id, ended_at, ..
        } => {
          if let Some(span) = self.spans.get_mut(&span_id) {
            span.ended_at = Some(ended_at);
          }
        }
        TraceRecord::Event {
          span_id,
          schema,
          payload,
          ..
        } => {
          if let (Some(span_id), Ok(value)) = (span_id, serde_json::from_str(payload.get())) {
            pending_events.push((span_id, schema.name().as_str().to_string(), value));
          }
        }
        TraceRecord::Artifact {
          span_id,
          occurred_at,
          metadata,
          ..
        } => {
          if let Some(span_id) = span_id
            && metadata.content_type().as_str().starts_with("image/")
          {
            pending_images.push((
              span_id,
              Image {
                path: FileTracingStore::artifact_path_in(store, &metadata),
                metadata,
                occurred_at,
              },
            ));
          }
        }
      }
    }
    // Attach after the whole store is read: an artifact record can follow its
    // span's end, because bodies are written before their record.
    for (span_id, name, value) in pending_events {
      if let Some(span) = self.spans.get_mut(&span_id) {
        span.events.push((name, value));
      }
    }
    for (span_id, image) in pending_images {
      if let Some(span) = self.spans.get_mut(&span_id) {
        span.images.push(image);
      }
    }
    Ok(())
  }

  /// One step per command span, plus one per Runner call outside any command.
  fn steps(&self) -> Vec<Step<'_>> {
    let commands = self.spans.iter().filter(|(_, span)| span.name == COMMAND_SPAN).map(|(id, _)| *id).collect::<Vec<_>>();
    let mut steps = Vec::new();
    for (id, span) in &self.spans {
      let owned_by_command = self.ancestor(span, |candidate| candidate.name == COMMAND_SPAN).is_some();
      let is_step = span.name == COMMAND_SPAN || (span.name == RPC_SPAN && !owned_by_command);
      if !is_step {
        continue;
      }
      // A command's own evidence wins; its Runner calls' images are used only
      // when the command recorded none (the remote path records captures in
      // both places).
      let mut images = span.images.clone();
      if images.is_empty() && commands.contains(id) {
        for child in self.spans.values() {
          if self.ancestor(child, |candidate| std::ptr::eq(candidate, span)).is_some() {
            images.extend(child.images.iter().cloned());
          }
        }
      }
      images.sort_by_key(|image| image.occurred_at);
      steps.push(Step {
        id: *id,
        span,
        images,
      });
    }
    steps.sort_by_key(|step| (step.span.started_at, step.span.order));
    steps
  }

  fn ancestor(&self, span: &Span, matches: impl Fn(&Span) -> bool) -> Option<&Span> {
    let mut parent = span.parent;
    // Bounded walk: a malformed store cannot loop forever.
    for _ in 0..64 {
      let candidate = self.spans.get(&parent?)?;
      if matches(candidate) {
        return Some(candidate);
      }
      parent = candidate.parent;
    }
    None
  }
}

struct Step<'a> {
  id: SpanId,
  span: &'a Span,
  images: Vec<Image>,
}

impl Step<'_> {
  fn event(&self, name: &str) -> Option<&Value> {
    self.span.events.iter().find(|(event, _)| event == name).map(|(_, value)| value)
  }

  fn is_command(&self) -> bool {
    self.span.name == COMMAND_SPAN
  }

  fn function_name(&self) -> String {
    let attribute = |key| match self.span.attributes.get(key) {
      Some(AttributeValue::String(value)) => value.clone(),
      _ => String::new(),
    };
    if self.is_command() {
      attribute("command_id")
    } else {
      format!("{}/{}", attribute("rpc.service"), attribute("rpc.method"))
    }
  }

  fn arguments(&self) -> Value {
    let arguments = if self.is_command() {
      self.event(COMMAND_STARTED).and_then(|started| started.get("inputs")).cloned()
    } else {
      self.event("auv.runner.rpc_request").and_then(|request| request.get("message")).cloned()
    };
    match arguments {
      Some(Value::Object(map)) => Value::Object(map),
      _ => json!({}),
    }
  }

  /// The direct result (command) or first response message (Runner call).
  fn result(&self) -> Option<&Value> {
    if self.is_command() {
      self.event(COMMAND_FINISHED)?.get("result")
    } else {
      self.event("auv.runner.rpc_finished")?.get("response")?.get("message")
    }
  }

  fn status(&self) -> (&'static str, Option<String>) {
    if self.is_command() {
      return match self.event(COMMAND_FINISHED) {
        Some(finished) if finished["status"] == "succeeded" => ("succeeded", None),
        Some(finished) => ("failed", finished["failure"]["message"].as_str().map(str::to_string)),
        None => ("unfinished", None),
      };
    }
    match self.event("auv.runner.rpc_finished") {
      Some(finished) if finished["grpc_status"] == 0 => ("succeeded", None),
      Some(finished) if finished.get("grpc_status").is_some() => ("failed", finished["grpc_message"].as_str().map(str::to_string)),
      _ => ("unfinished", None),
    }
  }

  fn to_atif(&self, step_id: usize, previous_image: Option<&Image>) -> Value {
    let call_id = self.id.to_string();
    let function_name = self.function_name();
    let arguments = self.arguments();
    let (status, failure) = self.status();
    let mut text = match &failure {
      Some(message) => format!("{status}: {message}"),
      None => status.to_string(),
    };
    if let Some(result) = self.result() {
      text.push('\n');
      text.push_str(&truncate(&result.to_string(), MAX_OBSERVATION_TEXT));
    }
    let mut content = vec![json!({ "type": "text", "text": text })];
    content.extend(self.images.iter().map(
      |image| json!({ "type": "image", "source": { "media_type": image.metadata.content_type().as_str(), "path": image.relative_path() } }),
    ));

    let mut auv = json!({
      "run_id": self.span.run_id.to_string(),
      "span_id": call_id,
      "status": status,
    });
    if self.is_command() {
      auv["command_id"] = json!(function_name);
    } else {
      auv["rpc"] = json!(function_name);
    }
    let mut tool_call = json!({ "tool_call_id": call_id, "function_name": function_name, "arguments": arguments });
    if let Some(action) = pointer_action(&function_name, &arguments, self.result()) {
      auv["screen_point"] = json!({ "x": action.point.0, "y": action.point.1 });
      // Drawn on the screenshot the actor last saw: the most recent capture
      // before this step, else this step's own first capture.
      let reference = previous_image.filter(|image| image.occurred_at <= self.span.started_at).or(self.images.first());
      if let Some((image, (x, y))) = reference.and_then(|image| image.pixel(action.point).map(|pixel| (image, pixel))) {
        let mut pointer = json!({ "x": x, "y": y, "kind": action.kind, "image": image.relative_path() });
        if let Some((start_x, start_y)) = action.start.and_then(|start| image.pixel(start)) {
          pointer["start"] = json!({ "x": start_x, "y": start_y });
        }
        tool_call["extra"] = json!({ "pointer": pointer });
      }
    }
    if let Some(image) = previous_image {
      auv["before_image"] = json!(image.relative_path());
    }
    let mut extra = json!({ "auv": auv });
    if let Some(ended_at) = self.span.ended_at {
      extra["latency_ms"] = json!(milliseconds_between(self.span.started_at, ended_at));
    }
    json!({
      "step_id": step_id,
      "timestamp": iso8601(self.span.started_at),
      "source": "agent",
      // The eval replaces this with the model's message for the step.
      "message": "",
      "tool_calls": [tool_call],
      "observation": { "results": [{ "source_call_id": self.id.to_string(), "content": content }] },
      "extra": extra,
    })
  }
}

struct PointerAction {
  point: (f64, f64),
  /// Where a drag began; `point` is where it ended.
  start: Option<(f64, f64)>,
  kind: &'static str,
}

/// Derives a pointer action from a step's call name, arguments, and result.
///
/// Command results name logical screen points in snake_case (`screen_point`,
/// `screen_start`), Runner responses in protobuf JSON camelCase
/// (`screenPoint`). Keyboard and capture calls have no pointer.
// TODO(atif-pointer-streams): `MoveMouse` and `StreamMouseMotion` record only
// their first response message, which holds the start, not the end point.
fn pointer_action(function_name: &str, arguments: &Value, result: Option<&Value>) -> Option<PointerAction> {
  let name = function_name.to_ascii_lowercase();
  let result = result?;
  let point = |value: &Value| Some((value.get("x")?.as_f64()?, value.get("y")?.as_f64()?));
  let field = |names: &[&str]| names.iter().find_map(|name| result.get(*name).and_then(point));
  let kind = if name.contains("drag") {
    "drag"
  } else if name.contains("scroll") {
    "scroll"
  } else if name.contains("click") {
    let button = arguments.get("button").and_then(Value::as_str).unwrap_or("left");
    let count = arguments
      .get("click-count")
      .and_then(|value| value.as_str().and_then(|text| text.parse().ok()).or(value.as_u64()))
      .or_else(|| arguments.pointer("/options/click/count").and_then(Value::as_u64))
      .unwrap_or(1);
    if button.eq_ignore_ascii_case("right") || arguments.pointer("/options/button").and_then(Value::as_str) == Some("MOUSE_BUTTON_RIGHT") {
      "right_click"
    } else if count >= 2 {
      "double_click"
    } else {
      "click"
    }
  } else if name.contains("move") {
    "move"
  } else {
    return None;
  };
  let point = match kind {
    "drag" => field(&["screen_end", "screenEnd", "point"]),
    _ => field(&["screen_point", "screenPoint", "point"]),
  }?;
  // TODO(atif-pointer-streams): `DragMouse` responses carry only the end; its
  // start is inside the request's motion curve and is not extracted.
  let start = (kind == "drag").then(|| field(&["screen_start", "screenStart"])).flatten();
  Some(PointerAction { point, start, kind })
}

fn truncate(text: &str, limit: usize) -> String {
  if text.len() <= limit {
    return text.to_string();
  }
  let mut end = limit;
  while !text.is_char_boundary(end) {
    end -= 1;
  }
  format!("{}…", &text[..end])
}

fn iso8601(timestamp: Timestamp) -> String {
  chrono::DateTime::<chrono::Utc>::from_timestamp(timestamp.unix_seconds(), timestamp.nanoseconds())
    .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
    .unwrap_or_default()
}

fn milliseconds_between(start: Timestamp, end: Timestamp) -> f64 {
  let nanos = |time: Timestamp| i128::from(time.unix_seconds()) * 1_000_000_000 + i128::from(time.nanoseconds());
  (nanos(end) - nanos(start)) as f64 / 1_000_000.0
}

/// Lists the Runs present in `stores`, newest first, with the number of steps
/// [`export`] would write for each, for `--run` discovery.
pub fn runs(stores: &[PathBuf]) -> Result<Vec<(RunId, Timestamp, usize)>, String> {
  let stores = expand_stores(stores)?;
  let mut run_ids = Vec::new();
  for store in &stores {
    let records = FileTracingStore::read_records(store).map_err(|error| format!("failed to read {}: {error}", store.display()))?;
    for record in records.filter_map(Result::ok) {
      if !run_ids.contains(&record.record.run_id()) {
        run_ids.push(record.record.run_id());
      }
    }
  }
  let mut runs = Vec::new();
  for run_id in run_ids {
    let mut loaded = Loaded::default();
    for store in &stores {
      loaded.read(store, &[run_id])?;
    }
    let steps = loaded.steps();
    if let Some(first) = steps.first() {
      runs.push((run_id, first.span.started_at, steps.len()));
    }
  }
  runs.sort_by_key(|(_, started, _)| std::cmp::Reverse(*started));
  Ok(runs)
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
