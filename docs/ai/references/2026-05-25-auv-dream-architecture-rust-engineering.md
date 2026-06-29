# AUV Target Architecture And Rust Engineering Reference

Date: 2026-05-25

Status: reference proposal, not an implementation plan

This document consolidates an architecture audit of the current AUV repository
with lessons from influential Rust and automation projects. It is intended to
answer what should be split, where responsibilities should live, which names are
worth keeping, and what "dream AUV" should look like as a usable runtime and API
surface.

It supersedes several assumptions in
`docs/ai/references/2026-05-12-auv-setup.md`, while preserving its core
direction: AUV is not just a CLI wrapper and not a generic LLM agent. AUV should
be a runtime, recording, inspection, recipe, and driver system that can expose
multiple frontends over one execution model.

## External References Worth Borrowing From

The most relevant projects are not direct clones of AUV. They are useful because
they show how mature Rust systems draw boundaries around frontends, core types,
runtimes, generated APIs, protocol clients, sessions, and drivers.

### kube-rs

Source:

- https://kube.rs/architecture/
- https://github.com/kube-rs/kube

Useful pattern:

- `kube` is a facade crate.
- `kube-core` holds API concepts and traits.
- `kube-client` owns client transport and API calls.
- `kube-runtime` owns higher-level controller runtime behavior.
- `kube-derive` owns macros.

AUV should copy the dependency direction, not the Kubernetes domain:

- `auv-core` should define IDs, refs, errors, capability traits, and operation
  schemas.
- `auv-runtime` should depend on `auv-core`, `auv-trace`, `auv-store`, and
  driver traits.
- `auv-cli`, MCP, JS bindings, and future UI servers should sit above runtime.
- A facade crate can re-export stable user-facing pieces while preventing cyclic
  dependencies.

### rust-analyzer

Source:

- https://rust-analyzer.github.io/book/contributing/architecture.html
- https://github.com/rust-lang/rust-analyzer/tree/master/crates

Useful pattern:

- It explicitly marks API boundaries.
- It keeps protocol serialization at the outer LSP crate, not in core semantic
  crates.
- It distinguishes internal computation crates from facade crates.
- It uses typed IDs, snapshots, and immutable views to make callers pleasant.
- It records architecture invariants in documentation.

AUV should adopt this rule: core runtime and observation types should not become
serializable just because one frontend needs JSON. Instead, define explicit
wire/API DTOs at the boundary for CLI JSON, MCP, WebDriver-compatible APIs, and
inspect-server APIs.

### Wasmtime

Source:

- https://docs.wasmtime.dev/contributing-architecture.html
- https://docs.wasmtime.dev/contributing-ci.html
- https://docs.wasmtime.dev/contributing-coding-guidelines.html
- https://github.com/bytecodealliance/wasmtime

Useful pattern:

- A safe public runtime API sits above lower-level implementation crates.
- CLI behavior and embedding API are not the same layer.
- Platform, generated, and C API boundaries are explicit.
- CI includes formatting, clippy, tests, docs, release artifacts, and platform
  matrix work.

AUV should treat macOS native Swift, future C/JS bindings, and generated command
schemas as boundary code, not as domain model code.

### Deno

Source:

- https://docs.deno.com/runtime/contributing/architecture/
- https://github.com/denoland/deno

Useful pattern:

- Runtime, CLI, extension crates, ops, permissions, and resources are separate
  concepts.
- Deno's `rid` resource model is a strong reference for JS-facing handles.

AUV should expose resource handles for sessions, devices, observations,
candidates, and artifacts instead of passing raw strings or paths through a JS
binding. A JS REPL should operate on handles with explicit close/drop semantics.

### Bollard and Docker API clients

Source:

- https://github.com/fussybeaver/bollard

Useful pattern:

- Transport setup is separate from resource modules.
- OS transport features are feature-gated.
- Generated protocol types are kept recognizable as protocol types.
- API version negotiation is a first-class concept.

AUV should use this for driver/provider negotiation: driver capabilities,
protocol versions, adapter versions, target app versions, and runtime versions
should be negotiated explicitly.

### thirtyfour and fantoccini

Source:

- https://docs.rs/thirtyfour/latest/thirtyfour/
- https://github.com/stevepryde/thirtyfour
- https://github.com/jonhoo/fantoccini

Useful pattern:

- A session handle owns the protocol session and shared client.
- Element handles are ergonomic references over protocol IDs.
- Query/wait APIs are separate from raw commands.
- Optional CDP and BiDi support are feature-gated.

AUV should provide `AuvSession`, `DeviceHandle`, `WindowHandle`,
`ObservationHandle`, `NodeHandle`, `CandidateHandle`, and `ArtifactRef` as
first-class concepts. Raw WebDriver-compatible endpoints can be implemented on
top of those handles, but should not be the internal model.

### Playwright

Source:

- https://playwright.dev/docs/languages
- https://github.com/microsoft/playwright/tree/main/packages/playwright-core/src

Useful pattern:

- Client API, protocol, remote transport, server dispatch, tracing, and recorder
  are distinct layers.
- Language bindings share one core implementation.

AUV should aim for one runtime with multiple frontends: CLI, MCP, JS/REPL,
WebDriver-compatible server, and future UI. The frontends should not fork
execution semantics.

### Appium

Source:

- https://appium.io/docs/en/latest/intro/drivers/
- https://appium.io/docs/en/latest/ecosystem/drivers/

Useful pattern:

- Appium drivers answer "how do we automate unrelated platforms?"
- Core server dispatches WebDriver commands to drivers.
- Drivers may proxy to lower-level automation stacks such as XCUITest,
  UiAutomator2, Chromium, Mac2, or Windows.
- Plugins extend behavior without becoming the driver itself.

AUV should borrow the driver/provider split. AUV drivers should expose
capabilities and operations. AUV's scheduler/runtime should decide which driver
or provider to use for a step. A WebDriver-compatible server can map W3C command
shapes onto AUV sessions, virtual elements, observations, and actions.

