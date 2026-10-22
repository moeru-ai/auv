# Run OSWorld through AUV on Kubernetes

This document describes the evaluation flow after a Kubernetes KVM/X11 guest
has been prepared and paired by the
[KVM/X11/AUV runbook](2026-10-07-kubernetes-kvm-x11-auv-runbook.md).

It is a workflow description, not a benchmark harness or a support claim. The
repository does not include task-specific Kubernetes controllers, scripted
policies, agent relays, or copied OSWorld evaluators. OSWorld remains the owner
of task definitions, guest preparation, reset behavior, and scoring.

## Responsibility split

```text
OSWorld task definition
  ├─ prepare/reset/evaluate ──> OSWorld setup and evaluator interfaces
  └─ instruction ─────────────> agent or scripted caller
                                      │
                                      ├─ observe ──> AUV capture
                                      └─ act ──────> AUV typed input
                                                          │
                                                          └─ guest X11 desktop
```

Keep the two control paths separate:

- OSWorld prepares task state and computes the benchmark result.
- AUV owns screenshots, GUI input delivery, Run recording, and artifacts.

Do not send GUI shell commands through OSWorld's generic execution endpoint.
Using the setup interface to launch an application is preparation; using it to
click, type, or mutate application state would bypass the system under test.

## Inputs recorded for a run

Before the first task, record:

- OSWorld repository commit or release;
- task manifest and task ID;
- guest image identity;
- AUV host and guest versions;
- Kubernetes namespace and task-owned resource names;
- selected AUV Device;
- output location for scores and AUV Run artifacts.

Environment identity helps reproduce a result. A file hash or successful input
delivery is not evidence that the task was semantically completed.

## One evaluation episode

### 1. Boot a clean guest

Create a task-owned runtime Pod from the selected guest image. Wait for the
setup API, guest reboot, X11 session, and noVNC view to become stable.

Use a fresh guest or the upstream OSWorld reset procedure for every task. A
successful prior task must not leave application or filesystem state behind.

### 2. Install and connect AUV

Follow the environment runbook in this order:

1. Upload and verify the AUV binary.
2. Start `auv serve` in the guest X11 session.
3. Wait until the guest AUV CLI can call the owner Unix listener.
4. Verify the forwarded TCP listener.
5. Create a pairing token.
6. Run `auv devices pair ... connect` on the operator machine.
7. Verify `display.list` and `display.capture` through the paired Device.

The readiness checks must happen before pairing. Otherwise a setup script can
consume a short-lived token while the daemon is still unavailable and leave an
ambiguous partial setup.

### 3. Prepare the task with OSWorld

Load one upstream task definition and call its normal setup path. This may copy
fixtures into the guest, open files, start applications, or configure mocked
websites. Keep the original task instruction unchanged.

Preparation finishes when the expected initial desktop state is visible. Use
noVNC for attended diagnosis, but use AUV capture for the observation supplied
to an agent or scripted policy.

### 4. Execute GUI actions through AUV

Create or retain one AUV Run for the episode. Select the paired Device and use
typed operations for observation and input, for example:

```bash
auv --device 'OSWorld guest' invoke display.capture --json
auv --device 'OSWorld guest' invoke input.pointerPosition --json
auv --device 'OSWorld guest' invoke input.clickPoint <x> <y> --json
auv --device 'OSWorld guest' invoke input.typeText '<text>' --json
auv --device 'OSWorld guest' invoke input.keys control q --json
auv --device 'OSWorld guest' invoke input.scrollPoint <x> <y> <dx> <dy> --json
```

Use the command catalog from the exact AUV revision being tested; available
operations and argument shapes can change. Preserve each operation result and
the AUV Run ID.

An `InputActionResult` proves only what the driver attempted and delivered. It
does not prove that the application reached the intended state. When an agent
needs semantic confirmation, capture a new observation and verify the visible
result separately.

Stop when the caller reports completion, reports failure, reaches its action or
time limit, or AUV returns an ambiguous delivery result. Do not silently retry
a GUI action whose effect might already have occurred.

### 5. Evaluate with OSWorld

After GUI execution stops, call the matching upstream evaluator without first
mutating the desktop through another control path. Record separately:

- the upstream score and evaluator output;
- evaluator errors or timeouts;
- the AUV Run ID and artifacts;
- the caller's completion reason;
- infrastructure failures.

Transport failure, evaluator failure, and a valid score of zero are different
outcomes. Do not turn all three into the same zero-valued record.

### 6. Reset and clean up

Run the upstream reset procedure or replace the guest before the next task.
Delete only task-owned Kubernetes resources. When a controller performs the
deletion, compare the current resource UID with the UID recorded at creation so
that it cannot delete a replacement Pod with the same name.

Persist the score, logs, and AUV artifacts before deleting the runtime Pod.

## Minimal orchestration sequence

An external orchestrator only needs to coordinate stable upstream and AUV
interfaces:

```text
boot guest
wait for X11/setup readiness
install AUV
start auv serve
wait for owner CLI request to succeed
pair and verify paired capture
OSWorld prepare(task)
repeat:
  AUV capture
  caller chooses one typed action
  AUV executes and records action
until caller finishes or limit/error occurs
OSWorld evaluate(task)
persist score + AUV Run reference
reset/delete task-owned resources
```

NOTICE: A maintained batch runner is intentionally outside this documentation
slice. Add one only when its ownership, supported OSWorld releases, task
coverage, and CI environment have an approved long-lived consumer.

## Interpretation boundary

These observations support different claims:

| Observation | Supported claim |
|---|---|
| `auv serve` accepts an owner-channel request | daemon readiness |
| paired `display.capture` succeeds | AUV can observe this guest X11 session |
| typed input returns delivery evidence | the driver attempted/delivered input |
| a new capture shows the intended state | visual state changed as expected |
| upstream evaluator returns a score | OSWorld judged the task outcome |

Do not promote one row into a stronger claim from a later row.
