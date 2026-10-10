# Run a KVM/X11 desktop with AUV on Kubernetes

This runbook describes the reusable infrastructure needed to run a virtual
desktop in Kubernetes and control that desktop through AUV. It covers KVM,
QEMU, X11, noVNC, AUV installation, daemon readiness, and Device pairing.

It deliberately does not pin a cluster name, node name, PVC, VM image digest,
or AUV build. Those values belong to the deployment that uses this procedure.

## Resulting topology

```text
operator workstation
  ├─ browser ───────────────> noVNC relay ──> guest X11 desktop
  └─ auv CLI ───────────────> forwarded AUV TCP listener
                                      │
Kubernetes Pod                         │
  └─ QEMU/KVM ──> Linux guest ──> auv serve ──> X11
          └──────> OSWorld setup API
```

The OSWorld setup API and AUV have different responsibilities:

- The setup API boots and prepares the guest and starts applications.
- noVNC lets an operator observe and debug the guest.
- AUV captures the desktop and delivers GUI input through X11.

Do not use noVNC or the setup API as the normal GUI-input path for an AUV run.

## Prerequisites

The selected Kubernetes node must provide:

- hardware virtualization enabled on the host;
- `/dev/kvm` visible to the kubelet;
- enough CPU and memory for the guest;
- storage for the guest disk image;
- permission to run the QEMU Pod with the required device access.

The operator machine needs `kubectl`, `curl`, and an AUV CLI built from the
same revision as the guest binary.

Check the node before scheduling a guest:

```bash
kubectl debug "node/<node-name>" -it --image=ubuntu:24.04 -- \
  sh -lc 'test -c /dev/kvm && ls -l /dev/kvm'
```

Some clusters expose KVM through a device plugin. Others require a privileged
Pod with a `/dev/kvm` hostPath. Use the narrower device-plugin configuration
when the cluster provides one.

## 1. Create the QEMU/KVM runtime

Prepare a PVC containing the OSWorld qcow2 image. Its access mode and node
affinity must match the selected node.

The runtime manifest must provide the following pieces. Adapt image-specific
environment variables and paths to the selected OSWorld release:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: osworld-runtime
  namespace: <namespace>
  labels:
    app.kubernetes.io/name: osworld-runtime
spec:
  nodeSelector:
    kubernetes.io/hostname: <kvm-node>
  terminationGracePeriodSeconds: 30
  containers:
    - name: qemu
      image: <pinned-osworld-runtime-image>
      securityContext:
        privileged: true
      resources:
        requests:
          cpu: "4"
          memory: 8Gi
        limits:
          cpu: "8"
          memory: 12Gi
      ports:
        - { name: setup, containerPort: 5000 }
        - { name: novnc, containerPort: 8006 }
        - { name: auv, containerPort: 8080 }
      startupProbe:
        httpGet: { path: /screenshot, port: setup }
        periodSeconds: 5
        timeoutSeconds: 15
        failureThreshold: 120
      readinessProbe:
        httpGet: { path: /screenshot, port: setup }
        periodSeconds: 5
        timeoutSeconds: 15
        failureThreshold: 3
      volumeMounts:
        - name: guest-image
          mountPath: /System.qcow2
          subPath: System.qcow2
          readOnly: true
        - name: kvm
          mountPath: /dev/kvm
  volumes:
    - name: guest-image
      persistentVolumeClaim:
        claimName: <guest-image-pvc>
    - name: kvm
      hostPath:
        path: /dev/kvm
        type: CharDevice
```

The privileged setting above is a compatibility fallback, not a general
recommendation. Remove it when the cluster can grant only the required KVM
device and capabilities.

Wait for the guest API, not only for the container process:

```bash
kubectl -n <namespace> wait \
  --for=condition=Ready pod/osworld-runtime \
  --timeout=15m
kubectl -n <namespace> logs pod/osworld-runtime -c qemu --tail=100
```

Confirm from the QEMU log that hardware acceleration is active. If the runtime
falls back to software emulation, stop and fix device access before continuing.

## 2. Reach the setup API and noVNC

The common OSWorld QEMU image forwards guest ports for traffic addressed to
the Pod IP, but it may not bind the same ports on container loopback. In that
case, `kubectl port-forward pod/osworld-runtime ...` does not work.

Create a short-lived relay Pod that connects to a Service selecting the QEMU
Pod. The relay image should already contain its TCP proxy; do not install
packages each time the Pod starts. Forward these relay ports locally:

| Port | Purpose |
|---:|---|
| 5000 | OSWorld guest setup API |
| 8006 | noVNC |
| 8080 | AUV TCP listener in the guest |

```bash
kubectl -n <namespace> port-forward pod/<relay-pod> \
  5000:5000 8006:8006 8080:8080