### Cargo

Source:

- https://doc.rust-lang.org/nightly/nightly-rustc/cargo/index.html
- https://github.com/rust-lang/cargo

Useful pattern:

- Cargo keeps command frontends thin and pushes reusable behavior into `ops`.
- `core`, resolver, sources, config, target layout, and lockfile handling are
  distinct domains.
- CLI command shape does not own the domain model.

AUV should copy the `ops` idea: `app probe`, `skill run`, `scan window-region`,
and future WebDriver/MCP/JS calls should lower into reusable runtime operations
instead of each frontend owning workflow logic.

### Tower, hyper, tracing, and OpenTelemetry

Sources:

- https://tower-rs.github.io/tower/tower/index.html
- https://hyper.rs/
- https://tokio-rs.github.io/tracing/tracing/
- https://github.com/open-telemetry/opentelemetry-rust

Useful pattern:

- Tower's `Service<Request> -> Response/Error` and `Layer` model is a good
  conceptual reference for routing, timeout, retry, load shedding, and testing.
- AUV does not need to depend on Tower immediately, but driver/provider dispatch
  should be designed so timeout, retry, backpressure, cancellation, and
  instrumentation are explicit middleware-like concerns.
- AUV run traces are product evidence. Operational telemetry is different:
  library crates should emit `tracing` spans/events, while binaries and servers
  configure subscribers/exporters. OTLP export should be optional plumbing, not
  the internal run model.

### MaaFramework and Cua

Sources:

- https://github.com/MaaXYZ/MaaFramework
- https://github.com/trycua/cua
- `docs/ai/references/2026-05-24-maa-recognition-pipeline-research.md`
- `docs/ai/references/2026-05-25-projects-research-and-repl-api.md`

Useful pattern:

- MaaFramework's recognition pipeline validates AUV's current
  `all/filtered/best` `RecognitionResult` shape. It also reinforces
  `roi -> box -> action target` as a useful visual automation model.
- Maa's static successor graph is not a direct fit; AUV should keep recipes,
  hooks, and runtime scheduling separate from detector outputs.
- Cua is useful for local computer-use driver shape: RPC-native driver process,
  tool registry, daemon mode, and a visual cursor overlay that does not move the
  hardware cursor.

AUV should keep Maa-like visual recognition as a provider family and Cua-like
native RPC/driver handles as an integration pattern, while preserving AUV's
stronger run recording, artifact, replay, and inspection model.

## Current AUV Shape

Current repository facts:

- `src/lib.rs` builds a default runtime from `LocalStore`,
  `default_command_catalog()`, and `default_driver_registry()`.
- `src/lib.rs` currently exposes `app`, `catalog`, `contract`, `driver`,
  `inspect`, `inspect_server`, `model`, `recording`, `run_recording`,
  `runtime`, `scroll_scan`, `skill`, `store`, `trace`, and `xtask` as
  first-class modules.
- `src/runtime.rs` owns command invocation, implicit run creation, span/event
  recording, driver dispatch, artifact staging, and local snapshot persistence.
- `src/catalog.rs` is a static command registry. It mixes command naming,
  disturbance policy, driver IDs, operation strings, and macOS command
  inventory. It also contains current scan/recognition/product commands such
  as `debug.observeWindowRegion`, `debug.findIconMatch`,
  `debug.scrollWindowRegion`, `music.search.results`, and
  `music.result.play`.
- `src/contract.rs` already defines important artifact-level contracts:
  `OperationResult`, `CandidateRef`, `RecognitionResult`, `SurfaceSelector`,
  `VerificationResult`, and `FailureLayer`. The gap is not "no contracts"; the
  gap is that command invocation still enters drivers through
  `BTreeMap<String, String>`.
- `src/skill.rs` mixes recipe manifest models, taxonomy parsing, catalog
  discovery, validation, execution, case matrices, reports, live-app locking,
  templating, and step variable export.
- `src/app.rs` owns the current `probe -> analyze -> distill -> validate`
  workflow, app identity, candidate shapes, live candidate validation, and
  `.auv/app-probes/...` outputs.
- `src/scroll_scan.rs` is an implemented orchestration module with stop policy,
  completeness claims, scan artifacts, hook decisions, observation parsing, and
  the `scan window-region` CLI surface.
- `src/run_recording.rs`, `src/recording.rs`, `src/store.rs`, and
  `src/inspect_server.rs` already implement a real recording/inspection layer:
  `RunRecordingBackend`, `RunRecorder`, local run snapshots, inspect-server
  session discovery, optional HTTP writes, and viewer-facing DTOs.
- The former bundle module was already partially modular with `model`,
  `catalog`, `validate`, `render`, `paths`, and `export`; it was retired on
  2026-06-11.
- `src/driver/macos/` has a useful capability-oriented shape in places:
  `ax_tree`, `capture`, `control`, `descriptor`, `dispatch`, `observe`,
  `overlay`, `native`, `support`, and `types`.
- The macOS Swift package is real. Generated `swift-bridge` files are ignored;
  `scripts/generate-swift-bridge` exists for SourceKit/IDE indexing before running
  SwiftPM-side checks.
- `default_driver_registry()` registers both the fixture driver and the macOS
  desktop driver today.
- The former `native.app.skill-tree.v0` bundle was retired on 2026-06-11.
  Recipes include validated QQ Music paths, experimental Notes/TextEdit/demo
  paths, experimental `music.result.play`, a needs-revalidation NetEase path,
  and a prototype scan hook recipe.
- Row-like producers and icon matching now emit `RecognitionResult` artifacts.
  `scroll_scan` prefers those artifacts over legacy row JSON. ONNX/neural
  detection was reverted and should not be treated as current mainline.

Current execution chain:

1. CLI parses commands in `src/cli.rs`.
2. `src/main.rs` dispatches CLI commands and discovers skills and case
   matrices.
