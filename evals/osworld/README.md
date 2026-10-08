# OSWorld desktop on Kubernetes

Run these commands from the repository root. The default profile creates the
classic OSWorld Ubuntu VM using a pinned QEMU runtime and the official QCOW2 disk.
Use `--profile selkies` for the original Plasma GPU X11 desktop through HAMi DRA.

**NOTICE:** This provisions the desktop environment. OSWorld task setup, reset,
evaluation, and AUV installation/pairing remain separate work.

## Before you start

- **Client:** Bash 3.2+, `kubectl`, and `jq`. Python is not required.
- **OSWorld:** An existing namespace and Linux amd64 node, scheduling gates
  (Kubernetes 1.30+), and a NetworkPolicy-enforcing CNI. The node needs registry
  access; the init container needs access to Hugging Face and its download CDN.
- **Selkies:** An existing namespace, GPU node, and HAMi DRA with consumable
  capacity and `resource.k8s.io/v1`. The node needs registry access or a cached image.
- **Access:** Create/read/delete Pods, exec/logs/port-forward. OSWorld additionally
  needs NetworkPolicies and Pod patching; Selkies needs Secrets and ResourceClaims.

The OSWorld API and noVNC have no transport authentication. The script creates a
Pod-owned deny-ingress policy before releasing its scheduling gate. Use a
namespace without overlapping ingress grants: NetworkPolicies are additive.
Neither profile creates a Service or Ingress; connect through local tunnels.

## 1. Create

Replace the kubeconfig path, node, and namespace for your cluster:

```sh
export KUBECONFIG=/path/to/kubeconfig
./evals/osworld/scripts/create-on-k8s create \
  --node YOUR_NODE --namespace YOUR_NAMESPACE > desktop.json
```

To create the original GPU desktop, add `--profile selkies`:

```sh
./evals/osworld/scripts/create-on-k8s create --profile selkies \
  --node YOUR_GPU_NODE --namespace YOUR_NAMESPACE > selkies.json
```

Wait for **`OSWorld ready`** or **`Desktop ready`**. OSWorld checks that the guest
screenshot API returns a PNG; Selkies checks X11, Plasma, and the NVIDIA OpenGL
renderer. A Pod being `Running` is not enough. Screenshot API readiness does not
prove compositor stability or task success; inspect the desktop before a task run.

Progress goes to the terminal; the JSON result goes to the receipt file. Keep
that file for connection and deletion, and use a different filename per desktop.
It contains the Pod name, UID, context, namespace and profile-specific results.

**Time:** Each OSWorld Pod downloads and verifies a 12.3 GB ZIP, then extracts the
system disk; the default deadline is one hour. Selkies downloads about 3.7 GB on a
first run, skips the download when cached, and defaults to 15 minutes.

## 2. Connect

Use the same kubeconfig as creation. For OSWorld:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  port-forward --address 127.0.0.1 "pod/$(jq -r .pod desktop.json)" \
  8006:8006 5000:5000
```

Keep this terminal open. Open [noVNC](http://127.0.0.1:8006), or fetch a screenshot
from a second terminal:

```sh
curl --fail http://127.0.0.1:5000/screenshot -o screenshot.png
```

Guest Chromium and VLC ports 9222 and 8080 can also be forwarded. Port 9847 is
reserved for a later AUV installation inside the VM; it does not mean AUV is
installed or ready. After starting the guest daemon, add `9847:9847` to the tunnel
(or `19847:9847` if local port 9847 is occupied), then pair through that local port.
Previously created Pods retain their original QEMU forwarding configuration.

### Selkies connection and sign-in

```sh
kubectl --context "$(jq -r .context selkies.json)" \
  -n "$(jq -r .namespace selkies.json)" \
  port-forward --address 127.0.0.1 "pod/$(jq -r .pod selkies.json)" 8080:8080
```

In a second terminal, using the same kubeconfig, retrieve the password:

```sh
kubectl --context "$(jq -r .context selkies.json)" \
  -n "$(jq -r .namespace selkies.json)" \
  get secret "$(jq -r .password_secret selkies.json)" \
  -o jsonpath='{.data.password}' | base64 --decode
```

Open <https://localhost:8080>, accept the self-signed certificate for this local
connection, and sign in as **`ubuntu`**. The endpoint requires TLS and a password.
The password stays in a Secret, not in the receipt.

## 3. Delete when finished

Copy needed files before deletion; desktop files and VM changes are temporary.
Use the same kubeconfig and context as creation, with the matching receipt:

```sh
./evals/osworld/scripts/create-on-k8s delete desktop.json
# Or: ./evals/osworld/scripts/create-on-k8s delete selkies.json
```

Deletion checks the Pod UID to protect a replacement Pod with the same name.
Kubernetes also deletes its owned NetworkPolicy, Secret or ResourceClaim. The
namespace stays, and old Selkies receipts still work. Stop tunnels with Ctrl-C.

## Optional commands and defaults

<details>
<summary>Cluster selection, images, resources, and graphical commands</summary>

For ihome, select the kubeconfig before creation:

```sh
export KUBECONFIG="$HOME/.kube/config.d/ihome.conf"
```

Put `--kubeconfig` and `--context` before the subcommand:

```sh
./evals/osworld/scripts/create-on-k8s --context YOUR_CONTEXT create \
  --node YOUR_NODE --namespace YOUR_NAMESPACE > desktop.json
