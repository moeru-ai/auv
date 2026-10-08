# OSWorld Kubernetes provisioning

## Approved scope

The owner requested that `evals/osworld/scripts/create-on-k8s` create the actual
OSWorld environment, potentially through a default `--image`. Classified as an
approved feature. The existing Selkies image is a general GPU desktop; the
classic OSWorld Docker provider combines a QEMU container with a separate QCOW2
guest disk. Both are needed for the requested default.

The default is now `--profile osworld`; `--profile selkies` selects the existing
HAMi desktop. This slice provisions a guest and verifies its screenshot API.
Task reset/setup, agent execution, evaluation, V2, GPU passthrough and shared
persistent caching remain outside this slice and require named follow-up work.

The follow-up provisioning refinement reserves guest TCP 9847 in QEMU's
`USER_PORTS` and reports `auv_port` in the OSWorld receipt. This prepares the
network path for a later AUV installation without rebuilding the VM. It does not
install AUV, pair a client, or assert daemon readiness. Selkies receipts do not
include this port. Manifest/receipt tests cover the addition; the live guest
probe below predates this port addition and does not validate AUV connectivity.

## Source-backed contract

- Classic OSWorld revision: `b138d348256078fa634fc3b73567a7337c793e6b`.
  [Provider](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/provider.py)
  and [manager](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/manager.py).
- Runtime: `happysixd/osworld-docker@sha256:0e6497a9295647cf05bf2b2af522fdd79bdeba2737595259cab310a3bcf6baa9`.
  Its actual `/run/install.sh` creates `/boot.qcow2` backed by `/System.qcow2`;
  `/run/network.sh` implements user-mode forwarding without `NET_ADMIN`;
  `/run/proc.sh` accepts explicit software emulation via `KVM=N`.
- Official ZIP revision: `a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133` of
  `xlangai/ubuntu_osworld`; `Ubuntu.qcow2.zip`, 12,273,896,463 bytes.
  SHA256 `b795b6cd4c69b252c1b4f10150a347795555032501b60fd031751ed09b896712`
  comes from [HF LFS metadata](https://huggingface.co/api/datasets/xlangai/ubuntu_osworld/tree/a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133).

## Lifecycle and isolation

Init verifies the ZIP and extracts only its expected member into an ephemeral
volume. The base is read-only to QEMU; its overlay disappears with the Pod. A
scheduling gate prevents startup before the owned NetworkPolicy exists. The
guest API is unauthenticated, so a policy-enforcing CNI and absence of overlapping
ingress grants are required. Access uses localhost Kubernetes port forwarding.

Software CPU emulation is the portable default, not a GPU capability claim.
`--kvm-resource` requests an existing device-plugin resource supplying `/dev/kvm`;
the script does not install infrastructure or grant privileged host access.

## Validation status

Automated checks passed:

- `scripts/tests/test-create-on-k8s` under both Homebrew Bash and macOS Bash 3.2:
  default OSWorld and explicit Selkies manifests/receipts, image overrides, KVM
  device resource requests, invalid option rejection, malformed screenshot bodies,
  failed Pod/init/policy cleanup, and refusal to delete a different Pod UID.
- `scripts/tests/test-prepare-osworld-image` in the real pinned amd64 runtime:
  valid QCOW2 ZIP accepted, SHA256 mismatch rejected before extraction, and an
  invalid QCOW2 rejected even with a matching archive hash. Only HTTP download is
  replaced with a local fixture; 7z, sha256sum and qemu-img are real.
- Bash syntax, ShellCheck on all four scripts, and `git diff --check`.

The first live attempt cleaned up on missing curl; actual runtime tools were
verified with `docker run --rm --platform linux/amd64 --entrypoint bash <pin>`.
The scripts now use wget/7z. A later attempt encountered temporary HF CDN DNS
failure; bounded DNS retries were added. The official URL then downloaded, but
slowly. That Pod and its NetworkPolicy were cleaned up before switching the live
probe to an explicit `hf-mirror.com` URL with the **same pinned official SHA256**.
The default URL remains Hugging Face. No node DNS, other workloads, or eviction
policy were changed.

The mirror probe `auv-desktop-591ee67374` passed archive verification and booted
Ubuntu 22.04.3. The guest API and noVNC returned HTTP 200, but screenshots remained
black. GNOME Shell repeatedly aborted with:

```text
LLVM ERROR: 64-bit code requested on a subtarget that doesn't support it!
```

Guest `lscpu` reported AuthenticAMD family 6, model 6. This matches
[QEMU #191](https://gitlab.com/qemu-project/qemu/-/issues/191) and the
[upstream CPUID correction proposal](https://patchew.org/QEMU/20210507133650.645526-1-berrange%40redhat.com/).
The software-emulation profile now sets
`CPU_MODEL=max,family=15,model=6,stepping=1`; KVM retains the runtime's host CPU.
This changes the exposed CPU identity, not the official guest image or packages.
A manifest regression failed before the change and passed afterward. The
unfixed guest was deleted through the script's UID-safe deletion path.

This also demonstrates the readiness boundary: upstream-style screenshot API
readiness alone does not establish compositor stability or task readiness.
Artifacts from the failed GUI probe remain in ignored
`docs/notes/neko/osworld/native-vm/mirror/` (screenshot and GNOME error JSON).
The corrected full guest probe passed on `neko-gpu-1`, Pod
`auv-desktop-d0da0e545f` (UID `d5548cd3-5d38-4bc0-9d59-9fe3981b1bdd`),
in namespace `auv-x11-hami-test`:

- The mirror download passed the pinned official archive SHA256 check.
- Ubuntu 22.04.3 booted under software emulation with AMD CPU family 15.
- The screenshot API returned a visible 1920×1080 GNOME desktop, inspected
  directly. noVNC returned HTTP 200.
- GNOME remained `active/running`, with `NRestarts=0` in two separate probes;
  the later probe was approximately 149 seconds after service activation.
- Screenshot SHA256:
  `b5c02799326440a236cf909abac2be8461046921ed6e33b5821b2563e79099d5`.

Local evidence remains in ignored `docs/notes/neko/osworld/native-vm/cpu-fixed/`:
receipt, creation log, screenshot, noVNC HTML, CPU inspection and stability JSON.
This is live provisioning/desktop evidence, not a completed benchmark task or
evaluation result. KVM allocation has manifest coverage but was not live-tested.