3. `Runtime::invoke` creates a command run.
4. `Runtime::invoke_in_span` resolves `CommandSpec`, resolves the driver,
   builds `DriverCall`, invokes the driver, stages artifacts, records events,
   and finishes spans.
5. Recipe execution validates a manifest, acquires live-app locks when needed,
   then invokes runtime commands for each step.
6. Case matrices wrap recipe execution in validation spans.

This chain is good enough for a prototype. It is not a durable API boundary.

Current grounding chain:

```text
capture or provider output
  -> RecognitionResult
  -> Candidate / CandidateRef / SurfaceSelector
  -> action consuming CandidateRef
  -> VerificationResult
  -> inspectable run artifacts
```

The current v0 surface selector contract supports AX, OCR, and row selectors.
DOM/CDP, visual detector selectors, and command/menu selectors are reserved
future directions, not current implemented backends.

## Main Code Smells

### Runtime Does Too Much

`Runtime` is currently an executor, recorder coordinator, artifact stager, run
storage writer, command resolver, and text event renderer. These should be
separate concerns:

- runtime invocation
- recording lifecycle
- artifact persistence
- command resolution
- driver dispatch
- frontend rendering

The runtime should orchestrate these services, not contain all policy.

### Command Catalog Is Too Platform-Specific

`src/catalog.rs` always exposes macOS commands and `src/driver/mod.rs` always
registers `MacOsDesktopDriver`. Unsupported platforms fail later through
`require_macos()`.

Target state:

- driver crates register their own command slices
- macOS command slices are behind `cfg(target_os = "macos")` and a Cargo
  feature such as `driver-macos`
- generic commands are separate from platform-native commands
- debug commands are clearly separated from product commands

### Driver Trait Is Too Shallow

The current `Driver` trait is effectively:

```rust
fn descriptor(&self) -> DriverDescriptor;
fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse>;
```

This forces most type information into string maps and operation strings.

Target state:

- drivers declare capabilities
- operations have typed request and response schemas
- unsupported operations fail before runtime invocation where possible
- raw provider payloads remain available as evidence, not as the only API
- ambiguity, stale references, permission failures, and unsupported capability
  failures are distinguishable

### Too Much `String`

Current command inputs use `BTreeMap<String, String>`. Recipe variables,
signals, scan attributes, and many error paths also collapse structured data to
strings.

This causes several problems:

- callers cannot discover schemas
- numbers and booleans are reparsed repeatedly
- JSON values are sometimes stringified and sometimes rejected
- errors become impossible to classify without text matching
- handles and refs lose type meaning

Target state:

- use typed request structs internally
- use `serde_json::Value` only at explicit wire/plugin boundaries
- use newtypes for IDs and handles
- use enums for operation kind, scope, disturbance, provider, and failure layer
- expose structured `OperationResult`, `RecognitionResult`, and
  `VerificationResult` consistently

### Recipe Module Is A Mini Application

`src/skill.rs` should be split even before crate extraction:

- `skill/manifest.rs`
- `skill/taxonomy.rs`
- `skill/catalog.rs`
- `skill/validate.rs`
- `skill/execute.rs`
- `skill/cases.rs`
- `skill/template.rs`
- `skill/report.rs`
- `skill/lock.rs`

After that, move the stable pieces into `auv-recipe`.

### Template Behavior Is Too Loose

Current template replacement silently leaves unknown placeholders. That is
convenient while experimenting but weak for reusable recipes.

Target state:

- recipes should declare input schemas
- template rendering should return a typed missing-variable error by default
- permissive rendering should be opt-in for exploratory/debug commands
- step outputs should export typed values, refs, and artifact handles, not only
  strings and file paths

### Handle And Ref Ergonomics Are Incomplete

Trace IDs, run IDs, span IDs, event IDs, and artifact IDs are already typed in
`src/trace.rs`. The remaining string gaps are mainly runtime-facing and
driver-facing surfaces: `InvokeResult`, `DriverRunContext`, command inputs,
app probe records, scan records, and step exports.

Target state:

- `RunId`, `SpanId`, `ArtifactId`, `SessionId`, `DeviceId`, `WindowId`,
  `ObservationId`, `NodeId`, and `CandidateId` should be newtypes.
- `RunRef`, `ArtifactRef`, `CandidateRef`, `ObservationRef`, and `NodeRef`
  should be explicit structs.
- refs should carry enough scope to prevent stale or cross-session mistakes.
- long-lived handles should be managed by a `Session` or `ResourceTable`.

Handle/ref rule:

- Durable refs point to persisted or reconstructable facts, such as `RunRef`
  and `ArtifactRef`.
- Snapshot refs point into one observation snapshot, such as `ObservationRef`,
  `NodeRef`, and `CandidateRef`.
- Live handles own process/session resources, such as `DeviceHandle`,
  `SessionHandle`, and `WindowHandle`.
- Durable refs can cross process and frontend boundaries. Snapshot refs must
  carry observation/session scope. Live handles should serialize only as
  resource IDs inside one protocol session.
- Cross-session use should fail with a typed error. Stale snapshot refs should
  fail with a typed stale-ref error that includes the re-observation path.
- Public Rust APIs should not rely on lifetime-heavy handle graphs. Use owned
  IDs, cheap clone handles, and explicit close/drop behavior.

### Error Handling Is Not Library-Ready

`AuvResult<T> = Result<T, String>` is acceptable for early CLI prototypes but
not for library, MCP, JS, or WebDriver-compatible callers.

Target state:

- `auv-core::Error` is a non-exhaustive top-level error.
- `RuntimeError`, `DriverError`, `RecipeError`, `StoreError`, and
  `ProtocolError` are distinguishable.
- driver errors include category, provider, operation, retryability,
  permission hint, stale-ref hint, and evidence refs.
- CLI can render human text from typed errors.
- machine frontends receive structured errors.

Recommended crates:

- `thiserror` for library and driver error enums
- `anyhow` only at CLI, examples, and top-level app boundaries

