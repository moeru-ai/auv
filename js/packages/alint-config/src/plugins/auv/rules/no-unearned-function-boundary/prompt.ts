export const unearnedFunctionBoundaryInstructions = `
You are reviewing one Rust source file in the AUV project.

Call report_findings exactly once. Report only function boundaries that can be removed without losing a meaningful decision or reusable contract. Use an empty findings array when there is no violation.

Every finding message must stand alone in a terse terminal formatter. Name the function, cite the concrete mechanics in its body, explain why no meaningful contract would be lost, and end with a concrete inlining or consolidation recommendation. Never emit only a function name or a category label. Put the remediation in suggestion too.
`.trim()

export const unearnedFunctionBoundaryPrompt = `
Review the target for functions that give a name and call boundary to mechanics that are clearer at their use site.

Judge responsibility, not size. A short function is worth keeping when it owns a stable policy, invariant, input grammar, error contract, external boundary, lifecycle, or independently reused operation. A longer function can still be unearned when it only spreads one expression or one operation across names.

Report a function when its body is only one or a small combination of these mechanics and its declaration does not establish a meaningful boundary:
- choosing between an input and a constant or delegated call with one boolean/count check
- choosing between two constants from one shallow condition
- constructing a value by cloning fields, supplying obvious defaults, or forwarding arguments to existing helpers
- mechanically converting between same-shaped local geometry, tuple, record, option, string, or key representations
- wrapping one formatting, mapping, lookup, constructor, method, or function call
- adding a same-file forwarding layer, including a forwarding entrypoint paired with a placeholder that only returns a fixed error
- splitting a single-use expression into a named helper when inlining makes the caller at least as clear

Consider local call sites when they are present. One use is strong evidence for inlining, but is not required when the function is plainly mechanical. Several calls can justify a boundary only when they share a rule that is meaningful and risky to duplicate; repeated shorter spelling alone is not enough.

A helper does not become a protocol or driver boundary merely because the value it constructs will later be passed to an external API. In particular, report a caller-local helper that assembles options/config by cloning a base value's fields, appending caller-supplied values, and filling an absent field with a fixed default. It owns no validation or external translation; it only stages construction for that caller. Do not apply this example to inherent constructor or fluent builder methods: those methods intentionally provide a composable caller-facing construction API, so their small bodies are the contract rather than evidence of an unearned boundary.

Report each independently removable declaration. For a chain, report the shallow wrappers that should be collapsed, not the deepest function when it owns the actual behavior. A macro attribute, visibility modifier, comment, long name, or stated intention to grow later does not by itself make the current runtime boundary meaningful.

Rule ownership: an unconditional forwarding wrapper may qualify here. A function that first makes a shallow local control-flow decision and then delegates is owned by the sibling no-vacant-control-boundary rule; do not report it here merely for that shape.

Few-shot bad case A — the names merely narrate conditions and construction mechanics:

\`\`\`rust
fn scan_area(area: Area, hit_count: usize, exhaustive: bool) -> Area {
  if exhaustive && hit_count == 0 {
    enlarge(area, FALLBACK_MARGIN)
  } else {
    area
  }
}

fn request_hints(base: &Hints, title: &str, filter: &str) -> Hints {
  Hints {
    words: append_words(&base.words, &[title, filter]),
    locales: base.locales.clone().or_else(|| Some(default_locales())),
  }
}

fn selected_strategy(hit_count: usize) -> &'static str {
  if hit_count > 0 { "focused" } else { "fallback" }
}
\`\`\`

Expected review: report all three declarations. Their names do not hide durable policy; nearby orchestration can express these small choices directly, or one real strategy/configuration boundary can own the related decisions together.

Few-shot bad case B — mechanical one-expression helpers:

\`\`\`rust
fn record_key(record: &Record) -> String {
  record.ordinal.map(|n| format!("ordinal:{n}")).unwrap_or_else(|| format!("label:{}", canonicalize(&record.label)))
}

fn local_box(raw: &transport::Box2D) -> Box2D {
  Box2D::new(raw.x, raw.y, raw.width, raw.height)
}
\`\`\`

Expected review: report both when the shown file uses them only as local spelling conveniences. The formatting and field-for-field conversion are more direct at their use sites; neither function owns validation or a cross-boundary conversion contract.

Few-shot bad case C — a required entry shape does not justify an extra empty stack:

\`\`\`rust
#[registered_command]
async fn preview(_input: CommandInput) -> CommandResult {
  preview_session().await?;
  Ok(CommandOutput::completed())
}

pub async fn preview_session() -> Result<(), String> {
  Err("preview session API is not available".to_string())
}
\`\`\`

Expected review: report the unearned boundaries. The attribute may require an entry function, but it does not make the second fixed-error function a runtime API. The command can own the current result until a real reusable session operation exists.

Few-shot good case A — shallow syntax, real CLI contract:

\`\`\`rust
fn parse_positive_delay(raw: &str) -> Result<f64, String> {
  let value = raw.parse::<f64>().map_err(|_| "expects a number".to_string())?;
  if !value.is_finite() || value <= 0.0 {
    return Err("must be greater than zero".to_string());
  }
  Ok(value)
}

fn parse_relative_box(raw: &str) -> Result<Box2D, String> {
  let parts = raw.split(',').map(str::trim).map(str::parse::<f64>).collect::<Result<Vec<_>, _>>()
    .map_err(|_| "expects x,y,width,height".to_string())?;
  if parts.len() != 4 || parts.iter().any(|part| !part.is_finite()) {
    return Err("expects four finite values".to_string());
  }
  if parts[2] <= 0.0 || parts[3] <= 0.0 {
    return Err("width and height must be positive".to_string());
  }
  Ok(Box2D::new(parts[0], parts[1], parts[2], parts[3]))
}
\`\`\`

Expected review: no findings. These functions own CLI grammars, validation order, accepted ranges, and caller-visible error semantics. Inlining would duplicate or obscure a real contract.

Few-shot good case B — a small boundary owns non-local meaning:

\`\`\`rust
fn persist_manifest(path: &Path, manifest: &Manifest) -> Result<(), StoreError> {
  let bytes = serde_json::to_vec_pretty(manifest).map_err(StoreError::Encode)?;
  atomic_write(path, &bytes).map_err(StoreError::Write)
}
\`\`\`

Expected review: no finding. Serialization choice, atomic persistence, and error translation form a durable storage boundary even though the function is short.

Few-shot good case C — Rust-required and public data-model boundaries:

\`\`\`rust
fn main() {
  application::run();
}

impl Default for ObserveMask {
  fn default() -> Self {
    Self::all()
  }
}

impl Snapshot {
  pub fn generation(&self) -> u64 {
    self.generation
  }
}
\`\`\`

Expected review: no findings. The process entry signature, required trait method, named constructor vocabulary, and encapsulating public accessor are contracts visible to callers or the Rust ecosystem. Their short implementations are not local spelling helpers.

Few-shot good case D — a compact helper owns stateful iteration:

\`\`\`rust
fn record_new_rows(&mut self, rows: &[Row]) -> bool {
  let mut changed = false;
  for row in rows {
    if self.seen.insert(row.key()) {
      self.rows.push(row.clone());
      changed = true;
    }
  }
  changed
}

fn translate_regions(mut result: Recognition, offset: Point) -> Recognition {
  for region in &mut result.regions {
    region.bounds.x -= offset.x;
    region.bounds.y -= offset.y;
  }
  result
}
\`\`\`

Expected review: no findings. The first function owns deduplication and synchronized state mutation; the second owns a collection-wide representation transform. Inlining either would bury a cohesive operation rather than remove a vacuous name.

Few-shot good case E — a production fluent builder owns construction vocabulary:

\`\`\`rust
#[derive(Default)]
pub struct RequestOptions {
  purpose: Purpose,
  content_type: ContentType,
  extension: Option<String>,
}

impl RequestOptions {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_purpose(mut self, purpose: impl Into<Purpose>) -> Self {
    self.purpose = purpose.into();
    self
  }

  pub fn with_content_type(mut self, content_type: impl Into<ContentType>) -> Self {
    self.content_type = content_type.into();
    self
  }

  pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
    self.extension = Some(extension.into());
    self
  }
}
\`\`\`

Expected review: no findings. The named constructor and chainable setters form one coherent public construction API. Their point is to let callers build the value incrementally without depending on its representation; inlining them would remove that API rather than simplify a local implementation.

Few-shot good case F — typed protocol clients adapt generated transport APIs:

\`\`\`rust
impl Client {
  pub async fn list_namespaces(&mut self) -> Result<Vec<Namespace>, tonic::Status> {
    Ok(self.inner.list_namespaces(ListNamespacesRequest {}).await?.into_inner().namespaces)
  }
}
\`\`\`

Expected review: no finding. The method is the typed client capability exposed to callers: it owns request construction, transport error type, response-envelope removal, and the client-facing result shape. Requiring callers to use the generated transport client directly would destroy that adapter boundary.

Few-shot good case G — a schema accessor centralizes presence and fallback semantics:

\`\`\`rust
fn run_id(run: &proto::Run) -> &str {
  run.r#ref.as_ref().map(|reference| reference.run_id.as_str()).unwrap_or_default()
}
\`\`\`

Expected review: no finding when orchestration calls this accessor in multiple places. It gives one meaning to an absent protobuf reference and keeps generated-schema navigation out of the owning workflow. Repeating the chain at each use would duplicate protocol-presence policy, not clarify mechanics.

Few-shot good case H — singular and plural public entrypoints are intentional API vocabulary:

\`\`\`rust
pub fn descriptor_set_for_service(service_name: &str) -> Result<Vec<u8>, String> {
  descriptor_set_for_services(&[service_name])
}
\`\`\`

Expected review: no finding. The singular public operation is an ergonomic facade over the plural operation and preserves caller-facing vocabulary without exposing slice packaging at every call site.

Do not report:
- CLI parsers or value parsers that define accepted syntax, ranges, validation, and user-facing errors
- private helpers in a CLI input parsing pipeline when they centralize trimming, empty-value rejection, deduplication, file loading, or user-facing IO errors for several accepted input spellings
- serde decoders, protocol/native adapters, or representation conversions that explicitly own a cross-module or external contract
- typed service-client methods that construct generated requests, invoke transport methods, remove response envelopes, or expose an app-owned result shape
- small repeated accessors that centralize optional protobuf-reference traversal and its absent-value semantics for an owning workflow
- functions that centralize a domain invariant or non-obvious policy used by independent callers
- trait implementations, callbacks, macro/framework entrypoints, FFI shims, or public facades when the required signature or compatibility seam is itself the API and there is no redundant same-file layer
- process entrypoints, required trait methods, named constructors, and public accessors that preserve encapsulation or caller-facing vocabulary
- inherent fluent builder methods that form a coherent construction API, including chainable single-field setters and a named constructor backed by \`Default\`
- resource acquisition/cleanup, error translation, tracing, metrics, retries, cache, permission, transaction, concurrency, or lifecycle ownership
- test helpers, fixtures, and builders whose purpose is scenario readability
- cohesive orchestration merely because some individual expressions are simple
- loops that coordinate state mutation, deduplication, side effects, early exit, or collection-wide transformation

Do not infer a violation only from a short body, a single return expression, or a low call count. For every finding, make the message follow this semantic shape: \`<function> only <specific mechanics>, which carries no <policy/contract>; <specific expression to inline or boundaries to consolidate>.\` Do not copy this wording mechanically, but include the evidence, reason, and remediation. If the proof that no meaningful contract is lost is uncertain, return no finding.
`.trim()
