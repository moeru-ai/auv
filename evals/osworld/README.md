# OSWorld desktop on Kubernetes

`scripts/create-on-k8s` defaults to the **classic OSWorld Ubuntu VM**: a pinned
QEMU container plus the official Ubuntu QCOW2 disk. It needs no Docker daemon
inside Kubernetes. The previous GPU desktop is available with `--profile selkies`.

**NOTICE:** This provisions a guest and verifies its screenshot API. Task setup,
reset, agent execution and evaluation need a separately approved runner/provider
integration. This is not an OSWorld V2 image or a benchmark support claim.

## Requirements

- Client: Bash 3.2+, `kubectl`, `jq`.
- Cluster: an existing namespace and Linux amd64 node, scheduling gates
  (Kubernetes 1.30+), and a NetworkPolicy-enforcing CNI.
- API access: create/read/delete Pods and NetworkPolicies, patch Pods,
  exec/logs/port-forward. Selkies additionally needs Secrets and ResourceClaims.
- OSWorld scheduling requests: 1 CPU, 5 GiB RAM, 40 GiB ephemeral storage;
  limits: 4 CPUs, 8 GiB RAM, 100 GiB ephemeral storage. Leave space above the
  node's disk-pressure threshold. Registry and Hugging Face/CDN access are needed.

The official guest API and noVNC have no transport authentication. The script
creates a Pod-owned deny-ingress NetworkPolicy **before releasing the scheduling
gate**. Use a dedicated namespace without overlapping policies that allow ingress
to these Pods: policies are additive. API acceptance does not prove CNI enforcement.
Access through localhost port forwarding; no Service or Ingress is created.

## Create

Run from the repository root, with your kubeconfig, namespace and node:

```sh
export KUBECONFIG="$HOME/.kube/config.d/ihome.conf"
./evals/osworld/scripts/create-on-k8s create \
  --node neko-gpu-1 --namespace auv-x11-hami-test > desktop.json
```

The init container downloads the 12,273,896,463-byte ZIP, verifies SHA256,
extracts only `Ubuntu.qcow2`, and checks the disk format. The guest mounts it
read-only at `/System.qcow2`; upstream creates a disposable `/boot.qcow2` write
layer. Wait for **`OSWorld ready`**: `/screenshot` must return a PNG. A successful
create writes the receipt to stdout; progress goes to stderr.

The default deadline is 3600 seconds (`--timeout` overrides it). Cold startup
includes downloading and unpacking the system disk. Each Pod has an ephemeral
base and writable guest state; shared persistent caching is intentionally deferred.

CPU software emulation (`KVM=N`) is the default and is slow. A corrected AMD64
CPU identity avoids the pinned runtime's LLVM/GNOME crash (see the evidence note).
If an existing device
plugin advertises a resource granting `/dev/kvm`, use its real resource name:

```sh
./evals/osworld/scripts/create-on-k8s create \
  --node YOUR_NODE --namespace YOUR_NAMESPACE \
  --kvm-resource YOUR_VENDOR_DOMAIN/kvm > desktop.json
```

The script does not install a device plugin, mount host devices, or grant
`privileged` / `NET_ADMIN`. QEMU uses user-mode networking. KVM accelerates the
virtual CPU; the VM profile does not request HAMi GPUs or claim GPU rendering.

## Connect

Use the same kubeconfig as creation and keep this process running:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  port-forward --address 127.0.0.1 "pod/$(jq -r .pod desktop.json)" \
  8006:8006 5000:5000 9222:9222 8080:8080
```

Open <http://127.0.0.1:8006> for noVNC. Check the guest API with:

```sh
curl --fail http://127.0.0.1:5000/screenshot -o screenshot.png
```

These endpoints prepare a future runner connection. Upstream
`DesktopEnv(provider_name="docker")` creates a separate Docker VM; it does not
attach to this Pod.

### Reserved AUV connection

The OSWorld profile also forwards TCP 9847 from the Pod into the guest and reports
`auv_port: 9847` in the receipt. This is a network configuration value, not a
daemon readiness result. AUV installation and pairing remain a separate step.
After installing AUV inside the VM and starting it in the desktop user's session
with `auv serve --listen http://0.0.0.0:9847`, open a separate local tunnel:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  port-forward --address 127.0.0.1 "pod/$(jq -r .pod desktop.json)" 9847:9847
```

Use `http://127.0.0.1:9847` as the local pairing endpoint. Pairing credentials
are still required by AUV. If local port 9847 is occupied, use `19847:9847` and
pair through local port 19847 instead. Previously created Pods retain their
original QEMU port configuration; this change applies to newly created Pods.

## Image selection