### Serialization Is Too Close To Core Types

`docs/ai/references/2026-05-12-auv-setup.md` says schema fields should be
camelCase, but some persisted trace structs serialize snake_case while inspect
server API wrappers use camelCase.

Target state:

- core structs can be Rust-shaped.
- persisted artifact schemas and external API DTOs must have explicit
  `#[serde(rename_all = "camelCase")]` or versioned custom conversions.
- any type that crosses a stable boundary should have an `apiVersion` story.
- internal types should not derive `Serialize` merely for convenience.

## Proposed Crate And Layer Split

Start with module splits inside the current crate. Move to workspace crates once
module boundaries compile cleanly and tests exist at each boundary.

### `auv-core`

Purpose:

- shared domain vocabulary
- typed IDs and refs
- error categories
- operation schemas
- capability declarations
- disturbance model
- feature/version compatibility types

Scope:

- no filesystem
- no OS calls
- no CLI
- no driver implementation
- no run store

Key types:

- `AuvError`
- `AuvResult<T>`
- `SessionId`, `DeviceId`, `RunId`, `SpanId`, `ArtifactId`
- `DriverId`, `OperationId`, `CapabilityId`
- `OperationRequest<T>`
- `OperationResponse<T>`
- `CapabilityDescriptor`
- `DisturbanceClass`
- `ObservationScope`
- `RatioRegion`

### `auv-trace`

Purpose:

- run/span/event/artifact model
- recording backend traits
- replay-facing trace contracts

Scope:

- versioned trace schemas
- artifact metadata
- event taxonomy
- span lifecycle model
- record/update DTOs for inspect server

It should not know about macOS, recipes, or CLI commands.

### `auv-store`

Purpose:

- local and future remote run/artifact storage
- artifact hashing
- retention and redaction policy
- retired bundle package IO only as historical read/import support if owner-approved

Scope:

- `.auv/runs`
- artifact staging
- content-addressed artifacts if needed
- `sha256` and media metadata

It should not execute drivers.

### `auv-runtime`

Purpose:

- session lifecycle
- driver registry
- capability negotiation
- command scheduling
- run recording orchestration
- stop/resume/replay orchestration

Scope:

- `AuvRuntime`
- `AuvSession`
- `RuntimeBuilder`
- `DriverRegistry`
- `CapabilityRouter`
- `RecordingPolicy`

Runtime should depend on abstractions, not concrete macOS driver modules.

### `auv-protocol`

Purpose:

- native AUV execution protocol
- transport-neutral request/response envelopes
- method names and DTOs for sessions, devices, runs, observations, actions,
  artifacts, and errors
- client/server traits used by MCP, JS, WebDriver adapter internals, and future
  UI servers

Scope:

- stable-ish protocol DTOs
- structured protocol errors
- capability negotiation payloads
- method registry
- transport adapters later: stdio JSON-RPC, HTTP, Unix socket, in-process

It should not shell out to the CLI and should not duplicate inspect-server DTOs.
`auv-inspect` remains a run-data access layer; `auv-protocol` is the execution
API.

### `auv-recipe`

Purpose:

- recipe manifests
- typed recipe inputs
- taxonomy
- validation
- templating
- case matrices
- recipe execution plan lowering

Scope:

- recipe schema
- case matrix schema
- validation diagnostics
- default input resolution
- step dependency graph

Recipe execution can be in `auv-runtime` or an `auv-recipe-runtime` adapter, but
manifest parsing and validation should not depend on live drivers.

### Retired `auv-bundle` candidate

Purpose:

This was a proposed crate boundary for bundle manifests, verification,
export/import/package rendering, and coverage reports. The active bundle
surface was retired on 2026-06-11, so this crate should not be introduced unless
the owner approves a purely archival reader.

Scope:

- package structure
- compatibility metadata
- member coverage
- static verification

Live target-app probing should be a runtime validation capability, not a static
manifest verifier side effect.

### `auv-inspect`

Purpose:

- view model for runs, spans, events, artifacts
- inspect server API DTOs
- live update subscriptions

Scope:

- HTTP/WebSocket server
- query APIs
- viewer-facing DTOs

It should not own execution semantics.

### `auv-driver-api`

Purpose:

- traits and shared helper types for drivers
- provider lifecycle
- capability declarations
- typed operation dispatch

Possible traits:

```rust
pub trait DriverProvider {
  fn descriptor(&self) -> DriverDescriptor;
  fn capabilities(&self) -> &[CapabilityDescriptor];
  fn open_session(&self, request: OpenSessionRequest)
    -> Result<Box<dyn DriverSession>, DriverError>;
}

pub trait DriverSession {
  fn session_id(&self) -> SessionId;
  fn invoke(&self, request: DriverOperationRequest)
    -> Result<DriverOperationResponse, DriverError>;
  fn close(&self) -> Result<(), DriverError>;
}
```

The exact trait shape is provisional. The important boundary is provider
lifecycle and session-scoped invocation.

Driver/provider APIs also need readiness and cancellation semantics:

- readiness or backpressure before accepting an operation
- per-operation timeout policy
- retry classification
- cancellation token or request id
- idempotent close
- clear distinction between driver-unavailable, capability-unsupported,
  permission-denied, stale-ref, ambiguous-target, and provider-failed errors

### `auv-driver-macos`

Purpose:

- macOS desktop driver implementation
- native Swift interop
- screen/window/capture/AX/OCR/pointer/keyboard/clipboard/permission modules

Scope:

- `screen`
- `display`
- `window`
- `capture`
- `ax_tree`
- `ocr`
- `pointer`
- `keyboard`
- `clipboard`
- `permission`
- `overlay`
- private `native` and `ffi`

Rules:

- expose capability-oriented modules
- keep Swift bridge details behind `native` or `ffi`
- target-gate the whole crate/module with `cfg(target_os = "macos")`
- do not let dependency names become public namespaces

