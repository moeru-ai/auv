# API client/server package architecture research

Date: 2026-08-02

Status: superseded for implementation direction by
[`2026-08-03-auv-facade-daemon-runner-architecture.md`](2026-08-03-auv-facade-daemon-runner-architecture.md).
This note remains primary-source research context; its capability-handle,
Runner lease, and package-ownership recommendations are not the accepted target.

Update (2026-08-03): the experimental core inference API and built-in
Ultralytics Runner described below were removed. Balatro now owns
`auv.game.balatro.v1.BalatroDetectionService` and the `auv.game.balatro`
Runner. The repository-state statements below are retained as historical input,
not current implementation guidance.

## Question

How should AUV preserve the existing daemon/core, driver, platform-specific
driver, and inference seams in its Rust client and server without making common
callers learn the complete gRPC package tree? In particular, how should future
inference tasks such as segmentation and multiple execution providers fit
without extending the current object-detection shape until it becomes a generic
inference container?

This note uses only project source and primary upstream sources. Sections headed
**Sourced facts** report what those sources do or require. Sections headed
**AUV inference** are design conclusions; they are not claims that an upstream
project requires AUV to make the same choice.

## Historical AUV evidence

### Sourced facts: repository state

- At the time of this research, the Protobuf tree declared separate, versioned packages:
  `auv.api.daemon.v1`, `auv.api.driver.v1`, `auv.api.driver.macos.v1`, and
  `auv.api.inference.v1`. `auv-api-proto` then preserved that hierarchy under
  `auv::api::{daemon,driver,inference}` and nested macOS below `driver`.
- Before the package-organization refactor on 2026-08-02, the handwritten
  `protocol::grpc::clients` module exported core, portable driver, macOS driver,
  and inference clients as siblings. The researched revision exposed explicit
  `daemon::v1`, `driver::v1`, `driver::macos::v1`, and `inference::v1`
  modules while retaining one shared gRPC transport client.
- Before the same refactor, `control_grpc.rs` implemented core, driver, macOS,
  and inference gRPC adapters in one file. The server adapters now use the
  matching package-shaped modules under `protocol::grpc`; listener
  registration and RPC behavior remain unchanged.
- `ObjectDetectorSpec` combined task configuration (confidence, IoU,
  labels and output limit), model loading (`model_path`), cache identity, and
  execution-provider selection (`InferenceDeviceKind`). The response can
  represent only rectangular detections. The schema is explicitly marked
  experimental/unstable.
- The daemon nevertheless has a useful internal seam: core admission chooses a
  Runner and `runner.rs` calls the appropriate generated Runner service. The
  public daemon listener aggregates those services without requiring the daemon
  to execute every capability itself.

### AUV inference

The Protobuf packages already express more architecture than the handwritten
client and server modules. Flattening is therefore not merely cosmetic: it
makes the composition root look like the owner of all capability semantics and
makes platform/inference dependencies appear universally available.

The former experimental `core` package was the daemon control contract in
practice: discovery, pairing, Device, Run, Runner, and RunnerClass are
daemon-owned resources. It was renamed to `auv.api.daemon.v1` before
stabilization so the wire package and Rust module name the same owner.

## Kubernetes: versioned groups below a clientset

### Sourced facts

