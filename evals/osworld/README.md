# OSWorld desktop on Kubernetes

Run these commands from the repository root to create a GPU X11 desktop.

**NOTICE:** This script creates a Plasma desktop in a Selkies container through HAMi DRA.
OSWorld task setup, reset, and evaluation remain outside this script until a separate provider integration is approved.

## Before you start

- **Client:** Bash 3.2+, `kubectl`, and `jq`. Python is not required.
- **Cluster:** An existing namespace, a GPU node, and HAMi DRA with consumable capacity and `resource.k8s.io/v1`.
- **Access:** Permission to create/read/delete Pods, Secrets and ResourceClaims, and run commands in Pods.
  The node needs registry access or the pinned image in its cache.

## 1. Create

Replace the kubeconfig path, node, and namespace for your cluster:

```sh
export KUBECONFIG=/path/to/kubeconfig
./evals/osworld/scripts/create-on-k8s create \
  --node liet-gpu-1 \
  --namespace auv-x11-hami-test > desktop.json
```

**Wait for `Desktop ready: ...`.** The script checks X11, Plasma, and the NVIDIA OpenGL renderer before it reports success.
Progress goes to the terminal. The result goes to `desktop.json`.

Keep `desktop.json` for connection and deletion. Use a different filename for each desktop.
The file contains the Pod name, UID, context, namespace, and GPU result.

**Time:** A first run downloads about 3.7 GB and unpacks the image. A cached image skips this download.
The default deadline is 15 minutes.

## 2. Connect

```sh
CONTEXT=$(jq -r .context desktop.json)
NS=$(jq -r .namespace desktop.json)
POD=$(jq -r .pod desktop.json)
kubectl --context "$CONTEXT" -n "$NS" port-forward "pod/$POD" 8080:8080
```

Keep this terminal open. The next step uses a second terminal.

## 3. Sign in

In a second terminal, use the same kubeconfig and repository directory.
Get the password:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  get secret "$(jq -r .password_secret desktop.json)" \
  -o jsonpath='{.data.password}' | base64 --decode
```

Open <https://localhost:8080>. Accept the self-signed certificate for this local connection.
Sign in as **`ubuntu`** with the password.

The endpoint requires TLS and a password. The script creates no Service or Ingress.
The password stays in a Secret, not in `desktop.json`.

## 4. Delete when finished

Copy needed files from the desktop before deletion. Desktop files are temporary.
Use the same kubeconfig and context as creation:

```sh
./evals/osworld/scripts/create-on-k8s delete desktop.json
```

Deletion checks the Pod UID to protect a replacement Pod with the same name.
Kubernetes also deletes the owned Secret and ResourceClaim. The namespace stays.
Stop port forwarding with Ctrl-C.

## Optional commands and defaults

<details>
<summary>Cluster selection, GPU resources, timeout, and graphical commands</summary>

For ihome, select the kubeconfig before step 1:

```sh
export KUBECONFIG="$HOME/.kube/config.d/ihome.conf"
```

Put `--kubeconfig` and `--context` before the subcommand:

```sh
./evals/osworld/scripts/create-on-k8s --context YOUR_CONTEXT create \
  --node YOUR_GPU_NODE --namespace YOUR_NAMESPACE > desktop.json
```

| Resource | Default | Control |
| --- | --- | --- |
| GPU capacity | 25 HAMi cores, 4 GiB | `--gpu-cores`, `--gpu-memory` |
| DeviceClass | `hami-core-gpu.project-hami.io` | `--device-class` |
| CPU / RAM | Request: 1 CPU / 2 GiB. Limit: 8 CPUs / 16 GiB. | Fixed in the script |
| Startup deadline | 900 seconds | `--timeout 1800` for a slower first run |
| Image | Selkies, pinned by digest | `--image` requires the same session contract |

DRA capacity requests do not prove Vulkan/OpenGL memory enforcement.
The measured graphics path uses Zink/Vulkan on NVIDIA.
`nvidia-smi` alone does not prove GPU graphics use.

To run a graphical command, load the session environment:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  exec "$(jq -r .pod desktop.json)" -- bash -c \
  '. /tmp/runtime-ubuntu/container-env; DISPLAY=:20 glxinfo -B'
```

</details>

## If startup stops

| Message | Meaning and next action |
| --- | --- |
| `Waiting for desktop session environment`, `Waiting for Plasma session`, `Waiting for X11 display` | Normal startup. Wait for `Desktop ready`. Pod status `Running` is not enough. |
| `ImagePullBackOff` / `ErrImagePull` | Inspect Pod events for registry or node DNS errors. Working Pod DNS does not prove working node DNS. |
| `Pending` | Inspect events for CPU/GPU capacity, node selectors, taints, or a missing DeviceClass. |
| Software renderer | The GPU check failed. Inspect the DRA allocation and graphics libraries. |

Use the Pod name from the terminal output:

```sh
kubectl -n YOUR_NAMESPACE describe pod YOUR_POD
kubectl -n YOUR_NAMESPACE logs YOUR_POD --tail=60
```

Startup failures trigger resource cleanup. If API access fails, inspect the printed resource identity for incomplete cleanup.
The script does not change node DNS. Registry failures need a node fix or an operator to preload the pinned image.

## Tests

```sh
./evals/osworld/scripts/tests/test-create-on-k8s
bash -n evals/osworld/scripts/create-on-k8s
shellcheck evals/osworld/scripts/create-on-k8s evals/osworld/scripts/tests/test-create-on-k8s
git diff --check
```

The Bash tests cover startup messages, GPU checks, receipt output, UID-safe deletion, and failure cleanup.
ShellCheck is optional for script development. The desktop script does not require it.
