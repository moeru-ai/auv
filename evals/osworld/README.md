# OSWorld on Kubernetes

The script defaults to the classic OSWorld Ubuntu VM: a pinned QEMU container
plus the official QCOW2 disk, downloaded and verified with SHA256. Use
`--profile selkies` for the previous HAMi GPU desktop.

## Create

Requires Bash 3.2+, jq, kubectl, an existing namespace and a Linux amd64 node.
OSWorld requests 1 CPU, 5 GiB RAM and 40 GiB ephemeral storage. The initial
12.3 GB download can take several minutes; the default timeout is one hour.

```sh
export KUBECONFIG="$HOME/.kube/config.d/ihome.conf"
./evals/osworld/scripts/create-on-k8s create \
  --node neko-gpu-1 --namespace auv-x11-hami-test > desktop.json
```

- `--profile osworld|selkies`: choose the environment (default: `osworld`).
- `--image IMAGE`: override the selected profile's container runtime.
- `--vm-image-url URL --vm-image-sha256 SHA256`: override the OSWorld disk;
  requires an HTTPS ZIP containing `Ubuntu.qcow2`.
- `--kvm-resource vendor.example/kvm`: use an existing device plugin supplying
  `/dev/kvm`. Otherwise QEMU uses slower software emulation.
- `--timeout SECONDS`: override the startup deadline.

Selkies requires HAMi DRA and supports `--gpu-cores`, `--gpu-memory` and
`--device-class`. Run `create --help` for defaults.

OSWorld requires scheduling gates (Kubernetes 1.30+) and a NetworkPolicy-enforcing
CNI. The script creates a deny-ingress policy before starting the Pod; use a
namespace without overlapping ingress grants. Access is through local tunnels.

## Connect and delete

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  port-forward --address 127.0.0.1 "pod/$(jq -r .pod desktop.json)" \
  8006:8006 5000:5000 9847:9847
```

Open [noVNC](http://127.0.0.1:8006), or fetch a screenshot:

```sh
curl --fail http://127.0.0.1:5000/screenshot -o screenshot.png
./evals/osworld/scripts/create-on-k8s delete desktop.json
```

Port 9847 is reserved for a later AUV installation inside the VM; installation
and pairing are separate steps. If the local port is occupied, use `19847:9847`.
Guest Chromium and VLC ports 9222 and 8080 can also be forwarded as needed.

For Selkies, forward port 8080 and open `https://127.0.0.1:8080` (self-signed
certificate). Sign in as `ubuntu`; retrieve the password with:

```sh
kubectl --context "$(jq -r .context desktop.json)" \
  -n "$(jq -r .namespace desktop.json)" \
  get secret "$(jq -r .password_secret desktop.json)" \
  -o jsonpath='{.data.password}' | base64 --decode
```

Deleting a Pod discards its guest state and owned resources; old Selkies receipts
still work. Stop tunnels separately. For startup failures, inspect
`kubectl logs POD -c prepare-image` or `-c desktop` in the chosen namespace.

## Maintenance

Edit bootstrap manifests in `scripts/assets/`; `create-on-k8s` fills in names,
images, credentials and resource options with jq. Image pins live in the script.
The download helper uses the tools shipped in the pinned runtime: wget, 7z and
qemu-img. Run ShellCheck after changes.

Evidence: the official disk has booted on Kubernetes under software emulation,
with a visible GNOME desktop and working screenshot/noVNC endpoints. KVM and AUV
pairing have not been live-tested. Screenshot readiness does not prove task
success; OSWorld task setup/reset and evaluation are outside this script.

Upstream: [classic Docker provider](https://github.com/xlang-ai/OSWorld/blob/b138d348256078fa634fc3b73567a7337c793e6b/desktop_env/providers/docker/provider.py),
[official guest image](https://huggingface.co/datasets/xlangai/ubuntu_osworld/tree/a5d9c3eaae98eebf6e3a0beb84e7e47cf72ae133).