- Kubernetes stores API types in group/version packages. For example,
  `apps/v1` imports shared core types from `api/core/v1` and common metadata
  from `apimachinery/pkg/apis/meta/v1`; `metav1` describes itself as shared
  types used across API groups rather than another application group.
  [apps/v1 source](https://github.com/kubernetes/api/blob/master/apps/v1/types.go),
  [metav1 source](https://github.com/kubernetes/apimachinery/blob/master/pkg/apis/meta/v1/types.go)
- Generated typed clients preserve group and version in their package paths.
  `kubernetes/typed/core/v1` defines a generated `CoreV1Interface` that exposes
  resource getters such as Pods, Nodes, and ConfigMaps.
  [CoreV1 generated client](https://github.com/kubernetes/client-go/blob/master/kubernetes/typed/core/v1/core_client.go)
- Kubernetes also provides one generated `Clientset` as a convenience
  aggregator. Its interface has explicit `CoreV1()`, `AppsV1()`,
  `AppsV1beta1()`, and other group-version accessors; the accessors return the
  corresponding typed group client rather than flattening all resources onto
  the clientset.
  [client-go Clientset](https://github.com/kubernetes/client-go/blob/master/kubernetes/clientset.go)

### AUV inference

Kubernetes supports the proposed analogy, but the important pattern is not
"make everything Kubernetes-shaped." It is two-level access:

1. keep a truthful group/version client below the seam; and
2. optionally aggregate those clients without erasing their group/version.

For AUV, `protocol::grpc::daemon::v1`, `driver::v1`,
`driver::macos::v1`, and `inference::v1` should therefore remain independently
navigable even if `Client` provides a convenient entry point. macOS should not
be a sibling of portable driver services, and inference should not be a sibling
of daemon resource clients.

One limit of the analogy matters: Kubernetes resources in a group share mature
CRUD/list/watch machinery. AUV driver and inference methods are capability
operations with different typed inputs and results. Copying Kubernetes's
resource getters mechanically would create shallow pass-through clients.

## Protobuf and gRPC: wire packages are real; generated stubs are not the product interface

### Sourced facts

- The Protocol Buffers language guide says packages prevent type-name clashes
  and map to namespaces/modules in most generated languages. The official style
  guide recommends short, unique, dot-delimited lower-snake-case package names.
  [Protobuf package guide](https://protobuf.dev/programming-guides/proto3/#packages),
  [Protobuf style guide](https://protobuf.dev/programming-guides/style/#packages)
- Google AIP-191, an official guide for Protobuf-defined Google APIs, requires
  each individual API to occupy one package ending in a version component and
  uses a matching directory. It also warns that filenames often become client
  library module names.
  [AIP-191](https://google.aip.dev/191)
- gRPC code generation works at service granularity. The official Go reference
  states that each Protobuf `service` produces a client interface and
  constructor, and that generated code follows Protobuf package mapping. The
  official Rust reference likewise describes one generated stub per service.
  [gRPC Go generated-code reference](https://grpc.io/docs/languages/go/generated-code/),
  [gRPC Rust generated-code reference](https://grpc.io/docs/languages/rust/generated-code/)
- Protobuf's compatibility guidance explicitly says clients and servers are not
  updated simultaneously and warns against assuming coordinated deployment.
  It also forbids reusing deleted field numbers and recommends reserving them.
  [Protobuf best practices](https://protobuf.dev/best-practices/dos-donts/)

### AUV inference

The generated unit is a service stub, not a mandate that all handwritten stubs
must be fields of one universal client. AUV can share one authenticated channel
while constructing group clients lazily from it.

Version should remain visible at the raw protocol seam. Hiding `v1` inside a
type alias is convenient today but makes future coexistence with `v2` harder.
Conversely, forcing every business caller to spell `protocol::grpc::...::v1`
leaks wire organization. This calls for two interfaces rather than one
compromise interface.

## KServe and Triton: generic inference protocol, explicit control/data split

### Sourced facts

- KServe documents its control plane as the owner of InferenceService lifecycle,
  reconciliation, deployment resources, networking, and model-server
  management. Its data plane is a separately specified inference interface.
  [KServe control plane](https://kserve.github.io/website/docs/concepts/architecture/control-plane),
  [KServe architecture](https://kserve.github.io/website/)
- KServe's Open Inference Protocol V2 standardizes server/model health,
  metadata, and `ModelInfer`. The inference payload is model-oriented and
  tensor-oriented: named inputs and outputs carry datatype and shape. The
  protocol defines an extension mechanism; it does not define object detection
  boxes or segmentation masks as the universal response.
  [KServe V2 protocol](https://kserve.github.io/website/docs/concepts/architecture/data-plane/v2-protocol)
- NVIDIA Triton implements KServe's inference protocols while also supplying
  extensions. Its documentation separates ordinary inference from restricted
  groups such as model repository, statistics, tracing, logging, and shared
  memory. It exposes HTTP, gRPC, and an in-process C interface over the same
  core server capability.
  [Triton inference protocols](https://docs.nvidia.com/deeplearning/triton-inference-server/user-guide/docs/customization_guide/inference_protocols.html)

### AUV inference

KServe demonstrates one viable generic *wire* seam: tensors in, tensors out,
provider-independent. It does not demonstrate that this is the best AUV caller
interface. AUV workflows benefit from task facts such as boxes, masks, source
coordinates, and provenance. Those task facts should remain typed above any
generic tensor adapter.

AUV should also keep daemon control operations separate from capability data
operations even when one listener aggregates both. Authentication, admission,
placement, lifecycle, and discovery are daemon concerns; detecting or
segmenting an image is an admitted Runner capability.

## ONNX Runtime and Transformers: providers and tasks are different axes

### Sourced facts

- ONNX Runtime uses the same inference-session interface across execution
  providers. Provider-specific libraries report supported nodes/subgraphs;
  callers can order providers by priority and set provider options. A CUDA to
  CPU fallback order is configuration of one session, not a different object
  detection result type.
  [ONNX Runtime execution providers](https://onnxruntime.ai/docs/execution-providers/)
- Hugging Face Transformers exposes a high-level `pipeline` wrapper plus
  task-specific pipelines. Its documented computer-vision tasks include both
  object detection and image segmentation as distinct pipelines, while model
  choice is supplied when constructing the pipeline.
  [Transformers pipeline documentation](https://github.com/huggingface/transformers/blob/main/docs/source/en/main_classes/pipelines.md)

### AUV inference

Provider and task are orthogonal axes:

- **Task** determines the caller's typed input, output, validation, and semantic
  error model: object detection returns labeled regions; segmentation returns
  masks/polygons; OCR returns text regions.
- **Provider** determines how a compatible model graph is executed: CPU, CUDA,
  Core ML, TensorRT, and so on.

`InferenceDeviceKind` currently puts provider/product names and hardware kinds
in one enum, while `ObjectDetectorSpec` exposes them inside a task request. That
will become harder to evolve because providers have provider-specific options
and availability, and because provider selection is often policy/fallback
rather than one fixed enum value.

The safer direction is separate task contracts plus a provider adapter seam.
Common callers should select a task and model. The Runner or an explicit
execution policy should select providers. Provider identity, version, fallback,
and timings should be returned as observation/trace facts, not encoded into the
semantic task result.

## Alternative 3: capability-scoped typed handles

This is deliberately more radical than a package-shaped `Clientset`. The raw
wire layer remains package/version-shaped, but the common caller never selects a
gRPC service client. It obtains typed handles that prove placement and
capability admission.

### Caller interface

```rust
let auv = Client::from_env_or_local().await?;
let run = auv.run(RunOptions::default()).await?;
let runner = run.runner(RunnerRequest::requiring([
  capability::OBJECT_DETECTION_V1,
])).await?;

let detector = runner
  .inference()
  .object_detection(ObjectDetector::model(ModelRef::named("ui-elements")))
  .await?;

let detections = detector.detect(frame).await?;
```

Segmentation is a separate task interface rather than a variant added to
`Detection`:

```rust
let segmenter = runner
  .inference()
  .segmentation(Segmenter::model(ModelRef::named("ui-segments")))
  .await?;

let masks = segmenter.segment(frame).await?;
```

Daemon administration remains intentionally explicit and versioned:

```rust
let runs = auv.daemon().core_v1().runs().list().await?;
```

Portable and platform-specific driver capability remain separate:

```rust
runner.driver().v1().input().click(request).await?;
runner.driver().macos().v1().accessibility().focus_text(request).await?;
```

The last two examples are escape hatches for callers that need exact wire-group
semantics. App-owned operations should normally consume narrower typed driver
or inference handles.

### Types and invariants

- `Client` owns connection context, authority, discovery, and placement policy;
  it does not expose every generated stub as state.
- `DaemonCoreV1` is a cheap handle over the authenticated transport. It owns
  daemon resource operations only.
- `Runner` contains a canonical lease and advertised capability set. Its fields
  are private, so a capability call cannot be formed without admission.
- `Capability<T>` is a sealed typed proof created from a `Runner`; `T` names a
  stable task or driver contract, not a provider implementation.
- `Detector` and `Segmenter` are loaded-task handles. Their construction is the
  place for model compatibility checks, normalized cache keys, and provider
  resolution. Repeated calls do not repeat model paths or provider details.
- `ModelRef` is an admitted model identity or configured model reference. A raw
  daemon-host path should not be the general remote client interface.
- `ExecutionPolicy` is optional placement policy such as `Auto`,
  `RequireAccelerator`, or an operator-defined profile reference. Concrete
  provider options remain adapter configuration unless a demonstrated caller
  needs portable control over them.
- Task results contain semantic facts only. A sibling `InferenceObservation`
  carries provider ID/version, fallback chain, model digest, timing, and cache
  facts for tracing and inspection.

### Error interface

Errors should preserve the layer at which an invariant failed:

```text
ConnectError
PlacementError
CapabilityError::NotAdvertised | LeaseExpired | AdmissionDenied
ModelError::Unknown | UnauthorizedSource | IncompatibleWithTask
InferenceError::InvalidInput | ProviderUnavailable | ResourceExhausted
TaskError::InvalidResult | UnsupportedOutput
TransportError
```

The raw group-version clients may continue returning `tonic::Status`. The
business handles should map it once into stable domain errors so every caller
does not interpret gRPC codes independently.

### Hidden implementation and internal seams

```text
Client
  -> placement/discovery
  -> authenticated Transport
  -> raw group-version adapters
       core/v1
       driver/v1
       driver/macos/v1
       inference/v1
  -> capability handle
  -> task adapter
       ObjectDetection -> provider adapter -> Runner transport
       Segmentation    -> provider adapter -> Runner transport
```

The external seam is the typed handle. Protocol versions, channel cloning,
metadata injection, request size settings, cache normalization, model loading,
provider fallback, and response validation are implementation details. Internal
tests may replace the raw group adapter or provider adapter; application tests
exercise the same typed handle interface as production callers.

### Crate and dependency shape

A maximum-isolation implementation could use companion adapter crates:

```text
auv-api-client                    business facade and typed handles
auv-api-client-grpc               channel/auth/metadata transport
auv-api-daemon-client-grpc        auv.api.daemon.v1 adapters
auv-api-driver-client-grpc        auv.api.driver.v1 adapters
auv-api-driver-macos-client-grpc  auv.api.driver.macos.v1 adapters
auv-api-inference-client-grpc     historical auv.api.inference.v1 adapters

auv-api-server                    daemon composition root
auv-api-daemon-server-grpc        daemon.v1 serving adapters
auv-api-driver-server-grpc        portable driver adapters
auv-api-driver-macos-server-grpc  macOS adapters
auv-api-inference-server-grpc     inference task adapters
```

Dependency direction would be facade/composition root -> group adapter ->
generated contract and domain owner. Group adapters must not depend on the
facade. Platform-specific crates may depend on portable driver contracts, but
portable driver crates must not depend on macOS. Inference task adapters depend
on inference/image contracts and a provider seam, not on daemon control
implementations.

This crate count is not a recommendation to split immediately. The same seams
can first exist as package-aligned modules inside the current two crates. A new
crate earns its keep only when it isolates a real dependency, build target,
feature, release cadence, platform, or ownership concern. Splitting identical
thin wrappers into crates would add manifests without adding depth.

### Tradeoffs

Benefits:

- Common callers learn `Client -> Run -> Runner -> capability`, not the gRPC
  service registry.
- Raw protocol users retain truthful group/version navigation.
- Segmentation, depth, OCR, and detection can evolve independently.
- Provider additions normally add an adapter and configuration, not variants to
  every task request/result.
- Server composition can omit macOS or inference adapter crates without making
  the portable driver contract conditional.
- Lease, model, and capability invariants are enforced once and become natural
  test seams.

Costs and risks:

- Typed handles add lifecycle semantics and domain error mapping beyond the
  generated stubs.
- A loaded-task handle needs an explicit decision about cache lifetime,
  invalidation, and whether model loading is observable as part of a Run.
- A `Capability<T>` design can become generic machinery with little leverage if
  only one task uses it; concrete `ObjectDetector`/`Segmenter` handles should be
  preferred until repetition is real.
- Splitting into many crates increases compile graph, release choreography, and
  feature combinations. Package-aligned modules should precede crate extraction.
- Provider-neutral policy can hide controls expert callers need. The design
  therefore needs an explicit expert configuration seam, but not provider
  fields copied into every task schema.

## Comparison and recommended decision sequence

| Concern | Flat current client/server | Package-shaped clientset | Capability-scoped handles |
| --- | --- | --- | --- |
| Wire group/version fidelity | Low | High | High internally/raw |
| Common caller ergonomics | Initially short, grows with every service | Moderate | High |
| Platform isolation | Low | High | High |
| New inference task | Adds another sibling service | Adds task under inference/vN | Adds task handle and wire adapter |
| New provider | Tends to expand task request enums | Still risks provider fields in task schemas | Adds provider adapter/policy |
| Generated-code transparency | High | High | Deliberately hidden for common callers |
| Implementation cost | Already paid | Low-to-medium refactor | Highest |

The evidence supports the following sequence, not an immediate large rewrite:

1. Historical proposal: treat `core`, `driver`, `driver::macos`, and
   `inference` as package seams. The accepted implementation instead keeps
   app-specific inference contracts with their owning app Runner.
2. Preserve one composition root and one shared authenticated transport; do not
   infer that every package needs a crate immediately.
3. Stop evolving `ObjectDetectorSpec` as provider configuration. Specify task,
   model identity/source authorization, provider policy, and observation as
   separate decisions before adding segmentation.
4. Design one concrete task handle through the public `Runner` hierarchy and
   compare its caller interface with a package-shaped raw client.
5. Extract group adapter crates only when a real dependency/platform/build or
   ownership need appears.

This sequence preserves the useful current Runner aggregation seam while
preventing the flat client and server composition files from becoming the
architecture themselves.