### `auv-driver-fixture`

Purpose:

- deterministic test driver
- golden trace and recipe tests
- replay and inspection fixtures

This should be available on all platforms and used heavily in CI.

### `auv-webdriver`

Purpose:

- WebDriver-compatible server and client-facing protocol mapping

Scope:

- W3C session endpoints
- element/node handle mapping
- command translation
- capability negotiation
- optional Appium-like extensions

It should be an adapter over `auv-runtime`, not a replacement runtime.

### `auv-js`

Purpose:

- JS/REPL binding
- resource table
- typed handles exposed to JavaScript

Scope:

- `Auv.connect()`
- `session.observe()`
- `session.find()`
- `node.click()`
- `artifact.save()`
- explicit close/dispose behavior

It should not use raw file paths or raw run IDs as the primary user API.

### `auv-cli`

Purpose:

- human command frontend
- debug utilities
- integration with local project directories

Scope:

- parse args
- render text
- optionally render machine JSON
- call runtime APIs

CLI should not own core execution logic.

### Dependency Direction And Extraction Guardrails

Target dependency direction:

```text
auv-core
  <- auv-trace
  <- auv-store
  <- auv-driver-api
  <- auv-recipe
  <- auv-runtime
       <- auv-protocol
       <- auv-inspect
       <- auv-driver-fixture
       <- auv-driver-macos
       <- auv-webdriver
       <- auv-js
       <- auv-cli
```

This is illustrative, not a Cargo.toml command. The real rule is simpler:
stable vocabulary flows downward, concrete providers and frontends depend
upward, and domain crates do not depend on CLI or platform-native crates.

Guardrails:

- Do not extract a crate until the module boundary has tests and a clear public
  surface.
- Prefer `pub(crate)` while names are still provisional.
- Use a facade crate only for intentionally re-exported user APIs.
- Keep generated, FFI, and protocol DTO crates separate from domain crates.
- Feature ownership belongs to the crate that owns the optional dependency.
- Avoid public dependency leaks unless they are part of the intended API.

## OS Feature Flag Rules

Use three levels of gating:

1. Target gating with `cfg(target_os = "...")` for code that cannot compile or
   link elsewhere.
2. Cargo features for optional providers such as `driver-macos`,
   `driver-fixture`, `webdriver-server`, `js-binding`, `inspect-server`,
   `ocr-vision`, `ocr-external`, `yolo`, `opencv`, `swift-native`.
3. Runtime capability negotiation for features that compile but may not be
   available on a given machine because of permissions, installed tools, target
   app version, or sidecar availability.

Examples:

- `auv-driver-macos/native/*` should be `target_os = "macos"`.
- `swift-bridge` should be private to macOS native implementation.
- WebDriver compatibility should be a portable adapter crate.
- OCR/YOLO/OpenCV should be provider capabilities, not hard dependencies of the
  core runtime.
- fixture drivers should always compile.

## Domain Layer Rules

Keep these domains together:

- Trace recording: run/span/event/artifact schemas and recorder traits.
- Store: filesystem/object-store layout, artifact staging, hashing, retention.
- Recipe: manifest, taxonomy, validation, templating, cases.
- Runtime: session, scheduling, capability routing, run orchestration.
- Driver API: provider/session traits and operation metadata.
- Platform driver: OS-specific observation/control implementation.
- Inspect: viewer-facing query/update DTOs.
- Frontends: CLI, MCP, WebDriver, JS, future UI.

Do not put these together:

- CLI argument parsing and recipe execution.
- macOS command inventory and generic command catalog.
- Swift FFI types and AUV public domain types.
- trace persistence and human text rendering.
- recipe validation and live app probing.
- WebDriver wire protocol and core virtual AX tree model.

## Device, Session, Run, And Resource Ownership

AUV needs first-class device and session concepts before WebDriver, JS, MCP, or
native RPC can feel coherent.

Definitions:

- `Device`: an automation target environment, such as local macOS desktop, a
  remote VM, an Android device, an iOS device, a browser context, or a fixture
  environment.
- `DeviceProfile`: static or discovered metadata about a device: platform,
  provider family, permissions, displays, input channels, supported capture
  surfaces, and safety constraints.
- `DeviceConnection`: the live connection to a device/provider. It owns process,
  socket, permission, and shutdown state.
- `Session`: a caller-visible automation context over one device and one
  default target. It owns capability snapshots, target defaults, observation
  cache, cursor identity, active recording policy, live handles, and timeout
  policy.
- `Run`: an inspectable execution record. A session can create many runs; a run
  is not the same thing as the session.
- `ResourceTable`: the protocol-side table that maps opaque resource IDs to
  live handles and closes them when a session ends.

Local desktop mutating actions should be guarded by a device-level action lock.
Observation-only operations may run more freely, but they must still respect
provider limits and snapshot consistency. This prevents two sessions from
typing, clicking, scrolling, or changing clipboard state on the same desktop at
the same time.

Sessions should make target defaults ergonomic without hiding them:

```text
DeviceConnection(local-macos)
  -> AuvSession(target=com.apple.Music, capabilities=[screen.capture, ocr.text])
       -> Run(search-and-play)
       -> Observation(snapshot_42)
       -> NodeHandle(snapshot_42/node_7)
       -> ArtifactRef(run_..., artifact_0003)
```

## Scheduler And Capability Routing

The scheduler sits above drivers and below recipes/frontends.

It owns:

- provider selection
- observe/recheck/action/verify sequencing
- capability routing
- wait, retry, timeout, and stability policy
- disturbance budget enforcement
- device/session action locks
- fallback policy
- cancellation and stop semantics
- run/span boundaries for multi-step operations

Drivers should expose primitive session-scoped operations and capability
metadata. They should not secretly own high-level orchestration policy. For
example, a driver can provide `captureWindow`, `ocrText`, `axPress`, or
`pointerClick`; the scheduler decides when to re-observe, whether AX is
preferred over pointer, which fallback is allowed by disturbance policy, and
how verification is recorded.