```

| Setting | OSWorld default | Selkies default | Control |
| --- | --- | --- | --- |
| Container image | Pinned OSWorld QEMU runtime | Pinned Selkies | `--image`, compatible with the selected profile |
| Startup deadline | 3600 seconds | 900 seconds | `--timeout SECONDS` |
| CPU / RAM request | 1 CPU / 5 GiB | 1 CPU / 2 GiB | `scripts/assets/*-pod.json` |
| CPU / RAM limit | 4 CPUs / 8 GiB | 8 CPUs / 16 GiB | `scripts/assets/*-pod.json` |
| Ephemeral storage request / limit | 40 GiB / 100 GiB | Unspecified | `scripts/assets/*-pod.json` |
| Acceleration | Software CPU emulation | HAMi GPU | OSWorld: `--kvm-resource vendor.example/kvm` |
| GPU capacity | Not requested | 25 HAMi cores / 4 GiB | `--gpu-cores`, `--gpu-memory` |
| DeviceClass | Not used | `hami-core-gpu.project-hami.io` | `--device-class` |

`--image` changes the container runtime, not the VM system disk. To override the
OSWorld disk, supply both `--vm-image-url URL` and `--vm-image-sha256 SHA256`.
The URL must use HTTPS and the ZIP must contain `Ubuntu.qcow2`; the hash covers
the ZIP. The guest gets a read-only base with a disposable write layer. Shared
persistent caching is not implemented.

Software emulation is slower. KVM requires an existing device plugin supplying
`/dev/kvm`; the script does not install one or grant privileged host access.
KVM accelerates the CPU, not guest GPU rendering. Leave node disk headroom above
the disk-pressure threshold.

For Selkies, DRA capacity requests do not prove Vulkan/OpenGL memory enforcement.
The measured graphics path uses Zink/Vulkan on NVIDIA; `nvidia-smi` alone does not
prove GPU graphics use. Load the session environment for graphical commands:

```sh
kubectl --context "$(jq -r .context selkies.json)" \
  -n "$(jq -r .namespace selkies.json)" \
  exec "$(jq -r .pod selkies.json)" -- bash -c \
  '. /tmp/runtime-ubuntu/container-env; DISPLAY=:20 glxinfo -B'
```

</details>

## If startup stops

| Message | Meaning and next action |
| --- | --- |
| `Preparing OSWorld VM disk` | Download, checksum or extraction is in progress; inspect `prepare-image` logs. |
| `Waiting for desktop session environment`, `Waiting for Plasma session`, `Waiting for X11 display` | Normal Selkies startup. Wait for `Desktop ready`. |
| `ImagePullBackOff` / `ErrImagePull` | Inspect Pod events for registry or node DNS errors. Working Pod DNS does not prove working node DNS. |
| `Pending` | Inspect events for CPU/GPU/storage capacity, node selectors, taints, or a missing DeviceClass. |
| Software renderer | Selkies GPU validation failed; inspect DRA allocation and graphics libraries. |

Use the Pod name printed in the terminal:

```sh
kubectl -n YOUR_NAMESPACE describe pod YOUR_POD
kubectl -n YOUR_NAMESPACE logs YOUR_POD -c desktop --tail=60
# OSWorld image preparation:
kubectl -n YOUR_NAMESPACE logs YOUR_POD -c prepare-image --tail=20
```

Startup failures trigger cleanup. If API access fails, inspect the printed
resource identity for incomplete cleanup. The script does not change node DNS
or eviction policy. Registry failures need a node fix or an operator to preload
the pinned image.

## Maintenance

Edit bootstrap manifests in `scripts/assets/`; the script fills names, images,
credentials and resource options with jq. Image pins live in the script. The
download helper uses wget, 7z and qemu-img from the pinned runtime.

```sh
bash -n evals/osworld/scripts/create-on-k8s evals/osworld/scripts/prepare-osworld-image
shellcheck evals/osworld/scripts/create-on-k8s evals/osworld/scripts/prepare-osworld-image
git diff --check
```

ShellCheck is optional for development; running the script does not require it.

Evidence: [PR #294](https://github.com/moeru-ai/auv/pull/294) records a live
software-emulated Ubuntu guest with a visible GNOME desktop and working
screenshot/noVNC endpoints. KVM and AUV pairing have not been live-tested.
Upstream: [classic Docker provider](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/provider.py),
[official guest image](https://huggingface.co/datasets/xlangai/ubuntu_osworld/tree/a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133).