```

In another terminal:

```bash
curl --fail --output /tmp/osworld-screen.png \
  http://127.0.0.1:5000/screenshot
```

Open `http://127.0.0.1:8006/` for noVNC. Wait until the X11 session and desktop
have finished starting; one successful screenshot during boot is not enough.

## 3. Install AUV in the guest

Build or obtain an AUV binary compatible with the guest distribution and CPU
architecture. Check its dynamic-library and glibc requirements before upload.
Do not reuse an unidentified binary from an earlier experiment.

Upload the binary through the setup API because `kubectl cp` copies into the
QEMU container, not into the guest:

```bash
curl --fail-with-body \
  -F 'file_path=<guest-home>/auv' \
  -F 'file_data=@/absolute/path/to/auv' \
  http://127.0.0.1:5000/setup/upload
```

Use the setup API to run these non-GUI installation checks in the guest:

```text
chmod 0700 <guest-home>/auv
<guest-home>/auv --version
ldd <guest-home>/auv
```

Install any missing runtime libraries through the guest image's normal package
mechanism, then repeat `auv --version`. Keep the host and guest AUV revisions
aligned; a version mismatch can allow discovery while later Runner operations
still fail.

## 4. Start `auv serve`

Start the daemon as the logged-in desktop user with the guest X11 environment:

```text
env DISPLAY=:0 XDG_SESSION_TYPE=x11 \
  <guest-home>/auv serve \
  --listen unix://<guest-home>/auv.sock \
  --listen http://0.0.0.0:8080 \
  --store-root <guest-home>/.local/share/auv-osworld \
  --pairing-store <guest-home>/.local/share/auv-osworld/pairings.json \
  --no-register
```

Run it through the setup API's background-launch operation or a guest process
supervisor. `auv serve` is a foreground process and must remain alive for the
whole desktop session.

The Unix listener is the owner channel. The TCP listener is only for paired
clients. Do not expose the TCP listener through a public Ingress or
LoadBalancer.

## 5. Wait for readiness before pairing

Pairing must not race daemon startup. Complete both checks below before
creating a token or running `auv devices pair ... connect`.

First, execute a CLI probe inside the guest through the owner channel:

```text
env AUV_ENDPOINT=unix://<guest-home>/auv.sock \
  <guest-home>/auv devices list
```

Retry with a bounded timeout until the command exits successfully. A running
process or an open socket file alone is not sufficient: this command verifies
that the AUV CLI can complete a request against `auv serve`.

Second, verify from the operator machine that the forwarded TCP port is open:

```bash
nc -vz 127.0.0.1 8080
```

If either check fails, inspect the daemon output and confirm `DISPLAY=:0`,
`XDG_SESSION_TYPE=x11`, the Unix-socket path, and the port-forward. Do not work
around a readiness failure by repeatedly creating pairing tokens.

## 6. Pair the operator CLI

Only after the readiness gates pass, create a short-lived token through the
owner channel inside the guest:

```text
env AUV_ENDPOINT=unix://<guest-home>/auv.sock \
  <guest-home>/auv devices pair create-token
```

Consume that token on the operator machine:

```bash
auv devices pair \
  --endpoint http://127.0.0.1:8080 \
  connect \
  --token '<token>' \
  --label 'OSWorld guest' \
  --profile osworld
```

Then verify the selected Device and its local Driver Runner:

```bash
auv devices list
auv --device 'OSWorld guest' invoke display.list --json
auv --device 'OSWorld guest' invoke display.capture --json
```

Do not start an evaluation episode until these commands succeed. Pairing only
establishes trust; the display calls verify that the selected Runner can reach
the guest X11 session.

## 7. Teardown

Stop the port-forward, delete task-owned relay and runtime resources, and
remove pairing credentials that should not survive the run. Keep a shared
guest-image PVC only when it is an intentional cache. Evaluation output and AUV
Run artifacts should live outside the disposable runtime Pod.

## Failure guide

| Symptom | Check |
|---|---|
| QEMU reports no KVM acceleration | node selection, `/dev/kvm`, device-plugin or security context |
| setup API responds but desktop is incomplete | wait for guest reboot and X11 session startup |
| noVNC works but AUV capture fails | daemon user's X11 access, `DISPLAY`, and `XDG_SESSION_TYPE` |
| owner-channel CLI probe fails | `auv serve` output and Unix-socket ownership/path |
| TCP probe fails | guest listener, QEMU port forwarding, relay Pod, and local port-forward |
| pairing works but invoke fails | host/guest AUV revision and selected Device/Runner health |