Provider choice should be explicit and inspectable:

```text
requested: ui.click(node)
planner:
  1. revalidate node liveness
  2. prefer native_ax.action if available and node supports press
  3. fallback to pointer.click if recipe/session allows pointer disturbance
  4. verify through declared verification provider
record:
  selected provider, rejected providers, fallback reason, evidence refs
```

## Observation Pipeline: Providers To Virtual Tree

The missing middle layer is the observation pipeline.

Target shape:

```text
capture/snapshot
  -> provider outputs
  -> RecognitionResult
  -> CandidateQuery / SurfaceSelector
  -> ObservationGraph
  -> VirtualAxTree
```

Provider outputs may come from:

- native AX tree
- OCR text boxes
- OCR rows
- visual row bands
- rule-based region segmentation
- icon/template matching
- YOLO or external detectors
- DOM/CDP snapshots
- app-specific APIs
- recipe hints and previously recorded geometry

The current mainline already has the important start of this shape:
`RecognitionResult` with `all`, `filtered`, `best`, `detail`, `evidence`, and
scope; `SurfaceSelector` v0 with AX/OCR/row clauses; and scroll scan consuming
recognition artifacts before legacy row JSON.

Rules:

- Preserve provider detail and rejected candidates.
- Preserve capture contracts and coordinate space.
- Preserve source artifact refs.
- Keep provider score inside provider detail or `RecognizedItem`, not as a
  universal top-level semantic truth.
- Do not add DOM/CDP/YOLO as current product backends before row/AX/OCR
  contracts survive consumer validation.
- Do not treat visual segmentation as proof of semantic state.

## Virtual AX Tree Direction

The current product direction should be:

1. Observe UI using the strongest available signals.
2. Normalize those signals into a structured observation graph.
3. Expose that graph as a virtual accessibility tree when native AX is missing
   or insufficient.
4. Map WebDriver/Appium-like commands onto virtual nodes and AUV sessions.
5. Preserve raw OCR/CV/AX/provider evidence for inspection and replay.

The virtual AX tree should not pretend OCR text boxes are native AX nodes. It
should explicitly record source and confidence.

Virtual nodes are snapshot nodes, not durable native elements. A node handle is
valid only for the observation/session/snapshot epoch that produced it. Acting
on a node must revalidate liveness before control, especially after scroll,
window movement, app focus changes, or timeouts.

Suggested model:

```rust
pub struct VirtualAxTree {
  pub tree_id: ObservationId,
  pub observation_id: ObservationId,
  pub snapshot_epoch: u64,
  pub scope: ObservationScope,
  pub root: VirtualNodeId,
  pub nodes: Vec<VirtualAxNode>,
  pub evidence: Vec<ArtifactRef>,
}

pub struct VirtualAxNode {
  pub id: VirtualNodeId,
  pub role: VirtualRole,
  pub name: Option<String>,
  pub value: Option<String>,
  pub bounds: Option<Rect>,
  pub coordinate_space: CoordinateSpace,
  pub state: NodeState,
  pub sources: Vec<NodeSource>,
  pub actions: Vec<VirtualAction>,
}

pub enum NodeSource {
  NativeAx { native_path: String },
  OcrText { confidence: f64, artifact: ArtifactRef },
  VisualRegion { confidence: f64, artifact: ArtifactRef },
  Detector { provider: String, confidence: f64, artifact: ArtifactRef },
  RecipeHint { recipe_id: String },
}
```

Virtual actions should be declared, not inferred at the final click call:

```rust
pub enum VirtualActionKind {
  NativeAxAction,
  ProviderCommand,
  Shortcut,
  MenuCommand,
  PointerClick,
  KeyboardInput,
  ClipboardPaste,
  CustomProvider,
}

pub struct VirtualAction {
  pub kind: VirtualActionKind,
  pub disturbance: DisturbanceClass,
  pub requires_frontmost: bool,
  pub requires_focus: bool,
  pub liveness: Vec<LivenessPrecondition>,
  pub fallback_policy: FallbackPolicy,
  pub verification: VerificationPlan,
}
```

This keeps control policy visible. A pointer click fallback should not be hidden
inside a node's `click` method unless the action metadata says that fallback is
allowed.

The node graph can be consumed by:

- recipe execution
- inspect viewer
- WebDriver-compatible element lookup
- JS/REPL APIs
- replay and diff tools

## WebDriver And Appium Compatibility

Compatibility should be an adapter layer, not the internal architecture.

Mapping:

- WebDriver session -> `AuvSession`
- capabilities -> AUV requested drivers/providers/capabilities
- window handle -> AUV `WindowHandle` or `SurfaceHandle`
- element id -> `VirtualNodeHandle`
- `findElement` -> query over the virtual observation graph
- `click` -> AUV control strategy selected from node action metadata
- `getText` -> node accessible name/value/text projection
- screenshot -> capture artifact
- Appium context/device extensions -> `DeviceHandle` and provider metadata

AUV extensions should be explicit:

- `/session/{id}/auv/observe`
- `/session/{id}/auv/artifacts`
- `/session/{id}/auv/runs/{runId}`
- `/session/{id}/auv/virtual-tree`
- `/session/{id}/auv/candidates`

This lets existing infrastructure reuse WebDriver-style sessions and clients
while AUV keeps richer concepts such as run recording, artifacts, observations,
and provider evidence.

Conformance boundary:

- W3C capability handshake should map into AUV capability negotiation.
- Standard WebDriver element IDs should map to `VirtualNodeHandle` values.
- stale element errors should be returned when the source observation is stale
  and revalidation cannot recover.
- timeout and wait semantics should be explicit; AUV must not hide arbitrary
  sleeps in drivers.
- screenshot endpoints should return capture artifacts and optionally standard
  base64 image payloads.
- actions API support should start with a documented subset.
- window/context mapping should distinguish desktop window, browser page, app
  context, and device context.
