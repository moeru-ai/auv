# OSWorld on Kubernetes with Vieval

This workspace lets Vieval discover, schedule, score, and report OSWorld tasks
without embedding the Python benchmark runner. Each case creates a fresh KVM
Pod over a read-only OSWorld base image, installs a pinned Linux AUV daemon,
pairs a same-commit macOS AUV client, runs typed AUV operations remotely,
executes the task's pinned OSWorld evaluator, retains the final AUV capture,
and deletes the task-owned Pod.

The first checked-in case is deliberately small and deterministic: it renames
one Desktop directory. It is not a general-purpose agent and makes no model
call. The `local:auv-pr258` model entry is only an opaque Vieval scheduling
identity; no inference executor is called. The official `exact_match`
evaluator remains the scoring authority.

## Evidence level

The supported live slice is:

```text
OSWorld V1 execute setup
  -> fresh qcow2 overlay in one Kubernetes Pod
  -> PR 258 Linux x86_64 daemon + same-commit macOS arm64 client
  -> typed input.keys / input.typeText operations
  -> AUV Run IDs and final capture artifact
  -> OSWorld vm_command_line result
  -> exact_match score in a Vieval report
```

This is single-task live evidence, not a benchmark-wide support claim. The
adapter rejects unrecognized setup and evaluator shapes during case discovery.
It currently supports only `execute` setup, `vm_command_line` results, and
`exact_match`. Proxy, fixed-IP, application-specific getters, compound
evaluators, and model-driven action selection are intentionally outside this
slice.

## Configure

Build AUV for Linux x86_64, then copy `.env.example` values into the shell:

```bash
export OSWORLD_AUV_LINUX_BINARY=/absolute/path/to/auv-linux-x86_64
export OSWORLD_AUV_MACOS_BINARY=/absolute/path/to/auv-macos-arm64
export OSWORLD_KUBECONFIG=/absolute/path/to/kubeconfig
```

The binary SHA-256 must equal the value pinned by every selected case. The
case also pins the OSWorld source revision, runtime image digest, base PVC,
namespace, and local setup port. Apply the checked-in Node Feature Discovery
rule once per cluster so the scheduler can distinguish nodes where KVM is
actually enabled from CPUs that merely advertise the VMX instruction:

```bash
kubectl apply -f evals/osworld/kubernetes/kvm-node-feature-rule.yaml
```

Create the shared TrueNAS-backed base-image claim once:

```bash
kubectl apply -f evals/osworld/kubernetes/base-image-pvc.yaml
```

Populate `System.qcow2` through a one-time operator-owned importer, then verify
its SHA-256 against the `auv.moeru.ai/system-qcow2-sha256` annotation before
running cases. The claim is `ReadWriteMany` so KVM nodes can consume the same
base image, but every episode mounts it read-only and writes only to its own
disposable qcow2 overlay. Do not let episode Pods download or modify the base.

KVM alone does not prove that the pinned OSWorld guest reaches its setup API.
After a node passes a complete live episode probe, add it to the scheduler's
validated OSWorld pool:

```bash
kubectl label node <validated-node> auv.moeru.ai/osworld-ready=true
```

Kubernetes then selects an amd64, KVM-enabled, OSWorld-validated node that
satisfies the PVC topology and available resources. The case never pins a
hostname; adding or removing nodes from the validated pool is cluster policy.
Vieval does not silently substitute a different binary or task.

From the AUV repository root, inspect all cases without touching Kubernetes:

```bash
pnpm -F @auv-js/eval-osworld cases:list
```

Run and persist a Vieval report:

```bash
pnpm -F @auv-js/eval-osworld eval:run
```

The runner emits structured Pretty-format logs through `@guiiai/logg` for every
`kubectl` process, host or guest AUV command, and Kubernetes resource lifecycle
action. Command events contain the exact executable, argv, copy-pasteable
command line, duration, and exit code. Non-Secret resource create events
contain the submitted manifest; delete events contain the resource identity
and outcome. Secret manifests, stdin, and environment values are intentionally
excluded so pairing tokens and other credentials do not enter logs. Guest apt
setup retries recognized apt/dpkg lock contention every two seconds for at
most two minutes; other failures return immediately.

The default case concurrency is one. After assigning a unique
`infrastructure.localSetupPort` and `localAuvPort` to every case and confirming
node/PVC capacity,
raise it explicitly:

```bash
pnpm -F @auv-js/eval-osworld eval:run -- --case-concurrency 2
```

Each case owns a distinct Pod name derived from task ID and local port. Cleanup
deletes only that exact task-owned Pod. Set `OSWORLD_KEEP_POD=true` only while
debugging a failed episode; the operator then owns manual cleanup.

## Add tasks and cases

`tasks/upstream/` contains byte-for-byte task JSON copied from a pinned OSWorld
revision. `tasks/cases/` contains the Vieval execution policy: provenance,
cluster pins, AUV binary digest, port assignment, and a minimal typed AUV
action sequence. `src/cases.ts` scans and validates both directories before
Vieval registers cases with `casesFromInputs`.

Checklist for another case:

1. Pin the upstream repository revision and copy the task JSON unchanged.
2. Audit every setup step; reject unsupported step types instead of translating
   them approximately.
3. Pin the runtime image by digest and name the shared read-only base PVC; let
   the KVM and OSWorld-readiness constraints drive node selection.
4. Assign a unique local setup port for the intended concurrency window.
5. Pin the exact Linux and macOS AUV binary SHA-256 values built from the same
   source commit.
6. Express GUI delivery only as registered AUV commands and argv arrays.
7. Keep activation separate from semantic verification; the OSWorld evaluator
   decides the task score.
8. Retain AUV Run IDs, the downloaded final artifact and digest, raw evaluator
   output, and the Vieval report.
9. Start with concurrency one, then increase only after checking KVM, memory,
   PVC, and node capacity.
10. Confirm cleanup after pass, failure, timeout, and interruption.

## Translating an OSWorld adapter

Translate benchmark orchestration, not the benchmark's Python internals:

- HTTP setup calls map directly to `fetch` with the same JSON or multipart
  payload. Preserve command argv, shell flags, placeholder substitution, and
  response status.
- Process execution maps to `tinyexec` argv arrays. Use stdin for manifests or
  secrets, never interpolate them into a shell command.
- Python task discovery maps to checked-in JSON scanning plus validation before
  Vieval registration.
- Python concurrency maps to Vieval `casesFromInputs` and
  `--case-concurrency`; the episode adapter must not create a second scheduler.
- Evaluators should be ported only when their semantics are small and exact.
  For broad application-specific Python evaluators, keep the upstream Python
  process behind a narrow JSON request/response boundary and let Vieval assert
  its returned score.
- gRPC is useful only when a long-lived Python evaluator service needs typed,
  versioned remote calls. It is unnecessary for one-shot task JSON, setup HTTP,
  and scalar scores; a foreground process with JSON stdin/stdout is smaller and
  easier to reap.

Two integration shapes therefore remain available:

1. **Direct Vieval adapter:** scan task/case JSON, execute the supported setup
   and evaluator shapes in Node.js, and score directly. This workspace proves
   that path.
2. **Wrapped upstream runner:** spawn a pinned OSWorld Python entry as a
   foreground child, exchange one versioned JSON request/result, and feed its
   score and artifacts into the same Vieval case. Use this when porting the
   evaluator would risk semantic drift.

Do not turn setup `/execute` or Python GUI helpers into an alternate input
backend. They may prepare and evaluate the guest; task actions must continue to
flow through AUV so delivery results and run artifacts remain inspectable.