`--image` overrides the **container runtime** within the selected profile, not
the VM system disk. Custom runtimes must preserve the profile's tool and startup
contract. OSWorld defaults are:

| Item | Pin |
| --- | --- |
| Runtime | `happysixd/osworld-docker@sha256:0e6497a9295647cf05bf2b2af522fdd79bdeba2737595259cab310a3bcf6baa9` |
| VM ZIP | `https://huggingface.co/datasets/xlangai/ubuntu_osworld/resolve/a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133/Ubuntu.qcow2.zip` |
| ZIP SHA256 | `b795b6cd4c69b252c1b4f10150a347795555032501b60fd031751ed09b896712` |

Custom disk downloads require **both** `--vm-image-url` (HTTPS) and
`--vm-image-sha256`. The ZIP must contain a root member named `Ubuntu.qcow2`.
The checksum covers the archive, not the extracted disk. See the
[upstream provider](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/provider.py),
[VM manager](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/manager.py),
and [HF metadata](https://huggingface.co/api/datasets/xlangai/ubuntu_osworld/tree/a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133).

## Optional Selkies GPU desktop

This is a general Ubuntu/KDE container, not the OSWorld guest image. It requires
HAMi DRA consumable capacity and `resource.k8s.io/v1` on the chosen GPU node:

```sh
./evals/osworld/scripts/create-on-k8s create --profile selkies \
  --node YOUR_GPU_NODE --namespace YOUR_NAMESPACE > selkies.json
```

Defaults remain 25 HAMi cores / 4 GiB GPU capacity, DeviceClass
`hami-core-gpu.project-hami.io`, a pinned Selkies image, and a 900-second deadline.
Use `--gpu-cores`, `--gpu-memory`, `--device-class`, `--image` and `--timeout` to
override them. CPU/RAM request: 1 CPU / 2 GiB; limit: 8 CPUs / 16 GiB. Readiness
checks Plasma, X11 and NVIDIA OpenGL. DRA requests do not prove graphics memory
enforcement.

Forward the receipt's Pod port 8080 to localhost and open
<https://127.0.0.1:8080>. Accept the local self-signed certificate. Sign in as
`ubuntu`; retrieve the generated password with:

```sh
kubectl --context "$(jq -r .context selkies.json)" \
  -n "$(jq -r .namespace selkies.json)" \
  get secret "$(jq -r .password_secret selkies.json)" \
  -o jsonpath='{.data.password}' | base64 --decode
```

Graphical commands in Selkies need `/tmp/runtime-ubuntu/container-env` and
`DISPLAY=:20`; the OSWorld VM uses a separate guest session.

## Delete and diagnose

```sh
./evals/osworld/scripts/create-on-k8s delete desktop.json
```

Deletion checks the Pod UID. Kubernetes garbage-collects owned NetworkPolicies,
Secrets and ResourceClaims. Old Selkies receipts still work: deletion needs only
context, namespace, name and UID. The namespace remains. Copy files before
deletion; guest changes are temporary. Stop port forwarding separately.

Startup failures trigger cleanup. If API access fails, inspect the printed
identity for incomplete cleanup. For a live Pod:

```sh
kubectl -n YOUR_NAMESPACE describe pod YOUR_POD
kubectl -n YOUR_NAMESPACE logs YOUR_POD -c prepare-image --tail=20
kubectl -n YOUR_NAMESPACE logs YOUR_POD -c desktop --tail=60
```

`Pending` can mean resource shortage, missing device capacity, image pull or disk
preparation. Init failures report download/checksum errors. `Running` alone does
not prove readiness. `OSWorld ready` checks the screenshot API, not compositor
stability or task setup; inspect the actual desktop before a task run. The script
does not change node DNS or disk eviction policy.

## Tests and evidence

```sh
./evals/osworld/scripts/tests/test-create-on-k8s
# Optional Docker check: uses the real pinned runtime with tiny disk fixtures.
./evals/osworld/scripts/tests/test-prepare-osworld-image
bash -n evals/osworld/scripts/create-on-k8s evals/osworld/scripts/prepare-osworld-image
shellcheck evals/osworld/scripts/create-on-k8s evals/osworld/scripts/prepare-osworld-image \
  evals/osworld/scripts/tests/test-create-on-k8s evals/osworld/scripts/tests/test-prepare-osworld-image
git diff --check
```

API-fixture tests cover profiles, resource isolation, metadata, option validation,
KVM requests and UID-safe cleanup; they do not prove guest boot. See the
[implementation/evidence note](../../docs/ai/references/ops/2026-10-09-osworld-kubernetes-provisioning-reference.md)
for live results and limitations.