- standard WebDriver errors should be used where they fit; AUV-specific
  structured errors should be available through extension endpoints.
- CSS and XPath should only be supported for DOM-capable providers. AX/OCR/row
  and visual selectors should use AUV extension locator strategies.
- unsupported endpoints must fail explicitly rather than pretending to be a
  fully conformant browser driver.

## JS/REPL Binding Capability

AUV should support a JS binding, but it should look like a resource API, not a
stringly CLI wrapper.

Dream shape:

```javascript
const auv = await Auv.connect({ projectRoot: "." });
const session = await auv.openSession({
  target: { app: "com.apple.Music" },
  capabilities: ["screen.capture", "ocr.text", "input.pointer"]
});

const obs = await session.observe({ scope: "window" });
const tree = await obs.virtualTree();
const row = await tree.find({ role: "row", text: /song name/i });
await row.click();

const run = await session.lastRun();
console.log(await run.summary());
```

Binding rules:

- handles are resources
- resources can be closed
- raw JSON is available for debug, not primary ergonomics
- every operation can return evidence refs
- errors are structured and serializable
- inspection and replay are accessible from the same session model

## Rust API Style Rules

Use these rules for future refactors.

### Constructors And Builders

- Use `new` for infallible small constructors.
- Use `try_new` when validation can fail.
- Use builders for multi-field runtime/session/driver config.
- Builders should validate once and return typed errors.
- Avoid passing long `BTreeMap<String, String>` configs into library APIs.

### Errors

- use `thiserror` for crate-level errors
- use `anyhow` only in binaries, examples, xtask-like tools, and one-off scripts
- do not return `String` from public library APIs
- include source error with `#[source]`
- preserve platform/provider raw details in structured fields
- classify retryability and failure layer where relevant

### Traits

- define traits at the consumer boundary, not inside a concrete provider
- keep traits small but meaningful
- avoid a single object-safe trait that only takes opaque strings
- prefer typed request/response structs over method explosion when operations
  are numerous
- make capability discovery explicit
- seal traits that are not intended for external implementation
- use `#[non_exhaustive]` for public enums and errors that will grow

### Macros And Derives

Use derives intentionally:

- `Debug` on public structs unless sensitive
- `Clone` only when ownership semantics are genuinely cheap or expected
- `#[must_use]` on builders, handles, and results where ignoring the value is
  suspicious
- `Serialize`/`Deserialize` only for wire/persisted schemas or explicit DTOs
- `thiserror::Error` for public error types
- schema derive or codegen only in schema crates

Do not derive serialization on core internal types just to make tests easier.

Command macros may be useful later, but only after command schemas stabilize.
Avoid hiding provisional naming behind macros too early.

### Strings And Conversion

- parse user strings at frontends
- convert into typed values before runtime execution
- use `PathBuf` for local paths, not `String`
- use `Url` for URLs
- use typed IDs for refs
- use enums for constrained values
- use `Cow<'static, str>` only for static descriptors where it clearly helps
- avoid arbitrary `serde_json::Value` in core; confine it to boundary detail
  fields

### Validation

- parse -> validate -> execute should be separate phases
- validation returns diagnostics, not only one string
- static recipe/case validation must not require live app access
- live validation should be an explicit runtime operation
- compatibility checks should distinguish spec API, runtime, adapter, provider,
  and target app versions
- public APIs should document stability and feature gates
- optional public APIs should use `doc_cfg` once docs.rs-style builds matter
- declared MSRV should live in `rust-version` and be tested separately once
  public crates matter
- library crates should not secretly create or own a global Tokio runtime unless
  they are explicitly runtime-owning frontends
- long-running operations should document cancellation and drop behavior

### Testing

Test at boundaries:

- core type validation tests
- recipe parse/validate golden tests
- fixture driver runtime tests
- trace snapshot tests
- inspect API DTO tests
- macOS driver unit tests behind target gates
- live macOS smoke tests as explicit ignored or CI-matrix tests
- WebDriver compatibility tests against fixture sessions
- `insta` snapshots for serialized DTOs, traces, and artifacts, with redactions
  for nondeterministic IDs and timestamps
- `expect-test` for parser, diagnostics, and CLI text expectations

Avoid tests that depend on private API shape when a golden fixture can assert
behavior.

## CI And Engineering Checklist

Current documented validation commands are:

- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`
- `cargo run --quiet -- list-commands`
- `cargo run --quiet -- skill cases list`

Target CI should add:

- `cargo clippy --all-targets --all-features -- -D warnings`
- feature matrix with `default`, `driver-fixture`, `driver-macos` on macOS,
  `webdriver-server`, `inspect-server`, and minimal features
- `cargo hack` for minimal/default/all-features plus bounded
  `--each-feature` or feature-powerset checks, with skip rules for mutually
  exclusive platform/provider features
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features` for public
  crates
- `cargo deny` with checked-in advisories, licenses, bans/duplicates, and
  sources policy
- `cargo semver-checks` once public crates stabilize
- `cargo nextest run` with checked-in `.config/nextest.toml` profiles,
  timeouts, slow-test detection, retries only for known flaky live tests, JUnit
  output, and optional sharding/archive support
- spell/typo check for docs and schema names
- JSON schema validation for recipes and case matrices
- golden trace/artifact verification
- generated Swift bridge check on macOS when `ffi.rs` changes
- `swift build` in `src/driver/macos/native/swift` after generating bridge
  files on macOS
- committed `rust-toolchain.toml` with channel, components, and any required
  targets
- MSRV check once public crates matter
- `cargo hakari` only if workspace build duplication becomes material; if
  adopted, CI should verify the generated workspace-hack crate is current

Do not make live desktop tests mandatory on every platform. Keep fixture and
golden tests portable; run live provider tests in targeted jobs.

## Replay Semantics

Replay must become a real contract, not a synonym for "run again."

Modes:

- Artifact-only replay: reconstruct views, candidate lists, recognition
  results, and verification reports from stored artifacts without touching a
  live app.
- Fixture replay: run recipes or operations against fixture drivers using
  recorded artifacts and deterministic responses.
- Live replay: re-run operations against a real target after checking
  compatibility and liveness.

Rules:

- A replay creates a new run. It links back to the source run through explicit
  attributes and refs.
- Replayed candidates must re-observe liveness before control. Stale candidate
  failures are successful replay findings, not internal errors.
- Coordinate projection must go through capture contracts, never raw screenshot
  pixels alone.
- Side-effect policy must be explicit: inspect-only, dry-run, allowed pointer,
  allowed keyboard, allowed clipboard, allowed app foregrounding.
- Replayed artifacts should cite both source evidence and replay evidence where
  available.
- Live replay should fail early on incompatible runtime, adapter, provider, or
  target app versions unless the caller explicitly accepts revalidation.

## What Is Written But Not Really Written Yet

The repository and references discuss these concepts, but the implementation is
not complete enough to treat them as stable:

- replay semantics
- resume semantics
- structured stop/stopped result state
- retention/redaction policy
- artifact hashing
- adapter dependency negotiation
- target application version probing
- typed command input/output schemas
- machine-readable CLI JSON mode
- external adapter/sidecar protocol
- WebDriver-compatible API
- JS/REPL binding
- virtual AX tree over OCR/CV segmentation
- provider capability routing across OCR/YOLO/OpenCV/AX/CDP/etc.

These should be documented as provisional until implemented.

Partially implemented but not complete:

- typed trace IDs and artifact refs exist, but many runtime and driver-facing
  APIs still expose strings
- `RecognitionResult`, `CandidateRef`, `SurfaceSelector`, and
  `VerificationResult` exist, but the consumer chain still needs stronger
  provenance and verification tests
- run recording and inspect-server writes exist, but replay and conflict-rich
  multi-writer recovery are not complete product surfaces
- scroll scan exists, but completeness, boundary confidence, and section-aware
  semantics remain provisional
- app probe/analyze/distill/validate exists, but promotion and stable app
  version compatibility are still incomplete
- icon matching exists as evidence; it should not automatically click or claim
  semantic success without consumer validation
- YOLO/ONNX and DOM/CDP should remain future provider directions until the
  current row/AX/OCR contracts are proven end to end

## Dream AUV

Dream AUV is a layered automation runtime:

- It observes application surfaces through native AX, OCR, YOLO, segmentation,
  CDP, app APIs, or device providers.
- It normalizes observations into typed contracts and a virtual UI tree.
- It exposes session, device, window, node, candidate, run, and artifact handles.
- It runs recipes through a shared runtime, not through CLI scripts.
- It records every run implicitly with spans, events, artifacts, and evidence.
- It can replay or inspect previous runs.
- It can expose CLI, MCP, JS, WebDriver-compatible, and future UI frontends over
  the same execution model.
- It can use Appium-style drivers/providers without making WebDriver the whole
  internal model.
- It keeps platform-native interop narrow and feature-gated.
- It makes schema and compatibility explicit.

The most important product bet is the virtual AX tree:

- Native AX when available.
- OCR text boxes when AX is absent.
- YOLO/icon detections for visual-only controls.
- Region segmentation for lists and layout.
- Scroll scan to build collections over time.
- Evidence-preserving node sources so inspection remains honest.

This gives downstream systems a familiar programming target without pretending
the underlying UI is always DOM, native AX, or WebDriver-native.

## Design Risk Register

Over-design risks:

- Too many crates too early. Guardrail: split modules first, extract crates only
  after boundary tests and public API intent exist.
- Universal selector engine too early. Guardrail: keep v0 AX/OCR/row selector
  scope until candidate/action/verification consumption is proven.
- WebDriver conformance burden. Guardrail: document a supported subset and use
  AUV extension endpoints for non-browser concepts.
- Virtual AX pretending certainty. Guardrail: every virtual node records source,
  confidence/evidence, coordinate space, and staleness behavior.

Under-design risks:

- Device/session remains implicit. Guardrail: introduce resource ownership
  before JS/WebDriver/MCP grow separate context models.
- Scheduler hides inside drivers. Guardrail: drivers expose primitive
  operations; runtime records provider selection and fallback.
- Replay remains aspirational. Guardrail: define artifact-only, fixture, and
  live replay modes before adding more providers.
- Evidence chain breaks at action time. Guardrail: tests must prove
  `RecognitionResult -> CandidateRef -> VerificationResult` preserves source
  artifact and recognized item identity.
- Strings remain the hidden API. Guardrail: keep compatibility shims, but make
  typed requests and structured errors the internal path.

## Suggested Migration Order

1. Split `src/skill.rs` into modules inside the current crate.
2. Add compatibility shims so existing CLI commands, recipes, and string inputs
   continue working during typed API migration. Do not restore bundle execution
   compatibility.
3. Split command catalog into generic, fixture, and macOS command slices.
4. Introduce typed error enums while preserving CLI text rendering.
5. Introduce native protocol DTOs for device/session/run/operation/artifact
   envelopes, without changing frontends yet.
6. Introduce `Device`, `AuvSession`, `ResourceTable`, and typed refs for
   run/span/artifact/observation/node/candidate.
7. Replace `DriverCall.inputs: BTreeMap<String, String>` with typed operation
   request parsing at the runtime/driver boundary.
8. Prove the existing recognition consumer chain:
   `RecognitionResult -> CandidateRef -> VerificationResult`.
9. Add fixture-backed observation graph and virtual AX tree contracts.
10. Move trace/store/recipe/driver-api modules toward workspace crates.
11. Add WebDriver and JS adapters over the native runtime/protocol, not over
    CLI commands.
12. Move macOS implementation into a target-gated driver crate.
13. Add CI feature matrix, docs checks, feature checks, and golden artifact
    tests.

This order keeps behavior intact while progressively creating real boundaries.
